//! First-class native Rust drivers for 13 coding agent CLIs (Antigravity, Codex, Claude Code,
//! OpenCode, Gemini CLI, Cursor Agent, GitHub Copilot, Goose, Pi, Kimi CLI, Cline, Mistral Vibe,
//! Augment) plus Universal ACP (`acp_generic` + curated 34-agent ACP Registry).

pub mod acp_callbacks;
pub mod acp_generic;
pub mod acp_providers;
pub mod acp_registry;
pub mod antigravity;
pub mod attachments;
pub mod claude;
pub mod codex;
pub mod driver;
mod managed_dispatch;
pub mod pi;
pub mod plan_approval;
pub mod readiness;
pub mod shell_snapshot;
pub mod side_question;

use std::collections::{HashMap, HashSet};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use orbit_protocol::{ModelOption, ProviderKind, ProviderModelCatalog, ProviderQuotaSnapshot};
use serde::{Deserialize, Serialize};
use similar::TextDiff;

pub const MAX_UNIFIED_DIFF_BYTES: usize = 24 * 1024;
const UNIFIED_DIFF_CONTEXT_RADIUS: usize = 3;

/// Compact an existing unified diff down to ±3 context lines around changed (`+`/`-`) hunks and
/// enforce a hard UTF-8-safe byte cap (`max_bytes`) with a clean truncation footer.
pub fn compact_unified_diff(diff: &str, max_bytes: usize) -> String {
    if diff.is_empty() {
        return String::new();
    }

    let compacted = compact_unified_diff_context(diff, UNIFIED_DIFF_CONTEXT_RADIUS);
    truncate_unified_diff_bytes(compacted, max_bytes)
}

fn compact_unified_diff_context(diff: &str, context_radius: usize) -> String {
    let lines: Vec<&str> = diff.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let has_trailing_newline = diff.ends_with('\n');
    let mut out: Vec<String> = Vec::with_capacity(lines.len().min(256));
    let mut i = 0usize;

    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") {
            let header = line;
            let parsed_header = parse_hunk_header(header);
            i += 1;
            let body_start = i;
            if let Some((_, old_total, _, new_total)) = parsed_header {
                let mut old_rem = old_total;
                let mut new_rem = new_total;
                while i < lines.len() {
                    let cur = lines[i];
                    if old_rem == 0 && new_rem == 0 {
                        if cur.starts_with('\\') {
                            i += 1;
                        }
                        break;
                    }
                    if cur.starts_with(' ') {
                        old_rem = old_rem.saturating_sub(1);
                        new_rem = new_rem.saturating_sub(1);
                        i += 1;
                    } else if cur.starts_with('-') {
                        old_rem = old_rem.saturating_sub(1);
                        i += 1;
                    } else if cur.starts_with('+') {
                        new_rem = new_rem.saturating_sub(1);
                        i += 1;
                    } else if cur.starts_with('\\') {
                        i += 1;
                    } else {
                        break;
                    }
                }
            } else {
                while i < lines.len()
                    && !lines[i].starts_with("@@ ")
                    && !lines[i].starts_with("diff --git ")
                    && !is_file_header_pair(&lines, i)
                {
                    i += 1;
                }
            }
            let body = &lines[body_start..i];
            let parsed_start = parsed_header.map(|(os, _, ns, _)| (os, ns));
            emit_compacted_hunk(
                Some(header),
                parsed_start,
                body,
                context_radius,
                true,
                &mut out,
            );
        } else if line.starts_with("diff --git ")
            || line.starts_with("index ")
            || is_file_header_pair(&lines, i)
            || line.starts_with("+++ ")
            || line.starts_with("new file mode ")
            || line.starts_with("deleted file mode ")
            || line.starts_with("old mode ")
            || line.starts_with("new mode ")
            || line.starts_with("rename from ")
            || line.starts_with("rename to ")
            || line.starts_with("similarity index ")
            || line.starts_with("Binary files ")
        {
            out.push(line.to_string());
            i += 1;
        } else if is_diff_body_line(line) {
            let body_start = i;
            while i < lines.len()
                && !lines[i].starts_with("@@ ")
                && !lines[i].starts_with("diff --git ")
                && !is_file_header_pair(&lines, i)
                && is_diff_body_line(lines[i])
            {
                i += 1;
            }
            let body = &lines[body_start..i];
            emit_compacted_hunk(None, None, body, context_radius, false, &mut out);
        } else {
            out.push(line.to_string());
            i += 1;
        }
    }

    let mut joined = out.join("\n");
    if has_trailing_newline && !joined.is_empty() {
        joined.push('\n');
    }
    joined
}

fn is_file_header_pair(lines: &[&str], idx: usize) -> bool {
    lines[idx].starts_with("--- ")
        && lines
            .get(idx + 1)
            .is_some_and(|next| next.starts_with("+++ "))
}

fn is_diff_body_line(line: &str) -> bool {
    line.starts_with(' ')
        || line.starts_with('+')
        || line.starts_with('-')
        || line.starts_with('\\')
}

