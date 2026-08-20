use std::collections::BTreeMap;

use proxy_crab_mgr::permission::PermissionMode;
use serde::{Deserialize, Serialize};

pub const PERMISSION_FILE_VERSION: u32 = 1;
pub const LOCAL_IDENTITY_ID: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionFile {
    pub version: u32,
    pub local: LocalPermissionRecord,
    pub api_keys: Vec<ApiKeyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPermissionRecord {
    pub permissions: BTreeMap<String, PermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiKeyRecord {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub salt: String,
    pub hash: String,
    pub created_at: u64,
    pub last_used_at: Option<u64>,
    pub permissions: BTreeMap<String, PermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionIdentityKind {
    Local,
    ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionIdentitySummary {
    pub id: String,
    pub kind: PermissionIdentityKind,
    pub name: String,
    pub prefix: Option<String>,
    pub created_at: Option<u64>,
    pub last_used_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionEntry {
    pub action_id: String,
    pub mode: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdentityPermissions {
    pub identity: PermissionIdentitySummary,
    pub permissions: Vec<PermissionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiActionView {
    pub id: String,
    pub method: String,
    pub route_template: String,
    pub default_mode: PermissionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreatedApiKey {
    pub identity: PermissionIdentitySummary,
    pub api_key: String,
}
