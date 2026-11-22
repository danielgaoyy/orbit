//! First-class native Rust drivers for 13 coding agent CLIs (Antigravity, Codex, Claude Code,
//! OpenCode, Gemini CLI, Cursor Agent, GitHub Copilot, Goose, Pi, Kimi CLI, Cline, Mistral Vibe, Augment).

pub mod acp_providers;
pub mod antigravity;
pub mod attachments;
pub mod claude;
pub mod codex;
pub mod driver;
pub mod pi;
pub mod shell_snapshot;
pub mod side_question;

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use orbit_protocol::{ModelOption, ProviderKind, ProviderModelCatalog, ProviderQuotaSnapshot};
use serde::{Deserialize, Serialize};
use similar::TextDiff;

const MAX_COMPACT_UNIFIED_DIFF_BYTES: usize = 64 * 1024;

/// Build a standard compact unified diff (3 context lines per hunk) from `old_text` and `new_text`.
/// Returns `None` when both texts are identical, and safely caps extreme payloads at 64 KiB on a
/// UTF-8 line boundary so large file rewrites never bloat SQLite or WebSocket frames.
pub(crate) fn build_compact_unified_diff(
    path: &str,
    old_text: &str,
    new_text: &str,
) -> Option<String> {
    if old_text == new_text {
        return None;
    }

    let clean_path = path.trim().trim_start_matches("./");
    let display_path = if clean_path.is_empty() {
        "file"
    } else {
        clean_path
    };
    let old_header = if old_text.is_empty() {
        "/dev/null".to_string()
    } else {
        format!("a/{display_path}")
    };
    let new_header = if new_text.is_empty() {
        "/dev/null".to_string()
    } else {
        format!("b/{display_path}")
    };

    let diff = TextDiff::from_lines(old_text, new_text);
    let mut rendered = diff
        .unified_diff()
        .context_radius(3)
        .header(&old_header, &new_header)
        .to_string();

    if rendered.trim().is_empty() {
        return None;
    }

    if rendered.len() > MAX_COMPACT_UNIFIED_DIFF_BYTES {
        let mut cut = MAX_COMPACT_UNIFIED_DIFF_BYTES;
        while cut > 0 && !rendered.is_char_boundary(cut) {
            cut -= 1;
        }
        if let Some(nl) = rendered[..cut].rfind('\n') {
            cut = nl + 1;
        }
        rendered.truncate(cut);
        rendered.push_str("\\ Diff truncated for storage efficiency\n");
    }

    Some(rendered)
}

pub use driver::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, PersistedBackgroundTask, ProviderSessionHandle,
    SessionLaunchConfig,
};
pub use shell_snapshot::{
    ShellSnapshotCache, ShellSnapshotStats, apply_cached_shell_env, apply_cached_std_shell_env,
    invalidate_shell_snapshot, merge_cached_shell_env_into_map, resolve_login_shell_env,
    resolve_login_shell_env_cached, set_snapshot_orbit_home, shell_snapshot_stats,
    warm_login_shell_env_in_background,
};
pub use side_question::{SideQuestionSpec, run_ephemeral_text_prompt, run_side_question};

static PROVIDER_EXECUTABLE_OVERRIDES: OnceLock<RwLock<HashMap<ProviderKind, PathBuf>>> =
    OnceLock::new();

/// Register a safe in-process provider executable override (primarily for deterministic E2E tests).
pub fn set_provider_executable_override(kind: ProviderKind, path: PathBuf) {
    let lock = PROVIDER_EXECUTABLE_OVERRIDES.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = lock.write() {
        guard.insert(kind, path);
    }
}

/// Returns `true` when ANY provider executable override is registered in-process (e.g. mock E2E tests).
pub fn has_any_provider_override() -> bool {
    if let Some(lock) = PROVIDER_EXECUTABLE_OVERRIDES.get()
        && let Ok(guard) = lock.read()
        && !guard.is_empty()
    {
        return true;
    }
    false
}

