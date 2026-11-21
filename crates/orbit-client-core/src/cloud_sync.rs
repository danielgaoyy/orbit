//! Zero-Server, Zero-Database End-to-End Encrypted Cloud Sync via GitHub Secret Gist.
//!
//! Architecture (BYOS — Bring Your Own Storage):
//! - Identity & auth: GitHub OAuth Device Flow (RFC 8628, public `client_id`, zero backend server)
//!   with optional Personal Access Token (`gist` scope) fallback.
//! - Storage: A single user-owned GitHub Secret Gist (`orbit-cloud-vault.v1.enc.json`).
//! - Confidentiality: Client-side Argon2id + AES-256-GCM (`orbit_crypto::vault`), so GitHub only
//!   ever stores opaque ciphertext.
//! - Conflict resolution: Deterministic per-entity Last-Write-Wins (LWW) with soft-delete
//!   tombstones across paired hosts, pinned projects, project order, and user preferences.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_crypto::{
    DerivedVaultKey, EncryptedVaultEnvelope, VaultCryptoError, VaultKdfParams, decrypt_vault_json,
    derive_new_vault_key, derive_vault_key, encrypt_vault_json, generate_recovery_passphrase,
    validate_envelope_header,
};
use orbit_store::{OrbitStore, StoredClientProject, StoredHostMeta, StoredHostProfile};
use serde::{Deserialize, Serialize};

/// Filename inside the user's GitHub Secret Gist holding the encrypted vault envelope.
pub const CLOUD_VAULT_GIST_FILENAME: &str = "orbit-cloud-vault.v1.enc.json";
pub const CLOUD_VAULT_GIST_DESCRIPTION: &str =
    "Orbit Encrypted Cloud Sync Vault (E2EE — managed by Orbit, do not edit manually)";
pub const CLOUD_VAULT_SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_GITHUB_WEB_BASE: &str = "https://github.com";
pub const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";

