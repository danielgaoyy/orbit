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
    LspCodeAction, LspCommand, LspCompletionItem, LspConfigSource, LspDiagnostic,
    LspDiagnosticSeverity, LspFileTextEdits, LspHoverInfo, LspLocation, LspRange,
    LspReferenceLocation, LspServerConfig, LspServerState, LspServerStatus, LspSymbolInfo,
    LspTextEdit, LspWorkspaceEdit, LspWorkspaceEditResult,
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

use crate::workspace_files::{
    canonicalize_workspace_root, is_path_inside_git, sha256_hex, write_workspace_file,
};

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
/// Maximum hover payload size returned over the wire (32 KiB).
const MAX_HOVER_BYTES: usize = 32 * 1024;
/// Maximum reference locations returned by a single query.
const MAX_REFERENCES_LIMIT: usize = 200;
/// Maximum completion items returned by a single query.
const MAX_COMPLETION_ITEMS: usize = 80;
/// Maximum code actions returned by a single query.
const MAX_CODE_ACTIONS_LIMIT: usize = 50;
/// Maximum files modified in a single workspace edit (`rename` / `codeAction`).
const MAX_WORKSPACE_EDIT_FILES: usize = 200;
/// Maximum total text edits applied in a single workspace edit.
const MAX_WORKSPACE_EDIT_TOTAL_EDITS: usize = 5000;
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

    let canonical_file = crate::platform::canonicalize_clean(&full)
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
/// Handles Unix paths (`/tmp/a.rs`), Windows drive paths (`C:\Users\me\a.rs`, `\\?\C:\...`),
/// and Windows UNC paths (`\\server\share\a.rs`, `\\?\UNC\server\share\a.rs`).
pub fn path_to_file_uri(path: &Path) -> String {
    let stripped = crate::platform::strip_windows_verbatim_prefix_str(&path.to_string_lossy());
    let raw = stripped.replace('\\', "/");
    let mut encoded = String::with_capacity(raw.len() + 8);
    if raw.starts_with("//") {
        // RFC 8089 UNC authority form: `\\server\share\a.rs` -> `file://server/share/a.rs`
        encoded.push_str("file:");
    } else {
        encoded.push_str("file://");
        if !raw.starts_with('/') {
            encoded.push('/');
        }
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
/// Rejects remote authorities (e.g. `file://evil.host/etc/passwd`), relative file URIs,
/// drive-relative Windows URIs (`file:///C:foo.rs`), and NUL bytes.
/// Supports both Unix (`/tmp/a.rs`) and Windows absolute drive-letter URIs (`file:///C:/Users/me/a.rs`).
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let without_suffix = uri.find(['?', '#']).map(|idx| &uri[..idx]).unwrap_or(uri);

    let path_part = without_suffix
        .strip_prefix("file://localhost/")
        .or_else(|| without_suffix.strip_prefix("file:///"))?;

    let bytes = path_part.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len() + 1);
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
    let sb = s.as_bytes();
    if sb.len() >= 2 && sb[0].is_ascii_alphabetic() && sb[1] == b':' {
        if sb.len() == 2 || sb[2] == b'/' || sb[2] == b'\\' {
            return Some(PathBuf::from(s));
        }
        // Reject ambiguous drive-relative Windows paths like `file:///C:foo.rs`
        return None;
    }
    Some(PathBuf::from(format!("/{s}")))
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
    if let Some(home) = crate::platform::resolve_user_home_dir() {
        search_dirs.push(home.join(".cargo").join("bin"));
        search_dirs.push(home.join("go").join("bin"));
        search_dirs.push(home.join(".local").join("bin"));
        #[cfg(unix)]
        search_dirs.push(home.join(".nvm/current/bin"));
        #[cfg(windows)]
        search_dirs.push(home.join("AppData").join("Roaming").join("npm"));
    }
    #[cfg(unix)]
    for sys_dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        search_dirs.push(PathBuf::from(sys_dir));
    }

    let mut seen = HashSet::new();
    for dir in search_dirs {
        if !seen.insert(dir.clone()) {
            continue;
        }
        if let Some(found) =
            crate::platform::find_executable_in_dir(&dir, trimmed, is_executable_file)
        {
            return Some(found);
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
                    "applyEdit": true,
                    "workspaceEdit": {
                        "documentChanges": true
                    },
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
                    "references": {
                        "dynamicRegistration": false
                    },
                    "hover": {
                        "dynamicRegistration": false,
                        "contentFormat": ["markdown", "plaintext"]
                    },
                    "completion": {
                        "dynamicRegistration": false,
                        "completionItem": {
                            "snippetSupport": false,
                            "documentationFormat": ["markdown", "plaintext"],
                            "deprecatedSupport": true
                        }
                    },
                    "codeAction": {
                        "dynamicRegistration": false,
                        "isPreferredSupport": true,
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "",
                                    "quickfix",
                                    "refactor",
                                    "refactor.extract",
                                    "refactor.inline",
                                    "refactor.rewrite",
                                    "source",
                                    "source.organizeImports"
                                ]
                            }
                        }
                    },
                    "rename": {
                        "dynamicRegistration": false,
                        "prepareSupport": false
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

    /// Resolve `textDocument/hover` at a 1-based `(line, utf16_col)` coordinate.
    pub async fn hover(
        &self,
        canonical_file: &Path,
        line_1based: u32,
        col_1based: u32,
        override_text: Option<&str>,
        timeout: Duration,
    ) -> Result<Option<LspHoverInfo>, String> {
        let (_, synced_text) = self
            .ensure_document_synced(canonical_file, override_text)
            .await?;
        let uri = path_to_file_uri(canonical_file);
        let (lsp_line, lsp_char) = clamp_utf16_col_for_line(&synced_text, line_1based, col_1based);

        let raw = self
            .send_request(
                "textDocument/hover",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                }),
                timeout,
            )
            .await?;

        Ok(parse_hover_response(&raw))
    }

    /// Resolve `textDocument/references` strictly confined to the canonical workspace root.
    pub async fn references(
        &self,
        canonical_file: &Path,
        line_1based: u32,
        col_1based: u32,
        include_declaration: bool,
        override_text: Option<&str>,
        timeout: Duration,
    ) -> Result<Vec<LspReferenceLocation>, String> {
        let (_, synced_text) = self
            .ensure_document_synced(canonical_file, override_text)
            .await?;
        let uri = path_to_file_uri(canonical_file);
        let (lsp_line, lsp_char) = clamp_utf16_col_for_line(&synced_text, line_1based, col_1based);

        let raw = self
            .send_request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                    "context": { "includeDeclaration": include_declaration },
                }),
                timeout,
            )
            .await?;

        let cwd_for_parse = self.canonical_cwd.clone();
        let mut refs =
            tokio::task::spawn_blocking(move || parse_reference_locations(&raw, &cwd_for_parse))
                .await
                .map_err(|e| format!("LSP references parse task failed: {e}"))?;
        refs.truncate(MAX_REFERENCES_LIMIT);
        Ok(refs)
    }

    /// Resolve `textDocument/completion` at a 1-based `(line, utf16_col)` coordinate (bounded to 80 items).
    pub async fn completion(
        &self,
        canonical_file: &Path,
        line_1based: u32,
        col_1based: u32,
        trigger_character: Option<&str>,
        override_text: Option<&str>,
        timeout: Duration,
    ) -> Result<Vec<LspCompletionItem>, String> {
        let (_, synced_text) = self
            .ensure_document_synced(canonical_file, override_text)
            .await?;
        let uri = path_to_file_uri(canonical_file);
        let (lsp_line, lsp_char) = clamp_utf16_col_for_line(&synced_text, line_1based, col_1based);

        let clean_trigger = trigger_character
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= 4);
        let context = match clean_trigger {
            Some(ch) => json!({
                "triggerKind": 2,
                "triggerCharacter": ch,
            }),
            None => json!({ "triggerKind": 1 }),
        };

        let raw = self
            .send_request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                    "context": context,
                }),
                timeout,
            )
            .await?;

        Ok(parse_completion_items(&raw))
    }

    /// Resolve `textDocument/codeAction` for a 1-based range and diagnostics.
    #[allow(clippy::too_many_arguments)]
    pub async fn code_action(
        &self,
        canonical_file: &Path,
        rel_path: &str,
        range: LspRange,
        diagnostics: &[LspDiagnostic],
        override_text: Option<&str>,
        timeout: Duration,
    ) -> Result<Vec<LspCodeAction>, String> {
        let (_, synced_text) = self
            .ensure_document_synced(canonical_file, override_text)
            .await?;
        let uri = path_to_file_uri(canonical_file);
        let (start_line, start_char) =
            clamp_utf16_col_for_line(&synced_text, range.line, range.col);
        let (end_line, end_char) =
            clamp_utf16_col_for_line(&synced_text, range.end_line, range.end_col);

        let effective_diags: Vec<LspDiagnostic> = if diagnostics.is_empty() {
            self.cached_diagnostics(Some(rel_path))
                .into_iter()
                .filter(|d| d.line <= range.end_line && d.end_line >= range.line)
                .take(32)
                .collect()
        } else {
            diagnostics.iter().take(32).cloned().collect()
        };

        let diag_values: Vec<Value> = effective_diags.iter().map(diagnostic_to_lsp_json).collect();

        let raw = self
            .send_request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": start_line, "character": start_char },
                        "end": { "line": end_line, "character": end_char },
                    },
                    "context": {
                        "diagnostics": diag_values,
                        "triggerKind": 1,
                    },
                }),
                timeout,
            )
            .await?;

        let cwd_for_parse = self.canonical_cwd.clone();
        tokio::task::spawn_blocking(move || parse_code_actions(&raw, &cwd_for_parse))
            .await
            .map_err(|e| format!("LSP codeAction parse task failed: {e}"))
    }

    /// Compute a workspace-confined `LspWorkspaceEdit` via `textDocument/rename`.
    pub async fn rename(
        &self,
        canonical_file: &Path,
        line_1based: u32,
        col_1based: u32,
        new_name: &str,
        override_text: Option<&str>,
        timeout: Duration,
    ) -> Result<LspWorkspaceEdit, String> {
        let trimmed = new_name.trim();
        if trimmed.is_empty()
            || trimmed.len() > 256
            || trimmed.chars().any(|c| c == '\0' || c.is_control())
        {
            return Err("Invalid rename identifier".into());
        }

        let (_, synced_text) = self
            .ensure_document_synced(canonical_file, override_text)
            .await?;
        let uri = path_to_file_uri(canonical_file);
        let (lsp_line, lsp_char) = clamp_utf16_col_for_line(&synced_text, line_1based, col_1based);

        let raw = self
            .send_request(
                "textDocument/rename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": lsp_line, "character": lsp_char },
                    "newName": trimmed,
                }),
                timeout,
            )
            .await?;

        if raw.is_null() {
            return Ok(LspWorkspaceEdit::default());
        }

        let cwd_for_parse = self.canonical_cwd.clone();
        tokio::task::spawn_blocking(move || parse_workspace_edit(&raw, &cwd_for_parse))
            .await
            .map_err(|e| format!("LSP rename parse task failed: {e}"))?
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
    let resolved = crate::platform::canonicalize_clean(&raw_path).unwrap_or(raw_path);
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
    let canon = crate::platform::canonicalize_clean(&file_path).ok()?;
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

