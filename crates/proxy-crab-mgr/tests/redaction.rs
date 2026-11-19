use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use proxy_crab_mgr::{MitmManager, ProxyCrabManager, dto::*, session_share::SessionShareService};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::LogBuffer,
    model::*,
    storage::{BodySide, BodySourceData, CaptureStore},
};
use std::sync::Arc;
use tower::ServiceExt;

use test_support::ShareWithScript;

mod test_support {
    use super::*;
    use proxy_crab_mgr::session_share::{SessionShareConfig, SessionShareState};

    pub(super) trait ShareWithScript {
        async fn enable_with_script(
            &self,
            manager: &Arc<dyn ProxyCrabManager>,
            session_id: u64,
            redaction_script: Option<String>,
        ) -> Result<SessionShareState, ManagerError>;
    }

    impl ShareWithScript for SessionShareService {
        async fn enable_with_script(
            &self,
            manager: &Arc<dyn ProxyCrabManager>,
            session_id: u64,
            redaction_script: Option<String>,
        ) -> Result<SessionShareState, ManagerError> {
            self.enable_with_config(
                manager,
                session_id,
                Some(SessionShareConfig {
                    session_pages: None,
                    session_config_share: None,
                    redaction_script: redaction_script.map(Some),
                    persistent: false,
                }),
            )
            .await
        }
    }
}

const MASK: &str = r#"
assert(context.api ~= nil)
if context.req.header == 'loaded' then
  req.uri = 'https://example.com/safe?q=hidden'
  if req.headers then req.headers:set('authorization', 'hidden'); req.headers:remove('cookie') end
end
if resp and context.resp.header == 'loaded' then
  resp.headers:remove('set-cookie')
  resp.headers:set('location', 'https://example.com/safe')
end
if context.req.body == 'loaded' then req.body:replace_with_string('masked request') end
if resp and context.resp.body == 'loaded' then resp.body:replace_with_string('masked response') end
"#;

struct Fixture {
    _dir: tempfile::TempDir,
    manager: Arc<MitmManager>,
    store: CaptureStore,
    sid: u64,
    id: u64,
}
impl Fixture {
    async fn new(script: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let runtime = ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap();
        let session = runtime.create_session(NewSession::default()).unwrap();
        let store = CaptureStore::open(session.id, runtime.workspace().root()).unwrap();
        let request = RequestData {
            method: "GET".into(),
            uri: "https://example.com/secret?q=secret".into(),
            version: "HTTP/1.1".into(),
            headers: HeaderValues::from([
                ("authorization".into(), vec!["secret".into()]),
                ("cookie".into(), vec!["token=secret".into()]),
                ("content-type".into(), vec!["text/plain".into()]),
            ]),
            tags: Default::default(),
        };
        let id = store.begin("client", &request, "request").unwrap();
        store
            .save_body(id, BodySide::Request, b"secret request")
            .unwrap();
        store
            .save_body(id, BodySide::Response, b"secret response")
            .unwrap();
        store
            .complete(
                id,
                &ResponseData {
                    status: 200,
                    version: "HTTP/1.1".into(),
                    headers: HeaderValues::from([
                        ("set-cookie".into(), vec!["token=secret".into()]),
                        ("location".into(), vec!["https://example.com/secret".into()]),
                        ("content-type".into(), vec!["text/plain".into()]),
                    ]),
                },
                &BodySourceType::Original,
            )
            .unwrap();
        let manager = MitmManager::new(runtime);
        manager
            .create_redaction_script(ScriptRequest {
                name: "mask".into(),
                content: script.into(),
            })
            .await
            .unwrap();
        Self {
            _dir: dir,
            manager,
            store,
            sid: session.id,
            id,
        }
    }
    fn ids(&self, option: FilterOption, input: &str) -> LogIdsRequest {
        LogIdsRequest {
            session_id: Some(self.sid),
            filter: Some(
                SessionFilter {
                    option: Some(option),
                    input: input.into(),
                }
                .into(),
            ),
            ids: None,
            min_id: None,
            max_id: None,
            limit: None,
        }
    }
}
fn column(column: FilterColumn) -> FilterOption {
    FilterOption::Column {
        column,
        regex: false,
    }
}

