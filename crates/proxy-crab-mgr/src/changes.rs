//! Resource invalidations shared by desktop commands and HTTP/SSE clients.
use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use futures::{Stream, StreamExt, stream};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::{
    ProxyCrabManager,
    dto::{HttpApiChange, HttpApiResource},
};

pub fn stream(manager: Arc<dyn ProxyCrabManager>) -> impl Stream<Item = HttpApiChange> + Send {
    let receiver = manager.change_sender().subscribe();
    let proxy = manager.subscribe_proxy_status_changes();
    stream::unfold(
        (receiver, proxy, true),
        |(mut receiver, mut proxy, initial)| async move {
            let change = if initial {
                HttpApiChange {
                    resources: vec![HttpApiResource::All],
                    session_id: None,
                }
            } else {
                tokio::select! {
                    value = receiver.recv() => match value {
                        Ok(change) => change,
                        Err(broadcast::error::RecvError::Lagged(_)) => HttpApiChange {
                            resources: vec![HttpApiResource::All], session_id: None,
                        },
                        Err(broadcast::error::RecvError::Closed) => return None,
                    },
                    value = proxy.changed() => {
                        if value.is_err() { return None; }
                        HttpApiChange { resources: vec![HttpApiResource::Proxy], session_id: None }
                    }
                }
            };
            Some((change, (receiver, proxy, false)))
        },
    )
}

pub fn response(
    events: impl Stream<Item = Event> + Send + 'static,
    cancellation: CancellationToken,
) -> Response {
    Sse::new(
        events
            .take_until(cancellation.cancelled_owned())
            .map(Ok::<_, Infallible>),
    )
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MitmManager,
        dto::{CreateSessionRequest, ScriptRequest, UpdateScriptRequest},
    };
    use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};

    #[tokio::test]
    async fn reconnect_and_lag_resynchronize_and_manager_writes_publish_once() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let changes = stream(manager.clone());
        futures::pin_mut!(changes);
        assert_eq!(
            changes.next().await.unwrap().resources,
            vec![HttpApiResource::All]
        );
        manager
            .create_column_script(ScriptRequest {
                name: "status".into(),
                content: "return 'ok'".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            changes.next().await.unwrap(),
            HttpApiChange::column_script("status".into())
        );
        let mut receiver = manager.change_sender().subscribe();
        assert!(
            manager
                .update_column_script(
                    "missing".into(),
                    UpdateScriptRequest {
                        content: "return ''".into()
                    }
                )
                .await
                .is_err()
        );
        assert!(receiver.try_recv().is_err());
        let session = manager
            .create_session(CreateSessionRequest::default())
            .await
            .unwrap();
        assert_eq!(
            changes.next().await.unwrap().resources,
            vec![HttpApiResource::Sessions]
        );
        let reconnected = stream(manager.clone());
        futures::pin_mut!(reconnected);
        assert_eq!(
            reconnected.next().await.unwrap().resources,
            vec![HttpApiResource::All]
        );
        for _ in 0..256 {
            manager
                .change_sender()
                .send(HttpApiChange {
                    resources: vec![HttpApiResource::SessionView],
                    session_id: Some(session.id),
                })
                .unwrap();
        }
        assert_eq!(
            changes.next().await.unwrap().resources,
            vec![HttpApiResource::All]
        );
    }

    #[tokio::test]
    async fn proxy_changes_wake_streams_and_shutdown_closes_open_responses() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        let manager = MitmManager::new(runtime);
        let mut config = manager.config().await.unwrap();
        config.proxy_port = 0;
        manager.replace_config(config).await.unwrap();
        let events = stream(manager.clone());
        futures::pin_mut!(events);
        events.next().await.unwrap();
        manager.start_proxy().await.unwrap();
        let change = tokio::time::timeout(Duration::from_secs(1), events.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(change.resources, vec![HttpApiResource::Proxy]);
        manager.stop_proxy().await.unwrap();
        let cancellation = CancellationToken::new();
        let response = response(stream::pending(), cancellation.clone());
        cancellation.cancel();
        let bytes = tokio::time::timeout(
            Duration::from_secs(1),
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(bytes.is_empty());
    }
}
