use axum::{
    body::{Body, to_bytes},
    http::{
        Request, StatusCode,
        header::{COOKIE, SET_COOKIE},
    },
};
use proxy_crab_mgr::{
    MitmManager, ProxyCrabManager,
    dto::{CreateSessionRequest, ErrorCode},
    session_page::PageStore,
    session_share::{SessionShareConfig, SessionShareService},
};
use proxy_crab_mitm::{ProxyCrab, log_buffer::LogBuffer};
use std::{
    io::{Cursor, Write},
    sync::Arc,
};
use tower::ServiceExt;
use zip::{ZipWriter, write::SimpleFileOptions};

fn package(id: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in [
        (
            "manifest.json",
            serde_json::json!({"version":1,"id":id,"name":id,"backend_version":1})
                .to_string()
                .into_bytes(),
        ),
        (
            "index.html",
            b"<!doctype html><script type=module>import {backend} from './init.js'</script>"
                .to_vec(),
        ),
        ("assets/app.css", b"body { color: red }".to_vec()),
    ]
    .into_iter()
    .chain(extra.iter().map(|(n, b)| (*n, b.to_vec())))
    {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn manager(root: &std::path::Path) -> Arc<dyn ProxyCrabManager> {
    MitmManager::new(ProxyCrab::open(root, Arc::new(LogBuffer::default())).unwrap())
}
#[test]
fn packages_validate_before_replacing_and_remain_one_file() {
    let root = tempfile::tempdir().unwrap();
    let store = PageStore::new(root.path());
    let original = package("demo", &[]);
    store.import(original.clone(), false).unwrap();
    assert_eq!(
        std::fs::read_dir(root.path().join("session-pages"))
            .unwrap()
            .count(),
        1
    );
    assert_eq!(store.export("demo").unwrap(), original);
    assert_eq!(
        store.import(package("demo", &[]), false).unwrap_err().code,
        ErrorCode::Conflict
    );
    for name in [
        "../outside",
        "assets/../outside",
        "bad\\name",
        "/absolute",
        "init.js",
    ] {
        assert!(
            store
                .import(package("demo", &[(name, b"bad")]), true)
                .is_err(),
            "{name}"
        );
        assert_eq!(store.export("demo").unwrap(), original);
    }
    for id in ["../escape", "Uppercase", "two/parts", ""] {
        assert!(store.import(package(id, &[]), true).is_err());
    }
    assert!(store.file("demo", "../manifest.json").is_err());
    let replacement = package("demo", &[("extra.txt", b"replacement")]);
    store.import(replacement.clone(), true).unwrap();
    assert_eq!(store.export("demo").unwrap(), replacement);
    store.delete("demo").unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[tokio::test]
async fn package_cookies_are_scoped_revocable_and_work_with_concurrent_session_tokens() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    manager
        .import_session_page(package("demo", &[]), false)
        .await
        .unwrap();
    manager
        .import_session_page(package("private", &[]), false)
        .await
        .unwrap();
    let first = manager
        .create_session(CreateSessionRequest::default())
        .await
        .unwrap();
    let second = manager
        .create_session(CreateSessionRequest::default())
        .await
        .unwrap();
    let shares = SessionShareService::open(root.path()).unwrap();
    let config = || {
        Some(SessionShareConfig {
            session_pages: Some(vec!["demo".into()]),
            persistent: true,
            ..Default::default()
        })
    };
    let token = shares
        .enable_with_config(&manager, first.id, config())
        .await
        .unwrap()
        .token
        .unwrap();
    let token2 = shares
        .enable_with_config(&manager, second.id, config())
        .await
        .unwrap()
        .token
        .unwrap();
    let app = proxy_crab_mgr::session_share::router(manager.clone(), shares.clone());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/session-pages/demo/?token={token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let cookie = response.headers()[SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/session-pages/demo/?token={token2}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie2 = response.headers()[SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_ne!(cookie.split('=').next(), cookie2.split('=').next());
    let resource = |path: &str, cookies: &str| {
        Request::builder()
            .uri(format!("/session-pages/{path}"))
            .header(COOKIE, cookies)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/assets/app.css", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    let init = app
        .clone()
        .oneshot(resource("demo/init.js", &cookie))
        .await
        .unwrap();
    assert_eq!(init.status(), StatusCode::OK);
    assert!(
        String::from_utf8(
            to_bytes(init.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec()
        )
        .unwrap()
        .contains("export { backend }")
    );
    assert_eq!(
        app.clone()
            .oneshot(resource("private/assets/app.css", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/assets/app.css", ""))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/%2e%2e/manifest.json", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/assets/%5c..%5cfile", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/missing.js", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/session-pages/demo?token={token}"))
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::TEMPORARY_REDIRECT
    );
    shares
        .enable_with_config(
            &manager,
            first.id,
            Some(SessionShareConfig {
                session_pages: Some(vec![]),
                persistent: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/assets/app.css", &cookie))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        app.clone()
            .oneshot(resource(
                "demo/assets/app.css",
                &format!("{cookie}; {cookie2}")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    shares.disable(&manager, second.id).await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(resource("demo/assets/app.css", &cookie2))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    // Persistent selections restore, and omitted page settings preserve selection and token.
    let restored = SessionShareService::open(root.path()).unwrap();
    assert!(
        restored
            .status(&manager, first.id)
            .await
            .unwrap()
            .session_pages
            .is_empty()
    );
}
#[tokio::test]
async fn missing_packages_are_cleaned_from_persistent_shares_without_rotating_tokens() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    manager
        .import_session_page(package("demo", &[]), false)
        .await
        .unwrap();
    let session = manager
        .create_session(CreateSessionRequest::default())
        .await
        .unwrap();
    let shares = SessionShareService::open(root.path()).unwrap();
    let shared = shares
        .enable_with_config(
            &manager,
            session.id,
            Some(SessionShareConfig {
                session_pages: Some(vec!["demo".into()]),
                persistent: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let restored = SessionShareService::open(root.path()).unwrap();
    assert_eq!(
        restored
            .status(&manager, session.id)
            .await
            .unwrap()
            .session_pages,
        vec!["demo"]
    );
    assert_eq!(
        shares
            .enable_with_config(&manager, session.id, None)
            .await
            .unwrap()
            .session_pages,
        vec!["demo"]
    );
    manager.delete_session_page("demo".into()).await.unwrap();
    let cleaned = shares.status(&manager, session.id).await.unwrap();
    assert!(cleaned.session_pages.is_empty());
    assert_eq!(cleaned.token, shared.token);
    manager
        .import_session_page(package("demo", &[]), false)
        .await
        .unwrap();
    assert!(
        SessionShareService::open(root.path())
            .unwrap()
            .status(&manager, session.id)
            .await
            .unwrap()
            .session_pages
            .is_empty()
    );
}