#[tokio::test]
async fn temporary_filter_uses_redacted_headers_and_bodies() {
    let f = Fixture::new(MASK).await;
    let reader = f.manager.reader(Some("mask"), "/api/logs/ids").unwrap();
    let query = |content: &str| LogIdsRequest {
        session_id: Some(f.sid),
        filter: Some(LogFilter {
            option: Some(LogFilterOption::ScriptContent {
                content: content.into(),
            }),
            input: String::new(),
        }),
        ids: None,
        min_id: None,
        max_id: None,
        limit: None,
    };
    let result = reader.log_ids(query(
        "return entry.req.uri.path == '/safe' and entry.req.body:as_string() == 'masked request' and entry.resp.body:as_string() == 'masked response'"
    )).await.unwrap();
    assert_eq!(result.matched_ids, vec![f.id]);
    assert!(reader.log_ids(query(
        "return entry.req.uri.path == '/secret' or entry.req.body:as_string() == 'secret request'"
    )).await.unwrap().matched_ids.is_empty());
    let error = reader
        .log_ids(query("error(entry.req.body:as_string())"))
        .await
        .unwrap_err();
    assert!(error.message.contains("masked request"));
    assert!(!error.message.contains("secret request"));
    assert!(
        f.manager
            .log(Some(f.sid), f.id)
            .await
            .unwrap()
            .request
            .uri
            .contains("secret")
    );
}

#[tokio::test]
async fn shared_inline_filter_is_scoped_and_redacted() {
    let f = Fixture::new(MASK).await;
    let manager: Arc<dyn ProxyCrabManager> = f.manager.clone();
    let shares = SessionShareService::new();
    let share = shares
        .enable_with_script(&manager, f.sid, Some("mask".into()))
        .await
        .unwrap();
    let app = proxy_crab_mgr::session_share::router(manager, shares);
    let token = share.token.unwrap();
    for (source, expected) in [
        (
            "return entry.req.uri.path == '/safe' and entry.req.body:as_string() == 'masked request'",
            vec![f.id],
        ),
        (
            "return entry.req.uri.path == '/secret' or entry.req.body:as_string() == 'secret request'",
            vec![],
        ),
    ] {
        let response = app.clone().oneshot(Request::builder().method("POST")
            .uri(format!("/share-api/logs/ids?token={token}"))
            .header("content-type", "application/json")
            .body(Body::from(serde_json::json!({
                "session_id": 999, "ids": [f.id],
                "filter": {"option": {"kind": "script_content", "content": source}, "input": ""}
            }).to_string())).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(body["data"]["matched_ids"], serde_json::json!(expected));
    }
}

#[tokio::test]
async fn inline_view_columns_use_redaction_and_shared_session_scope() {
    let f = Fixture::new(MASK).await;
    let columns = vec![
        LogViewColumn::ScriptContent {
            width: 160.0, name: "details".into(),
            content: "return entry.req.uri.path .. ':' .. entry.req.body:as_string() .. ':' .. entry.resp.body:as_string()".into(),
        },
        LogViewColumn::ScriptContent {
            width: 100.0, name: "details".into(),
            content: "error(entry.req.body:as_string())".into(),
        },
    ];
    let mut request = LogViewsRequest {
        session_id: Some(f.sid),
        logs: vec![LogViewItem {
            id: f.id,
            updated_at: Some(u64::MAX),
        }],
        view: Some(LogViewInput {
            columns: columns.clone(),
        }),
    };
    let verify = |result: LogViewsPayload| {
        assert_eq!(result.columns, columns);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            result.rows[0].cells,
            vec!["/safe:masked request:masked response", ""]
        );
        assert_eq!(result.exceptions.len(), 1);
        assert_eq!(result.exceptions[0].column_index, Some(1));
        assert!(result.exceptions[0].message.contains("details"));
        assert!(result.exceptions[0].message.contains("masked request"));
        assert!(!result.exceptions[0].message.contains("secret"));
    };
    let reader = f.manager.reader(Some("mask"), "/api/logs/views").unwrap();
    verify(reader.log_views(request.clone()).await.unwrap());
    let manager: Arc<dyn ProxyCrabManager> = f.manager.clone();
    let shares = SessionShareService::new();
    let share = shares
        .enable_with_script(&manager, f.sid, Some("mask".into()))
        .await
        .unwrap();
    let app = proxy_crab_mgr::session_share::router(manager, shares);
    request.session_id = Some(999);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!(
                    "/share-api/logs/views?token={}",
                    share.token.unwrap()
                ))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&request).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    verify(serde_json::from_value(body["data"].clone()).unwrap());
    assert!(f.manager.column_scripts().await.unwrap().is_empty());
    assert!(
        f.manager
            .log(Some(f.sid), f.id)
            .await
            .unwrap()
            .request
            .uri
            .contains("secret")
    );
}