fn extract_full_range_1based(range: &Value) -> LspRange {
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
        .saturating_add(1)
        .max(line);
    let end_col = end
        .and_then(|s| s.get("character"))
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
        .saturating_add(1);
    LspRange {
        line,
        col,
        end_line,
        end_col: if end_line == line {
            end_col.max(col)
        } else {
            end_col
        },
    }
}

/// Strictly resolve a `file://` URI to a canonical path + workspace-relative Unix path inside `canonical_cwd`.
/// Rejects URIs outside the workspace, inside `.git`, symlinks escaping workspace, or non-regular files.
fn resolve_confined_uri_to_rel_path(uri: &str, canonical_cwd: &Path) -> Option<(PathBuf, String)> {
    let raw_path = file_uri_to_path(uri)?;
    let canon = crate::platform::canonicalize_clean(&raw_path).ok()?;
    if !canon.starts_with(canonical_cwd) {
        return None;
    }
    if is_path_inside_git(canonical_cwd, &canon) {
        return None;
    }
    let meta = std::fs::metadata(&canon).ok()?;
    if !meta.is_file() {
        return None;
    }
    let rel = canon.strip_prefix(canonical_cwd).ok()?;
    let rel_str = path_to_unix_string(rel);
    if rel_str.is_empty() {
        return None;
    }
    Some((canon, rel_str))
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

fn format_marked_string(val: &Value) -> Option<(String, bool)> {
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((trimmed.to_string(), true))
            }
        }
        Value::Object(map) => {
            let value = map.get("value").and_then(Value::as_str)?.trim();
            if value.is_empty() {
                return None;
            }
            if let Some(lang) = map.get("language").and_then(Value::as_str) {
                let clean_lang = lang.trim();
                Some((format!("```{clean_lang}\n{value}\n```"), true))
            } else {
                let is_md = map
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_none_or(|k| !k.eq_ignore_ascii_case("plaintext"));
                Some((value.to_string(), is_md))
            }
        }
        _ => None,
    }
}

/// Parse an LSP `textDocument/hover` response (`MarkupContent | MarkedString | MarkedString[]`).
pub fn parse_hover_response(val: &Value) -> Option<LspHoverInfo> {
    if val.is_null() {
        return None;
    }
    let contents_val = val.get("contents")?;
    let (joined, is_markdown) = match contents_val {
        Value::Array(items) => {
            let mut parts = Vec::new();
            let mut any_md = false;
            for item in items {
                if let Some((part, md)) = format_marked_string(item) {
                    parts.push(part);
                    any_md |= md;
                }
            }
            if parts.is_empty() {
                return None;
            }
            (parts.join("\n\n---\n\n"), any_md)
        }
        other => format_marked_string(other)?,
    };

    let contents = truncate_utf8_bytes(&joined, MAX_HOVER_BYTES);
    let range = val.get("range").map(extract_full_range_1based);
    Some(LspHoverInfo {
        contents,
        format: if is_markdown {
            "markdown".to_string()
        } else {
            "plaintext".to_string()
        },
        range,
    })
}

