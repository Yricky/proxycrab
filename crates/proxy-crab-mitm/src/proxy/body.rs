use std::{
    convert::Infallible,
    error::Error as StdError,
    future::Future,
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::body::{Body, Frame};
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, Sleep, timeout},
};

use crate::{bypass::BypassStore, storage::CaptureBodyWriter};

use super::TaskGroup;

const PACE_CHUNKS_PER_SECOND: u64 = 50;

pub(super) type BoxError = Box<dyn StdError + Send + Sync>;
pub(super) type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PumpOutcome {
    Complete,
    InputError(String),
    FrameTimeout,
    OutputClosed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PumpResult {
    pub(super) outcome: PumpOutcome,
    pub(super) storage_error: Option<String>,
}

struct ChannelBody {
    receiver: mpsc::Receiver<Result<Frame<Bytes>, BoxError>>,
    size_hint: hyper::body::SizeHint,
}

impl Body for ChannelBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        self.get_mut().receiver.poll_recv(context)
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.size_hint
    }
}

pub(super) fn pump_body<B>(
    mut body: B,
    mut writer: Option<CaptureBodyWriter>,
    frame_timeout: Option<Duration>,
    bytes_per_second: Option<u64>,
    forward: bool,
    tracker: &TaskGroup,
) -> (Option<ProxyBody>, oneshot::Receiver<PumpResult>)
where
    B: Body<Data = Bytes> + Unpin + Send + 'static,
    B::Error: Into<BoxError> + Send + Sync + 'static,
{
    let size_hint = body.size_hint();
    let (frame_sender, frame_receiver) = mpsc::channel::<Result<Frame<Bytes>, BoxError>>(4);
    let (done_sender, done_receiver) = oneshot::channel();
    tracker.spawn(async move {
        let mut storage_error = None;
        let mut forwarding = forward;
        let mut output_closed = false;
        let outcome = loop {
            let next = match frame_timeout {
                Some(duration) => match timeout(duration, body.frame()).await {
                    Ok(frame) => frame,
                    Err(_) => {
                        if forwarding {
                            let _ = frame_sender
                                .send(Err(Box::new(io::Error::new(
                                    io::ErrorKind::TimedOut,
                                    "response body frame timed out",
                                ))))
                                .await;
                        }
                        break PumpOutcome::FrameTimeout;
                    }
                },
                None => body.frame().await,
            };
            let Some(frame) = next else {
                break PumpOutcome::Complete;
            };
            let frame = match frame {
                Ok(frame) => frame,
                Err(error) => {
                    let error = error.into();
                    let message = error.to_string();
                    if forwarding {
                        let _ = frame_sender.send(Err(error)).await;
                    }
                    break PumpOutcome::InputError(message);
                }
            };
            if let Some(data) = frame.data_ref()
                && let Some(active_writer) = &mut writer
                && let Err(error) = active_writer.write_all(data).await
            {
                storage_error = Some(error.to_string());
                writer = None;
            }
            if forwarding
                && send_frame(&frame_sender, frame, bytes_per_second)
                    .await
                    .is_err()
            {
                forwarding = false;
                output_closed = true;
            }
        };
        let outcome = if outcome == PumpOutcome::Complete && output_closed {
            PumpOutcome::OutputClosed
        } else {
            outcome
        };
        if let Some(active_writer) = writer
            && let Err(error) = active_writer.finish().await
        {
            storage_error = Some(error.to_string());
        }
        let _ = done_sender.send(PumpResult {
            outcome,
            storage_error,
        });
    });
    let output = forward.then(|| {
        ChannelBody {
            receiver: frame_receiver,
            size_hint,
        }
        .boxed_unsync()
    });
    (output, done_receiver)
}

