//! Pure-Rust deterministic lifecycle hooks engine.
//!
//! - User-global hooks are persisted in `$ORBIT_HOME/hooks.json` using atomic `0o600` writes.
//! - Project-scoped hooks live in `<authorized_cwd>/.orbit/hooks.json` and are NEVER executed
//!   until explicitly trusted by the user (SHA-256 digest tracked in
//!   `$ORBIT_HOME/trusted-project-hooks.json`). Any modification to `.orbit/hooks.json`
//!   immediately invalidates trust until re-approved.
//! - Command hooks spawn executables directly (no `sh -c` wrapper), scrub dangerous environment
//!   variables (`LD_*`, `DYLD_*`, `ORBIT_*`, `BASH_ENV`, `NODE_OPTIONS`, `PYTHONSTARTUP`,
//!   `GIT_SSH_COMMAND`, etc.), isolate child process groups on Unix (`process_group(0)`), enforce
//!   bounded timeouts (max 30s) with full process-group teardown on timeout, and cap stdout/stderr
//!   at 64 KiB.
//! - HTTP webhook hooks enforce DNS-resolved & IP-pinned SSRF protection (blocking link-local,
//!   metadata, `::ffff:`, NAT64, unspecified, multicast, broadcast) with `Policy::none()`
//!   redirects and mask sensitive headers/env values in all snapshots.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use orbit_protocol::{
    DEFAULT_HOOK_TIMEOUT_MS, HookAction, HookDecision, HookEventKind, HookExecutionRecord,
    HookScope, LifecycleHookRule, MAX_HOOK_EXECUTIONS, MAX_HOOK_OUTPUT_BYTES, MAX_HOOK_TIMEOUT_MS,
};
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, RwLock};
use uuid::Uuid;

use crate::mcp_servers::{
    is_blocked_mcp_env_key, is_blocked_ssrf_ip, mask_secret_value, resolve_and_validate_http_addrs,
};
use crate::workspace_files::canonicalize_workspace_root;

const HOOKS_FILE_VERSION: u32 = 1;
const MIN_HOOK_TIMEOUT_MS: u64 = 100;
const MAX_HOOKS_FILE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PREVIEW_CHARS: usize = 2048;
const MAX_HOOK_ARGS: usize = 128;
const MAX_HOOK_ENV_VARS: usize = 64;
const MAX_HOOK_HEADERS: usize = 64;
const MAX_HOOK_NAME_CHARS: usize = 120;
const MAX_HOOK_ID_CHARS: usize = 96;
const MAX_HOOK_DESC_CHARS: usize = 512;
const MAX_HOOKS_PER_FILE: usize = 200;

/// Snapshot returned to protocol callers (with secrets masked).
#[derive(Debug, Clone)]
pub struct LifecycleHooksSnapshotData {
    pub hooks: Vec<LifecycleHookRule>,
    pub recent_executions: Vec<HookExecutionRecord>,
    pub project_hooks_trusted: bool,
}

/// Aggregated result of dispatching an event across all matching enabled & trusted hooks.
#[derive(Debug, Clone)]
pub struct HookDispatchOutcome {
    pub decision: HookDecision,
    pub reason: Option<String>,
    pub updated_text: Option<String>,
    pub blocking_hook_name: Option<String>,
    pub records: Vec<HookExecutionRecord>,
}