fn is_diff_change_line(line: &str, inside_hunk: bool) -> bool {
    if inside_hunk {
        line.starts_with('+') || line.starts_with('-')
    } else {
        (line.starts_with('+') && !line.starts_with("+++ "))
            || (line.starts_with('-') && !line.starts_with("--- "))
    }
}

fn parse_hunk_range_spec(spec: &str) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = spec.split_once(',') {
        Some((
            start_s.parse::<usize>().ok()?,
            count_s.parse::<usize>().ok()?,
        ))
    } else {
        Some((spec.parse::<usize>().ok()?, 1))
    }
}

fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize, usize)> {
    let rest = header.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let range_spec = &rest[..end];
    let mut parts = range_spec.split_whitespace();
    let old_part = parts.next()?.strip_prefix('-')?;
    let new_part = parts.next()?.strip_prefix('+')?;
    let (old_start, old_count) = parse_hunk_range_spec(old_part)?;
    let (new_start, new_count) = parse_hunk_range_spec(new_part)?;
    Some((old_start, old_count, new_start, new_count))
}

fn emit_compacted_hunk(
    original_header: Option<&str>,
    parsed_start: Option<(usize, usize)>,
    body: &[&str],
    context_radius: usize,
    inside_hunk: bool,
    out: &mut Vec<String>,
) {
    let change_indices: Vec<usize> = body
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| is_diff_change_line(line, inside_hunk).then_some(idx))
        .collect();

    if change_indices.is_empty() {
        if let Some(h) = original_header {
            out.push(h.to_string());
        }
        for line in body {
            out.push((*line).to_string());
        }
        return;
    }

    // Build merged [start..=end] windows around each change line with `context_radius` lines of context.
    let mut windows: Vec<(usize, usize)> = Vec::new();
    for idx in change_indices {
        let start = idx.saturating_sub(context_radius);
        let mut end = (idx + context_radius).min(body.len().saturating_sub(1));
        // Keep trailing `\ No newline at end of file` marker attached to its preceding line.
        if end + 1 < body.len() && body[end + 1].starts_with('\\') {
            end += 1;
        }
        if let Some(last) = windows.last_mut()
            && start <= last.1 + 1
        {
            last.1 = last.1.max(end);
        } else {
            windows.push((start, end));
        }
    }

    // Fast path: hunk is already compact (single window covering the entire body).
    if windows.len() == 1 && windows[0].0 == 0 && windows[0].1 + 1 == body.len() {
        if let Some(h) = original_header {
            out.push(h.to_string());
        }
        for line in body {
            out.push((*line).to_string());
        }
        return;
    }

    // Precompute old/new line offsets for accurate @@ headers when splitting a wide hunk.
    let mut old_before: Vec<usize> = Vec::with_capacity(body.len() + 1);
    let mut new_before: Vec<usize> = Vec::with_capacity(body.len() + 1);
    let mut old_acc = 0usize;
    let mut new_acc = 0usize;
    for line in body {
        old_before.push(old_acc);
        new_before.push(new_acc);
        if line.starts_with(' ') {
            old_acc += 1;
            new_acc += 1;
        } else if line.starts_with('-') {
            old_acc += 1;
        } else if line.starts_with('+') {
            new_acc += 1;
        }
    }
    old_before.push(old_acc);
    new_before.push(new_acc);

    for (w_start, w_end) in windows {
        let old_count = old_before[w_end + 1].saturating_sub(old_before[w_start]);
        let new_count = new_before[w_end + 1].saturating_sub(new_before[w_start]);
        if let Some((base_old, base_new)) = parsed_start {
            let h_old = base_old.saturating_add(old_before[w_start]);
            let h_new = base_new.saturating_add(new_before[w_start]);
            out.push(format!("@@ -{h_old},{old_count} +{h_new},{new_count} @@"));
        } else {
            out.push("@@ ... @@".to_string());
        }
        for line in &body[w_start..=w_end] {
            out.push((*line).to_string());
        }
    }
}

fn truncate_unified_diff_bytes(mut rendered: String, max_bytes: usize) -> String {
    if rendered.len() <= max_bytes {
        return rendered;
    }
    let footer = if max_bytes >= 1024 && max_bytes.is_multiple_of(1024) {
        format!(
            "\n… [diff truncated at {} KB — open Git Review for full diff]",
            max_bytes / 1024
        )
    } else {
        "\n… [diff truncated — open Git Review for full diff]".to_string()
    };
    if max_bytes <= footer.len() {
        let mut cut = max_bytes;
        while cut > 0 && !rendered.is_char_boundary(cut) {
            cut -= 1;
        }
        rendered.truncate(cut);
        return rendered;
    }
    let mut cut = max_bytes - footer.len();
    while cut > 0 && !rendered.is_char_boundary(cut) {
        cut -= 1;
    }
    if let Some(nl) = rendered[..cut].rfind('\n')
        && nl > 0
    {
        cut = nl;
    }
    rendered.truncate(cut);
    rendered.push_str(&footer);
    rendered
}

