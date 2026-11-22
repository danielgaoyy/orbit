//! Pure-Rust LSP Language Server Manager for Orbit Daemon.
//!
//! Features:
//! - Auto-discovery of installed language servers on PATH + `$ORBIT_HOME/lsp.json` + project `.lsp.json` / `.orbit/lsp.json`.
//! - Strict bare-binary allowlist for untrusted project `.lsp.json` overrides, higher precedence for host-local `$ORBIT_HOME/lsp.json` (`UserConfig`), child env scrubbing, and canonical workspace-root containment.
//! - Lazy-start per `(canonical_cwd, server_id)` with bounded active session cap (`MAX_ACTIVE_LSP_SESSIONS`), automatic idle shutdown (10m default), and graceful `shutdown` + `exit` + `kill_on_drop`.
//! - Pure-Rust JSON-RPC 2.0 `Content-Length` framing over stdio with bounded header/body caps, pending request timeouts, non-blocking reader loop, and bounded `publishDiagnostics` caching.
//! - All wire coordinates use 1-based line and 1-based UTF-16 code-unit column indexing.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use orbit_protocol::{
    LspConfigSource, LspDiagnostic, LspDiagnosticSeverity, LspLocation, LspServerConfig,
    LspServerState, LspServerStatus, LspSymbolInfo,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as AsyncMutex, oneshot};
use tokio::task::JoinHandle;
use tracing::{debug, warn};

use crate::workspace_files::{canonicalize_workspace_root, is_path_inside_git, sha256_hex};

/// Maximum JSON-RPC message body size accepted from an LSP server (16 MiB).
pub const MAX_LSP_FRAME_BYTES: usize = 16 * 1024 * 1024;
/// Maximum single header line length accepted in an LSP frame (8 KiB).
pub const MAX_LSP_HEADER_LINE_BYTES: u64 = 8192;
/// Maximum concurrent active LSP server sessions per daemon instance.
pub const MAX_ACTIVE_LSP_SESSIONS: usize = 12;
/// Maximum document size sent to an LSP server in `didOpen`/`didChange` (2 MiB).
const MAX_LSP_DOC_BYTES: usize = 2 * 1024 * 1024;
/// Maximum number of files retained in a single LSP session's diagnostics cache.
const MAX_DIAGNOSTIC_FILES: usize = 256;
/// Maximum diagnostics retained per file.
const MAX_DIAGNOSTICS_PER_FILE: usize = 200;
/// Maximum total diagnostics returned across a workspace.
const MAX_TOTAL_DIAGNOSTICS: usize = 2000;
/// Maximum symbols returned by a single query.
const MAX_SYMBOLS_LIMIT: usize = 200;
/// Maximum definition locations returned by a single query.
const MAX_LOCATIONS_LIMIT: usize = 50;
/// Default timeout for regular LSP requests (`workspace/symbol`, `definition`, etc.).
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Default timeout for LSP `initialize` handshake.
const DEFAULT_INIT_TIMEOUT: Duration = Duration::from_secs(10);
/// Default idle auto-shutdown timeout (10 minutes).
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Brief settle delay after opening a fresh document when querying diagnostics.
const FRESH_OPEN_DIAGNOSTICS_SETTLE: Duration = Duration::from_millis(120);

/// Strict bare-binary allowlist permitted in untrusted project `.lsp.json` / `.orbit/lsp.json`.
/// Arbitrary custom binaries must be configured in host-local `$ORBIT_HOME/lsp.json` (`UserConfig`).
pub const ALLOWED_PROJECT_COMMANDS: &[&str] = &[
    "rust-analyzer",
    "typescript-language-server",
    "pyright-langserver",
    "basedpyright-langserver",
    "ruff",
    "gopls",
    "clangd",
    "zls",
    "jdtls",
    "sourcekit-lsp",
    "kotlin-language-server",
    "lua-language-server",
    "svelte-language-server",
    "vue-language-server",
    "taplo",
    "tinymist",
    "yaml-language-server",
    "vscode-json-language-server",
    "marksman",
    "texlab",
    "nil",
    "nixd",
    "ruby-lsp",
    "solargraph",
    "elixir-ls",
    "ocamllsp",
    "gleam",
];

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Returns `true` if an environment variable name must be stripped before spawning an LSP child.
pub fn should_strip_lsp_env_var(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    upper.starts_with("ORBIT_")
        || upper.starts_with("LD_")
        || upper.starts_with("DYLD_")
        || matches!(
            upper.as_str(),
            "BASH_ENV"
                | "ENV"
                | "NODE_OPTIONS"
                | "PYTHONSTARTUP"
                | "PYTHONPATH"
                | "PERL5OPT"
                | "RUBYOPT"
                | "GIT_SSH_COMMAND"
        )
}