impl Default for HookDispatchOutcome {
    fn default() -> Self {
        Self {
            decision: HookDecision::Allow,
            reason: None,
            updated_text: None,
            blocking_hook_name: None,
            records: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedHooksFile {
    #[serde(default = "default_file_version")]
    version: u32,
    #[serde(default)]
    hooks: Vec<LifecycleHookRule>,
}

impl Default for PersistedHooksFile {
    fn default() -> Self {
        Self {
            version: HOOKS_FILE_VERSION,
            hooks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustedProjectHooksFile {
    #[serde(default = "default_file_version")]
    version: u32,
    #[serde(default)]
    trusted: BTreeMap<String, String>,
}

impl Default for TrustedProjectHooksFile {
    fn default() -> Self {
        Self {
            version: HOOKS_FILE_VERSION,
            trusted: BTreeMap::new(),
        }
    }
}

fn default_file_version() -> u32 {
    HOOKS_FILE_VERSION
}

#[derive(Debug, Deserialize)]
struct HookOutputDecisionPayload {
    #[serde(default)]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    updated_text: Option<String>,
}

/// Pure-Rust deterministic lifecycle hooks engine owned by `DaemonRuntime`.
pub struct HooksEngine {
    home_dir: PathBuf,
    config_lock: AsyncMutex<()>,
    executions: RwLock<VecDeque<HookExecutionRecord>>,
}

impl HooksEngine {
    pub fn new(home_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            home_dir,
            config_lock: AsyncMutex::new(()),
            executions: RwLock::new(VecDeque::with_capacity(MAX_HOOK_EXECUTIONS)),
        })
    }

    /// List all user-global hooks plus optional project hooks (with secret headers/env masked).
    pub async fn list_snapshot(
        &self,
        authorized_cwd: Option<&Path>,
    ) -> Result<LifecycleHooksSnapshotData, String> {
        let _guard = self.config_lock.lock().await;
        self.build_snapshot_locked(authorized_cwd).await
    }

    /// Insert or update a lifecycle hook rule in user-global or project `.orbit/hooks.json`.
    pub async fn upsert_hook(
        &self,
        authorized_cwd: Option<&Path>,
        mut hook: LifecycleHookRule,
    ) -> Result<LifecycleHooksSnapshotData, String> {
        let _guard = self.config_lock.lock().await;
        normalize_and_validate_hook_id(&mut hook)?;

        let (target_path, private_mode, project_canonical_cwd) = match hook.scope {
            HookScope::UserGlobal => (user_hooks_path(&self.home_dir), true, None),
            HookScope::ProjectOrbit => {
                let cwd = authorized_cwd.ok_or_else(|| {
                    "A valid workspace directory is required for project hooks".to_string()
                })?;
                let (canonical_cwd, path) = resolve_project_hooks_path(cwd)?;
                (path, false, Some(canonical_cwd))
            }
        };

        let (mut file_data, was_trusted) = match &project_canonical_cwd {
            Some(canonical_cwd) => {
                let (parsed, trusted, _) =
                    load_project_hooks_and_trust(&self.home_dir, canonical_cwd, &target_path)?;
                (parsed, trusted)
            }
            None => (read_hooks_file(&target_path, hook.scope)?, true),
        };

        if let Some(existing) = file_data.hooks.iter().find(|h| h.id == hook.id) {
            preserve_masked_secrets(existing, &mut hook);
        } else if file_data.hooks.len() >= MAX_HOOKS_PER_FILE {
            return Err(format!(
                "Maximum number of hooks ({MAX_HOOKS_PER_FILE}) reached in this scope"
            ));
        }

        // Validate ONCE after restoring any preserved secret values, rejecting unresolved
        // `••••` placeholders and non-ASCII HTTP header values.
        normalize_and_validate_hook(&mut hook)?;
        // Never persist a stale `trusted` boolean on disk as source-of-truth; trust is derived
        // dynamically from scope + SHA-256 digest in `$ORBIT_HOME/trusted-project-hooks.json`.
        hook.trusted = matches!(hook.scope, HookScope::UserGlobal);

        if let Some(slot) = file_data.hooks.iter_mut().find(|h| h.id == hook.id) {
            *slot = hook;
        } else {
            file_data.hooks.push(hook);
        }

        let effective_private = private_mode || hooks_file_contains_secrets(&file_data.hooks);
        let written_digest = write_json_atomic(&target_path, &file_data, effective_private)?;

        // If the project hooks file was already explicitly trusted before this user-initiated
        // Orbit UI edit, pin the exact SHA-256 digest of the bytes we just atomically wrote
        // (zero disk re-read TOCTOU). Untrusted projects remain untrusted until explicitly approved.
        if let Some(ref canonical_cwd) = project_canonical_cwd
            && was_trusted
        {
            update_project_trust_digest(&self.home_dir, canonical_cwd, Some(&written_digest))?;
        }

        self.build_snapshot_locked(authorized_cwd).await
    }

    /// Delete a lifecycle hook rule by `hook_id` and `scope`.
    pub async fn delete_hook(
        &self,
        authorized_cwd: Option<&Path>,
        hook_id: &str,
        scope: HookScope,
    ) -> Result<LifecycleHooksSnapshotData, String> {
        let _guard = self.config_lock.lock().await;
        let clean_id = hook_id.trim();
        if clean_id.is_empty() {
            return Err("Hook ID cannot be empty".to_string());
        }

        let (target_path, private_mode, project_canonical_cwd) = match scope {
            HookScope::UserGlobal => (user_hooks_path(&self.home_dir), true, None),
            HookScope::ProjectOrbit => {
                let cwd = authorized_cwd.ok_or_else(|| {
                    "A valid workspace directory is required for project hooks".to_string()
                })?;
                let (canonical_cwd, path) = resolve_project_hooks_path(cwd)?;
                (path, false, Some(canonical_cwd))
            }
        };

        let (mut file_data, was_trusted) = match &project_canonical_cwd {
            Some(canonical_cwd) => {
                let (parsed, trusted, _) =
                    load_project_hooks_and_trust(&self.home_dir, canonical_cwd, &target_path)?;
                (parsed, trusted)
            }
            None => (read_hooks_file(&target_path, scope)?, true),
        };

        let before = file_data.hooks.len();
        file_data.hooks.retain(|h| h.id != clean_id);
        if file_data.hooks.len() == before {
            return Err(format!("Hook `{clean_id}` not found"));
        }

        let effective_private = private_mode || hooks_file_contains_secrets(&file_data.hooks);
        let written_digest = write_json_atomic(&target_path, &file_data, effective_private)?;

        if let Some(ref canonical_cwd) = project_canonical_cwd
            && was_trusted
        {
            update_project_trust_digest(&self.home_dir, canonical_cwd, Some(&written_digest))?;
        }

        self.build_snapshot_locked(authorized_cwd).await
    }

    /// Explicitly approve or revoke SHA-256 trust for `<authorized_cwd>/.orbit/hooks.json`.
    pub async fn set_project_trust(
        &self,
        authorized_cwd: &Path,
        trusted: bool,
    ) -> Result<LifecycleHooksSnapshotData, String> {
        let _guard = self.config_lock.lock().await;
        let (canonical_cwd, proj_path) = resolve_project_hooks_path(authorized_cwd)?;
        if trusted {
            let bytes = read_bounded_regular_file(&proj_path)?.ok_or_else(|| {
                "No `.orbit/hooks.json` file exists in this workspace to trust".to_string()
            })?;
            // Validate JSON parses cleanly before approving trust.
            let _ = parse_hooks_slice(&bytes, &proj_path, HookScope::ProjectOrbit)?;
            let digest = sha256_hex(&bytes);
            update_project_trust_digest(&self.home_dir, &canonical_cwd, Some(&digest))?;
        } else {
            update_project_trust_digest(&self.home_dir, &canonical_cwd, None)?;
        }
        self.build_snapshot_locked(Some(&canonical_cwd)).await
    }

    /// Dry-run test a specific hook by `hook_id` (optionally prefixed with `user_global:` or
    /// `project_orbit:` to disambiguate identical IDs across scopes) against `sample_payload`.
    /// Refuses to execute untrusted project hooks.
    pub async fn test_hook(
        &self,
        authorized_cwd: Option<&Path>,
        hook_id: &str,
        sample_payload: Value,
    ) -> Result<HookExecutionRecord, String> {
        let clean_id = hook_id.trim();
        if clean_id.is_empty() {
            return Err("Hook ID cannot be empty".to_string());
        }

        let (expected_scope, bare_id) = parse_scoped_hook_id(clean_id);

        let (rule, exec_cwd) = {
            let _guard = self.config_lock.lock().await;
            let (all_rules, project_trusted, canonical_cwd) =
                self.load_all_unmasked_locked(authorized_cwd)?;
            let found = all_rules
                .into_iter()
                .find(|h| h.id == bare_id && expected_scope.is_none_or(|scope| h.scope == scope))
                .ok_or_else(|| format!("Hook `{clean_id}` not found"))?;
            if found.scope == HookScope::ProjectOrbit && !project_trusted {
                return Err(
                    "Project hooks in `.orbit/hooks.json` are untrusted. Approve trust before running."
                        .to_string(),
                );
            }
            (found, canonical_cwd)
        };

        let target = sample_payload
            .get("target")
            .and_then(Value::as_str)
            .or_else(|| sample_payload.get("path").and_then(Value::as_str))
            .or_else(|| sample_payload.get("tool").and_then(Value::as_str))
            .or_else(|| sample_payload.get("provider").and_then(Value::as_str))
            .map(ToOwned::to_owned);

        let envelope = json!({
            "event": rule.event.slug(),
            "dry_run": true,
            "cwd": exec_cwd.as_ref().map(|p| p.to_string_lossy().to_string()),
            "target": target,
            "payload": sample_payload,
        });

        let (record, _updated_text) =
            execute_hook_rule(&rule, exec_cwd.as_deref(), target.as_deref(), &envelope).await;
        self.push_execution(record.clone()).await;
        Ok(record)
    }

    /// Dispatch a live lifecycle event to all matching, enabled, trusted hooks.
    ///
    /// - Untrusted project hooks are strictly skipped (never executed).
    /// - If a hook returns `HookDecision::Block`, execution short-circuits immediately.
    /// - If a hook returns `HookDecision::Modify` with `updated_text`, subsequent hooks receive
    ///   the updated text in `payload["prompt"]` / `payload["updated_text"]`.
    pub async fn dispatch_event(
        &self,
        event: HookEventKind,
        cwd: Option<&Path>,
        target: Option<&str>,
        mut payload: Value,
    ) -> HookDispatchOutcome {
        let (matching_rules, exec_cwd) = {
            let _guard = self.config_lock.lock().await;
            let Ok((all_rules, project_trusted, canonical_cwd)) =
                self.load_all_unmasked_locked(cwd)
            else {
                return HookDispatchOutcome::default();
            };

            let filtered: Vec<LifecycleHookRule> = all_rules
                .into_iter()
                .filter(|rule| {
                    rule.enabled
                        && rule.event == event
                        && (rule.scope == HookScope::UserGlobal || project_trusted)
                        && matches_hook_pattern(rule.matcher.as_deref(), target)
                })
                .collect();
            (filtered, canonical_cwd)
        };

        if matching_rules.is_empty() {
            return HookDispatchOutcome::default();
        }

        let mut outcome = HookDispatchOutcome::default();

        for rule in matching_rules {
            let (record, updated_text) =
                execute_hook_rule(&rule, exec_cwd.as_deref(), target, &payload).await;
            self.push_execution(record.clone()).await;

            match record.decision {
                HookDecision::Block => {
                    outcome.decision = HookDecision::Block;
                    outcome.reason = record.reason.clone();
                    outcome.blocking_hook_name = Some(rule.name.clone());
                    outcome.records.push(record);
                    break;
                }
                HookDecision::Modify => {
                    if let Some(new_text) = updated_text {
                        outcome.decision = HookDecision::Modify;
                        outcome.updated_text = Some(new_text.clone());
                        outcome.reason = record.reason.clone();
                        if let Some(obj) = payload.as_object_mut() {
                            if obj.contains_key("prompt") {
                                obj.insert("prompt".to_string(), Value::String(new_text));
                            } else {
                                obj.insert("updated_text".to_string(), Value::String(new_text));
                            }
                        }
                    }
                    outcome.records.push(record);
                }
                HookDecision::Allow => {
                    outcome.records.push(record);
                }
            }
        }

        outcome
    }

    async fn push_execution(&self, record: HookExecutionRecord) {
        let mut guard = self.executions.write().await;
        if guard.len() >= MAX_HOOK_EXECUTIONS {
            guard.pop_back();
        }
        guard.push_front(record);
    }

    async fn build_snapshot_locked(
        &self,
        authorized_cwd: Option<&Path>,
    ) -> Result<LifecycleHooksSnapshotData, String> {
        let (all_rules, project_trusted, _) = self.load_all_unmasked_locked(authorized_cwd)?;
        let masked_hooks = all_rules.into_iter().map(mask_hook_for_wire).collect();
        let recent_executions = self.executions.read().await.iter().cloned().collect();
        Ok(LifecycleHooksSnapshotData {
            hooks: masked_hooks,
            recent_executions,
            project_hooks_trusted: project_trusted,
        })
    }

    fn load_all_unmasked_locked(
        &self,
        authorized_cwd: Option<&Path>,
    ) -> Result<(Vec<LifecycleHookRule>, bool, Option<PathBuf>), String> {
        let mut hooks = Vec::new();
        let user_path = user_hooks_path(&self.home_dir);
        let user_file = read_hooks_file(&user_path, HookScope::UserGlobal)?;
        for mut h in user_file.hooks {
            h.scope = HookScope::UserGlobal;
            h.trusted = true;
            hooks.push(h);
        }

        let mut project_trusted = false;
        let mut canonical_cwd_out = None;
        if let Some(cwd) = authorized_cwd {
            match resolve_project_hooks_path(cwd) {
                Ok((canonical_cwd, proj_path)) => {
                    canonical_cwd_out = Some(canonical_cwd.clone());
                    // Single-read trust verification + deserialization from the exact same buffer
                    // (eliminates TOCTOU race between SHA-256 check and JSON parsing).
                    match load_project_hooks_and_trust(&self.home_dir, &canonical_cwd, &proj_path) {
                        Ok((proj_file, trusted, _)) => {
                            project_trusted = trusted;
                            for mut h in proj_file.hooks {
                                h.scope = HookScope::ProjectOrbit;
                                h.trusted = project_trusted;
                                hooks.push(h);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = %proj_path.display(),
                                error = %e,
                                "Skipping malformed or unsafe project `.orbit/hooks.json`"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(cwd = %cwd.display(), error = %e, "Skipping project hooks path");
                }
            }
        }

        // Deterministic sort: event order, scope (UserGlobal before ProjectOrbit), then name/id.
        hooks.sort_by(|a, b| {
            a.event
                .cmp(&b.event)
                .then_with(|| scope_rank(a.scope).cmp(&scope_rank(b.scope)))
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
                .then_with(|| a.id.cmp(&b.id))
        });

        Ok((hooks, project_trusted, canonical_cwd_out))
    }
}

fn scope_rank(scope: HookScope) -> u8 {
    match scope {
        HookScope::UserGlobal => 0,
        HookScope::ProjectOrbit => 1,
    }
}

fn user_hooks_path(home_dir: &Path) -> PathBuf {
    home_dir.join("hooks.json")
}

fn trusted_project_hooks_path(home_dir: &Path) -> PathBuf {
    home_dir.join("trusted-project-hooks.json")
}

fn parse_scoped_hook_id(raw: &str) -> (Option<HookScope>, &str) {
    if let Some(rest) = raw.strip_prefix("user_global:") {
        (Some(HookScope::UserGlobal), rest)
    } else if let Some(rest) = raw.strip_prefix("project_orbit:") {
        (Some(HookScope::ProjectOrbit), rest)
    } else {
        (None, raw)
    }
}

/// Strictly resolve `<canonical_cwd>/.orbit/hooks.json`, unconditionally rejecting symlinks on
/// both `.orbit` and `.orbit/hooks.json` for reads and writes alike.
pub fn resolve_project_hooks_path(cwd: &Path) -> Result<(PathBuf, PathBuf), String> {
    if let Some(raw) = cwd.to_str()
        && raw.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Workspace path contains invalid control characters".to_string());
    }
    if cwd.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Workspace path must not contain `..` traversal segments".to_string());
    }

    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let orbit_dir = canonical_cwd.join(".orbit");
    if let Ok(meta) = fs::symlink_metadata(&orbit_dir) {
        if meta.file_type().is_symlink() {
            return Err("Refusing to access symlinked `.orbit` directory".to_string());
        }
        if !meta.is_dir() {
            return Err("`.orbit` exists but is not a directory".to_string());
        }
    }

    let target = orbit_dir.join("hooks.json");
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err("Refusing to access symlinked `.orbit/hooks.json`".to_string());
        }
        if !meta.is_file() {
            return Err("`.orbit/hooks.json` exists but is not a regular file".to_string());
        }
    }

    Ok((canonical_cwd, target))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Read a regular, non-symlink file into memory with a strict `MAX_HOOKS_FILE_BYTES` (4 MiB) cap.
/// Returns `Ok(None)` if the file does not exist.
fn read_bounded_regular_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Failed to inspect `{}`: {e}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return Err(format!("Refusing to read symlinked `{}`", path.display()));
    }
    if !meta.is_file() {
        return Err(format!("`{}` is not a regular file", path.display()));
    }

    let file = File::open(path).map_err(|e| format!("Failed to open `{}`: {e}", path.display()))?;
    let opened_meta = file
        .metadata()
        .map_err(|e| format!("Failed to stat `{}`: {e}", path.display()))?;
    if !opened_meta.is_file() {
        return Err(format!("`{}` is not a regular file", path.display()));
    }

    let mut bytes = Vec::new();
    file.take((MAX_HOOKS_FILE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Failed to read `{}`: {e}", path.display()))?;
    if bytes.len() > MAX_HOOKS_FILE_BYTES {
        return Err(format!(
            "`{}` exceeds maximum size limit ({} bytes)",
            path.display(),
            MAX_HOOKS_FILE_BYTES
        ));
    }
    Ok(Some(bytes))
}

fn read_trusted_registry(home_dir: &Path) -> TrustedProjectHooksFile {
    let path = trusted_project_hooks_path(home_dir);
    let Ok(Some(bytes)) = read_bounded_regular_file(&path) else {
        return TrustedProjectHooksFile::default();
    };
    serde_json::from_slice::<TrustedProjectHooksFile>(&bytes).unwrap_or_default()
}

fn parse_hooks_slice(
    bytes: &[u8],
    path: &Path,
    scope: HookScope,
) -> Result<PersistedHooksFile, String> {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(PersistedHooksFile::default());
    }
    let mut parsed: PersistedHooksFile = serde_json::from_slice(bytes)
        .map_err(|e| format!("Invalid hooks JSON in `{}`: {e}", path.display()))?;
    for rule in &mut parsed.hooks {
        rule.scope = scope;
        if rule.timeout_ms == 0 {
            rule.timeout_ms = DEFAULT_HOOK_TIMEOUT_MS;
        } else {
            rule.timeout_ms = rule
                .timeout_ms
                .clamp(MIN_HOOK_TIMEOUT_MS, MAX_HOOK_TIMEOUT_MS);
        }
    }
    Ok(parsed)
}

/// Single-read helper for `<canonical_cwd>/.orbit/hooks.json`: reads the file ONCE into a
/// size-bounded buffer, computes SHA-256 on that exact buffer, checks trust against
/// `$ORBIT_HOME/trusted-project-hooks.json`, and deserializes from the same buffer.
fn load_project_hooks_and_trust(
    home_dir: &Path,
    canonical_cwd: &Path,
    proj_path: &Path,
) -> Result<(PersistedHooksFile, bool, Option<String>), String> {
    let Some(bytes) = read_bounded_regular_file(proj_path)? else {
        return Ok((PersistedHooksFile::default(), false, None));
    };
    let digest = sha256_hex(&bytes);
    let registry = read_trusted_registry(home_dir);
    let key = canonical_cwd.to_string_lossy().to_string();
    let is_trusted = registry.trusted.get(&key).is_some_and(|h| h == &digest);
    let parsed = parse_hooks_slice(&bytes, proj_path, HookScope::ProjectOrbit)?;
    Ok((parsed, is_trusted, Some(digest)))
}

fn update_project_trust_digest(
    home_dir: &Path,
    canonical_cwd: &Path,
    digest: Option<&str>,
) -> Result<(), String> {
    let mut registry = read_trusted_registry(home_dir);
    let key = canonical_cwd.to_string_lossy().to_string();
    if let Some(d) = digest {
        registry.trusted.insert(key, d.to_string());
    } else {
        registry.trusted.remove(&key);
    }
    let path = trusted_project_hooks_path(home_dir);
    let _ = write_json_atomic(&path, &registry, true)?;
    Ok(())
}

fn read_hooks_file(path: &Path, scope: HookScope) -> Result<PersistedHooksFile, String> {
    let Some(bytes) = read_bounded_regular_file(path)? else {
        return Ok(PersistedHooksFile::default());
    };
    parse_hooks_slice(&bytes, path, scope)
}

struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn write_json_atomic<T: Serialize>(
    path: &Path,
    value: &T,
    private_mode: bool,
) -> Result<String, String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid config path `{}`", path.display()))?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create directory: {e}"))?;

