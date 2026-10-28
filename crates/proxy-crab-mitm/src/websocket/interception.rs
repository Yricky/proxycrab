//! Connection-scoped FIFO interception. Only finalized messages are visible to Lua history.
use super::{
    MAGIC, MessageRef, WebSocketMessage, WsCallbackError, WsInterceptor, WsInvocation, WsProducer,
    transport,
};
use crate::{
    lua::websocket::{WsBody, WsCallback, WsHistory, WsMessageInput},
    storage::{BodySide, CaptureStore},
    workspace::now_millis,
};
use anyhow::{Result, anyhow};
use std::{
    collections::VecDeque,
    fs::{File, OpenOptions},
    io::Write,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

pub(crate) struct RegisteredCallback {
    pub source: WsInterceptor,
    pub callback: WsCallback,
}
struct Pending {
    side: BodySide,
    kind: String,
    body: Arc<[u8]>,
    started_at: u64,
    next: usize,
    root: Option<MessageRef>,
    parent: Option<MessageRef>,
    producer: Option<WsProducer>,
}
struct Processed {
    side: BodySide,
    meta: WebSocketMessage,
    body: Arc<[u8]>,
}
struct Chain {
    callbacks: Vec<RegisteredCallback>,
    queue: VecDeque<Pending>,
    next_id: u32,
    invocation: u64,
    store: CaptureStore,
    capture_id: u64,
    files: [File; 2],
}
fn side_index(side: BodySide) -> usize {
    if side == BodySide::Request { 0 } else { 1 }
}
impl Chain {
    fn new(
        callbacks: Vec<RegisteredCallback>,
        store: CaptureStore,
        capture_id: u64,
    ) -> Result<Self> {
        let open = |side| -> Result<File> {
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(store.websocket_body_path(capture_id, side))?;
            file.write_all(MAGIC)?;
            Ok(file)
        };
        let files = [open(BodySide::Request)?, open(BodySide::Response)?];
        Ok(Self {
            callbacks,
            queue: VecDeque::new(),
            next_id: 1,
            invocation: 0,
            store,
            capture_id,
            files,
        })
    }
    fn history(&self, before: u32) -> WsHistory {
        let store = self.store.clone();
        let capture_id = self.capture_id;
        Arc::new(move |after, limit| {
            let after = u32::try_from(after).unwrap_or(u32::MAX);
            let mut entries = Vec::new();
            for side in [BodySide::Request, BodySide::Response] {
                let page = store
                    .websocket_messages(capture_id, side, after, limit)?
                    .ok_or_else(|| anyhow!("WebSocket capture missing"))?;
                for meta in page.messages.into_iter().filter(|m| m.id < before) {
                    entries.push((side, meta));
                }
            }
            entries.sort_by_key(|(_, m)| m.id);
            entries.truncate(limit);
            entries
                .into_iter()
                .map(|(side, metadata)| {
                    let capture = store.clone();
                    let message_id = metadata.id;
                    let body = WsBody::lazy(move || {
                        capture.websocket_message_body(capture_id, side, message_id)
                    });
                    Ok(WsMessageInput {
                        side,
                        metadata,
                        body,
                    })
                })
                .collect()
        })
    }
    fn allocate(&mut self, pending: &Pending) -> Result<WebSocketMessage> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| anyhow!("WebSocket message id exhausted"))?;
        Ok(WebSocketMessage {
            id,
            kind: pending.kind.clone(),
            started_at: pending.started_at,
            completed_at: now_millis(),
            size: pending.body.len() as u64,
            content_available: true,
            omitted_reason: None,
            sequence: Some(id as u64),
            root: Some(pending.root.clone().unwrap_or(MessageRef {
                side: pending.side,
                id,
            })),
            parent: pending.parent.clone(),
            producer: pending.producer.clone(),
            dropped_by: None,
            status: None,
            error: None,
            errors: vec![],
        })
    }
    fn process(
        &mut self,
        original: Option<(BodySide, transport::Message)>,
    ) -> Result<Option<Processed>> {
        let pending = match original {
            Some((side, message)) => Pending {
                side,
                kind: if message.opcode == 1 {
                    "text"
                } else {
                    "binary"
                }
                .into(),
                body: message.payload.into(),
                started_at: message.started_at,
                next: 0,
                root: None,
                parent: None,
                producer: None,
            },
            None => match self.queue.pop_front() {
                Some(p) => p,
                None => return Ok(None),
            },
        };
        let mut meta = self.allocate(&pending)?;
        let history = self.history(meta.id);
        for position in pending.next..self.callbacks.len() {
            self.invocation = self
                .invocation
                .checked_add(1)
                .ok_or_else(|| anyhow!("WebSocket invocation id exhausted"))?;
            let registered = &self.callbacks[position];
            let source = &registered.source;
            let invocation_id = self.invocation;
            match registered.callback.call(
                WsMessageInput {
                    side: pending.side,
                    metadata: meta.clone(),
                    body: pending.body.clone().into(),
                },
                history.clone(),
                self.queue.len(),
            ) {
                Ok(effects) => {
                    for (send_index, send) in effects.sends.into_iter().enumerate() {
                        self.queue.push_back(Pending {
                            side: send.side,
                            kind: send.kind,
                            body: send.body.into(),
                            started_at: now_millis(),
                            next: position + 1,
                            root: meta.root.clone(),
                            parent: Some(MessageRef {
                                side: pending.side,
                                id: meta.id,
                            }),
                            producer: Some(WsProducer {
                                execution_id: source.execution_id,
                                name: source.name.clone(),
                                invocation_id,
                                send_index: send_index + 1,
                            }),
                        });
                    }
                    if effects.dropped {
                        meta.status = Some("dropped".into());
                        meta.dropped_by = Some(WsInvocation {
                            execution_id: source.execution_id,
                            name: source.name.clone(),
                            invocation_id,
                        });
                        break;
                    }
                }
                Err(error) => meta.errors.push(WsCallbackError {
                    execution_id: source.execution_id,
                    name: source.name.clone(),
                    invocation_id,
                    error,
                }),
            }
        }
        Ok(Some(Processed {
            side: pending.side,
            meta,
            body: pending.body,
        }))
    }
    fn persist(&mut self, mut message: Processed) -> Result<()> {
        message.meta.completed_at = now_millis();
        let json = serde_json::to_vec(&message.meta)?;
        if json.len() > super::MAX_MESSAGE {
            return Err(anyhow!("WebSocket metadata exceeds size limit"));
        }
        let len = u32::try_from(4 + json.len() + message.body.len())?;
        let file = &mut self.files[side_index(message.side)];
        file.write_all(&message.meta.id.to_le_bytes())?;
        file.write_all(&len.to_le_bytes())?;
        file.write_all(&(json.len() as u32).to_le_bytes())?;
        file.write_all(&json)?;
        file.write_all(&message.body)?;
        file.flush()?;
        Ok(())
    }
    fn persist_unsent(
        &mut self,
        side: BodySide,
        message: transport::Message,
        reason: &str,
    ) -> Result<()> {
        if message.opcode >= 8 {
            return Ok(());
        }
        let pending = Pending {
            side,
            kind: if message.opcode == 1 {
                "text"
            } else {
                "binary"
            }
            .into(),
            body: message.payload.into(),
            started_at: message.started_at,
            next: 0,
            root: None,
            parent: None,
            producer: None,
        };
        let mut meta = self.allocate(&pending)?;
        meta.status = Some("unsent".into());
        meta.error = Some(reason.into());
        self.persist(Processed {
            side,
            meta,
            body: pending.body,
        })
    }

    fn fail_pending(&mut self, reason: &str) -> Result<()> {
        while let Some(pending) = self.queue.pop_front() {
            let mut meta = self.allocate(&pending)?;
            meta.status = Some("unsent".into());
            meta.error = Some(reason.into());
            self.persist(Processed {
                side: pending.side,
                meta,
                body: pending.body,
            })?;
        }
        Ok(())
    }
}
async fn blocking<T: Send + 'static>(
    chain: &Arc<Mutex<Chain>>,
    f: impl FnOnce(&mut Chain) -> Result<T> + Send + 'static,
) -> Result<T> {
    let chain = chain.clone();
    tokio::task::spawn_blocking(move || {
        f(&mut *chain
            .lock()
            .map_err(|_| anyhow!("WebSocket chain lock poisoned"))?)
    })
    .await?
}