/// Built-in catalog of well-known language servers with zero external runtime dependencies.
pub fn builtin_lsp_catalog() -> Vec<LspServerConfig> {
    vec![
        LspServerConfig {
            id: "rust-analyzer".into(),
            name: "rust-analyzer".into(),
            command: "rust-analyzer".into(),
            args: Vec::new(),
            file_extensions: vec!["rs".into()],
            language_ids: vec!["rust".into()],
            root_markers: vec!["Cargo.toml".into(), "rust-project.json".into()],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "typescript-language-server".into(),
            name: "TypeScript".into(),
            command: "typescript-language-server".into(),
            args: vec!["--stdio".into()],
            file_extensions: vec![
                "ts".into(),
                "tsx".into(),
                "js".into(),
                "jsx".into(),
                "mjs".into(),
                "cjs".into(),
                "mts".into(),
                "cts".into(),
            ],
            language_ids: vec![
                "typescript".into(),
                "typescriptreact".into(),
                "javascript".into(),
                "javascriptreact".into(),
            ],
            root_markers: vec![
                "tsconfig.json".into(),
                "jsconfig.json".into(),
                "package.json".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "pyright".into(),
            name: "Pyright".into(),
            command: "pyright-langserver".into(),
            args: vec!["--stdio".into()],
            file_extensions: vec!["py".into(), "pyi".into()],
            language_ids: vec!["python".into()],
            root_markers: vec![
                "pyproject.toml".into(),
                "pyrightconfig.json".into(),
                "setup.py".into(),
                "requirements.txt".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "gopls".into(),
            name: "gopls".into(),
            command: "gopls".into(),
            args: Vec::new(),
            file_extensions: vec!["go".into()],
            language_ids: vec!["go".into()],
            root_markers: vec!["go.mod".into(), "go.work".into()],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "clangd".into(),
            name: "clangd".into(),
            command: "clangd".into(),
            args: Vec::new(),
            file_extensions: vec![
                "c".into(),
                "h".into(),
                "cc".into(),
                "cpp".into(),
                "cxx".into(),
                "hpp".into(),
                "hh".into(),
                "hxx".into(),
                "m".into(),
                "mm".into(),
            ],
            language_ids: vec![
                "c".into(),
                "cpp".into(),
                "objective-c".into(),
                "objective-cpp".into(),
            ],
            root_markers: vec![
                "compile_commands.json".into(),
                "CMakeLists.txt".into(),
                ".clangd".into(),
                "Makefile".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "zls".into(),
            name: "ZLS".into(),
            command: "zls".into(),
            args: Vec::new(),
            file_extensions: vec!["zig".into(), "zon".into()],
            language_ids: vec!["zig".into()],
            root_markers: vec!["build.zig".into(), "build.zig.zon".into()],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "jdtls".into(),
            name: "Eclipse JDTLS".into(),
            command: "jdtls".into(),
            args: Vec::new(),
            file_extensions: vec!["java".into()],
            language_ids: vec!["java".into()],
            root_markers: vec![
                "pom.xml".into(),
                "build.gradle".into(),
                "build.gradle.kts".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "sourcekit-lsp".into(),
            name: "SourceKit-LSP".into(),
            command: "sourcekit-lsp".into(),
            args: Vec::new(),
            file_extensions: vec!["swift".into()],
            language_ids: vec!["swift".into()],
            root_markers: vec!["Package.swift".into()],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "kotlin-language-server".into(),
            name: "Kotlin".into(),
            command: "kotlin-language-server".into(),
            args: Vec::new(),
            file_extensions: vec!["kt".into(), "kts".into()],
            language_ids: vec!["kotlin".into()],
            root_markers: vec![
                "build.gradle.kts".into(),
                "build.gradle".into(),
                "settings.gradle.kts".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
        LspServerConfig {
            id: "lua-language-server".into(),
            name: "Lua".into(),
            command: "lua-language-server".into(),
            args: Vec::new(),
            file_extensions: vec!["lua".into()],
            language_ids: vec!["lua".into()],
            root_markers: vec![
                ".luarc.json".into(),
                ".luarc.jsonc".into(),
                "init.lua".into(),
            ],
            enabled: true,
            source: LspConfigSource::Builtin,
        },
    ]
}

fn fallback_binaries_for_builtin(id: &str) -> &'static [&'static str] {
    match id {
        "pyright" => &["basedpyright-langserver"],
        _ => &[],
    }
}

/// Validate a configured LSP server ID and command for safety.
pub fn validate_server_config(cfg: &LspServerConfig) -> Result<(), String> {
    let id = cfg.id.trim();
    if id.is_empty()
        || id.len() > 64
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(format!("invalid LSP server id `{}`", cfg.id));
    }

    let cmd = cfg.command.trim();
    if cmd.is_empty() {
        return Err(format!("LSP server `{id}` has an empty command"));
    }
    if cmd.chars().any(|c| {
        c == '\0'
            || c.is_control()
            || matches!(
                c,
                ';' | '|' | '&' | '`' | '$' | '(' | ')' | '<' | '>' | '\n' | '\r'
            )
    }) {
        return Err(format!(
            "LSP server `{id}` command `{cmd}` contains forbidden shell characters"
        ));
    }
    if cfg.args.len() > 32 {
        return Err(format!(
            "LSP server `{id}` exceeds maximum argument count (32)"
        ));
    }
    for arg in &cfg.args {
        if arg.contains('\0') || arg.contains('\n') || arg.contains('\r') {
            return Err(format!(
                "LSP server `{id}` argument contains invalid control characters"
            ));
        }
    }

    if cfg.source == LspConfigSource::ProjectJson {
        if cmd.contains('/')
            || cmd.contains('\\')
            || cmd.contains("..")
            || Path::new(cmd).components().count() != 1
        {
            return Err(format!(
                "Project `.lsp.json` command `{cmd}` for `{id}` must be a bare allowlisted binary name (configure custom paths in `$ORBIT_HOME/lsp.json`)"
            ));
        }
        if !ALLOWED_PROJECT_COMMANDS.contains(&cmd) {
            return Err(format!(
                "Project `.lsp.json` command `{cmd}` for `{id}` is not in the built-in LSP allowlist (configure custom binaries in `$ORBIT_HOME/lsp.json`)"
            ));
        }
    }

    Ok(())
}

/// Validate & canonicalize a workspace `cwd` for LSP operations (blocks `/`, `$HOME`, `.git`, non-dirs).
pub fn validate_workspace_cwd(cwd: &Path) -> Result<PathBuf, String> {
    canonicalize_workspace_root(cwd)
}

/// Resolve a file path strictly inside `canonical_cwd`, rejecting traversal, symlink escapes,
/// and `.git` internals. Returns `(canonical_file_path, workspace_relative_unix_path)`.
pub fn resolve_workspace_file(
    canonical_cwd: &Path,
    raw_path: &str,
) -> Result<(PathBuf, String), String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return Err("File path cannot be empty".into());
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("File path contains invalid control characters".into());
    }
    if trimmed.contains('\\') {
        return Err("File path must use forward slashes (`/`)".into());
    }

    let candidate = Path::new(trimmed);
    let full = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        for comp in candidate.components() {
            match comp {
                Component::ParentDir => {
                    return Err(format!("Path `{trimmed}` escapes workspace boundary"));
                }
                Component::Normal(seg) if seg.to_string_lossy().eq_ignore_ascii_case(".git") => {
                    return Err("Accessing `.git` internals via LSP is forbidden".into());
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(format!("Invalid path `{trimmed}`"));
                }
                Component::CurDir | Component::Normal(_) => {}
            }
        }
        canonical_cwd.join(candidate)
    };

    let canonical_file = std::fs::canonicalize(&full)
        .map_err(|e| format!("File `{trimmed}` not found in workspace: {e}"))?;
    if !canonical_file.starts_with(canonical_cwd) {
        return Err(format!("Path `{trimmed}` escapes workspace boundary"));
    }
    if is_path_inside_git(canonical_cwd, &canonical_file) {
        return Err("Accessing `.git` internals via LSP is forbidden".into());
    }
    let meta = std::fs::metadata(&canonical_file)
        .map_err(|e| format!("Cannot inspect file `{trimmed}`: {e}"))?;
    if !meta.is_file() {
        return Err(format!("Path `{trimmed}` is not a file"));
    }

    let rel = canonical_file
        .strip_prefix(canonical_cwd)
        .map_err(|_| format!("Path `{trimmed}` escapes workspace boundary"))?;
    let rel_str = path_to_unix_string(rel);
    Ok((canonical_file, rel_str))
}

fn path_to_unix_string(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Convert a canonical filesystem path into a standards-compliant `file://` URI.
pub fn path_to_file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let mut encoded = String::with_capacity(raw.len() + 8);
    encoded.push_str("file://");
    if !raw.starts_with('/') {
        encoded.push('/');
    }
    for &b in raw.as_bytes() {
        let keep =
            b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~' | b':');
        if keep {
            encoded.push(b as char);
        } else {
            let _ = std::fmt::Write::write_fmt(&mut encoded, format_args!("%{b:02X}"));
        }
    }
    encoded
}

/// Convert a local `file:///` or `file://localhost/` URI back into a `PathBuf`.
/// Rejects remote authorities (e.g. `file://evil.host/etc/passwd`), relative file URIs, and NUL bytes.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let without_suffix = uri.find(['?', '#']).map(|idx| &uri[..idx]).unwrap_or(uri);

    let path_part = without_suffix
        .strip_prefix("file://localhost/")
        .or_else(|| without_suffix.strip_prefix("file:///"))?;

    let bytes = path_part.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() + 1);
    decoded.push(b'/');
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            let b = (hi << 4) | lo;
            if b == 0 {
                return None;
            }
            decoded.push(b);
            i += 3;
            continue;
        }
        if bytes[i] == 0 {
            return None;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let s = String::from_utf8(decoded).ok()?;
    Some(PathBuf::from(s))
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Clamp a 1-based `(line, utf16_col)` pair to 0-based LSP `(line, utf16_character)` without
/// exceeding the target line's UTF-16 length or splitting a UTF-16 surrogate pair.
pub fn clamp_utf16_col_for_line(content: &str, line_1based: u32, col_1based: u32) -> (u32, u32) {
    let lsp_line = line_1based.saturating_sub(1);
    let desired_col = col_1based.saturating_sub(1);
    let Some(target_idx) = usize::try_from(lsp_line).ok() else {
        return (lsp_line, desired_col);
    };
    let Some(line_str) = content.split('\n').nth(target_idx) else {
        return (lsp_line, desired_col);
    };
    let trimmed = line_str.strip_suffix('\r').unwrap_or(line_str);
    let mut utf16_pos: u32 = 0;
    for ch in trimmed.chars() {
        let ch_units = u32::try_from(ch.len_utf16()).unwrap_or(1);
        if desired_col < utf16_pos.saturating_add(ch_units) {
            // If `desired_col` lands on the trailing half of a surrogate pair, snap to its start.
            return (lsp_line, utf16_pos);
        }
        utf16_pos = utf16_pos.saturating_add(ch_units);
    }
    (lsp_line, utf16_pos)
}

/// Resolve a binary name or path on `PATH` (plus common user tool directories).
pub fn resolve_binary(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    let direct = Path::new(trimmed);
    if direct.components().count() > 1 || direct.is_absolute() {
        return is_executable_file(direct).then(|| direct.to_path_buf());
    }

    let mut search_dirs: Vec<PathBuf> = Vec::new();
    if let Some(path_var) = std::env::var_os("PATH") {
        search_dirs.extend(std::env::split_paths(&path_var));
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        search_dirs.push(home.join(".cargo/bin"));
        search_dirs.push(home.join("go/bin"));
        search_dirs.push(home.join(".local/bin"));
        search_dirs.push(home.join(".nvm/current/bin"));
    }
    for sys_dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        search_dirs.push(PathBuf::from(sys_dir));
    }

    let mut seen = HashSet::new();
    for dir in search_dirs {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let candidate = dir.join(trimmed);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
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

/// Resolve binary for a config, checking built-in fallback binary names if needed.
fn resolve_config_binary(cfg: &LspServerConfig) -> Option<PathBuf> {
    if let Some(found) = resolve_binary(&cfg.command) {
        return Some(found);
    }
    if cfg.source == LspConfigSource::Builtin {
        for fallback in fallback_binaries_for_builtin(&cfg.id) {
            if let Some(found) = resolve_binary(fallback) {
                return Some(found);
            }
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct RawLspEntry {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    command: Option<String>,
    #[serde(default)]
    args: Option<Vec<String>>,
    #[serde(default, alias = "extensions", alias = "extensionToLanguage")]
    file_extensions: Option<RawExtensions>,
    #[serde(default)]
    language_ids: Option<Vec<String>>,
    #[serde(default)]
    root_markers: Option<Vec<String>>,
    #[serde(default)]
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawExtensions {
    List(Vec<String>),
    Map(HashMap<String, String>),
}

/// Parse a `.lsp.json` / `.orbit/lsp.json` / `$ORBIT_HOME/lsp.json` document into `LspServerConfig`s.
pub fn parse_lsp_config_json(
    raw: &str,
    source: LspConfigSource,
) -> Result<Vec<LspServerConfig>, String> {
    let val: Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid LSP JSON config: {e}"))?;
    let Some(obj) = val.as_object() else {
        return Err("LSP JSON config root must be an object".into());
    };

    let mut parsed: Vec<LspServerConfig> = Vec::new();
    if let Some(servers_val) = obj.get("servers") {
        match servers_val {
            Value::Array(items) => {
                for item in items {
                    let entry: RawLspEntry = serde_json::from_value(item.clone())
                        .map_err(|e| format!("Invalid LSP server entry: {e}"))?;
                    if let Some(cfg) = raw_entry_to_config(None, entry, source)? {
                        parsed.push(cfg);
                    }
                }
            }
            Value::Object(map) => {
                for (key, item) in map {
                    let entry: RawLspEntry = serde_json::from_value(item.clone())
                        .map_err(|e| format!("Invalid LSP server `{key}`: {e}"))?;
                    if let Some(cfg) = raw_entry_to_config(Some(key.clone()), entry, source)? {
                        parsed.push(cfg);
                    }
                }
            }
            _ => return Err("`servers` field must be an array or object".into()),
        }
    } else {
        for (key, item) in obj {
            if key.starts_with('$') {
                continue;
            }
            if !item.is_object() {
                continue;
            }
            let entry: RawLspEntry = serde_json::from_value(item.clone())
                .map_err(|e| format!("Invalid LSP server `{key}`: {e}"))?;
            if let Some(cfg) = raw_entry_to_config(Some(key.clone()), entry, source)? {
                parsed.push(cfg);
            }
        }
    }

    Ok(parsed)
}

fn raw_entry_to_config(
    key_id: Option<String>,
    entry: RawLspEntry,
    source: LspConfigSource,
) -> Result<Option<LspServerConfig>, String> {
    let id = entry
        .id
        .or(key_id)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "LSP server config entry is missing `id`".to_string())?;

    let command = entry
        .command
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.clone());

    let (file_extensions, inferred_langs) = match entry.file_extensions {
        Some(RawExtensions::List(list)) => (
            list.into_iter()
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
            Vec::new(),
        ),
        Some(RawExtensions::Map(map)) => {
            let mut exts = Vec::new();
            let mut langs = Vec::new();
            for (k, v) in map {
                let clean_ext = k.trim().trim_start_matches('.').to_ascii_lowercase();
                if !clean_ext.is_empty() {
                    exts.push(clean_ext);
                }
                let clean_lang = v.trim().to_ascii_lowercase();
                if !clean_lang.is_empty() && !langs.contains(&clean_lang) {
                    langs.push(clean_lang);
                }
            }
            exts.sort();
            langs.sort();
            (exts, langs)
        }
        None => (Vec::new(), Vec::new()),
    };

    let language_ids = entry.language_ids.unwrap_or(inferred_langs);

    let cfg = LspServerConfig {
        name: entry.name.unwrap_or_else(|| id.clone()),
        id,
        command,
        args: entry.args.unwrap_or_default(),
        file_extensions,
        language_ids,
        root_markers: entry.root_markers.unwrap_or_default(),
        enabled: entry.enabled.unwrap_or(true),
        source,
    };
    validate_server_config(&cfg)?;
    Ok(Some(cfg))
}

/// Load merged effective LSP server configs for a workspace.
/// Precedence (lowest to highest): `Builtin` < `ProjectJson` (`.lsp.json`, `.orbit/lsp.json`) < `UserConfig` (`$ORBIT_HOME/lsp.json`),
/// ensuring an untrusted repository can never hijack a host-local user-configured server ID.
pub fn load_effective_configs(home_dir: &Path, canonical_cwd: &Path) -> Vec<LspServerConfig> {
    let mut by_id: HashMap<String, LspServerConfig> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for cfg in builtin_lsp_catalog() {
        order.push(cfg.id.clone());
        by_id.insert(cfg.id.clone(), cfg);
    }

    let candidate_files = [
        (
            canonical_cwd.join(".lsp.json"),
            LspConfigSource::ProjectJson,
        ),
        (
            canonical_cwd.join(".orbit/lsp.json"),
            LspConfigSource::ProjectJson,
        ),
        (home_dir.join("lsp.json"), LspConfigSource::UserConfig),
    ];

    for (path, source) in candidate_files {
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_lsp_config_json(&raw, source) {
            Ok(overrides) => {
                for mut cfg in overrides {
                    if let Some(existing) = by_id.get(&cfg.id) {
                        if cfg.file_extensions.is_empty() {
                            cfg.file_extensions = existing.file_extensions.clone();
                        }
                        if cfg.language_ids.is_empty() {
                            cfg.language_ids = existing.language_ids.clone();
                        }
                        if cfg.root_markers.is_empty() {
                            cfg.root_markers = existing.root_markers.clone();
                        }
                    } else {
                        order.push(cfg.id.clone());
                    }
                    by_id.insert(cfg.id.clone(), cfg);
                }
            }
            Err(err) => {
                warn!(path = %path.display(), error = %err, "Ignoring invalid LSP config file");
            }
        }
    }

    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .filter(|cfg| cfg.enabled)
        .collect()
}

fn workspace_matches_root_markers(canonical_cwd: &Path, markers: &[String]) -> bool {
    markers.iter().any(|m| {
        let clean = m.trim();
        !clean.is_empty() && canonical_cwd.join(clean).exists()
    })
}

fn language_id_for_path(path: &Path, cfg: &LspServerConfig) -> String {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "rs" => "rust".into(),
        "ts" | "mts" | "cts" => "typescript".into(),
        "tsx" => "typescriptreact".into(),
        "js" | "mjs" | "cjs" => "javascript".into(),
        "jsx" => "javascriptreact".into(),
        "py" | "pyi" => "python".into(),
        "go" => "go".into(),
        "c" | "h" => "c".into(),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => "cpp".into(),
        "m" => "objective-c".into(),
        "mm" => "objective-cpp".into(),
        "zig" | "zon" => "zig".into(),
        "java" => "java".into(),
        "swift" => "swift".into(),
        "kt" | "kts" => "kotlin".into(),
        "lua" => "lua".into(),
        _ => cfg
            .language_ids
            .first()
            .cloned()
            .unwrap_or_else(|| "plaintext".into()),
    }
}

// ============================================================================
// Pure-Rust JSON-RPC 2.0 Content-Length Framing
// ============================================================================

/// Read a single `Content-Length` framed JSON-RPC 2.0 message from an async buffered reader.
/// Returns `Ok(None)` cleanly on EOF. Enforces strict header line (`8 KiB`) and body (`16 MiB`) caps.
pub async fn read_lsp_message<R: AsyncBufRead + Unpin>(
    reader: &mut R,
) -> Result<Option<Value>, String> {
    let mut content_length: Option<usize> = None;
    let mut header_line = String::new();
    let mut header_count = 0usize;

    loop {
        header_line.clear();
        let n = reader
            .take(MAX_LSP_HEADER_LINE_BYTES)
            .read_line(&mut header_line)
            .await
            .map_err(|e| format!("LSP read header error: {e}"))?;
        if n == 0 {
            if header_count == 0 {
                return Ok(None);
            }
            return Err("Unexpected EOF while reading LSP headers".into());
        }
        if (n as u64) >= MAX_LSP_HEADER_LINE_BYTES && !header_line.ends_with('\n') {
            return Err(format!(
                "LSP header line exceeded {MAX_LSP_HEADER_LINE_BYTES}-byte limit"
            ));
        }
        header_count += 1;
        if header_count > 64 {
            return Err("LSP frame exceeded maximum header count".into());
        }
        let trimmed = header_line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, val)) = trimmed.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            let parsed = val
                .trim()
                .parse::<usize>()
                .map_err(|_| format!("Invalid Content-Length header `{trimmed}`"))?;
            if parsed > MAX_LSP_FRAME_BYTES {
                return Err(format!(
                    "LSP frame Content-Length {parsed} exceeds cap ({MAX_LSP_FRAME_BYTES} bytes)"
                ));
            }
            content_length = Some(parsed);
        }
    }

    let len =
        content_length.ok_or_else(|| "Missing Content-Length header in LSP frame".to_string())?;
    let mut body = vec![0u8; len];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|e| format!("LSP read body error: {e}"))?;

    let val: Value =
        serde_json::from_slice(&body).map_err(|e| format!("Invalid JSON in LSP frame: {e}"))?;
    Ok(Some(val))
}