    let tmp_name = format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|s| s.to_str()).unwrap_or("hooks"),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);
    let mut guard = TempFileGuard::new(tmp_path.clone());

    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("Failed to serialize hooks JSON: {e}"))?;
    bytes.push(b'\n');
    let digest = sha256_hex(&bytes);

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if private_mode {
            opts.mode(0o600);
        }
    }

    let mut file = opts
        .open(&tmp_path)
        .map_err(|e| format!("Failed to create temporary hooks file: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Failed to write hooks file: {e}"))?;
    drop(file);

    #[cfg(unix)]
    if private_mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set 0600 permissions on hooks file: {e}"))?;
    }

    fs::rename(&tmp_path, path)
        .map_err(|e| format!("Failed to persist `{}`: {e}", path.display()))?;
    guard.disarm();
    Ok(digest)
}

/// Returns `true` if `key` is a dangerous process environment variable that could hijack dynamic
/// linkers, shells, runtimes, Git, OpenSSL, or leak Orbit daemon secrets.
pub fn is_dangerous_env_key(key: &str) -> bool {
    if is_blocked_mcp_env_key(key) {
        return true;
    }
    let upper = key.trim().to_ascii_uppercase();
    if upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("ORBIT_")
        || upper.starts_with("BASH_FUNC_")
    {
        return true;
    }
    matches!(
        upper.as_str(),
        "BASH_ENV"
            | "ENV"
            | "ZDOTDIR"
            | "IFS"
            | "SHELLOPTS"
            | "PS4"
            | "OPENSSL_CONF"
            | "NODE_OPTIONS"
            | "NODE_EXTRA_CA_CERTS"
            | "PYTHONSTARTUP"
            | "PYTHONPATH"
            | "PYTHONHOME"
            | "GIT_SSH_COMMAND"
            | "GIT_PROXY_COMMAND"
            | "GIT_EXEC_PATH"
            | "GIT_TEMPLATE_DIR"
            | "RUBYOPT"
            | "PERL5OPT"
            | "PERL5LIB"
            | "JAVA_TOOL_OPTIONS"
            | "CLASSPATH"
            | "GCONV_PATH"
            | "HOSTALIASES"
            | "LOCALDOMAIN"
            | "RES_OPTIONS"
    )
}

fn is_secret_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    upper.contains("TOKEN")
        || upper.contains("SECRET")
        || upper.contains("KEY")
        || upper.contains("PASS")
        || upper.contains("AUTH")
        || upper.contains("BEARER")
        || upper.contains("CREDENTIAL")
        || upper.contains("COOKIE")
        || upper.contains("SIGNATURE")
        || upper.contains("PRIVATE")
}

fn hooks_file_contains_secrets(hooks: &[LifecycleHookRule]) -> bool {
    hooks.iter().any(|h| match &h.action {
        HookAction::Command { env, .. } => env.keys().any(|k| is_secret_key(k)),
        HookAction::HttpWebhook { headers, .. } => headers.keys().any(|k| is_secret_key(k)),
    })
}

fn mask_hook_for_wire(mut rule: LifecycleHookRule) -> LifecycleHookRule {
    match &mut rule.action {
        HookAction::Command { env, .. } => {
            for (k, v) in env.iter_mut() {
                if is_secret_key(k) {
                    *v = mask_secret_value(v);
                }
            }
        }
        HookAction::HttpWebhook { headers, .. } => {
            for (k, v) in headers.iter_mut() {
                if is_secret_key(k) {
                    *v = mask_secret_value(v);
                }
            }
        }
    }
    rule
}

fn is_masked_placeholder(val: &str) -> bool {
    val.trim().starts_with("••••")
}

fn preserve_masked_secrets(existing: &LifecycleHookRule, incoming: &mut LifecycleHookRule) {
    match (&existing.action, &mut incoming.action) {
        (HookAction::Command { env: old_env, .. }, HookAction::Command { env: new_env, .. }) => {
            for (k, new_val) in new_env.iter_mut() {
                let key = k.trim();
                let trimmed = new_val.trim();
                if is_secret_key(key)
                    && is_masked_placeholder(trimmed)
                    && let Some(old_val) = old_env.get(key)
                    && (trimmed == mask_secret_value(old_val) || trimmed == "••••")
                {
                    *new_val = old_val.clone();
                }
            }
        }
        (
            HookAction::HttpWebhook {
                headers: old_headers,
                ..
            },
            HookAction::HttpWebhook {
                headers: new_headers,
                ..
            },
        ) => {
            for (k, new_val) in new_headers.iter_mut() {
                let key = k.trim();
                let trimmed = new_val.trim();
                if is_secret_key(key)
                    && is_masked_placeholder(trimmed)
                    && let Some((_, old_val)) = old_headers
                        .iter()
                        .find(|(ok, _)| ok.trim().eq_ignore_ascii_case(key))
                    && (trimmed == mask_secret_value(old_val) || trimmed == "••••")
                {
                    *new_val = old_val.clone();
                }
            }
        }
        _ => {}
    }
}

fn is_forbidden_http_header_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower.starts_with("proxy-")
        || matches!(
            lower.as_str(),
            "host"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "te"
                | "trailer"
                | "upgrade"
        )
}

fn normalize_url_host(host_raw: &str) -> String {
    host_raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host_raw)
        .to_ascii_lowercase()
}

/// Validate a webhook URL upfront (scheme, credentials, control chars, trailing dot,
/// metadata hostnames, SSRF IPs) and normalize the host on the returned `reqwest::Url` so
/// DNS-pinned `resolve_to_addrs` lookups always match the exact request host.
pub fn validate_webhook_url(raw_url: &str) -> Result<reqwest::Url, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err("Webhook URL cannot be empty".to_string());
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Webhook URL contains invalid whitespace or control characters".to_string());
    }
    let mut parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid webhook URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "Unsupported webhook URL scheme `{}` (only http:// and https:// are allowed)",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "Webhook URL must not embed credentials; configure Authorization headers instead"
                .to_string(),
        );
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Webhook URL is missing a host".to_string())?
        .to_string();
    if host_raw.ends_with('.') {
        return Err("Webhook URL host must not end with a trailing dot".to_string());
    }
    let host_norm = normalize_url_host(&host_raw);
    if matches!(
        host_norm.as_str(),
        "metadata.google.internal" | "metadata.goog" | "metadata" | "instance-data"
    ) {
        return Err("Refusing to connect to cloud metadata host".to_string());
    }
    if let Ok(ip) = host_norm.parse::<IpAddr>()
        && is_blocked_ssrf_ip(ip)
    {
        return Err(format!(
            "Refusing to connect to link-local, metadata, or unspecified IP `{ip}`"
        ));
    }
    if !host_raw.starts_with('[') && host_raw != host_norm {
        parsed
            .set_host(Some(&host_norm))
            .map_err(|_| format!("Invalid webhook host `{host_raw}`"))?;
    }
    Ok(parsed)
}

