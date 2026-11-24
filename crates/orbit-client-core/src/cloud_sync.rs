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

pub const CLOUD_VAULT_KEY_GIST_FILENAME: &str = "orbit-cloud-vault-key.v1.json";
pub const CLOUD_VAULT_KEY_GIST_DESCRIPTION: &str = "Orbit Zero-Friction Auto-Unlock Key (GitHub-account bound — delete this gist to revert to manual passphrase only)";
pub const CLOUD_VAULT_KEY_FORMAT: &str = "orbit-auto-unlock-key-v1";
pub const CLOUD_SYNC_DEVICE_LINK_PREFIX: &str = "orbit://sync-login";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AutoUnlockKeyEnvelope {
    pub format: String,
    pub version: u32,
    pub key_b64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kdf_params: Option<orbit_crypto::VaultKdfParams>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct RemoteAutoUnlockKey {
    pub gist_id: String,
    pub envelope: AutoUnlockKeyEnvelope,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CloudSyncDeviceLinkPayload {
    pub v: u32,
    pub github_access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub salt_b64: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kdf_params: Option<orbit_crypto::VaultKdfParams>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_gist_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_url: Option<String>,
}

pub fn encode_device_link_uri(
    payload: &CloudSyncDeviceLinkPayload,
) -> Result<String, CloudSyncError> {
    let json =
        serde_json::to_vec(payload).map_err(|e| CloudSyncError::InvalidVault(e.to_string()))?;
    let b64 = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, json);
    Ok(format!("{CLOUD_SYNC_DEVICE_LINK_PREFIX}?v=1&p={b64}"))
}

pub fn decode_device_link_uri(raw: &str) -> Result<CloudSyncDeviceLinkPayload, CloudSyncError> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix(CLOUD_SYNC_DEVICE_LINK_PREFIX)
        .ok_or_else(|| {
            CloudSyncError::InvalidVault("Invalid Orbit device link URI prefix".into())
        })?;
    let query = rest.strip_prefix('?').ok_or_else(|| {
        CloudSyncError::InvalidVault("Missing query in Orbit device link URI".into())
    })?;
    let mut payload_b64: Option<&str> = None;
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=')
            && k == "p"
        {
            payload_b64 = Some(v);
            break;
        }
    }
    let b64 = payload_b64.ok_or_else(|| {
        CloudSyncError::InvalidVault("Missing p parameter in device link URI".into())
    })?;
    let decoded = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, b64)
        .or_else(|_| base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64))
        .map_err(|e| CloudSyncError::InvalidVault(format!("invalid base64 in device link: {e}")))?;
    let payload: CloudSyncDeviceLinkPayload = serde_json::from_slice(&decoded)
        .map_err(|e| CloudSyncError::InvalidVault(format!("malformed device link payload: {e}")))?;
    if payload.v != 1 || payload.github_access_token.trim().is_empty() {
        return Err(CloudSyncError::InvalidVault(
            "Unsupported or incomplete Orbit device link payload".into(),
        ));
    }
    Ok(payload)
}

pub const DEFAULT_GITHUB_WEB_BASE: &str = "https://github.com";
pub const DEFAULT_GITHUB_API_BASE: &str = "https://api.github.com";

/// Built-in public GitHub OAuth App Client ID for Orbit (RFC 8628 Device Flow — intentionally public,
/// no client secret). Can be overridden at compile time via `ORBIT_GITHUB_CLIENT_ID`, at runtime via
/// `ORBIT_GITHUB_CLIENT_ID`, or per-device in Settings / CLI.
pub const BUILTIN_GITHUB_CLIENT_ID: &str = "Ov23lit3VKcNBq6uNUfp";

pub const DEFAULT_GITHUB_CLIENT_ID: &str = match option_env!("ORBIT_GITHUB_CLIENT_ID") {
    Some(id) if !id.is_empty() => id,
    _ => BUILTIN_GITHUB_CLIENT_ID,
};

pub const KV_CLOUD_SYNC_CONFIG: &str = "cloud_sync_config_v1";
pub const KV_CLOUD_SYNC_META: &str = "cloud_sync_meta_v1";
pub const KV_CLOUD_SYNC_PREFS: &str = "cloud_sync_prefs_v1";
pub const KV_CLOUD_SYNC_FLEET_NET: &str = "cloud_sync_fleet_network_v1";

const MAX_BRIDGE_HOSTS_PER_HOST: usize = 16;

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
    pub instant_unlock_enabled: bool,
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

/// Shared account-level Fleet Network fabric metadata synced across all logged-in devices.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncedFleetNetwork {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_relay_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relay_urls: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_relay_host_id: Option<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fleet_network: Option<SyncedFleetNetwork>,
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
    pub pairing_secret: String,
    #[serde(default)]
    pub pairing_secret_updated_at_ms: i64,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bridge_host_ids: Vec<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_gist_id: Option<String>,
    #[serde(default)]
    pub require_manual_passphrase: bool,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_etag: Option<String>,
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
            key_gist_id: None,
            require_manual_passphrase: false,
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
            last_etag: None,
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
            instant_unlock_enabled: !self.require_manual_passphrase,
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
    pub fleet_network_changed: Option<SyncedFleetNetwork>,
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
    let builtin = BUILTIN_GITHUB_CLIENT_ID.trim();
    if !builtin.is_empty() {
        return Some(builtin.to_string());
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
        pairing_secret: rec.profile.pairing_secret.clone(),
        pairing_secret_updated_at_ms: rec.profile.meta.pairing_secret_updated_at_ms,
        custom_name: rec.profile.meta.custom_name.clone(),
        ssh_alias: rec.profile.meta.ssh_alias.clone(),
        route_mode: rec.profile.meta.route_mode.clone(),
        remote_daemon_port: rec.profile.meta.remote_daemon_port,
        custom_direct_endpoint: rec.profile.meta.custom_direct_endpoint.clone(),
        os_arch: rec.os_arch.clone(),
        bridge_host_ids: normalize_bridge_host_ids(
            &rec.profile.meta.bridge_host_ids,
            &rec.profile.host_id,
        ),
        updated_at_ms: rec.updated_at_ms,
        deleted_at_ms: None,
    }
}

