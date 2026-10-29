use proxy_crab_mgr::dto::ManagerError;
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default, Clone)]
pub(crate) struct FileWriters(Arc<Mutex<HashMap<String, SavedFile>>>);
struct SavedFile {
    owner: String,
    path: PathBuf,
    temporary: tempfile::NamedTempFile,
    failed: bool,
}
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

fn save_error(error: impl std::fmt::Display) -> ManagerError {
    ManagerError::internal(format!("保存文件失败：{error}"))
}
impl FileWriters {
    pub fn cleanup(&self, owner: &str) {
        self.0.lock().unwrap().retain(|_, file| file.owner != owner);
    }
    fn open(&self, owner: String, path: PathBuf) -> Result<String, ManagerError> {
        let temporary =
            tempfile::NamedTempFile::new_in(path.parent().unwrap_or(std::path::Path::new(".")))
                .map_err(save_error)?;
        let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed).to_string();
        self.0.lock().unwrap().insert(
            handle.clone(),
            SavedFile {
                owner,
                path,
                temporary,
                failed: false,
            },
        );
        Ok(handle)
    }
    fn append(&self, owner: &str, handle: &str, bytes: &[u8]) -> Result<(), ManagerError> {
        if bytes.len() > 256 * 1024 {
            return Err(ManagerError::bad_request("文件数据块过大"));
        }
        let mut files = self.0.lock().unwrap();
        let file = files
            .get_mut(handle)
            .filter(|f| f.owner == owner)
            .ok_or_else(|| ManagerError::bad_request("文件 Writer 不存在"))?;
        if file.failed {
            return Err(ManagerError::bad_request("文件写入已失败"));
        }
        if let Err(error) = file.temporary.write_all(bytes) {
            file.failed = true;
            return Err(save_error(error));
        }
        Ok(())
    }
    fn close(&self, owner: &str, handle: &str, finish: bool) -> Result<(), ManagerError> {
        let mut files = self.0.lock().unwrap();
        if files.get(handle).is_some_and(|f| f.owner != owner) {
            return Err(ManagerError::bad_request("文件 Writer 不属于当前窗口"));
        }
        let Some(mut file) = files.remove(handle) else {
            return if finish {
                Err(ManagerError::bad_request("文件 Writer 不存在"))
            } else {
                Ok(())
            };
        };
        drop(files);
        if finish {
            if file.failed {
                return Err(ManagerError::bad_request("文件写入已失败"));
            }
            file.temporary.flush().map_err(save_error)?;
            file.temporary
                .persist(&file.path)
                .map_err(|e| save_error(e.error))?;
        }
        Ok(())
    }
}

#[tauri::command]
pub(crate) async fn save_file_open(
    window: tauri::WebviewWindow,
    filename: String,
) -> Result<Option<String>, ManagerError> {
    let writers = window.state::<FileWriters>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(selected) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(&filename)
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(save_error)?;
        // A closed window must not leave a new writer after a delayed dialog result.
        if window
            .app_handle()
            .get_webview_window(window.label())
            .is_none()
        {
            return Ok(None);
        }
        writers.open(window.label().to_owned(), path).map(Some)
    })
    .await
    .map_err(save_error)?
}

#[tauri::command]
pub(crate) async fn save_file_append(
    window: tauri::WebviewWindow,
    writers: State<'_, FileWriters>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), ManagerError> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(ManagerError::bad_request("必须追加二进制数据"));
    };
    let handle = request
        .headers()
        .get("x-proxycrab-writer")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ManagerError::bad_request("缺少文件 Writer"))?
        .to_owned();
    let bytes = bytes.clone();
    let writers = writers.inner().clone();
    let owner = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || writers.append(&owner, &handle, &bytes))
        .await
        .map_err(save_error)?
}

#[tauri::command]
pub(crate) async fn save_file_finish(
    window: tauri::WebviewWindow,
    writers: State<'_, FileWriters>,
    handle: String,
) -> Result<(), ManagerError> {
    let writers = writers.inner().clone();
    let owner = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || writers.close(&owner, &handle, true))
        .await
        .map_err(save_error)?
}

#[tauri::command]
pub(crate) async fn save_file_abort(
    window: tauri::WebviewWindow,
    writers: State<'_, FileWriters>,
    handle: String,
) -> Result<(), ManagerError> {
    let writers = writers.inner().clone();
    let owner = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || writers.close(&owner, &handle, false))
        .await
        .map_err(save_error)?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn commit_abort_and_owner_cleanup_preserve_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture.har");
        std::fs::write(&path, b"original").unwrap();
        let writers = FileWriters::default();
        let handle = writers.open("main".into(), path.clone()).unwrap();
        writers.append("main", &handle, b"first").unwrap();
        assert!(writers.append("other", &handle, b"bad").is_err());
        assert!(writers.close("other", &handle, true).is_err());
        writers.append("main", &handle, b"second").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        writers.close("main", &handle, true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"firstsecond");
        let handle = writers.open("main".into(), path.clone()).unwrap();
        writers.append("main", &handle, b"partial").unwrap();
        writers.close("main", &handle, false).unwrap();
        writers.close("main", &handle, false).unwrap();
        let _handle = writers.open("main".into(), path.clone()).unwrap();
        writers.cleanup("main");
        assert_eq!(std::fs::read(&path).unwrap(), b"firstsecond");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
    #[test]
    fn failed_commit_does_not_leave_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let writers = FileWriters::default();
        let handle = writers
            .open("main".into(), dir.path().to_path_buf())
            .unwrap();
        assert!(writers.close("main", &handle, true).is_err());
        assert!(writers.0.lock().unwrap().is_empty());
    }
}
