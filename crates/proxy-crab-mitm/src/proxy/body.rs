use std::{
    convert::Infallible,
    error::Error as StdError,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::body::{Body, Frame};
use tokio::time::{Instant, Sleep};

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