fn normalize_and_validate_hook_id(hook: &mut LifecycleHookRule) -> Result<(), String> {
    hook.id = hook.id.trim().to_string();
    if hook.id.is_empty() {
        hook.id = format!("hk_{}", Uuid::new_v4().simple());
    }
    if hook.id.chars().count() > MAX_HOOK_ID_CHARS
        || hook.id.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Hook ID contains invalid characters or is too long".to_string());
    }
    Ok(())
}

fn normalize_and_validate_hook(hook: &mut LifecycleHookRule) -> Result<(), String> {
    normalize_and_validate_hook_id(hook)?;

    hook.name = hook.name.trim().to_string();
    if hook.name.is_empty() {
        return Err("Hook name cannot be empty".to_string());
    }
    if hook.name.chars().count() > MAX_HOOK_NAME_CHARS
        || hook.name.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Hook name contains invalid characters or is too long".to_string());
    }

    hook.matcher = hook
        .matcher
        .take()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    if let Some(ref m) = hook.matcher
        && (m.chars().count() > 256 || m.chars().any(|c| c == '\0' || c.is_control()))
    {
        return Err("Hook matcher contains invalid characters or is too long".to_string());
    }

    hook.description = hook
        .description
        .take()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty());
    if let Some(ref d) = hook.description
        && (d.chars().count() > MAX_HOOK_DESC_CHARS || d.contains('\0'))
    {
        return Err("Hook description is too long or contains NUL bytes".to_string());
    }

    hook.timeout_ms = if hook.timeout_ms == 0 {
        DEFAULT_HOOK_TIMEOUT_MS
    } else {
        hook.timeout_ms
            .clamp(MIN_HOOK_TIMEOUT_MS, MAX_HOOK_TIMEOUT_MS)
    };

    match &mut hook.action {
        HookAction::Command { command, args, env } => {
            *command = command.trim().to_string();
            if command.is_empty() {
                return Err("Hook command executable cannot be empty".to_string());
            }
            if command.chars().any(|c| c == '\0' || c.is_control()) {
                return Err("Hook command contains invalid control characters".to_string());
            }
            if args.len() > MAX_HOOK_ARGS {
                return Err(format!("Too many command arguments (max {MAX_HOOK_ARGS})"));
            }
            for arg in args.iter() {
                if arg.contains('\0') {
                    return Err("Hook command argument contains NUL byte".to_string());
                }
            }
            if env.len() > MAX_HOOK_ENV_VARS {
                return Err(format!(
                    "Too many environment variables (max {MAX_HOOK_ENV_VARS})"
                ));
            }
            let mut clean_env = BTreeMap::new();
            for (k, v) in env.iter() {
                let key = k.trim();
                if key.is_empty()
                    || key.contains('=')
                    || key.chars().any(|c| c == '\0' || c.is_control())
                {
                    return Err(format!("Invalid environment variable name `{k}`"));
                }
                if is_dangerous_env_key(key) {
                    return Err(format!(
                        "Environment variable `{key}` is blocked for security"
                    ));
                }
                if v.contains('\0') {
                    return Err(format!(
                        "Environment variable `{key}` value contains NUL byte"
                    ));
                }
                if is_secret_key(key) && is_masked_placeholder(v) {
                    return Err(format!(
                        "Environment variable `{key}` contains an unresolved masked placeholder; provide a real value"
                    ));
                }
                clean_env.insert(key.to_string(), v.clone());
            }
            *env = clean_env;
        }
        HookAction::HttpWebhook { url, headers } => {
            let parsed = validate_webhook_url(url)?;
            *url = parsed.to_string();
            if headers.len() > MAX_HOOK_HEADERS {
                return Err(format!("Too many webhook headers (max {MAX_HOOK_HEADERS})"));
            }
            let mut clean_headers = BTreeMap::new();
            for (k, v) in headers.iter() {
                let key = k.trim();
                if key.is_empty() || is_forbidden_http_header_name(key) {
                    return Err(format!("Forbidden or empty HTTP header `{k}`"));
                }
                if HeaderName::from_bytes(key.as_bytes()).is_err() {
                    return Err(format!("Invalid HTTP header name `{key}`"));
                }
                if is_secret_key(key) && is_masked_placeholder(v) {
                    return Err(format!(
                        "HTTP header `{key}` contains an unresolved masked placeholder; provide a real value"
                    ));
                }
                if !v.is_ascii()
                    || v.chars().any(|c| c == '\r' || c == '\n' || c == '\0')
                    || HeaderValue::from_str(v).is_err()
                {
                    return Err(format!(
                        "HTTP header `{key}` value must contain valid ASCII characters"
                    ));
                }
                clean_headers.insert(key.to_string(), v.clone());
            }
            *headers = clean_headers;
        }
    }

    Ok(())
}

/// Deterministic matcher supporting exact, prefix, `|` alternatives, and `*` wildcard glob matching
/// against tool names, file paths, or provider slugs.
pub fn matches_hook_pattern(matcher: Option<&str>, target: Option<&str>) -> bool {
    let Some(raw_matcher) = matcher.map(str::trim).filter(|m| !m.is_empty()) else {
        return true;
    };
    if raw_matcher == "*" {
        return true;
    }
    let Some(raw_target) = target.map(str::trim).filter(|t| !t.is_empty()) else {
        return false;
    };

    for alt in raw_matcher
        .split('|')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if alt == "*" {
            return true;
        }
        if alt.contains('*') {
            if wildcard_match(alt, raw_target) {
                return true;
            }
        } else if alt.ends_with('/') {
            if raw_target.starts_with(alt) {
                return true;
            }
        } else if raw_target == alt
            || raw_target
                .strip_prefix(alt)
                .is_some_and(|rest| rest.starts_with('/') || rest.starts_with(':'))
        {
            return true;
        }
    }
    false
}

/// Non-backtracking O(n*m) two-pointer `*` wildcard matcher (zero ReDoS risk).
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_idx: Option<usize> = None;
    let mut match_idx: usize = 0;

    while ti < t.len() {
        if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_idx = Some(pi);
            match_idx = ti;
            pi += 1;
        } else if let Some(si) = star_idx {
            pi = si + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn now_unix_ms() -> u64 {
    Utc::now().timestamp_millis().max(0) as u64
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

fn parse_hook_json_decision(raw: &str) -> Option<(HookDecision, Option<String>, Option<String>)> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed = serde_json::from_str::<HookOutputDecisionPayload>(trimmed).ok()?;
    let dec_str = parsed.decision?.trim().to_ascii_lowercase();
    let decision = match dec_str.as_str() {
        "allow" | "approve" | "ok" => HookDecision::Allow,
        "block" | "deny" | "reject" => HookDecision::Block,
        "modify" | "rewrite" => HookDecision::Modify,
        _ => return None,
    };
    let reason = parsed
        .reason
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty());
    let updated_text = parsed.updated_text;
    Some((decision, reason, updated_text))
}

