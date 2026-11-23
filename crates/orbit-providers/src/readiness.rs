//! Fast, non-blocking local provider readiness & authentication status probe.
//!
//! Performs purely local filesystem and environment checks (< 20 ms) without ever spawning
//! interactive CLIs or reading/logging/returning secret credential values.

use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use orbit_protocol::ProviderKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
use crate::resolve_provider_executable_in;
use crate::{default_executable_search_dirs, find_provider_executable_in};

/// Maximum number of bytes read from any local config/credential file during readiness probing.
const MAX_PROBE_FILE_BYTES: u64 = 64 * 1024;

/// Primary native coding agent providers always surfaced in onboarding & readiness diagnostics.
pub const PRIMARY_READINESS_PROVIDERS: [ProviderKind; 7] = [
    ProviderKind::ClaudeCode,
    ProviderKind::Codex,
    ProviderKind::Antigravity,
    ProviderKind::GeminiCli,
    ProviderKind::CursorAgent,
    ProviderKind::OpenCode,
    ProviderKind::Copilot,
];

/// Coarse local authentication/session readiness status for a coding agent CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthStatus {
    Authenticated,
    Unauthenticated,
    Unknown,
}

/// Snapshot of a local coding agent CLI's installation and login readiness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReadinessInfo {
    pub kind: ProviderKind,
    pub display_name: String,
    pub installed: bool,
    pub executable_path: Option<String>,
    pub auth_status: ProviderAuthStatus,
    pub install_hint: String,
    pub login_hint: String,
}

/// Fast, non-blocking local readiness probe covering primary native providers plus any
/// additional installed native providers on this machine.
pub fn probe_local_provider_readiness() -> Vec<ProviderReadinessInfo> {
    let search_dirs = default_executable_search_dirs();
    let home = resolve_home_dir();
    let env_lookup = |key: &str| env::var(key).ok();
    let exec_lookup = |kind: ProviderKind| find_provider_executable_in(kind, &search_dirs);
    probe_provider_readiness_with(home.as_deref(), &env_lookup, &exec_lookup)
}

/// Testable core readiness probe accepting injected home directory, environment lookup,
/// and executable resolver so tests remain completely isolated from host state.
pub(crate) fn probe_provider_readiness_with(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
    exec_lookup: &dyn Fn(ProviderKind) -> Option<PathBuf>,
) -> Vec<ProviderReadinessInfo> {
    let mut ordered: Vec<ProviderKind> = PRIMARY_READINESS_PROVIDERS.to_vec();
    let mut seen: HashSet<ProviderKind> = ordered.iter().copied().collect();

    for &kind in ProviderKind::native_all() {
        if !seen.contains(&kind) && exec_lookup(kind).is_some() {
            seen.insert(kind);
            ordered.push(kind);
        }
    }

    let mut results = Vec::with_capacity(ordered.len());
    for kind in ordered {
        let executable = exec_lookup(kind);
        let installed = executable.is_some();
        let auth_status = if installed {
            probe_provider_auth_status(kind, home, env_lookup)
        } else {
            ProviderAuthStatus::Unknown
        };
        let (install_hint, login_hint) = provider_hints(kind);

        results.push(ProviderReadinessInfo {
            kind,
            display_name: kind.display_name().to_string(),
            installed,
            executable_path: executable.map(|p| p.to_string_lossy().to_string()),
            auth_status,
            install_hint: install_hint.to_string(),
            login_hint: login_hint.to_string(),
        });
    }

    results
}

fn resolve_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
        .map(PathBuf::from)
}

fn has_non_empty_env(env_lookup: &dyn Fn(&str) -> Option<String>, keys: &[&str]) -> bool {
    keys.iter().any(|key| {
        env_lookup(key)
            .as_deref()
            .is_some_and(|val| !val.trim().is_empty())
    })
}

fn read_small_file_capped(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() == 0 {
        return None;
    }
    let mut buf = String::new();
    let mut limited = file.take(MAX_PROBE_FILE_BYTES);
    limited.read_to_string(&mut buf).ok()?;
    if buf.trim().is_empty() {
        None
    } else {
        Some(buf)
    }
}