/// Build a standard compact unified diff (3 context lines per hunk) from `old_text` and `new_text`.
/// Returns `None` when both texts are identical, and safely caps extreme payloads at 24 KiB on a
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
    let rendered = diff
        .unified_diff()
        .context_radius(UNIFIED_DIFF_CONTEXT_RADIUS)
        .header(&old_header, &new_header)
        .to_string();

    if rendered.trim().is_empty() {
        return None;
    }

    Some(compact_unified_diff(&rendered, MAX_UNIFIED_DIFF_BYTES))
}

pub use acp_registry::{
    ACP_PROBE_TIMEOUT, DEFAULT_ACP_REGISTRY_URL, MAX_REMOTE_REGISTRY_BYTES, PersistedAcpRegistry,
    acp_registry_path, builtin_acp_catalog, delete_custom_acp_agent, get_acp_agent_spec,
    has_any_installed_acp_agent, list_acp_agents, load_persisted_acp_registry, merged_acp_specs,
    parse_remote_registry_json, probe_acp_agent, refresh_acp_registry, resolve_acp_agent_command,
    save_persisted_acp_registry, upsert_custom_acp_agent, validate_acp_agent_spec,
    validate_remote_registry_url,
};
pub use claude::{reap_snapshotted_claude_mcp_configs, snapshot_claude_mcp_config_files};
pub use driver::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, PersistedBackgroundTask, PromptOrigin,
    ProviderSessionHandle, SessionLaunchConfig,
};
pub use pi::{reap_snapshotted_pi_mcp_configs, snapshot_pi_mcp_config_files};
pub use readiness::{
    PRIMARY_READINESS_PROVIDERS, ProviderAuthStatus, ProviderReadinessInfo,
    probe_local_provider_readiness,
};
pub use shell_snapshot::{
    ShellSnapshotCache, ShellSnapshotStats, apply_cached_shell_env, apply_cached_std_shell_env,
    invalidate_shell_snapshot, merge_cached_shell_env_into_map, resolve_login_shell_env,
    resolve_login_shell_env_cached, set_snapshot_orbit_home, shell_snapshot_stats,
    warm_login_shell_env_in_background,
};
pub use side_question::{
    HardenedGuardianCommandSpec, SideQuestionSpec, build_hardened_guardian_command_spec,
    is_hardened_guardian_provider, run_ephemeral_text_prompt, run_hardened_guardian_prompt,
    run_side_question, run_tool_free_prompt, validate_cli_model_name,
};

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
/// (Generic `ProviderKind::Acp` agents are discovered dynamically via `acp_registry::list_acp_agents`.)
pub fn discover_installed_providers() -> Vec<DiscoveredProvider> {
    let search_dirs = default_executable_search_dirs();
    let mut found = Vec::new();
    for &kind in ProviderKind::native_all() {
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
        #[cfg(windows)]
        {
            push_unique(home.join("AppData").join("Roaming").join("npm"));
            push_unique(home.join("AppData").join("Local").join("Programs"));
        }
    }
    #[cfg(unix)]
    for extra in [
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
    ] {
        push_unique(extra);
    }

    search_dirs
}

/// Build ordered executable candidate filenames for Windows (`PATHEXT` / `.exe`, `.cmd`, `.bat`, `.com`).
pub fn windows_executable_candidates(
    dir: &Path,
    binary_name: &str,
    pathext: Option<&str>,
) -> Vec<PathBuf> {
    let trimmed = binary_name.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let exts: Vec<String> = pathext
        .unwrap_or(".EXE;.CMD;.BAT;.COM")
        .split(';')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            if lower.starts_with('.') {
                lower
            } else {
                format!(".{lower}")
            }
        })
        .collect();
    let lower_name = trimmed.to_ascii_lowercase();
    if exts.iter().any(|ext| lower_name.ends_with(ext)) {
        return vec![dir.join(trimmed)];
    }
    let mut out = Vec::with_capacity(exts.len() + 1);
    for ext in &exts {
        out.push(dir.join(format!("{trimmed}{ext}")));
    }
    out.push(dir.join(trimmed));
    out
}