/// Public GitHub OAuth App Client ID (RFC 8628 Device Flow — intentionally public, no secret).
/// Can be injected at compile time via `ORBIT_GITHUB_CLIENT_ID`, overridden at runtime via
/// `ORBIT_GITHUB_CLIENT_ID`, or provided in UI/CLI settings.
pub const DEFAULT_GITHUB_CLIENT_ID: &str = match option_env!("ORBIT_GITHUB_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

pub const KV_CLOUD_SYNC_CONFIG: &str = "cloud_sync_config_v1";
pub const KV_CLOUD_SYNC_META: &str = "cloud_sync_meta_v1";
pub const KV_CLOUD_SYNC_PREFS: &str = "cloud_sync_prefs_v1";

const USER_AGENT: &str = concat!("Orbit-CloudSync/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum CloudSyncError {
    #[error(
        "GitHub OAuth Client ID is not configured; provide a Client ID or sign in with a GitHub token (`gist` scope)"
    )]
    MissingClientId,
    #[error("not signed in to Cloud Sync")]
    NotSignedIn,
    #[error("cloud vault is locked — enter your Sync Passphrase to unlock")]
    VaultLocked,
    #[error(
        "cloud vault passphrase changed on another device — enter the updated Sync Passphrase to unlock"
    )]
    PassphraseRotationRequired,
    #[error("GitHub authentication failed: {0}")]
    Auth(String),
    #[error("GitHub token is missing the required `gist` scope")]
    MissingGistScope,
    #[error("network error communicating with GitHub: {0}")]
    Http(String),
    #[error("GitHub API error ({status}): {message}")]
    GitHubApi { status: u16, message: String },
    #[error("vault cryptography error: {0}")]
    Crypto(#[from] VaultCryptoError),
    #[error("invalid cloud vault format: {0}")]
    InvalidVault(String),
    #[error("storage error: {0}")]
    Storage(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSyncProviderKind {
    #[default]
    GithubGist,
}

impl CloudSyncProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GithubGist => "github_gist",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSyncState {
    #[default]
    SignedOut,
    Locked,
    Ready,
    Syncing,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudSyncDevicePollStatus {
    Pending,
    SlowDown,
    Authorized,
    Expired,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDeviceFlowStartView {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in_secs: u64,
    pub interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncDevicePollView {
    pub status: CloudSyncDevicePollStatus,
    pub interval_secs: Option<u64>,
    pub error: Option<String>,
    pub sync_status: Option<CloudSyncStatusView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncStatusView {
    pub state: CloudSyncState,
    pub provider: String,
    pub github_oauth_configured: bool,
    pub github_client_id_override: Option<String>,
    pub account_login: Option<String>,
    pub account_name: Option<String>,
    pub account_avatar_url: Option<String>,
    pub gist_id: Option<String>,
    pub gist_html_url: Option<String>,
    pub remote_vault_exists: bool,
    pub auto_sync_enabled: bool,
    pub last_synced_at_ms: Option<i64>,
    pub last_vault_revision: u64,
    pub synced_host_count: u32,
    pub synced_project_count: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudSyncResultView {
    pub hosts_added: u32,
    pub hosts_updated: u32,
    pub hosts_removed: u32,
    pub projects_added: u32,
    pub projects_updated: u32,
    pub projects_removed: u32,
    pub preferences_updated: u32,
    pub pushed_remote: bool,
    pub vault_revision: u64,
    pub synced_at_ms: i64,
    pub gist_html_url: Option<String>,
}

/// Encrypted payload stored inside the GitHub Secret Gist (serialized as JSON, compressed, AES-256-GCM encrypted).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudVaultPayload {
    pub schema_version: u32,
    pub revision: u64,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub hosts: HashMap<String, SyncedHostEntry>,
    #[serde(default)]
    pub projects: HashMap<String, SyncedProjectEntry>,
    #[serde(default)]
    pub project_order: Vec<String>,
    #[serde(default)]
    pub project_order_updated_at_ms: i64,
    #[serde(default)]
    pub preferences: HashMap<String, SyncedPreferenceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedHostEntry {
    pub host_id: String,
    pub hostname: String,
    pub daemon_pubkey_b64: String,
    #[serde(default)]
    pub direct_endpoints: Vec<String>,
    #[serde(default)]
    pub relay_url: Option<String>,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub ssh_alias: Option<String>,
    #[serde(default)]
    pub route_mode: Option<String>,
    #[serde(default)]
    pub remote_daemon_port: Option<u16>,
    #[serde(default)]
    pub custom_direct_endpoint: Option<String>,
    #[serde(default)]
    pub os_arch: Option<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedProjectEntry {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncedPreferenceEntry {
    pub value: serde_json::Value,
    pub updated_at_ms: i64,
}

/// Persisted local Cloud Sync account & cached unlock key (saved in `client_kv` or `~/.orbit/cloud-sync.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCloudSyncConfig {
    #[serde(default)]
    pub provider: CloudSyncProviderKind,
    #[serde(default)]
    pub github_access_token: Option<String>,
    #[serde(default)]
    pub github_login: Option<String>,
    #[serde(default)]
    pub github_name: Option<String>,
    #[serde(default)]
    pub github_avatar_url: Option<String>,
    #[serde(default)]
    pub github_client_id: Option<String>,
    #[serde(default)]
    pub gist_id: Option<String>,
    #[serde(default)]
    pub gist_html_url: Option<String>,
    #[serde(default)]
    pub cached_key_b64: Option<String>,
    #[serde(default)]
    pub cached_salt_b64: Option<String>,
    #[serde(default)]
    pub cached_kdf_params: Option<VaultKdfParams>,
    #[serde(default = "default_true")]
    pub auto_sync_enabled: bool,
    #[serde(default)]
    pub last_synced_at_ms: Option<i64>,
    #[serde(default)]
    pub last_vault_revision: u64,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub remote_vault_exists: bool,
    #[serde(default)]
    pub synced_host_count: u32,
    #[serde(default)]
    pub synced_project_count: u32,
}

fn default_true() -> bool {
    true
}

impl Default for PersistedCloudSyncConfig {
    fn default() -> Self {
        Self {
            provider: CloudSyncProviderKind::GithubGist,
            github_access_token: None,
            github_login: None,
            github_name: None,
            github_avatar_url: None,
            github_client_id: None,
            gist_id: None,
            gist_html_url: None,
            cached_key_b64: None,
            cached_salt_b64: None,
            cached_kdf_params: None,
            auto_sync_enabled: true,
            last_synced_at_ms: None,
            last_vault_revision: 0,
            last_error: None,
            remote_vault_exists: false,
            synced_host_count: 0,
            synced_project_count: 0,
        }
    }
}

impl PersistedCloudSyncConfig {
    pub fn is_signed_in(&self) -> bool {
        self.github_access_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty())
    }

    pub fn derived_key(&self) -> Option<DerivedVaultKey> {
        let key_b64 = self.cached_key_b64.as_deref()?;
        let salt_b64 = self.cached_salt_b64.as_ref()?.clone();
        let params = self.cached_kdf_params.unwrap_or_default();
        DerivedVaultKey::from_b64(key_b64, salt_b64, params).ok()
    }

    pub fn set_derived_key(&mut self, key: &DerivedVaultKey) {
        self.cached_key_b64 = Some(key.key_b64().to_string());
        self.cached_salt_b64 = Some(key.salt_b64.clone());
        self.cached_kdf_params = Some(key.kdf_params);
    }

    pub fn clear_derived_key(&mut self) {
        self.cached_key_b64 = None;
        self.cached_salt_b64 = None;
        self.cached_kdf_params = None;
    }

    pub fn to_status_view(&self, is_syncing: bool) -> CloudSyncStatusView {
        let state = if !self.is_signed_in() {
            CloudSyncState::SignedOut
        } else if is_syncing {
            CloudSyncState::Syncing
        } else if self.derived_key().is_none() {
            CloudSyncState::Locked
        } else if self.last_error.is_some() {
            CloudSyncState::Error
        } else {
            CloudSyncState::Ready
        };
        let resolved_cid = resolve_github_client_id(None, self.github_client_id.as_deref());
        CloudSyncStatusView {
            state,
            provider: self.provider.as_str().to_string(),
            github_oauth_configured: resolved_cid.is_some(),
            github_client_id_override: self.github_client_id.clone(),
            account_login: self.github_login.clone(),
            account_name: self.github_name.clone(),
            account_avatar_url: self.github_avatar_url.clone(),
            gist_id: self.gist_id.clone(),
            gist_html_url: self.gist_html_url.clone(),
            remote_vault_exists: self.remote_vault_exists,
            auto_sync_enabled: self.auto_sync_enabled,
            last_synced_at_ms: self.last_synced_at_ms,
            last_vault_revision: self.last_vault_revision,
            synced_host_count: self.synced_host_count,
            synced_project_count: self.synced_project_count,
            last_error: self.last_error.clone(),
        }
    }
}

/// Local per-entity change timestamps & soft-delete tombstones so removals propagate cleanly across devices.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CloudSyncLocalMeta {
    #[serde(default)]
    pub host_updated_at_ms: HashMap<String, i64>,
    #[serde(default)]
    pub host_deleted_at_ms: HashMap<String, i64>,
    #[serde(default)]
    pub project_deleted_at_ms: HashMap<String, i64>,
    #[serde(default)]
    pub project_order_updated_at_ms: i64,
}

impl CloudSyncLocalMeta {
    pub fn record_host_mutated(&mut self, host_id: &str, now_ms: i64) {
        self.host_updated_at_ms.insert(host_id.to_string(), now_ms);
        self.host_deleted_at_ms.remove(host_id);
    }

    pub fn record_host_deleted(&mut self, host_id: &str, now_ms: i64) {
        self.host_deleted_at_ms.insert(host_id.to_string(), now_ms);
        self.host_updated_at_ms.remove(host_id);
    }

    pub fn record_project_mutated(&mut self, project_id: &str) {
        self.project_deleted_at_ms.remove(project_id);
    }

    pub fn record_project_deleted(&mut self, project_id: &str, now_ms: i64) {
        self.project_deleted_at_ms
            .insert(project_id.to_string(), now_ms);
    }

    pub fn record_project_order_mutated(&mut self, now_ms: i64) {
        self.project_order_updated_at_ms = now_ms;
    }
}

/// Snapshot of a locally stored host ready for LWW merge.
#[derive(Debug, Clone)]
pub struct LocalHostSyncRecord {
    pub profile: StoredHostProfile,
    pub os_arch: Option<String>,
    pub updated_at_ms: i64,
}

/// Result of merging local state with an optional remote `CloudVaultPayload`.
#[derive(Debug, Clone)]
pub struct VaultMergeOutcome {
    pub merged: CloudVaultPayload,
    pub hosts_to_upsert: Vec<SyncedHostEntry>,
    pub hosts_to_remove: Vec<String>,
    pub projects_to_upsert: Vec<SyncedProjectEntry>,
    pub projects_to_remove: Vec<String>,
    pub project_order_changed: Option<(Vec<String>, i64)>,
    pub preferences_to_apply: HashMap<String, SyncedPreferenceEntry>,
    pub needs_push: bool,
    pub hosts_added_count: u32,
    pub hosts_updated_count: u32,
    pub hosts_removed_count: u32,
    pub projects_added_count: u32,
    pub projects_updated_count: u32,
    pub projects_removed_count: u32,
    pub preferences_updated_count: u32,
}

pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Generate a fresh high-entropy human-readable Recovery Key (`orbit-XXXX-XXXX-XXXX-XXXX-XXXX`).
pub fn generate_sync_passphrase() -> String {
    generate_recovery_passphrase()
}

/// Resolve the effective GitHub OAuth App Client ID in priority order:
/// 1. Explicit caller override
/// 2. Runtime `ORBIT_GITHUB_CLIENT_ID` environment variable
/// 3. Previously saved override in local config
/// 4. Compile-time `DEFAULT_GITHUB_CLIENT_ID`
pub fn resolve_github_client_id(explicit: Option<&str>, saved: Option<&str>) -> Option<String> {
    if let Some(id) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(id.to_string());
    }
    if let Ok(env_id) = std::env::var("ORBIT_GITHUB_CLIENT_ID") {
        let trimmed = env_id.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    if let Some(id) = saved.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(id.to_string());
    }
    let compiled = DEFAULT_GITHUB_CLIENT_ID.trim();
    if !compiled.is_empty() {
        return Some(compiled.to_string());
    }
    None
}

/// Strip machine-local loopback URLs (`127.0.0.1`, `localhost`, `[::1]`) before uploading to the
/// cloud vault so another device never mistakes Machine A's loopback address for itself.
pub fn filter_shareable_endpoints(endpoints: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for raw in endpoints {
        let trimmed = raw.trim();
        if trimmed.is_empty() || is_loopback_endpoint(trimmed) {
            continue;
        }
        if seen.insert(trimmed.to_string()) {
            out.push(trimmed.to_string());
        }
    }
    out
}

pub fn is_loopback_endpoint(endpoint: &str) -> bool {
    let lower = endpoint.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("ws://")
        .or_else(|| lower.strip_prefix("wss://"))
        .or_else(|| lower.strip_prefix("http://"))
        .or_else(|| lower.strip_prefix("https://"))
        .unwrap_or(&lower);
    let authority = rest.split('/').next().unwrap_or("");
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .unwrap_or("")
            .trim_start_matches('[')
    } else {
        authority.split(':').next().unwrap_or("")
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1" | "0.0.0.0") || host.starts_with("127.")
}

pub fn synced_host_from_local(rec: &LocalHostSyncRecord) -> SyncedHostEntry {
    SyncedHostEntry {
        host_id: rec.profile.host_id.clone(),
        hostname: rec.profile.display_name.clone(),
        daemon_pubkey_b64: rec.profile.daemon_pubkey_b64.clone(),
        direct_endpoints: filter_shareable_endpoints(&rec.profile.direct_endpoints),
        relay_url: rec
            .profile
            .relay_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        custom_name: rec.profile.meta.custom_name.clone(),
        ssh_alias: rec.profile.meta.ssh_alias.clone(),
        route_mode: rec.profile.meta.route_mode.clone(),
        remote_daemon_port: rec.profile.meta.remote_daemon_port,
        custom_direct_endpoint: rec.profile.meta.custom_direct_endpoint.clone(),
        os_arch: rec.os_arch.clone(),
        updated_at_ms: rec.updated_at_ms,
        deleted_at_ms: None,
    }
}

pub fn stored_host_from_synced(entry: &SyncedHostEntry) -> StoredHostProfile {
    StoredHostProfile {
        host_id: entry.host_id.clone(),
        display_name: entry.hostname.clone(),
        daemon_pubkey_b64: entry.daemon_pubkey_b64.clone(),
        direct_endpoints: entry.direct_endpoints.clone(),
        relay_url: entry.relay_url.clone(),
        meta: StoredHostMeta {
            custom_name: entry.custom_name.clone(),
            ssh_alias: entry.ssh_alias.clone(),
            route_mode: entry.route_mode.clone(),
            remote_daemon_port: entry.remote_daemon_port,
            custom_direct_endpoint: entry.custom_direct_endpoint.clone(),
        },
    }
}

fn host_effective_ts(entry: &SyncedHostEntry) -> i64 {
    entry.deleted_at_ms.unwrap_or(entry.updated_at_ms)
}

fn project_effective_ts(entry: &SyncedProjectEntry) -> i64 {
    entry.deleted_at_ms.unwrap_or(entry.updated_at_ms)
}

fn hosts_content_equal(a: &SyncedHostEntry, b: &SyncedHostEntry) -> bool {
    a.hostname == b.hostname
        && a.daemon_pubkey_b64 == b.daemon_pubkey_b64
        && a.direct_endpoints == b.direct_endpoints
        && a.relay_url == b.relay_url
        && a.custom_name == b.custom_name
        && a.ssh_alias == b.ssh_alias
        && a.route_mode == b.route_mode
        && a.remote_daemon_port == b.remote_daemon_port
        && a.custom_direct_endpoint == b.custom_direct_endpoint
        && a.deleted_at_ms == b.deleted_at_ms
}

fn projects_content_equal(a: &SyncedProjectEntry, b: &SyncedProjectEntry) -> bool {
    a.host_id == b.host_id
        && a.name == b.name
        && a.directories == b.directories
        && a.deleted_at_ms == b.deleted_at_ms
}

/// Deterministic per-entity LWW merge between the decrypted remote vault and current local state.
#[allow(clippy::too_many_arguments)]
pub fn merge_cloud_vault(
    remote: Option<&CloudVaultPayload>,
    local_hosts: &[LocalHostSyncRecord],
    local_projects: &[StoredClientProject],
    local_project_order: &[String],
    local_preferences: &HashMap<String, SyncedPreferenceEntry>,
    local_meta: &CloudSyncLocalMeta,
    now_ms: i64,
) -> VaultMergeOutcome {
    let mut merged = CloudVaultPayload {
        schema_version: CLOUD_VAULT_SCHEMA_VERSION,
        revision: remote.map(|r| r.revision).unwrap_or(0),
        updated_at_ms: now_ms,
        hosts: HashMap::new(),
        projects: HashMap::new(),
        project_order: Vec::new(),
        project_order_updated_at_ms: 0,
        preferences: HashMap::new(),
    };

    let mut needs_push = remote.is_none();
    let mut hosts_to_upsert = Vec::new();
    let mut hosts_to_remove = Vec::new();
    let mut hosts_added_count = 0u32;
    let mut hosts_updated_count = 0u32;
    let mut hosts_removed_count = 0u32;

    // 1. Merge Hosts
    let local_host_map: HashMap<&str, &LocalHostSyncRecord> = local_hosts
        .iter()
        .map(|h| (h.profile.host_id.as_str(), h))
        .collect();

    let mut all_host_ids: HashSet<String> = HashSet::new();
    if let Some(r) = remote {
        all_host_ids.extend(r.hosts.keys().cloned());
    }
    for h in local_hosts {
        all_host_ids.insert(h.profile.host_id.clone());
    }
    all_host_ids.extend(local_meta.host_deleted_at_ms.keys().cloned());

    for host_id in all_host_ids {
        let remote_entry = remote.and_then(|r| r.hosts.get(&host_id));
        let local_rec = local_host_map.get(host_id.as_str()).copied();
        let local_del_ts = local_meta.host_deleted_at_ms.get(&host_id).copied();

        let local_entry: Option<SyncedHostEntry> = match (local_rec, local_del_ts) {
            (Some(rec), Some(del_ts)) if del_ts > rec.updated_at_ms => Some(SyncedHostEntry {
                host_id: host_id.clone(),
                hostname: rec.profile.display_name.clone(),
                daemon_pubkey_b64: rec.profile.daemon_pubkey_b64.clone(),
                direct_endpoints: Vec::new(),
                relay_url: None,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                updated_at_ms: del_ts,
                deleted_at_ms: Some(del_ts),
            }),
            (Some(rec), _) => Some(synced_host_from_local(rec)),
            (None, Some(del_ts)) => Some(SyncedHostEntry {
                host_id: host_id.clone(),
                hostname: remote_entry.map(|r| r.hostname.clone()).unwrap_or_default(),
                daemon_pubkey_b64: remote_entry
                    .map(|r| r.daemon_pubkey_b64.clone())
                    .unwrap_or_default(),
                direct_endpoints: Vec::new(),
                relay_url: None,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                updated_at_ms: del_ts,
                deleted_at_ms: Some(del_ts),
            }),
            (None, None) => None,
        };

        let winner = match (remote_entry, local_entry) {
            (Some(r), Some(l)) => {
                let r_ts = host_effective_ts(r);
                let l_ts = host_effective_ts(&l);
                if r_ts > l_ts {
                    // Remote wins
                    if r.deleted_at_ms.is_some() {
                        if local_rec.is_some() {
                            hosts_to_remove.push(host_id.clone());
                            hosts_removed_count += 1;
                        }
                    } else if local_rec.is_none() {
                        hosts_to_upsert.push(r.clone());
                        hosts_added_count += 1;
                    } else if !hosts_content_equal(r, &l) {
                        hosts_to_upsert.push(r.clone());
                        hosts_updated_count += 1;
                    }
                    r.clone()
                } else if l_ts > r_ts {
                    // Local wins
                    if !hosts_content_equal(r, &l) {
                        needs_push = true;
                    }
                    l
                } else {
                    // Equal timestamp: if both active and content differs, union endpoints & prefer richer metadata
                    if r.deleted_at_ms.is_none() && l.deleted_at_ms.is_none() {
                        let mut combined = r.clone();
                        let mut eps = filter_shareable_endpoints(&combined.direct_endpoints);
                        for ep in filter_shareable_endpoints(&l.direct_endpoints) {
                            if !eps.contains(&ep) {
                                eps.push(ep);
                            }
                        }
                        combined.direct_endpoints = eps;
                        if combined.relay_url.is_none() {
                            combined.relay_url = l.relay_url.clone();
                        }
                        if combined.custom_name.is_none() {
                            combined.custom_name = l.custom_name.clone();
                        }
                        if combined.ssh_alias.is_none() {
                            combined.ssh_alias = l.ssh_alias.clone();
                        }
                        if !hosts_content_equal(&combined, r) {
                            needs_push = true;
                        }
                        if !hosts_content_equal(&combined, &l) {
                            hosts_to_upsert.push(combined.clone());
                            hosts_updated_count += 1;
                        }
                        combined
                    } else {
                        r.clone()
                    }
                }
            }
            (Some(r), None) => {
                if r.deleted_at_ms.is_none() {
                    hosts_to_upsert.push(r.clone());
                    hosts_added_count += 1;
                }
                r.clone()
            }
            (None, Some(l)) => {
                if l.deleted_at_ms.is_none() {
                    needs_push = true;
                }
                l
            }
            (None, None) => continue,
        };

        merged.hosts.insert(host_id, winner);
    }

    let tombstoned_hosts: HashSet<&str> = merged
        .hosts
        .values()
        .filter(|h| h.deleted_at_ms.is_some())
        .map(|h| h.host_id.as_str())
        .collect();

    // 2. Merge Pinned Client Projects
    let mut projects_to_upsert = Vec::new();
    let mut projects_to_remove = Vec::new();
    let mut projects_added_count = 0u32;
    let mut projects_updated_count = 0u32;
    let mut projects_removed_count = 0u32;

    let local_proj_map: HashMap<&str, &StoredClientProject> = local_projects
        .iter()
        .map(|p| (p.project_id.as_str(), p))
        .collect();

    let mut all_proj_ids: HashSet<String> = HashSet::new();
    if let Some(r) = remote {
        all_proj_ids.extend(r.projects.keys().cloned());
    }
    for p in local_projects {
        all_proj_ids.insert(p.project_id.clone());
    }
    all_proj_ids.extend(local_meta.project_deleted_at_ms.keys().cloned());

    for proj_id in all_proj_ids {
        let remote_entry = remote.and_then(|r| r.projects.get(&proj_id));
        let local_rec = local_proj_map.get(proj_id.as_str()).copied();
        let local_del_ts = local_meta.project_deleted_at_ms.get(&proj_id).copied();

        let local_entry: Option<SyncedProjectEntry> = match (local_rec, local_del_ts) {
            (Some(rec), Some(del_ts)) if del_ts > rec.updated_at_ms => Some(SyncedProjectEntry {
                project_id: proj_id.clone(),
                host_id: rec.host_id.clone(),
                name: rec.name.clone(),
                directories: Vec::new(),
                updated_at_ms: del_ts,
                deleted_at_ms: Some(del_ts),
            }),
            (Some(rec), _) => Some(SyncedProjectEntry {
                project_id: rec.project_id.clone(),
                host_id: rec.host_id.clone(),
                name: rec.name.clone(),
                directories: rec.directories.clone(),
                updated_at_ms: rec.updated_at_ms,
                deleted_at_ms: None,
            }),
            (None, Some(del_ts)) => Some(SyncedProjectEntry {
                project_id: proj_id.clone(),
                host_id: remote_entry.map(|r| r.host_id.clone()).unwrap_or_default(),
                name: remote_entry.map(|r| r.name.clone()).unwrap_or_default(),
                directories: Vec::new(),
                updated_at_ms: del_ts,
                deleted_at_ms: Some(del_ts),
            }),
            (None, None) => None,
        };

        let winner = match (remote_entry, local_entry) {
            (Some(r), Some(l)) => {
                let r_ts = project_effective_ts(r);
                let l_ts = project_effective_ts(&l);
                if r_ts > l_ts {
                    if r.deleted_at_ms.is_some() {
                        if local_rec.is_some() {
                            projects_to_remove.push(proj_id.clone());
                            projects_removed_count += 1;
                        }
                    } else if !tombstoned_hosts.contains(r.host_id.as_str()) {
                        if local_rec.is_none() {
                            projects_to_upsert.push(r.clone());
                            projects_added_count += 1;
                        } else if !projects_content_equal(r, &l) {
                            projects_to_upsert.push(r.clone());
                            projects_updated_count += 1;
                        }
                    }
                    r.clone()
                } else if l_ts > r_ts {
                    if !projects_content_equal(r, &l) {
                        needs_push = true;
                    }
                    l
                } else {
                    r.clone()
                }
            }
            (Some(r), None) => {
                if r.deleted_at_ms.is_none() && !tombstoned_hosts.contains(r.host_id.as_str()) {
                    projects_to_upsert.push(r.clone());
                    projects_added_count += 1;
                }
                r.clone()
            }
            (None, Some(l)) => {
                if l.deleted_at_ms.is_none() {
                    needs_push = true;
                }
                l
            }
            (None, None) => continue,
        };

        merged.projects.insert(proj_id, winner);
    }

    // 3. Merge Manual Project Order
    let remote_order_ts = remote.map(|r| r.project_order_updated_at_ms).unwrap_or(0);
    let local_order_ts = local_meta.project_order_updated_at_ms;
    let mut project_order_changed = None;
    if let Some(r) = remote {
        if remote_order_ts > local_order_ts
            || (remote_order_ts == local_order_ts
                && local_project_order.is_empty()
                && !r.project_order.is_empty())
        {
            merged.project_order = r.project_order.clone();
            merged.project_order_updated_at_ms = remote_order_ts;
            if r.project_order != local_project_order {
                project_order_changed = Some((r.project_order.clone(), remote_order_ts));
            }
        } else {
            merged.project_order = local_project_order.to_vec();
            merged.project_order_updated_at_ms = local_order_ts;
            if local_project_order != r.project_order.as_slice() {
                needs_push = true;
            }
        }
    } else {
        merged.project_order = local_project_order.to_vec();
        merged.project_order_updated_at_ms =
            local_order_ts.max(if local_project_order.is_empty() {
                0
            } else {
                now_ms
            });
    }

    // 4. Merge User Preferences
    let mut preferences_to_apply = HashMap::new();
    let mut preferences_updated_count = 0u32;
    let mut all_pref_keys: HashSet<String> = HashSet::new();
    if let Some(r) = remote {
        all_pref_keys.extend(r.preferences.keys().cloned());
    }
    all_pref_keys.extend(local_preferences.keys().cloned());

    for key in all_pref_keys {
        let r_pref = remote.and_then(|r| r.preferences.get(&key));
        let l_pref = local_preferences.get(&key);
        let winner = match (r_pref, l_pref) {
            (Some(r), Some(l)) => {
                if r.updated_at_ms > l.updated_at_ms {
                    if r.value != l.value {
                        preferences_to_apply.insert(key.clone(), r.clone());
                        preferences_updated_count += 1;
                    }
                    r.clone()
                } else if l.updated_at_ms > r.updated_at_ms {
                    if r.value != l.value {
                        needs_push = true;
                    }
                    l.clone()
                } else {
                    r.clone()
                }
            }
            (Some(r), None) => {
                preferences_to_apply.insert(key.clone(), r.clone());
                preferences_updated_count += 1;
                r.clone()
            }
            (None, Some(l)) => {
                needs_push = true;
                l.clone()
            }
            (None, None) => continue,
        };
        merged.preferences.insert(key, winner);
    }

    if needs_push {
        merged.revision = merged.revision.saturating_add(1);
    }

    VaultMergeOutcome {
        merged,
        hosts_to_upsert,
        hosts_to_remove,
        projects_to_upsert,
        projects_to_remove,
        project_order_changed,
        preferences_to_apply,
        needs_push,
        hosts_added_count,
        hosts_updated_count,
        hosts_removed_count,
        projects_added_count,
        projects_updated_count,
        projects_removed_count,
        preferences_updated_count,
    }
}

// --- Persistence Helpers (SQLite `client_kv` + Headless CLI `~/.orbit/cloud-sync.json`) ---

pub fn load_sync_config_from_store(store: &OrbitStore) -> PersistedCloudSyncConfig {
    store
        .get_client_kv(KV_CLOUD_SYNC_CONFIG)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_sync_config_to_store(
    store: &OrbitStore,
    config: &PersistedCloudSyncConfig,
) -> Result<(), CloudSyncError> {
    let raw = serde_json::to_string(config).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    store
        .set_client_kv(KV_CLOUD_SYNC_CONFIG, &raw, now_unix_ms())
        .map_err(|e| CloudSyncError::Storage(e.to_string()))
}

pub fn load_sync_meta_from_store(store: &OrbitStore) -> CloudSyncLocalMeta {
    store
        .get_client_kv(KV_CLOUD_SYNC_META)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_sync_meta_to_store(
    store: &OrbitStore,
    meta: &CloudSyncLocalMeta,
) -> Result<(), CloudSyncError> {
    let raw = serde_json::to_string(meta).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    store
        .set_client_kv(KV_CLOUD_SYNC_META, &raw, now_unix_ms())
        .map_err(|e| CloudSyncError::Storage(e.to_string()))
}

pub fn load_sync_prefs_from_store(store: &OrbitStore) -> HashMap<String, SyncedPreferenceEntry> {
    store
        .get_client_kv(KV_CLOUD_SYNC_PREFS)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_sync_prefs_to_store(
    store: &OrbitStore,
    prefs: &HashMap<String, SyncedPreferenceEntry>,
) -> Result<(), CloudSyncError> {
    let raw = serde_json::to_string(prefs).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    store
        .set_client_kv(KV_CLOUD_SYNC_PREFS, &raw, now_unix_ms())
        .map_err(|e| CloudSyncError::Storage(e.to_string()))
}

pub fn cli_sync_config_path(orbit_home: &Path) -> PathBuf {
    orbit_home.join("cloud-sync.json")
}

pub fn load_cli_sync_config(orbit_home: &Path) -> PersistedCloudSyncConfig {
    let path = cli_sync_config_path(orbit_home);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_cli_sync_config(
    orbit_home: &Path,
    config: &PersistedCloudSyncConfig,
) -> Result<(), CloudSyncError> {
    std::fs::create_dir_all(orbit_home).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    let path = cli_sync_config_path(orbit_home);
    let raw =
        serde_json::to_string_pretty(config).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    std::fs::write(&path, raw).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn delete_cli_sync_config(orbit_home: &Path) -> Result<(), CloudSyncError> {
    let path = cli_sync_config_path(orbit_home);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    }
    Ok(())
}

// --- GitHub OAuth Device Flow + Secret Gist HTTP Client ---

#[derive(Debug, Clone)]
pub struct GitHubUserProfile {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoteVaultGist {
    pub gist_id: String,
    pub html_url: String,
    pub envelope: EncryptedVaultEnvelope,
}

#[derive(Debug, Clone)]
pub enum DevicePollOutcome {
    Pending,
    SlowDown { interval_secs: u64 },
    Authorized { access_token: String },
    Expired,
    Denied,
}

#[derive(Debug, Clone)]
pub struct GitHubCloudClient {
    http: reqwest::Client,
    web_base: String,
    api_base: String,
}

impl Default for GitHubCloudClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GitHubCloudClient {
    pub fn new() -> Self {
        Self::with_bases(DEFAULT_GITHUB_WEB_BASE, DEFAULT_GITHUB_API_BASE)
    }

    pub fn with_bases(web_base: impl Into<String>, api_base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(25))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            http,
            web_base: web_base.into().trim_end_matches('/').to_string(),
            api_base: api_base.into().trim_end_matches('/').to_string(),
        }
    }

    /// Initiate GitHub OAuth 2.0 Device Authorization Grant (`scope=gist`).
    pub async fn start_device_flow(
        &self,
        client_id: &str,
    ) -> Result<GitHubDeviceFlowStartView, CloudSyncError> {
        let cid = client_id.trim();
        if cid.is_empty() {
            return Err(CloudSyncError::MissingClientId);
        }
        let url = format!("{}/login/device/code", self.web_base);
        let resp = self
            .http
            .post(&url)
            .header("Accept", "application/json")
            .form(&[("client_id", cid), ("scope", "gist")])
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;

        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid GitHub response: {e}")))?;

        if !status.is_success() || body.get("error").is_some() {
            let msg = body
                .get("error_description")
                .or_else(|| body.get("error"))
                .and_then(|v| v.as_str())
                .unwrap_or("failed to start GitHub Device Flow")
                .to_string();
            return Err(CloudSyncError::Auth(msg));
        }

        let device_code = body
            .get("device_code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CloudSyncError::Auth("missing device_code in GitHub response".into()))?
            .to_string();
        let user_code = body
            .get("user_code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CloudSyncError::Auth("missing user_code in GitHub response".into()))?
            .to_string();
        let verification_uri = body
            .get("verification_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("https://github.com/login/device")
            .to_string();
        let expires_in_secs = body
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(900);
        let interval_secs = body.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);

        Ok(GitHubDeviceFlowStartView {
            device_code,
            user_code,
            verification_uri,
            expires_in_secs,
            interval_secs,
        })
    }

    /// Perform one poll against GitHub's Device Flow token endpoint.
    pub async fn poll_device_flow_once(
        &self,
        client_id: &str,
        device_code: &str,
    ) -> Result<DevicePollOutcome, CloudSyncError> {
        let cid = client_id.trim();
        if cid.is_empty() {
            return Err(CloudSyncError::MissingClientId);
        }
        let url = format!("{}/login/oauth/access_token", self.web_base);
        let resp = self
            .http
            .post(&url)
            .header("Accept", "application/json")
            .form(&[
                ("client_id", cid),
                ("device_code", device_code.trim()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid token response: {e}")))?;

        if let Some(token) = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            return Ok(DevicePollOutcome::Authorized {
                access_token: token.to_string(),
            });
        }

        match body.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => Ok(DevicePollOutcome::Pending),
            Some("slow_down") => {
                let interval = body.get("interval").and_then(|v| v.as_u64()).unwrap_or(10);
                Ok(DevicePollOutcome::SlowDown {
                    interval_secs: interval,
                })
            }
            Some("expired_token") => Ok(DevicePollOutcome::Expired),
            Some("access_denied") => Ok(DevicePollOutcome::Denied),
            Some(other) => {
                let desc = body
                    .get("error_description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(other);
                Err(CloudSyncError::Auth(desc.to_string()))
            }
            None => Err(CloudSyncError::Auth(
                "unexpected empty response from GitHub OAuth".into(),
            )),
        }
    }

    /// Verify a GitHub access token and fetch the authenticated user's profile.
    pub async fn verify_token(&self, token: &str) -> Result<GitHubUserProfile, CloudSyncError> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return Err(CloudSyncError::Auth("GitHub token is empty".into()));
        }
        let url = format!("{}/user", self.api_base);
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {trimmed}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;

        let status = resp.status();
        // Classic OAuth / classic PAT responses expose `X-OAuth-Scopes`; fine-grained PATs omit it.
        let scopes_header = resp
            .headers()
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid user response: {e}")))?;

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("invalid GitHub token")
                .to_string();
            return Err(CloudSyncError::GitHubApi {
                status: status.as_u16(),
                message: msg,
            });
        }

        if let Some(ref scopes) = scopes_header {
            let has_gist = scopes
                .split(',')
                .map(str::trim)
                .any(|s| s.eq_ignore_ascii_case("gist"));
            if !scopes.trim().is_empty() && !has_gist {
                return Err(CloudSyncError::MissingGistScope);
            }
        }

        let login = body
            .get("login")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CloudSyncError::Auth("missing login in GitHub user profile".into()))?
            .to_string();
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let avatar_url = body
            .get("avatar_url")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);

        Ok(GitHubUserProfile {
            login,
            name,
            avatar_url,
        })
    }

    /// Find and download the user's Orbit Encrypted Vault Secret Gist (checking cached `gist_id` first).
    pub async fn fetch_vault_gist(
        &self,
        token: &str,
        cached_gist_id: Option<&str>,
    ) -> Result<Option<RemoteVaultGist>, CloudSyncError> {
        if let Some(gid) = cached_gist_id.map(str::trim).filter(|s| !s.is_empty())
            && let Some(found) = self.fetch_gist_by_id(token, gid).await?
        {
            return Ok(Some(found));
        }

        // Scan up to 5 pages (500 gists) for `orbit-cloud-vault.v1.enc.json`
        for page in 1..=5u32 {
            let url = format!("{}/gists?per_page=100&page={page}", self.api_base);
            let resp = self
                .http
                .get(&url)
                .header("Authorization", format!("Bearer {}", token.trim()))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .send()
                .await
                .map_err(|e| CloudSyncError::Http(e.to_string()))?;

            let status = resp.status();
            let list: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CloudSyncError::Http(format!("invalid gists list response: {e}")))?;

            if !status.is_success() {
                let msg = list
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("failed to list gists")
                    .to_string();
                return Err(CloudSyncError::GitHubApi {
                    status: status.as_u16(),
                    message: msg,
                });
            }

            let Some(arr) = list.as_array() else {
                break;
            };
            for item in arr {
                let has_file = item
                    .get("files")
                    .and_then(|f| f.as_object())
                    .is_some_and(|map| map.contains_key(CLOUD_VAULT_GIST_FILENAME));
                if has_file && let Some(gid) = item.get("id").and_then(|v| v.as_str()) {
                    return self.fetch_gist_by_id(token, gid).await;
                }
            }
            if arr.len() < 100 {
                break;
            }
        }
        Ok(None)
    }

    async fn fetch_gist_by_id(
        &self,
        token: &str,
        gist_id: &str,
    ) -> Result<Option<RemoteVaultGist>, CloudSyncError> {
        let url = format!("{}/gists/{}", self.api_base, gist_id.trim());
        let resp = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.trim()))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;

        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid gist response: {e}")))?;

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("failed to fetch gist")
                .to_string();
            return Err(CloudSyncError::GitHubApi {
                status: status.as_u16(),
                message: msg,
            });
        }

        let Some(file_obj) = body
            .get("files")
            .and_then(|f| f.get(CLOUD_VAULT_GIST_FILENAME))
        else {
            return Ok(None);
        };

        let html_url = body
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let truncated = file_obj
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let raw_content = if truncated {
            let raw_url = file_obj
                .get("raw_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    CloudSyncError::InvalidVault("truncated gist missing raw_url".into())
                })?;
            let raw_resp = self
                .http
                .get(raw_url)
                .header("Authorization", format!("Bearer {}", token.trim()))
                .send()
                .await
                .map_err(|e| CloudSyncError::Http(e.to_string()))?;
            raw_resp
                .text()
                .await
                .map_err(|e| CloudSyncError::Http(e.to_string()))?
        } else {
            file_obj
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| CloudSyncError::InvalidVault("gist file missing content".into()))?
                .to_string()
        };

        let envelope: EncryptedVaultEnvelope = serde_json::from_str(&raw_content)
            .map_err(|e| CloudSyncError::InvalidVault(format!("malformed vault envelope: {e}")))?;
        validate_envelope_header(&envelope)?;

        Ok(Some(RemoteVaultGist {
            gist_id: gist_id.trim().to_string(),
            html_url,
            envelope,
        }))
    }

    /// Create or update the user's Orbit Encrypted Vault Secret Gist.
    pub async fn save_vault_gist(
        &self,
        token: &str,
        existing_gist_id: Option<&str>,
        envelope: &EncryptedVaultEnvelope,
    ) -> Result<(String, String), CloudSyncError> {
        let content = serde_json::to_string_pretty(envelope)
            .map_err(|e| CloudSyncError::InvalidVault(e.to_string()))?;

        let (resp, creating) =
            if let Some(gid) = existing_gist_id.map(str::trim).filter(|s| !s.is_empty()) {
                let url = format!("{}/gists/{gid}", self.api_base);
                let payload = serde_json::json!({
                    "description": CLOUD_VAULT_GIST_DESCRIPTION,
                    "files": {
                        CLOUD_VAULT_GIST_FILENAME: {
                            "content": content,
                        }
                    }
                });
                let r = self
                    .http
                    .patch(&url)
                    .header("Authorization", format!("Bearer {}", token.trim()))
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| CloudSyncError::Http(e.to_string()))?;
                (r, false)
            } else {
                let url = format!("{}/gists", self.api_base);
                let payload = serde_json::json!({
                    "description": CLOUD_VAULT_GIST_DESCRIPTION,
                    "public": false,
                    "files": {
                        CLOUD_VAULT_GIST_FILENAME: {
                            "content": content,
                        }
                    }
                });
                let r = self
                    .http
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", token.trim()))
                    .header("Accept", "application/vnd.github+json")
                    .header("X-GitHub-Api-Version", "2022-11-28")
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| CloudSyncError::Http(e.to_string()))?;
                (r, true)
            };

        let status = resp.status();
        // If PATCH returned 404 (user manually deleted the gist on GitHub), transparently recreate it!
        if !creating && status.as_u16() == 404 {
            return Box::pin(self.save_vault_gist(token, None, envelope)).await;
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid gist save response: {e}")))?;

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("failed to save vault gist")
                .to_string();
            return Err(CloudSyncError::GitHubApi {
                status: status.as_u16(),
                message: msg,
            });
        }

        let id = body
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| CloudSyncError::InvalidVault("gist save missing id".into()))?
            .to_string();
        let html_url = body
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok((id, html_url))
    }
}

