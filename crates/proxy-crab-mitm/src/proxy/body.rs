use std::{
    convert::Infallible,
    error::Error as StdError,
    future::Future,
    pin::Pin,
    sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}},
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::body::{Body, Frame};
use tokio::time::{Instant, Sleep};

use crate::bypass::BypassStore;

const PACE_CHUNKS_PER_SECOND: u64 = 50;

pub(super) type BoxError = Box<dyn StdError + Send + Sync>;
pub(super) type ProxyBody = UnsyncBoxBody<Bytes, BoxError>;

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