fn find_executable_in_dirs(binary_name: &str, search_dirs: &[PathBuf]) -> Option<PathBuf> {
    #[cfg(windows)]
    let pathext = env::var("PATHEXT").ok();
    for dir in search_dirs {
        #[cfg(windows)]
        {
            for candidate in windows_executable_candidates(dir, binary_name, pathext.as_deref()) {
                if is_executable_file(&candidate) {
                    return Some(candidate);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let candidate = dir.join(binary_name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn find_executable(binary_name: &str) -> Option<PathBuf> {
    let search_dirs = default_executable_search_dirs();
    find_executable_in_dirs(binary_name, &search_dirs)
}

pub(crate) fn is_executable_file(path: &Path) -> bool {
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
        ProviderKind::Acp => acp_generic::spawn_acp_session(cfg).await,
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
    for &kind in ProviderKind::native_all() {
        match kind {
            ProviderKind::ClaudeCode
            | ProviderKind::Codex
            | ProviderKind::Antigravity
            | ProviderKind::Acp => {}
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
            ProviderKind::Acp => {}
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

/// Determine whether `(provider, model)` supports Fast output mode, preferring live probed
/// `ProviderModelCatalog` capabilities when cached and falling back to provider heuristics.
pub fn provider_model_supports_fast_mode(
    provider: ProviderKind,
    model: Option<&str>,
    cached_catalogs: Option<&[ProviderModelCatalog]>,
) -> bool {
    let trimmed_model = model.map(str::trim).filter(|s| !s.is_empty());

    if let Some(catalog) = cached_catalogs.and_then(|cats| {
        cats.iter()
            .find(|c| c.provider == provider && !c.models.is_empty())
    }) {
        if let Some(m) = trimmed_model {
            if let Some(found) = catalog.models.iter().find(|s| s.id.eq_ignore_ascii_case(m)) {
                return found.supports_fast_mode;
            }
            // Strip Antigravity wire thinking-effort suffix (e.g. `gemini-4-carbon-high`)
            if provider == ProviderKind::Antigravity {
                let lower = m.to_ascii_lowercase();
                for suffix in [
                    "-off", "-low", "-medium", "-high", "-xhigh", "-max", "-ultra",
                ] {
                    if let Some(base) = lower.strip_suffix(suffix)
                        && let Some(found) = catalog
                            .models
                            .iter()
                            .find(|s| s.id.eq_ignore_ascii_case(base))
                    {
                        return found.supports_fast_mode;
                    }
                }
            }
            // Resolve Claude short family aliases (`opus`, `sonnet`, `haiku`, `fable`) against catalog
            if provider == ProviderKind::ClaudeCode {
                let lower = m.to_ascii_lowercase();
                if matches!(lower.as_str(), "opus" | "sonnet" | "haiku" | "fable") {
                    let needle = format!("-{lower}-");
                    let tail = format!("-{lower}");
                    if let Some(found) = catalog.models.iter().find(|s| {
                        let sid = s.id.to_ascii_lowercase();
                        sid.contains(&needle) || sid.ends_with(&tail)
                    }) {
                        return found.supports_fast_mode;
                    }
                }
            }
        } else {
            // No explicit model: resolve catalog's configured or marked default
            if let Some(def_id) = catalog.configured_default_model.as_deref()
                && let Some(found) = catalog
                    .models
                    .iter()
                    .find(|s| s.id.eq_ignore_ascii_case(def_id))
            {
                return found.supports_fast_mode;
            }
            if let Some(found) = catalog
                .models
                .iter()
                .find(|s| s.is_default)
                .or_else(|| catalog.models.first())
            {
                return found.supports_fast_mode;
            }
        }
    }

    match provider {
        ProviderKind::ClaudeCode => match trimmed_model {
            Some(m) => claude::claude_model_supports_fast_mode(m),
            None => claude::fallback_claude_models()
                .models
                .iter()
                .find(|m| m.is_default)
                .map(|m| m.supports_fast_mode)
                .unwrap_or(true),
        },
        ProviderKind::Codex => match trimmed_model {
            Some(m) => codex::codex_model_supports_fast_mode(m, None),
            None => true,
        },
        ProviderKind::Antigravity => match trimmed_model {
            Some(m) => antigravity::antigravity_model_supports_fast_mode(m, None, false),
            None => antigravity::fallback_antigravity_models()
                .models
                .iter()
                .find(|m| m.is_default)
                .map(|m| m.supports_fast_mode)
                .unwrap_or(true),
        },
        _ => false,
    }
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

/// Strictly parsed ACP `session/request_permission` option IDs.
///
/// Enforces a strict boundary between one-shot `allow_once` and persistent
/// `allow_always` options so `"allow"` NEVER escalates to an `allow_always`
/// provider grant when `allow_once` is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AcpPermissionOptions {
    pub allow_once: Option<String>,
    pub allow_always: Option<String>,
    pub deny: Option<String>,
}

impl AcpPermissionOptions {
    pub(crate) fn parse(options: &[serde_json::Value]) -> Self {
        if options.is_empty() {
            return Self {
                allow_once: Some("allow_once".to_string()),
                allow_always: None,
                deny: Some("reject_once".to_string()),
            };
        }

        fn opt_id(o: &serde_json::Value) -> Option<String> {
            o.get("optionId")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        }
        fn opt_kind(o: &serde_json::Value) -> Option<&str> {
            o.get("kind")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        }

        let mut allow_once = options
            .iter()
            .find(|o| opt_kind(o) == Some("allow_once"))
            .and_then(opt_id);
        let mut allow_always = options
            .iter()
            .find(|o| opt_kind(o) == Some("allow_always"))
            .and_then(opt_id);
        let mut deny = options
            .iter()
            .find(|o| opt_kind(o) == Some("reject_once"))
            .and_then(opt_id)
            .or_else(|| {
                options
                    .iter()
                    .find(|o| opt_kind(o) == Some("reject_always"))
                    .and_then(opt_id)
            });

        // Substring fallback applies ONLY to legacy options that omit `kind`,
        // and NEVER lets `allow_once` match an option mentioning "always".
        for o in options {
            if opt_kind(o).is_some() {
                continue;
            }
            let Some(id) = opt_id(o) else {
                continue;
            };
            let id_lower = id.to_ascii_lowercase();
            let name_lower = o
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();

            let is_deny = id_lower.contains("reject")
                || id_lower.contains("deny")
                || id_lower.contains("cancel")
                || name_lower.contains("reject")
                || name_lower.contains("deny")
                || name_lower.contains("cancel");
            if is_deny {
                if deny.is_none() {
                    deny = Some(id);
                }
                continue;
            }

            let is_allow = id_lower.contains("allow")
                || id_lower.contains("approve")
                || name_lower.contains("allow")
                || name_lower.contains("approve");
            if !is_allow {
                continue;
            }

            let is_always = id_lower.contains("always") || name_lower.contains("always");
            if is_always {
                if allow_always.is_none() {
                    allow_always = Some(id);
                }
            } else if allow_once.is_none() {
                allow_once = Some(id);
            }
        }

        Self {
            allow_once,
            allow_always,
            deny,
        }
    }

    pub(crate) fn allowed_decisions(
        &self,
        is_plan_approval: bool,
        allow_persistent_grant: bool,
    ) -> Vec<String> {
        let mut decisions = Vec::with_capacity(3);
        if self.allow_once.is_some() {
            decisions.push("allow".to_string());
        }
        if !is_plan_approval && allow_persistent_grant && self.allow_always.is_some() {
            decisions.push("allow_always".to_string());
        }
        decisions.push("deny".to_string());
        decisions
    }

    pub(crate) fn outcome_for_decision(&self, decision: &str) -> serde_json::Value {
        match decision {
            // Strict: `"allow"` NEVER falls back to `allow_always`.
            "allow" => match self.allow_once.as_deref() {
                Some(opt_id) => serde_json::json!({
                    "outcome": "selected",
                    "optionId": opt_id
                }),
                None => serde_json::json!({ "outcome": "cancelled" }),
            },
            "allow_always" => match self.allow_always.as_deref().or(self.allow_once.as_deref()) {
                Some(opt_id) => serde_json::json!({
                    "outcome": "selected",
                    "optionId": opt_id
                }),
                None => serde_json::json!({ "outcome": "cancelled" }),
            },
            _ => self.deny_outcome(),
        }
    }

    pub(crate) fn deny_outcome(&self) -> serde_json::Value {
        match self.deny.as_deref() {
            Some(opt_id) => serde_json::json!({
                "outcome": "selected",
                "optionId": opt_id
            }),
            None => serde_json::json!({ "outcome": "cancelled" }),
        }
    }

    pub(crate) fn question_option_id(&self) -> &str {
        self.allow_once.as_deref().unwrap_or("allow_once")
    }
}

/// Extract an explicit shell working directory from ACP `rawInput`, checking all known field names.
pub(crate) fn extract_acp_shell_cwd(raw_in: Option<&serde_json::Value>) -> Option<String> {
    raw_in
        .and_then(|i| {
            i.get("Cwd")
                .or_else(|| i.get("cwd"))
                .or_else(|| i.get("workingDirectory"))
                .or_else(|| i.get("working_dir"))
                .or_else(|| i.get("workdir"))
        })
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

/// Returns `true` when an ACP `session/request_permission` `toolCall` carries an
/// authoritative command, path, or tool identifier in structured fields rather than
/// falling back to a human-readable display `title`.
///
/// Title-only confirmations MUST stay individual: Orbit omits `"allow_always"` and
/// marks the detail with `"__orbit_title_only": true` so display prose can never be
/// auto-allowed or persisted as a cross-provider permission rule.
pub(crate) fn acp_tool_call_has_authoritative_identity(tool_call: &serde_json::Value) -> bool {
    let kind = tool_call
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let raw_in = tool_call.get("rawInput");
    let non_empty_str = |v: Option<&serde_json::Value>| {
        v.and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    };

    if kind == "execute"
        || raw_in
            .and_then(|i| {
                i.get("CommandLine")
                    .or_else(|| i.get("command"))
                    .or_else(|| i.get("cmd"))
            })
            .is_some()
    {
        return non_empty_str(raw_in.and_then(|i| {
            i.get("CommandLine")
                .or_else(|| i.get("command"))
                .or_else(|| i.get("cmd"))
        }));
    }

    if matches!(kind, "edit" | "write" | "delete" | "move" | "read") {
        let has_input_path = non_empty_str(raw_in.and_then(|i| {
            i.get("path")
                .or_else(|| i.get("Path"))
                .or_else(|| i.get("file_path"))
                .or_else(|| i.get("TargetFile"))
                .or_else(|| i.get("AbsolutePath"))
        }));
        let has_location_path = non_empty_str(tool_call.pointer("/locations/0/path"));
        return has_input_path || has_location_path;
    }

    let has_non_empty_raw_input = match raw_in {
        Some(serde_json::Value::Object(map)) => !map.is_empty(),
        Some(serde_json::Value::Array(arr)) => !arr.is_empty(),
        Some(serde_json::Value::String(s)) => !s.trim().is_empty(),
        Some(serde_json::Value::Null) | None => false,
        Some(_) => true,
    };
    let title = tool_call
        .get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let title_is_identifier = !title.is_empty()
        && !title
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control());
    title_is_identifier && (has_non_empty_raw_input || !kind.is_empty())
}

/// Build an explicit title-only `ToolCallDetail::Generic` marked with `"__orbit_title_only": true`
/// so permission evaluation keeps display-title prompts individual instead of matching shell/path rules.
pub(crate) fn acp_title_only_detail(
    title: &str,
    tool_call: &serde_json::Value,
) -> orbit_protocol::ToolCallDetail {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "__orbit_title_only".to_string(),
        serde_json::Value::Bool(true),
    );
    payload.insert(
        "title".to_string(),
        serde_json::Value::String(title.to_string()),
    );
    if let Some(kind) = tool_call.get("kind").and_then(serde_json::Value::as_str)
        && !kind.trim().is_empty()
    {
        payload.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
    }
    if let Some(raw_in) = tool_call.get("rawInput")
        && !raw_in.is_null()
    {
        payload.insert("rawInput".to_string(), raw_in.clone());
    }
    orbit_protocol::ToolCallDetail::Generic {
        tool_name: title.to_string(),
        arguments_json: serde_json::Value::Object(payload).to_string(),
    }
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

        for &kind in ProviderKind::native_all() {
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
        for &kind in ProviderKind::native_all() {
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
        for &kind in ProviderKind::native_all() {
            if kind == ProviderKind::MistralVibe {
                make_exec(&bin_dir.join("vibe"));
            } else {
                make_exec(&bin_dir.join(kind.default_binary()));
            }
        }

        let search = vec![bin_dir.clone()];

        // 1. Every provider is discovered from PATH, and MistralVibe falls back to `vibe`
        for &kind in ProviderKind::native_all() {
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
        for &kind in ProviderKind::native_all() {
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
        for &kind in ProviderKind::native_all() {
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

#[cfg(test)]
mod capability_tests {
    use super::*;
    use orbit_protocol::ProviderModelSpec;

    #[test]
    fn provider_model_supports_fast_mode_uses_catalog_and_fallbacks() {
        let catalogs = vec![
            ProviderModelCatalog {
                provider: ProviderKind::ClaudeCode,
                models: vec![
                    ProviderModelSpec {
                        id: "claude-opus-4-6".into(),
                        label: "Opus 4.6".into(),
                        description: String::new(),
                        is_default: true,
                        default_effort: None,
                        thinking_options: Vec::new(),
                        supports_fast_mode: true,
                    },
                    ProviderModelSpec {
                        id: "claude-sonnet-4-6".into(),
                        label: "Sonnet 4.6".into(),
                        description: String::new(),
                        is_default: false,
                        default_effort: None,
                        thinking_options: Vec::new(),
                        supports_fast_mode: false,
                    },
                ],
                configured_default_model: Some("claude-opus-4-6".into()),
                configured_default_effort: None,
            },
            ProviderModelCatalog {
                provider: ProviderKind::Antigravity,
                models: vec![
                    ProviderModelSpec {
                        id: "gemini-4-carbon".into(),
                        label: "Gemini 4 Carbon".into(),
                        description: String::new(),
                        is_default: true,
                        default_effort: None,
                        thinking_options: Vec::new(),
                        supports_fast_mode: true,
                    },
                    ProviderModelSpec {
                        id: "gemini-3.8-flash".into(),
                        label: "Gemini 3.8 Flash".into(),
                        description: String::new(),
                        is_default: false,
                        default_effort: None,
                        thinking_options: Vec::new(),
                        supports_fast_mode: false,
                    },
                ],
                configured_default_model: None,
                configured_default_effort: None,
            },
        ];

        assert!(provider_model_supports_fast_mode(
            ProviderKind::ClaudeCode,
            Some("opus"),
            Some(&catalogs)
        ));
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::ClaudeCode,
            Some("sonnet"),
            Some(&catalogs)
        ));
        assert!(provider_model_supports_fast_mode(
            ProviderKind::Antigravity,
            Some("gemini-4-carbon-high"),
            Some(&catalogs)
        ));
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::Antigravity,
            Some("gemini-3.8-flash-medium"),
            Some(&catalogs)
        ));
        // Fallbacks when no cached catalog exists
        assert!(provider_model_supports_fast_mode(
            ProviderKind::Codex,
            Some("gpt-5.4"),
            None
        ));
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::Codex,
            Some("gpt-oss-120b"),
            None
        ));
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::ClaudeCode,
            Some("claude-haiku-4-5"),
            None
        ));
        // Non-core ACP/Pi providers default to false unless catalog specifies otherwise
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::OpenCode,
            Some("openai/gpt-5"),
            None
        ));
        assert!(!provider_model_supports_fast_mode(
            ProviderKind::Pi,
            Some("default"),
            None
        ));
    }

    #[test]
    fn compact_unified_diff_compacts_full_file_context_to_3_lines_and_caps_utf8_safely() {
        // Build a synthetic full-file diff with 40 leading context lines, 1 edit, 40 middle
        // context lines, 1 edit, and 40 trailing context lines.
        let mut raw = String::from("--- a/demo.rs\n+++ b/demo.rs\n@@ -1,122 +1,122 @@\n");
        for i in 1..=40 {
            raw.push_str(&format!(" line_{i}\n"));
        }
        raw.push_str("-old_mid_1\n+new_mid_1\n");
        for i in 42..=81 {
            raw.push_str(&format!(" line_{i}\n"));
        }
        raw.push_str("-old_mid_2\n+new_mid_2\n");
        for i in 83..=122 {
            raw.push_str(&format!(" line_{i}\n"));
        }

        let compacted = compact_unified_diff(&raw, MAX_UNIFIED_DIFF_BYTES);
        assert!(compacted.contains("-old_mid_1\n+new_mid_1"));
        assert!(compacted.contains("-old_mid_2\n+new_mid_2"));
        // Leading/middle/trailing far-away context lines must be dropped to ±3 lines
        assert!(!compacted.contains(" line_1\n"));
        assert!(!compacted.contains(" line_37\n"));
        assert!(compacted.contains(" line_38\n"));
        assert!(compacted.contains(" line_44\n"));
        assert!(!compacted.contains(" line_45\n"));
        assert!(!compacted.contains(" line_78\n"));
        assert!(compacted.contains(" line_79\n"));
        assert!(compacted.contains(" line_85\n"));
        assert!(!compacted.contains(" line_86\n"));
        assert!(
            compacted.matches("@@ ").count() == 2,
            "expected two compacted hunks, got:\n{compacted}"
        );

        // Giant UTF-8 diff (> 24 KiB with multibyte CJK characters) must cap cleanly <= 24 KiB
        let mut giant = String::from("--- a/big.txt\n+++ b/big.txt\n@@ -1,1 +1,1000 @@\n");
        for i in 0..1000 {
            giant.push_str(&format!(
                "+第{i:04}行：这是一段用于测试截断边界的中文内容 payload\n"
            ));
        }
        assert!(giant.len() > MAX_UNIFIED_DIFF_BYTES);
        let capped = compact_unified_diff(&giant, MAX_UNIFIED_DIFF_BYTES);
        assert!(
            capped.len() <= MAX_UNIFIED_DIFF_BYTES,
            "capped len {} exceeded {}",
            capped.len(),
            MAX_UNIFIED_DIFF_BYTES
        );
        assert!(capped.ends_with("… [diff truncated at 24 KB — open Git Review for full diff]"));
    }

    #[test]
    fn compact_unified_diff_preserves_deleted_sql_comments_yaml_separators_and_plus_plus_lines() {
        let mut raw = String::from("--- a/query.sql\n+++ b/query.sql\n@@ -1,24 +1,24 @@\n");
        for i in 1..=10 {
            raw.push_str(&format!(" SELECT {i};\n"));
        }
        // Deleting `-- SQL comment` produces `--- SQL comment`, deleting `---` produces `----`,
        // adding `++ x` produces `+++ x` — all MUST stay inside the hunk and be treated as changes!
        raw.push_str("--- SQL comment deleted\n");
        raw.push_str("----\n");
        raw.push_str("+++ x\n");
        raw.push_str("+++ y\n");
        for i in 11..=22 {
            raw.push_str(&format!(" SELECT {i};\n"));
        }

        let compacted = compact_unified_diff(&raw, MAX_UNIFIED_DIFF_BYTES);
        assert!(compacted.contains("--- SQL comment deleted"));
        assert!(compacted.contains("----\n"));
        assert!(compacted.contains("+++ x\n"));
        assert!(compacted.contains("+++ y\n"));
        assert!(
            !compacted.contains(" SELECT 1;\n"),
            "far leading context must be compacted:\n{compacted}"
        );
        assert!(
            compacted.contains(" SELECT 8;\n") && compacted.contains(" SELECT 13;\n"),
            "±3 context lines around --/++ changes must be preserved:\n{compacted}"
        );
        assert!(
            !compacted.contains(" SELECT 14;\n"),
            "far trailing context must be compacted:\n{compacted}"
        );
        assert_eq!(
            compacted.matches("@@ ").count(),
            1,
            "deleted -- and added ++ lines must NOT split the hunk:\n{compacted}"
        );
    }

    #[test]
    fn windows_executable_candidates_append_pathext_extensions() {
        let dir = Path::new(r"C:\Users\dev\AppData\Roaming\npm");
        let cands = windows_executable_candidates(dir, "codex", Some(".EXE;.CMD;.BAT"));
        let names: Vec<String> = cands
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert_eq!(names, vec!["codex.exe", "codex.cmd", "codex.bat", "codex"]);

        let already_ext = windows_executable_candidates(dir, "claude.exe", Some(".EXE;.CMD"));
        assert_eq!(already_ext.len(), 1);
        assert_eq!(
            already_ext[0]
                .file_name()
                .map(|n| n.to_string_lossy().to_string()),
            Some("claude.exe".to_string())
        );
    }

    #[test]
    fn acp_permission_options_strictly_separates_allow_once_and_allow_always() {
        // Finding 10: When ACP only offers `allow_always` + `reject_once`, `"allow"` MUST NOT
        // be advertised and resolving `"allow"` MUST cancel rather than escalate to `allow_always`!
        let only_always = vec![
            serde_json::json!({ "optionId": "opt_always", "kind": "allow_always" }),
            serde_json::json!({ "optionId": "opt_deny", "kind": "reject_once" }),
        ];
        let parsed = AcpPermissionOptions::parse(&only_always);
        assert_eq!(parsed.allow_once, None);
        assert_eq!(parsed.allow_always.as_deref(), Some("opt_always"));
        assert_eq!(parsed.deny.as_deref(), Some("opt_deny"));
        assert_eq!(
            parsed.allowed_decisions(false, true),
            vec!["allow_always".to_string(), "deny".to_string()]
        );
        assert_eq!(
            parsed.outcome_for_decision("allow"),
            serde_json::json!({ "outcome": "cancelled" })
        );
        assert_eq!(
            parsed.outcome_for_decision("allow_always"),
            serde_json::json!({ "outcome": "selected", "optionId": "opt_always" })
        );
        assert_eq!(
            parsed.outcome_for_decision("deny"),
            serde_json::json!({ "outcome": "selected", "optionId": "opt_deny" })
        );

        // When only `allow_once` + `reject_always` are offered, `"allow_always"` is omitted
        let only_once = vec![
            serde_json::json!({ "optionId": "a1", "kind": "allow_once" }),
            serde_json::json!({ "optionId": "r_all", "kind": "reject_always" }),
        ];
        let parsed_once = AcpPermissionOptions::parse(&only_once);
        assert_eq!(
            parsed_once.allowed_decisions(false, true),
            vec!["allow".to_string(), "deny".to_string()]
        );
        assert_eq!(
            parsed_once.deny_outcome(),
            serde_json::json!({ "outcome": "selected", "optionId": "r_all" })
        );

        // Legacy options without `kind` still never map `allow_always` into `allow_once`
        let legacy = vec![
            serde_json::json!({ "optionId": "approve_always", "name": "Always Allow" }),
            serde_json::json!({ "optionId": "cancel_action" }),
        ];
        let parsed_legacy = AcpPermissionOptions::parse(&legacy);
        assert_eq!(parsed_legacy.allow_once, None);
        assert_eq!(
            parsed_legacy.allow_always.as_deref(),
            Some("approve_always")
        );
        assert_eq!(parsed_legacy.deny.as_deref(), Some("cancel_action"));

        // Non-authoritative / plan-approval prompts never advertise `allow_always`
        let full = vec![
            serde_json::json!({ "optionId": "once", "kind": "allow_once" }),
            serde_json::json!({ "optionId": "always", "kind": "allow_always" }),
            serde_json::json!({ "optionId": "deny", "kind": "reject_once" }),
        ];
        let parsed_full = AcpPermissionOptions::parse(&full);
        assert_eq!(
            parsed_full.allowed_decisions(false, true),
            vec![
                "allow".to_string(),
                "allow_always".to_string(),
                "deny".to_string()
            ]
        );
        assert_eq!(
            parsed_full.allowed_decisions(false, false),
            vec!["allow".to_string(), "deny".to_string()]
        );
        assert_eq!(
            parsed_full.allowed_decisions(true, true),
            vec!["allow".to_string(), "deny".to_string()]
        );
    }

    #[test]
    fn acp_authoritative_identity_rejects_title_only_prompts() {
        // Missing CommandLine/command on execute -> NOT authoritative
        assert!(!acp_tool_call_has_authoritative_identity(
            &serde_json::json!({
                "kind": "execute",
                "title": "cargo test",
                "rawInput": {}
            })
        ));
        // Concrete CommandLine -> authoritative
        assert!(acp_tool_call_has_authoritative_identity(
            &serde_json::json!({
                "kind": "execute",
                "title": "Run tests",
                "rawInput": { "CommandLine": "cargo test", "Cwd": "/repo" }
            })
        ));
        // Missing path on edit -> NOT authoritative
        assert!(!acp_tool_call_has_authoritative_identity(
            &serde_json::json!({
                "kind": "edit",
                "title": "src/lib.rs",
                "rawInput": {}
            })
        ));
        // Concrete path in locations -> authoritative
        assert!(acp_tool_call_has_authoritative_identity(
            &serde_json::json!({
                "kind": "edit",
                "title": "Edit file",
                "locations": [{ "path": "src/lib.rs" }]
            })
        ));
        // Prose confirmation title with no structured input -> NOT authoritative
        assert!(!acp_tool_call_has_authoritative_identity(
            &serde_json::json!({
                "title": "Confirm workspace migration"
            })
        ));
    }
}
