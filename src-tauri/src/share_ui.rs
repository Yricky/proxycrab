use axum::{
    Router,
    body::Body,
    extract::Path,
    http::{
        HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE, REFERRER_POLICY},
    },
    response::{IntoResponse, Response},
    routing::get,
};

include!(concat!(env!("OUT_DIR"), "/share_ui_assets.rs"));

pub fn router() -> Router {
    Router::new()
        .route("/session", get(index))
        .route("/assets/{*path}", get(asset))
}

async fn index() -> Response {
    static_asset("index.html")
}

async fn asset(Path(path): Path<String>) -> Response {
    static_asset(&format!("assets/{path}"))
}

fn static_asset(path: &str) -> Response {
    let Some((_, bytes)) = SHARE_UI_ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        _ => "application/octet-stream",
    };
    let mut response = Response::new(Body::from(*bytes));
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        CACHE_CONTROL,
        HeaderValue::from_static(if path == "index.html" {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        }),
    );
    if path == "index.html" {
        response
            .headers_mut()
            .insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    }
    response
}
