use std::{
    fs::{self, File},
    io::{ErrorKind, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::State,
    http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE, REFERRER_POLICY},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ProxyCrabManager,
    dto::{ManagerError, ManagerResult},
};

const CONFIG_DIRECTORY: &str = "clash";
const CONFIG_FILE: &str = "config.yaml";
const MAX_CONFIG_BYTES: usize = 1024 * 1024;
const SERVER_PLACEHOLDER: &str = "{{server}}";
const PORT_PLACEHOLDER: &str = "{{port}}";
const DEFAULT_CONFIG: &str = r#"# ProxyCrab Clash subscription
# The server and port placeholders are replaced when Clash downloads this file.
mode: rule
proxies:
  - name: ProxyCrab
    type: http
    server: "{{server}}"
    port: {{port}}
rules:
  - MATCH,ProxyCrab
"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClashSubscriptionState {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnableClashSubscriptionRequest {
    pub server: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClashConfig {
    pub yaml: String,
    pub default_yaml: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveClashConfigRequest {
    pub yaml: String,
}

pub(crate) struct ClashSubscription {
    state: Mutex<ClashSubscriptionState>,
    config_path: PathBuf,
}

impl ClashSubscription {
    pub fn new(workspace: &Path) -> Self {
        Self {
            state: Mutex::new(ClashSubscriptionState::default()),
            config_path: workspace.join(CONFIG_DIRECTORY).join(CONFIG_FILE),
        }
    }

    pub fn state(&self) -> ClashSubscriptionState {
        self.state
            .lock()
            .expect("Clash subscription lock poisoned")
            .clone()
    }

    pub fn enable(&self, server: &str) -> ManagerResult<ClashSubscriptionState> {
        let ip: Ipv4Addr = server
            .parse()
            .map_err(|_| ManagerError::bad_request("server must be a non-loopback IPv4 address"))?;
        if ip.octets()[0] == 0 || ip.is_loopback() || ip.is_multicast() || ip.is_broadcast() {
            return Err(ManagerError::bad_request(
                "server must be a non-loopback unicast IPv4 address",
            ));
        }
        let mut state = self.state.lock().expect("Clash subscription lock poisoned");
        state.enabled = true;
        state.server = Some(ip.to_string());
        Ok(state.clone())
    }

    pub fn disable(&self) -> ClashSubscriptionState {
        let mut state = self.state.lock().expect("Clash subscription lock poisoned");
        *state = ClashSubscriptionState::default();
        state.clone()
    }

    pub fn config(&self) -> ManagerResult<ClashConfig> {
        Ok(ClashConfig {
            yaml: self.config_yaml()?,
            default_yaml: DEFAULT_CONFIG.to_owned(),
        })
    }

    pub fn save_config(&self, yaml: &str) -> ManagerResult<ClashConfig> {
        validate_config(yaml)?;
        self.write_config(yaml)?;
        self.config()
    }

    pub fn reset_config(&self) -> ManagerResult<ClashConfig> {
        self.write_config(DEFAULT_CONFIG)?;
        self.config()
    }

    pub fn yaml(&self, port: u16) -> ManagerResult<String> {
        let server = {
            let state = self.state.lock().expect("Clash subscription lock poisoned");
            if !state.enabled {
                return Err(invalid_link());
            }
            state.server.clone().ok_or_else(invalid_link)?
        };
        let template = self.config_yaml()?;
        Ok(template
            .replace(SERVER_PLACEHOLDER, &server)
            .replace(PORT_PLACEHOLDER, &port.to_string()))
    }

    fn config_yaml(&self) -> ManagerResult<String> {
        match fs::read_to_string(&self.config_path) {
            Ok(yaml) => Ok(yaml),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                self.write_config(DEFAULT_CONFIG)?;
                Ok(DEFAULT_CONFIG.to_owned())
            }
            Err(error) => Err(ManagerError::internal(format!(
                "read Clash config {}: {error}",
                self.config_path.display()
            ))),
        }
    }

    fn write_config(&self, yaml: &str) -> ManagerResult<()> {
        let directory = self.config_path.parent().ok_or_else(|| {
            ManagerError::internal("Clash config path must have a parent directory")
        })?;
        fs::create_dir_all(directory).map_err(|error| {
            ManagerError::internal(format!(
                "create Clash config directory {}: {error}",
                directory.display()
            ))
        })?;
        let temporary = self.config_path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let mut file = File::create(&temporary)?;
            file.write_all(yaml.as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.config_path)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(&temporary);
            return Err(ManagerError::internal(format!(
                "write Clash config {}: {error}",
                self.config_path.display()
            )));
        }
        Ok(())
    }
}

