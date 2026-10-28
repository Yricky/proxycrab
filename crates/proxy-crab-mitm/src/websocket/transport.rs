//! Message transport for active interception. Outbound messages are uncompressed, even when
//! permessage-deflate is negotiated, so dropping a message cannot corrupt the peer's dictionary.
use anyhow::{Result, bail};
use flate2::{Decompress, FlushDecompress, Status};
use std::collections::VecDeque;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::MAX_MESSAGE;
use crate::{storage::BodySide, workspace::now_millis};

#[derive(Debug)]
pub(crate) struct Message {
    pub opcode: u8,
    pub payload: Vec<u8>,
    pub started_at: u64,
}

struct PartialMessage {
    message: Message,
    compressed: bool,
}

struct Frame {
    opcode: u8,
    fin: bool,
    remaining: u64,
    offset: usize,
    mask: Option<[u8; 4]>,
}

/// All partial reads live in this object; cancelling `next()` does not discard frame bytes.
pub(crate) struct Reader<R> {
    reader: R,
    header: Vec<u8>,
    frame: Option<Frame>,
    message: Option<PartialMessage>,
    control: Vec<u8>,
    ready: VecDeque<Message>,
    inflater: Option<Decompress>,
    no_takeover: bool,
    expect_mask: bool,
    started_at: u64,
    closed: bool,
    error: Option<anyhow::Error>,
}