/// Returns `true` when `kind` is pointed at a test override binary (via in-process override or env var).
pub fn is_provider_overridden(kind: ProviderKind) -> bool {
    if let Some(lock) = PROVIDER_EXECUTABLE_OVERRIDES.get()
        && let Ok(guard) = lock.read()
        && guard.contains_key(&kind)
    {
        return true;
    }
    env::var(kind.env_override_key())
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredProvider {
    pub kind: ProviderKind,
    pub executable_path: PathBuf,
    pub protocol_transport: &'static str,
}

/// Probe `PATH` and standard user bin locations (`~/.local/bin`, `~/.cargo/bin`, `/opt/homebrew/bin`)
/// across Linux workstations, cloud Devboxes, and macOS (Mac mini) for all 13 native providers.
pub fn discover_installed_providers() -> Vec<DiscoveredProvider> {
    let search_dirs = default_executable_search_dirs();
    let mut found = Vec::new();
    for &kind in ProviderKind::all() {
        if let Some(path) = find_provider_executable_in(kind, &search_dirs) {
            found.push(DiscoveredProvider {
                kind,
                executable_path: path,
                protocol_transport: kind.protocol_transport(),
            });
        }
    }
    found
}

fn find_provider_executable_in(kind: ProviderKind, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(lock) = PROVIDER_EXECUTABLE_OVERRIDES.get()
        && let Ok(guard) = lock.read()
        && let Some(p) = guard.get(&kind)
        && is_executable_file(p)
    {
        return Some(p.clone());
    }
    let env_override = env::var(kind.env_override_key()).ok();
    resolve_provider_executable_in(kind, env_override.as_deref(), search_dirs)
}

pub fn find_provider_executable(kind: ProviderKind) -> Option<PathBuf> {
    if let Some(lock) = PROVIDER_EXECUTABLE_OVERRIDES.get()
        && let Ok(guard) = lock.read()
        && let Some(p) = guard.get(&kind)
        && is_executable_file(p)
    {
        return Some(p.clone());
    }
    let env_override = env::var(kind.env_override_key()).ok();
    let search_dirs = default_executable_search_dirs();
    resolve_provider_executable_in(kind, env_override.as_deref(), &search_dirs)
}

pub(crate) fn resolve_provider_executable_in(
    kind: ProviderKind,
    env_override: Option<&str>,
    search_dirs: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(override_path) = env_override {
        let trimmed = override_path.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if is_executable_file(&p) {
                return Some(p);
            }
        }
    }
    if let Some(primary) = find_executable_in_dirs(kind.default_binary(), search_dirs) {
        return Some(primary);
    }
    for &fallback in kind.fallback_binaries() {
        if let Some(alt) = find_executable_in_dirs(fallback, search_dirs) {
            return Some(alt);
        }
    }
    None
}

fn default_executable_search_dirs() -> Vec<PathBuf> {
    let mut search_dirs: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut push_unique = |dir: PathBuf| {
        if !dir.as_os_str().is_empty() && seen.insert(dir.clone()) {
            search_dirs.push(dir);
        }
    };

    // Search cached login-shell PATH directories ahead of inherited PATH (consistent with `merge_paths`).
    for dir in shell_snapshot::cached_login_shell_path_dirs() {
        push_unique(dir);
    }

    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            push_unique(dir);
        }
    }

    if let Some(home) = env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
        .map(PathBuf::from)
    {
        for extra in [
            home.join(".local/bin"),
            home.join(".npm-global/bin"),
            home.join(".cargo/bin"),
        ] {
            push_unique(extra);
        }
    }
    for extra in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        push_unique(extra);
    }

    search_dirs
}