/// High-level helper used by both `OrbitAppCore` and `orbit` CLI to decrypt or initialize the remote vault
/// given a passphrase, returning the derived key + optional remote payload + remote gist info.
pub async fn unlock_remote_vault_with_passphrase(
    client: &GitHubCloudClient,
    token: &str,
    cached_gist_id: Option<&str>,
    passphrase: &str,
) -> Result<
    (
        DerivedVaultKey,
        Option<RemoteVaultGist>,
        Option<CloudVaultPayload>,
    ),
    CloudSyncError,
> {
    let remote = client.fetch_vault_gist(token, cached_gist_id).await?;
    if let Some(gist) = remote {
        let key = derive_vault_key(
            passphrase,
            &gist.envelope.salt_b64,
            gist.envelope.kdf_params,
        )?;
        let payload: CloudVaultPayload = decrypt_vault_json(&key, &gist.envelope)?;
        Ok((key, Some(gist), Some(payload)))
    } else {
        let key = derive_new_vault_key(passphrase)?;
        Ok((key, None, None))
    }
}

/// High-level helper to decrypt a fetched remote vault using the locally cached derived key.
pub fn decrypt_remote_with_cached_key(
    key: &DerivedVaultKey,
    gist: &RemoteVaultGist,
) -> Result<CloudVaultPayload, CloudSyncError> {
    if !key.matches_envelope(&gist.envelope) {
        return Err(CloudSyncError::PassphraseRotationRequired);
    }
    let payload: CloudVaultPayload = decrypt_vault_json(key, &gist.envelope)?;
    Ok(payload)
}

