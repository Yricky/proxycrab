use std::{collections::HashSet, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use tauri::{AssetResolver, Wry};

#[derive(Clone)]
struct ShareUiState {
    assets: Arc<AssetResolver<Wry>>,
    embedded_paths: Arc<HashSet<String>>,
}

pub fn router(assets: AssetResolver<Wry>) -> Router {
    let embedded_paths = assets.iter().map(|(path, _)| path.into_owned()).collect();
    let assets = Arc::new(assets);
    Router::new()
        .route("/session", get(page))
        .route("/assets/{*path}", get(asset))
        .with_state(ShareUiState {
            assets,
            embedded_paths: Arc::new(embedded_paths),
        })
}

async fn page(State(state): State<ShareUiState>) -> Response {
    static_asset(&state, "session.html")
}

async fn asset(State(state): State<ShareUiState>, Path(path): Path<String>) -> Response {
    if path
        .split('/')
        .any(|segment| matches!(segment, "" | "." | ".."))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    static_asset(&state, &format!("assets/{path}"))
}

fn static_asset(state: &ShareUiState, path: &str) -> Response {
    if !is_known_asset(path, &state.embedded_paths) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(asset) = state.assets.get(path.into()) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let is_html = path.ends_with(".html");
    let content_type = HeaderValue::from_str(&asset.mime_type)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let mut response = Response::new(Body::from(asset.bytes));
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if is_html {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        }),
    );
    if is_html {
        response
            .headers_mut()
            .insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    }
    response
}

fn is_known_asset(path: &str, embedded_paths: &HashSet<String>) -> bool {
    embedded_paths.is_empty() || embedded_paths.contains(path)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::is_known_asset;

    #[test]
    fn accepts_embedded_asset_path() {
        let paths = HashSet::from(["session.html".to_owned(), "assets/app.js".to_owned()]);
        assert!(is_known_asset("assets/app.js", &paths));
    }

    #[test]
    fn rejects_missing_embedded_asset_path() {
        let paths = HashSet::from(["session.html".to_owned(), "assets/app.js".to_owned()]);
        assert!(!is_known_asset("assets/missing.js", &paths));
    }

    #[test]
    fn defers_to_filesystem_resolution_when_no_assets_are_embedded() {
        assert!(is_known_asset("assets/app.js", &HashSet::new()));
    }
}