fn validate_config(yaml: &str) -> ManagerResult<()> {
    if yaml.len() > MAX_CONFIG_BYTES {
        return Err(ManagerError::bad_request(format!(
            "Clash config must be at most {MAX_CONFIG_BYTES} bytes"
        )));
    }
    if !yaml.contains(SERVER_PLACEHOLDER) || !yaml.contains(PORT_PLACEHOLDER) {
        return Err(ManagerError::bad_request(
            "Clash config must contain both {{server}} and {{port}} placeholders",
        ));
    }
    let rendered = yaml
        .replace(SERVER_PLACEHOLDER, "192.0.2.1")
        .replace(PORT_PLACEHOLDER, "8089");
    let value: Value = serde_yaml_ng::from_str(&rendered)
        .map_err(|error| ManagerError::bad_request(format!("invalid Clash YAML: {error}")))?;
    let root = value
        .as_object()
        .ok_or_else(|| ManagerError::bad_request("Clash config must be a YAML mapping"))?;
    let proxies = root
        .get("proxies")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ManagerError::bad_request("Clash config requires a non-empty proxies list")
        })?;
    if proxies.is_empty() {
        return Err(ManagerError::bad_request(
            "Clash config requires a non-empty proxies list",
        ));
    }
    for proxy in proxies {
        let proxy = proxy.as_object().ok_or_else(|| {
            ManagerError::bad_request("Clash proxy entries must be mappings with name and type")
        })?;
        let name = proxy
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        let kind = proxy
            .get("type")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default();
        if name.is_empty() || kind.is_empty() {
            return Err(ManagerError::bad_request(
                "Clash proxy entries must be mappings with name and type",
            ));
        }
    }
    let rules = root
        .get("rules")
        .and_then(Value::as_array)
        .ok_or_else(|| ManagerError::bad_request("Clash config requires a non-empty rules list"))?;
    if rules.is_empty() {
        return Err(ManagerError::bad_request(
            "Clash config requires a non-empty rules list",
        ));
    }
    for rule in rules {
        let rule = rule
            .as_str()
            .map(str::trim)
            .ok_or_else(|| ManagerError::bad_request("Clash rules must be non-empty strings"))?;
        if rule.is_empty() {
            return Err(ManagerError::bad_request(
                "Clash rules must be non-empty strings",
            ));
        }
    }
    let target = rules
        .last()
        .and_then(Value::as_str)
        .map(str::trim)
        .and_then(|rule| rule.strip_prefix("MATCH,"))
        .map(str::trim)
        .unwrap_or_default();
    if target.is_empty() {
        return Err(ManagerError::bad_request(
            "the final Clash rule must be MATCH,<target>",
        ));
    }
    Ok(())
}

fn invalid_link() -> ManagerError {
    ManagerError::not_found("Clash subscription is invalid or disabled")
}

pub fn router(manager: Arc<dyn ProxyCrabManager>) -> Router {
    Router::new()
        .route("/clash.yaml", get(download))
        .with_state(manager)
}

