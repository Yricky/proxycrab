//! Workspace-shaped configuration bundles. Intents describe contents, never activation.
use anyhow::{Context, Result, bail};
use proxy_crab_mitm::{
    ProxyCrab,
    model::{
        Column, FilterColumn, FilterOption, NewSession, SessionInterceptors, SessionMetadata,
        SessionView,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read, Write},
    path::{Component, Path},
    sync::Mutex,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

const MAX_BYTES: usize = 128 * 1024 * 1024;
const MAX_FILES: usize = 4096;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigShare {
    pub assets: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub assets: Vec<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u32,
    pub intents: Vec<Intent>,
}
#[derive(Clone, Serialize)]
pub struct FileStatus {
    pub path: String,
    pub status: String,
    pub fingerprint: Option<String>,
    pub can_rename: bool,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub path: String,
    pub action: String,
    pub fingerprint: Option<String>,
    pub name: Option<String>,
}
#[derive(Serialize)]
pub struct ImportResult {
    pub import_id: Option<String>,
    pub files: Vec<FileStatus>,
    pub sessions: Vec<SessionMetadata>,
}
struct Pending {
    _directory: TempDir,
    files: BTreeMap<String, Vec<u8>>,
    intents: Vec<Intent>,
    created: Instant,
}
#[derive(Default)]
pub struct BundleImports {
    pending: Mutex<BTreeMap<String, Pending>>,
}

pub fn valid_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.contains('\\')
        || path.contains('\0')
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("invalid bundle path: {path}");
    }
    Ok(())
}
fn local_path(root: &Path, path: &str) -> Result<std::path::PathBuf> {
    valid_path(path)?;
    let mut current = root.to_path_buf();
    for part in path.split('/') {
        current.push(part);
        if fs::symlink_metadata(&current).is_ok_and(|m| m.is_symlink()) {
            bail!("symlink in bundle target: {path}");
        }
    }
    Ok(current)
}
fn json<T: serde::de::DeserializeOwned>(
    files: &BTreeMap<String, Vec<u8>>,
    path: &str,
) -> Result<T> {
    serde_json::from_slice(
        files
            .get(path)
            .with_context(|| format!("missing bundle file: {path}"))?,
    )
    .with_context(|| format!("invalid bundle file: {path}"))
}
fn read_zip(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>> {
    if bytes.len() > MAX_BYTES {
        bail!("bundle exceeds 128 MiB");
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_FILES {
        bail!("too many bundle files");
    }
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for i in 0..archive.len() {
        let file = archive.by_index(i)?;
        let name = file.name()?.into_owned();
        if file.is_dir() {
            valid_path(name.trim_end_matches('/'))?;
            continue;
        }
        valid_path(&name)?;
        if file
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            bail!("bundle symlinks are unsupported");
        }
        total = total
            .checked_add(usize::try_from(file.size())?)
            .context("bundle too large")?;
        if total > MAX_BYTES {
            bail!("expanded bundle exceeds 128 MiB");
        }
        let mut data = Vec::new();
        file.take((MAX_BYTES + 1) as u64).read_to_end(&mut data)?;
        if data.len() > MAX_BYTES || files.insert(name.clone(), data).is_some() {
            bail!("invalid or duplicate bundle file: {name}");
        }
    }
    Ok(files)
}
fn script_path(kind: &str, name: &str) -> Result<String> {
    valid_path(name)?;
    if name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
    {
        bail!("invalid script name: {name}");
    }
    Ok(format!("scripts/{kind}/{name}.lua"))
}
fn dependencies(view: &SessionView, chains: &SessionInterceptors) -> Result<BTreeSet<String>> {
    let mut paths = BTreeSet::new();
    for item in &chains.request {
        paths.insert(script_path("request", &item.name)?);
    }
    for item in &chains.response {
        paths.insert(script_path("response", &item.name)?);
    }
    for column in &view.columns {
        if let Column::Script { script_name, .. } = column {
            paths.insert(script_path("column", script_name)?);
        }
    }
    match &view.filter.option {
        Some(FilterOption::Script { script_name }) => {
            paths.insert(script_path("filter", script_name)?);
        }
        Some(FilterOption::Column {
            column: FilterColumn::Script { script_name },
            ..
        }) => {
            paths.insert(script_path("column", script_name)?);
        }
        _ => {}
    }
    Ok(paths)
}
fn asset_metadata_path(id: &str) -> String {
    match id.rsplit_once('/') {
        Some((parent, name)) => format!("assets/.metadata/{parent}/{name}.json"),
        None => format!("assets/.metadata/{id}.json"),
    }
}
fn selected_assets<'a>(
    rules: &[String],
    ids: impl Iterator<Item = &'a str>,
) -> Result<BTreeSet<String>> {
    let ids = ids.collect::<Vec<_>>();
    let mut selected = BTreeSet::new();
    for rule in rules {
        let prefix = rule.ends_with('/');
        let id = if prefix {
            &rule[..rule.len() - 1]
        } else {
            rule
        };
        proxy_crab_mitm::asset::validate_asset_id(id)?;
        let matches = ids
            .iter()
            .filter(|id| {
                if prefix {
                    id.starts_with(rule.as_str())
                } else {
                    **id == rule
                }
            })
            .copied()
            .collect::<Vec<_>>();
        if matches.is_empty() {
            bail!("resource rule has no match: {rule}");
        }
        selected.extend(matches.into_iter().map(str::to_owned));
    }
    Ok(selected)
}
fn sessions(files: &BTreeMap<String, Vec<u8>>, intents: &[Intent]) -> Result<Vec<NewSession>> {
    intents
        .iter()
        .map(|intent| {
            if intent.kind != "session" {
                bail!("unsupported intent: {}", intent.kind);
            }
            valid_path(&intent.path)?;
            let parts = intent.path.split('/').collect::<Vec<_>>();
            if parts.len() != 2 || parts[0] != "sessions" || parts[1].parse::<u64>().is_err() {
                bail!("invalid session intent path");
            }
            let meta: SessionMetadata = json(files, &format!("{}/metadata.json", intent.path))?;
            let view: SessionView = json(files, &format!("{}/view.json", intent.path))?;
            let chains: SessionInterceptors =
                json(files, &format!("{}/interceptors.json", intent.path))?;
            for path in dependencies(&view, &chains)? {
                let data = files
                    .get(&path)
                    .with_context(|| format!("missing script: {path}"))?;
                std::str::from_utf8(data)?;
            }
            Ok(NewSession {
                name: Some(meta.name),
                description: meta.description,
                view: Some(view),
                interceptors: Some(chains),
            })
        })
        .collect()
}
fn required_files(
    files: &BTreeMap<String, Vec<u8>>,
    intents: &[Intent],
) -> Result<BTreeMap<String, Vec<u8>>> {
    let params = sessions(files, intents)?;
    let mut required = BTreeSet::new();
    for (intent, params) in intents.iter().zip(params) {
        required.extend(dependencies(
            params.view.as_ref().unwrap(),
            params.interceptors.as_ref().unwrap(),
        )?);
        let ids = files
            .keys()
            .filter_map(|p| p.strip_prefix("assets/"))
            .filter(|p| !p.starts_with(".metadata/"));
        for id in selected_assets(&intent.assets, ids)? {
            let metadata_path = asset_metadata_path(&id);
            let meta: proxy_crab_mitm::asset::AssetMetadata = json(files, &metadata_path)?;
            let data = files
                .get(&format!("assets/{id}"))
                .context("missing asset")?;
            if meta.id != id || meta.size != data.len() as u64 || meta.sha256 != hash(data) {
                bail!("invalid asset metadata: {id}");
            }
            required.insert(format!("assets/{id}"));
            required.insert(metadata_path);
        }
    }
    required
        .into_iter()
        .map(|p| {
            Ok((
                p.clone(),
                files
                    .get(&p)
                    .with_context(|| format!("missing file: {p}"))?
                    .clone(),
            ))
        })
        .collect()
}
fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn target_bytes(root: &Path, path: &str) -> Result<Option<Vec<u8>>> {
    match fs::read(local_path(root, path)?) {
        Ok(data) => Ok(Some(data)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn statuses(root: &Path, files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<FileStatus>> {
    files
        .iter()
        .filter(|(p, _)| !p.starts_with("assets/.metadata/"))
        .map(|(path, data)| {
            let current = target_bytes(root, path)?;
            // Asset metadata is part of the resource: same body with a different MIME type is a conflict.
            let mut same = current.as_ref() == Some(data);
            let mut fingerprint = current.as_deref().map(hash);
            if let Some(id) = path.strip_prefix("assets/") {
                let old = target_bytes(root, &asset_metadata_path(id))?;
                let incoming: proxy_crab_mitm::asset::AssetMetadata =
                    json(files, &asset_metadata_path(id))?;
                let old_meta = old.as_ref().and_then(|b| {
                    serde_json::from_slice::<proxy_crab_mitm::asset::AssetMetadata>(b).ok()
                });
                same &= old_meta.as_ref().is_some_and(|m| {
                    m.content_type == incoming.content_type && m.sha256 == incoming.sha256
                });
                if let Some(body) = &current {
                    let mut combined = body.clone();
                    combined.extend(old.unwrap_or_default());
                    fingerprint = Some(hash(&combined));
                }
            }
            Ok(FileStatus {
                path: path.clone(),
                status: if same {
                    "same"
                } else if current.is_some() {
                    "conflict"
                } else {
                    "new"
                }
                .into(),
                fingerprint,
                can_rename: path.starts_with("scripts/"),
            })
        })
        .collect()
}

fn add_local_file(files: &mut BTreeMap<String, Vec<u8>>, root: &Path, path: String) -> Result<()> {
    let remaining = MAX_BYTES.saturating_sub(files.values().map(Vec::len).sum::<usize>());
    let mut bytes = Vec::new();
    fs::File::open(local_path(root, &path)?)
        .with_context(|| format!("missing bundle file: {path}"))?
        .take((remaining + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > remaining || files.len() >= MAX_FILES {
        bail!("bundle too large");
    }
    files.insert(path, bytes);
    Ok(())
}

pub fn export(runtime: &ProxyCrab, session_id: u64, config: &ConfigShare) -> Result<Vec<u8>> {
    let workspace = runtime.workspace();
    let root = workspace.root();
    let meta = runtime
        .sessions()
        .into_iter()
        .find(|s| s.id == session_id)
        .context("session not found")?;
    let view = workspace.session_view(session_id)?;
    let chains = workspace.session_interceptors(session_id)?;
    let intent = Intent {
        id: format!("session-{session_id}"),
        kind: "session".into(),
        path: format!("sessions/{session_id}"),
        assets: config.assets.clone(),
    };
    let mut files = BTreeMap::new();
    files.insert(
        "intents.json".into(),
        serde_json::to_vec(&Manifest {
            version: 1,
            intents: vec![intent.clone()],
        })?,
    );
    files.insert(
        "workspace_schema.json".into(),
        fs::read(root.join("workspace_schema.json"))?,
    );
    files.insert(
        format!("{}/metadata.json", intent.path),
        serde_json::to_vec(&meta)?,
    );
    files.insert(
        format!("{}/view.json", intent.path),
        serde_json::to_vec(&view)?,
    );
    files.insert(
        format!("{}/interceptors.json", intent.path),
        serde_json::to_vec(&chains)?,
    );
    for path in dependencies(&view, &chains)? {
        add_local_file(&mut files, root, path)?;
    }
    let assets = runtime.assets()?;
    for id in selected_assets(&config.assets, assets.iter().map(|a| a.id.as_str()))? {
        for path in [format!("assets/{id}"), asset_metadata_path(&id)] {
            add_local_file(&mut files, root, path)?;
        }
    }
    required_files(&files, &[intent])?;
    if files.len() > MAX_FILES || files.values().map(Vec::len).sum::<usize>() > MAX_BYTES {
        bail!("bundle too large");
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, data) in files {
        writer.start_file(
            path,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )?;
        writer.write_all(&data)?;
    }
    let bytes = writer.finish()?.into_inner();
    if bytes.len() > MAX_BYTES {
        bail!("compressed bundle too large");
    }
    Ok(bytes)
}
impl BundleImports {
    pub fn import(
        &self,
        runtime: &ProxyCrab,
        bytes: Vec<u8>,
        accepted: Vec<String>,
    ) -> Result<ImportResult> {
        let files = read_zip(&bytes)?;
        let manifest: Manifest = json(&files, "intents.json")?;
        if manifest.version != 1 {
            bail!("unsupported bundle version");
        }
        let schema: serde_json::Value = json(&files, "workspace_schema.json")?;
        let local: serde_json::Value = serde_json::from_slice(&fs::read(
            runtime.workspace().root().join("workspace_schema.json"),
        )?)?;
        if schema != local {
            bail!("unsupported workspace schema");
        }
        let mut ids = BTreeSet::new();
        for intent in &manifest.intents {
            if intent.id.is_empty() || !ids.insert(intent.id.clone()) {
                bail!("empty or duplicate intent id");
            }
        }
        let selected = accepted.iter().collect::<BTreeSet<_>>();
        if selected.is_empty()
            || selected.len() != accepted.len()
            || accepted.iter().any(|id| !ids.contains(id))
        {
            bail!("accepted_intent_ids must explicitly select unique existing intents");
        }
        let intents = manifest
            .intents
            .into_iter()
            .filter(|i| selected.contains(&i.id))
            .collect::<Vec<_>>();
        required_files(&files, &intents)?;
        let directory = tempfile::tempdir()?;
        fs::write(directory.path().join("bundle.zip"), bytes)?;
        let pending = Pending {
            _directory: directory,
            files,
            intents,
            created: Instant::now(),
        };
        let id = random_id()?;
        let mut imports = self.pending.lock().unwrap();
        imports.retain(|_, p| p.created.elapsed() < Duration::from_secs(1800));
        if imports.len() >= 16 {
            bail!("too many pending imports");
        }
        imports.insert(id.clone(), pending);
        drop(imports);
        self.resolve(runtime, id, vec![])
    }
    pub fn resolve(
        &self,
        runtime: &ProxyCrab,
        id: String,
        resolutions: Vec<Resolution>,
    ) -> Result<ImportResult> {
        let mut imports = self.pending.lock().unwrap();
        imports.retain(|_, p| p.created.elapsed() < Duration::from_secs(1800));
        let p = imports.get(&id).context("import expired or not found")?;
        let root = runtime.workspace().root();
        let required = required_files(&p.files, &p.intents)?;
        let files = statuses(root, &required)?;
        let mut decisions = BTreeMap::new();
        for r in &resolutions {
            if decisions.insert(r.path.clone(), r).is_some()
                || !files.iter().any(|f| f.path == r.path)
            {
                bail!("invalid conflict resolution: {}", r.path);
            }
        }
        if files.iter().any(|f| {
            f.status == "conflict"
                && !decisions
                    .get(&f.path)
                    .is_some_and(|r| r.fingerprint == f.fingerprint)
        }) {
            return Ok(ImportResult {
                import_id: Some(id),
                files,
                sessions: vec![],
            });
        }
        let mut params = sessions(&p.files, &p.intents)?;
        let mut writes = BTreeMap::new();
        for f in &files {
            let r = (f.status == "conflict")
                .then(|| decisions.get(&f.path))
                .flatten();
            if f.status == "same" || r.is_some_and(|r| r.action == "keep") {
                continue;
            }
            let mut target = f.path.clone();
            if let Some(r) = r {
                match r.action.as_str() {
                    "overwrite" => {}
                    "rename" if f.can_rename => {
                        let (_, rest) = f.path.split_once('/').unwrap();
                        let (kind, old) = rest.split_once('/').unwrap();
                        let name = r.name.as_ref().context("rename requires name")?;
                        target = script_path(kind, name)?;
                        if target_bytes(root, &target)?.is_some() || required.contains_key(&target)
                        {
                            bail!("rename target exists: {target}");
                        }
                        for param in &mut params {
                            rename(param, kind, old.strip_suffix(".lua").unwrap(), name);
                        }
                    }
                    _ => bail!("invalid conflict action"),
                }
            }
            if writes
                .insert(
                    target.clone(),
                    (required[&f.path].clone(), target_bytes(root, &target)?),
                )
                .is_some()
            {
                bail!("duplicate rename target");
            }
            if let Some(asset_id) = f.path.strip_prefix("assets/") {
                let path = asset_metadata_path(asset_id);
                writes.insert(
                    path.clone(),
                    (required[&path].clone(), target_bytes(root, &path)?),
                );
            }
        }
        let latest = statuses(root, &required)?;
        if latest
            .iter()
            .zip(&files)
            .any(|(new, old)| new.status != old.status || new.fingerprint != old.fingerprint)
        {
            return Ok(ImportResult {
                import_id: Some(id),
                files: latest,
                sessions: vec![],
            });
        }
        let changes = writes
            .into_iter()
            .map(
                |(path, (data, expected))| proxy_crab_mitm::workspace::ImportFile {
                    path,
                    data,
                    expected,
                },
            )
            .collect();
        let result = runtime.workspace().import_batch(changes, params);
        imports.remove(&id);
        Ok(ImportResult {
            import_id: None,
            files,
            sessions: result?,
        })
    }
}
fn rename(param: &mut NewSession, kind: &str, old: &str, name: &str) {
    let view = param.view.as_mut().unwrap();
    let chains = param.interceptors.as_mut().unwrap();
    let change = |s: &mut String| {
        if s == old {
            *s = name.into();
        }
    };
    match kind {
        "request" => {
            for i in &mut chains.request {
                change(&mut i.name)
            }
        }
        "response" => {
            for i in &mut chains.response {
                change(&mut i.name)
            }
        }
        "column" => {
            for c in &mut view.columns {
                if let Column::Script { script_name, .. } = c {
                    change(script_name)
                }
            }
            if let Some(FilterOption::Column {
                column: FilterColumn::Script { script_name },
                ..
            }) = &mut view.filter.option
            {
                change(script_name)
            }
        }
        "filter" => {
            if let Some(FilterOption::Script { script_name }) = &mut view.filter.option {
                change(script_name)
            }
        }
        _ => {}
    }
}
fn random_id() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(hash(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proxy_crab_mitm::{
        log_buffer::LogBuffer,
        model::{Script, ScriptKind, SessionInterceptor},
    };
    use std::sync::Arc;
    fn runtime(dir: &TempDir) -> Arc<ProxyCrab> {
        ProxyCrab::open(dir.path(), Arc::new(LogBuffer::default())).unwrap()
    }
    fn sample(r: &ProxyCrab) -> u64 {
        r.workspace()
            .save_script(
                ScriptKind::RequestInterceptor,
                Script {
                    name: "rewrite".into(),
                    content: "function on_request(req) end".into(),
                },
                false,
            )
            .unwrap();
        r.create_session(NewSession {
            name: Some("demo".into()),
            interceptors: Some(SessionInterceptors {
                request: vec![SessionInterceptor {
                    name: "rewrite".into(),
                    enabled: false,
                }],
                response: vec![],
            }),
            ..Default::default()
        })
        .unwrap()
        .id
    }
    async fn asset(r: &ProxyCrab, id: &str, bytes: &[u8]) {
        if r.asset(id).unwrap().is_some() {
            fs::remove_file(r.workspace().root().join(format!("assets/{id}"))).unwrap();
            fs::remove_file(r.workspace().root().join(asset_metadata_path(id))).unwrap();
        }
        let mut upload = r
            .begin_asset_upload(id, "application/json".into())
            .await
            .unwrap();
        upload.write(bytes).await.unwrap();
        upload.finish().await.unwrap();
    }
    #[tokio::test]
    async fn round_trip_assets_scripts_and_explicit_intents_without_activation() {
        let source = tempfile::tempdir().unwrap();
        let r = runtime(&source);
        let id = sample(&r);
        asset(&r, "mock/a.json", b"{}").await;
        asset(&r, "mock/nested/b.json", b"[]").await;
        let config = ConfigShare {
            assets: vec!["mock/".into(), "mock/a.json".into()],
        };
        let bytes = export(&r, id, &config).unwrap();
        let dest = tempfile::tempdir().unwrap();
        let target = runtime(&dest);
        let service = BundleImports::default();
        assert!(service.import(&target, bytes.clone(), vec![]).is_err());
        assert!(
            service
                .import(&target, bytes.clone(), vec!["missing".into()])
                .is_err()
        );
        let result = service
            .import(&target, bytes.clone(), vec![format!("session-{id}")])
            .unwrap();
        assert!(result.import_id.is_none());
        assert_eq!(result.sessions.len(), 1);
        assert_eq!(target.active_session_id(), None);
        assert_eq!(
            target
                .asset("mock/nested/b.json")
                .unwrap()
                .unwrap()
                .metadata
                .sha256,
            hash(b"[]")
        );
        let second = service
            .import(&target, bytes, vec![format!("session-{id}")])
            .unwrap();
        assert!(second.files.iter().all(|f| f.status == "same"));
        assert_eq!(target.sessions().len(), 2);
        asset(&r, "mock/later.json", b"null").await;
        assert!(
            read_zip(&export(&r, id, &config).unwrap())
                .unwrap()
                .contains_key("assets/mock/later.json")
        );
        assert!(
            export(
                &r,
                id,
                &ConfigShare {
                    assets: vec!["missing/".into()]
                }
            )
            .is_err()
        );
    }
    #[tokio::test]
    async fn multi_intent_import_selects_only_accepted_dependencies_and_preserves_manifest_order() {
        let source = tempfile::tempdir().unwrap();
        let r = runtime(&source);
        let first = sample(&r);
        let second = r
            .create_session(NewSession {
                name: Some("second".into()),
                ..Default::default()
            })
            .unwrap()
            .id;
        asset(&r, "first/a.json", b"1").await;
        asset(&r, "second/b.json", b"2").await;
        let mut files = read_zip(
            &export(
                &r,
                first,
                &ConfigShare {
                    assets: vec!["first/".into()],
                },
            )
            .unwrap(),
        )
        .unwrap();
        let extra = read_zip(
            &export(
                &r,
                second,
                &ConfigShare {
                    assets: vec!["second/".into()],
                },
            )
            .unwrap(),
        )
        .unwrap();
        let mut manifest: Manifest = json(&files, "intents.json").unwrap();
        let other: Manifest = json(&extra, "intents.json").unwrap();
        manifest.intents.extend(other.intents);
        files.extend(extra);
        files.insert(
            "intents.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, data) in files {
            zip.start_file(path, SimpleFileOptions::default()).unwrap();
            zip.write_all(&data).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        let destination = tempfile::tempdir().unwrap();
        let target = runtime(&destination);
        let service = BundleImports::default();
        service
            .import(&target, bytes.clone(), vec![format!("session-{second}")])
            .unwrap();
        assert!(target.asset("first/a.json").unwrap().is_none());
        assert!(target.asset("second/b.json").unwrap().is_some());
        assert!(
            !destination
                .path()
                .join("scripts/request/rewrite.lua")
                .exists()
        );
        let imported = service
            .import(
                &target,
                bytes,
                vec![format!("session-{second}"), format!("session-{first}")],
            )
            .unwrap();
        assert_eq!(
            imported
                .sessions
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["demo", "second"]
        );
        assert!(target.asset("first/a.json").unwrap().is_some());
    }

    #[tokio::test]
    async fn conflicts_are_rechecked_and_script_rename_updates_session_references() {
        let source = tempfile::tempdir().unwrap();
        let r = runtime(&source);
        let id = sample(&r);
        asset(&r, "mock/a.json", b"new").await;
        let bytes = export(
            &r,
            id,
            &ConfigShare {
                assets: vec!["mock/".into()],
            },
        )
        .unwrap();
        let dest = tempfile::tempdir().unwrap();
        let target = runtime(&dest);
        sample(&target);
        asset(&target, "mock/a.json", b"old").await;
        target
            .workspace()
            .save_script(
                ScriptKind::RequestInterceptor,
                Script {
                    name: "rewrite".into(),
                    content: "local old = true".into(),
                },
                true,
            )
            .unwrap();
        let service = BundleImports::default();
        let pending = service
            .import(&target, bytes, vec![format!("session-{id}")])
            .unwrap();
        assert!(pending.import_id.is_some());
        assert_eq!(target.sessions().len(), 1);
        let resolutions = pending
            .files
            .iter()
            .filter(|f| f.status == "conflict")
            .map(|f| Resolution {
                path: f.path.clone(),
                action: if f.can_rename { "rename" } else { "overwrite" }.into(),
                fingerprint: f.fingerprint.clone(),
                name: Some("rewrite-imported".into()),
            })
            .collect::<Vec<_>>();
        asset(&target, "mock/a.json", b"changed").await;
        let updated = service
            .resolve(&target, pending.import_id.unwrap(), resolutions.clone())
            .unwrap();
        assert!(updated.import_id.is_some());
        assert_eq!(target.sessions().len(), 1);
        let resolutions = updated
            .files
            .iter()
            .filter(|f| f.status == "conflict")
            .map(|f| Resolution {
                path: f.path.clone(),
                action: if f.can_rename { "rename" } else { "overwrite" }.into(),
                fingerprint: f.fingerprint.clone(),
                name: Some("rewrite-imported".into()),
            })
            .collect();
        let result = service
            .resolve(&target, updated.import_id.unwrap(), resolutions)
            .unwrap();
        assert_eq!(
            target
                .workspace()
                .session_interceptors(result.sessions[0].id)
                .unwrap()
                .request[0]
                .name,
            "rewrite-imported"
        );
        assert_eq!(
            fs::read(target.asset("mock/a.json").unwrap().unwrap().path()).unwrap(),
            b"new"
        );
    }
    #[test]
    fn selected_intents_do_not_import_unaccepted_resources() {
        let dir = tempfile::tempdir().unwrap();
        let r = runtime(&dir);
        let id = sample(&r);
        let mut files =
            read_zip(&export(&r, id, &ConfigShare { assets: vec![] }).unwrap()).unwrap();
        files.insert("assets/unselected.txt".into(), b"private".to_vec());
        let required = required_files(
            &files,
            &[Intent {
                id: "chosen".into(),
                kind: "session".into(),
                path: format!("sessions/{id}"),
                assets: vec![],
            }],
        )
        .unwrap();
        assert!(!required.contains_key("assets/unselected.txt"));
        files.remove("scripts/request/rewrite.lua");
        assert!(
            required_files(
                &files,
                &[Intent {
                    id: "chosen".into(),
                    kind: "session".into(),
                    path: format!("sessions/{id}"),
                    assets: vec![]
                }]
            )
            .is_err()
        );
        assert!(read_zip(&[1, 2, 3]).is_err());
    }
}