pub fn normalize_bridge_host_ids(ids: &[String], target_host_id: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in ids {
        let clean = raw.trim();
        if clean.is_empty() || clean == target_host_id || out.iter().any(|x| x == clean) {
            continue;
        }
        out.push(clean.to_string());
        if out.len() >= MAX_BRIDGE_HOSTS_PER_HOST {
            break;
        }
    }
    out
}

pub fn stored_host_from_synced(entry: &SyncedHostEntry) -> StoredHostProfile {
    StoredHostProfile {
        host_id: entry.host_id.clone(),
        display_name: entry.hostname.clone(),
        daemon_pubkey_b64: entry.daemon_pubkey_b64.clone(),
        direct_endpoints: entry.direct_endpoints.clone(),
        relay_url: entry.relay_url.clone(),
        pairing_secret: entry.pairing_secret.clone(),
        meta: StoredHostMeta {
            custom_name: entry.custom_name.clone(),
            ssh_alias: entry.ssh_alias.clone(),
            route_mode: entry.route_mode.clone(),
            remote_daemon_port: entry.remote_daemon_port,
            custom_direct_endpoint: entry.custom_direct_endpoint.clone(),
            pairing_secret_updated_at_ms: entry.pairing_secret_updated_at_ms,
            bridge_host_ids: normalize_bridge_host_ids(&entry.bridge_host_ids, &entry.host_id),
        },
    }
}

fn host_effective_ts(entry: &SyncedHostEntry) -> i64 {
    entry.deleted_at_ms.unwrap_or(entry.updated_at_ms)
}

fn project_effective_ts(entry: &SyncedProjectEntry) -> i64 {
    entry.deleted_at_ms.unwrap_or(entry.updated_at_ms)
}

/// Resolve the winning `(pairing_secret, pairing_secret_updated_at_ms)` independently of host
/// metadata timestamps so renaming or editing host metadata on a stale device can never roll back
/// a rotated pairing secret. Empty secrets are ignored completely so they can never stamp an older
/// non-empty secret with a newer timestamp.
fn merge_host_pairing_secret(
    remote: &SyncedHostEntry,
    local: &SyncedHostEntry,
    prefer_remote_on_tie: bool,
) -> (String, i64) {
    let r_empty = remote.pairing_secret.trim().is_empty();
    let l_empty = local.pairing_secret.trim().is_empty();
    match (r_empty, l_empty) {
        (true, true) => (String::new(), 0),
        (true, false) => (
            local.pairing_secret.clone(),
            local.pairing_secret_updated_at_ms,
        ),
        (false, true) => (
            remote.pairing_secret.clone(),
            remote.pairing_secret_updated_at_ms,
        ),
        (false, false) => {
            let r_ts = remote.pairing_secret_updated_at_ms;
            let l_ts = local.pairing_secret_updated_at_ms;
            if r_ts > l_ts {
                (remote.pairing_secret.clone(), r_ts)
            } else if l_ts > r_ts {
                (local.pairing_secret.clone(), l_ts)
            } else if prefer_remote_on_tie {
                (remote.pairing_secret.clone(), r_ts)
            } else {
                (local.pairing_secret.clone(), l_ts)
            }
        }
    }
}

fn hosts_content_equal(a: &SyncedHostEntry, b: &SyncedHostEntry) -> bool {
    a.hostname == b.hostname
        && a.daemon_pubkey_b64 == b.daemon_pubkey_b64
        && a.direct_endpoints == b.direct_endpoints
        && a.relay_url == b.relay_url
        && a.pairing_secret == b.pairing_secret
        && a.pairing_secret_updated_at_ms == b.pairing_secret_updated_at_ms
        && a.custom_name == b.custom_name
        && a.ssh_alias == b.ssh_alias
        && a.route_mode == b.route_mode
        && a.remote_daemon_port == b.remote_daemon_port
        && a.custom_direct_endpoint == b.custom_direct_endpoint
        && a.bridge_host_ids == b.bridge_host_ids
        && a.deleted_at_ms == b.deleted_at_ms
}

fn projects_content_equal(a: &SyncedProjectEntry, b: &SyncedProjectEntry) -> bool {
    a.host_id == b.host_id
        && a.name == b.name
        && a.directories == b.directories
        && a.deleted_at_ms == b.deleted_at_ms
}