fn read_json_capped(path: &Path) -> Option<Value> {
    let raw = read_small_file_capped(path)?;
    serde_json::from_str::<Value>(&raw).ok()
}

fn file_is_non_empty(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

fn json_string_non_empty(value: &Value) -> bool {
    value.as_str().is_some_and(|s| !s.trim().is_empty())
}

fn json_object_non_empty(value: &Value) -> bool {
    value.as_object().is_some_and(|m| !m.is_empty())
}

fn probe_claude_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(
        env_lookup,
        &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ],
    ) {
        return ProviderAuthStatus::Authenticated;
    }

    let cfg_dir = env_lookup("CLAUDE_CONFIG_DIR")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".claude")));

    if let Some(dir) = cfg_dir.as_deref() {
        if let Some(creds) = read_json_capped(&dir.join(".credentials.json")) {
            let has_oauth = creds.get("claudeAiOauth").is_some_and(|v| {
                v.get("accessToken").is_some_and(json_string_non_empty) || json_object_non_empty(v)
            });
            let has_direct_token = creds.get("accessToken").is_some_and(json_string_non_empty)
                || creds.get("refreshToken").is_some_and(json_string_non_empty)
                || creds.get("apiKey").is_some_and(json_string_non_empty);
            if has_oauth || has_direct_token {
                return ProviderAuthStatus::Authenticated;
            }
        }

        if let Some(settings) = read_json_capped(&dir.join("settings.json")) {
            let has_helper = settings
                .get("apiKeyHelper")
                .is_some_and(json_string_non_empty);
            let has_env_token =
                settings
                    .get("env")
                    .and_then(Value::as_object)
                    .is_some_and(|env_map| {
                        [
                            "ANTHROPIC_API_KEY",
                            "ANTHROPIC_AUTH_TOKEN",
                            "CLAUDE_CODE_OAUTH_TOKEN",
                        ]
                        .iter()
                        .any(|k| env_map.get(*k).is_some_and(json_string_non_empty))
                    });
            if has_helper || has_env_token {
                return ProviderAuthStatus::Authenticated;
            }
        }

        if claude_state_json_has_account(&dir.join(".claude.json")) {
            return ProviderAuthStatus::Authenticated;
        }
    }

    if let Some(h) = home
        && claude_state_json_has_account(&h.join(".claude.json"))
    {
        return ProviderAuthStatus::Authenticated;
    }

    ProviderAuthStatus::Unauthenticated
}

fn claude_state_json_has_account(path: &Path) -> bool {
    let Some(doc) = read_json_capped(path) else {
        return false;
    };
    let has_oauth_account = doc.get("oauthAccount").is_some_and(json_object_non_empty);
    let has_primary_key = doc.get("primaryApiKey").is_some_and(json_string_non_empty);
    let has_account_uuid = doc.get("accountUuid").is_some_and(json_string_non_empty);
    has_oauth_account || has_primary_key || has_account_uuid
}

