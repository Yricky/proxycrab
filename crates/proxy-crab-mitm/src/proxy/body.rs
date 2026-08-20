use std::{
    convert::Infallible,
    error::Error as StdError,
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc as std_mpsc,
    },
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use futures::TryStreamExt;
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{
    HeaderMap,
    body::{Body, Frame},
};
use tokio::{
    fs::File,
    sync::{mpsc, oneshot},
    time::{Instant, Sleep, timeout},
};
use tokio_util::io::ReaderStream;

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
pub(crate) struct PumpResult {
    pub(super) outcome: PumpOutcome,
    pub(super) storage_error: Option<String>,
}

struct ChannelBody {
    receiver: mpsc::Receiver<Result<Frame<Bytes>, BoxError>>,
    size_hint: hyper::body::SizeHint,
}

enum DeferredCommand {
    Read {
        frame_timeout: Option<Duration>,
        result: std_mpsc::Sender<Result<DeferredBodyRead, String>>,
    },
    Finish {
        forward: bool,
        bytes_per_second: Option<u64>,
        frame_timeout: Option<Duration>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DeferredBodyRead {
    Empty,
    File(PathBuf),
}

#[derive(Clone)]
pub(crate) struct DeferredBodyReader {
    sender: mpsc::UnboundedSender<DeferredCommand>,
}

impl DeferredBodyReader {
    pub(crate) fn read(&self, frame_timeout: Option<Duration>) -> Result<DeferredBodyRead, String> {
        let (sender, receiver) = std_mpsc::channel();
        self.sender
            .send(DeferredCommand::Read {
                frame_timeout,
                result: sender,
            })
            .map_err(|_| "body reader is no longer available".to_owned())?;
        receiver
            .recv()
            .map_err(|_| "body reader stopped before completing".to_owned())?
    }
}

pub(crate) struct DeferredBody {
    reader: DeferredBodyReader,
    sender: mpsc::UnboundedSender<DeferredCommand>,
    receiver: Mutex<Option<mpsc::Receiver<Result<Frame<Bytes>, BoxError>>>>,
    size_hint: hyper::body::SizeHint,
    done: Option<oneshot::Receiver<PumpResult>>,
}

impl DeferredBody {
    pub(crate) fn new<B>(body: B, writer: CaptureBodyWriter, tracker: &TaskGroup) -> Self
    where
        B: Body<Data = Bytes> + Unpin + Send + 'static,
        B::Error: Into<BoxError> + Send + Sync + 'static,
    {
        let size_hint = body.size_hint();
        let path = writer.path().to_path_buf();
        let (command_sender, command_receiver) = mpsc::unbounded_channel();
        let (frame_sender, frame_receiver) = mpsc::channel(4);
        let (done_sender, done_receiver) = oneshot::channel();
        tracker.spawn(run_deferred_body(
            body,
            writer,
            path,
            command_receiver,
            frame_sender,
            done_sender,
        ));
        Self {
            reader: DeferredBodyReader {
                sender: command_sender.clone(),
            },
            sender: command_sender,
            receiver: Mutex::new(Some(frame_receiver)),
            size_hint,
            done: Some(done_receiver),
        }
    }

    pub(crate) fn reader(&self) -> DeferredBodyReader {
        self.reader.clone()
    }

    pub(crate) fn finish(
        mut self,
        forward: bool,
        bytes_per_second: Option<u64>,
        frame_timeout: Option<Duration>,
    ) -> (Option<ProxyBody>, oneshot::Receiver<PumpResult>) {
        let _ = self.sender.send(DeferredCommand::Finish {
            forward,
            bytes_per_second,
            frame_timeout,
        });
        let output = forward.then(|| {
            ChannelBody {
                receiver: self
                    .receiver
                    .lock()
                    .expect("deferred body receiver lock poisoned")
                    .take()
                    .expect("deferred body can only be finished once"),
                size_hint: self.size_hint,
            }
            .boxed_unsync()
        });
        (
            output,
            self.done.take().expect("deferred body has completion"),
        )
    }
}

async fn run_deferred_body<B>(
    mut body: B,
    writer: CaptureBodyWriter,
    path: PathBuf,
    mut commands: mpsc::UnboundedReceiver<DeferredCommand>,
    frames: mpsc::Sender<Result<Frame<Bytes>, BoxError>>,
    done: oneshot::Sender<PumpResult>,
) where
    B: Body<Data = Bytes> + Unpin + Send + 'static,
    B::Error: Into<BoxError> + Send + Sync + 'static,
{
    let mut writer = Some(writer);
    let mut completed: Option<(PumpResult, bool)> = None;
    let mut trailers = Vec::new();
    let mut read_waiters = Vec::new();
    let (forward, speed, frame_timeout) = loop {
        match commands.recv().await {
            Some(DeferredCommand::Read {
                frame_timeout,
                result: waiter,
            }) => {
                if let Some((result, stored)) = &completed {
                    let _ = waiter.send(read_result(result, *stored, &path));
                    continue;
                }
                read_waiters.push(waiter);
                let result = drain_body(
                    &mut body,
                    writer.as_mut(),
                    frame_timeout,
                    None,
                    false,
                    &frames,
                    Some(&mut trailers),
                )
                .await;
                let (result, stored) = finish_writer(
                    writer.take().expect("deferred body writer is available"),
                    result,
                )
                .await;
                for waiter in read_waiters.drain(..) {
                    let _ = waiter.send(read_result(&result, stored, &path));
                }
                completed = Some((result, stored));
            }
            Some(DeferredCommand::Finish {
                forward,
                bytes_per_second,
                frame_timeout,
            }) => break (forward, bytes_per_second, frame_timeout),
            None => {
                let result = completed.map(|(result, _)| result).unwrap_or(PumpResult {
                    outcome: PumpOutcome::OutputClosed,
                    storage_error: None,
                });
                let _ = done.send(result);
                return;
            }
        }
    };

    let result = if let Some((result, stored)) = completed {
        if forward && result.outcome == PumpOutcome::Complete && result.storage_error.is_none() {
            let mut replay_result = if stored {
                match File::open(&path).await {
                    Ok(file) => {
                        let mut replay = http_body_util::StreamBody::new(
                            ReaderStream::new(file)
                                .map_ok(Frame::data)
                                .map_err(|error| Box::new(error) as BoxError),
                        );
                        drain_body(&mut replay, None, None, speed, true, &frames, None).await
                    }
                    Err(error) => PumpResult {
                        outcome: PumpOutcome::InputError(error.to_string()),
                        storage_error: None,
                    },
                }
            } else {
                result
            };
            if replay_result.outcome == PumpOutcome::Complete {
                for trailer in trailers {
                    if send_frame(&frames, Frame::trailers(trailer), speed)
                        .await
                        .is_err()
                    {
                        replay_result.outcome = PumpOutcome::OutputClosed;
                        break;
                    }
                }
            }
            replay_result
        } else {
            if forward {
                let message = result
                    .storage_error
                    .clone()
                    .or_else(|| match &result.outcome {
                        PumpOutcome::InputError(error) => Some(error.clone()),
                        PumpOutcome::FrameTimeout => Some("response body frame timed out".into()),
                        PumpOutcome::OutputClosed => {
                            Some("body output closed before completion".into())
                        }
                        PumpOutcome::Complete => None,
                    });
                if let Some(message) = message {
                    let _ = frames.send(Err(Box::new(io::Error::other(message)))).await;
                }
            }
            result
        }
    } else {
        let result = drain_body(
            &mut body,
            writer.as_mut(),
            frame_timeout,
            speed,
            forward,
            &frames,
            None,
        )
        .await;
        finish_writer(
            writer.take().expect("deferred body writer is available"),
            result,
        )
        .await
        .0
    };
    let _ = done.send(result);
}

fn read_result(
    result: &PumpResult,
    stored: bool,
    path: &std::path::Path,
) -> Result<DeferredBodyRead, String> {
    if let Some(error) = &result.storage_error {
        return Err(error.clone());
    }
    match &result.outcome {
        PumpOutcome::Complete if stored => Ok(DeferredBodyRead::File(path.to_path_buf())),
        PumpOutcome::Complete => Ok(DeferredBodyRead::Empty),
        PumpOutcome::InputError(error) => Err(error.clone()),
        PumpOutcome::FrameTimeout => Err("response body frame timed out".into()),
        PumpOutcome::OutputClosed => Err("body output closed before completion".into()),
    }
}

async fn finish_writer(writer: CaptureBodyWriter, mut result: PumpResult) -> (PumpResult, bool) {
    let stored = writer.has_file();
    if let Err(error) = writer.finish().await {
        result.storage_error = Some(error.to_string());
    }
    (result, stored)
}

async fn drain_body<B>(
    body: &mut B,
    mut writer: Option<&mut CaptureBodyWriter>,
    frame_timeout: Option<Duration>,
    bytes_per_second: Option<u64>,
    forward: bool,
    frames: &mpsc::Sender<Result<Frame<Bytes>, BoxError>>,
    mut captured_trailers: Option<&mut Vec<HeaderMap>>,
) -> PumpResult
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    let mut storage_error = None;
    let mut forwarding = forward;
    let mut output_closed = false;
    let outcome = loop {
        let next = match frame_timeout {
            Some(duration) => match timeout(duration, body.frame()).await {
                Ok(frame) => frame,
                Err(_) => {
                    if forwarding {
                        let _ = frames
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
                    let _ = frames.send(Err(error)).await;
                }
                break PumpOutcome::InputError(message);
            }
        };
        if let Some(trailers) = frame.trailers_ref()
            && let Some(captured) = captured_trailers.as_deref_mut()
        {
            captured.push(trailers.clone());
        }
        if let Some(data) = frame.data_ref()
            && let Some(active_writer) = writer.as_deref_mut()
            && let Err(error) = active_writer.write_all(data).await
        {
            storage_error = Some(error.to_string());
            writer = None;
        }
        if forwarding && send_frame(frames, frame, bytes_per_second).await.is_err() {
            forwarding = false;
            output_closed = true;
        }
    };
    PumpResult {
        outcome: if outcome == PumpOutcome::Complete && output_closed {
            PumpOutcome::OutputClosed
        } else {
            outcome
        },
        storage_error,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{HeaderValues, RequestData, RequestTags},
        storage::{BodySide, CaptureStore},
    };
    use futures::stream;
    use http_body_util::StreamBody;
    use hyper::http::HeaderValue;
    use tempfile::tempdir;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deferred_empty_body_can_be_read_and_replayed_without_a_file() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(1, root.path()).unwrap();
        let request = RequestData {
            method: "POST".into(),
            uri: "http://example.com/upload".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: RequestTags::new(),
        };
        let id = store.begin("test", &request, "request").unwrap();
        let writer = store
            .create_body_writer(id, BodySide::Request, false)
            .await
            .unwrap();
        let path = writer.path().to_path_buf();
        let tracker = TaskGroup::new();
        let deferred = DeferredBody::new(http_body_util::Empty::new(), writer, &tracker);
        let reader = deferred.reader();

        let body = tokio::task::spawn_blocking(move || reader.read(None))
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(body, DeferredBodyRead::Empty));

        let (replayed, done) = deferred.finish(true, None, None);
        let collected = replayed.unwrap().collect().await.unwrap();
        assert!(collected.to_bytes().is_empty());
        assert_eq!(done.await.unwrap().outcome, PumpOutcome::Complete);
        assert!(!path.exists());
        tracker.shutdown(Duration::from_secs(1)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deferred_body_replays_trailers_after_a_complete_read() {
        let root = tempdir().unwrap();
        let store = CaptureStore::open(1, root.path()).unwrap();
        let request = RequestData {
            method: "POST".into(),
            uri: "http://example.com/upload".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::new(),
            tags: RequestTags::new(),
        };
        let id = store.begin("test", &request, "request").unwrap();
        let writer = store
            .create_body_writer(id, BodySide::Request, false)
            .await
            .unwrap();
        let mut trailers = HeaderMap::new();
        trailers.insert("x-checksum", HeaderValue::from_static("complete"));
        let body = StreamBody::new(stream::iter([
            Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"payload"))),
            Ok(Frame::trailers(trailers)),
        ]));
        let tracker = TaskGroup::new();
        let deferred = DeferredBody::new(body, writer, &tracker);
        let reader = deferred.reader();

        let body = tokio::task::spawn_blocking(move || reader.read(None))
            .await
            .unwrap()
            .unwrap();
        let DeferredBodyRead::File(path) = body else {
            panic!("non-empty deferred body should be backed by a file");
        };
        assert_eq!(tokio::fs::read(path).await.unwrap(), b"payload");

        let (replayed, done) = deferred.finish(true, None, None);
        let collected = replayed.unwrap().collect().await.unwrap();
        assert_eq!(
            collected.trailers().unwrap().get("x-checksum").unwrap(),
            "complete"
        );
        assert_eq!(collected.to_bytes().as_ref(), b"payload");
        assert_eq!(done.await.unwrap().outcome, PumpOutcome::Complete);
        tracker.shutdown(Duration::from_secs(1)).await;
    }
}
