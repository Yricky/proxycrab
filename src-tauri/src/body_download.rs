use std::path::Path;

use proxy_crab_mgr::{dto::ManagerError, http::body_reader};
use proxy_crab_mitm::storage::{BodySide, BodySource};
use serde::Deserialize;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tokio::io::AsyncWriteExt;

use crate::BackendState;

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum BodyTarget {
    Log {
        id: u64,
        session_id: u64,
    },
    Interceptor {
        id: u64,
        session_id: u64,
        execution_id: u64,
    },
    Breakpoint {
        id: u64,
    },
}

#[tauri::command]
pub(crate) async fn download_body(
    window: tauri::WebviewWindow,
    state: State<'_, BackendState>,
    target: BodyTarget,
    side: String,
    filename: String,
) -> Result<bool, ManagerError> {
    let side = match side.as_str() {
        "request" => BodySide::Request,
        "response" => BodySide::Response,
        _ => {
            return Err(ManagerError::bad_request(
                "body side must be request or response",
            ));
        }
    };
    let manager = state.manager();
    let source = match target {
        BodyTarget::Log { id, session_id } => {
            manager.log_body_source(Some(session_id), id, side).await?
        }
        BodyTarget::Interceptor {
            id,
            session_id,
            execution_id,
        } => {
            manager
                .interceptor_snapshot_body_source(Some(session_id), id, execution_id, side)
                .await?
        }
        BodyTarget::Breakpoint { id } => manager.breakpoint_body_source(id, side).await?,
    };
    let selected = tauri::async_runtime::spawn_blocking(move || {
        window
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name(&filename)
            .blocking_save_file()
    })
    .await
    .map_err(|error| ManagerError::internal(format!("打开保存对话框失败：{error}")))?;
    let Some(selected) = selected else {
        return Ok(false);
    };
    let path = selected
        .into_path()
        .map_err(|error| ManagerError::bad_request(format!("无法访问所选路径：{error}")))?;
    save_body(&source, &path).await?;
    Ok(true)
}

async fn save_body(source: &BodySource, path: &Path) -> Result<(), ManagerError> {
    let mut reader = body_reader(source, true).await?;
    // Publish only a complete decoded file; errors leave an existing destination intact.
    let temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(save_error)?;
    let mut writer = tokio::fs::File::from_std(temporary.reopen().map_err(save_error)?);
    tokio::io::copy(&mut reader, &mut writer)
        .await
        .map_err(save_error)?;
    writer.flush().await.map_err(save_error)?;
    drop(writer);
    temporary
        .persist(path)
        .map_err(|error| save_error(error.error))?;
    Ok(())
}

fn save_error(error: std::io::Error) -> ManagerError {
    ManagerError::internal(format!("保存 Body 失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_crab_mitm::storage::BodySourceData;

    #[tokio::test]
    async fn save_preserves_bytes_and_keeps_existing_file_on_decode_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("body.bin");
        let mut source = BodySource {
            data: BodySourceData::Bytes(vec![0, 255, 13, 10, 42]),
            path: None,
            stored_size: 5,
            content_type: None,
            content_encodings: vec![],
        };
        save_body(&source, &path).await.unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), [0, 255, 13, 10, 42]);
        source.content_encodings.push("gzip".into());
        assert!(save_body(&source, &path).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), [0, 255, 13, 10, 42]);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn save_decodes_gzip_without_formatting_text() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("body.json");
        let source = BodySource {
            data: BodySourceData::Bytes(vec![
                31, 139, 8, 0, 0, 0, 0, 0, 2, 255, 83, 168, 86, 80, 74, 84, 178, 82, 48, 84, 168,
                229, 2, 0, 11, 213, 96, 119, 12, 0, 0, 0,
            ]),
            path: None,
            stored_size: 32,
            content_type: Some("application/json".into()),
            content_encodings: vec!["gzip".into()],
        };
        save_body(&source, &path).await.unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b" { \"a\": 1 }\n");
    }

    #[tokio::test]
    #[ignore = "writes a file larger than 1 GiB; run explicitly for download verification"]
    async fn save_streams_files_beyond_preview_limit() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.bin");
        let length = 1024 * 1024 * 1024 + 1;
        std::fs::File::create(&input)
            .unwrap()
            .set_len(length)
            .unwrap();
        let output = directory.path().join("output.bin");
        let source = BodySource {
            data: BodySourceData::File(input),
            path: None,
            stored_size: length,
            content_type: None,
            content_encodings: vec![],
        };
        save_body(&source, &output).await.unwrap();
        assert_eq!(std::fs::metadata(output).unwrap().len(), length);
    }
}