async fn download(State(manager): State<Arc<dyn ProxyCrabManager>>) -> Response {
    let result = manager.clash_subscription_yaml().await;
    let mut response = match result {
        Ok(yaml) => (
            [
                (CONTENT_TYPE, "application/yaml; charset=utf-8"),
                (
                    CONTENT_DISPOSITION,
                    "attachment; filename=\"ProxyCrab.yaml\"",
                ),
            ],
            yaml,
        )
            .into_response(),
        Err(error) => (
            error.code.http_status(),
            Json(serde_json::json!({"ok": false, "error": error})),
        )
            .into_response(),
    };
    response
        .headers_mut()
        .insert(CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    fn subscription() -> (tempfile::TempDir, ClashSubscription) {
        let directory = tempdir().unwrap();
        let subscription = ClashSubscription::new(directory.path());
        (directory, subscription)
    }

    #[test]
    fn subscription_is_ephemeral_updatable_and_revocable() {
        let (_directory, subscription) = subscription();
        assert!(!subscription.state().enabled);
        assert!(subscription.yaml(8089).is_err());
        let first = subscription.enable("192.168.1.20").unwrap();
        assert!(first.enabled);
        assert_eq!(first.server.as_deref(), Some("192.168.1.20"));
        assert_eq!(subscription.state(), first);
        assert_eq!(subscription.enable("192.168.1.20").unwrap(), first);
        let yaml = subscription.yaml(8089).unwrap();
        assert!(yaml.contains("server: \"192.168.1.20\""));
        assert!(yaml.contains("port: 8089"));
        assert!(yaml.ends_with("rules:\n  - MATCH,ProxyCrab\n"));
        subscription.enable("10.0.0.2").unwrap();
        let yaml = subscription.yaml(9999).unwrap();
        assert!(yaml.contains("server: \"10.0.0.2\""));
        assert!(yaml.contains("port: 9999"));
        assert_eq!(subscription.disable(), ClashSubscriptionState::default());
        assert!(subscription.yaml(8089).is_err());
        assert!(subscription.enable("10.0.0.2").unwrap().enabled);
        assert_eq!(
            ClashSubscription::new(Path::new("/tmp")).state(),
            ClashSubscriptionState::default()
        );
    }

    #[test]
    fn invalid_addresses_do_not_change_subscription() {
        let (_directory, subscription) = subscription();
        let state = subscription.enable("192.168.1.20").unwrap();
        for server in [
            "127.0.0.1",
            "127.0.0.2",
            "0.0.0.0",
            "0.1.2.3",
            "::1",
            "localhost",
            "224.0.0.1",
            "255.255.255.255",
            "1.2.3.4\nrules: []",
        ] {
            assert!(subscription.enable(server).is_err(), "{server}");
            assert_eq!(subscription.state(), state);
        }
    }

    #[test]
    fn config_defaults_validate_and_reset() {
        let (directory, subscription) = subscription();
        let config = subscription.config().unwrap();
        assert_eq!(config.default_yaml, DEFAULT_CONFIG);
        assert_eq!(config.yaml, DEFAULT_CONFIG);
        assert!(directory.path().join("clash").join("config.yaml").is_file());
        assert!(serde_json::to_value(&config).unwrap().get("path").is_none());

        let custom = DEFAULT_CONFIG.replace("MATCH,ProxyCrab", "MATCH,DIRECT");
        let saved = subscription.save_config(&custom).unwrap();
        assert_eq!(saved.yaml, custom);
        assert_eq!(subscription.config().unwrap().yaml, custom);
        assert_eq!(subscription.reset_config().unwrap().yaml, DEFAULT_CONFIG);
    }

    #[test]
    fn config_validation_rejects_missing_placeholders_and_bad_structure() {
        let (_directory, subscription) = subscription();
        let missing_server = DEFAULT_CONFIG.replace(SERVER_PLACEHOLDER, "192.168.1.20");
        let missing_port = DEFAULT_CONFIG.replace(PORT_PLACEHOLDER, "8089");
        for yaml in [
            String::new(),
            missing_server,
            missing_port,
            "mode: [".to_owned(),
            "- not a mapping".to_owned(),
            "proxies: []\nrules:\n  - MATCH,ProxyCrab\n{{server}}{{port}}".to_owned(),
            "proxies:\n  - ProxyCrab\nrules:\n  - MATCH,ProxyCrab\n{{server}}{{port}}".to_owned(),
            "proxies:\n  - name: ProxyCrab\n    type: http\nrules: []\n{{server}}{{port}}".to_owned(),
            "proxies:\n  - name: ProxyCrab\n    type: http\nrules:\n  - 1\n{{server}}{{port}}"
                .to_owned(),
            "proxies:\n  - name: ProxyCrab\n    type: http\nrules:\n  - DOMAIN,example.com,ProxyCrab\n{{server}}{{port}}"
                .to_owned(),
            "proxies:\n  - name: ProxyCrab\n    type: http\nrules:\n  - MATCH,\n{{server}}{{port}}"
                .to_owned(),
            "#".repeat(MAX_CONFIG_BYTES + 1),
        ] {
            assert!(subscription.save_config(&yaml).is_err(), "{yaml}");
        }
        assert_eq!(subscription.config().unwrap().yaml, DEFAULT_CONFIG);
    }

    #[test]
    fn downloads_substitute_placeholders_without_revalidating_external_edits() {
        let (_directory, subscription) = subscription();
        subscription.enable("10.0.0.2").unwrap();
        let custom = DEFAULT_CONFIG.replace(
            "rules:\n  - MATCH,ProxyCrab\n",
            "rules:\n  - DOMAIN-SUFFIX,example.com,ProxyCrab\n  - MATCH,DIRECT\n",
        );
        subscription.save_config(&custom).unwrap();
        let yaml = subscription.yaml(9999).unwrap();
        assert!(yaml.contains("server: \"10.0.0.2\""));
        assert!(yaml.contains("port: 9999"));
        assert!(yaml.contains("DOMAIN-SUFFIX,example.com,ProxyCrab"));
        assert!(yaml.contains("MATCH,DIRECT"));

        fs::write(&subscription.config_path, "not: [valid").unwrap();
        assert_eq!(subscription.yaml(9999).unwrap(), "not: [valid");
    }

    #[test]
    fn config_persists_across_instances_without_persisting_subscription_state() {
        let directory = tempdir().unwrap();
        let custom = DEFAULT_CONFIG.replace("MATCH,ProxyCrab", "MATCH,DIRECT");
        ClashSubscription::new(directory.path())
            .save_config(&custom)
            .unwrap();
        let reopened = ClashSubscription::new(directory.path());
        assert_eq!(reopened.config().unwrap().yaml, custom);
        assert!(!reopened.state().enabled);
    }
}
