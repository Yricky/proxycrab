use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use futures::TryStreamExt;
use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager,
    dto::*,
    har_share::{EnableHarShareRequest, HarShareScope, HarShareService},
    session_share::SessionShareService,
};
use proxy_crab_mitm::{
    ProxyCrab,
    log_buffer::LogBuffer,
    model::*,
    storage::{BodySide, BodySourceData, CaptureStore},
};
use std::sync::Arc;
use tower::ServiceExt;

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
            filter: Some(SessionFilter {
                option: Some(option),
                input: input.into(),
            }),
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
            view: Some(SessionViewInput {
                columns: vec![Column::Script {
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
                view: Some(SessionViewInput {
                    columns: vec![Column::Script {
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
async fn redaction_har_download_transforms_derived_fields_and_pins_script() {
    let f = Fixture::new(MASK).await;
    let manager: Arc<dyn ProxyCrabManager> = f.manager.clone();
    let shares = HarShareService::new();
    let share = shares
        .enable(
            &manager,
            EnableHarShareRequest {
                redaction_script: Some("mask".into()),
                session_id: f.sid,
                scope: HarShareScope::All,
                log_ids: vec![f.id],
            },
        )
        .await
        .unwrap();
    let app = proxy_crab_mgr::har_share::router(manager, shares);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/session.har?token={}", share.token.unwrap()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // The download keeps the version resolved before streaming starts.
    f.manager
        .update_redaction_script(
            "mask".into(),
            UpdateScriptRequest {
                content: "error('changed')".into(),
            },
        )
        .await
        .unwrap();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!text.contains("secret"));
    let har: serde_json::Value = serde_json::from_str(&text).unwrap();
    let entry = &har["log"]["entries"][0];
    assert_eq!(entry["request"]["queryString"][0]["value"], "hidden");
    assert_eq!(entry["request"]["cookies"], serde_json::json!([]));
    assert_eq!(entry["response"]["redirectURL"], "https://example.com/safe");
    assert_eq!(entry["response"]["content"]["text"], "masked response");
}

#[tokio::test]
async fn redaction_har_stops_after_error_and_never_emits_original_entry() {
    let f = Fixture::new("error('secret')").await;
    let reader = f.manager.reader(Some("mask"), "/session.har").unwrap();
    let mut output = reader
        .export_logs(ExportLogsRequest {
            format: "har".into(),
            session_id: Some(f.sid),
            log_ids: Some(vec![f.id]),
        })
        .await
        .unwrap()
        .body;
    assert!(output.try_next().await.unwrap().is_some());
    assert_eq!(
        output.try_next().await.unwrap_err().code,
        ErrorCode::RedactionFailed
    );
    assert!(output.try_next().await.unwrap().is_none());
}

#[tokio::test]
async fn forced_redaction_overrides_missing_and_unbound_page_and_har_policies() {
    let f = Fixture::new(MASK).await;
    for binding in [None, Some("mask".to_string())] {
        let original: Arc<dyn ProxyCrabManager> = f.manager.clone();
        let pages = SessionShareService::new();
        let page = pages
            .enable_with_script(&original, f.sid, binding.clone())
            .await
            .unwrap();
        let hars = HarShareService::new();
        let har = hars
            .enable(
                &original,
                EnableHarShareRequest {
                    session_id: f.sid,
                    scope: HarShareScope::All,
                    log_ids: vec![f.id],
                    redaction_script: binding,
                },
            )
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
        for (app, uri) in [
            (
                proxy_crab_mgr::session_share::router(forced.clone(), pages),
                format!("/share-api/logs/{}?token={}", f.id, page.token.unwrap()),
            ),
            (
                proxy_crab_mgr::har_share::router(forced, hars),
                format!("/session.har?token={}", har.token.unwrap()),
            ),
        ] {
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