/// Parse an LSP `textDocument/references` response, strictly confining every hit inside `canonical_cwd`.
pub fn parse_reference_locations(val: &Value, canonical_cwd: &Path) -> Vec<LspReferenceLocation> {
    let Some(items) = val.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut file_cache: HashMap<PathBuf, Vec<String>> = HashMap::new();

    for item in items {
        if out.len() >= MAX_REFERENCES_LIMIT {
            break;
        }
        let Some(uri) = item
            .get("uri")
            .or_else(|| item.get("targetUri"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Some(range_val) = item
            .get("range")
            .or_else(|| item.get("targetSelectionRange"))
            .or_else(|| item.get("targetRange"))
        else {
            continue;
        };
        let Some((canon_path, rel_path)) = resolve_confined_uri_to_rel_path(uri, canonical_cwd)
        else {
            continue;
        };
        let range = extract_full_range_1based(range_val);

        let lines = file_cache.entry(canon_path.clone()).or_insert_with(|| {
            std::fs::metadata(&canon_path)
                .ok()
                .filter(|m| m.is_file() && (m.len() as usize) <= MAX_LSP_DOC_BYTES)
                .and_then(|_| std::fs::read_to_string(&canon_path).ok())
                .map(|s| s.lines().map(ToOwned::to_owned).collect())
                .unwrap_or_default()
        });
        let preview_line = usize::try_from(range.line.saturating_sub(1))
            .ok()
            .and_then(|idx| lines.get(idx))
            .map(|l| l.trim().chars().take(160).collect::<String>())
            .filter(|s| !s.is_empty());

        out.push(LspReferenceLocation {
            path: rel_path,
            line: range.line,
            character: range.col,
            end_line: range.end_line,
            end_character: range.end_col,
            preview_line,
        });
    }
    out
}

fn lsp_completion_kind_label(code: u64) -> &'static str {
    match code {
        1 => "text",
        2 => "method",
        3 => "function",
        4 => "constructor",
        5 => "field",
        6 => "variable",
        7 => "class",
        8 => "interface",
        9 => "module",
        10 => "property",
        11 => "unit",
        12 => "value",
        13 => "enum",
        14 => "keyword",
        15 => "snippet",
        16 => "color",
        17 => "file",
        18 => "reference",
        19 => "folder",
        20 => "enum_member",
        21 => "constant",
        22 => "struct",
        23 => "event",
        24 => "operator",
        25 => "type_parameter",
        _ => "text",
    }
}

/// Strip LSP snippet tabstop markers (`$0`, `$1`, `${1:placeholder}`, `${1|a,b|}`) into plain insert text.
pub fn strip_lsp_snippet_syntax(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = String::with_capacity(raw.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if matches!(next, b'$' | b'}' | b'\\') {
                out.push(next as char);
                i += 2;
                continue;
            }
        }
        if bytes[i] == b'$' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                let mut j = i + 2;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > i + 2 && j < bytes.len() {
                    if bytes[j] == b'}' {
                        i = j + 1;
                        continue;
                    }
                    if bytes[j] == b':' || bytes[j] == b'|' {
                        let sep = bytes[j];
                        let content_start = j + 1;
                        let mut depth = 1usize;
                        let mut k = content_start;
                        while k < bytes.len() && depth > 0 {
                            if bytes[k] == b'\\' && k + 1 < bytes.len() {
                                k += 2;
                                continue;
                            }
                            if bytes[k] == b'{' {
                                depth += 1;
                            } else if bytes[k] == b'}' {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                            }
                            k += 1;
                        }
                        if depth == 0 {
                            let inner = &raw[content_start..k];
                            if sep == b'|' {
                                let trimmed = inner.trim_end_matches('|');
                                if let Some(first_choice) = trimmed.split(',').next() {
                                    out.push_str(first_choice);
                                }
                            } else {
                                out.push_str(&strip_lsp_snippet_syntax(inner));
                            }
                            i = k + 1;
                            continue;
                        }
                    }
                }
            } else if i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        if let Some(ch) = raw[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out
}

/// Parse an LSP `textDocument/completion` response (`CompletionItem[] | CompletionList`), sorted and capped to 80 items.
pub fn parse_completion_items(val: &Value) -> Vec<LspCompletionItem> {
    let items_slice = match val {
        Value::Array(arr) => arr.as_slice(),
        Value::Object(map) => match map.get("items").and_then(Value::as_array) {
            Some(arr) => arr.as_slice(),
            None => return Vec::new(),
        },
        _ => return Vec::new(),
    };

    let mut out = Vec::new();
    for item in items_slice {
        let Some(label) = item.get("label").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if label.is_empty() {
            continue;
        }
        let kind = lsp_completion_kind_label(item.get("kind").and_then(Value::as_u64).unwrap_or(1))
            .to_string();
        let detail = item
            .get("detail")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| truncate_utf8_bytes(s, 512));

        let documentation = item.get("documentation").and_then(|doc| match doc {
            Value::String(s) => {
                let trimmed = s.trim();
                (!trimmed.is_empty()).then(|| truncate_utf8_bytes(trimmed, 4096))
            }
            Value::Object(map) => map
                .get("value")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| truncate_utf8_bytes(s, 4096)),
            _ => None,
        });

        let is_snippet = item
            .get("insertTextFormat")
            .and_then(Value::as_u64)
            .is_some_and(|f| f == 2);

        let raw_insert = item
            .get("textEdit")
            .and_then(|te| te.get("newText"))
            .and_then(Value::as_str)
            .or_else(|| item.get("insertText").and_then(Value::as_str));

        let insert_text = raw_insert.map(|s| {
            let cleaned = if is_snippet {
                strip_lsp_snippet_syntax(s)
            } else {
                s.to_string()
            };
            truncate_utf8_bytes(&cleaned, 2048)
        });

        let sort_text = item
            .get("sortText")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let filter_text = item
            .get("filterText")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        out.push(LspCompletionItem {
            label: truncate_utf8_bytes(label, 256),
            kind,
            detail,
            documentation,
            insert_text,
            sort_text,
            filter_text,
        });
    }

    out.sort_by(|a, b| {
        let ka = a.sort_text.as_deref().unwrap_or(&a.label);
        let kb = b.sort_text.as_deref().unwrap_or(&b.label);
        ka.cmp(kb).then_with(|| a.label.cmp(&b.label))
    });
    out.truncate(MAX_COMPLETION_ITEMS);
    out
}

fn diagnostic_to_lsp_json(d: &LspDiagnostic) -> Value {
    let severity: u32 = match d.severity {
        LspDiagnosticSeverity::Error => 1,
        LspDiagnosticSeverity::Warning => 2,
        LspDiagnosticSeverity::Info => 3,
        LspDiagnosticSeverity::Hint => 4,
    };
    let mut obj = serde_json::Map::new();
    obj.insert(
        "range".into(),
        json!({
            "start": {
                "line": d.line.saturating_sub(1),
                "character": d.col.saturating_sub(1),
            },
            "end": {
                "line": d.end_line.max(d.line).saturating_sub(1),
                "character": d.end_col.saturating_sub(1),
            },
        }),
    );
    obj.insert("severity".into(), json!(severity));
    obj.insert("message".into(), json!(d.message));
    if let Some(src) = &d.source {
        obj.insert("source".into(), json!(src));
    }
    if let Some(code) = &d.code {
        obj.insert("code".into(), json!(code));
    }
    Value::Object(obj)
}