#[tokio::test]
async fn redaction_covers_detail_body_columns_and_filters_without_changing_originals() {
    let f = Fixture::new(MASK).await;
    let reader = f.manager.reader(Some("mask"), "/api/logs/{id}").unwrap();
    let detail = reader.log(Some(f.sid), f.id).await.unwrap();
    assert!(detail.request.uri.contains("safe"));
    assert!(
        matches!(detail.request.body, BodyPayload::Text { content, .. } if content == "masked request")
    );
    let body = reader
        .log_body_source(Some(f.sid), f.id, BodySide::Response)
        .await
        .unwrap();
    assert!(matches!(body.data, BodySourceData::Bytes(bytes) if bytes == b"masked response"));
    let raw = f.manager.log(Some(f.sid), f.id).await.unwrap();
    assert!(raw.request.uri.contains("secret"));
    assert!(
        matches!(raw.request.body, BodyPayload::Text { content, .. } if content == "secret request")
    );
    f.manager
        .create_column_script(ScriptRequest {
            name: "body".into(),
            content:
                "return entry.req.headers:get('authorization') .. ':' .. entry.req.body:as_string()"
                    .into(),
        })
        .await
        .unwrap();
    let views = reader
        .log_views(LogViewsRequest {
            session_id: Some(f.sid),
            logs: vec![LogViewItem {
                id: f.id,
                updated_at: Some(u64::MAX),
            }],
            view: Some(LogViewInput {
                columns: vec![LogViewColumn::Script {
                    script_name: "body".into(),
                    width: 100.0,
                }],
            }),
        })
        .await
        .unwrap();
    assert_eq!(views.rows[0].cells, ["hidden:masked request"]);
    let ids = reader
        .log_ids(f.ids(column(FilterColumn::Uri), "safe"))
        .await
        .unwrap();
    assert_eq!(ids.matched_ids, [f.id]);
    let ids = reader
        .log_ids(f.ids(
            column(FilterColumn::Script {
                script_name: "body".into(),
            }),
            "masked request",
        ))
        .await
        .unwrap();
    assert_eq!(ids.matched_ids, [f.id]);
    f.manager
        .create_filter_script(ScriptRequest {
            name: "body".into(),
            content: "return entry.resp.body:as_string() == 'masked response'".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        reader
            .log_ids(f.ids(
                FilterOption::Script {
                    script_name: "body".into()
                },
                "x"
            ))
            .await
            .unwrap()
            .matched_ids,
        [f.id]
    );
}

#[tokio::test]
async fn redaction_failures_are_not_swallowed_by_filter_or_column_evaluation() {
    let f =
        Fixture::new("if context.req.body == 'loaded' then error('secret must not escape') end")
            .await;
    let reader = f.manager.reader(Some("mask"), "/api/logs/ids").unwrap();
    f.manager
        .create_filter_script(ScriptRequest {
            name: "catch".into(),
            content: "pcall(function() entry.req.body:as_string() end); return true".into(),
        })
        .await
        .unwrap();
    let error = reader
        .log_ids(f.ids(
            FilterOption::Script {
                script_name: "catch".into(),
            },
            "x",
        ))
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RedactionFailed);
    assert!(!error.message.contains("secret"));
    f.manager
        .create_column_script(ScriptRequest {
            name: "catch".into(),
            content: "pcall(function() entry.req.body:as_string() end); return 'ignored'".into(),
        })
        .await
        .unwrap();
    assert!(
        reader
            .log_views(LogViewsRequest {
                session_id: Some(f.sid),
                logs: vec![LogViewItem {
                    id: f.id,
                    updated_at: None
                }],
                view: Some(LogViewInput {
                    columns: vec![LogViewColumn::Script {
                        script_name: "catch".into(),
                        width: 100.0
                    }]
                })
            })
            .await
            .is_err()
    );
}