async fn execute_hook_rule(
    rule: &LifecycleHookRule,
    cwd: Option<&Path>,
    target: Option<&str>,
    payload: &Value,
) -> (HookExecutionRecord, Option<String>) {
    match &rule.action {
        HookAction::Command { command, args, env } => {
            execute_command_hook(rule, command, args, env, cwd, target, payload).await
        }
        HookAction::HttpWebhook { url, headers } => {
            execute_webhook_hook(rule, url, headers, target, payload).await
        }
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (meta.permissions().mode() & 0o111) != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_executable_path(cmd: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    if trimmed.contains('/') || trimmed.contains('\\') {
        let candidate = if p.is_absolute() {
            p.to_path_buf()
        } else if let Some(dir) = cwd {
            dir.join(p)
        } else {
            p.to_path_buf()
        };
        return is_executable_file(&candidate).then_some(candidate);
    }

    let mut search_dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();

    #[cfg(unix)]
    {
        for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
            let ep = PathBuf::from(extra);
            if !search_dirs.contains(&ep) {
                search_dirs.push(ep);
            }
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            for rel in [".local/bin", ".cargo/bin", ".npm-global/bin"] {
                let ep = home.join(rel);
                if !search_dirs.contains(&ep) {
                    search_dirs.push(ep);
                }
            }
        }
    }

    for dir in search_dirs {
        let candidate = dir.join(trimmed);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

async fn kill_process_group_by_pid(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        let pgid_arg = format!("-{pid}");
        let kill_bin = if Path::new("/bin/kill").is_file() {
            "/bin/kill"
        } else {
            "kill"
        };
        let _ = Command::new(kill_bin)
            .args(["-KILL", "--", &pgid_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    #[cfg(not(unix))]
    let _ = pid;
}

async fn kill_process_group_and_reap(child: &mut tokio::process::Child, pid: Option<u32>) {
    kill_process_group_by_pid(child.id().or(pid)).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn await_task_bounded<T: Default>(
    handle: tokio::task::JoinHandle<T>,
    wait: Duration,
) -> (T, bool) {
    let abort = handle.abort_handle();
    match tokio::time::timeout(wait, handle).await {
        Ok(Ok(val)) => (val, false),
        Ok(Err(_)) => (T::default(), false),
        Err(_) => {
            abort.abort();
            (T::default(), true)
        }
    }
}

/// Read up to `limit` bytes from `reader`, draining any excess bytes to EOF so the child never
/// blocks on a full pipe buffer.
async fn read_bounded_stream<R: AsyncRead + Unpin>(mut reader: R, limit: usize) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if captured.len() < limit {
                    let take = n.min(limit - captured.len());
                    captured.extend_from_slice(&chunk[..take]);
                }
            }
        }
    }
    captured
}

async fn execute_command_hook(
    rule: &LifecycleHookRule,
    command: &str,
    args: &[String],
    hook_env: &BTreeMap<String, String>,
    cwd: Option<&Path>,
    target: Option<&str>,
    payload: &Value,
) -> (HookExecutionRecord, Option<String>) {
    let started = Instant::now();
    let exec_id = format!("hx_{}", Uuid::new_v4().simple());
    let timeout_ms = rule
        .timeout_ms
        .clamp(MIN_HOOK_TIMEOUT_MS, MAX_HOOK_TIMEOUT_MS);

    let fail_open = |reason: String, stderr: String, exit_code: Option<i32>| {
        (
            HookExecutionRecord {
                id: exec_id.clone(),
                hook_id: rule.id.clone(),
                hook_name: rule.name.clone(),
                event: rule.event,
                matched_target: target.map(ToOwned::to_owned),
                decision: HookDecision::Allow,
                reason: Some(reason),
                exit_code,
                duration_ms: started.elapsed().as_millis() as u64,
                stdout_preview: String::new(),
                stderr_preview: truncate_preview(&stderr, MAX_PREVIEW_CHARS),
                executed_at_ms: now_unix_ms(),
            },
            None,
        )
    };

    let cmd_owned = command.to_string();
    let cwd_owned = cwd.map(Path::to_path_buf);
    let resolved = tokio::task::spawn_blocking(move || {
        resolve_executable_path(&cmd_owned, cwd_owned.as_deref())
    })
    .await
    .ok()
    .flatten();
    let Some(resolved) = resolved else {
        return fail_open(
            format!("Executable `{command}` not found or not executable"),
            format!("Executable `{command}` not found or not executable"),
            None,
        );
    };

    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(dir) = cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    // Isolate into its own process group on Unix so timeouts terminate all descendants safely
    // without requiring any `unsafe` code.
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    // Scrub dangerous environment variables from inherited environment + hook config.
    cmd.env_clear();
    for (k, v) in std::env::vars_os() {
        let key_lossy = k.to_string_lossy();
        if !is_dangerous_env_key(&key_lossy) {
            cmd.env(k, v);
        }
    }
    for (k, v) in hook_env {
        if !is_dangerous_env_key(k) {
            cmd.env(k, v);
        }
    }

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return fail_open(
                format!("Failed to spawn `{}`: {e}", resolved.display()),
                e.to_string(),
                None,
            );
        }
    };
    let child_pid = child.id();

    let stdin_bytes = serde_json::to_vec(payload).unwrap_or_else(|_| b"{}".to_vec());
    let stdin_opt = child.stdin.take();
    let stdout_opt = child.stdout.take();
    let stderr_opt = child.stderr.take();

    let stdin_task = tokio::spawn(async move {
        if let Some(mut stdin) = stdin_opt {
            let _ = stdin.write_all(&stdin_bytes).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.shutdown().await;
        }
    });
    let stdout_task = tokio::spawn(async move {
        match stdout_opt {
            Some(s) => read_bounded_stream(s, MAX_HOOK_OUTPUT_BYTES).await,
            None => Vec::new(),
        }
    });
    let stderr_task = tokio::spawn(async move {
        match stderr_opt {
            Some(s) => read_bounded_stream(s, MAX_HOOK_OUTPUT_BYTES).await,
            None => Vec::new(),
        }
    });

    let wait_res = tokio::time::timeout_at(deadline, child.wait()).await;

    let exit_status = match wait_res {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            kill_process_group_and_reap(&mut child, child_pid).await;
            stdin_task.abort();
            stdout_task.abort();
            stderr_task.abort();
            return fail_open(format!("Hook process wait error: {e}"), e.to_string(), None);
        }
        Err(_) => {
            kill_process_group_and_reap(&mut child, child_pid).await;
            stdin_task.abort();
            let (stdout_bytes, _) =
                await_task_bounded(stdout_task, Duration::from_millis(200)).await;
            let (stderr_bytes, _) =
                await_task_bounded(stderr_task, Duration::from_millis(200)).await;
            let stdout_str = String::from_utf8_lossy(&stdout_bytes).to_string();
            let stderr_str = String::from_utf8_lossy(&stderr_bytes).to_string();
            return (
                HookExecutionRecord {
                    id: exec_id,
                    hook_id: rule.id.clone(),
                    hook_name: rule.name.clone(),
                    event: rule.event,
                    matched_target: target.map(ToOwned::to_owned),
                    decision: HookDecision::Allow,
                    reason: Some(format!("Hook timed out after {timeout_ms}ms")),
                    exit_code: None,
                    duration_ms: started.elapsed().as_millis() as u64,
                    stdout_preview: truncate_preview(&stdout_str, MAX_PREVIEW_CHARS),
                    stderr_preview: truncate_preview(&stderr_str, MAX_PREVIEW_CHARS),
                    executed_at_ms: now_unix_ms(),
                },
                None,
            );
        }
    };

    // Bound post-exit stdin/stdout/stderr collection so a detached grandchild that inherited
    // open pipes can never hang the hook beyond `min(remaining_deadline, 500ms)`.
    let drain_budget = deadline
        .saturating_duration_since(tokio::time::Instant::now())
        .min(Duration::from_millis(500));
    let (_, stdin_hung) = await_task_bounded(stdin_task, drain_budget).await;
    let (stdout_bytes, stdout_hung) = await_task_bounded(stdout_task, drain_budget).await;
    let (stderr_bytes, stderr_hung) = await_task_bounded(stderr_task, drain_budget).await;
    if stdin_hung || stdout_hung || stderr_hung {
        kill_process_group_by_pid(child_pid).await;
    }
    let stdout_str = String::from_utf8_lossy(&stdout_bytes).to_string();
    let stderr_str = String::from_utf8_lossy(&stderr_bytes).to_string();
    let exit_code = exit_status.code();

    let (decision, reason, updated_text) = match exit_code {
        Some(2) => {
            let parsed_reason = parse_hook_json_decision(&stdout_str).and_then(|(_, r, _)| r);
            let reason = parsed_reason
                .or_else(|| {
                    let s = stderr_str.trim();
                    (!s.is_empty()).then(|| truncate_preview(s, 300))
                })
                .or_else(|| {
                    let s = stdout_str.trim();
                    (!s.is_empty()).then(|| truncate_preview(s, 300))
                })
                .or_else(|| Some("Blocked by hook (exit 2)".to_string()));
            (HookDecision::Block, reason, None)
        }
        Some(0) => {
            if let Some((dec, r, updated)) = parse_hook_json_decision(&stdout_str) {
                (dec, r, updated)
            } else {
                (HookDecision::Allow, None, None)
            }
        }
        other => {
            let msg = match other {
                Some(code) => format!("Hook exited with code {code} (fail-open)"),
                None => "Hook terminated by signal (fail-open)".to_string(),
            };
            (HookDecision::Allow, Some(msg), None)
        }
    };

    (
        HookExecutionRecord {
            id: exec_id,
            hook_id: rule.id.clone(),
            hook_name: rule.name.clone(),
            event: rule.event,
            matched_target: target.map(ToOwned::to_owned),
            decision,
            reason,
            exit_code,
            duration_ms: started.elapsed().as_millis() as u64,
            stdout_preview: truncate_preview(&stdout_str, MAX_PREVIEW_CHARS),
            stderr_preview: truncate_preview(&stderr_str, MAX_PREVIEW_CHARS),
            executed_at_ms: now_unix_ms(),
        },
        updated_text,
    )
}