/// Deterministic SHA-256 fingerprint of a decrypted `CloudVaultPayload` (used for live change gossip).
pub fn vault_fingerprint(payload: &CloudVaultPayload) -> String {
    use sha2::{Digest, Sha256};

    let bytes = serde_json::to_vec(payload).unwrap_or_default();
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn union_bridge_host_ids(target_host_id: &str, a: &[String], b: &[String]) -> Vec<String> {
    let mut combined = Vec::with_capacity(a.len() + b.len());
    combined.extend_from_slice(a);
    combined.extend_from_slice(b);
    normalize_bridge_host_ids(&combined, target_host_id)
}

fn push_unique_relay_url(list: &mut Vec<String>, candidate: &str) {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return;
    }
    let canon =
        orbit_relay_transport::normalize_relay_url(trimmed).unwrap_or_else(|_| trimmed.to_string());
    // Prefer variant carrying `?ft=`/`&ft=` if base matches an existing entry without token
    let canon_has_ft = canon.contains("?ft=") || canon.contains("&ft=");
    let canon_base = canon.split('?').next().unwrap_or(&canon);
    if let Some(existing) = list
        .iter_mut()
        .find(|u| u.split('?').next().unwrap_or(u) == canon_base)
    {
        let existing_has_ft = existing.contains("?ft=") || existing.contains("&ft=");
        if canon_has_ft && !existing_has_ft {
            *existing = canon;
        }
        return;
    }
    if list.len() < 16 {
        list.push(canon);
    }
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
    merge_cloud_vault_with_network(
        remote,
        local_hosts,
        local_projects,
        local_project_order,
        local_preferences,
        local_meta,
        None,
        now_ms,
    )
}

/// Deterministic per-entity LWW merge including account-wide `SyncedFleetNetwork` convergence.
#[allow(clippy::too_many_arguments)]
pub fn merge_cloud_vault_with_network(
    remote: Option<&CloudVaultPayload>,
    local_hosts: &[LocalHostSyncRecord],
    local_projects: &[StoredClientProject],
    local_project_order: &[String],
    local_preferences: &HashMap<String, SyncedPreferenceEntry>,
    local_meta: &CloudSyncLocalMeta,
    local_fleet_network: Option<&SyncedFleetNetwork>,
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
        fleet_network: None,
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
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
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
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
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
                    // Remote metadata wins; if both sides are active, resolve pairing_secret independently by its own timestamp!
                    if r.deleted_at_ms.is_some() {
                        if local_rec.is_some() {
                            hosts_to_remove.push(host_id.clone());
                            hosts_removed_count += 1;
                        }
                        r.clone()
                    } else {
                        let mut combined = r.clone();
                        combined.bridge_host_ids =
                            union_bridge_host_ids(&host_id, &r.bridge_host_ids, &l.bridge_host_ids);
                        if l.deleted_at_ms.is_none() {
                            let (sec, sec_ts) = merge_host_pairing_secret(r, &l, true);
                            combined.pairing_secret = sec;
                            combined.pairing_secret_updated_at_ms = sec_ts;
                        }
                        if !hosts_content_equal(&combined, r) {
                            needs_push = true;
                        }
                        if local_rec.is_none() {
                            hosts_to_upsert.push(combined.clone());
                            hosts_added_count += 1;
                        } else if !hosts_content_equal(&combined, &l) {
                            hosts_to_upsert.push(combined.clone());
                            hosts_updated_count += 1;
                        }
                        combined
                    }
                } else if l_ts > r_ts {
                    // Local metadata wins; if both sides are active, preserve a newer remote rotated pairing_secret!
                    if l.deleted_at_ms.is_some() {
                        if !hosts_content_equal(r, &l) {
                            needs_push = true;
                        }
                        l
                    } else {
                        let mut combined = l.clone();
                        combined.bridge_host_ids =
                            union_bridge_host_ids(&host_id, &l.bridge_host_ids, &r.bridge_host_ids);
                        if r.deleted_at_ms.is_none() {
                            let (sec, sec_ts) = merge_host_pairing_secret(r, &l, false);
                            combined.pairing_secret = sec;
                            combined.pairing_secret_updated_at_ms = sec_ts;
                        }
                        if !hosts_content_equal(&combined, r) {
                            needs_push = true;
                        }
                        if !hosts_content_equal(&combined, &l) {
                            hosts_to_upsert.push(combined.clone());
                            hosts_updated_count += 1;
                        }
                        combined
                    }
                } else {
                    // Equal timestamp: if both active and content differs, union endpoints/bridges, prefer richer metadata,
                    // and resolve pairing_secret independently by `pairing_secret_updated_at_ms`.
                    if r.deleted_at_ms.is_none() && l.deleted_at_ms.is_none() {
                        let mut combined = r.clone();
                        let mut eps = filter_shareable_endpoints(&combined.direct_endpoints);
                        for ep in filter_shareable_endpoints(&l.direct_endpoints) {
                            if !eps.contains(&ep) {
                                eps.push(ep);
                            }
                        }
                        combined.direct_endpoints = eps;
                        combined.bridge_host_ids =
                            union_bridge_host_ids(&host_id, &r.bridge_host_ids, &l.bridge_host_ids);
                        if combined.relay_url.is_none() {
                            combined.relay_url = l.relay_url.clone();
                        }
                        if combined.custom_name.is_none() {
                            combined.custom_name = l.custom_name.clone();
                        }
                        if combined.ssh_alias.is_none() {
                            combined.ssh_alias = l.ssh_alias.clone();
                        }
                        let (sec, sec_ts) = merge_host_pairing_secret(r, &l, true);
                        combined.pairing_secret = sec;
                        combined.pairing_secret_updated_at_ms = sec_ts;
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

    // 5. Merge Account Fleet Network Fabric (`primary_relay_url` + union of known relays)
    let r_net = remote.and_then(|r| r.fleet_network.as_ref());
    let l_net = local_fleet_network;
    let mut merged_net = match (r_net, l_net) {
        (Some(rn), Some(ln)) => {
            if ln.updated_at_ms > rn.updated_at_ms {
                ln.clone()
            } else {
                rn.clone()
            }
        }
        (Some(rn), None) => rn.clone(),
        (None, Some(ln)) => ln.clone(),
        (None, None) => SyncedFleetNetwork::default(),
    };
    let mut all_relays = Vec::new();
    if let Some(ref primary) = merged_net.primary_relay_url {
        push_unique_relay_url(&mut all_relays, primary);
    }
    if let Some(rn) = r_net {
        if let Some(ref p) = rn.primary_relay_url {
            push_unique_relay_url(&mut all_relays, p);
        }
        for u in &rn.relay_urls {
            push_unique_relay_url(&mut all_relays, u);
        }
    }
    if let Some(ln) = l_net {
        if let Some(ref p) = ln.primary_relay_url {
            push_unique_relay_url(&mut all_relays, p);
        }
        for u in &ln.relay_urls {
            push_unique_relay_url(&mut all_relays, u);
        }
    }
    let mut active_host_keys: Vec<&String> = merged.hosts.keys().collect();
    active_host_keys.sort();
    for hid in active_host_keys {
        if let Some(host) = merged.hosts.get(hid)
            && host.deleted_at_ms.is_none()
            && let Some(ref u) = host.relay_url
        {
            push_unique_relay_url(&mut all_relays, u);
        }
    }
    if let Some(ref cur_primary) = merged_net.primary_relay_url {
        let cur_base = cur_primary.split('?').next().unwrap_or(cur_primary);
        if let Some(upgraded) = all_relays
            .iter()
            .find(|u| u.split('?').next().unwrap_or(u) == cur_base)
        {
            merged_net.primary_relay_url = Some(upgraded.clone());
        }
    } else if let Some(first) = all_relays.first() {
        merged_net.primary_relay_url = Some(first.clone());
        if merged_net.updated_at_ms == 0 {
            merged_net.updated_at_ms = now_ms;
        }
    }
    merged_net.relay_urls = all_relays;

    let merged_net_opt = if merged_net.primary_relay_url.is_some()
        || !merged_net.relay_urls.is_empty()
        || merged_net.primary_relay_host_id.is_some()
    {
        Some(merged_net)
    } else {
        None
    };
    if merged_net_opt.as_ref() != r_net {
        needs_push = true;
    }
    let fleet_network_changed = if merged_net_opt.as_ref() != l_net {
        merged_net_opt.clone()
    } else {
        None
    };
    merged.fleet_network = merged_net_opt;

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
        fleet_network_changed,
    }
}