fn parse_lsp_text_edit(val: &Value) -> Option<LspTextEdit> {
    let range_val = val.get("range")?;
    let new_text = val.get("newText").and_then(Value::as_str)?;
    let range = extract_full_range_1based(range_val);
    Some(LspTextEdit {
        line: range.line,
        col: range.col,
        end_line: range.end_line,
        end_col: range.end_col,
        new_text: new_text.to_string(),
    })
}

/// Parse an LSP `WorkspaceEdit` (`changes` and/or `documentChanges`) with strict fail-closed workspace confinement.
/// If ANY target URI escapes `canonical_cwd`, targets `.git`, or contains an unsupported resource operation,
/// returns `Err(...)` so partial edits are never constructed.
pub fn parse_workspace_edit(val: &Value, canonical_cwd: &Path) -> Result<LspWorkspaceEdit, String> {
    if val.is_null() {
        return Ok(LspWorkspaceEdit::default());
    }
    let Some(obj) = val.as_object() else {
        return Err("Invalid LSP WorkspaceEdit payload".into());
    };

    let mut order: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, Vec<LspTextEdit>> = HashMap::new();
    let mut total_edits: usize = 0;

    let mut push_uri_edits = |uri: &str, raw_edits: &[Value]| -> Result<(), String> {
        let (_, rel_path) =
            resolve_confined_uri_to_rel_path(uri, canonical_cwd).ok_or_else(|| {
                format!("LSP workspace edit targets forbidden path outside workspace: `{uri}`")
            })?;
        if !grouped.contains_key(&rel_path) {
            if order.len() >= MAX_WORKSPACE_EDIT_FILES {
                return Err(format!(
                    "LSP workspace edit exceeds maximum file limit ({MAX_WORKSPACE_EDIT_FILES})"
                ));
            }
            order.push(rel_path.clone());
        }
        let bucket = grouped.entry(rel_path).or_default();
        for raw_edit in raw_edits {
            total_edits += 1;
            if total_edits > MAX_WORKSPACE_EDIT_TOTAL_EDITS {
                return Err(format!(
                    "LSP workspace edit exceeds maximum edit count ({MAX_WORKSPACE_EDIT_TOTAL_EDITS})"
                ));
            }
            let edit = parse_lsp_text_edit(raw_edit)
                .ok_or_else(|| "Malformed TextEdit inside LSP WorkspaceEdit".to_string())?;
            bucket.push(edit);
        }
        Ok(())
    };

    if let Some(doc_changes) = obj.get("documentChanges").and_then(Value::as_array) {
        for change in doc_changes {
            if let Some(kind) = change.get("kind").and_then(Value::as_str) {
                return Err(format!(
                    "Unsupported file resource operation `{kind}` in LSP WorkspaceEdit"
                ));
            }
            let uri = change
                .get("textDocument")
                .and_then(|td| td.get("uri"))
                .and_then(Value::as_str)
                .ok_or_else(|| "Missing `textDocument.uri` in LSP documentChanges".to_string())?;
            let edits = change
                .get("edits")
                .and_then(Value::as_array)
                .ok_or_else(|| "Missing `edits` array in LSP documentChanges".to_string())?;
            push_uri_edits(uri, edits)?;
        }
    } else if let Some(changes) = obj.get("changes").and_then(Value::as_object) {
        let mut uris: Vec<&String> = changes.keys().collect();
        uris.sort();
        for uri in uris {
            let Some(edits) = changes.get(uri).and_then(Value::as_array) else {
                continue;
            };
            push_uri_edits(uri, edits)?;
        }
    }

    let file_edits = order
        .into_iter()
        .filter_map(|path| {
            let edits = grouped.remove(&path)?;
            (!edits.is_empty()).then_some(LspFileTextEdits { path, edits })
        })
        .collect();

    Ok(LspWorkspaceEdit { file_edits })
}

fn parse_lsp_command(val: &Value) -> Option<LspCommand> {
    let command = val.get("command").and_then(Value::as_str)?.trim();
    if command.is_empty() {
        return None;
    }
    let title = val
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(command)
        .to_string();
    let arguments = val
        .get("arguments")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Some(LspCommand {
        title,
        command: command.to_string(),
        arguments,
    })
}

/// Parse an LSP `textDocument/codeAction` response (`(Command | CodeAction)[]`).
pub fn parse_code_actions(val: &Value, canonical_cwd: &Path) -> Vec<LspCodeAction> {
    let Some(items) = val.as_array() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        if out.len() >= MAX_CODE_ACTIONS_LIMIT {
            break;
        }
        // Skip disabled code actions
        if item.get("disabled").is_some_and(|d| !d.is_null()) {
            continue;
        }
        let Some(title) = item.get("title").and_then(Value::as_str).map(str::trim) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }

        // Case 1: bare LSP Command (`{ title, command: string, arguments? }`)
        if item.get("command").and_then(Value::as_str).is_some() {
            if let Some(cmd) = parse_lsp_command(item) {
                out.push(LspCodeAction {
                    title: title.to_string(),
                    kind: None,
                    is_preferred: false,
                    edit: None,
                    command: Some(cmd),
                });
            }
            continue;
        }

        // Case 2: CodeAction object
        let kind = item
            .get("kind")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let is_preferred = item
            .get("isPreferred")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let edit = match item.get("edit") {
            Some(edit_val) if !edit_val.is_null() => {
                match parse_workspace_edit(edit_val, canonical_cwd) {
                    Ok(ws_edit) => Some(ws_edit),
                    Err(err) => {
                        debug!(
                            title = %title,
                            error = %err,
                            "Dropping LSP code action whose WorkspaceEdit failed workspace confinement"
                        );
                        continue;
                    }
                }
            }
            _ => None,
        };
        let command = item.get("command").and_then(parse_lsp_command);
        if edit.is_none() && command.is_none() {
            continue;
        }
        out.push(LspCodeAction {
            title: title.to_string(),
            kind,
            is_preferred,
            edit,
            command,
        });
    }

    // Surface preferred quick fixes first while preserving server ordering among ties
    out.sort_by_key(|b| std::cmp::Reverse(b.is_preferred));
    out
}

/// Convert a 1-based `(line, utf16_col)` position to an exact UTF-8 byte offset inside `content`.
pub fn utf16_pos_to_byte_offset(
    content: &str,
    line_1based: u32,
    col_1based: u32,
) -> Result<usize, String> {
    let target_line_idx = usize::try_from(line_1based.saturating_sub(1))
        .map_err(|_| "Line index overflow".to_string())?;
    let target_col_0based = col_1based.saturating_sub(1);

    let mut line_idx = 0usize;
    let mut byte_offset = 0usize;
    let total_bytes = content.len();

    for raw_line in content.split('\n') {
        if line_idx == target_line_idx {
            let line_no_cr = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let mut utf16_acc: u32 = 0;
            for (char_byte_idx, ch) in line_no_cr.char_indices() {
                if utf16_acc == target_col_0based {
                    return Ok(byte_offset + char_byte_idx);
                }
                let units = u32::try_from(ch.len_utf16()).unwrap_or(1);
                if target_col_0based < utf16_acc.saturating_add(units) {
                    return Err(format!(
                        "LSP edit position {line_1based}:{col_1based} splits a UTF-16 surrogate pair"
                    ));
                }
                utf16_acc = utf16_acc.saturating_add(units);
            }
            // Column at or past EOL clamps cleanly to the end of the line (before `\r\n` / `\n`).
            return Ok(byte_offset + line_no_cr.len());
        }
        byte_offset = byte_offset.saturating_add(raw_line.len()).saturating_add(1);
        line_idx += 1;
    }

    if target_line_idx == line_idx && target_col_0based == 0 {
        return Ok(total_bytes);
    }

    Err(format!(
        "LSP edit line {line_1based} is out of bounds (file has {line_idx} lines)"
    ))
}