/// Write a single `Content-Length` framed JSON-RPC 2.0 message to an async writer and flush.
pub async fn write_lsp_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    payload: &Value,
) -> Result<(), String> {
    let body = serde_json::to_vec(payload).map_err(|e| format!("LSP serialize error: {e}"))?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|e| format!("LSP write header error: {e}"))?;
    writer
        .write_all(&body)
        .await
        .map_err(|e| format!("LSP write body error: {e}"))?;
    writer
        .flush()
        .await
        .map_err(|e| format!("LSP flush error: {e}"))?;
    Ok(())
}

// ============================================================================
// Live LSP Client Session (Stdio or In-Memory Duplex for Tests)
// ============================================================================

struct SessionDiagnosticsCache {
    /// Keyed by normalized workspace-relative path (or clean absolute path if outside workspace).
    by_path: HashMap<String, Vec<LspDiagnostic>>,
}

impl SessionDiagnosticsCache {
    fn new() -> Self {
        Self {
            by_path: HashMap::new(),
        }
    }

    fn update_for_path(&mut self, path: String, mut diagnostics: Vec<LspDiagnostic>) {
        if diagnostics.is_empty() {
            self.by_path.remove(&path);
            return;
        }
        diagnostics.truncate(MAX_DIAGNOSTICS_PER_FILE);
        if !self.by_path.contains_key(&path)
            && self.by_path.len() >= MAX_DIAGNOSTIC_FILES
            && let Some(oldest_key) = self.by_path.keys().next().cloned()
        {
            self.by_path.remove(&oldest_key);
        }
        self.by_path.insert(path, diagnostics);
    }

    fn total_count(&self) -> u32 {
        let total: usize = self.by_path.values().map(Vec::len).sum();
        u32::try_from(total).unwrap_or(u32::MAX)
    }

    fn snapshot(&self, filter_path: Option<&str>) -> Vec<LspDiagnostic> {
        let mut out: Vec<LspDiagnostic> = match filter_path {
            Some(target) => self.by_path.get(target).cloned().unwrap_or_default(),
            None => self
                .by_path
                .values()
                .flat_map(|v| v.iter().cloned())
                .collect(),
        };
        out.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.col.cmp(&b.col))
        });
        out.truncate(MAX_TOTAL_DIAGNOSTICS);
        out
    }
}

struct OpenedDocumentState {
    version: i32,
    content_hash: String,
}

type PendingMap = HashMap<u64, oneshot::Sender<Result<Value, String>>>;

pub struct LspClientSession {
    config: LspServerConfig,
    canonical_cwd: PathBuf,
    binary_path: Option<String>,
    writer: Arc<AsyncMutex<Box<dyn AsyncWrite + Send + Unpin>>>,
    next_id: AtomicU64,
    pending: Arc<Mutex<PendingMap>>,
    diagnostics: Arc<Mutex<SessionDiagnosticsCache>>,
    opened_docs: AsyncMutex<HashMap<PathBuf, OpenedDocumentState>>,
    state: Arc<Mutex<(LspServerState, Option<String>)>>,
    alive: Arc<AtomicBool>,
    last_used_epoch_secs: AtomicU64,
    child: AsyncMutex<Option<Child>>,
    reader_task: Mutex<Option<JoinHandle<()>>>,
}

impl LspClientSession {
    /// Spawn a real stdio LSP language server process with scrubbed environment and complete `initialize` + `initialized`.
    async fn spawn_stdio(
        config: LspServerConfig,
        binary_path: PathBuf,
        canonical_cwd: PathBuf,
        init_timeout: Duration,
    ) -> Result<Arc<Self>, String> {
        let mut cmd = Command::new(&binary_path);
        cmd.args(&config.args)
            .current_dir(&canonical_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        // Strip sensitive Orbit credentials and code-injection loader env vars before exec.
        for (key, _) in std::env::vars_os() {
            if let Some(k_str) = key.to_str()
                && should_strip_lsp_env_var(k_str)
            {
                cmd.env_remove(&key);
            }
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to launch `{}`: {e}", config.name))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("Failed to capture stdin for `{}`", config.name))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("Failed to capture stdout for `{}`", config.name))?;

        let session = Self::from_transport(
            config,
            Some(binary_path.to_string_lossy().into_owned()),
            canonical_cwd,
            BufReader::new(stdout),
            Box::new(stdin),
            Some(child),
            init_timeout,
        )
        .await?;

        Ok(session)
    }

    /// Construct an LSP session over any async buffered reader + writer (used by both stdio & tests).
    pub async fn from_transport<R>(
        config: LspServerConfig,
        binary_path: Option<String>,
        canonical_cwd: PathBuf,
        mut reader: R,
        writer: Box<dyn AsyncWrite + Send + Unpin>,
        child: Option<Child>,
        init_timeout: Duration,
    ) -> Result<Arc<Self>, String>
    where
        R: AsyncBufRead + Send + Unpin + 'static,
    {
        let writer_arc: Arc<AsyncMutex<Box<dyn AsyncWrite + Send + Unpin>>> =
            Arc::new(AsyncMutex::new(writer));
        let pending: Arc<Mutex<PendingMap>> = Arc::new(Mutex::new(HashMap::new()));
        let diagnostics = Arc::new(Mutex::new(SessionDiagnosticsCache::new()));
        let state = Arc::new(Mutex::new((LspServerState::Starting, None)));
        let alive = Arc::new(AtomicBool::new(true));

        let session = Arc::new(Self {
            config,
            canonical_cwd: canonical_cwd.clone(),
            binary_path,
            writer: Arc::clone(&writer_arc),
            next_id: AtomicU64::new(1),
            pending: Arc::clone(&pending),
            diagnostics: Arc::clone(&diagnostics),
            opened_docs: AsyncMutex::new(HashMap::new()),
            state: Arc::clone(&state),
            alive: Arc::clone(&alive),
            last_used_epoch_secs: AtomicU64::new(now_epoch_secs()),
            child: AsyncMutex::new(child),
            reader_task: Mutex::new(None),
        });

        let pending_bg = Arc::clone(&pending);
        let diags_bg = Arc::clone(&diagnostics);
        let state_bg = Arc::clone(&state);
        let alive_bg = Arc::clone(&alive);
        let writer_bg = Arc::clone(&writer_arc);
        let cwd_bg = canonical_cwd.clone();
        let server_name = session.config.name.clone();

        let handle = tokio::spawn(async move {
            loop {
                match read_lsp_message(&mut reader).await {
                    Ok(Some(msg)) => {
                        handle_incoming_lsp_message(
                            &msg,
                            &cwd_bg,
                            &pending_bg,
                            &diags_bg,
                            &writer_bg,
                        );
                    }
                    Ok(None) => {
                        debug!(server = %server_name, "LSP server stdout closed (EOF)");
                        break;
                    }
                    Err(err) => {
                        warn!(server = %server_name, error = %err, "LSP reader loop terminated");
                        if let Ok(mut st) = state_bg.lock() {
                            *st = (LspServerState::Error, Some(err));
                        }
                        break;
                    }
                }
            }

            alive_bg.store(false, Ordering::SeqCst);
            if let Ok(mut st) = state_bg.lock()
                && st.0 != LspServerState::Error
            {
                *st = (LspServerState::Stopped, None);
            }
            if let Ok(mut map) = pending_bg.lock() {
                for (_, tx) in map.drain() {
                    let _ = tx.send(Err("LSP server disconnected".into()));
                }
            }
        });

        if let Ok(mut slot) = session.reader_task.lock() {
            *slot = Some(handle);
        }

        // Perform LSP initialize handshake
        let root_uri = path_to_file_uri(&canonical_cwd);
        let folder_name = canonical_cwd
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("workspace")
            .to_string();

        let init_params = json!({
            "processId": std::process::id(),
            "clientInfo": {
                "name": "orbit-daemon",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "rootPath": canonical_cwd.to_string_lossy(),
            "rootUri": root_uri,
            "workspaceFolders": [{
                "uri": root_uri,
                "name": folder_name,
            }],
            "capabilities": {
                "workspace": {
                    "symbol": {
                        "dynamicRegistration": false
                    },
                    "workspaceFolders": true
                },
                "textDocument": {
                    "synchronization": {
                        "dynamicRegistration": false,
                        "didSave": true
                    },
                    "definition": {
                        "dynamicRegistration": false,
                        "linkSupport": true
                    },
                    "documentSymbol": {
                        "dynamicRegistration": false,
                        "hierarchicalDocumentSymbolSupport": true
                    },
                    "publishDiagnostics": {
                        "relatedInformation": false
                    }
                }
            }
        });

        if let Err(err) = session
            .send_request("initialize", init_params, init_timeout)
            .await
        {
            if let Ok(mut st) = session.state.lock() {
                *st = (LspServerState::Error, Some(err.clone()));
            }
            session.shutdown().await;
            return Err(format!(
                "LSP `{}` initialize failed: {err}",
                session.config.name
            ));
        }

        session.send_notification("initialized", json!({})).await?;

        if let Ok(mut st) = session.state.lock() {
            *st = (LspServerState::Ready, None);
        }
        session.touch();

        Ok(session)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn touch(&self) {
        self.last_used_epoch_secs
            .store(now_epoch_secs(), Ordering::Relaxed);
    }

    pub fn last_used_epoch_secs(&self) -> u64 {
        self.last_used_epoch_secs.load(Ordering::Relaxed)
    }

    pub fn idle_secs(&self) -> u64 {
        let last = self.last_used_epoch_secs.load(Ordering::Relaxed);
        now_epoch_secs().saturating_sub(last)
    }

    #[cfg(test)]
    fn set_last_used_for_test(&self, epoch_secs: u64) {
        self.last_used_epoch_secs
            .store(epoch_secs, Ordering::Relaxed);
    }

    pub fn status(&self) -> LspServerStatus {
        let (st, err) = self
            .state
            .lock()
            .map(|g| g.clone())
            .unwrap_or((LspServerState::Error, Some("lock poisoned".into())));
        let diagnostics_count = self
            .diagnostics
            .lock()
            .map(|d| d.total_count())
            .unwrap_or(0);
        LspServerStatus {
            id: self.config.id.clone(),
            name: self.config.name.clone(),
            state: st,
            workspace_cwd: self.canonical_cwd.to_string_lossy().into_owned(),
            binary_path: self.binary_path.clone(),
            diagnostics_count,
            error: err,
            language_ids: self.config.language_ids.clone(),
            file_extensions: self.config.file_extensions.clone(),
            source: self.config.source,
        }
    }

    pub fn cached_diagnostics(&self, filter_rel_path: Option<&str>) -> Vec<LspDiagnostic> {
        self.diagnostics
            .lock()
            .map(|d| d.snapshot(filter_rel_path))
            .unwrap_or_default()
    }

    pub async fn send_request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        if !self.is_alive() {
            return Err(format!("LSP server `{}` is not running", self.config.name));
        }
        self.touch();

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self
                .pending
                .lock()
                .map_err(|_| "LSP pending lock poisoned".to_string())?;
            guard.insert(id, tx);
        }

        let payload = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let write_res = {
            let mut w = self.writer.lock().await;
            write_lsp_message(&mut *w, &payload).await
        };
        if let Err(err) = write_res {
            if let Ok(mut guard) = self.pending.lock() {
                guard.remove(&id);
            }
            return Err(err);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => {
                self.touch();
                result
            }
            Ok(Err(_)) => Err(format!(
                "LSP server `{}` closed before replying to `{method}`",
                self.config.name
            )),
            Err(_) => {
                if let Ok(mut guard) = self.pending.lock() {
                    guard.remove(&id);
                }
                Err(format!(
                    "LSP request `{method}` to `{}` timed out after {}s",
                    self.config.name,
                    timeout.as_secs()
                ))
            }
        }
    }

    pub async fn send_notification(&self, method: &str, params: Value) -> Result<(), String> {
        if !self.is_alive() {
            return Err(format!("LSP server `{}` is not running", self.config.name));
        }
        self.touch();
        let payload = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let mut w = self.writer.lock().await;
        write_lsp_message(&mut *w, &payload).await
    }

    /// Ensure a file is synchronized with the LSP server via `didOpen` or `didChange`.
    /// Holds `opened_docs` across `send_notification` (strict lock order `opened_docs -> writer`)
    /// and mutates document state ONLY after write succeeds.
    /// Returns `(changed, synced_text)`.
    pub async fn ensure_document_synced(
        &self,
        canonical_file: &Path,
        override_text: Option<&str>,
    ) -> Result<(bool, String), String> {
        let text = match override_text {
            Some(t) => {
                if t.len() > MAX_LSP_DOC_BYTES {
                    return Err("File exceeds 2 MiB LSP synchronization limit".into());
                }
                t.to_string()
            }
            None => {
                let path_owned = canonical_file.to_path_buf();
                tokio::task::spawn_blocking(move || {
                    let meta = std::fs::metadata(&path_owned)
                        .map_err(|e| format!("Failed reading file metadata: {e}"))?;
                    if !meta.is_file() {
                        return Err("Target path is not a regular file".into());
                    }
                    if meta.len() as usize > MAX_LSP_DOC_BYTES {
                        return Err("File exceeds 2 MiB LSP synchronization limit".into());
                    }
                    std::fs::read_to_string(&path_owned)
                        .map_err(|e| format!("Failed reading UTF-8 file: {e}"))
                })
                .await
                .map_err(|e| format!("File read task failed: {e}"))??
            }
        };

        let hash = sha256_hex(text.as_bytes());
        let uri = path_to_file_uri(canonical_file);
        let mut docs = self.opened_docs.lock().await;

        if let Some(existing) = docs.get_mut(canonical_file) {
            if existing.content_hash == hash {
                return Ok((false, text));
            }
            let next_ver = existing.version.saturating_add(1);
            self.send_notification(
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": next_ver,
                    },
                    "contentChanges": [{ "text": text }],
                }),
            )
            .await?;
            existing.version = next_ver;
            existing.content_hash = hash;
            return Ok((true, text));
        }