pub(crate) async fn forward<A, B>(
    downstream: A,
    upstream: B,
    store: CaptureStore,
    id: u64,
    extensions: &str,
    cancellation: CancellationToken,
    callbacks: Vec<RegisteredCallback>,
) -> Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (dr, mut dw) = tokio::io::split(downstream);
    let (ur, mut uw) = tokio::io::split(upstream);
    let mut downstream = transport::Reader::new(dr, extensions, BodySide::Request)?;
    let mut upstream = transport::Reader::new(ur, extensions, BodySide::Response)?;
    let capture = store.clone();
    let chain = Arc::new(Mutex::new(
        tokio::task::spawn_blocking(move || Chain::new(callbacks, capture, id)).await??,
    ));
    // All terminal paths converge on cleanup, including readers with additional complete messages.
    let result: Result<()> = async {
        loop {
            let (side, message) = tokio::select! {
                _ = cancellation.cancelled() => return Err(anyhow!("WebSocket proxy connection cancelled")),
                message = downstream.next() => (BodySide::Request,message?),
                message = upstream.next() => (BodySide::Response,message?),
            };
            let Some(message) = message else {return Ok(());};
            if message.opcode >= 8 {
                let closed = message.opcode == 8;
                if closed {
                    let code = (message.payload.len() >= 2).then(||u16::from_be_bytes([message.payload[0],message.payload[1]]));
                    let reason = if message.payload.len() > 2 {std::str::from_utf8(&message.payload[2..])?} else {""};
                    let close_store = store.clone();
                    let reason = reason.to_owned();
                    tokio::task::spawn_blocking(move ||close_store.websocket_close_frame(id,code,&reason)).await??;
                }
                let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
                tokio::select! {
                    _ = cancellation.cancelled() => return Err(anyhow!("WebSocket proxy connection cancelled")),
                    _ = tokio::time::sleep_until(deadline) => return Err(anyhow!("WebSocket control forwarding timed out")),
                    result = async {
                        if side==BodySide::Request {transport::write(&mut uw,message.opcode,&message.payload,true).await}
                        else {transport::write(&mut dw,message.opcode,&message.payload,false).await}
                    } => result?,
                }
                if closed {
                    // Keep reading until the peer actually acknowledges Close. Data received
                    // meanwhile is recorded, but cannot be sent after our Close frame.
                    loop {
                        let response = tokio::select! {
                            _ = cancellation.cancelled()=>return Err(anyhow!("WebSocket proxy connection cancelled")),
                            _ = tokio::time::sleep_until(deadline) => return Err(anyhow!("WebSocket close handshake timed out")),
                            response = async {if side==BodySide::Request {upstream.next().await} else {downstream.next().await}} => response?,
                        };
                        let Some(response) = response else {return Ok(());};
                        if response.opcode < 8 {
                            let peer_side = if side==BodySide::Request {BodySide::Response} else {BodySide::Request};
                            blocking(&chain, move |chain|chain.persist_unsent(peer_side,response,"WebSocket close handshake started")).await?;
                            continue;
                        }
                        let acknowledged = response.opcode == 8;
                        tokio::select! {
                            _ = cancellation.cancelled()=>return Err(anyhow!("WebSocket proxy connection cancelled")),
                            _ = tokio::time::sleep_until(deadline) => return Err(anyhow!("WebSocket close handshake timed out")),
                            result = async {
                                if side==BodySide::Request {transport::write(&mut dw,response.opcode,&response.payload,false).await}
                                else {transport::write(&mut uw,response.opcode,&response.payload,true).await}
                            } => result?,
                        }
                        if acknowledged {return Ok(());}
                    }
                }
                continue;
            }
            let mut original = Some((side,message));
            loop {
                if original.is_none() && cancellation.is_cancelled() {
                    return Err(anyhow!("WebSocket proxy connection cancelled"));
                }
                let root = original.take();
                let Some(mut message) = blocking(&chain,move |chain| chain.process(root)).await? else {break;};
                let failure = if message.meta.status.as_deref()==Some("dropped") { None } else {
                    let result = tokio::select! {
                        _=cancellation.cancelled()=>Err(anyhow!("WebSocket proxy connection cancelled")),
                        result=async {
                            let opcode=if message.meta.kind=="text" {1} else {2};
                            if message.side==BodySide::Request {transport::write(&mut uw,opcode,&message.body,true).await}
                            else {transport::write(&mut dw,opcode,&message.body,false).await}
                        }=>result,
                    };
                    match result {
                        Ok(())=>{message.meta.status=Some("sent".into());None},
                        Err(error)=>{message.meta.status=Some(if cancellation.is_cancelled(){"unsent"}else{"failed"}.into());message.meta.error=Some(error.to_string());Some(error.to_string())},
                    }
                };
                blocking(&chain,move |chain|chain.persist(message)).await?;
                if let Some(error)=failure { return Err(anyhow!(error)); }
            }
        }
    }.await;
    let reason = match &result {
        Ok(()) => "WebSocket connection closed".into(),
        Err(error) => error.to_string(),
    };
    let downstream = downstream.take_buffered();
    let upstream = upstream.take_buffered();
    blocking(&chain, move |chain| {
        chain.fail_pending(&reason)?;
        for (side, messages) in [
            (BodySide::Request, downstream),
            (BodySide::Response, upstream),
        ] {
            for message in messages {
                chain.persist_unsent(side, message, &reason)?;
            }
        }
        Ok(())
    })
    .await?;
    result
}

#[cfg(test)]
#[path = "chain_tests.rs"]
mod chain_tests;
