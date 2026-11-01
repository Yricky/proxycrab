//! Passive RFC 6455 capture. Capture failures never interrupt the upgraded stream.
use crate::{
    model::{CaptureError, ErrorStage},
    storage::{BodySide, CaptureStore},
    workspace::now_millis,
};
use anyhow::{Result, bail};
use flate2::{Decompress, FlushDecompress};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) mod interception;
#[cfg(test)]
mod interception_tests;
pub(crate) mod transport;

const MAGIC: &[u8; 8] = b"PCWS\x01\0\0\0";
const MAX_MESSAGE: usize = 16 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketInfo {
    pub handshake_log_id: u64,
    pub websocket_log_id: u64,
    pub extensions: String,
    pub subprotocol: Option<String>,
    pub opened_at: u64,
    pub closed_at: Option<u64>,
    pub close_code: Option<u16>,
    pub close_reason: Option<String>,
    #[serde(default)]
    pub interceptors: Vec<WsInterceptor>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsInterceptor {
    pub execution_id: u64,
    pub name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageRef {
    pub side: BodySide,
    pub id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsInvocation {
    pub execution_id: u64,
    pub name: String,
    pub invocation_id: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsProducer {
    pub execution_id: u64,
    pub name: String,
    pub invocation_id: u64,
    pub send_index: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WsCallbackError {
    pub execution_id: u64,
    pub name: String,
    pub invocation_id: u64,
    pub error: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub id: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub started_at: u64,
    pub completed_at: u64,
    pub size: u64,
    pub content_available: bool,
    pub omitted_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<MessageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<MessageRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<WsProducer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped_by: Option<WsInvocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<WsCallbackError>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketPage {
    pub messages: Vec<WebSocketMessage>,
    pub has_more: bool,
    pub next_after_id: u32,
}

#[derive(Default)]
pub(crate) struct RecordIndex {
    files: HashMap<PathBuf, BTreeMap<u32, u64>>,
}
impl RecordIndex {
    pub(crate) fn read(
        &mut self,
        path: &Path,
        after: u32,
        limit: usize,
        body_id: Option<u32>,
    ) -> Result<(WebSocketPage, Option<Vec<u8>>)> {
        if self.files.len() >= 64 && !self.files.contains_key(path) {
            self.files.clear();
        }
        let offsets = self.files.entry(path.to_owned()).or_default();
        read_records_indexed(path, after, limit, body_id, offsets)
    }
}
#[cfg(test)]
fn read_records(
    path: &Path,
    after: u32,
    limit: usize,
    body_id: Option<u32>,
) -> Result<(WebSocketPage, Option<Vec<u8>>)> {
    read_records_indexed(path, after, limit, body_id, &mut BTreeMap::new())
}

// Readers skip payloads with seek; pagination never loads preceding message content.
fn read_records_indexed(
    path: &Path,
    after: u32,
    limit: usize,
    body_id: Option<u32>,
    offsets: &mut BTreeMap<u32, u64>,
) -> Result<(WebSocketPage, Option<Vec<u8>>)> {
    let mut page = WebSocketPage {
        messages: Vec::new(),
        has_more: false,
        next_after_id: after,
    };
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((page, None)),
        Err(e) => return Err(e.into()),
    };
    let length = file.metadata()?.len();
    if length < 8 {
        return Ok((page, None));
    }
    let mut magic = [0; 8];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("invalid WebSocket container version");
    }
    if offsets.values().any(|offset| *offset > length) {
        offsets.clear();
    }
    let target = body_id.unwrap_or(after.saturating_add(1));
    let mut position = offsets
        .range(..=target)
        .next_back()
        .map(|(_, offset)| *offset)
        .unwrap_or(8);
    file.seek(SeekFrom::Start(position))?;
    while length.saturating_sub(position) >= 12 {
        let mut header = [0; 12];
        file.read_exact(&mut header)?;
        let id = u32::from_le_bytes(header[0..4].try_into()?);
        let record_len = u32::from_le_bytes(header[4..8].try_into()?) as u64;
        let meta_len = u32::from_le_bytes(header[8..12].try_into()?) as u64;
        if record_len < 4
            || meta_len > record_len - 4
            || meta_len > MAX_MESSAGE as u64
            || record_len > 2 * MAX_MESSAGE as u64 + 4
        {
            bail!("invalid WebSocket record length");
        }
        let end = position + 8 + record_len;
        if end > length {
            break;
        }
        offsets.insert(id, position);
        offsets.insert(id.saturating_add(1), end);
        while offsets.len() > 4096 {
            offsets.pop_first();
        }
        if body_id == Some(id) || (body_id.is_none() && id > after) {
            let mut meta = vec![0; meta_len as usize];
            file.read_exact(&mut meta)?;
            let message: WebSocketMessage = serde_json::from_slice(&meta)?;
            if (message.content_available && message.size != record_len - 4 - meta_len)
                || (!message.content_available && record_len - 4 - meta_len != 0)
            {
                bail!("WebSocket payload size mismatch");
            }
            if message.id != id {
                bail!("WebSocket record id mismatch");
            }
            if body_id == Some(id) {
                if !message.content_available {
                    return Ok((page, None));
                }
                let mut body = vec![0; (record_len - 4 - meta_len) as usize];
                file.read_exact(&mut body)?;
                return Ok((page, Some(body)));
            }
            if page.messages.len() == limit.clamp(1, 2000) {
                page.has_more = true;
                break;
            }
            page.next_after_id = message.id;
            page.messages.push(message);
        }
        file.seek(SeekFrom::Start(end))?;
        position = end;
    }
    Ok((page, None))
}

struct Message {
    kind: u8,
    started: u64,
    compressed: bool,
    size: u64,
    payload: Vec<u8>,
}
struct Frame {
    fin: bool,
    opcode: u8,
    remaining: u64,
    offset: u64,
    mask: Option<[u8; 4]>,
}
struct Decoder {
    file: File,
    next_id: u32,
    header: Vec<u8>,
    frame_started: u64,
    frame: Option<Frame>,
    message: Option<Message>,
    close: Option<(Option<u16>, String)>,
    control: Vec<u8>,
    inflater: Option<Decompress>,
    no_takeover: bool,
    expect_mask: bool,
}
impl Decoder {
    fn new(path: &Path, extensions: &str, side: BodySide) -> Result<Self> {
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(path)?;
        file.write_all(MAGIC)?;
        let mut compressed = false;
        let mut no_takeover = false;
        let prefix = if side == BodySide::Request {
            "client"
        } else {
            "server"
        };
        for extension in extensions.split(',').filter(|s| !s.trim().is_empty()) {
            let mut parts = extension.split(';').map(str::trim);
            if parts.next() != Some("permessage-deflate") {
                bail!("unsupported WebSocket extension");
            }
            compressed = true;
            for part in parts {
                if part == format!("{prefix}_no_context_takeover") {
                    no_takeover = true;
                } else if let Some((key, value)) = part.split_once('=') {
                    if key == "client_max_window_bits" || key == "server_max_window_bits" {
                        let bits: u8 = value.trim_matches('"').parse()?;
                        if !(8..=15).contains(&bits) {
                            bail!("invalid deflate window");
                        }
                        // A 15-bit decoder accepts every negotiated smaller window too.
                    } else {
                        bail!("unsupported deflate parameter");
                    }
                } else if part != "client_no_context_takeover"
                    && part != "server_no_context_takeover"
                {
                    bail!("unsupported deflate parameter");
                }
            }
        }
        Ok(Self {
            file,
            next_id: 1,
            header: Vec::new(),
            frame_started: 0,
            frame: None,
            message: None,
            close: None,
            control: Vec::new(),
            inflater: compressed.then(|| Decompress::new(false)),
            no_takeover,
            expect_mask: side == BodySide::Request,
        })
    }
    fn append(&mut self, message: Message) -> Result<()> {
        let available = message.size <= MAX_MESSAGE as u64;
        if message.kind == 1 && available {
            std::str::from_utf8(&message.payload)?;
        }
        let meta = WebSocketMessage {
            id: self.next_id,
            kind: match message.kind {
                1 => "text",
                2 => "binary",
                _ => "close",
            }
            .into(),
            started_at: message.started,
            completed_at: now_millis(),
            size: message.size,
            content_available: available,
            omitted_reason: (!available).then(|| "size_limit_exceeded".into()),
            sequence: None,
            root: None,
            parent: None,
            producer: None,
            dropped_by: None,
            status: None,
            error: None,
            errors: vec![],
        };
        let json = serde_json::to_vec(&meta)?;
        let payload = if available {
            message.payload.as_slice()
        } else {
            &[]
        };
        let record_len = (4 + json.len() + payload.len()) as u32;
        self.file.write_all(&self.next_id.to_le_bytes())?;
        self.file.write_all(&record_len.to_le_bytes())?;
        self.file.write_all(&(json.len() as u32).to_le_bytes())?;
        self.file.write_all(&json)?;
        self.file.write_all(payload)?;
        self.file.flush()?;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("message id exhausted"))?;
        Ok(())
    }
    fn decoded(message: &mut Message, bytes: &[u8]) {
        message.size = message.size.saturating_add(bytes.len() as u64);
        if message.size <= MAX_MESSAGE as u64 {
            message.payload.extend_from_slice(bytes);
        } else {
            message.payload.clear();
        }
    }
    fn data(&mut self, mut bytes: &[u8]) -> Result<()> {
        let message = self
            .message
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("continuation without message"))?;
        if !message.compressed {
            Self::decoded(message, bytes);
            return Ok(());
        }
        let inflater = self
            .inflater
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("unnegotiated compression"))?;
        loop {
            let mut output = [0; 8192];
            let before_in = inflater.total_in();
            let before_out = inflater.total_out();
            inflater.decompress(bytes, &mut output, FlushDecompress::Sync)?;
            let used = (inflater.total_in() - before_in) as usize;
            let produced = (inflater.total_out() - before_out) as usize;
            Self::decoded(message, &output[..produced]);
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
    fn feed(&mut self, mut input: &[u8]) -> Result<()> {
        while !input.is_empty() || self.frame.as_ref().is_some_and(|f| f.remaining == 0) {
            if self.frame.is_none() {
                if self.header.is_empty() {
                    self.frame_started = now_millis();
                }
                self.header.push(input[0]);
                input = &input[1..];
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
                if masked != self.expect_mask || first & 48 != 0 || length >> 63 != 0 {
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
                    self.message = Some(Message {
                        kind: opcode,
                        started: self.frame_started,
                        compressed,
                        size: 0,
                        payload: Vec::new(),
                    });
                }
                let mask = masked.then(|| self.header[need - 4..need].try_into().unwrap());
                self.frame = Some(Frame {
                    fin,
                    opcode,
                    remaining: length,
                    offset: 0,
                    mask,
                });
                self.header.clear();
                self.control.clear();
            }
            let frame = self.frame.as_mut().unwrap();
            let n = input.len().min(frame.remaining.min(8192) as usize);
            let mut bytes = input[..n].to_vec();
            input = &input[n..];
            if let Some(mask) = frame.mask {
                for (i, b) in bytes.iter_mut().enumerate() {
                    *b ^= mask[((frame.offset + i as u64) % 4) as usize];
                }
            }
            frame.offset += n as u64;
            frame.remaining -= n as u64;
            let opcode = frame.opcode;
            let fin = frame.fin;
            let done = frame.remaining == 0;
            if opcode < 8 {
                self.data(&bytes)?;
            } else {
                self.control.extend_from_slice(&bytes);
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
                    let message = self.message.take().unwrap();
                    self.append(message)?;
                } else if opcode == 8 {
                    if self.control.len() == 1 {
                        bail!("invalid close payload");
                    }
                    if self.control.len() > 2 {
                        std::str::from_utf8(&self.control[2..])?;
                    }
                    self.close = Some((
                        if self.control.len() >= 2 {
                            Some(u16::from_be_bytes([self.control[0], self.control[1]]))
                        } else {
                            None
                        },
                        if self.control.len() > 2 {
                            std::str::from_utf8(&self.control[2..])?.to_owned()
                        } else {
                            String::new()
                        },
                    ));
                }
            }
        }
        Ok(())
    }
}

pub(crate) async fn forward<A, B>(
    downstream: A,
    upstream: B,
    store: CaptureStore,
    id: u64,
    extensions: &str,
    cancellation: tokio_util::sync::CancellationToken,
    pin: Option<Arc<crate::runtime::SessionPin>>,
) -> std::io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    async fn pump<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
        mut reader: R,
        mut writer: W,
        store: &CaptureStore,
        id: u64,
        side: BodySide,
        extensions: &str,
        cancellation: &tokio_util::sync::CancellationToken,
        pin: &Option<Arc<crate::runtime::SessionPin>>,
    ) -> std::io::Result<()> {
        let note = |error: &dyn std::fmt::Display| {
            let _ = store.note_error(
                id,
                &CaptureError {
                    stage: ErrorStage::ResponseBody,
                    kind: "websocket_capture_failed".into(),
                    message: error.to_string(),
                },
            );
        };
        let mut decoder = match Decoder::new(&store.websocket_body_path(id, side), extensions, side)
        {
            Ok(d) => Some(d),
            Err(e) => {
                note(&e);
                None
            }
        };
        let mut buffer = [0; 16384];
        loop {
            let cancelled = || {
                std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "WebSocket proxy connection cancelled",
                )
            };
            let count = tokio::select! { result=reader.read(&mut buffer)=>result?,_=cancellation.cancelled()=>return Err(cancelled()) };
            if count == 0 {
                if decoder.as_ref().is_some_and(|d| {
                    d.message.is_some() || d.frame.is_some() || !d.header.is_empty()
                }) {
                    note(&"incomplete WebSocket message at EOF");
                }
                tokio::select! {result=writer.shutdown()=>result?,_=cancellation.cancelled()=>return Err(cancelled())};
                return Ok(());
            }
            tokio::select! {result=writer.write_all(&buffer[..count])=>result?,_=cancellation.cancelled()=>return Err(cancelled())};
            // HTTPS MITM can wrap the socket in a BufStream; flush small frames before
            // waiting for more input so data and heartbeat replies reach the peer promptly.
            tokio::select! {result=writer.flush()=>result?,_=cancellation.cancelled()=>return Err(cancelled())};
            if let Some(mut d) = decoder.take() {
                // At most one bounded input chunk per direction is in flight. Forwarding
                // precedes capture; disk writes and decompression do not block the IO worker.
                let bytes = buffer[..count].to_vec();
                let capture = store.clone();
                let pin = pin.clone();
                match tokio::task::spawn_blocking(move || -> Result<Decoder> {
                    let _pin = pin;
                    d.feed(&bytes)?;
                    if let Some((code, reason)) = d.close.take() {
                        capture.websocket_close_frame(id, code, &reason)?;
                    }
                    Ok(d)
                })
                .await
                {
                    Ok(Ok(d)) => decoder = Some(d),
                    Ok(Err(error)) => note(&error),
                    Err(error) => note(&error),
                }
            }
        }
    }
    let (dr, dw) = tokio::io::split(downstream);
    let (ur, uw) = tokio::io::split(upstream);
    let cancellation = cancellation.child_token();
    let (request, response) = tokio::join!(
        async {
            let result = pump(
                dr,
                uw,
                &store,
                id,
                BodySide::Request,
                extensions,
                &cancellation,
                &pin,
            )
            .await;
            if result.is_err() {
                cancellation.cancel();
            }
            result
        },
        async {
            let result = pump(
                ur,
                dw,
                &store,
                id,
                BodySide::Response,
                extensions,
                &cancellation,
                &pin,
            )
            .await;
            if result.is_err() {
                cancellation.cancel();
            }
            result
        }
    );
    request.and(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(first: u8, body: &[u8], masked: bool) -> Vec<u8> {
        let mut output = vec![first];
        let flag = if masked { 128 } else { 0 };
        if body.len() < 126 {
            output.push(flag | body.len() as u8);
        } else if body.len() <= 65535 {
            output.push(flag | 126);
            output.extend_from_slice(&(body.len() as u16).to_be_bytes());
        } else {
            output.push(flag | 127);
            output.extend_from_slice(&(body.len() as u64).to_be_bytes());
        }
        let mask = [1, 2, 3, 4];
        if masked {
            output.extend_from_slice(&mask);
        }
        output.extend(
            body.iter()
                .enumerate()
                .map(|(i, b)| if masked { b ^ mask[i % 4] } else { *b }),
        );
        output
    }
    #[test]
    fn fragmented_masked_with_controls_and_incomplete_tail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body");
        let mut decoder = Decoder::new(&path, "", BodySide::Request).unwrap();
        let bytes = [
            frame(1, b"hel", true),
            frame(137, b"ping", true),
            frame(128, b"lo", true),
            frame(130, b"binary", true),
            frame(136, &[3, 232], true),
        ]
        .concat();
        for byte in bytes {
            decoder.feed(&[byte]).unwrap();
        }
        let (page, _) = read_records(&path, 0, 1, None).unwrap();
        assert!(page.has_more);
        assert_eq!(page.next_after_id, 1);
        assert_eq!(page.messages[0].kind, "text");
        assert_eq!(
            read_records(&path, 0, 1, Some(1)).unwrap().1.unwrap(),
            b"hello"
        );
        let (page, _) = read_records(&path, 1, 10, None).unwrap();
        assert_eq!(page.messages.len(), 1);
        assert_eq!(decoder.close.as_ref().unwrap().0, Some(1000));
        assert!(!page.has_more);
        assert_eq!(page.next_after_id, 2);
        decoder
            .file
            .write_all(&[4, 0, 0, 0, 100, 0, 0, 0, 50, 0, 0, 0, b'{'])
            .unwrap();
        assert!(
            read_records(&path, 3, 10, None)
                .unwrap()
                .0
                .messages
                .is_empty()
        );
    }
    #[test]
    fn oversized_fragment_skips_content_and_recovers_next_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body");
        let mut decoder = Decoder::new(&path, "", BodySide::Response).unwrap();
        let body = vec![42; MAX_MESSAGE];
        decoder.feed(&frame(2, &body, false)).unwrap();
        decoder.feed(&frame(128, b"x", false)).unwrap();
        decoder.feed(&frame(129, b"next", false)).unwrap();
        let page = read_records(&path, 0, 10, None).unwrap().0;
        assert_eq!(page.messages[0].size, MAX_MESSAGE as u64 + 1);
        assert_eq!(
            page.messages[0].omitted_reason.as_deref(),
            Some("size_limit_exceeded")
        );
        assert!(read_records(&path, 0, 1, Some(1)).unwrap().1.is_none());
        assert_eq!(
            read_records(&path, 0, 1, Some(2)).unwrap().1.unwrap(),
            b"next"
        );
    }
    #[test]
    fn compressed_context_takeover_and_reset() {
        use flate2::{Compress, Compression, FlushCompress};
        for reset in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("body");
            let extension = if reset {
                "permessage-deflate; server_no_context_takeover; server_max_window_bits=10"
            } else {
                "permessage-deflate"
            };
            let mut decoder = Decoder::new(&path, extension, BodySide::Response).unwrap();
            let mut compressor = Compress::new(Compression::default(), false);
            for id in 1..=3 {
                let text = b"repeated vocabulary repeated vocabulary repeated vocabulary";
                let mut output = [0; 1024];
                let before = compressor.total_out();
                compressor
                    .compress(text, &mut output, FlushCompress::Sync)
                    .unwrap();
                let n = (compressor.total_out() - before) as usize;
                assert_eq!(&output[n - 4..n], &[0, 0, 255, 255]);
                let compressed = &output[..n - 4];
                let middle = compressed.len() / 2;
                decoder
                    .feed(&frame(65, &compressed[..middle], false))
                    .unwrap();
                decoder
                    .feed(&frame(128, &compressed[middle..], false))
                    .unwrap();
                assert_eq!(
                    read_records(&path, 0, 1, Some(id)).unwrap().1.unwrap(),
                    text
                );
                if reset {
                    compressor.reset();
                }
            }
        }
    }
    #[test]
    fn record_index_stays_bounded_when_reading_older_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body");
        let mut decoder = Decoder::new(&path, "", BodySide::Response).unwrap();
        for _ in 0..4200 {
            decoder.feed(&frame(129, b"message", false)).unwrap();
        }
        let mut index = RecordIndex::default();
        assert_eq!(
            index.read(&path, 0, 1, Some(4200)).unwrap().1.unwrap(),
            b"message"
        );
        assert_eq!(index.files[&path].len(), 4096);
        for id in [1, 4, 7, 10] {
            assert_eq!(
                index.read(&path, 0, 1, Some(id)).unwrap().1.unwrap(),
                b"message"
            );
            assert_eq!(index.files[&path].len(), 4096);
        }
    }

    #[test]
    fn compressed_oversize_preserves_context_and_index_survives_append() {
        use flate2::{Compress, Compression, FlushCompress};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("body");
        let mut decoder = Decoder::new(&path, "permessage-deflate", BodySide::Response).unwrap();
        let mut compressor = Compress::new(Compression::default(), false);
        let big = vec![b'a'; MAX_MESSAGE + 1];
        for input in [big.as_slice(), b"aaaa next"] {
            let mut output = Vec::with_capacity(128 * 1024);
            compressor
                .compress_vec(input, &mut output, FlushCompress::Sync)
                .unwrap();
            output.truncate(output.len() - 4);
            decoder.feed(&frame(193, &output, false)).unwrap();
        }
        let mut index = RecordIndex::default();
        let page = index.read(&path, 0, 1, None).unwrap().0;
        assert!(page.has_more);
        assert!(!page.messages[0].content_available);
        assert_eq!(page.messages[0].size, MAX_MESSAGE as u64 + 1);
        assert_eq!(
            index.read(&path, 0, 1, Some(2)).unwrap().1.unwrap(),
            b"aaaa next"
        );
        assert!(index.read(&path, 2, 1, None).unwrap().0.messages.is_empty());
        decoder.feed(&frame(129, b"later", false)).unwrap();
        assert_eq!(index.read(&path, 2, 1, None).unwrap().0.messages[0].id, 3);
        assert_eq!(
            index.read(&path, 0, 1, Some(3)).unwrap().1.unwrap(),
            b"later"
        );
    }

    #[tokio::test]
    async fn buffered_forwarding_delivers_small_data_and_control_frames_without_waiting_for_eof() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaptureStore::open(1, dir.path()).unwrap();
        let request = crate::model::RequestData {
            method: "GET".into(),
            uri: "wss://example.test/".into(),
            version: "WebSocket/13".into(),
            headers: Default::default(),
            tags: Default::default(),
        };
        let id = store.begin("test", &request, "websocket").unwrap();
        let (mut client, downstream) = tokio::io::duplex(1024);
        let (upstream, mut server) = tokio::io::duplex(1024);
        let cancellation = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(forward(
            tokio::io::BufStream::new(downstream),
            tokio::io::BufStream::new(upstream),
            store,
            id,
            "",
            cancellation.clone(),
            None,
        ));

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            for opcode in [130, 137, 138] {
                let request = frame(opcode, b"small", true);
                client.write_all(&request).await.unwrap();
                let mut received = vec![0; request.len()];
                server.read_exact(&mut received).await.unwrap();
                assert_eq!(received, request);

                let response = frame(opcode, b"small", false);
                server.write_all(&response).await.unwrap();
                let mut received = vec![0; response.len()];
                client.read_exact(&mut received).await.unwrap();
                assert_eq!(received, response);
            }
        })
        .await
        .expect("small WebSocket frames must reach both peers before either connection closes");

        cancellation.cancel();
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            std::io::ErrorKind::Interrupted
        );
    }

    #[tokio::test]
    async fn buffered_forwarding_can_cancel_a_blocked_flush() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaptureStore::open(1, dir.path()).unwrap();
        let request = crate::model::RequestData {
            method: "GET".into(),
            uri: "wss://example.test/".into(),
            version: "WebSocket/13".into(),
            headers: Default::default(),
            tags: Default::default(),
        };
        let id = store.begin("test", &request, "websocket").unwrap();
        let (mut client, downstream) = tokio::io::duplex(1024);
        let (upstream, mut server) = tokio::io::duplex(1);
        let cancellation = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(forward(
            downstream,
            tokio::io::BufStream::new(upstream),
            store,
            id,
            "",
            cancellation.clone(),
            None,
        ));

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            client.write_all(&frame(137, b"ping", true)).await.unwrap();
            let mut first_byte = [0];
            server.read_exact(&mut first_byte).await.unwrap();
            assert_eq!(first_byte, [137]);
            // Leave the one-byte transport full so flush cannot finish without cancellation.
            cancellation.cancel();
            assert_eq!(
                task.await.unwrap().unwrap_err().kind(),
                std::io::ErrorKind::Interrupted
            );
        })
        .await
        .expect("cancelling a WebSocket connection must interrupt a blocked flush");
    }

    #[tokio::test]
    async fn capture_error_does_not_change_bidirectional_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = CaptureStore::open(1, dir.path()).unwrap();
        let request = crate::model::RequestData {
            method: "GET".into(),
            uri: "ws://example.test/".into(),
            version: "WebSocket/13".into(),
            headers: Default::default(),
            tags: Default::default(),
        };
        let id = store.begin("test", &request, "websocket").unwrap();
        let (mut client, downstream) = tokio::io::duplex(1024);
        let (upstream, mut server) = tokio::io::duplex(1024);
        let task = tokio::spawn(async move {
            forward(
                downstream,
                upstream,
                store,
                id,
                "",
                tokio_util::sync::CancellationToken::new(),
                None,
            )
            .await
            .unwrap();
        });
        client
            .write_all(b"invalid frame but still forwarded")
            .await
            .unwrap();
        let mut bytes = [0; 33];
        server.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"invalid frame but still forwarded");
        let response = frame(129, b"still works", false);
        server.write_all(&response).await.unwrap();
        let mut received = vec![0; response.len()];
        client.read_exact(&mut received).await.unwrap();
        assert_eq!(received, response);
        client.shutdown().await.unwrap();
        server.shutdown().await.unwrap();
        task.await.unwrap();
    }
}