        let lang_id = language_id_for_path(canonical_file, &self.config);
        self.send_notification(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": lang_id,
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await?;
        docs.insert(
            canonical_file.to_path_buf(),
            OpenedDocumentState {
                version: 1,
                content_hash: hash,
            },
        );
        Ok((true, text))
    }

    pub async fn notify_did_save(&self, canonical_file: &Path, text: &str) {
        if self
            .ensure_document_synced(canonical_file, Some(text))
            .await
            .is_ok()
        {
            let uri = path_to_file_uri(canonical_file);
            let _ = self
                .send_notification(
                    "textDocument/didSave",
                    json!({
                        "textDocument": { "uri": uri },
                        "text": text,
                    }),
                )
                .await;
        }
    }

    /// Query `workspace/symbol` (when `canonical_file` is `None`) or `textDocument/documentSymbol`.
    pub async fn query_symbols(
        &self,
        query: &str,
        canonical_file: Option<(&Path, &str)>,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<LspSymbolInfo>, String> {
        let cap = limit.clamp(1, MAX_SYMBOLS_LIMIT);
        let query_lower = query.trim().to_ascii_lowercase();

        if let Some((file_path, rel_path)) = canonical_file {
            let _ = self.ensure_document_synced(file_path, None).await?;
            let uri = path_to_file_uri(file_path);
            let raw = self
                .send_request(
                    "textDocument/documentSymbol",
                    json!({ "textDocument": { "uri": uri } }),
                    timeout,
                )
                .await?;
            let mut out = Vec::new();
            parse_document_symbols(&raw, rel_path, &self.canonical_cwd, None, &mut out);
            if !query_lower.is_empty() {
                out.retain(|s| s.name.to_ascii_lowercase().contains(&query_lower));
            }
            out.truncate(cap);
            return Ok(out);
        }

        let raw = self
            .send_request(
                "workspace/symbol",
                json!({ "query": query.trim() }),
                timeout,
            )
            .await?;
        let mut out = parse_workspace_symbols(&raw, &self.canonical_cwd);
        if !query_lower.is_empty() {
            out.retain(|s| {
                s.name.to_ascii_lowercase().contains(&query_lower)
                    || s.container_name
                        .as_deref()
                        .is_some_and(|c| c.to_ascii_lowercase().contains(&query_lower))
            });
        }
        out.truncate(cap);
        Ok(out)
    }

    /// Resolve `textDocument/definition` at a 1-based `(line, utf16_col)` coordinate.
    pub async fn goto_definition(
        &self,
        canonical_file: &Path,
        line_1based: u32,
        col_1based: u32,
        timeout: Duration,
    ) -> Result<Vec<LspLocation>, String> {
        let (_, synced_text) = self.ensure_document_synced(canonical_file, None).await?;
        let uri = path_to_file_uri(canonical_file);
        let (lsp_line, lsp_char) = clamp_utf16_col_for_line(&synced_text, line_1based, col_1based);

        let raw = self
            .send_request(
                "textDocument/definition",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                }),
                timeout,
            )
            .await?;

        let cwd_for_parse = self.canonical_cwd.clone();
        let mut locs =
            tokio::task::spawn_blocking(move || parse_definition_locations(&raw, &cwd_for_parse))
                .await
                .map_err(|e| format!("LSP definition parse task failed: {e}"))?;
        locs.truncate(MAX_LOCATIONS_LIMIT);
        Ok(locs)
    }

    /// Gracefully shut down the LSP server (`shutdown` + await reply up to 300ms -> `exit` -> kill child).
    pub async fn shutdown(&self) {
        if self.alive.swap(false, Ordering::SeqCst) {
            let shutdown_id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            if let Ok(mut guard) = self.pending.lock() {
                guard.insert(shutdown_id, tx);
            }

            let shutdown_payload = json!({
                "jsonrpc": "2.0",
                "id": shutdown_id,
                "method": "shutdown",
                "params": Value::Null,
            });
            let exit_payload = json!({
                "jsonrpc": "2.0",
                "method": "exit",
                "params": Value::Null,
            });

            let wrote_shutdown = if let Ok(mut w) =
                tokio::time::timeout(Duration::from_millis(500), self.writer.lock()).await
            {
                tokio::time::timeout(
                    Duration::from_millis(500),
                    write_lsp_message(&mut *w, &shutdown_payload),
                )
                .await
                .is_ok_and(|r| r.is_ok())
            } else {
                false
            };

            if wrote_shutdown {
                let _ = tokio::time::timeout(Duration::from_millis(300), rx).await;
            }
            if let Ok(mut guard) = self.pending.lock() {
                guard.remove(&shutdown_id);
            }

            if let Ok(mut w) =
                tokio::time::timeout(Duration::from_millis(250), self.writer.lock()).await
            {
                let _ = tokio::time::timeout(
                    Duration::from_millis(250),
                    write_lsp_message(&mut *w, &exit_payload),
                )
                .await;
            }
        }

        if let Ok(mut st) = self.state.lock() {
            *st = (LspServerState::Stopped, None);
        }
        if let Ok(mut map) = self.pending.lock() {
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("LSP server shut down".into()));
            }
        }
        if let Some(mut child) = self.child.lock().await.take() {
            let _ = tokio::time::timeout(Duration::from_millis(400), child.wait()).await;
            let _ = child.kill().await;
        }
        if let Ok(mut slot) = self.reader_task.lock()
            && let Some(handle) = slot.take()
        {
            handle.abort();
        }
    }
}

impl Drop for LspClientSession {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut slot) = self.reader_task.lock()
            && let Some(handle) = slot.take()
        {
            handle.abort();
        }
    }
}

/// Non-blocking incoming LSP dispatcher called on the stdout reader task.
/// Never awaits `writer.lock()` inline (spawns bounded background reply tasks for server requests)
/// and performs zero blocking filesystem I/O.
fn handle_incoming_lsp_message(
    msg: &Value,
    canonical_cwd: &Path,
    pending: &Arc<Mutex<PendingMap>>,
    diagnostics: &Arc<Mutex<SessionDiagnosticsCache>>,
    writer: &Arc<AsyncMutex<Box<dyn AsyncWrite + Send + Unpin>>>,
) {
    let has_method = msg.get("method").and_then(Value::as_str);
    let has_id = msg.get("id");

    // 1. Response to a client request
    if has_method.is_none()
        && let Some(id_val) = has_id
        && let Some(id) = id_val.as_u64()
    {
        let tx = pending.lock().ok().and_then(|mut m| m.remove(&id));
        if let Some(tx) = tx {
            if let Some(err) = msg.get("error") {
                let err_msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("LSP error")
                    .to_string();
                let _ = tx.send(Err(err_msg));
            } else {
                let result = msg.get("result").cloned().unwrap_or(Value::Null);
                let _ = tx.send(Ok(result));
            }
        }
        return;
    }

    // 2. Incoming notification or server-to-client request
    if let Some(method) = has_method {
        if let Some(id_val) = has_id {
            // Server-to-client request: spawn reply with bounded timeout so stdout reader never deadlocks
            let result = match method {
                "workspace/configuration" => {
                    let count = msg
                        .get("params")
                        .and_then(|p| p.get("items"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Value::Array(vec![Value::Null; count])
                }
                _ => Value::Null,
            };
            let reply = json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": result,
            });
            let writer_clone = Arc::clone(writer);
            tokio::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(2), async move {
                    let mut w = writer_clone.lock().await;
                    let _ = write_lsp_message(&mut *w, &reply).await;
                })
                .await;
            });
            return;
        }

        if method == "textDocument/publishDiagnostics"
            && let Some(params) = msg.get("params")
            && let Some(uri) = params.get("uri").and_then(Value::as_str)
            && let Some(path) = normalize_uri_to_display_path_lexical(uri, canonical_cwd)
        {
            let mut parsed_diags = Vec::new();
            if let Some(items) = params.get("diagnostics").and_then(Value::as_array) {
                for item in items.iter().take(MAX_DIAGNOSTICS_PER_FILE) {
                    if let Some(d) = parse_single_diagnostic(item, &path) {
                        parsed_diags.push(d);
                    }
                }
            }
            if let Ok(mut cache) = diagnostics.lock() {
                cache.update_for_path(path, parsed_diags);
            }
        }
    }
}

/// Pure lexical URI-to-workspace-relative display path normalizer (zero blocking FS calls).
fn normalize_uri_to_display_path_lexical(uri: &str, canonical_cwd: &Path) -> Option<String> {
    let raw_path = file_uri_to_path(uri)?;
    if let Ok(rel) = raw_path.strip_prefix(canonical_cwd) {
        return Some(path_to_unix_string(rel));
    }
    // On macOS, servers may emit `/var/...` when `canonical_cwd` is `/private/var/...`
    if let Ok(without_private) = canonical_cwd.strip_prefix("/private") {
        let alt_cwd = Path::new("/").join(without_private);
        if let Ok(rel) = raw_path.strip_prefix(&alt_cwd) {
            return Some(path_to_unix_string(rel));
        }
    }
    Some(raw_path.to_string_lossy().into_owned())
}