/// Apply a set of non-overlapping 1-based UTF-16 `LspTextEdit`s to a UTF-8 string in a single O(N) pass.
pub fn apply_text_edits_to_str(content: &str, edits: &[LspTextEdit]) -> Result<String, String> {
    if edits.is_empty() {
        return Ok(content.to_string());
    }

    let mut resolved: Vec<(usize, usize, usize, &str)> = Vec::with_capacity(edits.len());
    for (idx, edit) in edits.iter().enumerate() {
        let start = utf16_pos_to_byte_offset(content, edit.line, edit.col)?;
        let end = utf16_pos_to_byte_offset(content, edit.end_line, edit.end_col)?;
        if start > end {
            return Err(format!(
                "LSP text edit has inverted range ({}:{} > {}:{})",
                edit.line, edit.col, edit.end_line, edit.end_col
            ));
        }
        resolved.push((start, end, idx, edit.new_text.as_str()));
    }

    resolved.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.cmp(&b.2))
    });

    for pair in resolved.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("LSP workspace edit contains overlapping text ranges".into());
        }
    }

    let mut out = String::with_capacity(content.len());
    let mut cursor = 0usize;
    for (start, end, _, new_text) in resolved {
        out.push_str(&content[cursor..start]);
        out.push_str(new_text);
        cursor = end;
    }
    out.push_str(&content[cursor..]);
    Ok(out)
}

struct StagedWorkspaceFileEdit {
    rel_path: String,
    original_etag: String,
    new_text: String,
    edit_count: u32,
}