async fn execute_webhook_hook(
    rule: &LifecycleHookRule,
    url: &str,
    headers: &BTreeMap<String, String>,
    target: Option<&str>,
    payload: &Value,
) -> (HookExecutionRecord, Option<String>) {
    let started = Instant::now();
    let exec_id = format!("hx_{}", Uuid::new_v4().simple());
    let timeout_ms = rule
        .timeout_ms
        .clamp(MIN_HOOK_TIMEOUT_MS, MAX_HOOK_TIMEOUT_MS);

    let fail_open = |reason: String, stderr: String| {
        (
            HookExecutionRecord {
                id: exec_id.clone(),
                hook_id: rule.id.clone(),
                hook_name: rule.name.clone(),
                event: rule.event,
                matched_target: target.map(ToOwned::to_owned),
                decision: HookDecision::Allow,
                reason: Some(reason),
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                stdout_preview: String::new(),
                stderr_preview: truncate_preview(&stderr, MAX_PREVIEW_CHARS),
                executed_at_ms: now_unix_ms(),
            },
            None,
        )
    };

    let parsed_url = match validate_webhook_url(url) {
        Ok(u) => u,
        Err(e) => return fail_open(e.clone(), e),
    };

    // DNS-resolve upfront, validate EVERY IP against SSRF rules, pin those exact SocketAddrs
    // on reqwest, and disable redirects via `Policy::none()`.
    let (host_norm, pinned_addrs) = match resolve_and_validate_http_addrs(&parsed_url).await {
        Ok(res) => res,
        Err(e) => return fail_open(e.clone(), e),
    };

    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .redirect(reqwest::redirect::Policy::none());
    if host_norm.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(&host_norm, &pinned_addrs);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => return fail_open(format!("HTTP client build failed: {e}"), e.to_string()),
    };

    let mut header_map = HeaderMap::new();
    header_map.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    for (k, v) in headers {
        let key = k.trim();
        if key.is_empty() || is_forbidden_http_header_name(key) {
            continue;
        }
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(key.as_bytes()),
            HeaderValue::from_str(v),
        ) {
            header_map.insert(hn, hv);
        }
    }

    let mut response = match client
        .post(parsed_url)
        .headers(header_map)
        .json(payload)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => return fail_open(format!("Webhook request failed: {e}"), e.to_string()),
    };

    let status = response.status();
    let mut body_bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if body_bytes.len() < MAX_HOOK_OUTPUT_BYTES {
            let take = chunk.len().min(MAX_HOOK_OUTPUT_BYTES - body_bytes.len());
            body_bytes.extend_from_slice(&chunk[..take]);
        } else {
            break;
        }
    }
    let body_str = String::from_utf8_lossy(&body_bytes).to_string();

    if status.is_redirection() {
        let msg = format!("Webhook redirect blocked (HTTP {})", status.as_u16());
        return (
            HookExecutionRecord {
                id: exec_id,
                hook_id: rule.id.clone(),
                hook_name: rule.name.clone(),
                event: rule.event,
                matched_target: target.map(ToOwned::to_owned),
                decision: HookDecision::Allow,
                reason: Some(msg.clone()),
                exit_code: Some(i32::from(status.as_u16())),
                duration_ms: started.elapsed().as_millis() as u64,
                stdout_preview: truncate_preview(&body_str, MAX_PREVIEW_CHARS),
                stderr_preview: msg,
                executed_at_ms: now_unix_ms(),
            },
            None,
        );
    }

    if !status.is_success() {
        let msg = format!("Webhook returned HTTP {} (fail-open)", status.as_u16());
        return (
            HookExecutionRecord {
                id: exec_id,
                hook_id: rule.id.clone(),
                hook_name: rule.name.clone(),
                event: rule.event,
                matched_target: target.map(ToOwned::to_owned),
                decision: HookDecision::Allow,
                reason: Some(msg.clone()),
                exit_code: Some(i32::from(status.as_u16())),
                duration_ms: started.elapsed().as_millis() as u64,
                stdout_preview: truncate_preview(&body_str, MAX_PREVIEW_CHARS),
                stderr_preview: msg,
                executed_at_ms: now_unix_ms(),
            },
            None,
        );
    }

    let (decision, reason, updated_text) =
        parse_hook_json_decision(&body_str).unwrap_or((HookDecision::Allow, None, None));

    (
        HookExecutionRecord {
            id: exec_id,
            hook_id: rule.id.clone(),
            hook_name: rule.name.clone(),
            event: rule.event,
            matched_target: target.map(ToOwned::to_owned),
            decision,
            reason,
            exit_code: Some(i32::from(status.as_u16())),
            duration_ms: started.elapsed().as_millis() as u64,
            stdout_preview: truncate_preview(&body_str, MAX_PREVIEW_CHARS),
            stderr_preview: String::new(),
            executed_at_ms: now_unix_ms(),
        },
        updated_text,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{Router, http::StatusCode, routing::post};
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, OrchestrationConfig, ProviderKind, SessionMode,
        WorkspaceSetupStatus, WorkspaceSummary,
    };
    use orbit_store::OrbitStore;
    use tokio::sync::RwLock;

    use super::*;
    use crate::keep_awake::KeepAwakeManager;
    use crate::manager::AgentOrchestrator;
    use crate::workspace_runtime::WorkspaceScriptRegistry;

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("orbit-hooks-{prefix}-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn matcher_supports_exact_prefix_alternatives_and_wildcard_globs() {
        assert!(matches_hook_pattern(None, None));
        assert!(matches_hook_pattern(Some(""), Some("anything")));
        assert!(matches_hook_pattern(Some("*"), Some("src/main.rs")));

        // Exact & namespace prefix
        assert!(matches_hook_pattern(Some("Bash"), Some("Bash")));
        assert!(matches_hook_pattern(Some("Bash"), Some("Bash:git")));
        assert!(!matches_hook_pattern(Some("Bash"), Some("Bashful")));

        // Directory prefix
        assert!(matches_hook_pattern(Some("src/"), Some("src/hooks.rs")));
        assert!(!matches_hook_pattern(Some("src/"), Some("crates/hooks.rs")));

        // Wildcard globs & pipe alternatives
        assert!(matches_hook_pattern(
            Some("*.rs|*.ts"),
            Some("crates/a/lib.rs")
        ));
        assert!(matches_hook_pattern(Some("*.rs|*.ts"), Some("apps/ui.ts")));
        assert!(!matches_hook_pattern(Some("*.rs|*.ts"), Some("README.md")));
        assert!(matches_hook_pattern(
            Some("crates/*/src/*"),
            Some("crates/orbit-daemon/src/hooks.rs")
        ));
        assert!(!matches_hook_pattern(Some("*.rs"), None));
    }

    #[tokio::test]
    async fn crud_and_atomic_0600_persistence_with_secret_masking() {
        let home = temp_dir("crud");
        let engine = HooksEngine::new(home.clone());

        let rule = LifecycleHookRule {
            id: "hk_webhook_1".into(),
            name: "Audit Webhook".into(),
            scope: HookScope::UserGlobal,
            event: HookEventKind::TurnStop,
            matcher: Some("claude*".into()),
            action: HookAction::HttpWebhook {
                url: "https://hooks.example.com/orbit".into(),
                headers: BTreeMap::from([
                    (
                        "Authorization".into(),
                        "Bearer super-secret-token-9876".into(),
                    ),
                    ("X-Custom".into(), "plain-value".into()),
                ]),
            },
            timeout_ms: 8_000,
            enabled: true,
            trusted: false,
            description: Some("Notify on turn stop".into()),
        };

        let snap = engine
            .upsert_hook(None, rule.clone())
            .await
            .expect("upsert user hook");
        assert_eq!(snap.hooks.len(), 1);
        assert!(snap.hooks[0].trusted);

        // Secret header must be masked over the wire.
        match &snap.hooks[0].action {
            HookAction::HttpWebhook { headers, .. } => {
                let auth = headers.get("Authorization").expect("auth header");
                assert!(
                    auth.starts_with("••••"),
                    "expected masked secret, got {auth}"
                );
                assert!(!auth.contains("super-secret"));
                assert_eq!(
                    headers.get("X-Custom").map(String::as_str),
                    Some("plain-value")
                );
            }
            other => panic!("unexpected action: {other:?}"),
        }

        // Verify atomic 0o600 permissions on `$ORBIT_HOME/hooks.json`.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(home.join("hooks.json"))
                .expect("hooks.json metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "hooks.json must be mode 0600");
        }

        // Toggling `enabled` with the masked snapshot value must preserve the real secret on disk.
        let mut toggled = snap.hooks[0].clone();
        toggled.enabled = false;
        let snap2 = engine
            .upsert_hook(None, toggled)
            .await
            .expect("toggle hook");
        assert!(!snap2.hooks[0].enabled);

        let disk_raw = fs::read_to_string(home.join("hooks.json")).expect("read hooks.json");
        assert!(
            disk_raw.contains("Bearer super-secret-token-9876"),
            "updating with masked sentinel must preserve stored secret"
        );

        // Delete hook
        let snap3 = engine
            .delete_hook(None, "hk_webhook_1", HookScope::UserGlobal)
            .await
            .expect("delete hook");
        assert!(snap3.hooks.is_empty());

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn untrusted_project_hooks_refused_until_sha256_trusted_and_invalidated_on_modify() {
        let home = temp_dir("trust-home");
        let project = temp_dir("trust-proj");
        fs::create_dir_all(project.join(".orbit")).expect("create .orbit");
        let marker = project.join("ran.txt");

        let proj_hooks = PersistedHooksFile {
            version: 1,
            hooks: vec![LifecycleHookRule {
                id: "hk_proj_block".into(),
                name: "Project Blocker".into(),
                scope: HookScope::ProjectOrbit,
                event: HookEventKind::PrePromptSubmit,
                matcher: None,
                action: HookAction::Command {
                    command: "/bin/sh".into(),
                    args: vec![
                        "-c".into(),
                        format!("echo ran > '{}' && exit 2", marker.display()),
                    ],
                    env: BTreeMap::new(),
                },
                timeout_ms: 5_000,
                enabled: true,
                trusted: true, // Spoofed `trusted: true` in repo JSON MUST be ignored!
                description: None,
            }],
        };
        let hooks_path = project.join(".orbit").join("hooks.json");
        fs::write(
            &hooks_path,
            serde_json::to_string_pretty(&proj_hooks).expect("serialize"),
        )
        .expect("write project hooks");

        let engine = HooksEngine::new(home.clone());

        // 1. Listing shows untrusted even though JSON had `trusted: true`.
        let snap = engine
            .list_snapshot(Some(&project))
            .await
            .expect("list snapshot");
        assert!(!snap.project_hooks_trusted);
        assert_eq!(snap.hooks.len(), 1);
        assert!(!snap.hooks[0].trusted);

        // 2. Dispatch MUST NOT execute untrusted project hook.
        let outcome = engine
            .dispatch_event(
                HookEventKind::PrePromptSubmit,
                Some(&project),
                Some("codex"),
                json!({"prompt": "hi"}),
            )
            .await;
        assert_eq!(outcome.decision, HookDecision::Allow);
        assert!(outcome.records.is_empty());
        assert!(
            !marker.exists(),
            "untrusted project hook must never execute"
        );

        // 3. Test dry-run MUST also refuse untrusted project hook.
        let test_err = engine
            .test_hook(Some(&project), "hk_proj_block", json!({}))
            .await
            .expect_err("untrusted test must fail");
        assert!(test_err.contains("untrusted"));

        // 4. Approve SHA-256 trust -> now executes and blocks!
        let trusted_snap = engine
            .set_project_trust(&project, true)
            .await
            .expect("trust project hooks");
        assert!(trusted_snap.project_hooks_trusted);
        assert!(trusted_snap.hooks[0].trusted);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(home.join("trusted-project-hooks.json"))
                .expect("trust file metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }

        let outcome_trusted = engine
            .dispatch_event(
                HookEventKind::PrePromptSubmit,
                Some(&project),
                Some("codex"),
                json!({"prompt": "hi"}),
            )
            .await;
        assert_eq!(outcome_trusted.decision, HookDecision::Block);
        assert!(marker.exists(), "trusted project hook should execute");
        let _ = fs::remove_file(&marker);

        // 5. Modify `.orbit/hooks.json` externally -> SHA-256 mismatch MUST invalidate trust!
        let mut raw = fs::read_to_string(&hooks_path).expect("read");
        raw.push('\n');
        fs::write(&hooks_path, raw).expect("tamper hooks.json");

        let snap_after_tamper = engine
            .list_snapshot(Some(&project))
            .await
            .expect("list after tamper");
        assert!(!snap_after_tamper.project_hooks_trusted);
        assert!(!snap_after_tamper.hooks[0].trusted);

        let outcome_after_tamper = engine
            .dispatch_event(
                HookEventKind::PrePromptSubmit,
                Some(&project),
                Some("codex"),
                json!({"prompt": "hi"}),
            )
            .await;
        assert_eq!(outcome_after_tamper.decision, HookDecision::Allow);
        assert!(
            !marker.exists(),
            "modified project hooks file must not execute before re-trust"
        );

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn command_allow_block_exit_2_and_modify_json_decisions() {
        let home = temp_dir("decisions");
        let engine = HooksEngine::new(home.clone());

        // 1. Modify hook (exit 0 + JSON)
        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_mod".into(),
                    name: "Append footer".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::PrePromptSubmit,
                    matcher: Some("claude*".into()),
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            r#"printf '{"decision":"modify","reason":"added guard","updated_text":"MODIFIED PROMPT"}'"#.into(),
                        ],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert modify hook");

        let mod_out = engine
            .dispatch_event(
                HookEventKind::PrePromptSubmit,
                None,
                Some("claude_code"),
                json!({"prompt": "orig"}),
            )
            .await;
        assert_eq!(mod_out.decision, HookDecision::Modify);
        assert_eq!(mod_out.updated_text.as_deref(), Some("MODIFIED PROMPT"));
        assert_eq!(mod_out.reason.as_deref(), Some("added guard"));

        // 2. Exit-2 block hook
        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_block_exit2".into(),
                    name: "Exit 2 Blocker".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::PrePromptSubmit,
                    matcher: Some("codex".into()),
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec!["-c".into(), "echo 'secret detected' >&2; exit 2".into()],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert exit-2 hook");

        let blk_out = engine
            .dispatch_event(
                HookEventKind::PrePromptSubmit,
                None,
                Some("codex"),
                json!({"prompt": "AKIA..."}),
            )
            .await;
        assert_eq!(blk_out.decision, HookDecision::Block);
        assert_eq!(blk_out.records[0].exit_code, Some(2));
        assert!(
            blk_out
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("secret detected"))
        );

        // 3. Allow hook (exit 0 plain output)
        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_allow".into(),
                    name: "Allow Logger".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::TurnStop,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec!["-c".into(), "cat >/dev/null && echo ok".into()],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert allow hook");

        let allow_out = engine
            .dispatch_event(HookEventKind::TurnStop, None, Some("codex"), json!({}))
            .await;
        assert_eq!(allow_out.decision, HookDecision::Allow);
        assert_eq!(allow_out.records[0].exit_code, Some(0));
        assert_eq!(allow_out.records[0].stdout_preview, "ok");

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn command_timeout_kills_child_process_group() {
        let home = temp_dir("timeout");
        let engine = HooksEngine::new(home.clone());
        let pid_file = home.join("grandchild.pid");

        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_hang".into(),
                    name: "Hanging Hook".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::SessionStart,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            format!("sleep 30 & echo $! > '{}'; wait", pid_file.display()),
                        ],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 350,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert timeout hook");

        let started = Instant::now();
        let rec = engine
            .test_hook(None, "hk_hang", json!({}))
            .await
            .expect("test timeout hook");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(4),
            "timeout should abort promptly, took {elapsed:?}"
        );
        assert_eq!(rec.decision, HookDecision::Allow);
        assert_eq!(rec.exit_code, None);
        assert!(
            rec.reason
                .as_deref()
                .is_some_and(|r| r.contains("timed out"))
        );

        // Verify grandchild process in the isolated process group was killed too.
        #[cfg(unix)]
        if let Ok(pid_raw) = fs::read_to_string(&pid_file)
            && let Ok(gpid) = pid_raw.trim().parse::<u32>()
        {
            tokio::time::sleep(Duration::from_millis(120)).await;
            let status = Command::new("/bin/kill")
                .args(["-0", &gpid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .expect("run kill -0");
            assert!(
                !status.success(),
                "grandchild PID {gpid} should have been terminated with process group"
            );
        }

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn dangerous_env_vars_are_rejected_and_scrubbed() {
        let home = temp_dir("env-scrub");
        let engine = HooksEngine::new(home.clone());

        for bad_key in [
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "ORBIT_MCP_TOKEN",
            "BASH_ENV",
            "BASH_FUNC_evil%%",
            "SHELLOPTS",
            "PS4",
            "OPENSSL_CONF",
            "NODE_OPTIONS",
            "PYTHONSTARTUP",
            "GIT_SSH_COMMAND",
        ] {
            assert!(is_dangerous_env_key(bad_key), "{bad_key} must be flagged");
            let err = engine
                .upsert_hook(
                    None,
                    LifecycleHookRule {
                        id: format!("hk_bad_{bad_key}"),
                        name: "Bad Env".into(),
                        scope: HookScope::UserGlobal,
                        event: HookEventKind::Notification,
                        matcher: None,
                        action: HookAction::Command {
                            command: "/bin/echo".into(),
                            args: vec![],
                            env: BTreeMap::from([(bad_key.to_string(), "evil".into())]),
                        },
                        timeout_ms: 5_000,
                        enabled: true,
                        trusted: true,
                        description: None,
                    },
                )
                .await
                .expect_err("dangerous env key must be rejected");
            assert!(err.contains("blocked"));
        }

        // Verify safe env passes through while dangerous keys are scrubbed.
        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_env_check".into(),
                    name: "Check Env".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::Notification,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            "printf 'SAFE=%s|ORBIT=%s|LD=%s|NODE=%s' \"$SAFE_VAR\" \"$ORBIT_MCP_TOKEN\" \"$LD_PRELOAD\" \"$NODE_OPTIONS\"".into(),
                        ],
                        env: BTreeMap::from([("SAFE_VAR".into(), "ok123".into())]),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert safe env hook");

        let rec = engine
            .test_hook(None, "hk_env_check", json!({}))
            .await
            .expect("run env check");
        assert_eq!(rec.stdout_preview, "SAFE=ok123|ORBIT=|LD=|NODE=");

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn webhook_ssrf_blocking_redirect_refusal_and_live_block_decision() {
        // 1. Static SSRF URL validation blocks cloud metadata, link-local, ::ffff:, NAT64.
        for blocked in [
            "http://169.254.169.254/latest/meta-data",
            "http://metadata.google.internal/computeMetadata/v1/",
            "http://[::ffff:169.254.169.254]/",
            "http://[64:ff9b::169.254.169.254]/",
            "http://[fd00:ec2::254]/latest/api/token",
            "http://100.100.100.200/latest/meta-data",
            "http://0.0.0.0:8080/hook",
            "file:///etc/passwd",
        ] {
            assert!(
                validate_webhook_url(blocked).is_err(),
                "expected SSRF URL `{blocked}` to be blocked"
            );
        }

        // 2. Local test server verifying real HTTP webhook JSON decision & `Policy::none()` redirect refusal.
        let app = Router::new()
            .route(
                "/block",
                post(|| async {
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "decision": "block",
                            "reason": "blocked by webhook policy"
                        })),
                    )
                }),
            )
            .route(
                "/redirect",
                post(|| async {
                    (
                        StatusCode::FOUND,
                        [(axum::http::header::LOCATION, "http://169.254.169.254/")],
                        "",
                    )
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind webhook listener");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let home = temp_dir("webhook");
        let engine = HooksEngine::new(home.clone());

        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_web_block".into(),
                    name: "Policy Webhook".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::PrePromptSubmit,
                    matcher: None,
                    action: HookAction::HttpWebhook {
                        url: format!("http://{addr}/block"),
                        headers: BTreeMap::from([("X-Orbit-Test".into(), "1".into())]),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert webhook");

        let rec = engine
            .test_hook(None, "hk_web_block", json!({"prompt": "rm -rf /"}))
            .await
            .expect("test webhook");
        assert_eq!(rec.decision, HookDecision::Block);
        assert_eq!(rec.reason.as_deref(), Some("blocked by webhook policy"));

        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_web_redir".into(),
                    name: "Redirect Webhook".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::Notification,
                    matcher: None,
                    action: HookAction::HttpWebhook {
                        url: format!("http://{addr}/redirect"),
                        headers: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert redirect webhook");

        let redir_rec = engine
            .test_hook(None, "hk_web_redir", json!({}))
            .await
            .expect("test redirect webhook");
        assert_eq!(redir_rec.decision, HookDecision::Allow);
        assert!(
            redir_rec
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("redirect blocked")),
            "expected redirect blocked, got {:?}",
            redir_rec.reason
        );

        server.abort();
        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn pre_prompt_submit_hook_blocks_agent_prompt_integration() {
        let home = temp_dir("pre-prompt-int");
        let workdir = temp_dir("pre-prompt-cwd");
        let store = Arc::new(OrbitStore::open(home.join("orbit.db")).expect("open store"));
        let scripts = WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let hooks = HooksEngine::new(home.clone());

        hooks
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_block_prod".into(),
                    name: "Guard Production".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::PrePromptSubmit,
                    matcher: Some("codex".into()),
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            "echo 'Production deploy blocked by policy' >&2; exit 2".into(),
                        ],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert blocking pre-prompt hook");

        let orch = AgentOrchestrator::new(
            home.clone(),
            "host_test".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            store.clone(),
            scripts,
            keep_awake,
            Arc::new(RwLock::new(None)),
            hooks.clone(),
        )
        .await;

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_hook_test".into(),
                title: "Hook Test Workspace".into(),
                cwd: workdir.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("insert workspace");
        let summary = AgentSummary {
            agent_id: "ag_hook_test".into(),
            workspace_id: "ws_hook_test".into(),
            cwd: workdir.to_string_lossy().to_string(),
            title: "Hook Test".into(),
            provider: ProviderKind::Codex,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: Default::default(),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("codex".into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store.upsert_agent(&summary, None).expect("insert agent");

        let err = orch
            .send_prompt("ag_hook_test", "deploy to prod".into())
            .await
            .expect_err("PrePromptSubmit hook must block send_prompt");
        let msg = err.to_string();
        assert!(
            msg.contains("Guard Production") && msg.contains("Production deploy blocked"),
            "unexpected error message: {msg}"
        );

        let snap = hooks.list_snapshot(None).await.expect("list snapshot");
        assert_eq!(snap.recent_executions.len(), 1);
        assert_eq!(snap.recent_executions[0].decision, HookDecision::Block);

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&workdir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn project_hooks_reject_symlinks_unconditionally_and_cap_file_size() {
        use std::os::unix::fs::symlink;

        let home = temp_dir("symlink-home");
        let project = temp_dir("symlink-proj");
        fs::create_dir_all(project.join(".orbit")).expect("create .orbit");

        // Even an in-workspace symlink for `.orbit/hooks.json` MUST be rejected unconditionally.
        let real_file = project.join("real-hooks.json");
        fs::write(&real_file, r#"{"version":1,"hooks":[]}"#).expect("write real hooks");
        symlink(&real_file, project.join(".orbit").join("hooks.json")).expect("symlink hooks.json");

        assert!(
            resolve_project_hooks_path(&project).is_err(),
            "symlinked .orbit/hooks.json must be rejected unconditionally"
        );
        let engine = HooksEngine::new(home.clone());
        assert!(
            engine.set_project_trust(&project, true).await.is_err(),
            "trusting symlinked .orbit/hooks.json must fail"
        );

        // Symlinked `.orbit` directory MUST also be rejected unconditionally.
        let project2 = temp_dir("symlink-dir-proj");
        let real_orbit_dir = project2.join("real_orbit");
        fs::create_dir_all(&real_orbit_dir).expect("mkdir real_orbit");
        fs::write(
            real_orbit_dir.join("hooks.json"),
            r#"{"version":1,"hooks":[]}"#,
        )
        .expect("write hooks");
        symlink(&real_orbit_dir, project2.join(".orbit")).expect("symlink .orbit");
        assert!(
            resolve_project_hooks_path(&project2).is_err(),
            "symlinked .orbit directory must be rejected unconditionally"
        );

        // Oversized (> 4 MiB) `.orbit/hooks.json` must be rejected by bounded reader.
        let project3 = temp_dir("oversized-proj");
        fs::create_dir_all(project3.join(".orbit")).expect("create .orbit");
        let huge = vec![b' '; MAX_HOOKS_FILE_BYTES + 16];
        fs::write(project3.join(".orbit").join("hooks.json"), huge).expect("write oversized");
        let err = engine
            .set_project_trust(&project3, true)
            .await
            .expect_err("oversized hooks.json must fail");
        assert!(err.contains("exceeds maximum size limit"));

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&project);
        let _ = fs::remove_dir_all(&project2);
        let _ = fs::remove_dir_all(&project3);
    }

    #[test]
    fn webhook_rejects_trailing_dot_hostnames_and_normalizes_case() {
        for trailing in [
            "http://example.com./hook",
            "https://hooks.example.com./notify",
            "http://metadata.google.internal./computeMetadata/v1/",
            "http://localhost./hook",
        ] {
            let err = validate_webhook_url(trailing)
                .expect_err("trailing-dot webhook hostname must be rejected");
            assert!(
                err.contains("trailing dot"),
                "unexpected error for `{trailing}`: {err}"
            );
        }

        let normalized =
            validate_webhook_url("https://Hooks.Example.COM/orbit").expect("valid mixed-case URL");
        assert_eq!(normalized.host_str(), Some("hooks.example.com"));
    }

    #[tokio::test]
    async fn detached_grandchild_holding_stdout_does_not_hang_past_timeout() {
        let home = temp_dir("detached-grandchild");
        let engine = HooksEngine::new(home.clone());

        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_detach".into(),
                    name: "Detached Grandchild".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::Notification,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            "(sleep 30) & printf 'quick-out'; exit 0".into(),
                        ],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 1_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert detached hook");

        let started = Instant::now();
        let rec = engine
            .test_hook(None, "hk_detach", json!({}))
            .await
            .expect("run detached hook");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "hook with detached grandchild holding stdout must not hang; took {elapsed:?}"
        );
        assert_eq!(rec.exit_code, Some(0));

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn secret_preservation_case_insensitive_and_rejects_unmatched_placeholders_and_non_ascii()
    {
        let home = temp_dir("secret-hardening");
        let engine = HooksEngine::new(home.clone());

        // 1. Seed a webhook hook with an Authorization secret.
        let seeded = engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_sec_1".into(),
                    name: "Secret Webhook".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::TurnStop,
                    matcher: None,
                    action: HookAction::HttpWebhook {
                        url: "https://hooks.example.com/orbit".into(),
                        headers: BTreeMap::from([(
                            "Authorization".into(),
                            "Bearer top-secret-4321".into(),
                        )]),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("seed webhook");

        let masked = match &seeded.hooks[0].action {
            HookAction::HttpWebhook { headers, .. } => {
                headers.get("Authorization").expect("masked auth").clone()
            }
            other => panic!("unexpected action: {other:?}"),
        };
        assert!(masked.starts_with("••••"));

        // 2. Update using different header casing (`authorization`) + masked placeholder -> must preserve!
        let mut updated = seeded.hooks[0].clone();
        updated.action = HookAction::HttpWebhook {
            url: "https://hooks.example.com/orbit".into(),
            headers: BTreeMap::from([("authorization".into(), masked.clone())]),
        };
        engine
            .upsert_hook(None, updated)
            .await
            .expect("case-insensitive secret preservation must succeed");

        let disk_raw = fs::read_to_string(home.join("hooks.json")).expect("read hooks.json");
        assert!(
            disk_raw.contains("Bearer top-secret-4321"),
            "stored secret must be preserved across case-insensitive header update"
        );

        // 3. Submitting an unmatched `••••` placeholder on a new hook MUST be rejected.
        let err_placeholder = engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_sec_new".into(),
                    name: "Unmatched Placeholder".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::TurnStop,
                    matcher: None,
                    action: HookAction::HttpWebhook {
                        url: "https://hooks.example.com/orbit".into(),
                        headers: BTreeMap::from([("Authorization".into(), "••••9999".into())]),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect_err("unmatched placeholder must be rejected");
        assert!(err_placeholder.contains("unresolved masked placeholder"));

        // 4. Submitting a non-ASCII HTTP header value MUST be rejected.
        let err_ascii = engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_non_ascii".into(),
                    name: "Non ASCII Header".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::TurnStop,
                    matcher: None,
                    action: HookAction::HttpWebhook {
                        url: "https://hooks.example.com/orbit".into(),
                        headers: BTreeMap::from([("X-Tag".into(), "héllo".into())]),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect_err("non-ASCII header must be rejected");
        assert!(err_ascii.contains("ASCII"));

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn test_hook_scope_prefix_prevents_user_global_shadowing_project_hook() {
        let home = temp_dir("scope-shadow-home");
        let project = temp_dir("scope-shadow-proj");
        let engine = HooksEngine::new(home.clone());

        engine
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "same_id".into(),
                    name: "Global Hook".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::Notification,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec!["-c".into(), "printf global-ran".into()],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert global hook");

        engine
            .upsert_hook(
                Some(&project),
                LifecycleHookRule {
                    id: "same_id".into(),
                    name: "Project Hook".into(),
                    scope: HookScope::ProjectOrbit,
                    event: HookEventKind::Notification,
                    matcher: None,
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec!["-c".into(), "printf project-ran".into()],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: false,
                    description: None,
                },
            )
            .await
            .expect("upsert project hook");

        engine
            .set_project_trust(&project, true)
            .await
            .expect("trust project");

        let proj_rec = engine
            .test_hook(Some(&project), "project_orbit:same_id", json!({}))
            .await
            .expect("test scoped project hook");
        assert_eq!(proj_rec.stdout_preview, "project-ran");

        let user_rec = engine
            .test_hook(Some(&project), "user_global:same_id", json!({}))
            .await
            .expect("test scoped user hook");
        assert_eq!(user_rec.stdout_preview, "global-ran");

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&project);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn relative_command_path_resolves_against_cwd_and_requires_exec_bit() {
        use std::os::unix::fs::PermissionsExt;

        let home = temp_dir("rel-cmd-home");
        let project = temp_dir("rel-cmd-proj");
        fs::create_dir_all(project.join(".orbit")).expect("mkdir .orbit");
        let script_path = project.join(".orbit").join("check.sh");
        fs::write(&script_path, "#!/bin/sh\nprintf 'relative-ok'\n").expect("write script");
        // Not executable yet (0644)
        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o644)).expect("chmod 644");

        let engine = HooksEngine::new(home.clone());
        engine
            .upsert_hook(
                Some(&project),
                LifecycleHookRule {
                    id: "hk_rel".into(),
                    name: "Relative Script".into(),
                    scope: HookScope::ProjectOrbit,
                    event: HookEventKind::PostFileEdit,
                    matcher: None,
                    action: HookAction::Command {
                        command: "./.orbit/check.sh".into(),
                        args: vec![],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: false,
                    description: None,
                },
            )
            .await
            .expect("upsert relative hook");
        engine
            .set_project_trust(&project, true)
            .await
            .expect("trust project");

        let non_exec = engine
            .test_hook(Some(&project), "project_orbit:hk_rel", json!({}))
            .await
            .expect("run non-exec hook");
        assert!(
            non_exec
                .reason
                .as_deref()
                .is_some_and(|r| r.contains("not executable"))
        );

        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755)).expect("chmod 755");
        let ok_rec = engine
            .test_hook(Some(&project), "project_orbit:hk_rel", json!({}))
            .await
            .expect("run exec hook");
        assert_eq!(ok_rec.exit_code, Some(0));
        assert_eq!(ok_rec.stdout_preview, "relative-ok");

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&project);
    }

    #[tokio::test]
    async fn pre_prompt_submit_block_during_create_agent_leaves_zero_ghost_rows_or_keep_awake() {
        let home = temp_dir("create-block-home");
        let workdir = temp_dir("create-block-cwd");
        fs::create_dir_all(home.join("shims")).expect("mkdir shims");

        orbit_providers::upsert_custom_acp_agent(
            &home,
            orbit_protocol::AcpAgentSpec {
                id: "mock-acp-hook-test".into(),
                name: "Mock ACP".into(),
                description: "Test mock ACP".into(),
                command: "/bin/sh".into(),
                args: vec!["-c".into(), "cat >/dev/null".into()],
                env: Default::default(),
                install_hint: None,
                homepage: None,
                version: None,
                source: orbit_protocol::AcpAgentSource::Custom,
                enabled: true,
            },
        )
        .expect("register mock ACP agent");

        let store = Arc::new(OrbitStore::open(home.join("orbit.db")).expect("open store"));
        let scripts = WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(KeepAwakeManager::new(true));
        let hooks = HooksEngine::new(home.clone());

        hooks
            .upsert_hook(
                None,
                LifecycleHookRule {
                    id: "hk_block_create".into(),
                    name: "Block Initial Prompt".into(),
                    scope: HookScope::UserGlobal,
                    event: HookEventKind::PrePromptSubmit,
                    matcher: Some("acp".into()),
                    action: HookAction::Command {
                        command: "/bin/sh".into(),
                        args: vec![
                            "-c".into(),
                            "echo 'No initial prod prompt' >&2; exit 2".into(),
                        ],
                        env: BTreeMap::new(),
                    },
                    timeout_ms: 5_000,
                    enabled: true,
                    trusted: true,
                    description: None,
                },
            )
            .await
            .expect("upsert blocking hook");

        let orch = AgentOrchestrator::new(
            home.clone(),
            "host_test".into(),
            "tok".into(),
            Arc::new(RwLock::new(0)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            store.clone(),
            scripts,
            keep_awake.clone(),
            Arc::new(RwLock::new(None)),
            hooks.clone(),
        )
        .await;

        let err = orch
            .create_and_start_agent(crate::manager::CreateAgentSpec {
                cwd: workdir.to_string_lossy().to_string(),
                extra_dirs: Vec::new(),
                title: Some("Should Not Exist".into()),
                provider: ProviderKind::Acp,
                model: None,
                effort: None,
                fast_mode: None,
                mode: SessionMode::Default,
                initial_prompt: "deploy to prod immediately".into(),
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree: false,
                workspace_id: None,
                parent_agent_id: None,
                parent_host_id: None,
                profile_id: None,
                background: false,
                auto_archive: false,
                base_ref: None,
                worktree_mode: orbit_protocol::WorktreeCreateMode::BranchOff,
                branch: None,
                pr_number: None,
                handle: None,
                auto_cluster: Some(false),
                acp_agent_id: Some("mock-acp-hook-test".into()),
            })
            .await
            .expect_err("PrePromptSubmit block must reject create_and_start_agent");
        assert!(err.to_string().contains("Block Initial Prompt"));

        // Zero ghost agent rows (neither active nor archived)!
        let all_agents = store
            .list_agents_filtered(None, true, true, None)
            .expect("list agents");
        assert!(
            all_agents.is_empty(),
            "blocked create_and_start_agent must not persist any agent row, found: {all_agents:?}"
        );
        // Keep-Awake must NOT be held busy!
        let ka_status = keep_awake.status();
        assert_eq!(
            ka_status.active_agents, 0,
            "blocked create_and_start_agent must not leak keep-awake assertion"
        );
        assert!(
            !ka_status.active,
            "blocked create_and_start_agent must leave keep-awake inactive"
        );

        let _ = fs::remove_dir_all(&home);
        let _ = fs::remove_dir_all(&workdir);
    }
}