/// Blocking-thread URI-to-display-path normalizer with `fs::canonicalize` fallback.
fn normalize_uri_to_display_path_blocking(uri: &str, canonical_cwd: &Path) -> Option<String> {
    let raw_path = file_uri_to_path(uri)?;
    let resolved = std::fs::canonicalize(&raw_path).unwrap_or(raw_path);
    if let Ok(rel) = resolved.strip_prefix(canonical_cwd) {
        return Some(path_to_unix_string(rel));
    }
    Some(resolved.to_string_lossy().into_owned())
}

fn parse_single_diagnostic(item: &Value, path: &str) -> Option<LspDiagnostic> {
    let message = item
        .get("message")
        .and_then(Value::as_str)?
        .trim()
        .to_string();
    if message.is_empty() {
        return None;
    }
    let range = item.get("range")?;
    let start = range.get("start")?;
    let end = range.get("end").unwrap_or(start);

    let line = start
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    let col = start
        .get("character")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    let end_line = end
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    let end_col = end
        .get("character")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);

    let severity = match item.get("severity").and_then(Value::as_u64).unwrap_or(1) {
        1 => LspDiagnosticSeverity::Error,
        2 => LspDiagnosticSeverity::Warning,
        3 => LspDiagnosticSeverity::Info,
        _ => LspDiagnosticSeverity::Hint,
    };

    let source = item
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let code = match item.get("code") {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    };

    Some(LspDiagnostic {
        path: path.to_string(),
        line,
        col,
        end_line,
        end_col,
        severity,
        message,
        source,
        code,
    })
}

fn lsp_symbol_kind_label(code: u64) -> &'static str {
    match code {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_parameter",
        _ => "symbol",
    }
}

