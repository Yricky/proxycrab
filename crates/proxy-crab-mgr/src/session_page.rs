//! Workspace page packages remain single ZIP files; no package code executes in Rust.
use crate::dto::{ManagerError, ManagerResult};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Mutex,
};
use ts_rs::TS;
use zip::ZipArchive;

const MAX_BYTES: u64 = 128 * 1024 * 1024;
const MAX_FILES: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SessionPageManifest {
    pub version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub backend_version: u32,
}
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct SessionPage {
    #[serde(flatten)]
    #[ts(flatten)]
    pub manifest: SessionPageManifest,
    pub size: u64,
}

pub struct PageStore {
    root: PathBuf,
    mutation: Mutex<()>,
}

pub fn valid_id(id: &str) -> ManagerResult<()> {
    if id.is_empty()
        || id.len() > 100
        || !id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
    {
        return Err(ManagerError::bad_request("invalid page package ID"));
    }
    Ok(())
}
pub fn valid_path(path: &str) -> ManagerResult<()> {
    crate::bundle::valid_path(path).map_err(|e| ManagerError::bad_request(e.to_string()))?;
    if path.contains(':') {
        return Err(ManagerError::bad_request("invalid package path"));
    }
    Ok(())
}
fn invalid(error: impl std::fmt::Display) -> ManagerError {
    ManagerError::bad_request(format!("invalid page package: {error}"))
}
fn read_entry<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
) -> ManagerResult<Vec<u8>> {
    let file = zip
        .by_name(name)
        .map_err(|_| ManagerError::not_found("page file not found"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("file too large"));
    }
    Ok(bytes)
}
fn manifest<R: Read + std::io::Seek>(
    zip: &mut ZipArchive<R>,
) -> ManagerResult<SessionPageManifest> {
    let manifest: SessionPageManifest =
        serde_json::from_slice(&read_entry(zip, "manifest.json")?).map_err(invalid)?;
    valid_id(&manifest.id)?;
    if manifest.version != 1
        || manifest.backend_version != 1
        || manifest.name.trim().is_empty()
        || manifest.name.len() > 256
    {
        return Err(invalid("unsupported version or invalid name"));
    }
    Ok(manifest)
}
impl PageStore {
    pub fn new(workspace: &Path) -> Self {
        Self {
            root: workspace.join("session-pages"),
            mutation: Mutex::new(()),
        }
    }
    fn path(&self, id: &str) -> ManagerResult<PathBuf> {
        valid_id(id)?;
        let path = self.root.join(format!("{id}.zip"));
        for part in [&self.root, &path] {
            if fs::symlink_metadata(part).is_ok_and(|m| m.is_symlink()) {
                return Err(invalid("symlink target"));
            }
        }
        Ok(path)
    }
    pub fn list(&self) -> ManagerResult<Vec<SessionPage>> {
        let _lock = self.mutation.lock().unwrap();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(ManagerError::internal(e.to_string())),
        };
        let mut pages = Vec::new();
        for entry in entries {
            let path = entry.map_err(invalid)?.path();
            if path.extension().is_none_or(|ext| ext != "zip") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| invalid("filename"))?;
            let file = fs::File::open(self.path(id)?).map_err(invalid)?;
            let size = file.metadata().map_err(invalid)?.len();
            let mut zip = ZipArchive::new(file).map_err(invalid)?;
            let manifest = manifest(&mut zip)?;
            if manifest.id != id {
                return Err(invalid("package ID does not match filename"));
            }
            pages.push(SessionPage { manifest, size });
        }
        pages.sort_by(|a, b| a.manifest.id.cmp(&b.manifest.id));
        Ok(pages)
    }
    pub fn import(&self, bytes: Vec<u8>, replace: bool) -> ManagerResult<SessionPage> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(invalid("ZIP exceeds 128 MiB"));
        }
        let mut zip = ZipArchive::new(Cursor::new(&bytes)).map_err(invalid)?;
        if zip.len() > MAX_FILES {
            return Err(invalid("too many files"));
        }
        let mut names = BTreeSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(invalid)?;
            let name = file.name().map_err(invalid)?.into_owned();
            valid_path(name.trim_end_matches('/'))?;
            if !names.insert(name.clone())
                || name == "init.js"
                || name.starts_with("init.js/")
                || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            {
                return Err(invalid("duplicate, reserved init.js, or symlink entry"));
            }
            if file.is_dir() {
                continue;
            }
            total = total
                .checked_add(file.size())
                .ok_or_else(|| invalid("expanded size overflow"))?;
            if total > MAX_BYTES {
                return Err(invalid("expanded ZIP exceeds 128 MiB"));
            }
            // Read every entry so truncated streams, CRC failures, and misleading sizes fail at import.
            let copied =
                std::io::copy(&mut file.by_ref().take(MAX_BYTES + 1), &mut std::io::sink())
                    .map_err(invalid)?;
            if copied != file.size() {
                return Err(invalid("entry size mismatch"));
            }
        }
        if !names.contains("index.html") {
            return Err(invalid("missing index.html"));
        }
        let manifest = manifest(&mut zip)?;
        std::str::from_utf8(&read_entry(&mut zip, "index.html")?).map_err(invalid)?;
        let _lock = self.mutation.lock().unwrap();
        let target = self.path(&manifest.id)?;
        if target.exists() && !replace {
            return Err(ManagerError::conflict(
                "page package exists; confirm replacement",
            ));
        }
        fs::create_dir_all(&self.root).map_err(invalid)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&self.root).map_err(invalid)?;
        use std::io::Write;
        temporary.write_all(&bytes).map_err(invalid)?;
        temporary.as_file().sync_all().map_err(invalid)?;
        temporary.persist(target).map_err(invalid)?;
        Ok(SessionPage {
            manifest,
            size: bytes.len() as u64,
        })
    }
    pub fn export(&self, id: &str) -> ManagerResult<Vec<u8>> {
        let _lock = self.mutation.lock().unwrap();
        fs::read(self.path(id)?).map_err(|_| ManagerError::not_found("page package not found"))
    }
    pub fn delete(&self, id: &str) -> ManagerResult<()> {
        let _lock = self.mutation.lock().unwrap();
        fs::remove_file(self.path(id)?)
            .map_err(|_| ManagerError::not_found("page package not found"))
    }
    pub fn file(&self, id: &str, path: &str) -> ManagerResult<Vec<u8>> {
        valid_path(path)?;
        let _lock = self.mutation.lock().unwrap();
        let file = fs::File::open(self.path(id)?)
            .map_err(|_| ManagerError::not_found("page package not found"))?;
        read_entry(&mut ZipArchive::new(file).map_err(invalid)?, path)
    }
}

pub fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
