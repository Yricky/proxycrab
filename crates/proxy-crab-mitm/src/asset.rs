use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::io::AsyncWriteExt;

use crate::workspace::now_millis;

const ASSETS_DIRECTORY: &str = "assets";
const METADATA_DIRECTORY: &str = ".metadata";
const UPLOADS_DIRECTORY: &str = ".asset-uploads";
const MAX_ASSET_ID_BYTES: usize = 255;
const MAX_ASSET_SEGMENT_BYTES: usize = 100;
static NEXT_UPLOAD_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetMetadata {
    pub id: String,
    pub size: u64,
    pub content_type: String,
    pub sha256: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub metadata: AssetMetadata,
    path: PathBuf,
}

impl Asset {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("invalid asset id: {0}")]
    InvalidId(String),
    #[error("asset {0} not found")]
    NotFound(String),
    #[error("asset {0} already exists")]
    AlreadyExists(String),
    #[error("asset path conflicts with an existing file or directory: {0}")]
    PathConflict(String),
    #[error("asset storage failed: {0}")]
    Storage(#[from] std::io::Error),
    #[error("asset metadata is invalid: {0}")]
    InvalidMetadata(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct AssetStore {
    assets: PathBuf,
    metadata: PathBuf,
    uploads: PathBuf,
}

impl AssetStore {
    pub fn open(workspace: &Path) -> Result<Self, AssetError> {
        let assets = workspace.join(ASSETS_DIRECTORY);
        let metadata = assets.join(METADATA_DIRECTORY);
        let uploads = workspace.join(UPLOADS_DIRECTORY);
        std::fs::create_dir_all(&metadata).map_err(map_directory_error)?;
        std::fs::create_dir_all(&uploads).map_err(map_directory_error)?;
        Ok(Self {
            assets,
            metadata,
            uploads,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<Asset>, AssetError> {
        validate_asset_id(id)?;
        let metadata_path = self.metadata_path(id);
        let bytes = match std::fs::read(&metadata_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata: AssetMetadata = serde_json::from_slice(&bytes)?;
        if metadata.id != id {
            return Err(AssetError::Storage(std::io::Error::new(
                ErrorKind::InvalidData,
                "asset metadata id mismatch",
            )));
        }
        let path = self.asset_path(id);
        match std::fs::metadata(&path) {
            Ok(value) if value.is_file() => Ok(Some(Asset { metadata, path })),
            Ok(_) => Err(AssetError::PathConflict(id.to_owned())),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn begin_upload(
        &self,
        id: &str,
        content_type: String,
    ) -> Result<AssetUpload, AssetError> {
        validate_asset_id(id)?;
        self.ensure_available(id)?;
        let sequence = NEXT_UPLOAD_ID.fetch_add(1, Ordering::Relaxed);
        let temporary = self.uploads.join(format!(
            "{}-{}-{sequence}.tmp",
            std::process::id(),
            now_millis()
        ));
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .await?;
        Ok(AssetUpload {
            store: self.clone(),
            id: id.to_owned(),
            content_type,
            temporary,
            file: Some(file),
            size: 0,
            digest: Sha256::new(),
            committed: false,
        })
    }

    fn ensure_available(&self, id: &str) -> Result<(), AssetError> {
        let asset_path = self.asset_path(id);
        let metadata_path = self.metadata_path(id);
        if asset_path.is_dir() {
            return Err(AssetError::PathConflict(id.to_owned()));
        }
        if asset_path.exists() || metadata_path.exists() {
            return Err(AssetError::AlreadyExists(id.to_owned()));
        }
        for ancestor in asset_path.ancestors().skip(1) {
            if ancestor == self.assets {
                break;
            }
            if ancestor.is_file() {
                return Err(AssetError::PathConflict(id.to_owned()));
            }
        }
        Ok(())
    }

    fn asset_path(&self, id: &str) -> PathBuf {
        self.assets.join(id)
    }

    fn metadata_path(&self, id: &str) -> PathBuf {
        let relative = Path::new(id);
        let filename = format!(
            "{}.json",
            relative
                .file_name()
                .expect("validated asset id has a filename")
                .to_string_lossy()
        );
        relative.parent().map_or_else(
            || self.metadata.join(&filename),
            |parent| self.metadata.join(parent).join(&filename),
        )
    }
}

pub struct AssetUpload {
    store: AssetStore,
    id: String,
    content_type: String,
    temporary: PathBuf,
    file: Option<tokio::fs::File>,
    size: u64,
    digest: Sha256,
    committed: bool,
}

impl AssetUpload {
    pub async fn write(&mut self, bytes: &[u8]) -> Result<(), AssetError> {
        self.file
            .as_mut()
            .expect("unfinished asset upload has a file")
            .write_all(bytes)
            .await?;
        self.size = self.size.saturating_add(bytes.len() as u64);
        self.digest.update(bytes);
        Ok(())
    }

    pub async fn finish(mut self) -> Result<AssetMetadata, AssetError> {
        let mut file = self
            .file
            .take()
            .expect("unfinished asset upload has a file");
        file.flush().await?;
        file.sync_all().await?;
        drop(file);

        let asset_path = self.store.asset_path(&self.id);
        let metadata_path = self.store.metadata_path(&self.id);
        create_parent(&asset_path, &self.id)?;
        create_parent(&metadata_path, &self.id)?;
        match std::fs::hard_link(&self.temporary, &asset_path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                return Err(AssetError::AlreadyExists(self.id.clone()));
            }
            Err(error) if is_path_conflict(&error) => {
                return Err(AssetError::PathConflict(self.id.clone()));
            }
            Err(error) => return Err(error.into()),
        }

        let metadata = AssetMetadata {
            id: self.id.clone(),
            size: self.size,
            content_type: self.content_type.clone(),
            sha256: format!("{:x}", self.digest.clone().finalize()),
            created_at: now_millis(),
        };
        let metadata_temporary = self.temporary.with_extension("metadata.tmp");
        let result = (|| -> Result<(), AssetError> {
            let bytes = serde_json::to_vec_pretty(&metadata)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&metadata_temporary)?;
            std::io::Write::write_all(&mut file, &bytes)?;
            file.sync_all()?;
            std::fs::hard_link(&metadata_temporary, &metadata_path).map_err(|error| {
                if error.kind() == ErrorKind::AlreadyExists {
                    AssetError::AlreadyExists(self.id.clone())
                } else if is_path_conflict(&error) {
                    AssetError::PathConflict(self.id.clone())
                } else {
                    AssetError::Storage(error)
                }
            })?;
            Ok(())
        })();
        let _ = std::fs::remove_file(&metadata_temporary);
        if let Err(error) = result {
            let _ = std::fs::remove_file(&asset_path);
            return Err(error);
        }
        self.committed = true;
        let _ = std::fs::remove_file(&self.temporary);
        Ok(metadata)
    }
}

impl Drop for AssetUpload {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.temporary);
        }
    }
}

pub fn validate_asset_id(id: &str) -> Result<(), AssetError> {
    if id.is_empty() || id.len() > MAX_ASSET_ID_BYTES {
        return Err(AssetError::InvalidId(id.to_owned()));
    }
    if id.starts_with('/') || id.ends_with('/') || id.contains("//") {
        return Err(AssetError::InvalidId(id.to_owned()));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"_/.".contains(&byte))
    {
        return Err(AssetError::InvalidId(id.to_owned()));
    }
    for segment in id.split('/') {
        if segment.is_empty()
            || segment.len() > MAX_ASSET_SEGMENT_BYTES
            || matches!(segment, "." | ".." | METADATA_DIRECTORY)
        {
            return Err(AssetError::InvalidId(id.to_owned()));
        }
    }
    Ok(())
}

fn create_parent(path: &Path, id: &str) -> Result<(), AssetError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    std::fs::create_dir_all(parent).map_err(|error| {
        if is_path_conflict(&error) {
            AssetError::PathConflict(id.to_owned())
        } else {
            AssetError::Storage(error)
        }
    })
}

fn map_directory_error(error: std::io::Error) -> AssetError {
    AssetError::Storage(error)
}

fn is_path_conflict(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::AlreadyExists | ErrorKind::NotADirectory | ErrorKind::IsADirectory
    )
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn validates_asset_ids() {
        for valid in ["a", "fixtures/example.json", "body.v2/empty_file"] {
            validate_asset_id(valid).unwrap();
        }
        for invalid in [
            "",
            "/a",
            "a/",
            "a//b",
            "a/../b",
            "a/./b",
            ".metadata/a",
            "a/.metadata/b",
            "Upper",
            "a-b",
        ] {
            assert!(
                matches!(validate_asset_id(invalid), Err(AssetError::InvalidId(_))),
                "{invalid}"
            );
        }
    }

    #[tokio::test]
    async fn streams_nested_asset_and_persists_metadata() {
        let root = TempDir::new().unwrap();
        let store = AssetStore::open(root.path()).unwrap();
        let mut upload = store
            .begin_upload("fixtures/example.json", "application/json".into())
            .await
            .unwrap();
        upload.write(b"{\"ok\":").await.unwrap();
        upload.write(b"true}").await.unwrap();
        let metadata = upload.finish().await.unwrap();
        assert_eq!(metadata.size, 11);
        assert_eq!(
            metadata.sha256,
            format!("{:x}", Sha256::digest(b"{\"ok\":true}"))
        );
        let asset = store.get("fixtures/example.json").unwrap().unwrap();
        assert_eq!(asset.metadata, metadata);
        assert_eq!(std::fs::read(asset.path()).unwrap(), b"{\"ok\":true}");
    }

    #[tokio::test]
    async fn assets_are_immutable_and_empty_assets_are_allowed() {
        let root = TempDir::new().unwrap();
        let store = AssetStore::open(root.path()).unwrap();
        let upload = store
            .begin_upload("empty.bin", "application/octet-stream".into())
            .await
            .unwrap();
        let metadata = upload.finish().await.unwrap();
        assert_eq!(metadata.size, 0);
        assert!(matches!(
            store
                .begin_upload("empty.bin", "application/octet-stream".into())
                .await,
            Err(AssetError::AlreadyExists(_))
        ));
    }

    #[tokio::test]
    async fn reports_nested_path_conflicts() {
        let root = TempDir::new().unwrap();
        let store = AssetStore::open(root.path()).unwrap();
        store
            .begin_upload("fixtures", "application/octet-stream".into())
            .await
            .unwrap()
            .finish()
            .await
            .unwrap();
        assert!(matches!(
            store
                .begin_upload("fixtures/a.bin", "application/octet-stream".into())
                .await,
            Err(AssetError::PathConflict(_))
        ));
    }
}
