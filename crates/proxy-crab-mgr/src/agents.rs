use std::{
    collections::HashSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::dto::{AgentsPreset, AgentsPresetState};

const PRESETS_DIRECTORY: &str = "presets";
const CONFIG_FILE: &str = "config.json";
const FULL_CAPABILITY_ID: &str = "full-capability";
const FULL_CAPABILITY_NAME: &str = "充分使用能力";
const QUIET_INVESTIGATION_ID: &str = "quiet-investigation";
const QUIET_INVESTIGATION_NAME: &str = "静默排查";

pub const FULL_CAPABILITY_CONTENT: &str = r#"# ProxyCrab Agent 行为：充分使用能力

## 指令优先级

- 当前用户的明确要求高于本预设；发生冲突时，以用户要求为准。

## 排查方式

- 为完成用户要求，可以按需创建、编辑或切换 Session。
- 可以按需调整 Session 过滤条件、自定义列和拦截器链，也可以创建或修改列、过滤、分流和拦截器脚本。
- 允许这些操作同步影响 ProxyCrab 桌面界面，但应控制修改范围，并在结果中说明重要的界面或持久状态变化。
- 删除 Session 或脚本、启停代理、重新生成 CA、清空日志或记录、修改 workspace 或应用配置等高影响操作，必须先得到用户明确要求。
- 不要覆盖用户尚未保存的编辑内容；遇到外部修改冲突时保留用户内容。
"#;

pub const QUIET_INVESTIGATION_CONTENT: &str = r#"# ProxyCrab Agent 行为：静默排查

## 指令优先级

- 当前用户的明确要求高于本预设；发生冲突时，以用户要求为准。

## 排查方式

- 默认只读取现有 Session、捕获记录、脚本和配置，避免改变用户当前看到的界面或任何持久状态。
- `POST /api/logs/ids` 始终只读；只有用户明确要求保存时才调用独立的 Session filter 写接口。
- 不得自行创建、编辑、删除或切换 Session，不得修改 Session 视图、自定义列、脚本、分流选择、拦截器链、应用配置或其他会同步到桌面界面的状态。
- 如果继续排查确实需要产生界面或持久状态变化，先说明原因并请求用户授权。
- 不要覆盖用户尚未保存的编辑内容。
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PresetMetadata {
    id: String,
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PresetsConfig {
    active_id: String,
    presets: Vec<PresetMetadata>,
}

pub struct AgentsStore {
    root: PathBuf,
    operation: Mutex<()>,
}

impl AgentsStore {
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            root: workspace_root.join("agents"),
            operation: Mutex::new(()),
        }
    }

    pub fn initialize(&self) -> Result<()> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        self.initialize_unlocked()
    }

    pub fn state(&self) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let config = self.load_config_unlocked()?;
        self.state_unlocked(config)
    }

    pub fn active_markdown(&self) -> Result<String> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let config = self.load_config_unlocked()?;
        fs::read_to_string(self.preset_path(&config.active_id))
            .context("active AGENTS.md preset is unavailable")
    }

    pub fn create(&self, name: String) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let mut config = self.load_config_unlocked()?;
        let name = validate_name(&name)?;
        require_unique_name(&config, &name, None)?;
        let id = next_id(&self.root, &config);
        write_text_atomic(&self.preset_path(&id), "")?;
        config.presets.push(PresetMetadata { id, name });
        self.save_config(&config)?;
        self.state_unlocked(config)
    }

    pub fn update(
        &self,
        id: &str,
        name: Option<String>,
        content: Option<String>,
    ) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let mut config = self.load_config_unlocked()?;
        let index = preset_index(&config, id)?;
        if let Some(name) = name {
            let name = validate_name(&name)?;
            require_unique_name(&config, &name, Some(id))?;
            config.presets[index].name = name;
        }
        if let Some(content) = content {
            write_text_atomic(&self.preset_path(id), &content)?;
        }
        self.save_config(&config)?;
        self.state_unlocked(config)
    }

    pub fn activate(&self, id: &str) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let mut config = self.load_config_unlocked()?;
        preset_index(&config, id)?;
        config.active_id = id.to_string();
        self.save_config(&config)?;
        self.state_unlocked(config)
    }

    pub fn delete(&self, id: &str) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let mut config = self.load_config_unlocked()?;
        if config.presets.len() == 1 {
            bail!("cannot delete the final AGENTS.md preset");
        }
        let index = preset_index(&config, id)?;
        config.presets.remove(index);
        if config.active_id == id {
            config.active_id = config.presets[index.min(config.presets.len() - 1)]
                .id
                .clone();
        }
        self.save_config(&config)?;
        let _ = fs::remove_file(self.preset_path(id));
        self.state_unlocked(config)
    }

    pub fn reimport_defaults(&self) -> Result<AgentsPresetState> {
        let _operation = self
            .operation
            .lock()
            .expect("AGENTS.md store lock poisoned");
        let mut config = self.load_config_unlocked()?;
        for (default_id, name, content) in default_presets() {
            let id = match config.presets.iter().find(|preset| preset.name == name) {
                Some(preset) => preset.id.clone(),
                None => {
                    let id = if config.presets.iter().all(|preset| preset.id != default_id) {
                        default_id.to_string()
                    } else {
                        next_id(&self.root, &config)
                    };
                    config.presets.push(PresetMetadata {
                        id: id.clone(),
                        name: name.to_string(),
                    });
                    id
                }
            };
            write_text_atomic(&self.preset_path(&id), content)?;
        }
        self.save_config(&config)?;
        self.state_unlocked(config)
    }

    fn initialize_unlocked(&self) -> Result<()> {
        fs::create_dir_all(self.root.join(PRESETS_DIRECTORY))?;
        let config_path = self.root.join(CONFIG_FILE);
        if config_path.exists() {
            return Ok(());
        }
        let presets = default_presets()
            .into_iter()
            .map(|(id, name, content)| {
                write_text_atomic(&self.preset_path(id), content)?;
                Ok(PresetMetadata {
                    id: id.to_string(),
                    name: name.to_string(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.save_config(&PresetsConfig {
            active_id: FULL_CAPABILITY_ID.to_string(),
            presets,
        })
    }

    fn load_config_unlocked(&self) -> Result<PresetsConfig> {
        self.initialize_unlocked()?;
        let config = read_json(&self.root.join(CONFIG_FILE))?;
        validate_config(&self.root, &config)?;
        Ok(config)
    }

    fn state_unlocked(&self, config: PresetsConfig) -> Result<AgentsPresetState> {
        let presets = config
            .presets
            .iter()
            .map(|preset| {
                Ok(AgentsPreset {
                    id: preset.id.clone(),
                    name: preset.name.clone(),
                    content: fs::read_to_string(self.preset_path(&preset.id)).with_context(
                        || format!("AGENTS.md preset {} is unavailable", preset.name),
                    )?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(AgentsPresetState {
            active_id: config.active_id,
            presets,
        })
    }

    fn preset_path(&self, id: &str) -> PathBuf {
        self.root.join(PRESETS_DIRECTORY).join(format!("{id}.md"))
    }

    fn save_config(&self, config: &PresetsConfig) -> Result<()> {
        write_json_atomic(&self.root.join(CONFIG_FILE), config)
    }
}

fn default_presets() -> [(&'static str, &'static str, &'static str); 2] {
    [
        (
            FULL_CAPABILITY_ID,
            FULL_CAPABILITY_NAME,
            FULL_CAPABILITY_CONTENT,
        ),
        (
            QUIET_INVESTIGATION_ID,
            QUIET_INVESTIGATION_NAME,
            QUIET_INVESTIGATION_CONTENT,
        ),
    ]
}

fn validate_config(root: &Path, config: &PresetsConfig) -> Result<()> {
    if config.presets.is_empty() {
        bail!("AGENTS.md preset config must contain at least one preset");
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for preset in &config.presets {
        if !ids.insert(&preset.id) || !names.insert(&preset.name) {
            bail!("AGENTS.md preset config contains duplicate entries");
        }
        validate_id(&preset.id)?;
        validate_name(&preset.name)?;
        if !root
            .join(PRESETS_DIRECTORY)
            .join(format!("{}.md", preset.id))
            .is_file()
        {
            bail!("AGENTS.md preset {} is unavailable", preset.name);
        }
    }
    if !ids.contains(&config.active_id) {
        bail!("active AGENTS.md preset is not present in config");
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        bail!("invalid AGENTS.md preset ID");
    }
    Ok(())
}

fn validate_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 128 {
        bail!("AGENTS.md preset name must contain 1 to 128 characters");
    }
    Ok(name.to_string())
}

fn require_unique_name(config: &PresetsConfig, name: &str, except_id: Option<&str>) -> Result<()> {
    if config
        .presets
        .iter()
        .any(|preset| preset.name == name && Some(preset.id.as_str()) != except_id)
    {
        bail!("AGENTS.md preset name already exists");
    }
    Ok(())
}

fn preset_index(config: &PresetsConfig, id: &str) -> Result<usize> {
    config
        .presets
        .iter()
        .position(|preset| preset.id == id)
        .with_context(|| format!("AGENTS.md preset {id} not found"))
}

fn next_id(root: &Path, config: &PresetsConfig) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = format!("preset-{timestamp}");
    let mut suffix = 0_u64;
    loop {
        let candidate = if suffix == 0 {
            base.clone()
        } else {
            format!("{base}-{suffix}")
        };
        let configured = config.presets.iter().any(|preset| preset.id == candidate);
        let exists = root
            .join(PRESETS_DIRECTORY)
            .join(format!("{candidate}.md"))
            .exists();
        if !configured && !exists {
            return candidate;
        }
        suffix += 1;
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> Result<()> {
    write_bytes_atomic(path, &serde_json::to_vec_pretty(value)?)
}

fn write_text_atomic(path: &Path, content: &str) -> Result<()> {
    write_bytes_atomic(path, content.as_bytes())
}

fn write_bytes_atomic(path: &Path, content: &[u8]) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    file.write_all(content)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{
        AgentsStore, FULL_CAPABILITY_CONTENT, FULL_CAPABILITY_NAME, QUIET_INVESTIGATION_CONTENT,
        QUIET_INVESTIGATION_NAME,
    };

    #[test]
    fn initializes_default_presets_and_manages_their_lifecycle() {
        let root = tempdir().unwrap();
        let store = AgentsStore::new(root.path());
        store.initialize().unwrap();
        let state = store.state().unwrap();
        assert_eq!(state.presets.len(), 2);
        assert_eq!(store.active_markdown().unwrap(), FULL_CAPABILITY_CONTENT);
        std::fs::write(
            root.path().join("agents/presets/full-capability.md"),
            "# External edit",
        )
        .unwrap();
        assert_eq!(store.active_markdown().unwrap(), "# External edit");
        store.reimport_defaults().unwrap();

        let state = store.create("自定义".into()).unwrap();
        let id = state
            .presets
            .iter()
            .find(|preset| preset.name == "自定义")
            .unwrap()
            .id
            .clone();
        store
            .update(&id, Some("已重命名".into()), Some("# Custom".into()))
            .unwrap();
        store.activate(&id).unwrap();
        assert_eq!(store.active_markdown().unwrap(), "# Custom");
        let state = store.delete(&id).unwrap();
        assert_ne!(state.active_id, id);

        let full = state
            .presets
            .iter()
            .find(|preset| preset.name == FULL_CAPABILITY_NAME)
            .unwrap();
        store
            .update(&full.id, None, Some("changed".into()))
            .unwrap();
        let active_before = store.state().unwrap().active_id;
        let state = store.reimport_defaults().unwrap();
        assert_eq!(state.active_id, active_before);
        assert_eq!(
            state
                .presets
                .iter()
                .find(|preset| preset.name == FULL_CAPABILITY_NAME)
                .unwrap()
                .content,
            FULL_CAPABILITY_CONTENT
        );
        assert_eq!(
            state
                .presets
                .iter()
                .find(|preset| preset.name == QUIET_INVESTIGATION_NAME)
                .unwrap()
                .content,
            QUIET_INVESTIGATION_CONTENT
        );
    }

    #[test]
    fn final_preset_cannot_be_deleted() {
        let root = tempdir().unwrap();
        let store = AgentsStore::new(root.path());
        let state = store.state().unwrap();
        store.delete(&state.presets[1].id).unwrap();
        drop(store);

        let reopened = AgentsStore::new(root.path());
        reopened.initialize().unwrap();
        let state = reopened.state().unwrap();
        assert_eq!(state.presets.len(), 1);
        assert!(reopened.delete(&state.presets[0].id).is_err());
    }
}