fn probe_codex_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(env_lookup, &["OPENAI_API_KEY", "CODEX_API_KEY"]) {
        return ProviderAuthStatus::Authenticated;
    }

    let codex_home = env_lookup("CODEX_HOME")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".codex")));

    if let Some(dir) = codex_home.as_deref()
        && let Some(doc) = read_json_capped(&dir.join("auth.json"))
    {
        let has_api_key = doc.get("OPENAI_API_KEY").is_some_and(json_string_non_empty)
            || doc.get("api_key").is_some_and(json_string_non_empty);
        let has_tokens = doc
            .get("tokens")
            .and_then(Value::as_object)
            .is_some_and(|t| {
                t.get("access_token").is_some_and(json_string_non_empty)
                    || t.get("refresh_token").is_some_and(json_string_non_empty)
                    || t.get("id_token").is_some_and(json_string_non_empty)
            });
        if has_api_key || has_tokens {
            return ProviderAuthStatus::Authenticated;
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn probe_antigravity_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(
        env_lookup,
        &[
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "ANTIGRAVITY_API_KEY",
            "GOOGLE_CLOUD_PROJECT",
        ],
    ) {
        return ProviderAuthStatus::Authenticated;
    }

    if let Some(h) = home {
        if gemini_oauth_marker_exists(&h.join(".gemini")) {
            return ProviderAuthStatus::Authenticated;
        }
        let agy_dir = h.join(".gemini").join("antigravity-cli");
        if dir_has_non_empty_auth_artifact(&agy_dir) {
            return ProviderAuthStatus::Authenticated;
        }
        let agy_config_dir = h.join(".config").join("antigravity");
        if dir_has_non_empty_auth_artifact(&agy_config_dir) {
            return ProviderAuthStatus::Authenticated;
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn dir_has_non_empty_auth_artifact(dir: &Path) -> bool {
    let known_files = [
        "credentials.json",
        "oauth_creds.json",
        "auth.json",
        "token.json",
        "session.json",
        "google_accounts.json",
    ];
    for name in known_files {
        let candidate = dir.join(name);
        if let Some(doc) = read_json_capped(&candidate)
            && json_object_non_empty(&doc)
        {
            return true;
        }
    }
    // Antigravity CLI also persists local user state / token caches under `~/.gemini/antigravity-cli`
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pb") || ext.eq_ignore_ascii_case("db"))
                && file_is_non_empty(&path)
            {
                return true;
            }
        }
    }
    false
}

fn gemini_oauth_marker_exists(gemini_dir: &Path) -> bool {
    if let Some(doc) = read_json_capped(&gemini_dir.join("oauth_creds.json"))
        && json_object_non_empty(&doc)
    {
        return true;
    }
    if let Some(doc) = read_json_capped(&gemini_dir.join("google_accounts.json"))
        && doc.get("active").is_some_and(json_string_non_empty)
    {
        return true;
    }
    false
}

fn probe_gemini_cli_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(
        env_lookup,
        &[
            "GEMINI_API_KEY",
            "GOOGLE_API_KEY",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "GOOGLE_GENAI_USE_VERTEXAI",
        ],
    ) {
        return ProviderAuthStatus::Authenticated;
    }

    if let Some(h) = home {
        let gemini_dir = h.join(".gemini");
        if gemini_oauth_marker_exists(&gemini_dir) {
            return ProviderAuthStatus::Authenticated;
        }
        if let Some(settings) = read_json_capped(&gemini_dir.join("settings.json"))
            && settings
                .get("selectedAuthType")
                .is_some_and(json_string_non_empty)
        {
            return ProviderAuthStatus::Authenticated;
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn probe_cursor_agent_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(env_lookup, &["CURSOR_API_KEY"]) {
        return ProviderAuthStatus::Authenticated;
    }

    if let Some(h) = home {
        for candidate in [
            h.join(".cursor").join("auth.json"),
            h.join(".cursor").join("cli-config.json"),
            h.join(".config").join("cursor").join("auth.json"),
        ] {
            if let Some(doc) = read_json_capped(&candidate)
                && json_object_non_empty(&doc)
            {
                return ProviderAuthStatus::Authenticated;
            }
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn probe_opencode_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(
        env_lookup,
        &[
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "OPENROUTER_API_KEY",
            "OPENCODE_API_KEY",
        ],
    ) {
        return ProviderAuthStatus::Authenticated;
    }

    let xdg_data = env_lookup("XDG_DATA_HOME")
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local").join("share")));
    if let Some(data_dir) = xdg_data.as_deref()
        && let Some(doc) = read_json_capped(&data_dir.join("opencode").join("auth.json"))
        && json_object_non_empty(&doc)
    {
        return ProviderAuthStatus::Authenticated;
    }

    if let Some(h) = home {
        for candidate in [
            h.join(".config").join("opencode").join("auth.json"),
            h.join(".config").join("opencode").join("opencode.json"),
        ] {
            if let Some(doc) = read_json_capped(&candidate)
                && json_object_non_empty(&doc)
            {
                return ProviderAuthStatus::Authenticated;
            }
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn probe_copilot_auth(
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    if has_non_empty_env(
        env_lookup,
        &["COPILOT_GITHUB_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"],
    ) {
        return ProviderAuthStatus::Authenticated;
    }

    if let Some(h) = home {
        for json_path in [
            h.join(".config").join("github-copilot").join("hosts.json"),
            h.join(".config").join("github-copilot").join("apps.json"),
            h.join(".copilot").join("config.json"),
        ] {
            if let Some(doc) = read_json_capped(&json_path)
                && json_object_non_empty(&doc)
            {
                return ProviderAuthStatus::Authenticated;
            }
        }
        let gh_hosts = h.join(".config").join("gh").join("hosts.yml");
        if let Some(raw) = read_small_file_capped(&gh_hosts)
            && (raw.contains("oauth_token:") || raw.contains("user:"))
        {
            return ProviderAuthStatus::Authenticated;
        }
    }

    ProviderAuthStatus::Unauthenticated
}

fn probe_provider_auth_status(
    kind: ProviderKind,
    home: Option<&Path>,
    env_lookup: &dyn Fn(&str) -> Option<String>,
) -> ProviderAuthStatus {
    match kind {
        ProviderKind::ClaudeCode => probe_claude_auth(home, env_lookup),
        ProviderKind::Codex => probe_codex_auth(home, env_lookup),
        ProviderKind::Antigravity => probe_antigravity_auth(home, env_lookup),
        ProviderKind::GeminiCli => probe_gemini_cli_auth(home, env_lookup),
        ProviderKind::CursorAgent => probe_cursor_agent_auth(home, env_lookup),
        ProviderKind::OpenCode => probe_opencode_auth(home, env_lookup),
        ProviderKind::Copilot => probe_copilot_auth(home, env_lookup),
        ProviderKind::Goose => {
            if has_non_empty_env(
                env_lookup,
                &[
                    "ANTHROPIC_API_KEY",
                    "OPENAI_API_KEY",
                    "GOOGLE_API_KEY",
                    "OPENROUTER_API_KEY",
                ],
            ) || home.is_some_and(|h| {
                file_is_non_empty(&h.join(".config").join("goose").join("config.yaml"))
            }) {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unknown
            }
        }
        ProviderKind::Pi => {
            if has_non_empty_env(
                env_lookup,
                &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"],
            ) || home.is_some_and(|h| {
                read_json_capped(&h.join(".pi").join("agent").join("auth.json"))
                    .is_some_and(|d| json_object_non_empty(&d))
            }) {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unauthenticated
            }
        }
        ProviderKind::Kimi => {
            if has_non_empty_env(env_lookup, &["KIMI_API_KEY", "MOONSHOT_API_KEY"])
                || home.is_some_and(|h| {
                    file_is_non_empty(&h.join(".kimi").join("config.toml"))
                        || read_json_capped(&h.join(".kimi").join("credentials.json"))
                            .is_some_and(|d| json_object_non_empty(&d))
                })
            {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unauthenticated
            }
        }
        ProviderKind::Cline => {
            if has_non_empty_env(
                env_lookup,
                &["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY"],
            ) || home.is_some_and(|h| {
                file_is_non_empty(&h.join(".cline").join("data").join("globalState.json"))
            }) {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unknown
            }
        }
        ProviderKind::MistralVibe => {
            if has_non_empty_env(env_lookup, &["MISTRAL_API_KEY"])
                || home.is_some_and(|h| {
                    file_is_non_empty(&h.join(".vibe").join("config.toml"))
                        || file_is_non_empty(
                            &h.join(".config").join("mistral-vibe").join("config.toml"),
                        )
                })
            {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unauthenticated
            }
        }
        ProviderKind::Augment => {
            if has_non_empty_env(env_lookup, &["AUGMENT_API_TOKEN", "AUGMENT_SESSION_AUTH"])
                || home.is_some_and(|h| {
                    read_json_capped(&h.join(".augment").join("session.json"))
                        .is_some_and(|d| json_object_non_empty(&d))
                })
            {
                ProviderAuthStatus::Authenticated
            } else {
                ProviderAuthStatus::Unauthenticated
            }
        }
        ProviderKind::Acp => ProviderAuthStatus::Unknown,
    }
}

fn provider_hints(kind: ProviderKind) -> (&'static str, &'static str) {
    match kind {
        ProviderKind::ClaudeCode => (
            "npm install -g @anthropic-ai/claude-code",
            "claude auth login",
        ),
        ProviderKind::Codex => ("npm install -g @openai/codex", "codex login"),
        ProviderKind::Antigravity => ("brew install antigravity-cli", "agy auth login"),
        ProviderKind::GeminiCli => ("npm install -g @google/gemini-cli", "gemini"),
        ProviderKind::CursorAgent => (
            "curl https://cursor.com/install -fsS | bash",
            "cursor-agent login",
        ),
        ProviderKind::OpenCode => ("brew install opencode", "opencode auth login"),
        ProviderKind::Copilot => ("npm install -g @github/copilot", "copilot login"),
        ProviderKind::Goose => ("brew install block-goose-cli", "goose configure"),
        ProviderKind::Pi => ("npm install -g @mariozechner/pi-coding-agent", "pi"),
        ProviderKind::Kimi => ("uv tool install kimi-cli", "kimi login"),
        ProviderKind::Cline => ("npm install -g cline", "cline auth"),
        ProviderKind::MistralVibe => ("uv tool install mistral-vibe", "vibe"),
        ProviderKind::Augment => ("npm install -g @augmentcode/auggie", "auggie login"),
        ProviderKind::Acp => ("", ""),
    }
}

/// Convenience helper for resolving provider executable against an explicit search path in tests.
#[cfg(test)]
fn resolve_exec_in_dirs(kind: ProviderKind, dirs: &[PathBuf]) -> Option<PathBuf> {
    resolve_provider_executable_in(kind, None, dirs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_readiness_{tag}_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[cfg(unix)]
    fn make_exec(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, "#!/bin/sh\nexit 0\n").expect("write exec");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    #[cfg(not(unix))]
    fn make_exec(path: &Path) {
        std::fs::write(path, "echo ok").expect("write exec");
    }

    #[test]
    fn not_installed_primary_providers_report_unknown_and_hints() {
        let tmp = make_temp_dir("empty");
        let empty_bin = tmp.join("bin");
        let empty_home = tmp.join("home");
        std::fs::create_dir_all(&empty_bin).expect("mkdir bin");
        std::fs::create_dir_all(&empty_home).expect("mkdir home");

        let env_map: HashMap<&str, &str> = HashMap::new();
        let search_dirs = vec![empty_bin];
        let results = probe_provider_readiness_with(
            Some(&empty_home),
            &|k| env_map.get(k).map(|v| (*v).to_string()),
            &|kind| resolve_exec_in_dirs(kind, &search_dirs),
        );

        assert_eq!(results.len(), PRIMARY_READINESS_PROVIDERS.len());
        for item in &results {
            assert!(!item.installed, "expected {:?} not installed", item.kind);
            assert!(item.executable_path.is_none());
            assert_eq!(item.auth_status, ProviderAuthStatus::Unknown);
            assert!(!item.install_hint.is_empty());
            assert!(!item.login_hint.is_empty());
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn installed_providers_detect_unauthenticated_and_credential_files() {
        let tmp = make_temp_dir("files");
        let bin_dir = tmp.join("bin");
        let home_dir = tmp.join("home");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");

        // Install Claude, Codex, Antigravity, Gemini, plus extra native provider Pi
        make_exec(&bin_dir.join(ProviderKind::ClaudeCode.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::Codex.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::Antigravity.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::GeminiCli.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::Pi.default_binary()));

        let search_dirs = vec![bin_dir];
        let env_map: HashMap<&str, &str> = HashMap::new();

        // 1. Before any credentials exist -> installed ones should be Unauthenticated
        let before = probe_provider_readiness_with(
            Some(&home_dir),
            &|k| env_map.get(k).map(|v| (*v).to_string()),
            &|kind| resolve_exec_in_dirs(kind, &search_dirs),
        );
        assert_eq!(before.len(), PRIMARY_READINESS_PROVIDERS.len() + 1);
        for kind in [
            ProviderKind::ClaudeCode,
            ProviderKind::Codex,
            ProviderKind::Antigravity,
            ProviderKind::GeminiCli,
            ProviderKind::Pi,
        ] {
            let entry = before
                .iter()
                .find(|r| r.kind == kind)
                .expect("provider present");
            assert!(entry.installed);
            assert!(entry.executable_path.is_some());
            assert_eq!(
                entry.auth_status,
                ProviderAuthStatus::Unauthenticated,
                "expected {:?} unauthenticated initially",
                kind
            );
        }

        // 2. Empty JSON `{}` should NOT count as authenticated
        std::fs::create_dir_all(home_dir.join(".codex")).expect("mkdir .codex");
        std::fs::write(home_dir.join(".codex").join("auth.json"), "{}").expect("write empty");
        let empty_json_probe = probe_provider_readiness_with(
            Some(&home_dir),
            &|k| env_map.get(k).map(|v| (*v).to_string()),
            &|kind| resolve_exec_in_dirs(kind, &search_dirs),
        );
        let codex_empty = empty_json_probe
            .iter()
            .find(|r| r.kind == ProviderKind::Codex)
            .expect("codex present");
        assert_eq!(codex_empty.auth_status, ProviderAuthStatus::Unauthenticated);

        // 3. Populate valid local credential markers -> Authenticated
        std::fs::write(
            home_dir.join(".claude.json"),
            r#"{"oauthAccount":{"accountUuid":"acct-1"}}"#,
        )
        .expect("write .claude.json");
        std::fs::write(
            home_dir.join(".codex").join("auth.json"),
            r#"{"tokens":{"access_token":"tok-redacted"}}"#,
        )
        .expect("write codex auth.json");
        std::fs::create_dir_all(home_dir.join(".gemini").join("antigravity-cli"))
            .expect("mkdir agy");
        std::fs::write(
            home_dir
                .join(".gemini")
                .join("antigravity-cli")
                .join("credentials.json"),
            r#"{"token":"ok"}"#,
        )
        .expect("write agy creds");
        std::fs::write(
            home_dir.join(".gemini").join("oauth_creds.json"),
            r#"{"access_token":"ok"}"#,
        )
        .expect("write gemini oauth");

        let after = probe_provider_readiness_with(
            Some(&home_dir),
            &|k| env_map.get(k).map(|v| (*v).to_string()),
            &|kind| resolve_exec_in_dirs(kind, &search_dirs),
        );
        for kind in [
            ProviderKind::ClaudeCode,
            ProviderKind::Codex,
            ProviderKind::Antigravity,
            ProviderKind::GeminiCli,
        ] {
            let entry = after
                .iter()
                .find(|r| r.kind == kind)
                .expect("provider present");
            assert_eq!(
                entry.auth_status,
                ProviderAuthStatus::Authenticated,
                "expected {:?} authenticated after credential files",
                kind
            );
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn env_vars_mark_installed_providers_authenticated() {
        let tmp = make_temp_dir("env");
        let bin_dir = tmp.join("bin");
        let home_dir = tmp.join("home");
        std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
        std::fs::create_dir_all(&home_dir).expect("mkdir home");

        make_exec(&bin_dir.join(ProviderKind::ClaudeCode.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::CursorAgent.default_binary()));
        make_exec(&bin_dir.join(ProviderKind::Copilot.default_binary()));

        let mut env_map: HashMap<&str, &str> = HashMap::new();
        env_map.insert("ANTHROPIC_API_KEY", "sk-ant-placeholder");
        env_map.insert("CURSOR_API_KEY", "cur-placeholder");
        env_map.insert("GH_TOKEN", "ghp-placeholder");

        let search_dirs = vec![bin_dir];
        let results = probe_provider_readiness_with(
            Some(&home_dir),
            &|k| env_map.get(k).map(|v| (*v).to_string()),
            &|kind| resolve_exec_in_dirs(kind, &search_dirs),
        );

        for kind in [
            ProviderKind::ClaudeCode,
            ProviderKind::CursorAgent,
            ProviderKind::Copilot,
        ] {
            let entry = results
                .iter()
                .find(|r| r.kind == kind)
                .expect("provider present");
            assert!(entry.installed);
            assert_eq!(entry.auth_status, ProviderAuthStatus::Authenticated);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