impl<R: AsyncRead + Unpin> Reader<R> {
    pub fn new(reader: R, extensions: &str, side: BodySide) -> Result<Self> {
        let prefix = if side == BodySide::Request {
            "client"
        } else {
            "server"
        };
        let mut compressed = false;
        let mut no_takeover = false;
        for extension in extensions
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let mut parts = extension.split(';').map(str::trim);
            if parts.next() != Some("permessage-deflate") || compressed {
                bail!("unsupported or duplicate WebSocket extension");
            }
            compressed = true;
            let mut seen = std::collections::HashSet::new();
            for part in parts {
                let key = part.split('=').next().unwrap().trim();
                if !seen.insert(key) {
                    bail!("duplicate deflate parameter");
                }
                if part == format!("{prefix}_no_context_takeover") {
                    no_takeover = true;
                } else if let Some((key, value)) = part.split_once('=') {
                    if !matches!(
                        key.trim(),
                        "client_max_window_bits" | "server_max_window_bits"
                    ) {
                        bail!("unsupported deflate parameter");
                    }
                    let bits: u8 = value.trim().trim_matches('"').parse()?;
                    if !(8..=15).contains(&bits) {
                        bail!("invalid deflate window");
                    }
                } else if !matches!(
                    part,
                    "client_no_context_takeover" | "server_no_context_takeover"
                ) {
                    bail!("unsupported deflate parameter");
                }
            }
        }
        Ok(Self {
            reader,
            header: Vec::new(),
            frame: None,
            message: None,
            control: Vec::new(),
            ready: VecDeque::new(),
            inflater: compressed.then(|| Decompress::new(false)),
            no_takeover,
            expect_mask: side == BodySide::Request,
            started_at: 0,
            closed: false,
            error: None,
        })
    }

    /// Complete messages already read from the socket, used when the connection terminates.
    pub(crate) fn take_buffered(&mut self) -> Vec<Message> {
        self.ready.drain(..).collect()
    }

    pub async fn next(&mut self) -> Result<Option<Message>> {
        loop {
            if let Some(message) = self.ready.pop_front() {
                return Ok(Some(message));
            }
            if let Some(error) = self.error.take() {
                self.closed = true;
                return Err(error);
            }
            if self.closed {
                return Ok(None);
            }
            let mut bytes = [0; 8192];
            let n = self.reader.read(&mut bytes).await?;
            if n == 0 {
                if !self.header.is_empty() || self.frame.is_some() || self.message.is_some() {
                    bail!("WebSocket stream ended during a message");
                }
                return Ok(None);
            }
            if let Err(error) = self.feed(&bytes[..n]) {
                // Deliver earlier complete messages before reporting a later malformed frame.
                self.error = Some(error);
            }
        }
    }

    fn data(&mut self, mut bytes: &[u8]) -> Result<()> {
        let message = self
            .message
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("missing message"))?;
        if !message.compressed {
            if message.message.payload.len().saturating_add(bytes.len()) > MAX_MESSAGE {
                bail!("WebSocket message exceeds size limit");
            }
            message.message.payload.extend_from_slice(bytes);
            return Ok(());
        }
        let inflater = self.inflater.as_mut().unwrap();
        loop {
            let mut output = [0; 8192];
            let before_in = inflater.total_in();
            let before_out = inflater.total_out();
            let status = inflater.decompress(bytes, &mut output, FlushDecompress::Sync)?;
            if status == Status::StreamEnd {
                bail!("unexpected final deflate block");
            }
            let used = (inflater.total_in() - before_in) as usize;
            let produced = (inflater.total_out() - before_out) as usize;
            if message.message.payload.len().saturating_add(produced) > MAX_MESSAGE {
                bail!("WebSocket message exceeds size limit");
            }
            message
                .message
                .payload
                .extend_from_slice(&output[..produced]);
            bytes = &bytes[used..];
            if bytes.is_empty() && produced < output.len() {
                break;
            }
            if used == 0 && produced == 0 {
                bail!("deflate decoder made no progress");
            }
        }
        Ok(())
    }

    fn feed(&mut self, mut bytes: &[u8]) -> Result<()> {
        while !self.closed
            && (!bytes.is_empty() || self.frame.as_ref().is_some_and(|f| f.remaining == 0))
        {
            if self.frame.is_none() {
                if self.header.is_empty() {
                    self.started_at = now_millis();
                }
                self.header.push(bytes[0]);
                bytes = &bytes[1..];
                if self.header.len() < 2 {
                    continue;
                }
                let short = self.header[1] & 127;
                let extended = match short {
                    126 => 2,
                    127 => 8,
                    _ => 0,
                };
                let masked = self.header[1] & 128 != 0;
                let need = 2 + extended + if masked { 4 } else { 0 };
                if self.header.len() < need {
                    continue;
                }
                let first = self.header[0];
                let opcode = first & 15;
                let fin = first & 128 != 0;
                let compressed = first & 64 != 0;
                let length = match short {
                    126 => u16::from_be_bytes(self.header[2..4].try_into()?) as u64,
                    127 => u64::from_be_bytes(self.header[2..10].try_into()?),
                    _ => short as u64,
                };
                if masked != self.expect_mask
                    || first & 48 != 0
                    || length >> 63 != 0
                    || (short == 126 && length < 126)
                    || (short == 127 && length < 65536)
                {
                    bail!("invalid WebSocket frame");
                }
                if !matches!(opcode, 0 | 1 | 2 | 8 | 9 | 10)
                    || (opcode >= 8 && (!fin || length > 125 || compressed))
                {
                    bail!("invalid WebSocket opcode/control frame");
                }
                if opcode == 0 {
                    if compressed || self.message.is_none() {
                        bail!("invalid continuation");
                    }
                } else if opcode < 8 {
                    if self.message.is_some() || (compressed && self.inflater.is_none()) {
                        bail!("invalid message start");
                    }
                    self.message = Some(PartialMessage {
                        message: Message {
                            opcode,
                            payload: Vec::new(),
                            started_at: self.started_at,
                        },
                        compressed,
                    });
                }
                // Stream compressed input in bounded chunks; only decoded bytes count toward the cap.
                if opcode < 8
                    && !self.message.as_ref().unwrap().compressed
                    && length
                        .saturating_add(self.message.as_ref().unwrap().message.payload.len() as u64)
                        > MAX_MESSAGE as u64
                {
                    bail!("WebSocket message exceeds size limit");
                }
                let mask = masked.then(|| self.header[need - 4..need].try_into().unwrap());
                self.frame = Some(Frame {
                    opcode,
                    fin,
                    remaining: length,
                    offset: 0,
                    mask,
                });
                self.header.clear();
                self.control.clear();
            }
            let frame = self.frame.as_mut().unwrap();
            let n = bytes.len().min(frame.remaining.min(8192) as usize);
            let mut chunk = bytes[..n].to_vec();
            bytes = &bytes[n..];
            if let Some(mask) = frame.mask {
                for (i, byte) in chunk.iter_mut().enumerate() {
                    *byte ^= mask[(frame.offset + i) % 4];
                }
            }
            frame.offset = (frame.offset + n) % 4;
            frame.remaining -= n as u64;
            let (opcode, fin, done) = (frame.opcode, frame.fin, frame.remaining == 0);
            if opcode < 8 {
                self.data(&chunk)?;
            } else {
                self.control.extend_from_slice(&chunk);
            }
            if done {
                self.frame = None;
                if opcode < 8 && fin {
                    if self.message.as_ref().unwrap().compressed {
                        self.data(&[0, 0, 255, 255])?;
                        if self.no_takeover {
                            self.inflater.as_mut().unwrap().reset(false);
                        }
                    }
                    let message = self.message.take().unwrap().message;
                    validate_payload(message.opcode, &message.payload)?;
                    self.ready.push_back(message);
                } else if opcode >= 8 {
                    validate_payload(opcode, &self.control)?;
                    self.ready.push_back(Message {
                        opcode,
                        payload: std::mem::take(&mut self.control),
                        started_at: self.started_at,
                    });
                    if opcode == 8 {
                        self.closed = true;
                    }
                }
            }
        }
        Ok(())
    }
}