fn find_executable_in_dirs(binary_name: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    for dir in search_dirs {
        let candidate = dir.join(binary_name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

pub fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let search_dirs = default_executable_search_dirs();
    find_executable_in_dirs(binary_name, &search_dirs)
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Unified session launcher dispatching to the appropriate native Rust protocol driver.
pub async fn spawn_provider_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    match cfg.provider {
        ProviderKind::Antigravity => antigravity::spawn_antigravity_session(cfg).await,
        ProviderKind::Codex => codex::spawn_codex_session(cfg).await,
        ProviderKind::ClaudeCode => claude::spawn_claude_session(cfg).await,
        ProviderKind::Pi => pi::spawn_pi_session(cfg).await,
        ProviderKind::OpenCode
        | ProviderKind::GeminiCli
        | ProviderKind::CursorAgent
        | ProviderKind::Copilot
        | ProviderKind::Goose
        | ProviderKind::Kimi
        | ProviderKind::Cline
        | ProviderKind::MistralVibe
        | ProviderKind::Augment => acp_providers::spawn_acp_provider_session(cfg).await,
    }
}

/// Concurrently fetch real-time quota/usage windows across installed providers that expose usage APIs.
pub async fn probe_all_provider_quotas() -> Vec<ProviderQuotaSnapshot> {
    let installed = tokio::task::spawn_blocking(discover_installed_providers)
        .await
        .unwrap_or_default();
    let mut agy_path = None;
    let mut has_codex = false;
    let mut has_claude = false;

    for p in installed {
        match p.kind {
            ProviderKind::Antigravity => agy_path = Some(p.executable_path),
            ProviderKind::Codex => has_codex = true,
            ProviderKind::ClaudeCode => has_claude = true,
            _ => {}
        }
    }

    let agy_fut = async move {
        if let Some(p) = agy_path {
            Some(antigravity::probe_antigravity_quota(&p).await)
        } else {
            None
        }
    };
    let codex_fut = async move {
        if has_codex {
            Some(codex::probe_codex_quota().await)
        } else {
            None
        }
    };
    let claude_fut = async move {
        if has_claude {
            Some(claude::probe_claude_quota().await)
        } else {
            None
        }
    };

    let (a, b, c) = tokio::join!(agy_fut, codex_fut, claude_fut);
    [a, b, c].into_iter().flatten().collect()
}

/// Instant non-subprocess model catalogs across all 13 native providers.
pub async fn fallback_all_provider_models() -> Vec<ProviderModelCatalog> {
    let codex = codex::fallback_codex_models().await;
    let claude = claude::fallback_claude_models();
    let agy = antigravity::fallback_antigravity_models();
    let mut out = vec![claude, codex, agy];
    for &kind in ProviderKind::all() {
        match kind {
            ProviderKind::ClaudeCode | ProviderKind::Codex | ProviderKind::Antigravity => {}
            ProviderKind::Pi => out.push(pi::fallback_pi_models()),
            other => out.push(acp_providers::fallback_acp_provider_models(other)),
        }
    }
    out
}

/// Concurrently probe live available models & per-model thinking options across installed providers
/// (fast live probes for Claude Code, Codex, and Antigravity + instant catalogs for installed ACP/Pi providers).
pub async fn probe_all_provider_models() -> Vec<ProviderModelCatalog> {
    let installed = tokio::task::spawn_blocking(discover_installed_providers)
        .await
        .unwrap_or_default();
    let mut agy_path = None;
    let mut codex_path = None;
    let mut claude_path = None;
    let mut extra_installed: Vec<ProviderKind> = Vec::new();

    for p in installed {
        match p.kind {
            ProviderKind::Antigravity => agy_path = Some(p.executable_path),
            ProviderKind::Codex => codex_path = Some(p.executable_path),
            ProviderKind::ClaudeCode => claude_path = Some(p.executable_path),
            other => extra_installed.push(other),
        }
    }

    let (claude_cat, codex_cat, agy_cat) = tokio::join!(
        claude::probe_claude_models(claude_path.as_deref()),
        codex::probe_codex_models(codex_path.as_deref()),
        antigravity::probe_antigravity_models(agy_path.as_deref()),
    );

    let mut catalogs = vec![claude_cat, codex_cat, agy_cat];
    for kind in extra_installed {
        match kind {
            ProviderKind::Pi => catalogs.push(pi::fallback_pi_models()),
            other => catalogs.push(acp_providers::fallback_acp_provider_models(other)),
        }
    }
    catalogs
}

/// Alias for `probe_all_provider_models` returning model catalogs for installed providers.
pub async fn discover_all_provider_models() -> Vec<ProviderModelCatalog> {
    probe_all_provider_models().await
}

/// Project structured `ProviderModelCatalog` slices into legacy `ModelOption` maps for mobile FFI.
pub fn catalogs_to_model_map(
    catalogs: &[ProviderModelCatalog],
) -> HashMap<ProviderKind, Vec<ModelOption>> {
    let mut map = HashMap::new();
    for cat in catalogs {
        let items = cat
            .models
            .iter()
            .map(|m| ModelOption {
                id: m.id.clone(),
                label: m.label.clone(),
                description: if m.description.is_empty() {
                    None
                } else {
                    Some(m.description.clone())
                },
                is_default: m.is_default,
            })
            .collect();
        map.insert(cat.provider, items);
    }
    map
}

#[cfg(test)]
mod discovery_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn provider_override_and_vibe_fallback_discovery_cover_all_13_providers() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = std::env::temp_dir().join(format!(
            "orbit_prov_disc_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");

        for &kind in ProviderKind::all() {
            let fake_bin = tmp.join(format!("fake_{}", kind.slug()));
            std::fs::write(&fake_bin, "#!/bin/sh\nexit 0\n").expect("write bin");
            std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755))
                .expect("chmod");
            set_provider_executable_override(kind, fake_bin.clone());
            assert!(is_provider_overridden(kind));
            assert_eq!(
                find_provider_executable(kind).as_deref(),
                Some(fake_bin.as_path())
            );
        }

        let discovered = discover_installed_providers();
        assert_eq!(discovered.len(), 13);
        for &kind in ProviderKind::all() {
            let found = discovered
                .iter()
                .find(|d| d.kind == kind)
                .expect("provider discovered");
            assert_eq!(found.protocol_transport, kind.protocol_transport());
        }

        // Clear overrides after test so sibling tests remain clean
        if let Some(lock) = PROVIDER_EXECUTABLE_OVERRIDES.get()
            && let Ok(mut guard) = lock.write()
        {
            guard.clear();
        }
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn path_discovery_env_override_and_mistral_vibe_fallback_work_for_all_13_providers() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = std::env::temp_dir().join(format!(
            "orbit_prov_path_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let bin_dir = tmp.join("bin");
        let override_dir = tmp.join("override");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        std::fs::create_dir_all(&override_dir).expect("mkdir override");

        let make_exec = |path: &Path| {
            std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("write exec");
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
                .expect("chmod exec");
        };

        // Populate default binaries for all 13 providers except MistralVibe (give it fallback `vibe` first)
        for &kind in ProviderKind::all() {
            if kind == ProviderKind::MistralVibe {
                make_exec(&bin_dir.join("vibe"));
            } else {
                make_exec(&bin_dir.join(kind.default_binary()));
            }
        }

        let search = vec![bin_dir.clone()];

        // 1. Every provider is discovered from PATH, and MistralVibe falls back to `vibe`
        for &kind in ProviderKind::all() {
            let found = resolve_provider_executable_in(kind, None, &search)
                .expect("should discover provider binary");
            if kind == ProviderKind::MistralVibe {
                assert_eq!(found, bin_dir.join("vibe"));
            } else {
                assert_eq!(found, bin_dir.join(kind.default_binary()));
            }
        }

        // 2. When `vibe-acp` appears, MistralVibe prefers `vibe-acp` over `vibe`
        make_exec(&bin_dir.join("vibe-acp"));
        assert_eq!(
            resolve_provider_executable_in(ProviderKind::MistralVibe, None, &search),
            Some(bin_dir.join("vibe-acp"))
        );

        // 3. Env override path takes precedence over PATH for every single provider
        for &kind in ProviderKind::all() {
            let custom = override_dir.join(format!("custom_{}", kind.slug()));
            make_exec(&custom);
            let custom_str = custom.to_string_lossy().to_string();
            assert_eq!(
                resolve_provider_executable_in(kind, Some(&custom_str), &search),
                Some(custom)
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn fallback_all_provider_models_covers_all_13_providers() {
        let catalogs = fallback_all_provider_models().await;
        assert_eq!(catalogs.len(), 13);
        for &kind in ProviderKind::all() {
            let cat = catalogs
                .iter()
                .find(|c| c.provider == kind)
                .expect("catalog present");
            assert!(
                !cat.models.is_empty(),
                "empty fallback catalog for {kind:?}"
            );
        }
    }
}