pub fn load_fleet_network_from_store(store: &OrbitStore) -> Option<SyncedFleetNetwork> {
    store
        .get_client_kv(KV_CLOUD_SYNC_FLEET_NET)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

pub fn save_fleet_network_to_store(
    store: &OrbitStore,
    network: &SyncedFleetNetwork,
) -> Result<(), CloudSyncError> {
    let raw = serde_json::to_string(network).map_err(|e| CloudSyncError::Storage(e.to_string()))?;
    store
        .set_client_kv(KV_CLOUD_SYNC_FLEET_NET, &raw, now_unix_ms())
        .map_err(|e| CloudSyncError::Storage(e.to_string()))
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
pub enum GistConditionalFetch {
    NotModified,
    Updated {
        gist: Box<RemoteVaultGist>,
        etag: Option<String>,
    },
    NotFound,
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

    /// Fetch both encrypted Vault Gist and optional Zero-Friction Auto-Unlock Key Gist.
    /// Uses cached IDs first and scans `/gists` at most once for any missing gist.
    pub async fn fetch_vault_and_auto_key(
        &self,
        token: &str,
        cached_vault_gist_id: Option<&str>,
        cached_key_gist_id: Option<&str>,
    ) -> Result<(Option<RemoteVaultGist>, Option<RemoteAutoUnlockKey>), CloudSyncError> {
        let mut vault = if let Some(gid) = cached_vault_gist_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            self.fetch_gist_by_id(token, gid).await?
        } else {
            None
        };
        let mut auto_key =
            if let Some(kid) = cached_key_gist_id.map(str::trim).filter(|s| !s.is_empty()) {
                self.fetch_auto_unlock_key_by_id(token, kid).await?
            } else {
                None
            };

        if vault.is_some() && auto_key.is_some() {
            return Ok((vault, auto_key));
        }

        // Scan up to 5 pages (500 gists) once for whichever gist is still missing
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
                let Some(files) = item.get("files").and_then(|f| f.as_object()) else {
                    continue;
                };
                let Some(gid) = item.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                if vault.is_none() && files.contains_key(CLOUD_VAULT_GIST_FILENAME) {
                    vault = self.fetch_gist_by_id(token, gid).await?;
                }
                if auto_key.is_none() && files.contains_key(CLOUD_VAULT_KEY_GIST_FILENAME) {
                    auto_key = self.fetch_auto_unlock_key_by_id(token, gid).await?;
                }
                if vault.is_some() && auto_key.is_some() {
                    return Ok((vault, auto_key));
                }
            }
            if arr.len() < 100 {
                break;
            }
        }
        Ok((vault, auto_key))
    }

    async fn fetch_auto_unlock_key_by_id(
        &self,
        token: &str,
        gist_id: &str,
    ) -> Result<Option<RemoteAutoUnlockKey>, CloudSyncError> {
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
            .map_err(|e| CloudSyncError::Http(format!("invalid key gist response: {e}")))?;

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("failed to fetch auto-unlock key gist")
                .to_string();
            return Err(CloudSyncError::GitHubApi {
                status: status.as_u16(),
                message: msg,
            });
        }

        let Some(file_obj) = body
            .get("files")
            .and_then(|f| f.get(CLOUD_VAULT_KEY_GIST_FILENAME))
        else {
            return Ok(None);
        };

        let truncated = file_obj
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let raw_content = if truncated {
            let raw_url = file_obj
                .get("raw_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    CloudSyncError::InvalidVault("truncated key gist missing raw_url".into())
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
                .ok_or_else(|| {
                    CloudSyncError::InvalidVault("key gist file missing content".into())
                })?
                .to_string()
        };

        let envelope: AutoUnlockKeyEnvelope = serde_json::from_str(&raw_content)
            .map_err(|e| CloudSyncError::InvalidVault(format!("malformed auto-unlock key: {e}")))?;
        if envelope.format != CLOUD_VAULT_KEY_FORMAT || envelope.version != 1 {
            return Err(CloudSyncError::InvalidVault(format!(
                "unsupported auto-unlock key format {} v{}",
                envelope.format, envelope.version
            )));
        }

        Ok(Some(RemoteAutoUnlockKey {
            gist_id: gist_id.trim().to_string(),
            envelope,
        }))
    }

    /// Find and download the user's Orbit Encrypted Vault Secret Gist (checking cached `gist_id` first).
    pub async fn fetch_vault_gist(
        &self,
        token: &str,
        cached_gist_id: Option<&str>,
    ) -> Result<Option<RemoteVaultGist>, CloudSyncError> {
        let (vault, _) = self
            .fetch_vault_and_auto_key(token, cached_gist_id, None)
            .await?;
        Ok(vault)
    }

    async fn fetch_gist_by_id(
        &self,
        token: &str,
        gist_id: &str,
    ) -> Result<Option<RemoteVaultGist>, CloudSyncError> {
        match self
            .fetch_gist_by_id_conditional(token, gist_id, None)
            .await?
        {
            GistConditionalFetch::Updated { gist, .. } => Ok(Some(*gist)),
            GistConditionalFetch::NotModified | GistConditionalFetch::NotFound => Ok(None),
        }
    }

    /// Fetch the user's Cloud Sync Vault Gist using `If-None-Match` (`ETag`) conditional requests
    /// so instant polling costs zero payload decryption and preserves GitHub API rate limit on HTTP 304.
    pub async fn fetch_vault_gist_conditional(
        &self,
        token: &str,
        gist_id: &str,
        if_none_match: Option<&str>,
    ) -> Result<GistConditionalFetch, CloudSyncError> {
        self.fetch_gist_by_id_conditional(token, gist_id, if_none_match)
            .await
    }

    async fn fetch_gist_by_id_conditional(
        &self,
        token: &str,
        gist_id: &str,
        if_none_match: Option<&str>,
    ) -> Result<GistConditionalFetch, CloudSyncError> {
        let url = format!("{}/gists/{}", self.api_base, gist_id.trim());
        let mut req = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.trim()))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(etag) = if_none_match.map(str::trim).filter(|s| !s.is_empty()) {
            req = req.header("If-None-Match", etag);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;

        let status = resp.status();
        if status.as_u16() == 304 {
            return Ok(GistConditionalFetch::NotModified);
        }
        if status.as_u16() == 404 {
            return Ok(GistConditionalFetch::NotFound);
        }
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned);
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
            return Ok(GistConditionalFetch::NotFound);
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

        Ok(GistConditionalFetch::Updated {
            gist: Box::new(RemoteVaultGist {
                gist_id: gist_id.trim().to_string(),
                html_url,
                envelope,
            }),
            etag,
        })
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

    /// Create or update the companion private Secret Gist holding the Zero-Friction Auto-Unlock Key.
    pub async fn save_auto_unlock_key_gist(
        &self,
        token: &str,
        existing_key_gist_id: Option<&str>,
        key: &DerivedVaultKey,
    ) -> Result<String, CloudSyncError> {
        let envelope = AutoUnlockKeyEnvelope {
            format: CLOUD_VAULT_KEY_FORMAT.to_string(),
            version: 1,
            key_b64: key.key_b64().to_string(),
            salt_b64: Some(key.salt_b64.clone()),
            kdf_params: Some(key.kdf_params),
            updated_at_ms: now_unix_ms(),
        };
        let content = serde_json::to_string_pretty(&envelope)
            .map_err(|e| CloudSyncError::InvalidVault(e.to_string()))?;

        let (resp, creating) = if let Some(gid) = existing_key_gist_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let url = format!("{}/gists/{gid}", self.api_base);
            let payload = serde_json::json!({
                "description": CLOUD_VAULT_KEY_GIST_DESCRIPTION,
                "files": {
                    CLOUD_VAULT_KEY_GIST_FILENAME: {
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
                "description": CLOUD_VAULT_KEY_GIST_DESCRIPTION,
                "public": false,
                "files": {
                    CLOUD_VAULT_KEY_GIST_FILENAME: {
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
        if !creating && status.as_u16() == 404 {
            return Box::pin(self.save_auto_unlock_key_gist(token, None, key)).await;
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| CloudSyncError::Http(format!("invalid key gist save response: {e}")))?;

        if !status.is_success() {
            let msg = body
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("failed to save auto-unlock key gist")
                .to_string();
            return Err(CloudSyncError::GitHubApi {
                status: status.as_u16(),
                message: msg,
            });
        }

        body.get("id")
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| CloudSyncError::InvalidVault("key gist save missing id".into()))
    }

    /// Delete a GitHub Gist by ID (ignores 404 Already Deleted).
    pub async fn delete_gist(&self, token: &str, gist_id: &str) -> Result<(), CloudSyncError> {
        let gid = gist_id.trim();
        if gid.is_empty() {
            return Ok(());
        }
        let url = format!("{}/gists/{gid}", self.api_base);
        let resp = self
            .http
            .delete(&url)
            .header("Authorization", format!("Bearer {}", token.trim()))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| CloudSyncError::Http(e.to_string()))?;
        let status = resp.status();
        if status.is_success() || status.as_u16() == 404 {
            return Ok(());
        }
        let msg = resp
            .json::<serde_json::Value>()
            .await
            .ok()
            .and_then(|v| {
                v.get("message")
                    .and_then(|m| m.as_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "failed to delete gist".to_string());
        Err(CloudSyncError::GitHubApi {
            status: status.as_u16(),
            message: msg,
        })
    }
}

/// Reconstruct a [`DerivedVaultKey`] from the Dual-Gist Auto-Unlock Key envelope, binding the salt
/// and KDF parameters from the encrypted Vault Gist when available.
pub fn derived_key_from_auto_unlock(
    remote_key: &RemoteAutoUnlockKey,
    remote_vault: Option<&RemoteVaultGist>,
) -> Result<DerivedVaultKey, CloudSyncError> {
    let (salt_b64, kdf_params) = if let Some(vault) = remote_vault {
        (vault.envelope.salt_b64.clone(), vault.envelope.kdf_params)
    } else {
        (
            remote_key
                .envelope
                .salt_b64
                .clone()
                .unwrap_or_else(orbit_crypto::generate_vault_salt_b64),
            remote_key.envelope.kdf_params.unwrap_or_default(),
        )
    };
    DerivedVaultKey::from_b64(&remote_key.envelope.key_b64, salt_b64, kdf_params)
        .map_err(|e| CloudSyncError::InvalidVault(e.to_string()))
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
                pairing_secret: format!("sec_{host_id}"),
                meta: StoredHostMeta::default(),
            },
            os_arch: Some("linux/aarch64".into()),
            updated_at_ms,
        }
    }

    #[test]
    fn resolves_builtin_github_client_id_when_unset() {
        assert_eq!(DEFAULT_GITHUB_CLIENT_ID, BUILTIN_GITHUB_CLIENT_ID);
        assert_eq!(
            resolve_github_client_id(Some("Ov23liOverride"), None).as_deref(),
            Some("Ov23liOverride")
        );
        assert_eq!(
            resolve_github_client_id(None, None).as_deref(),
            Some(BUILTIN_GITHUB_CLIENT_ID)
        );
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
                pairing_secret: "sec_host_a_remote_secret_value_00000000".into(),
                pairing_secret_updated_at_ms: 2000,
                custom_name: Some("Studio".into()),
                ssh_alias: None,
                route_mode: Some("auto".into()),
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: Some("macos/aarch64".into()),
                bridge_host_ids: Vec::new(),
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
                pairing_secret: "sec_host_b_remote_secret_value_00000000".into(),
                pairing_secret_updated_at_ms: 1500,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
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
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 0,
                custom_name: None,
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: None,
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
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

        // host_b is now tombstoned in merged payload (with secret scrubbed)
        assert_eq!(outcome.merged.hosts["host_b"].deleted_at_ms, Some(2600));
        assert!(outcome.merged.hosts["host_b"].pairing_secret.is_empty());
        // host_a and host_d preserve their pairing_secret through sync merge
        assert_eq!(
            outcome.hosts_to_upsert[0].pairing_secret,
            "sec_host_a_remote_secret_value_00000000"
        );
        assert_eq!(outcome.merged.hosts["host_d"].pairing_secret, "sec_host_d");
        // host_d has loopback stripped!
        assert_eq!(
            outcome.merged.hosts["host_d"].direct_endpoints,
            vec!["ws://10.0.0.9:6868/ws".to_string()]
        );
    }

    #[test]
    fn metadata_rename_on_stale_device_never_rolls_back_rotated_pairing_secret() {
        // Case 1: Remote has rotated secret at t=2000; stale local device renamed host at t=3000 with old secret (secret_ts=1000).
        let mut remote = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 5,
            updated_at_ms: 2000,
            ..Default::default()
        };
        remote.hosts.insert(
            "host_x".into(),
            SyncedHostEntry {
                host_id: "host_x".into(),
                hostname: "workstation".into(),
                daemon_pubkey_b64: "pub_x".into(),
                direct_endpoints: vec!["ws://10.0.0.5:6868/ws".into()],
                relay_url: None,
                pairing_secret: "secret_v2_rotated".into(),
                pairing_secret_updated_at_ms: 2000,
                custom_name: Some("Old Name".into()),
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
                updated_at_ms: 2000,
                deleted_at_ms: None,
            },
        );
        let mut local_rec = sample_local_host(
            "host_x",
            "workstation",
            vec!["ws://10.0.0.5:6868/ws"],
            None,
            3000,
        );
        local_rec.profile.daemon_pubkey_b64 = "pub_x".into();
        local_rec.profile.pairing_secret = "secret_v1_stale".into();
        local_rec.profile.meta.pairing_secret_updated_at_ms = 1000;
        local_rec.profile.meta.custom_name = Some("Renamed Locally".into());

        let outcome = merge_cloud_vault(
            Some(&remote),
            &[local_rec],
            &[],
            &[],
            &HashMap::new(),
            &CloudSyncLocalMeta::default(),
            3000,
        );
        assert!(outcome.needs_push);
        let merged_x = &outcome.merged.hosts["host_x"];
        assert_eq!(merged_x.custom_name.as_deref(), Some("Renamed Locally"));
        assert_eq!(merged_x.pairing_secret, "secret_v2_rotated");
        assert_eq!(merged_x.pairing_secret_updated_at_ms, 2000);
        assert_eq!(outcome.hosts_to_upsert.len(), 1);
        assert_eq!(
            outcome.hosts_to_upsert[0].pairing_secret,
            "secret_v2_rotated"
        );

        // Case 2: Remote renamed host at t=4000 with stale secret (secret_ts=1000); local rotated secret at t=3500.
        let mut remote2 = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 6,
            updated_at_ms: 4000,
            ..Default::default()
        };
        remote2.hosts.insert(
            "host_y".into(),
            SyncedHostEntry {
                host_id: "host_y".into(),
                hostname: "mac-mini".into(),
                daemon_pubkey_b64: "pub_y".into(),
                direct_endpoints: vec!["ws://10.0.0.6:6868/ws".into()],
                relay_url: None,
                pairing_secret: "secret_v1_stale".into(),
                pairing_secret_updated_at_ms: 1000,
                custom_name: Some("Renamed Remotely".into()),
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
                updated_at_ms: 4000,
                deleted_at_ms: None,
            },
        );
        let mut local_y = sample_local_host(
            "host_y",
            "mac-mini",
            vec!["ws://10.0.0.6:6868/ws"],
            None,
            3500,
        );
        local_y.profile.daemon_pubkey_b64 = "pub_y".into();
        local_y.profile.pairing_secret = "secret_v3_local_rotated".into();
        local_y.profile.meta.pairing_secret_updated_at_ms = 3500;
        local_y.profile.meta.custom_name = Some("Old Local Name".into());

        let outcome2 = merge_cloud_vault(
            Some(&remote2),
            &[local_y],
            &[],
            &[],
            &HashMap::new(),
            &CloudSyncLocalMeta::default(),
            4000,
        );
        assert!(outcome2.needs_push);
        let merged_y = &outcome2.merged.hosts["host_y"];
        assert_eq!(merged_y.custom_name.as_deref(), Some("Renamed Remotely"));
        assert_eq!(merged_y.pairing_secret, "secret_v3_local_rotated");
        assert_eq!(merged_y.pairing_secret_updated_at_ms, 3500);
        assert_eq!(outcome2.hosts_to_upsert.len(), 1);
        assert_eq!(
            outcome2.hosts_to_upsert[0].pairing_secret,
            "secret_v3_local_rotated"
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

    #[test]
    fn empty_pairing_secret_with_newer_timestamp_never_bumps_older_secret_timestamp() {
        let mut remote_hosts = HashMap::new();
        remote_hosts.insert(
            "h_empty_ts".into(),
            SyncedHostEntry {
                host_id: "h_empty_ts".into(),
                hostname: "box".into(),
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: String::new(),
                pairing_secret_updated_at_ms: 9000,
                custom_name: Some("Renamed".into()),
                ssh_alias: None,
                route_mode: None,
                remote_daemon_port: Some(6868),
                custom_direct_endpoint: None,
                os_arch: None,
                bridge_host_ids: Vec::new(),
                updated_at_ms: 9000,
                deleted_at_ms: None,
            },
        );
        let remote = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 2,
            updated_at_ms: 9000,
            hosts: remote_hosts,
            projects: HashMap::new(),
            project_order: vec![],
            project_order_updated_at_ms: 0,
            preferences: HashMap::new(),
            fleet_network: None,
        };

        let local = LocalHostSyncRecord {
            profile: StoredHostProfile {
                host_id: "h_empty_ts".into(),
                display_name: "box".into(),
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: "old-secret-0123456789abcdef0123456789abcdef".into(),
                meta: StoredHostMeta {
                    pairing_secret_updated_at_ms: 1000,
                    ..Default::default()
                },
            },
            os_arch: None,
            updated_at_ms: 1000,
        };

        let outcome = merge_cloud_vault(
            Some(&remote),
            &[local],
            &[],
            &[],
            &HashMap::new(),
            &CloudSyncLocalMeta::default(),
            9500,
        );
        let merged = outcome.merged.hosts.get("h_empty_ts").expect("merged host");
        assert_eq!(
            merged.pairing_secret,
            "old-secret-0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            merged.pairing_secret_updated_at_ms, 1000,
            "empty remote entry must NEVER stamp older non-empty secret with newer timestamp 9000"
        );
    }

    #[test]
    fn merge_with_network_propagates_primary_relay_and_unions_bridge_host_ids() {
        let mut r_host = sample_local_host(
            "host_isolated",
            "lan-worker",
            vec!["ws://192.168.10.20:6868/ws"],
            None,
            2000,
        );
        r_host.profile.meta.bridge_host_ids = vec!["bridge_b".into()];
        let mut remote = CloudVaultPayload {
            schema_version: CLOUD_VAULT_SCHEMA_VERSION,
            revision: 4,
            updated_at_ms: 2000,
            fleet_network: Some(SyncedFleetNetwork {
                primary_relay_url: Some("wss://relay.example.com?ft=tok_abc".into()),
                relay_urls: vec!["wss://relay.example.com?ft=tok_abc".into()],
                primary_relay_host_id: Some("bridge_b".into()),
                updated_at_ms: 2000,
            }),
            ..Default::default()
        };
        remote
            .hosts
            .insert("host_isolated".into(), synced_host_from_local(&r_host));

        let mut local_isolated = sample_local_host(
            "host_isolated",
            "lan-worker",
            vec!["ws://192.168.10.20:6868/ws"],
            None,
            1000,
        );
        local_isolated.profile.meta.bridge_host_ids = vec!["bridge_a".into()];

        let outcome = merge_cloud_vault_with_network(
            Some(&remote),
            &[local_isolated],
            &[],
            &[],
            &HashMap::new(),
            &CloudSyncLocalMeta::default(),
            None,
            3000,
        );
        let merged_isolated = &outcome.merged.hosts["host_isolated"];
        assert!(
            merged_isolated
                .bridge_host_ids
                .contains(&"bridge_a".to_string())
        );
        assert!(
            merged_isolated
                .bridge_host_ids
                .contains(&"bridge_b".to_string())
        );
        let net = outcome.merged.fleet_network.expect("fleet network");
        assert_eq!(
            net.primary_relay_url.as_deref(),
            Some("wss://relay.example.com?ft=tok_abc")
        );
        assert!(outcome.fleet_network_changed.is_some());
    }

    #[test]
    fn device_link_uri_roundtrip_and_validation() {
        let payload = CloudSyncDeviceLinkPayload {
            v: 1,
            github_access_token: "gho_test_token_12345".into(),
            key_b64: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into()),
            salt_b64: Some(orbit_crypto::generate_vault_salt_b64()),
            kdf_params: Some(orbit_crypto::VaultKdfParams::default()),
            gist_id: Some("gist_vault_1".into()),
            key_gist_id: Some("gist_key_1".into()),
            github_login: Some("octocat".into()),
            relay_url: Some("wss://tunnel.trycloudflare.com?ft=tok_xyz".into()),
        };
        let uri = encode_device_link_uri(&payload).expect("encode uri");
        assert!(uri.starts_with(CLOUD_SYNC_DEVICE_LINK_PREFIX));
        let decoded = decode_device_link_uri(&uri).expect("decode uri");
        assert_eq!(decoded.github_access_token, "gho_test_token_12345");
        assert_eq!(decoded.gist_id.as_deref(), Some("gist_vault_1"));
        assert_eq!(decoded.key_gist_id.as_deref(), Some("gist_key_1"));
        assert_eq!(
            decoded.relay_url.as_deref(),
            Some("wss://tunnel.trycloudflare.com?ft=tok_xyz")
        );

        assert!(decode_device_link_uri("orbit://pair/abc").is_err());

        let bad_ver = CloudSyncDeviceLinkPayload {
            v: 99,
            ..payload.clone()
        };
        let bad_ver_uri = encode_device_link_uri(&bad_ver).expect("encode bad ver");
        assert!(decode_device_link_uri(&bad_ver_uri).is_err());

        let empty_tok = CloudSyncDeviceLinkPayload {
            github_access_token: "   ".into(),
            ..payload
        };
        let empty_tok_uri = encode_device_link_uri(&empty_tok).expect("encode empty tok");
        assert!(decode_device_link_uri(&empty_tok_uri).is_err());
    }

    #[test]
    fn derived_key_from_auto_unlock_prefers_vault_kdf_metadata() {
        let key = orbit_crypto::generate_random_vault_key();
        let vault_salt = orbit_crypto::generate_vault_salt_b64();
        let remote_key = RemoteAutoUnlockKey {
            gist_id: "kid_1".into(),
            envelope: AutoUnlockKeyEnvelope {
                format: CLOUD_VAULT_KEY_FORMAT.into(),
                version: 1,
                key_b64: key.key_b64().to_string(),
                salt_b64: Some(key.salt_b64.clone()),
                kdf_params: Some(key.kdf_params),
                updated_at_ms: 1000,
            },
        };
        let dummy_vault = RemoteVaultGist {
            gist_id: "vid_1".into(),
            html_url: String::new(),
            envelope: EncryptedVaultEnvelope {
                format: orbit_crypto::CLOUD_VAULT_ENVELOPE_FORMAT.into(),
                kdf: orbit_crypto::CLOUD_VAULT_KDF.into(),
                kdf_params: orbit_crypto::VaultKdfParams::default(),
                salt_b64: vault_salt.clone(),
                nonce_b64: "AAAAAAAAAAAAAAAA".into(),
                cipher: orbit_crypto::CLOUD_VAULT_CIPHER.into(),
                ciphertext_b64: "AAAA".into(),
                updated_at_ms: 1000,
            },
        };
        let recovered =
            derived_key_from_auto_unlock(&remote_key, Some(&dummy_vault)).expect("recover key");
        assert_eq!(recovered.salt_b64, vault_salt);
        assert_eq!(&*recovered.key_b64(), &*key.key_b64());
    }
}