async fn send_frame(
    sender: &mpsc::Sender<Result<Frame<Bytes>, BoxError>>,
    frame: Frame<Bytes>,
    bytes_per_second: Option<u64>,
) -> Result<(), ()> {
    let Some(bytes_per_second) = bytes_per_second else {
        return sender.send(Ok(frame)).await.map_err(|_| ());
    };
    let data = match frame.into_data() {
        Ok(data) => data,
        Err(frame) => return sender.send(Ok(frame)).await.map_err(|_| ()),
    };
    let chunk_len = usize::try_from(bytes_per_second.div_ceil(PACE_CHUNKS_PER_SECOND).max(1))
        .unwrap_or(usize::MAX);
    for chunk in data.chunks(chunk_len) {
        let nanos = (chunk.len() as u128 * 1_000_000_000_u128).div_ceil(bytes_per_second as u128);
        tokio::time::sleep(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
        .await;
        sender
            .send(Ok(Frame::data(Bytes::copy_from_slice(chunk))))
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}

pub(super) struct PacedBody {
    bytes: Bytes,
    offset: usize,
    bytes_per_second: u64,
    pending_chunk_len: usize,
    sleep: Option<Pin<Box<Sleep>>>,
}

impl PacedBody {
    pub(super) fn new(bytes: Bytes, bytes_per_second: u64) -> Self {
        debug_assert!(bytes_per_second > 0);
        Self {
            bytes,
            offset: 0,
            bytes_per_second,
            pending_chunk_len: 0,
            sleep: None,
        }
    }

    fn next_chunk_len(&self) -> usize {
        let remaining = self.bytes.len() - self.offset;
        let target = self
            .bytes_per_second
            .div_ceil(PACE_CHUNKS_PER_SECOND)
            .max(1);
        usize::try_from(target).unwrap_or(usize::MAX).min(remaining)
    }

    fn chunk_delay(&self, chunk_len: usize) -> Duration {
        let nanos =
            (chunk_len as u128 * 1_000_000_000_u128).div_ceil(self.bytes_per_second as u128);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

impl Body for PacedBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.offset == this.bytes.len() {
            return Poll::Ready(None);
        }
        if this.sleep.is_none() {
            this.pending_chunk_len = this.next_chunk_len();
            this.sleep = Some(Box::pin(tokio::time::sleep_until(
                Instant::now() + this.chunk_delay(this.pending_chunk_len),
            )));
        }
        if this
            .sleep
            .as_mut()
            .expect("paced body sleep was initialized")
            .as_mut()
            .poll(context)
            .is_pending()
        {
            return Poll::Pending;
        }
        this.sleep = None;
        let end = this.offset + this.pending_chunk_len;
        let chunk = this.bytes.slice(this.offset..end);
        this.offset = end;
        Poll::Ready(Some(Ok(Frame::data(chunk))))
    }

    fn is_end_stream(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        hyper::body::SizeHint::with_exact((self.bytes.len() - self.offset) as u64)
    }
}

pub(super) fn boxed_full(bytes: Bytes) -> ProxyBody {
    Full::new(bytes)
        .map_err(|error: Infallible| match error {})
        .boxed_unsync()
}

/// Shared accounting state for a bypass/streaming transfer. Written by
/// [`TrackedBody`] as request/response frames flow through; finalizes the
/// underlying bypass entry exactly once (or fails it on error/truncation).
pub(super) struct BypassTransfer {
    store: BypassStore,
    entry_id: Option<u64>,
    upload_bytes: AtomicU64,
    download_bytes: AtomicU64,
    finalized: AtomicBool,
}

impl BypassTransfer {
    pub(super) fn new(store: BypassStore, entry_id: Option<u64>) -> Arc<Self> {
        Arc::new(Self {
            store,
            entry_id,
            upload_bytes: AtomicU64::new(0),
            download_bytes: AtomicU64::new(0),
            finalized: AtomicBool::new(false),
        })
    }

    pub(super) fn add_upload(&self, bytes: u64) {
        self.upload_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub(super) fn add_download(&self, bytes: u64) {
        self.download_bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    pub(super) fn complete(&self, response_status: Option<u16>) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(id) = self.entry_id {
            let _ = self.store.complete(
                id,
                response_status,
                Some(self.upload_bytes.load(Ordering::Relaxed)),
                Some(self.download_bytes.load(Ordering::Relaxed)),
            );
        }
    }

    pub(super) fn fail(&self, error: &str) {
        if self.finalized.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(id) = self.entry_id {
            let _ = self.store.fail(
                id,
                error,
                Some(self.upload_bytes.load(Ordering::Relaxed)),
                Some(self.download_bytes.load(Ordering::Relaxed)),
            );
        }
    }
}

/// [`Body`] wrapper that counts upload/download bytes into a [`BypassTransfer`]
/// and finalizes the underlying bypass entry exactly once, mirroring the
/// downstream stream's end-of-stream, error, or early-drop lifecycle.
pub(super) struct TrackedBody<B> {
    inner: Pin<Box<B>>,
    transfer: Arc<BypassTransfer>,
    response_status: Option<u16>,
    response_remaining: Option<u64>,
    reached_eof: bool,
}

impl<B> TrackedBody<B> {
    pub(super) fn request(inner: B, transfer: Arc<BypassTransfer>) -> Self {
        Self {
            inner: Box::pin(inner),
            transfer,
            response_status: None,
            response_remaining: None,
            reached_eof: false,
        }
    }

    pub(super) fn response(inner: B, transfer: Arc<BypassTransfer>, response_status: u16) -> Self
    where
        B: Body,
    {
        let response_remaining = inner.size_hint().exact();
        let reached_eof = inner.is_end_stream() || response_remaining == Some(0);
        if reached_eof {
            transfer.complete(Some(response_status));
        }
        Self {
            inner: Box::pin(inner),
            transfer,
            response_status: Some(response_status),
            response_remaining,
            reached_eof,
        }
    }
}

impl<B> Body for TrackedBody<B>
where
    B: Body<Data = Bytes>,
    B::Error: StdError + Send + Sync + 'static,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        match this.inner.as_mut().poll_frame(context) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    if this.response_status.is_some() {
                        this.transfer.add_download(data.len() as u64);
                        if let Some(remaining) = &mut this.response_remaining {
                            *remaining = remaining.saturating_sub(data.len() as u64);
                            if *remaining == 0 {
                                this.reached_eof = true;
                                this.transfer.complete(this.response_status);
                            }
                        }
                    } else {
                        this.transfer.add_upload(data.len() as u64);
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(error))) => {
                this.reached_eof = true;
                this.transfer.fail(&error.to_string());
                Poll::Ready(Some(Err(Box::new(error))))
            }
            Poll::Ready(None) => {
                this.reached_eof = true;
                if let Some(status) = this.response_status {
                    this.transfer.complete(Some(status));
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> hyper::body::SizeHint {
        self.inner.size_hint()
    }
}

impl<B> Drop for TrackedBody<B> {
    fn drop(&mut self) {
        if self.response_status.is_some() && !self.reached_eof {
            self.transfer
                .fail("downstream response body closed before completion");
        }
    }
}