#[tokio::test]
async fn redaction_builtin_fast_paths_and_uri_only_environment() {
    let f = Fixture::new("assert(req.headers == nil and req.body == nil and resp == nil); req.uri = 'https://example.com/masked'").await;
    let reader = f.manager.reader(Some("mask"), "/api/logs/ids").unwrap();
    assert_eq!(
        reader
            .log_ids(f.ids(column(FilterColumn::Uri), "masked"))
            .await
            .unwrap()
            .matched_ids,
        [f.id]
    );
    f.manager
        .update_redaction_script(
            "mask".into(),
            UpdateScriptRequest {
                content: "error('must not execute')".into(),
            },
        )
        .await
        .unwrap();
    let reader = f.manager.reader(Some("mask"), "/api/logs/ids").unwrap();
    assert_eq!(
        reader
            .log_ids(f.ids(column(FilterColumn::Method), "GET"))
            .await
            .unwrap()
            .matched_ids,
        [f.id]
    );
    assert!(
        reader
            .log_ids(f.ids(column(FilterColumn::Uri), "secret"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn binary_redaction_stream_lifetime_and_snapshot_do_not_retain_original_content() {
    use tokio::io::AsyncReadExt;
    let f = Fixture::new(
        "if req.body then req.body:replace_with_bytes(bytes.view(string.char(0,255))) end",
    )
    .await;
    let reader = f
        .manager
        .reader(Some("mask"), "/api/logs/{id}/body")
        .unwrap();
    let source = reader
        .log_body_source(Some(f.sid), f.id, BodySide::Request)
        .await
        .unwrap();
    let BodySourceData::TemporaryFile(path) = &source.data else {
        panic!("binary redaction output must remain temporary");
    };
    let path = path.to_path_buf();
    let mut stream = proxy_crab_mgr::http::body_reader(&source, false)
        .await
        .unwrap();
    drop(source);
    assert!(path.exists());
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, [0, 255]);
    drop(stream);
    assert!(!path.exists());

    let snapshot = RequestInterceptorSnapshot {
        method: "GET".into(),
        uri: "https://example.com/".into(),
        version: "HTTP/1.1".into(),
        headers: HeaderValues::from([("content-type".into(), vec!["text/plain".into()])]),
        body: BodySourceType::String {
            content: "original inline secret".into(),
        },
    };
    let run = InterceptorRun {
        origin: InterceptorExecutionOrigin::Saved,
        completed: true,
        phase: InterceptorKind::Request,
        position: 0,
        name: "historical".into(),
        script_hash: script_content_hash("-- original"),
        content: "-- original".into(),
        error: None,
        modifications: vec![Modification::Snapshot {
            request: Some(snapshot),
            response: None,
        }],
    };
    let execution = f.store.begin_interceptor_run(f.id, &run).unwrap();
    let redacted = reader
        .interceptor_snapshot(Some(f.sid), f.id, execution)
        .await
        .unwrap();
    assert_eq!(redacted.request.unwrap().body, BodySourceType::Original);
    let body = reader
        .interceptor_snapshot_body_source(Some(f.sid), f.id, execution, BodySide::Request)
        .await
        .unwrap();
    assert_eq!(body.data.read_all().unwrap(), [0, 255]);
    let original = f
        .manager
        .interceptor_snapshot(Some(f.sid), f.id, execution)
        .await
        .unwrap();
    assert!(
        matches!(original.request.unwrap().body, BodySourceType::String { content } if content == "original inline secret")
    );
}

#[tokio::test]
async fn redaction_shared_snapshot_and_body_are_scoped_and_transformed() {
    let f = Fixture::new(MASK).await;
    let snapshot = RequestInterceptorSnapshot {
        method: "GET".into(),
        uri: "https://example.com/old-secret".into(),
        version: "HTTP/1.1".into(),
        headers: HeaderValues::from([
            ("authorization".into(), vec!["old-secret".into()]),
            ("content-type".into(), vec!["text/plain".into()]),
        ]),
        body: BodySourceType::String {
            content: "old secret".into(),
        },
    };
    let run = InterceptorRun {
        origin: InterceptorExecutionOrigin::Saved,
        completed: true,
        phase: InterceptorKind::Request,
        position: 0,
        name: "historical".into(),
        script_hash: script_content_hash("-- original"),
        content: "-- original".into(),
        modifications: vec![
            Modification::Snapshot {
                request: Some(snapshot),
                response: None,
            },
            Modification::HeaderSet {
                name: "authorization".into(),
                value: "historical secret stays out of scope".into(),
            },
        ],
        error: None,
    };
    let execution = f.store.begin_interceptor_run(f.id, &run).unwrap();
    let manager: Arc<dyn ProxyCrabManager> = f.manager.clone();
    let shares = SessionShareService::new();
    let share = shares
        .enable_with_script(&manager, f.sid, Some("mask".into()))
        .await
        .unwrap();
    let app = proxy_crab_mgr::session_share::router(manager, shares);
    let token = share.token.unwrap();
    for suffix in [
        format!("interceptors/{execution}/snapshot"),
        format!("body?side=request&execution_id={execution}&session_id=999"),
    ] {
        let sep = if suffix.contains('?') { '&' } else { '?' };
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/share-api/logs/{}/{suffix}{sep}token={token}",
                        f.id
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(!text.contains("old-secret") && !text.contains("old secret"));
        assert!(text.contains("masked request"));
    }
    let raw = f
        .manager
        .interceptor_snapshot(Some(f.sid), f.id, execution)
        .await
        .unwrap();
    assert_eq!(raw.request.unwrap().uri, "https://example.com/old-secret");
}

#[tokio::test]
async fn forced_redaction_overrides_missing_and_unbound_page_policies() {
    let f = Fixture::new(MASK).await;
    for binding in [None, Some("mask".to_string())] {
        let original: Arc<dyn ProxyCrabManager> = f.manager.clone();
        let pages = SessionShareService::new();
        let page = pages
            .enable_with_script(&original, f.sid, binding.clone())
            .await
            .unwrap();
        let forced: Arc<dyn ProxyCrabManager> = f
            .manager
            .with_forced_redaction(Script {
                name: "startup.lua".into(),
                content: MASK.into(),
            })
            .unwrap();
        // Even a binding whose library script no longer exists cannot defeat startup policy.
        if page.redaction_script.is_some() {
            f.manager
                .delete_redaction_script("mask".into())
                .await
                .unwrap();
        }
        for (app, uri) in [(
            proxy_crab_mgr::session_share::router(forced.clone(), pages),
            format!("/share-api/logs/{}?token={}", f.id, page.token.unwrap()),
        )] {
            let response = app
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let text = String::from_utf8(
                to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(text.contains("masked response"));
            assert!(!text.contains("secret"));
        }
    }
    let forced = f
        .manager
        .with_forced_redaction(Script {
            name: "broken.lua".into(),
            content: "error('secret')".into(),
        })
        .unwrap();
    let error = forced
        .reader(None, "/api/logs/{id}")
        .unwrap()
        .log(Some(f.sid), f.id)
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RedactionFailed);
    assert!(!error.message.contains("secret"));
    assert!(
        f.manager
            .log(Some(f.sid), f.id)
            .await
            .unwrap()
            .request
            .uri
            .contains("secret")
    );
}
