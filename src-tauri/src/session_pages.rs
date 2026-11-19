use crate::BackendState;
use proxy_crab_mgr::{dto::ManagerError, session_page::SessionPage};
use tauri::State;

#[tauri::command]
pub(crate) async fn list_session_pages(
    state: State<'_, BackendState>,
) -> Result<Vec<SessionPage>, ManagerError> {
    state.manager().session_pages().await
}
#[tauri::command]
pub(crate) async fn import_session_page(
    state: State<'_, BackendState>,
    request: tauri::ipc::Request<'_>,
) -> Result<SessionPage, ManagerError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(ManagerError::bad_request("expected binary ZIP body"));
    };
    let replace = request
        .headers()
        .get("x-page-replace")
        .is_some_and(|v| v == "true");
    let result = state
        .manager()
        .import_session_page(bytes.clone(), replace)
        .await?;
    Ok(result)
}
#[tauri::command]
pub(crate) async fn export_session_page(
    state: State<'_, BackendState>,
    id: String,
) -> Result<tauri::ipc::Response, ManagerError> {
    Ok(tauri::ipc::Response::new(
        state.manager().export_session_page(id).await?,
    ))
}
#[tauri::command]
pub(crate) async fn delete_session_page(
    state: State<'_, BackendState>,
    id: String,
) -> Result<(), ManagerError> {
    state.manager().delete_session_page(id).await?;
    Ok(())
}