/// Encrypt a `CloudVaultPayload` with `key` and upload it to GitHub Secret Gist.
pub async fn push_vault_payload(
    client: &GitHubCloudClient,
    token: &str,
    existing_gist_id: Option<&str>,
    key: &DerivedVaultKey,
    payload: &CloudVaultPayload,
) -> Result<(String, String), CloudSyncError> {
    let envelope = encrypt_vault_json(key, payload, payload.updated_at_ms)?;
    client
        .save_vault_gist(token, existing_gist_id, &envelope)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_local_host(
        host_id: &str,
        name: &str,
        eps: Vec<&str>,
        relay: Option<&str>,
        updated_at_ms: i64,
    ) -> LocalHostSyncRecord {
        LocalHostSyncRecord {
            profile: StoredHostProfile {
                host_id: host_id.to_string(),
                display_name: name.to_string(),
                daemon_pubkey_b64: format!("pub_{host_id}"),
                direct_endpoints: eps.into_iter().map(ToOwned::to_owned).collect(),
                relay_url: relay.map(ToOwned::to_owned),
                meta: StoredHostMeta::default(),
            },
            os_arch: Some("linux/aarch64".into()),
            updated_at_ms,
        }
    }

    #[test]
    fn filters_loopback_endpoints_but_preserves_lan_and_tailscale() {
        let eps = vec![
            "ws://127.0.0.1:6868/ws".to_string(),
            "ws://localhost:6868/ws".to_string(),
            "ws://[::1]:6868/ws".to_string(),
            "ws://192.168.1.50:6868/ws".to_string(),
            "ws://100.64.0.12:6868/ws".to_string(),
            "ws://192.168.1.50:6868/ws".to_string(),
        ];
        assert_eq!(
            filter_shareable_endpoints(&eps),
            vec![
                "ws://192.168.1.50:6868/ws".to_string(),
                "ws://100.64.0.12:6868/ws".to_string(),
            ]
        );
    }

    #[test]
    fn merge_adds_remote_hosts_pushes_local_hosts_and_propagates_tombstones() {
        let mut remote = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 3,
            updated_at_ms: 2000,
            ..Default::default()
        };
        remote.hosts.insert(
            "host_a".into(),
            SyncedHostEntry {
                host_id: "host_a".into(),
                hostname: "mac-studio".into(),
                daemon_pubkey_b64: "pub_a".into(),
                direct_endpoints: vec!["ws://192.168.1.10:6868/ws".into()],
                relay_url: Some("wss://relay.example.com:7878".into()),
                custom_name: Some("Studio".into()),
                ssh_alias: None,
                route_mode: Some("auto".into()),
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: Some("macos/aarch64".into()),
                updated_at_ms: 2000,
                deleted_at_ms: None,
            },
        );
        remote.hosts.insert(
            "host_b".into(),
            SyncedHostEntry {
                host_id: "host_b".into(),
                hostname: "old-box".into(),
                daemon_pubkey_b64: "pub_b".into(),
                direct_endpoints: vec![],
                relay_url: None,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: None,
                updated_at_ms: 1500,
                deleted_at_ms: None,
            },
        );
        remote.hosts.insert(
            "host_c".into(),
            SyncedHostEntry {
                host_id: "host_c".into(),
                hostname: "deleted-remotely".into(),
                daemon_pubkey_b64: "pub_c".into(),
                direct_endpoints: vec![],
                relay_url: None,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                updated_at_ms: 2500,
                deleted_at_ms: Some(2500),
            },
        );

        let local_hosts = vec![
            // Older local copy of host_c -> should be removed by remote tombstone (2500 > 1000)
            sample_local_host("host_c", "deleted-remotely", vec![], None, 1000),
            // Brand-new local host_d -> should be pushed to remote
            sample_local_host(
                "host_d",
                "gpu-rig",
                vec!["ws://127.0.0.1:6868/ws", "ws://10.0.0.9:6868/ws"],
                Some("ssh+relay://gpu"),
                3000,
            ),
        ];

        let mut local_meta = CloudSyncLocalMeta::default();
        // Local device explicitly deleted host_b at t=2600 (> remote 1500)
        local_meta.record_host_deleted("host_b", 2600);

        let outcome = merge_cloud_vault(
            Some(&remote),
            &local_hosts,
            &[],
            &[],
            &HashMap::new(),
            &local_meta,
            3000,
        );

        assert!(outcome.needs_push);
        assert_eq!(outcome.merged.revision, 4);
        assert_eq!(outcome.hosts_added_count, 1);
        assert_eq!(outcome.hosts_to_upsert[0].host_id, "host_a");
        assert_eq!(outcome.hosts_removed_count, 1);
        assert_eq!(outcome.hosts_to_remove, vec!["host_c".to_string()]);

        // host_b is now tombstoned in merged payload
        assert_eq!(outcome.merged.hosts["host_b"].deleted_at_ms, Some(2600));
        // host_d has loopback stripped!
        assert_eq!(
            outcome.merged.hosts["host_d"].direct_endpoints,
            vec!["ws://10.0.0.9:6868/ws".to_string()]
        );
    }

    #[test]
    fn merge_projects_order_and_preferences_by_lww() {
        let mut remote = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 1,
            updated_at_ms: 2000,
            project_order: vec!["p2".into(), "p1".into()],
            project_order_updated_at_ms: 2000,
            ..Default::default()
        };
        remote.preferences.insert(
            "theme".into(),
            SyncedPreferenceEntry {
                value: serde_json::json!("dark"),
                updated_at_ms: 2000,
            },
        );
        remote.preferences.insert(
            "enterToSend".into(),
            SyncedPreferenceEntry {
                value: serde_json::json!(true),
                updated_at_ms: 1000,
            },
        );

        let mut local_prefs = HashMap::new();
        local_prefs.insert(
            "theme".into(),
            SyncedPreferenceEntry {
                value: serde_json::json!("light"),
                updated_at_ms: 1500,
            },
        );
        local_prefs.insert(
            "enterToSend".into(),
            SyncedPreferenceEntry {
                value: serde_json::json!(false),
                updated_at_ms: 2500,
            },
        );

        let local_meta = CloudSyncLocalMeta {
            project_order_updated_at_ms: 1000,
            ..Default::default()
        };

        let outcome = merge_cloud_vault(
            Some(&remote),
            &[],
            &[],
            &["p1".into()],
            &local_prefs,
            &local_meta,
            3000,
        );

        // Remote won theme & project_order; local won enterToSend -> needs_push = true
        assert!(outcome.needs_push);
        assert_eq!(
            outcome.project_order_changed,
            Some((vec!["p2".into(), "p1".into()], 2000))
        );
        assert_eq!(
            outcome.preferences_to_apply["theme"].value,
            serde_json::json!("dark")
        );
        assert_eq!(
            outcome.merged.preferences["enterToSend"].value,
            serde_json::json!(false)
        );
    }
}