/// Safely and atomically apply an `LspWorkspaceEdit` to files strictly confined inside `cwd`.
///
/// Security & atomicity guarantees:
/// 1. Validates and resolves EVERY file strictly inside `canonical_cwd` (zero traversal, no `.git`, no symlinks) BEFORE writing any file.
/// 2. Computes ALL updated file contents in memory first (rejecting out-of-bounds or overlapping ranges with zero disk mutations).
/// 3. Writes each changed file atomically via `write_workspace_file` using its verified SHA-256 etag.
/// 4. Notifies any active matching LSP sessions (`didChange` + `didSave`) so open LSP buffers stay synchronized.
pub async fn lsp_apply_workspace_edit(
    manager: Option<&LspManager>,
    cwd: &Path,
    edit: &LspWorkspaceEdit,
) -> Result<LspWorkspaceEditResult, String> {
    let canonical_cwd = validate_workspace_cwd(cwd)?;
    if edit.file_edits.is_empty() {
        return Ok(LspWorkspaceEditResult::default());
    }
    if edit.file_edits.len() > MAX_WORKSPACE_EDIT_FILES {
        return Err(format!(
            "Workspace edit exceeds maximum file limit ({MAX_WORKSPACE_EDIT_FILES})"
        ));
    }

    let edit_owned = edit.clone();
    let cwd_owned = canonical_cwd.clone();

    let staged: Vec<StagedWorkspaceFileEdit> = tokio::task::spawn_blocking(move || {
        // Merge duplicate file entries by canonical path while validating every single target strictly inside workspace.
        let mut order: Vec<PathBuf> = Vec::new();
        let mut by_canon: HashMap<PathBuf, (String, Vec<LspTextEdit>)> = HashMap::new();
        let mut total_count: usize = 0;

        for fe in &edit_owned.file_edits {
            if fe.edits.is_empty() {
                continue;
            }
            let (canon_file, rel_path) = resolve_workspace_file(&cwd_owned, &fe.path)?;
            let raw_candidate = {
                let p = Path::new(fe.path.trim());
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    cwd_owned.join(p)
                }
            };
            if let Ok(raw_sym_meta) = std::fs::symlink_metadata(&raw_candidate)
                && raw_sym_meta.file_type().is_symlink()
            {
                return Err(format!("Refusing to edit through symlink `{}`", fe.path));
            }
            let sym_meta = std::fs::symlink_metadata(&canon_file)
                .map_err(|e| format!("Cannot inspect `{rel_path}`: {e}"))?;
            if sym_meta.file_type().is_symlink() {
                return Err(format!("Refusing to edit through symlink `{rel_path}`"));
            }
            if !sym_meta.is_file() {
                return Err(format!("Target `{rel_path}` is not a regular file"));
            }
            if sym_meta.len() as usize > MAX_LSP_DOC_BYTES {
                return Err(format!(
                    "File `{rel_path}` exceeds 2 MiB LSP workspace edit limit"
                ));
            }
            total_count = total_count.saturating_add(fe.edits.len());
            if total_count > MAX_WORKSPACE_EDIT_TOTAL_EDITS {
                return Err(format!(
                    "Workspace edit exceeds maximum edit count ({MAX_WORKSPACE_EDIT_TOTAL_EDITS})"
                ));
            }
            if !by_canon.contains_key(&canon_file) {
                if order.len() >= MAX_WORKSPACE_EDIT_FILES {
                    return Err(format!(
                        "Workspace edit exceeds maximum file limit ({MAX_WORKSPACE_EDIT_FILES})"
                    ));
                }
                order.push(canon_file.clone());
                by_canon.insert(canon_file.clone(), (rel_path, Vec::new()));
            }
            if let Some((_, edits_vec)) = by_canon.get_mut(&canon_file) {
                edits_vec.extend(fe.edits.iter().cloned());
            }
        }

        let mut prepared = Vec::with_capacity(order.len());
        for canon_file in order {
            let Some((rel_path, edits)) = by_canon.remove(&canon_file) else {
                continue;
            };
            let raw_bytes = std::fs::read(&canon_file)
                .map_err(|e| format!("Failed reading `{rel_path}`: {e}"))?;
            let original_etag = sha256_hex(&raw_bytes);
            let original_text = String::from_utf8(raw_bytes)
                .map_err(|_| format!("File `{rel_path}` is not valid UTF-8"))?;
            let new_text = apply_text_edits_to_str(&original_text, &edits)
                .map_err(|e| format!("Failed applying edits to `{rel_path}`: {e}"))?;
            if new_text != original_text {
                let edit_count = u32::try_from(edits.len()).unwrap_or(u32::MAX);
                prepared.push(StagedWorkspaceFileEdit {
                    rel_path,
                    original_etag,
                    new_text,
                    edit_count,
                });
            }
        }
        Ok::<Vec<StagedWorkspaceFileEdit>, String>(prepared)
    })
    .await
    .map_err(|e| format!("Workspace edit preparation task failed: {e}"))??;

    if staged.is_empty() {
        return Ok(LspWorkspaceEditResult::default());
    }

    let mut modified_files = Vec::with_capacity(staged.len());
    let mut total_edits: u32 = 0;

    for item in staged {
        let outcome = write_workspace_file(
            &canonical_cwd,
            &item.rel_path,
            &item.new_text,
            Some(&item.original_etag),
            false,
        )
        .await?;
        if outcome.conflict.is_some() {
            return Err(format!(
                "File `{}` changed on disk while applying workspace edit",
                item.rel_path
            ));
        }
        if let Some(mgr) = manager {
            mgr.notify_file_saved(&canonical_cwd, &item.rel_path, &item.new_text)
                .await;
        }
        modified_files.push(item.rel_path);
        total_edits = total_edits.saturating_add(item.edit_count);
    }

    Ok(LspWorkspaceEditResult {
        applied: !modified_files.is_empty(),
        modified_files,
        total_edits,
    })
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

    /// Resolve `textDocument/hover` documentation at a 1-based `(line, utf16_col)` in `path`.
    pub async fn hover(
        &self,
        cwd: &Path,
        path: &str,
        line: u32,
        col: u32,
        content: Option<&str>,
    ) -> Result<Option<LspHoverInfo>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, _) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Ok(None);
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        sess.hover(&canon_file, line, col, content, self.request_timeout)
            .await
    }

    /// Resolve workspace-confined `textDocument/references` locations at a 1-based `(line, utf16_col)`.
    pub async fn references(
        &self,
        cwd: &Path,
        path: &str,
        line: u32,
        col: u32,
        include_declaration: bool,
        content: Option<&str>,
    ) -> Result<Vec<LspReferenceLocation>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, _) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Ok(Vec::new());
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        sess.references(
            &canon_file,
            line,
            col,
            include_declaration,
            content,
            self.request_timeout,
        )
        .await
    }

    /// Resolve `textDocument/completion` candidates at a 1-based `(line, utf16_col)` (capped at 80 items).
    pub async fn completion(
        &self,
        cwd: &Path,
        path: &str,
        line: u32,
        col: u32,
        trigger_character: Option<&str>,
        content: Option<&str>,
    ) -> Result<Vec<LspCompletionItem>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, _) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Ok(Vec::new());
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        sess.completion(
            &canon_file,
            line,
            col,
            trigger_character,
            content,
            self.request_timeout,
        )
        .await
    }

    /// Resolve `textDocument/codeAction` quick fixes and refactors for a 1-based range and diagnostics.
    pub async fn code_action(
        &self,
        cwd: &Path,
        path: &str,
        range: LspRange,
        diagnostics: &[LspDiagnostic],
        content: Option<&str>,
    ) -> Result<Vec<LspCodeAction>, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, rel_path) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Ok(Vec::new());
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        sess.code_action(
            &canon_file,
            &rel_path,
            range,
            diagnostics,
            content,
            self.request_timeout,
        )
        .await
    }

    /// Compute a symbol rename via `textDocument/rename` and atomically apply it inside `cwd`.
    pub async fn rename(
        &self,
        cwd: &Path,
        path: &str,
        line: u32,
        col: u32,
        new_name: &str,
        content: Option<&str>,
    ) -> Result<LspWorkspaceEditResult, String> {
        let canonical_cwd = validate_workspace_cwd(cwd)?;
        let (canon_file, _) = resolve_workspace_file(&canonical_cwd, path)?;
        let Some(cfg) = self
            .select_config_for_file(&canonical_cwd, &canon_file)
            .await?
        else {
            return Err("No language server available for this file type".into());
        };
        let sess = self.get_or_start_session(&canonical_cwd, cfg).await?;
        let ws_edit = sess
            .rename(
                &canon_file,
                line,
                col,
                new_name,
                content,
                self.request_timeout,
            )
            .await?;
        self.apply_workspace_edit(&canonical_cwd, &ws_edit).await
    }

    /// Apply a workspace-confined `LspWorkspaceEdit` atomically and synchronize open LSP buffers.
    pub async fn apply_workspace_edit(
        &self,
        cwd: &Path,
        edit: &LspWorkspaceEdit,
    ) -> Result<LspWorkspaceEditResult, String> {
        lsp_apply_workspace_edit(Some(self), cwd, edit).await
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
        // Windows drive-letter and verbatim `\\?\` file URI round-tripping
        assert_eq!(
            path_to_file_uri(Path::new(r"\\?\C:\Users\me\my file.rs")),
            "file:///C:/Users/me/my%20file.rs"
        );
        assert_eq!(
            path_to_file_uri(Path::new(r"\\?\UNC\server\share\my file.rs")),
            "file://server/share/my%20file.rs"
        );
        assert_eq!(
            file_uri_to_path("file:///C:/Users/me/my%20file.rs"),
            Some(PathBuf::from("C:/Users/me/my file.rs"))
        );
        // Drive-relative Windows URI rejected
        assert!(file_uri_to_path("file:///C:foo.rs").is_none());
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

    #[tokio::test]
    async fn lsp_hover_references_completion_code_action_and_rename_end_to_end() {
        let base = std::env::temp_dir().join(format!("orbit-lsp-full-{}", uuid::Uuid::new_v4()));
        let ws = base.join("repo");
        let outside = base.join("outside.rs");
        std::fs::create_dir_all(ws.join("src")).expect("create src");
        std::fs::write(
            ws.join("src/lib.rs"),
            "pub fn old_name(x: u32) -> u32 {\n    x + 1\n}\n",
        )
        .expect("write lib.rs");
        std::fs::write(
            ws.join("src/main.rs"),
            "fn main() {\n    let v = old_name(41);\n    println!(\"{v}\");\n}\n",
        )
        .expect("write main.rs");
        std::fs::write(&outside, "pub fn old_name() {}\n").expect("write outside");

        let canon_cwd = validate_workspace_cwd(&ws).expect("canon cwd");
        let (canon_lib, rel_lib) = resolve_workspace_file(&canon_cwd, "src/lib.rs").expect("lib");
        let (canon_main, _) = resolve_workspace_file(&canon_cwd, "src/main.rs").expect("main");
        let lib_uri = path_to_file_uri(&canon_lib);
        let main_uri = path_to_file_uri(&canon_main);
        let outside_uri = path_to_file_uri(
            &crate::platform::canonicalize_clean(&outside).expect("outside canon"),
        );

        let (client_read, server_write) = tokio::io::duplex(64 * 1024);
        let (server_read, client_write) = tokio::io::duplex(64 * 1024);

        let lib_uri_srv = lib_uri.clone();
        let main_uri_srv = main_uri.clone();
        let outside_uri_srv = outside_uri.clone();

        let server_handle = tokio::spawn(async move {
            let mut reader = BufReader::new(server_read);
            let mut writer = server_write;
            let mut seen_methods = Vec::new();
            let mut seen_did_save_uris = Vec::new();

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
                                "id": id,
                                "result": {
                                    "capabilities": {
                                        "hoverProvider": true,
                                        "referencesProvider": true,
                                        "completionProvider": { "triggerCharacters": [".", ":"] },
                                        "codeActionProvider": true,
                                        "renameProvider": true
                                    }
                                }
                            }),
                        )
                        .await;
                    }
                    "textDocument/didSave" => {
                        if let Some(u) = msg
                            .get("params")
                            .and_then(|p| p.get("textDocument"))
                            .and_then(|td| td.get("uri"))
                            .and_then(Value::as_str)
                        {
                            seen_did_save_uris.push(u.to_string());
                        }
                    }
                    "textDocument/hover" => {
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "contents": [
                                        { "language": "rust", "value": "pub fn old_name(x: u32) -> u32" },
                                        "Increments `x` by one."
                                    ],
                                    "range": {
                                        "start": { "line": 0, "character": 7 },
                                        "end": { "line": 0, "character": 15 }
                                    }
                                }
                            }),
                        )
                        .await;
                    }
                    "textDocument/references" => {
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": [
                                    {
                                        "uri": lib_uri_srv,
                                        "range": {
                                            "start": { "line": 0, "character": 7 },
                                            "end": { "line": 0, "character": 15 }
                                        }
                                    },
                                    {
                                        "uri": main_uri_srv,
                                        "range": {
                                            "start": { "line": 1, "character": 12 },
                                            "end": { "line": 1, "character": 20 }
                                        }
                                    },
                                    {
                                        // Must be filtered out by strict workspace confinement!
                                        "uri": outside_uri_srv,
                                        "range": {
                                            "start": { "line": 0, "character": 7 },
                                            "end": { "line": 0, "character": 15 }
                                        }
                                    }
                                ]
                            }),
                        )
                        .await;
                    }
                    "textDocument/completion" => {
                        let mut items = Vec::new();
                        items.push(json!({
                            "label": "old_name",
                            "kind": 3,
                            "detail": "fn(u32) -> u32",
                            "documentation": { "kind": "markdown", "value": "Increment helper" },
                            "insertText": "old_name(${1:x})$0",
                            "insertTextFormat": 2,
                            "sortText": "0000",
                            "filterText": "old_name"
                        }));
                        // Add 95 extra items to verify strict 80-item bound
                        for i in 1..=95 {
                            items.push(json!({
                                "label": format!("item_{i:03}"),
                                "kind": 6,
                                "sortText": format!("{i:04}")
                            }));
                        }
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "isIncomplete": true,
                                    "items": items
                                }
                            }),
                        )
                        .await;
                    }
                    "textDocument/codeAction" => {
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": [
                                    {
                                        "title": "Escape workspace evil fix",
                                        "kind": "quickfix",
                                        "isPreferred": true,
                                        "edit": {
                                            "changes": {
                                                outside_uri_srv.clone(): [{
                                                    "range": {
                                                        "start": { "line": 0, "character": 0 },
                                                        "end": { "line": 0, "character": 3 }
                                                    },
                                                    "newText": "bad"
                                                }]
                                            }
                                        }
                                    },
                                    {
                                        "title": "Add #[must_use]",
                                        "kind": "quickfix",
                                        "isPreferred": true,
                                        "edit": {
                                            "changes": {
                                                lib_uri_srv.clone(): [{
                                                    "range": {
                                                        "start": { "line": 0, "character": 0 },
                                                        "end": { "line": 0, "character": 0 }
                                                    },
                                                    "newText": "#[must_use]\n"
                                                }]
                                            }
                                        }
                                    },
                                    {
                                        "title": "Disabled action",
                                        "disabled": { "reason": "not applicable" }
                                    },
                                    {
                                        "title": "Run cargo check",
                                        "command": "rust-analyzer.runCheck",
                                        "arguments": ["all"]
                                    }
                                ]
                            }),
                        )
                        .await;
                    }
                    "textDocument/rename" => {
                        let new_name = msg
                            .get("params")
                            .and_then(|p| p.get("newName"))
                            .and_then(Value::as_str)
                            .unwrap_or("renamed");
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "documentChanges": [
                                        {
                                            "textDocument": { "uri": lib_uri_srv, "version": 1 },
                                            "edits": [{
                                                "range": {
                                                    "start": { "line": 0, "character": 7 },
                                                    "end": { "line": 0, "character": 15 }
                                                },
                                                "newText": new_name
                                            }]
                                        },
                                        {
                                            "textDocument": { "uri": main_uri_srv, "version": 1 },
                                            "edits": [{
                                                "range": {
                                                    "start": { "line": 1, "character": 12 },
                                                    "end": { "line": 1, "character": 20 }
                                                },
                                                "newText": new_name
                                            }]
                                        }
                                    ]
                                }
                            }),
                        )
                        .await;
                    }
                    "shutdown" => {
                        let _ = write_lsp_message(
                            &mut writer,
                            &json!({ "jsonrpc": "2.0", "id": id, "result": Value::Null }),
                        )
                        .await;
                    }
                    "exit" => break,
                    _ => {}
                }
            }
            (seen_methods, seen_did_save_uris)
        });

        let cfg = LspServerConfig {
            id: "mock-full".into(),
            name: "Mock Full LS".into(),
            command: "mock-full".into(),
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
            canon_cwd.clone(),
            BufReader::new(client_read),
            Box::new(client_write),
            None,
            Duration::from_secs(3),
        )
        .await
        .expect("init full mock session");

        // 1. textDocument/hover
        let hover = session
            .hover(&canon_lib, 1, 8, None, Duration::from_secs(3))
            .await
            .expect("hover ok")
            .expect("hover present");
        assert_eq!(hover.format, "markdown");
        assert!(
            hover
                .contents
                .contains("```rust\npub fn old_name(x: u32) -> u32\n```")
        );
        assert!(hover.contents.contains("Increments `x` by one."));
        assert_eq!(
            hover.range,
            Some(LspRange {
                line: 1,
                col: 8,
                end_line: 1,
                end_col: 16
            })
        );

        // 2. textDocument/references (outside workspace URI strictly excluded)
        let refs = session
            .references(&canon_lib, 1, 8, true, None, Duration::from_secs(3))
            .await
            .expect("references ok");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].path, "src/lib.rs");
        assert_eq!(refs[0].line, 1);
        assert_eq!(refs[0].character, 8);
        assert_eq!(refs[0].end_character, 16);
        assert_eq!(
            refs[0].preview_line.as_deref(),
            Some("pub fn old_name(x: u32) -> u32 {")
        );
        assert_eq!(refs[1].path, "src/main.rs");
        assert_eq!(refs[1].line, 2);
        assert_eq!(refs[1].character, 13);
        assert_eq!(
            refs[1].preview_line.as_deref(),
            Some("let v = old_name(41);")
        );

        // 3. textDocument/completion (snippet cleaned + capped to 80 items)
        let comps = session
            .completion(&canon_main, 2, 14, Some("."), None, Duration::from_secs(3))
            .await
            .expect("completion ok");
        assert_eq!(comps.len(), MAX_COMPLETION_ITEMS);
        assert_eq!(comps[0].label, "old_name");
        assert_eq!(comps[0].kind, "function");
        assert_eq!(comps[0].detail.as_deref(), Some("fn(u32) -> u32"));
        assert_eq!(comps[0].documentation.as_deref(), Some("Increment helper"));
        assert_eq!(comps[0].insert_text.as_deref(), Some("old_name(x)"));

        // 4. textDocument/codeAction (escaping action dropped, disabled skipped, preferred first)
        let actions = session
            .code_action(
                &canon_lib,
                &rel_lib,
                LspRange {
                    line: 1,
                    col: 1,
                    end_line: 1,
                    end_col: 15,
                },
                &[],
                None,
                Duration::from_secs(3),
            )
            .await
            .expect("codeAction ok");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].title, "Add #[must_use]");
        assert!(actions[0].is_preferred);
        assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
        assert!(actions[0].edit.is_some());
        assert_eq!(actions[1].title, "Run cargo check");
        assert_eq!(
            actions[1].command.as_ref().map(|c| c.command.as_str()),
            Some("rust-analyzer.runCheck")
        );

        // 5. textDocument/rename + atomic lsp_apply_workspace_edit + didChange/didSave sync
        let mgr = LspManager::with_timeouts(
            base.clone(),
            Duration::from_secs(60),
            Duration::from_secs(3),
            Duration::from_secs(3),
        );
        mgr.insert_test_session(canon_cwd.clone(), Arc::clone(&session))
            .await;

        let ws_edit = session
            .rename(&canon_lib, 1, 8, "new_name", None, Duration::from_secs(3))
            .await
            .expect("rename workspace edit");
        assert_eq!(ws_edit.file_edits.len(), 2);

        let applied = lsp_apply_workspace_edit(Some(&mgr), &canon_cwd, &ws_edit)
            .await
            .expect("apply workspace edit");
        assert!(applied.applied);
        assert_eq!(applied.total_edits, 2);
        assert_eq!(applied.modified_files, vec!["src/lib.rs", "src/main.rs"]);

        let lib_after = std::fs::read_to_string(ws.join("src/lib.rs")).expect("read lib");
        let main_after = std::fs::read_to_string(ws.join("src/main.rs")).expect("read main");
        assert!(lib_after.starts_with("pub fn new_name(x: u32) -> u32 {"));
        assert!(main_after.contains("let v = new_name(41);"));

        mgr.shutdown_all().await;
        let (seen_methods, did_save_uris) = server_handle.await.expect("server exit");
        assert!(seen_methods.iter().any(|m| m == "textDocument/hover"));
        assert!(seen_methods.iter().any(|m| m == "textDocument/references"));
        assert!(seen_methods.iter().any(|m| m == "textDocument/completion"));
        assert!(seen_methods.iter().any(|m| m == "textDocument/codeAction"));
        assert!(seen_methods.iter().any(|m| m == "textDocument/rename"));
        assert!(did_save_uris.contains(&lib_uri));
        assert!(did_save_uris.contains(&main_uri));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn lsp_apply_workspace_edit_confinement_utf16_and_atomicity() {
        let base = std::env::temp_dir().join(format!("orbit-lsp-wse-{}", uuid::Uuid::new_v4()));
        let ws = base.join("repo");
        let outside = base.join("secret.rs");
        std::fs::create_dir_all(ws.join("src")).expect("create src");
        std::fs::create_dir_all(ws.join(".git")).expect("create .git");
        std::fs::write(ws.join("src/a.rs"), "let s = \"🚀\"; let foo = 1;\r\n").expect("write a");
        std::fs::write(ws.join("src/b.rs"), "const B: i32 = 2;\n").expect("write b");
        std::fs::write(ws.join(".git/config"), "[core]\n").expect("write git");
        std::fs::write(&outside, "secret\n").expect("write secret");
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(ws.join("src/a.rs"), ws.join("src/link_a.rs"));
            let _ = std::os::unix::fs::symlink(&outside, ws.join("src/link_out.rs"));
        }

        let canon_cwd = validate_workspace_cwd(&ws).expect("canon cwd");

        // 1. UTF-16 surrogate pair accuracy ("🚀" takes 2 UTF-16 code units!) + CRLF preservation
        // Line 1: `let s = "🚀"; let foo = 1;`
        // Cols (1-based UTF-16):
        // 1..=9: `let s = "` (9 units)
        // 10..=11: `🚀` (2 units)
        // 12..=18: `"; let ` (7 units)
        // 19..=21: `foo` (start col 19, end col 22)
        let utf16_edit = LspWorkspaceEdit {
            file_edits: vec![LspFileTextEdits {
                path: "src/a.rs".into(),
                edits: vec![LspTextEdit {
                    line: 1,
                    col: 19,
                    end_line: 1,
                    end_col: 22,
                    new_text: "bar".into(),
                }],
            }],
        };
        let res = lsp_apply_workspace_edit(None, &canon_cwd, &utf16_edit)
            .await
            .expect("utf16 edit ok");
        assert!(res.applied);
        assert_eq!(
            std::fs::read_to_string(ws.join("src/a.rs")).expect("read a"),
            "let s = \"🚀\"; let bar = 1;\r\n"
        );

        // 2. Overlapping edits rejected with zero disk mutations
        let overlap_edit = LspWorkspaceEdit {
            file_edits: vec![LspFileTextEdits {
                path: "src/b.rs".into(),
                edits: vec![
                    LspTextEdit {
                        line: 1,
                        col: 1,
                        end_line: 1,
                        end_col: 8,
                        new_text: "let".into(),
                    },
                    LspTextEdit {
                        line: 1,
                        col: 5,
                        end_line: 1,
                        end_col: 10,
                        new_text: "X".into(),
                    },
                ],
            }],
        };
        assert!(
            lsp_apply_workspace_edit(None, &canon_cwd, &overlap_edit)
                .await
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(ws.join("src/b.rs")).expect("b unchanged"),
            "const B: i32 = 2;\n"
        );

        // 3. Multi-file edit where 2nd file escapes workspace (`../secret.rs` or `.git/config`)
        // MUST reject ALL files in Phase 1 before modifying src/b.rs!
        for bad_path in ["../secret.rs", ".git/config", &outside.to_string_lossy()] {
            let evil_edit = LspWorkspaceEdit {
                file_edits: vec![
                    LspFileTextEdits {
                        path: "src/b.rs".into(),
                        edits: vec![LspTextEdit {
                            line: 1,
                            col: 7,
                            end_line: 1,
                            end_col: 8,
                            new_text: "Z".into(),
                        }],
                    },
                    LspFileTextEdits {
                        path: bad_path.to_string(),
                        edits: vec![LspTextEdit {
                            line: 1,
                            col: 1,
                            end_line: 1,
                            end_col: 2,
                            new_text: "X".into(),
                        }],
                    },
                ],
            };
            assert!(
                lsp_apply_workspace_edit(None, &canon_cwd, &evil_edit)
                    .await
                    .is_err(),
                "expected `{bad_path}` to be rejected"
            );
            assert_eq!(
                std::fs::read_to_string(ws.join("src/b.rs")).expect("b still untouched"),
                "const B: i32 = 2;\n"
            );
        }

        // 4. Symlinks (both escaping and intra-workspace) rejected in Phase 1
        #[cfg(unix)]
        for sym_path in ["src/link_out.rs", "src/link_a.rs"] {
            let sym_edit = LspWorkspaceEdit {
                file_edits: vec![LspFileTextEdits {
                    path: sym_path.into(),
                    edits: vec![LspTextEdit {
                        line: 1,
                        col: 1,
                        end_line: 1,
                        end_col: 2,
                        new_text: "X".into(),
                    }],
                }],
            };
            assert!(
                lsp_apply_workspace_edit(None, &canon_cwd, &sym_edit)
                    .await
                    .is_err(),
                "expected symlink `{sym_path}` to be rejected"
            );
        }

        // 5. Snippet placeholder stripper unit checks
        assert_eq!(
            strip_lsp_snippet_syntax("foo(${1:bar}, ${2|a,b,c|})$0"),
            "foo(bar, a)"
        );
        assert_eq!(strip_lsp_snippet_syntax(r"cost: \$100 $1"), "cost: $100 ");

        let _ = std::fs::remove_dir_all(&base);
    }
}
