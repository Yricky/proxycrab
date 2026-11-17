//! Package URLs preserve ZIP-relative paths. Cookies authorize code resources, never select data.
use super::*;
use axum::http::header::{COOKIE, SET_COOKIE, X_CONTENT_TYPE_OPTIONS};
use axum::response::Redirect;

pub(super) fn router(state: Arc<ShareState>) -> Router {
    Router::new()
        .route("/session-pages/{id}", get(redirect))
        .route("/session-pages/{id}/", get(index))
        .route("/session-pages/{id}/{*path}", get(file))
        .layer(middleware::from_fn(no_store))
        .with_state(state)
}
async fn redirect(Path(id): Path<String>, request: Request) -> Result<Redirect, ShareApiError> {
    crate::session_page::valid_id(&id)?;
    let mut location = format!("/session-pages/{id}/");
    if let Some(query) = request.uri().query() {
        location.push('?');
        location.push_str(query);
    }
    Ok(Redirect::temporary(&location))
}
async fn index(
    State(state): State<Arc<ShareState>>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ShareApiError> {
    serve(state, id, "index.html".into(), request).await
}
async fn file(
    State(state): State<Arc<ShareState>>,
    Path((id, path)): Path<(String, String)>,
    request: Request,
) -> Result<Response, ShareApiError> {
    serve(state, id, path, request).await
}
async fn authorized(state: &ShareState, token: &str, id: &str) -> bool {
    let Ok(scope) = state.shares.authenticate(token) else {
        return false;
    };
    scope.session_pages.iter().any(|p| p == id)
        && state
            .manager
            .sessions()
            .await
            .is_ok_and(|s| s.sessions.iter().any(|s| s.id == scope.session_id))
}
async fn serve(
    state: Arc<ShareState>,
    id: String,
    path: String,
    request: Request,
) -> Result<Response, ShareApiError> {
    crate::session_page::valid_id(&id)?;
    crate::session_page::valid_path(&path)?;
    let token = query_token(&request);
    let has_token = request
        .uri()
        .query()
        .is_some_and(|q| url::form_urlencoded::parse(q.as_bytes()).any(|(k, _)| k == "token"));
    let cookie_name = "pcrab_page_";
    if let Some(token) = &token {
        if !authorized(&state, token, &id).await {
            return Err(ManagerError::new(
                ErrorCode::PageNotAuthorized,
                "page package is not authorized by this share",
            )
            .into());
        }
    } else {
        if has_token || path == "index.html" {
            return Err(
                ManagerError::bad_request("page entry requires exactly one valid token").into(),
            );
        }
        let mut allowed = false;
        for cookie in request
            .headers()
            .get_all(COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(';'))
        {
            if let Some((name, value)) = cookie.trim().split_once('=') {
                if name.starts_with(cookie_name) && authorized(&state, value, &id).await {
                    allowed = true;
                    break;
                }
            }
        }
        if !allowed {
            return Err(ManagerError::new(
                ErrorCode::PageNotAuthorized,
                "page package authorization required",
            )
            .into());
        }
    }
    // Require the package to still exist, including when serving the application-owned init module.
    let bytes = if path == "init.js" {
        state
            .manager
            .session_page_file(id.clone(), "manifest.json".into())
            .await?;
        b"export { backend } from '/assets/session-page-runtime.js';\n".to_vec()
    } else {
        state
            .manager
            .session_page_file(id.clone(), path.clone())
            .await?
    };
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static(crate::session_page::mime(&path)),
    );
    response
        .headers_mut()
        .insert(X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if let Some(token) = token {
        let hash = URL_SAFE_NO_PAD.encode(Sha256::digest(token.as_bytes()));
        // Different tokens use different cookie names, so concurrent Sessions do not replace grants.
        let cookie = format!(
            "{cookie_name}{}={token}; Path=/session-pages/{id}/; HttpOnly; SameSite=Lax",
            &hash[..16]
        );
        response.headers_mut().append(
            SET_COOKIE,
            HeaderValue::from_str(&cookie).map_err(|e| ManagerError::internal(e.to_string()))?,
        );
    }
    Ok(response)
}