fn extract_range_1based(range: &Value) -> (u32, u32, u32) {
    let start = range.get("start");
    let end = range.get("end").or(start);
    let line = start
        .and_then(|s| s.get("line"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    let col = start
        .and_then(|s| s.get("character"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    let end_line = end
        .and_then(|s| s.get("line"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    (line, col, end_line.max(line))
}

fn parse_document_symbols(
    val: &Value,
    fallback_rel_path: &str,
    canonical_cwd: &Path,
    container: Option<&str>,
    out: &mut Vec<LspSymbolInfo>,
) {
    let Some(items) = val.as_array() else {
        return;
    };
    for item in items {
        if out.len() >= MAX_SYMBOLS_LIMIT * 2 {
            return;
        }
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let kind = lsp_symbol_kind_label(item.get("kind").and_then(Value::as_u64).unwrap_or(0))
            .to_string();

        if let Some(location) = item.get("location") {
            let path = location
                .get("uri")
                .and_then(Value::as_str)
                .and_then(|u| normalize_uri_to_display_path_lexical(u, canonical_cwd))
                .unwrap_or_else(|| fallback_rel_path.to_string());
            let (line, col, end_line) = location
                .get("range")
                .map(extract_range_1based)
                .unwrap_or((1, 1, 1));
            let container_name = item
                .get("containerName")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| container.map(ToOwned::to_owned));
            out.push(LspSymbolInfo {
                name: name.to_string(),
                kind,
                container_name,
                path,
                line,
                col,
                end_line,
            });
        } else if let Some(range) = item.get("selectionRange").or_else(|| item.get("range")) {
            let (line, col, _) = extract_range_1based(range);
            let (_, _, end_line) = item
                .get("range")
                .map(extract_range_1based)
                .unwrap_or((line, col, line));
            out.push(LspSymbolInfo {
                name: name.to_string(),
                kind,
                container_name: container.map(ToOwned::to_owned),
                path: fallback_rel_path.to_string(),
                line,
                col,
                end_line,
            });
            if let Some(children) = item.get("children") {
                parse_document_symbols(children, fallback_rel_path, canonical_cwd, Some(name), out);
            }
        }
    }
}

fn parse_workspace_symbols(val: &Value, canonical_cwd: &Path) -> Vec<LspSymbolInfo> {
    let mut out = Vec::new();
    let Some(items) = val.as_array() else {
        return out;
    };
    for item in items.iter().take(MAX_SYMBOLS_LIMIT * 2) {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some(location) = item.get("location") else {
            continue;
        };
        let Some(uri) = location.get("uri").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = normalize_uri_to_display_path_lexical(uri, canonical_cwd) else {
            continue;
        };
        let kind = lsp_symbol_kind_label(item.get("kind").and_then(Value::as_u64).unwrap_or(0))
            .to_string();
        let (line, col, end_line) = location
            .get("range")
            .map(extract_range_1based)
            .unwrap_or((1, 1, 1));
        let container_name = item
            .get("containerName")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        out.push(LspSymbolInfo {
            name: name.to_string(),
            kind,
            container_name,
            path,
            line,
            col,
            end_line,
        });
    }
    out
}

fn parse_definition_locations(val: &Value, canonical_cwd: &Path) -> Vec<LspLocation> {
    let mut out = Vec::new();
    match val {
        Value::Null => {}
        Value::Object(_) => {
            if let Some(loc) = parse_single_location(val, canonical_cwd) {
                out.push(loc);
            }
        }
        Value::Array(items) => {
            for item in items.iter().take(MAX_LOCATIONS_LIMIT) {
                if let Some(loc) = parse_single_location(item, canonical_cwd) {
                    out.push(loc);
                }
            }
        }
        _ => {}
    }
    out
}

fn parse_single_location(item: &Value, canonical_cwd: &Path) -> Option<LspLocation> {
    let uri = item
        .get("uri")
        .or_else(|| item.get("targetUri"))
        .and_then(Value::as_str)?;
    let range = item
        .get("range")
        .or_else(|| item.get("targetSelectionRange"))
        .or_else(|| item.get("targetRange"))?;
    let (line, col, end_line) = extract_range_1based(range);
    let path = normalize_uri_to_display_path_blocking(uri, canonical_cwd)?;
    let preview = read_line_preview(uri, canonical_cwd, line);
    Some(LspLocation {
        path,
        line,
        col,
        end_line,
        preview,
    })
}

/// Safely read a single-line preview for a workspace-contained regular text file.
/// Blocks `.git`, symlinks escaping workspace, FIFOs/devices/dirs (`!meta.is_file()`), and huge files.
pub fn read_line_preview(uri: &str, canonical_cwd: &Path, line_1based: u32) -> Option<String> {
    let file_path = file_uri_to_path(uri)?;
    let canon = std::fs::canonicalize(&file_path).ok()?;
    if !canon.starts_with(canonical_cwd) {
        return None;
    }
    if is_path_inside_git(canonical_cwd, &canon) {
        return None;
    }
    let meta = std::fs::metadata(&canon).ok()?;
    if !meta.is_file() || meta.len() as usize > MAX_LSP_DOC_BYTES {
        return None;
    }
    let content = std::fs::read_to_string(&canon).ok()?;
    let target_idx = usize::try_from(line_1based.saturating_sub(1)).ok()?;
    content
        .lines()
        .nth(target_idx)
        .map(|l| l.trim().chars().take(160).collect::<String>())
        .filter(|s| !s.is_empty())
}

// ============================================================================
// LspManager: Multi-Workspace Lazy Lifecycle + Bounded Cap + Idle Auto-Shutdown
// ============================================================================

type SessionKey = (PathBuf, String);
type SessionStartLockMap = HashMap<SessionKey, Arc<AsyncMutex<()>>>;

pub struct LspManager {
    home_dir: PathBuf,
    idle_timeout: Duration,
    request_timeout: Duration,
    init_timeout: Duration,
    sessions: AsyncMutex<HashMap<SessionKey, Arc<LspClientSession>>>,
    start_locks: AsyncMutex<SessionStartLockMap>,
}

impl LspManager {
    pub fn new(home_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            home_dir,
            idle_timeout: DEFAULT_IDLE_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
            init_timeout: DEFAULT_INIT_TIMEOUT,
            sessions: AsyncMutex::new(HashMap::new()),
            start_locks: AsyncMutex::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    fn with_timeouts(
        home_dir: PathBuf,
        idle_timeout: Duration,
        request_timeout: Duration,
        init_timeout: Duration,
    ) -> Arc<Self> {
        Arc::new(Self {
            home_dir,
            idle_timeout,
            request_timeout,
            init_timeout,
            sessions: AsyncMutex::new(HashMap::new()),
            start_locks: AsyncMutex::new(HashMap::new()),
        })
    }

    /// Spawn the background idle reaper task (checks every 60s and gracefully shuts down idle LSPs).
    pub fn spawn_idle_reaper(self: &Arc<Self>) -> JoinHandle<()> {
        let mgr = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                interval.tick().await;
                let evicted = mgr.reap_idle_sessions().await;
                if evicted > 0 {
                    debug!(evicted, "Reaped idle LSP language server sessions");
                }
            }
        })
    }

    /// Evict and shut down any sessions that are dead or idle past `idle_timeout`,
    /// and prune uncontended entries from `start_locks`.
    pub async fn reap_idle_sessions(&self) -> usize {
        let max_idle = self.idle_timeout.as_secs();
        let (to_shutdown, active_keys): (Vec<Arc<LspClientSession>>, HashSet<SessionKey>) = {
            let mut guard = self.sessions.lock().await;
            let mut evict_keys = Vec::new();
            for (k, sess) in guard.iter() {
                if !sess.is_alive() || sess.idle_secs() >= max_idle {
                    evict_keys.push(k.clone());
                }
            }
            let evicted = evict_keys
                .into_iter()
                .filter_map(|k| guard.remove(&k))
                .collect();
            let remaining: HashSet<SessionKey> = guard.keys().cloned().collect();
            (evicted, remaining)
        };

        {
            let mut locks = self.start_locks.lock().await;
            locks.retain(|k, v| active_keys.contains(k) || Arc::strong_count(v) > 1);
        }

        let count = to_shutdown.len();
        for sess in to_shutdown {
            sess.shutdown().await;
        }
        count
    }

    async fn acquire_start_lock(&self, key: &SessionKey) -> Arc<AsyncMutex<()>> {
        let mut locks = self.start_locks.lock().await;
        Arc::clone(
            locks
                .entry(key.clone())
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// Insert a session into `sessions`, enforcing `MAX_ACTIVE_LSP_SESSIONS` by evicting and
    /// shutting down the most-idle session if at capacity.
    async fn insert_session_bounded(&self, key: SessionKey, session: Arc<LspClientSession>) {
        let evicted = {
            let mut guard = self.sessions.lock().await;
            let mut to_evict: Option<Arc<LspClientSession>> = None;
            if !guard.contains_key(&key) && guard.len() >= MAX_ACTIVE_LSP_SESSIONS {
                let victim_key = guard
                    .iter()
                    .min_by_key(|(_, s)| {
                        let alive_rank: u8 = if s.is_alive() { 1 } else { 0 };
                        (alive_rank, s.last_used_epoch_secs())
                    })
                    .map(|(k, _)| k.clone());
                if let Some(vk) = victim_key {
                    to_evict = guard.remove(&vk);
                }
            }
            let replaced = guard.insert(key, session);
            (to_evict, replaced)
        };

        if let Some(old) = evicted.0 {
            old.shutdown().await;
        }
        if let Some(old) = evicted.1 {
            old.shutdown().await;
        }
    }

    async fn spawn_and_register_locked(
        &self,
        key: SessionKey,
        canonical_cwd: &Path,
        config: LspServerConfig,
    ) -> Result<Arc<LspClientSession>, String> {
        let cfg_clone = config.clone();
        let binary_path = tokio::task::spawn_blocking(move || resolve_config_binary(&cfg_clone))
            .await
            .map_err(|e| format!("LSP binary probe failed: {e}"))?
            .ok_or_else(|| {
                format!(
                    "LSP server `{}` (`{}`) is not installed on this host",
                    config.name, config.command
                )
            })?;

        let session = LspClientSession::spawn_stdio(
            config,
            binary_path,
            canonical_cwd.to_path_buf(),
            self.init_timeout,
        )
        .await?;

        self.insert_session_bounded(key, Arc::clone(&session)).await;
        Ok(session)
    }

    /// List discovered and active LSP servers for `cwd` WITHOUT spawning any server.
    pub async fn list_servers(&self, cwd: &Path) -> Result<Vec<LspServerStatus>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let home_dir = self.home_dir.clone();
        let cwd_for_scan = canonical_cwd.clone();

        let discovered = tokio::task::spawn_blocking(move || {
            let configs = load_effective_configs(&home_dir, &cwd_for_scan);
            configs
                .into_iter()
                .map(|cfg| {
                    let bin = resolve_config_binary(&cfg);
                    let relevant = cfg.source != LspConfigSource::Builtin
                        || workspace_matches_root_markers(&cwd_for_scan, &cfg.root_markers);
                    (cfg, bin, relevant)
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| format!("LSP discovery task failed: {e}"))?;

        let active_sessions: HashMap<String, Arc<LspClientSession>> = {
            let guard = self.sessions.lock().await;
            guard
                .iter()
                .filter(|((c, _), _)| c == &canonical_cwd)
                .map(|((_, id), s)| (id.clone(), Arc::clone(s)))
                .collect()
        };

        let cwd_str = canonical_cwd.to_string_lossy().into_owned();
        let mut results: Vec<(u8, LspServerStatus)> = Vec::new();

        for (cfg, bin, relevant) in discovered {
            if let Some(sess) = active_sessions.get(&cfg.id)
                && sess.is_alive()
            {
                results.push((0, sess.status()));
                continue;
            }

            if !relevant && bin.is_none() {
                continue;
            }

            let state = if bin.is_some() {
                LspServerState::Stopped
            } else {
                LspServerState::Unavailable
            };
            let priority = match (relevant, bin.is_some()) {
                (true, true) => 1,
                (true, false) => 2,
                (false, true) => 3,
                (false, false) => 4,
            };
            results.push((
                priority,
                LspServerStatus {
                    id: cfg.id,
                    name: cfg.name,
                    state,
                    workspace_cwd: cwd_str.clone(),
                    binary_path: bin.map(|p| p.to_string_lossy().into_owned()),
                    diagnostics_count: 0,
                    error: if state == LspServerState::Unavailable {
                        Some(format!("`{}` not found in PATH", cfg.command))
                    } else {
                        None
                    },
                    language_ids: cfg.language_ids,
                    file_extensions: cfg.file_extensions,
                    source: cfg.source,
                },
            ));
        }

        results.sort_by(|(pa, a), (pb, b)| pa.cmp(pb).then_with(|| a.name.cmp(&b.name)));
        Ok(results.into_iter().map(|(_, s)| s).collect())
    }

    /// Restart (or start fresh) a specific LSP server for `cwd` and return updated statuses.
    /// Serialized with `start_locks` so concurrent restarts and lazy starts never race or orphan children.
    pub async fn restart_server(
        &self,
        cwd: &Path,
        server_id: &str,
    ) -> Result<Vec<LspServerStatus>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let clean_id = server_id.trim();
        if clean_id.is_empty() {
            return Err("`server_id` cannot be empty".into());
        }

        let key = (canonical_cwd.clone(), clean_id.to_string());
        let key_lock = self.acquire_start_lock(&key).await;
        let _start_guard = key_lock.lock().await;

        let old = self.sessions.lock().await.remove(&key);
        if let Some(sess) = old {
            sess.shutdown().await;
        }

        let home_dir = self.home_dir.clone();
        let cwd_scan = canonical_cwd.clone();
        let id_owned = clean_id.to_string();
        let target_cfg = tokio::task::spawn_blocking(move || {
            load_effective_configs(&home_dir, &cwd_scan)
                .into_iter()
                .find(|c| c.id == id_owned)
        })
        .await
        .map_err(|e| format!("LSP config task failed: {e}"))?
        .ok_or_else(|| format!("LSP server `{clean_id}` is not configured for this workspace"))?;

        let _ = self
            .spawn_and_register_locked(key, &canonical_cwd, target_cfg)
            .await?;
        drop(_start_guard);

        self.list_servers(&canonical_cwd).await
    }

    /// Query symbols across a workspace (`path = None`) or within a single file (`path = Some(...)`).
    pub async fn query_symbols(
        &self,
        cwd: &Path,
        query: &str,
        path: Option<&str>,
        limit: u32,
    ) -> Result<Vec<LspSymbolInfo>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let cap = usize::try_from(limit)
            .unwrap_or(50)
            .clamp(1, MAX_SYMBOLS_LIMIT);

        if let Some(raw_path) = path.map(str::trim).filter(|s| !s.is_empty()) {
            let (canon_file, rel_path) = resolve_workspace_file(&canonical_cwd, raw_path)?;
            let Some(cfg) = self
                .select_config_for_file(&canonical_cwd, &canon_file)
                .await?
            else {
                return Ok(Vec::new());
            };
            let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
            return sess
                .query_symbols(
                    query,
                    Some((&canon_file, &rel_path)),
                    cap,
                    self.request_timeout,
                )
                .await;
        }

        let configs = self.select_workspace_configs(&canonical_cwd).await?;
        let mut all_symbols: Vec<LspSymbolInfo> = Vec::new();
        for cfg in configs {
            if all_symbols.len() >= cap {
                break;
            }
            if let Ok(sess) = self.get_or_start_session(&canonical_cwd, cfg).await
                && let Ok(mut syms) = sess
                    .query_symbols(query, None, cap - all_symbols.len(), self.request_timeout)
                    .await
            {
                all_symbols.append(&mut syms);
            }
        }
        all_symbols.truncate(cap);
        Ok(all_symbols)
    }

    /// Fetch cached LSP diagnostics for a file or entire workspace.
    pub async fn get_diagnostics(
        &self,
        cwd: &Path,
        path: Option<&str>,
    ) -> Result<Vec<LspDiagnostic>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;

        if let Some(raw_path) = path.map(str::trim).filter(|s| !s.is_empty()) {
            let (canon_file, rel_path) = resolve_workspace_file(&canonical_cwd, raw_path)?;
            let Some(cfg) = self
                .select_config_for_file(&canonical_cwd, &canon_file)
                .await?
            else {
                return Ok(Vec::new());
            };
            let sess = match self.get_or_start_session(&canonical_cwd, cfg).await {
                Ok(s) => s,
                Err(err) => {
                    debug!(
                        error = %err,
                        path = %rel_path,
                        "Skipping LSP diagnostics because language server failed to start"
                    );
                    return Ok(Vec::new());
                }
            };
            let (synced, _) = sess.ensure_document_synced(&canon_file, None).await?;
            if synced && sess.cached_diagnostics(Some(&rel_path)).is_empty() {
                tokio::time::sleep(FRESH_OPEN_DIAGNOSTICS_SETTLE).await;
            }
            return Ok(sess.cached_diagnostics(Some(&rel_path)));
        }

        let running: Vec<Arc<LspClientSession>> = {
            let guard = self.sessions.lock().await;
            guard
                .iter()
                .filter(|((c, _), s)| c == &canonical_cwd && s.is_alive())
                .map(|(_, s)| Arc::clone(s))
                .collect()
        };

        let mut all = Vec::new();
        for sess in running {
            all.extend(sess.cached_diagnostics(None));
        }
        all.sort_by(|a, b| {
            a.severity
                .cmp(&b.severity)
                .then_with(|| a.path.cmp(&b.path))
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.col.cmp(&b.col))
        });
        all.truncate(MAX_TOTAL_DIAGNOSTICS);
        Ok(all)
    }

    /// Resolve Go-to-Definition locations for a 1-based `(line, utf16_col)` in `path`.
    pub async fn goto_definition(
        &self,
        cwd: &Path,
        path: &str,
        line: u32,
        col: u32,
    ) -> Result<Vec<LspLocation>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, _) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Ok(Vec::new());
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        sess.goto_definition(&canon_file, line, col, self.request_timeout)
            .await
    }

    /// Notify any already-running matching LSP server that a file was saved in the editor.
    /// Does NOT spawn a server if none is running.
    pub async fn notify_file_saved(&self, cwd: &Path, rel_or_abs_path: &str, content: &str) {
        let Ok(canonical_cwd) = validate_workspace_cwd(cwd) else {
            return;
        };
        let Ok((canon_file, _)) = resolve_workspace_file(&canonical_cwd, rel_or_abs_path) else {
            return;
        };
        let ext = canon_file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext.is_empty() {
            return;
        }

        let matching: Vec<Arc<LspClientSession>> = {
            let guard = self.sessions.lock().await;
            guard
                .iter()
                .filter(|((c, _), s)| {
                    c == &canonical_cwd
                        && s.is_alive()
                        && s.config
                            .file_extensions
                            .iter()
                            .any(|e| e.eq_ignore_ascii_case(&ext))
                })
                .map(|(_, s)| Arc::clone(s))
                .collect()
        };

        for sess in matching {
            sess.notify_did_save(&canon_file, content).await;
        }
    }

    /// Gracefully shut down all active sessions and prune `start_locks` (daemon teardown / tests).
    pub async fn shutdown_all(&self) {
        let drained: Vec<Arc<LspClientSession>> = {
            let mut guard = self.sessions.lock().await;
            guard.drain().map(|(_, s)| s).collect()
        };
        {
            let mut locks = self.start_locks.lock().await;
            locks.retain(|_, v| Arc::strong_count(v) > 1);
        }
        for sess in drained {
            sess.shutdown().await;
        }
    }

    async fn select_config_for_file(
        &self,
        canonical_cwd: &Path,
        canonical_file: &Path,
    ) -> Result<Option<LspServerConfig>, String> {
        let ext = canonical_file
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext.is_empty() {
            return Ok(None);
        }

        let home_dir = self.home_dir.clone();
        let cwd_scan = canonical_cwd.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let configs = load_effective_configs(&home_dir, &cwd_scan);
            // UserConfig takes highest precedence, followed by ProjectJson, then Builtin
            let mut candidates: Vec<LspServerConfig> = configs
                .into_iter()
                .filter(|c| {
                    c.file_extensions
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&ext))
                })
                .collect();
            candidates.sort_by_key(|c| match c.source {
                LspConfigSource::UserConfig => 0u8,
                LspConfigSource::ProjectJson => 1u8,
                LspConfigSource::Builtin => 2u8,
            });
            candidates
                .into_iter()
                .find(|c| resolve_config_binary(c).is_some())
        })
        .await
        .map_err(|e| format!("LSP file matcher failed: {e}"))
    }

    async fn select_workspace_configs(
        &self,
        canonical_cwd: &Path,
    ) -> Result<Vec<LspServerConfig>, String> {
        let home_dir = self.home_dir.clone();
        let cwd_scan = canonical_cwd.to_path_buf();
        tokio::task::spawn_blocking(move || {
            load_effective_configs(&home_dir, &cwd_scan)
                .into_iter()
                .filter(|c| {
                    (c.source != LspConfigSource::Builtin
                        || workspace_matches_root_markers(&cwd_scan, &c.root_markers))
                        && resolve_config_binary(c).is_some()
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| format!("LSP workspace matcher failed: {e}"))
    }

    async fn get_or_start_session(
        &self,
        canonical_cwd: &Path,
        config: LspServerConfig,
    ) -> Result<Arc<LspClientSession>, String> {
        let key = (canonical_cwd.to_path_buf(), config.id.clone());

        if let Some(existing) = self.sessions.lock().await.get(&key).cloned()
            && existing.is_alive()
        {
            existing.touch();
            return Ok(existing);
        }

        let key_lock = self.acquire_start_lock(&key).await;
        let _start_guard = key_lock.lock().await;

        if let Some(existing) = self.sessions.lock().await.get(&key).cloned()
            && existing.is_alive()
        {
            existing.touch();
            return Ok(existing);
        }

        self.spawn_and_register_locked(key, canonical_cwd, config)
            .await
    }

    /// Query `workspace/symbol` ONLY across already-running, alive LSP sessions for `cwd`.
    ///
    /// Used by `@symbol` autocomplete (`WorkspaceSymbolIndex`) so lexical results are enriched
    /// immediately when an LSP session is already warm without cold-spawning language servers
    /// on every keystroke.
    pub async fn query_ready_workspace_symbols(
        &self,
        cwd: &Path,
        query: &str,
        limit: usize,
    ) -> Vec<orbit_protocol::WorkspaceSymbolEntry> {
        let Ok(canonical_cwd) = validate_workspace_cwd(cwd) else {
            return Vec::new();
        };
        let cap = limit.clamp(1, MAX_SYMBOLS_LIMIT);
        let ready_sessions: Vec<Arc<LspClientSession>> = {
            let guard = self.sessions.lock().await;
            guard
                .iter()
                .filter(|((sess_cwd, _), sess)| sess_cwd == &canonical_cwd && sess.is_alive())
                .map(|(_, sess)| Arc::clone(sess))
                .collect()
        };
        if ready_sessions.is_empty() {
            return Vec::new();
        }

        let per_session_timeout = self.request_timeout.min(Duration::from_millis(650));
        let mut out: Vec<orbit_protocol::WorkspaceSymbolEntry> = Vec::new();
        for sess in ready_sessions {
            if out.len() >= cap {
                break;
            }
            let remaining = cap - out.len();
            if let Ok(syms) = sess
                .query_symbols(query, None, remaining, per_session_timeout)
                .await
            {
                for sym in syms {
                    let language = language_id_for_path(Path::new(&sym.path), &sess.config);
                    out.push(orbit_protocol::WorkspaceSymbolEntry {
                        name: sym.name,
                        kind: sym.kind,
                        path: sym.path,
                        start_line: sym.line,
                        end_line: sym.end_line.max(sym.line),
                        signature: None,
                        container: sym.container_name,
                        language,
                    });
                    if out.len() >= cap {
                        break;
                    }
                }
            }
        }
        out
    }

    #[cfg(test)]
    async fn insert_test_session(&self, canonical_cwd: PathBuf, session: Arc<LspClientSession>) {
        let key = (canonical_cwd, session.config.id.clone());
        self.insert_session_bounded(key, session).await;
    }
}

/// Adapter bridging [`LspManager`] into [`crate::symbols::ExternalSymbolProvider`] so `@symbol`
/// autocomplete seamlessly enriches pure-Rust lexical hits with live LSP `workspace/symbol`
/// results whenever a language server session is already ready for the workspace.
pub struct LspExternalSymbolProvider {
    lsp: Arc<LspManager>,
}

impl LspExternalSymbolProvider {
    pub fn new(lsp: Arc<LspManager>) -> Self {
        Self { lsp }
    }
}

impl crate::symbols::ExternalSymbolProvider for LspExternalSymbolProvider {
    fn search_symbols<'a>(
        &'a self,
        cwd: &'a Path,
        query: &'a str,
        limit: usize,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Vec<orbit_protocol::WorkspaceSymbolEntry>> + Send + 'a,
        >,
    > {
        Box::pin(async move {
            self.lsp
                .query_ready_workspace_symbols(cwd, query, limit)
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    /// Pure-Rust in-process mock LSP server communicating over `Content-Length` framed JSON-RPC 2.0.
    fn spawn_mock_lsp_server(
        target_uri: String,
        expected_diag_uri: String,
    ) -> (
        tokio::io::DuplexStream,
        tokio::io::DuplexStream,
        JoinHandle<Vec<String>>,
    ) {
        let (client_read, server_write) = tokio::io::duplex(64 * 1024);
        let (server_read, client_write) = tokio::io::duplex(64 * 1024);

        let handle = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut writer = server_write;
            let mut seen_methods = Vec::new();

            while let Ok(Some(msg)) = read_lsp_message(&mut reader).await {
                let method = msg
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let id = msg.get("id").cloned();

                if !method.is_empty() {
                    seen_methods.push(method.clone());
                }

                match method.as_str() {
                    "initialize" => {
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": 9001,
                                "method": "workspace/configuration",
                                "params": { "items": [{ "section": "rust-analyzer" }] }
                            }),
                        )
                        .await;
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "capabilities": {
                                    "definitionProvider": true,
                                    "workspaceSymbolProvider": true,
                                    "documentSymbolProvider": true
                                }
                            }
                        });
                        let _ = write_lsp_message(&mut writer, &reply).await;
                    }
                    "initialized" => {}
                    "textDocument/didOpen" | "textDocument/didChange" => {
                        let diag_notif = json!({
                            "jsonrpc": "2.0",
                            "method": "textDocument/publishDiagnostics",
                            "params": {
                                "uri": expected_diag_uri,
                                "diagnostics": [
                                    {
                                        "range": {
                                            "start": { "line": 1, "character": 4 },
                                            "end": { "line": 1, "character": 12 }
                                        },
                                        "severity": 1,
                                        "code": "E0308",
                                        "source": "mock-ls",
                                        "message": "mismatched types"
                                    },
                                    {
                                        "range": {
                                            "start": { "line": 0, "character": 0 },
                                            "end": { "line": 0, "character": 3 }
                                        },
                                        "severity": 2,
                                        "code": 42,
                                        "source": "mock-ls",
                                        "message": "unused import"
                                    }
                                ]
                            }
                        });
                        let _ = write_lsp_message(&mut writer, &diag_notif).await;
                    }
                    "workspace/symbol" => {
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": [
                                {
                                    "name": "compute_answer",
                                    "kind": 12,
                                    "containerName": "math",
                                    "location": {
                                        "uri": target_uri,
                                        "range": {
                                            "start": { "line": 2, "character": 0 },
                                            "end": { "line": 4, "character": 1 }
                                        }
                                    }
                                },
                                {
                                    "name": "other_helper",
                                    "kind": 12,
                                    "location": {
                                        "uri": target_uri,
                                        "range": {
                                            "start": { "line": 6, "character": 0 },
                                            "end": { "line": 7, "character": 1 }
                                        }
                                    }
                                }
                            ]
                        });
                        let _ = write_lsp_message(&mut writer, &reply).await;
                    }
                    "textDocument/documentSymbol" => {
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": [
                                {
                                    "name": "Engine",
                                    "kind": 23,
                                    "range": {
                                        "start": { "line": 0, "character": 0 },
                                        "end": { "line": 5, "character": 1 }
                                    },
                                    "selectionRange": {
                                        "start": { "line": 0, "character": 11 },
                                        "end": { "line": 0, "character": 17 }
                                    },
                                    "children": [
                                        {
                                            "name": "run",
                                            "kind": 6,
                                            "range": {
                                                "start": { "line": 2, "character": 4 },
                                                "end": { "line": 4, "character": 5 }
                                            },
                                            "selectionRange": {
                                                "start": { "line": 2, "character": 11 },
                                                "end": { "line": 2, "character": 14 }
                                            }
                                        }
                                    ]
                                }
                            ]
                        });
                        let _ = write_lsp_message(&mut writer, &reply).await;
                    }
                    "textDocument/definition" => {
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": [{
                                "targetUri": target_uri,
                                "targetRange": {
                                    "start": { "line": 2, "character": 0 },
                                    "end": { "line": 4, "character": 1 }
                                },
                                "targetSelectionRange": {
                                    "start": { "line": 2, "character": 7 },
                                    "end": { "line": 2, "character": 21 }
                                }
                            }]
                        });
                        let _ = write_lsp_message(&mut writer, &reply).await;
                    }
                    "shutdown" => {
                        let reply = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": Value::Null
                        });
                        let _ = write_lsp_message(&mut writer, &reply).await;
                    }
                    "exit" => break,
                    _ => {}
                }
            }
            seen_methods
        });

        (client_read, client_write, handle)
    }

    #[tokio::test]
    async fn lsp_framing_initialize_symbols_definition_diagnostics_and_shutdown() {
        let tmp = std::env::temp_dir().join(format!("orbit-lsp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(tmp.join("src")).expect("create src dir");
        let file_path = tmp.join("src/lib.rs");
        std::fs::write(
            &file_path,
            "pub struct Engine;\n\npub fn compute_answer() -> u32 {\n    42\n}\n",
        )
        .expect("write test rs");
        let canon_cwd = validate_workspace_cwd(&tmp).expect("canon cwd");
        let (canon_file, rel_path) =
            resolve_workspace_file(&canon_cwd, "src/lib.rs").expect("resolve file");
        let file_uri = path_to_file_uri(&canon_file);

        let (client_read, client_write, server_handle) =
            spawn_mock_lsp_server(file_uri.clone(), file_uri.clone());

        let cfg = LspServerConfig {
            id: "mock-rs".into(),
            name: "Mock Rust LS".into(),
            command: "mock-rs".into(),
            args: Vec::new(),
            file_extensions: vec!["rs".into()],
            language_ids: vec!["rust".into()],
            root_markers: vec!["Cargo.toml".into()],
            enabled: true,
            source: LspConfigSource::Builtin,
        };

        let session = LspClientSession::from_transport(
            cfg,
            Some("/mock/mock-rs".into()),
            canon_cwd.clone(),
            BufReader::new(client_read),
            Box::new(client_write),
            None,
            Duration::from_secs(3),
        )
        .await
        .expect("initialize mock lsp");

        assert_eq!(session.status().state, LspServerState::Ready);

        // 1. workspace/symbol filtering
        let ws_syms = session
            .query_symbols("compute", None, 20, Duration::from_secs(3))
            .await
            .expect("workspace symbols");
        assert_eq!(ws_syms.len(), 1);
        assert_eq!(ws_syms[0].name, "compute_answer");
        assert_eq!(ws_syms[0].kind, "function");
        assert_eq!(ws_syms[0].container_name.as_deref(), Some("math"));
        assert_eq!(ws_syms[0].path, "src/lib.rs");
        assert_eq!(ws_syms[0].line, 3);
        assert_eq!(ws_syms[0].col, 1);

        // 2. textDocument/documentSymbol (hierarchical children flattening)
        let doc_syms = session
            .query_symbols(
                "",
                Some((&canon_file, &rel_path)),
                20,
                Duration::from_secs(3),
            )
            .await
            .expect("doc symbols");
        assert_eq!(doc_syms.len(), 2);
        assert_eq!(doc_syms[0].name, "Engine");
        assert_eq!(doc_syms[0].kind, "struct");
        assert_eq!(doc_syms[1].name, "run");
        assert_eq!(doc_syms[1].container_name.as_deref(), Some("Engine"));

        // 3. textDocument/definition (LocationLink + 1-based coords + line preview)
        let defs = session
            .goto_definition(&canon_file, 3, 10, Duration::from_secs(3))
            .await
            .expect("goto definition");
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].path, "src/lib.rs");
        assert_eq!(defs[0].line, 3);
        assert_eq!(defs[0].col, 8);
        assert_eq!(
            defs[0].preview.as_deref(),
            Some("pub fn compute_answer() -> u32 {")
        );

        // 4. publishDiagnostics cache populated on didOpen & sorted by severity
        tokio::time::sleep(Duration::from_millis(40)).await;
        let diags = session.cached_diagnostics(Some("src/lib.rs"));
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].severity, LspDiagnosticSeverity::Error);
        assert_eq!(diags[0].line, 2);
        assert_eq!(diags[0].col, 5);
        assert_eq!(diags[0].code.as_deref(), Some("E0308"));
        assert_eq!(diags[1].severity, LspDiagnosticSeverity::Warning);
        assert_eq!(diags[1].code.as_deref(), Some("42"));
        assert_eq!(session.status().diagnostics_count, 2);

        // 5. Graceful shutdown, idle reaper, and start_locks pruning verification
        let mgr = LspManager::with_timeouts(
            tmp.clone(),
            Duration::from_secs(0),
            Duration::from_secs(3),
            Duration::from_secs(3),
        );
        let key = (canon_cwd.clone(), "mock-rs".to_string());
        let _ = mgr.acquire_start_lock(&key).await;
        mgr.insert_test_session(canon_cwd.clone(), Arc::clone(&session))
            .await;
        let evicted = mgr.reap_idle_sessions().await;
        assert_eq!(evicted, 1);
        assert!(!session.is_alive());
        assert!(mgr.start_locks.lock().await.is_empty());

        let seen = server_handle.await.expect("mock server exited cleanly");
        assert!(seen.iter().any(|m| m == "initialize"));
        assert!(seen.iter().any(|m| m == "initialized"));
        assert!(seen.iter().any(|m| m == "textDocument/didOpen"));
        assert!(seen.iter().any(|m| m == "workspace/symbol"));
        assert!(seen.iter().any(|m| m == "textDocument/definition"));
        assert!(seen.iter().any(|m| m == "shutdown"));
        assert!(seen.iter().any(|m| m == "exit"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn lsp_project_allowlist_user_precedence_and_env_scrubbing() {
        // Allowlisted bare binary in project `.lsp.json` succeeds
        let valid_project = r#"{
            "rust-analyzer": {
                "command": "rust-analyzer",
                "args": ["--log-file", "/tmp/ra.log"],
                "extensionToLanguage": { ".rs": "rust" }
            },
            "ruff-check": {
                "name": "Ruff",
                "command": "ruff",
                "args": ["server"],
                "file_extensions": ["py"]
            }
        }"#;
        let parsed = parse_lsp_config_json(valid_project, LspConfigSource::ProjectJson)
            .expect("parse allowlisted .lsp.json");
        assert_eq!(parsed.len(), 2);

        // Non-allowlisted commands, interpreters, and path-qualified binaries MUST be rejected in ProjectJson
        for bad_cmd in [
            "/usr/bin/awk",
            "python3.12",
            "git",
            "npx",
            "sh",
            "./rust-analyzer",
            "/usr/local/bin/rust-analyzer",
            "rust-analyzer; rm -rf /",
            "../../bin/evil",
        ] {
            let raw = json!({ "bad": { "command": bad_cmd } }).to_string();
            assert!(
                parse_lsp_config_json(&raw, LspConfigSource::ProjectJson).is_err(),
                "expected ProjectJson to reject `{bad_cmd}`"
            );
        }

        // Host-local UserConfig (`$ORBIT_HOME/lsp.json`) CAN configure custom binaries
        let custom_user = r#"{
            "my-ls": {
                "command": "/opt/custom/bin/my-ls",
                "args": ["--stdio"],
                "file_extensions": ["foo"]
            }
        }"#;
        let user_parsed = parse_lsp_config_json(custom_user, LspConfigSource::UserConfig)
            .expect("UserConfig permits custom path");
        assert_eq!(user_parsed.len(), 1);
        assert_eq!(user_parsed[0].source, LspConfigSource::UserConfig);

        // UserConfig takes higher precedence than ProjectJson on conflicting server ID
        let base = std::env::temp_dir().join(format!("orbit-lsp-prec-{}", uuid::Uuid::new_v4()));
        let home = base.join("orbit_home");
        let ws = base.join("repo");
        std::fs::create_dir_all(&home).expect("create home");
        std::fs::create_dir_all(&ws).expect("create ws");
        std::fs::write(
            ws.join(".lsp.json"),
            r#"{ "rust-analyzer": { "command": "rust-analyzer", "args": ["--from-project"] } }"#,
        )
        .expect("write project lsp.json");
        std::fs::write(
            home.join("lsp.json"),
            r#"{ "rust-analyzer": { "command": "/custom/rust-analyzer", "args": ["--from-user"] } }"#,
        )
        .expect("write user lsp.json");

        let canon_ws = validate_workspace_cwd(&ws).expect("canon ws");
        let effective = load_effective_configs(&home, &canon_ws);
        let ra = effective
            .iter()
            .find(|c| c.id == "rust-analyzer")
            .expect("rust-analyzer present");
        assert_eq!(ra.source, LspConfigSource::UserConfig);
        assert_eq!(ra.command, "/custom/rust-analyzer");
        assert_eq!(ra.args, vec!["--from-user"]);
        let _ = std::fs::remove_dir_all(&base);

        // Dangerous child env vars stripped while normal tool vars are preserved
        for stripped in [
            "ORBIT_MCP_TOKEN",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
            "BASH_ENV",
            "ENV",
            "NODE_OPTIONS",
            "PYTHONSTARTUP",
            "PYTHONPATH",
            "PERL5OPT",
            "RUBYOPT",
            "GIT_SSH_COMMAND",
        ] {
            assert!(
                should_strip_lsp_env_var(stripped),
                "expected `{stripped}` to be stripped"
            );
        }
        for kept in ["PATH", "HOME", "RUSTUP_HOME", "CARGO_HOME", "GOPATH"] {
            assert!(
                !should_strip_lsp_env_var(kept),
                "expected `{kept}` to be kept"
            );
        }
    }

    #[test]
    fn lsp_path_traversal_home_git_and_uri_hardening() {
        let base = std::env::temp_dir().join(format!("orbit-lsp-sec-{}", uuid::Uuid::new_v4()));
        let ws = base.join("repo");
        let outside = base.join("secret.txt");
        std::fs::create_dir_all(ws.join("src")).expect("create ws");
        std::fs::create_dir_all(ws.join(".git")).expect("create .git");
        std::fs::write(ws.join("src/main.rs"), "fn main() {}\n").expect("write main.rs");
        std::fs::write(ws.join(".git/config"), "[core]\n").expect("write git config");
        std::fs::write(&outside, "top-secret\n").expect("write secret");

        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&outside, ws.join("src/escape.rs"));
        }

        // Root `/` and `$HOME` rejected by validate_workspace_cwd
        assert!(validate_workspace_cwd(Path::new("/")).is_err());
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            assert!(validate_workspace_cwd(&home).is_err());
        }
        assert!(validate_workspace_cwd(&ws.join(".git")).is_err());

        let canon_cwd = validate_workspace_cwd(&ws).expect("valid ws");

        // Valid relative path succeeds
        let (_, rel) = resolve_workspace_file(&canon_cwd, "src/main.rs").expect("valid rel");
        assert_eq!(rel, "src/main.rs");

        // Traversal, outside, `.git`, backslash, NUL, and escaping symlink rejected
        assert!(resolve_workspace_file(&canon_cwd, "../secret.txt").is_err());
        assert!(resolve_workspace_file(&canon_cwd, "src/../../secret.txt").is_err());
        assert!(resolve_workspace_file(&canon_cwd, &outside.to_string_lossy()).is_err());
        assert!(resolve_workspace_file(&canon_cwd, ".git/config").is_err());
        assert!(resolve_workspace_file(&canon_cwd, "src\\main.rs").is_err());
        assert!(resolve_workspace_file(&canon_cwd, "src/main.rs\0").is_err());

        #[cfg(unix)]
        {
            assert!(resolve_workspace_file(&canon_cwd, "src/escape.rs").is_err());
        }

        // Strict file:// URI parsing
        let spaced = canon_cwd.join("my file.rs");
        let uri = path_to_file_uri(&spaced);
        assert!(uri.contains("%20"));
        assert_eq!(file_uri_to_path(&uri), Some(spaced.clone()));
        assert_eq!(
            file_uri_to_path(&format!("{uri}?version=1#L10")),
            Some(spaced)
        );
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/demo.rs"),
            Some(PathBuf::from("/tmp/demo.rs"))
        );
        // Remote authority, relative URI, and NUL byte rejected
        assert!(file_uri_to_path("file://evil.host/etc/passwd").is_none());
        assert!(file_uri_to_path("file://relative/path.rs").is_none());
        assert!(file_uri_to_path("file:///tmp/a%00b.rs").is_none());

        // read_line_preview blocks `.git`, outside files, and directories (non-regular files)
        let git_uri = path_to_file_uri(&canon_cwd.join(".git/config"));
        assert!(read_line_preview(&git_uri, &canon_cwd, 1).is_none());
        let dir_uri = path_to_file_uri(&canon_cwd.join("src"));
        assert!(read_line_preview(&dir_uri, &canon_cwd, 1).is_none());
        let out_uri = path_to_file_uri(&outside);
        assert!(read_line_preview(&out_uri, &canon_cwd, 1).is_none());
        let main_uri = path_to_file_uri(&canon_cwd.join("src/main.rs"));
        assert_eq!(
            read_line_preview(&main_uri, &canon_cwd, 1).as_deref(),
            Some("fn main() {}")
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn lsp_header_line_cap_session_cap_eviction_and_utf16_clamping() {
        // 1. Oversized header line without newline is rejected cleanly
        let huge = format!(
            "X-Evil: {}",
            "A".repeat(MAX_LSP_HEADER_LINE_BYTES as usize + 64)
        );
        let mut cursor = BufReader::new(huge.as_bytes());
        let err = read_lsp_message(&mut cursor)
            .await
            .expect_err("oversized header rejected");
        assert!(err.contains("8192"));

        // 2. UTF-16 column clamping handles surrogate pairs without splitting
        let sample = "let s = \"🚀\";\r\nfn foo() {}\n";
        // Line 1: `let s = "` = 9 UTF-16 units; `🚀` = 2 UTF-16 units (offsets 9, 10); `";` = 2 units (total 13)
        assert_eq!(clamp_utf16_col_for_line(sample, 1, 10), (0, 9)); // start of emoji
        assert_eq!(clamp_utf16_col_for_line(sample, 1, 11), (0, 9)); // trailing surrogate snaps back to 9!
        assert_eq!(clamp_utf16_col_for_line(sample, 1, 12), (0, 11)); // closing quote
        assert_eq!(clamp_utf16_col_for_line(sample, 1, 999), (0, 13)); // clamped to line end
        assert_eq!(clamp_utf16_col_for_line(sample, 2, 4), (1, 3));

        // 3. MAX_ACTIVE_LSP_SESSIONS eviction removes the most-idle session
        let tmp = std::env::temp_dir().join(format!("orbit-lsp-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).expect("create tmp");
        let canon_cwd = validate_workspace_cwd(&tmp).expect("canon tmp");
        let mgr = LspManager::with_timeouts(
            tmp.clone(),
            Duration::from_secs(600),
            Duration::from_secs(3),
            Duration::from_secs(3),
        );

        let mut sessions = Vec::new();
        for idx in 0..=MAX_ACTIVE_LSP_SESSIONS {
            let uri = path_to_file_uri(&canon_cwd.join(format!("f{idx}.rs")));
            let (cr, cw, _h) = spawn_mock_lsp_server(uri.clone(), uri);
            let cfg = LspServerConfig {
                id: format!("srv-{idx}"),
                name: format!("Srv {idx}"),
                command: "mock".into(),
                args: Vec::new(),
                file_extensions: vec!["rs".into()],
                language_ids: vec!["rust".into()],
                root_markers: Vec::new(),
                enabled: true,
                source: LspConfigSource::Builtin,
            };
            let s = LspClientSession::from_transport(
                cfg,
                None,
                canon_cwd.clone(),
                BufReader::new(cr),
                Box::new(cw),
                None,
                Duration::from_secs(3),
            )
            .await
            .expect("init session");
            // Session 0 has the oldest timestamp so it must be evicted when inserting the 13th
            s.set_last_used_for_test(1000 + idx as u64);
            mgr.insert_test_session(canon_cwd.clone(), Arc::clone(&s))
                .await;
            sessions.push(s);
        }

        assert_eq!(mgr.sessions.lock().await.len(), MAX_ACTIVE_LSP_SESSIONS);
        assert!(
            !sessions[0].is_alive(),
            "oldest session should have been evicted and shut down"
        );
        assert!(
            sessions[MAX_ACTIVE_LSP_SESSIONS].is_alive(),
            "newest session should remain alive"
        );

        mgr.shutdown_all().await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn lsp_concurrent_did_open_sends_exactly_once() {
        let tmp = std::env::temp_dir().join(format!("orbit-lsp-conc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).expect("create tmp");
        let file = tmp.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").expect("write main.rs");
        let canon_cwd = validate_workspace_cwd(&tmp).expect("canon cwd");
        let (canon_file, _) = resolve_workspace_file(&canon_cwd, "main.rs").expect("resolve file");
        let uri = path_to_file_uri(&canon_file);

        let (cr, cw, server_handle) = spawn_mock_lsp_server(uri.clone(), uri);
        let cfg = LspServerConfig {
            id: "mock-conc".into(),
            name: "Mock Conc".into(),
            command: "mock".into(),
            args: Vec::new(),
            file_extensions: vec!["rs".into()],
            language_ids: vec!["rust".into()],
            root_markers: Vec::new(),
            enabled: true,
            source: LspConfigSource::Builtin,
        };
        let session = LspClientSession::from_transport(
            cfg,
            None,
            canon_cwd,
            BufReader::new(cr),
            Box::new(cw),
            None,
            Duration::from_secs(3),
        )
        .await
        .expect("init session");

        // Fire 6 concurrent sync requests on the same unopened file
        let mut tasks = Vec::new();
        for _ in 0..6 {
            let s = Arc::clone(&session);
            let f = canon_file.clone();
            tasks.push(tokio::spawn(async move {
                s.ensure_document_synced(&f, None).await
            }));
        }
        let mut opened_count = 0usize;
        for t in tasks {
            let (changed, _) = t.await.expect("join").expect("sync ok");
            if changed {
                opened_count += 1;
            }
        }
        assert_eq!(opened_count, 1, "exactly one caller should send didOpen");

        session.shutdown().await;
        let seen = server_handle.await.expect("server exit");
        let did_open_msgs = seen
            .iter()
            .filter(|m| m.as_str() == "textDocument/didOpen")
            .count();
        assert_eq!(did_open_msgs, 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