fn validate_payload(opcode: u8, payload: &[u8]) -> Result<()> {
    if opcode == 1 {
        std::str::from_utf8(payload)?;
    }
    if opcode == 8 {
        if payload.len() == 1 {
            bail!("invalid close payload");
        }
        if payload.len() >= 2 {
            let code = u16::from_be_bytes([payload[0], payload[1]]);
            if !matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999) {
                bail!("invalid close code");
            }
            std::str::from_utf8(&payload[2..])?;
        }
    }
    Ok(())
}

/// Writes one complete uncompressed frame. `masked` is true when writing to the server.
pub(crate) async fn write<W: AsyncWrite + Unpin>(
    writer: &mut W,
    opcode: u8,
    payload: &[u8],
    masked: bool,
) -> Result<()> {
    if !matches!(opcode, 1 | 2 | 8 | 9 | 10) || (opcode >= 8 && payload.len() > 125) {
        bail!("invalid outgoing WebSocket message");
    }
    if payload.len() > MAX_MESSAGE {
        bail!("WebSocket message exceeds size limit");
    }
    validate_payload(opcode, payload)?;
    let mut header = Vec::with_capacity(14);
    header.push(128 | opcode);
    let flag = if masked { 128 } else { 0 };
    match payload.len() {
        n @ 0..=125 => header.push(flag | n as u8),
        n @ 126..=65535 => {
            header.push(flag | 126);
            header.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            header.push(flag | 127);
            header.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    let mut mask = [0; 4];
    if masked {
        getrandom::fill(&mut mask)
            .map_err(|e| anyhow::anyhow!("WebSocket masking randomness: {e}"))?;
        header.extend_from_slice(&mask);
    }
    writer.write_all(&header).await?;
    if masked {
        let mut buffer = [0; 8192];
        for (index, chunk) in payload.chunks(buffer.len()).enumerate() {
            for (i, byte) in chunk.iter().enumerate() {
                buffer[i] = byte ^ mask[(index * 8192 + i) % 4];
            }
            writer.write_all(&buffer[..chunk.len()]).await?;
        }
    } else {
        writer.write_all(payload).await?;
    }
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(first: u8, bytes: &[u8]) -> Vec<u8> {
        let mut result = vec![first];
        if bytes.len() < 126 {
            result.push(bytes.len() as u8);
        } else {
            result.push(126);
            result.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
        }
        result.extend_from_slice(bytes);
        result
    }

    #[tokio::test]
    async fn fragments_controls_and_round_trip_masked_binary() {
        let input = [frame(1, b"hel"), frame(137, b"ping"), frame(128, b"lo")].concat();
        let mut reader = Reader::new(input.as_slice(), "", BodySide::Response).unwrap();
        let ping = reader.next().await.unwrap().unwrap();
        assert_eq!(
            (ping.opcode, ping.payload.as_slice()),
            (9, b"ping".as_slice())
        );
        assert_eq!(reader.next().await.unwrap().unwrap().payload, b"hello");
        assert!(reader.next().await.unwrap().is_none());
        for n in [0, 125, 126, 65535, 65536] {
            let payload: Vec<_> = (0..n).map(|i| i as u8).collect();
            let mut encoded = Vec::new();
            write(&mut encoded, 2, &payload, true).await.unwrap();
            assert_eq!(encoded[0], 130);
            assert_ne!(encoded[1] & 128, 0);
            let mut reader = Reader::new(encoded.as_slice(), "", BodySide::Request).unwrap();
            assert_eq!(reader.next().await.unwrap().unwrap().payload, payload);
        }
    }

    #[tokio::test]
    async fn compressed_dictionary_survives_discard_and_interleaved_plain_message() {
        use flate2::{Compress, Compression, FlushCompress};
        for reset in [false, true] {
            let extension = if reset {
                "permessage-deflate; server_no_context_takeover"
            } else {
                "permessage-deflate"
            };
            let mut compressor = Compress::new(Compression::default(), false);
            let payload = b"dictionary repeated dictionary repeated dictionary repeated";
            let mut encoded = Vec::new();
            for _ in 0..3 {
                let mut output = [0; 1024];
                let before = compressor.total_out();
                compressor
                    .compress(payload, &mut output, FlushCompress::Sync)
                    .unwrap();
                let n = (compressor.total_out() - before) as usize;
                assert_eq!(&output[n - 4..n], &[0, 0, 255, 255]);
                let half = (n - 4) / 2;
                encoded.extend(frame(65, &output[..half]));
                encoded.extend(frame(137, b"ping"));
                encoded.extend(frame(128, &output[half..n - 4]));
                encoded.extend(frame(129, b"plain"));
                if reset {
                    compressor.reset();
                }
            }
            let mut reader =
                Reader::new(encoded.as_slice(), extension, BodySide::Response).unwrap();
            for _ in 0..3 {
                assert_eq!(reader.next().await.unwrap().unwrap().opcode, 9);
                // Discarding the returned message must not reset the input dictionary.
                assert_eq!(reader.next().await.unwrap().unwrap().payload, payload);
                assert_eq!(reader.next().await.unwrap().unwrap().payload, b"plain");
            }
        }
    }

    #[tokio::test]
    async fn malformed_and_incomplete_messages_fail_explicitly() {
        for bytes in [
            vec![129, 1],
            vec![128, 0],
            vec![193, 0],
            vec![129, 126, 0, 1, b'x'],
            vec![136, 1, 0],
            vec![129, 1, 255],
        ] {
            let mut reader = Reader::new(bytes.as_slice(), "", BodySide::Response).unwrap();
            assert!(reader.next().await.is_err(), "{bytes:?}");
        }
        let mut oversized = vec![130, 127];
        oversized.extend_from_slice(&((MAX_MESSAGE + 1) as u64).to_be_bytes());
        let mut reader = Reader::new(oversized.as_slice(), "", BodySide::Response).unwrap();
        assert!(
            reader
                .next()
                .await
                .unwrap_err()
                .to_string()
                .contains("size limit")
        );
    }

    #[tokio::test]
    async fn delivers_complete_message_before_later_protocol_error() {
        let bytes = [frame(129, b"complete"), vec![128, 0]].concat();
        let mut reader = Reader::new(bytes.as_slice(), "", BodySide::Response).unwrap();
        assert_eq!(reader.next().await.unwrap().unwrap().payload, b"complete");
        assert!(reader.next().await.is_err());
    }

    #[tokio::test]
    async fn cancelling_read_preserves_partial_frame() {
        let (mut writer, stream) = tokio::io::duplex(64);
        let mut reader = Reader::new(stream, "", BodySide::Response).unwrap();
        writer.write_all(&[129, 5, b'h']).await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), reader.next())
                .await
                .is_err()
        );
        writer.write_all(b"ello").await.unwrap();
        assert_eq!(reader.next().await.unwrap().unwrap().payload, b"hello");
    }
}
