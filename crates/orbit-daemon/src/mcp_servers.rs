//! Pure-Rust External MCP Server Manager & Live JSON-RPC 2.0 Probe Client.
//!
//! Zero external Node.js/Python dependencies:
//! - Manages Orbit user-scoped (`$ORBIT_HOME/mcp-servers.json`, mode `0600` on Unix) and
//!   project-scoped (`<canonical_cwd>/.mcp.json`, strict workspace containment) external MCP servers.
//! - Discovers read-only external MCP servers configured in Claude Code (`~/.claude.json`),
//!   Codex (`~/.codex/config.toml`), and Antigravity (`~/.gemini/antigravity/mcp_config.json`),
//!   skipping Orbit's own built-in `orbit` MCP entry.
//! - Masks secret env vars & HTTP headers before returning snapshots to any client, and supports
//!   `keep_existing_secrets` so editing non-secret fields or importing read-only servers never
//!   leaks or clobbers credentials while allowing explicit deletion of removed secret keys.
//! - Probes both `stdio` and Streamable HTTP/SSE MCP servers via JSON-RPC 2.0
//!   (`initialize` -> `notifications/initialized` -> `tools/list`) with strict 6-second timeout,
//!   process-group cleanup, dangerous env stripping, DNS-pinned SSRF guardrails, disabled
//!   cross-origin redirects, incremental SSE parsing, and 2 MiB body caps.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use orbit_protocol::{
    ExternalMcpServerRecord, McpProbeResult, McpServerScope, McpToolSummary, McpTransportKind,
    McpWriteScope,
};
use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use uuid::Uuid;

use crate::workspace_files::canonicalize_workspace_root;

pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";
pub const MCP_PROBE_TIMEOUT: Duration = Duration::from_secs(6);
pub const MAX_MCP_PROBE_TOOLS: usize = 200;
pub const MAX_MCP_PROBE_CACHE_ENTRIES: usize = 128;
pub const MAX_HTTP_PROBE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDIO_PROBE_STDOUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_STDIO_PROBE_STDERR_BYTES: usize = 8 * 1024;
const MAX_TOOL_DESC_CHARS: usize = 500;
const MAX_CONFIG_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_SERVER_NAME_LEN: usize = 64;
const MAX_ENV_ENTRIES: usize = 128;
const MAX_HEADER_ENTRIES: usize = 64;
const MAX_URL_LEN: usize = 2048;

const SECRET_KEY_MARKERS: &[&str] = &[
    "TOKEN",
    "SECRET",
    "KEY",
    "PASS",
    "AUTH",
    "CREDENTIAL",
    "COOKIE",
];

const BLOCKED_EXACT_ENV_KEYS: &[&str] = &[
    "NODE_OPTIONS",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PYTHONHOME",
    "BASH_ENV",
    "ENV",
    "IFS",
    "PROMPT_COMMAND",
    "GIT_SSH_COMMAND",
    "GIT_EXEC_PATH",
    "PERL5OPT",
    "RUBYOPT",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "GCONV_PATH",
    "GLIBC_TUNABLES",
];

/// Internal resolved server entry including unmasked env/headers (never serialized over the wire).
#[derive(Debug, Clone)]
pub(crate) struct ResolvedExternalMcpServer {
    pub id: String,
    pub name: String,
    pub scope: McpServerScope,
    pub transport: ResolvedTransport,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
    pub editable: bool,
    pub source_path: String,
}

#[derive(Debug, Clone)]
pub(crate) enum ResolvedTransport {
    Stdio { command: String, args: Vec<String> },
    StreamableHttp { url: String },
}

/// Request payload for upserting an external MCP server.
#[derive(Debug, Clone)]
pub struct UpsertExternalMcpInput {
    pub scope: McpWriteScope,
    pub cwd: Option<PathBuf>,
    pub name: String,
    pub transport: McpTransportKind,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub keep_existing_secrets: bool,
    pub enabled: bool,
}

/// Build a canonical probe cache key, namespacing `project_dot_mcp:*` entries by canonical `cwd`
/// so different workspaces never collide.
pub fn probe_cache_key(server_id: &str, canonical_cwd: Option<&Path>) -> String {
    if server_id.starts_with("project_dot_mcp:")
        && let Some(dir) = canonical_cwd
    {
        return format!("{}@{server_id}", dir.to_string_lossy());
    }
    server_id.to_string()
}

/// Insert a probe result into the bounded `mcp_probe_cache` (max 128 entries, evicting oldest
/// `probed_at_ms`), skipping transient "not found" errors.
pub fn insert_probe_cache(
    cache: &mut HashMap<String, McpProbeResult>,
    key: String,
    result: McpProbeResult,
) {
    if !result.ok
        && result
            .error
            .as_deref()
            .is_some_and(|e| e.to_ascii_lowercase().contains("not found"))
    {
        return;
    }
    cache.insert(key, result);
    while cache.len() > MAX_MCP_PROBE_CACHE_ENTRIES {
        let oldest_key = cache
            .iter()
            .min_by_key(|(k, v)| (v.probed_at_ms, k.as_str()))
            .map(|(k, _)| k.clone());
        match oldest_key {
            Some(k) => {
                cache.remove(&k);
            }
            None => break,
        }
    }
}

/// Returns `true` when an environment variable or HTTP header name likely carries a secret.
pub fn is_secret_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    SECRET_KEY_MARKERS.iter().any(|m| upper.contains(m))
}

/// Mask a secret value for safe display in clients (`••••abcd` for >8 chars, `••••` otherwise).
pub fn mask_secret_value(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = value.chars().collect();
    if chars.len() > 8 {
        let suffix: String = chars[chars.len() - 4..].iter().collect();
        format!("••••{suffix}")
    } else {
        "••••".to_string()
    }
}

/// Returns `true` when a submitted value is a masked placeholder echoed back by a UI client.
pub fn is_masked_placeholder(value: &str) -> bool {
    value.starts_with("••••")
}

/// Mask a map of env vars or headers so secret values never cross the daemon wire boundary.
pub fn mask_kv_map(raw: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    raw.iter()
        .map(|(k, v)| {
            let masked = if is_secret_key(k) {
                mask_secret_value(v)
            } else {
                v.clone()
            };
            (k.clone(), masked)
        })
        .collect()
}

/// Returns `true` if an env var key is dangerous for spawning stdio MCP probes.
/// Rejects dynamic linker injection (`LD_*`, `DYLD_*`), internal Orbit variables (`ORBIT_*`),
/// and runtime code-injection / shell-hook env variables (`NODE_OPTIONS`, `PYTHONSTARTUP`,
/// `PYTHONPATH`, `PYTHONHOME`, `BASH_ENV`, `ENV`, `IFS`, `PROMPT_COMMAND`, `GIT_SSH_COMMAND`,
/// `GIT_EXEC_PATH`, `PERL5OPT`, `RUBYOPT`, `JAVA_TOOL_OPTIONS`, `_JAVA_OPTIONS`, `GCONV_PATH`,
/// `GLIBC_TUNABLES`).
pub fn is_blocked_mcp_env_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    upper == "LD_PRELOAD"
        || upper.starts_with("LD_")
        || upper == "DYLD_INSERT_LIBRARIES"
        || upper.starts_with("DYLD_")
        || upper.starts_with("ORBIT_")
        || BLOCKED_EXACT_ENV_KEYS.contains(&upper.as_str())
}

/// Return a copy of `env` with all dangerous loader, hook, and `ORBIT_*` keys stripped.
pub fn sanitize_stdio_env(env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    env.iter()
        .filter(|(k, _)| !is_blocked_mcp_env_key(k))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn is_forbidden_http_header_name(key: &str) -> bool {
    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "host" | "content-length" | "transfer-encoding"
    )
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn make_server_id(scope: McpServerScope, name: &str) -> String {
    format!("{}:{name}", scope.id_prefix())
}

fn validate_server_name(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("MCP server name cannot be empty".to_string());
    }
    if trimmed.len() > MAX_SERVER_NAME_LEN {
        return Err(format!(
            "MCP server name exceeds {MAX_SERVER_NAME_LEN} characters"
        ));
    }
    if trimmed.eq_ignore_ascii_case("orbit") {
        return Err("Server name `orbit` is reserved for Orbit's built-in MCP server".to_string());
    }
    let mut chars = trimmed.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !first_ok || !rest_ok || trimmed == "." || trimmed == ".." {
        return Err(
            "Server name must start with a letter/digit/`_` and contain only ASCII letters, digits, `.`, `_`, or `-`"
                .to_string(),
        );
    }
    Ok(trimmed.to_string())
}

fn validate_env_map(env: &BTreeMap<String, String>) -> Result<BTreeMap<String, String>, String> {
    if env.len() > MAX_ENV_ENTRIES {
        return Err(format!(
            "Too many environment variables (max {MAX_ENV_ENTRIES})"
        ));
    }
    let mut clean = BTreeMap::new();
    for (k, v) in env {
        let key = k.trim();
        if key.is_empty() || key.len() > 128 {
            return Err(format!("Invalid environment variable name `{k}`"));
        }
        let mut chars = key.chars();
        let first_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        let rest_ok = chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !first_ok || !rest_ok {
            return Err(format!("Invalid environment variable name `{key}`"));
        }
        if is_blocked_mcp_env_key(key) {
            return Err(format!(
                "Environment variable `{key}` is blocked for security reasons"
            ));
        }
        if v.contains('\0') {
            return Err(format!("Environment variable `{key}` contains NUL byte"));
        }
        clean.insert(key.to_string(), v.clone());
    }
    Ok(clean)
}

fn validate_headers_map(
    headers: &BTreeMap<String, String>,
    allow_masked_placeholders: bool,
) -> Result<BTreeMap<String, String>, String> {
    if headers.len() > MAX_HEADER_ENTRIES {
        return Err(format!("Too many HTTP headers (max {MAX_HEADER_ENTRIES})"));
    }
    let mut clean = BTreeMap::new();
    for (k, v) in headers {
        let key = k.trim();
        if key.is_empty() || key.len() > 128 {
            return Err(format!("Invalid HTTP header name `{k}`"));
        }
        if HeaderName::from_bytes(key.as_bytes()).is_err() {
            return Err(format!("Invalid HTTP header name `{key}`"));
        }
        if is_forbidden_http_header_name(key) {
            return Err(format!("HTTP header `{key}` cannot be overridden"));
        }
        if v.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
            return Err(format!(
                "HTTP header `{key}` contains invalid control characters"
            ));
        }
        let skip_value_parse = allow_masked_placeholders
            && is_secret_key(key)
            && (v.is_empty() || is_masked_placeholder(v));
        if !skip_value_parse && !v.is_empty() && HeaderValue::from_str(v).is_err() {
            return Err(format!("Invalid value for HTTP header `{key}`"));
        }
        clean.insert(key.to_string(), v.clone());
    }
    Ok(clean)
}

/// Validate an external Streamable HTTP MCP URL.
///
/// Transport security policy:
/// - External MCP servers are explicitly configured by the authenticated host owner, so loopback
///   and private LAN MCP servers (e.g. `http://127.0.0.1:8080/mcp`, `http://localhost:3000/mcp`)
///   are explicitly permitted alongside remote `https://` MCP endpoints.
/// - Non-HTTP(S) schemes (`file://`, `ftp://`, `gopher://`, etc.), embedded URL credentials,
///   unspecified bind targets (`0.0.0.0`, `::`), and cloud instance metadata / link-local SSRF
///   endpoints (`169.254.0.0/16`, `fe80::/10`, `fd00:ec2::254`, `100.100.100.200`, IPv4-mapped,
///   IPv4-compatible `::169.254.169.254`, NAT64 `64:ff9b::/96` wrapping blocked IPv4,
///   `metadata.google.internal`, `metadata.goog`) are unconditionally rejected — both at URL
///   parse time AND after DNS resolution via IP pinning with redirects disabled.
pub fn validate_mcp_http_url(raw: &str) -> Result<reqwest::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Streamable HTTP URL cannot be empty".to_string());
    }
    if trimmed.len() > MAX_URL_LEN {
        return Err(format!("URL exceeds {MAX_URL_LEN} characters"));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("URL contains invalid whitespace or control characters".to_string());
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid Streamable HTTP URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "Unsupported URL scheme `{}` (only http:// and https:// are allowed)",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(
            "URL must not embed credentials; configure Authorization headers instead".to_string(),
        );
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Streamable HTTP URL is missing a host".to_string())?;
    let host_norm = normalize_url_host(host_raw);

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

    Ok(parsed)
}

fn normalize_url_host(host_raw: &str) -> String {
    host_raw
        .trim_end_matches('.')
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host_raw.trim_end_matches('.'))
        .to_ascii_lowercase()
}

pub(crate) fn is_blocked_ssrf_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                // Alibaba Cloud metadata endpoint
                || oct == [100, 100, 100, 200]
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_ssrf_ip(IpAddr::V4(mapped));
            }
            // Deprecated IPv4-compatible IPv6 addresses (e.g. ::169.254.169.254),
            // explicitly excluding ::1 loopback.
            if !v6.is_loopback()
                && let Some(compat_v4) = v6.to_ipv4()
                && is_blocked_ssrf_ip(IpAddr::V4(compat_v4))
            {
                return true;
            }
            let seg = v6.segments();
            // NAT64 well-known prefix 64:ff9b::/96 wrapping an IPv4 address
            if seg[0] == 0x0064
                && seg[1] == 0xff9b
                && seg[2] == 0
                && seg[3] == 0
                && seg[4] == 0
                && seg[5] == 0
            {
                let nat64_v4 = Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    (seg[6] & 0xff) as u8,
                    (seg[7] >> 8) as u8,
                    (seg[7] & 0xff) as u8,
                );
                if is_blocked_ssrf_ip(IpAddr::V4(nat64_v4)) {
                    return true;
                }
            }
            v6.is_unspecified()
                || v6.is_multicast()
                // Link-local fe80::/10
                || (seg[0] & 0xffc0) == 0xfe80
                // AWS EC2 IMDSv2 IPv6 endpoint fd00:ec2::254
                || (seg[0] == 0xfd00
                    && seg[1] == 0x0ec2
                    && seg[2] == 0
                    && seg[3] == 0
                    && seg[4] == 0
                    && seg[5] == 0
                    && seg[6] == 0
                    && seg[7] == 0x0254)
        }
    }
}

/// Resolve and validate every IP returned by DNS for `parsed_url`, preventing DNS rebinding and
/// hostname-based metadata/link-local SSRF when pinned on `reqwest::ClientBuilder`.
pub(crate) async fn resolve_and_validate_http_addrs(
    parsed_url: &reqwest::Url,
) -> Result<(String, Vec<SocketAddr>), String> {
    let host_raw = parsed_url
        .host_str()
        .ok_or_else(|| "Streamable HTTP URL is missing a host".to_string())?;
    let host_norm = normalize_url_host(host_raw);
    let port = parsed_url
        .port_or_known_default()
        .unwrap_or(if parsed_url.scheme() == "https" {
            443
        } else {
            80
        });

    if let Ok(ip) = host_norm.parse::<IpAddr>() {
        if is_blocked_ssrf_ip(ip) {
            return Err(format!(
                "Refusing to connect to link-local, metadata, or unspecified IP `{ip}`"
            ));
        }
        return Ok((host_norm, vec![SocketAddr::new(ip, port)]));
    }

    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host_norm.as_str(), port))
        .await
        .map_err(|e| format!("Failed to resolve host `{host_norm}`: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!(
            "DNS lookup returned no addresses for `{host_norm}`"
        ));
    }
    for addr in &addrs {
        if is_blocked_ssrf_ip(addr.ip()) {
            return Err(format!(
                "Refusing to connect: `{host_norm}` resolved to blocked IP `{}`",
                addr.ip()
            ));
        }
    }
    Ok((host_norm, addrs))
}

fn validate_transport(transport: &McpTransportKind) -> Result<(), String> {
    match transport {
        McpTransportKind::Stdio { command, args } => {
            let cmd = command.trim();
            if cmd.is_empty() {
                return Err("Stdio command cannot be empty".to_string());
            }
            if cmd.chars().any(|c| c == '\0' || c.is_control()) {
                return Err("Stdio command contains invalid control characters".to_string());
            }
            if args.len() > 128 {
                return Err("Too many stdio command arguments (max 128)".to_string());
            }
            for arg in args {
                if arg.contains('\0') {
                    return Err("Stdio argument contains NUL byte".to_string());
                }
            }
            Ok(())
        }
        McpTransportKind::StreamableHttp { url, .. } => {
            validate_mcp_http_url(url)?;
            Ok(())
        }
    }
}

fn resolve_user_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Strictly validate a project workspace directory and resolve `<canonical_cwd>/.mcp.json`.
/// Rejects raw `..` traversal components, non-directories, filesystem/`$HOME` root, `.git`,
/// and any `.mcp.json` symlink that escapes `canonical_cwd` (or any symlink when `for_write` is true).
pub fn resolve_project_dot_mcp_path(cwd: &Path, for_write: bool) -> Result<PathBuf, String> {
    if let Some(raw) = cwd.to_str()
        && raw.chars().any(|c| c == '\0' || c.is_control())
    {
        return Err("Workspace path contains invalid control characters".to_string());
    }
    if cwd.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Workspace path must not contain `..` traversal segments".to_string());
    }
    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let target = canonical_cwd.join(".mcp.json");
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            if for_write {
                return Err("Refusing to write through a symlinked `.mcp.json`".to_string());
            }
            let canon_target = fs::canonicalize(&target)
                .map_err(|e| format!("Invalid `.mcp.json` symlink: {e}"))?;
            if !canon_target.starts_with(&canonical_cwd) {
                return Err("`.mcp.json` symlink escapes the workspace directory".to_string());
            }
        } else if !meta.is_file() {
            return Err("`.mcp.json` exists but is not a regular file".to_string());
        }
    }
    Ok(target)
}

fn orbit_user_mcp_path(home_dir: &Path) -> PathBuf {
    home_dir.join("mcp-servers.json")
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

fn write_json_atomic(path: &Path, value: &Value, private_mode: bool) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid config path `{}`", path.display()))?;
    fs::create_dir_all(parent).map_err(|e| format!("Failed to create config directory: {e}"))?;

    let tmp_name = format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("mcp-servers"),
        Uuid::new_v4().simple()
    );
    let tmp_path = parent.join(tmp_name);
    let mut guard = TempFileGuard::new(tmp_path.clone());

    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("Failed to serialize MCP config JSON: {e}"))?;
    bytes.push(b'\n');

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
        .map_err(|e| format!("Failed to create temporary MCP config file: {e}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("Failed to write MCP config file: {e}"))?;
    drop(file);

    #[cfg(unix)]
    if private_mode {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("Failed to set 0600 permissions on MCP config: {e}"))?;
    }

    fs::rename(&tmp_path, path).map_err(|e| format!("Failed to save `{}`: {e}", path.display()))?;
    guard.disarm();
    Ok(())
}

/// Returns `true` if ANY server entry in `root["mcpServers"]` contains a secret env var or header key.
fn json_root_has_secrets(root: &Value) -> bool {
    let Some(servers_obj) = root.get("mcpServers").and_then(Value::as_object) else {
        return false;
    };
    for entry in servers_obj.values() {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        for field in ["env", "headers", "httpHeaders", "http_headers"] {
            if let Some(map) = obj.get(field).and_then(Value::as_object)
                && map.keys().any(|k| is_secret_key(k))
            {
                return true;
            }
        }
    }
    false
}

fn read_bounded_text(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CONFIG_FILE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn is_builtin_orbit_mcp_entry(name: &str, transport: &ResolvedTransport) -> bool {
    if name.trim().eq_ignore_ascii_case("orbit") {
        return true;
    }
    match transport {
        ResolvedTransport::StreamableHttp { url } => {
            if let Ok(parsed) = reqwest::Url::parse(url)
                && parsed.path().trim_end_matches('/') == "/mcp/agents"
                && matches!(
                    parsed.host_str(),
                    Some("127.0.0.1" | "localhost" | "::1" | "[::1]")
                )
            {
                return true;
            }
            false
        }
        ResolvedTransport::Stdio { command, args } => {
            let bin_name = Path::new(command)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(command);
            bin_name.eq_ignore_ascii_case("orbit")
                && args.first().map(String::as_str) == Some("mcp")
        }
    }
}

fn parse_string_map(val: Option<&Value>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Some(obj) = val.and_then(Value::as_object) else {
        return map;
    };
    for (k, v) in obj {
        if let Some(s) = v.as_str() {
            map.insert(k.clone(), s.to_string());
        } else if v.is_number() || v.is_boolean() {
            map.insert(k.clone(), v.to_string());
        }
    }
    map
}

fn parse_json_mcp_servers_value(
    root: &Value,
    scope: McpServerScope,
    source_path: &Path,
    skip_orbit_builtin: bool,
) -> Vec<ResolvedExternalMcpServer> {
    let Some(servers_obj) = root.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    let source_str = source_path.to_string_lossy().to_string();
    let mut out = Vec::new();

    for (raw_name, entry) in servers_obj {
        let name = raw_name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            continue;
        }
        let Some(obj) = entry.as_object() else {
            continue;
        };

        let url_opt = obj
            .get("url")
            .or_else(|| obj.get("serverUrl"))
            .or_else(|| obj.get("httpUrl"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let command_opt = obj
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let transport = if let Some(url) = url_opt {
            ResolvedTransport::StreamableHttp { url }
        } else if let Some(command) = command_opt {
            let args = obj
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            ResolvedTransport::Stdio { command, args }
        } else {
            continue;
        };

        if skip_orbit_builtin && is_builtin_orbit_mcp_entry(name, &transport) {
            continue;
        }

        let env = parse_string_map(obj.get("env"));
        let headers = parse_string_map(
            obj.get("headers")
                .or_else(|| obj.get("httpHeaders"))
                .or_else(|| obj.get("http_headers")),
        );

        let enabled = if let Some(en) = obj.get("enabled").and_then(Value::as_bool) {
            en
        } else if let Some(dis) = obj.get("disabled").and_then(Value::as_bool) {
            !dis
        } else {
            true
        };

        out.push(ResolvedExternalMcpServer {
            id: make_server_id(scope, name),
            name: name.to_string(),
            scope,
            transport,
            env,
            headers,
            enabled,
            editable: scope.is_editable(),
            source_path: source_str.clone(),
        });
    }

    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

fn has_unclosed_toml_array(val: &str) -> bool {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut depth = 0i32;
    for ch in val.chars() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '[' => depth += 1,
                ']' => depth -= 1,
                _ => {}
            }
        }
    }
    depth > 0
}

/// Pure-Rust subset TOML parser for Codex `~/.codex/config.toml` `[mcp_servers.*]` tables.
/// Supports `[mcp_servers.<name>]`, subtables `.env` / `.headers` / `.http_headers`,
/// inline tables, dotted keys (`env.KEY = ...`, `http_headers."X-Api-Key" = ...`),
/// and multiline `args = [ ... ]`.
fn parse_codex_toml_mcp_servers(
    toml_text: &str,
    source_path: &Path,
) -> Vec<ResolvedExternalMcpServer> {
    #[derive(Default)]
    struct CodexEntry {
        command: Option<String>,
        args: Vec<String>,
        url: Option<String>,
        env: BTreeMap<String, String>,
        headers: BTreeMap<String, String>,
        enabled: Option<bool>,
    }

    enum SectionTarget {
        Ignore,
        Server(String),
        ServerEnv(String),
        ServerHeaders(String),
    }

    let mut entries: BTreeMap<String, CodexEntry> = BTreeMap::new();
    let mut section = SectionTarget::Ignore;
    let mut pending_multiline_args: Option<(String, String)> = None;

    for raw_line in toml_text.lines() {
        let line = strip_toml_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some((srv_name, mut acc)) = pending_multiline_args.take() {
            acc.push(' ');
            acc.push_str(line);
            if has_unclosed_toml_array(&acc) {
                pending_multiline_args = Some((srv_name, acc));
            } else if let Some(arr) = parse_toml_string_array(&acc) {
                entries.entry(srv_name).or_default().args = arr;
            }
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
            let header = &line[1..line.len() - 1];
            let segs = parse_toml_table_path(header);
            section = match segs.as_slice() {
                [root, name] if root == "mcp_servers" && !name.is_empty() => {
                    entries.entry(name.clone()).or_default();
                    SectionTarget::Server(name.clone())
                }
                [root, name, sub] if root == "mcp_servers" && !name.is_empty() && sub == "env" => {
                    entries.entry(name.clone()).or_default();
                    SectionTarget::ServerEnv(name.clone())
                }
                [root, name, sub]
                    if root == "mcp_servers"
                        && !name.is_empty()
                        && matches!(sub.as_str(), "headers" | "http_headers") =>
                {
                    entries.entry(name.clone()).or_default();
                    SectionTarget::ServerHeaders(name.clone())
                }
                _ => SectionTarget::Ignore,
            };
            continue;
        }

        let Some((raw_k, raw_v)) = split_toml_kv(line) else {
            continue;
        };
        let key_path = parse_toml_table_path(raw_k.trim());
        let val = raw_v.trim();
        if key_path.is_empty() || val.is_empty() {
            continue;
        }

        match &section {
            SectionTarget::Ignore => {}
            SectionTarget::Server(name) => {
                let entry = entries.entry(name.clone()).or_default();
                if key_path.len() == 2 {
                    let prefix = key_path[0].as_str();
                    let sub_key = &key_path[1];
                    if !sub_key.is_empty()
                        && let Some(s) = parse_toml_scalar_as_string(val)
                    {
                        if prefix == "env" {
                            entry.env.insert(sub_key.clone(), s);
                        } else if matches!(prefix, "headers" | "http_headers") {
                            entry.headers.insert(sub_key.clone(), s);
                        }
                    }
                    continue;
                }
                let key = key_path[0].as_str();
                match key {
                    "command" => {
                        if let Some(s) = parse_toml_string(val) {
                            entry.command = Some(s);
                        }
                    }
                    "url" => {
                        if let Some(s) = parse_toml_string(val) {
                            entry.url = Some(s);
                        }
                    }
                    "args" => {
                        if val.starts_with('[') && has_unclosed_toml_array(val) {
                            pending_multiline_args = Some((name.clone(), val.to_string()));
                        } else if let Some(arr) = parse_toml_string_array(val) {
                            entry.args = arr;
                        }
                    }
                    "enabled" => {
                        if val == "true" {
                            entry.enabled = Some(true);
                        } else if val == "false" {
                            entry.enabled = Some(false);
                        }
                    }
                    "env" => {
                        if let Some(tbl) = parse_toml_inline_table(val) {
                            entry.env.extend(tbl);
                        }
                    }
                    "headers" | "http_headers" => {
                        if let Some(tbl) = parse_toml_inline_table(val) {
                            entry.headers.extend(tbl);
                        }
                    }
                    _ => {}
                }
            }
            SectionTarget::ServerEnv(name) => {
                let key = unquote_toml_key(raw_k.trim());
                if !key.is_empty()
                    && let Some(s) = parse_toml_scalar_as_string(val)
                {
                    entries.entry(name.clone()).or_default().env.insert(key, s);
                }
            }
            SectionTarget::ServerHeaders(name) => {
                let key = unquote_toml_key(raw_k.trim());
                if !key.is_empty()
                    && let Some(s) = parse_toml_scalar_as_string(val)
                {
                    entries
                        .entry(name.clone())
                        .or_default()
                        .headers
                        .insert(key, s);
                }
            }
        }
    }

    let source_str = source_path.to_string_lossy().to_string();
    let mut out = Vec::new();
    for (name, e) in entries {
        if name.chars().any(char::is_control) {
            continue;
        }
        let transport = if let Some(url) = e.url.filter(|s| !s.trim().is_empty()) {
            ResolvedTransport::StreamableHttp { url }
        } else if let Some(command) = e.command.filter(|s| !s.trim().is_empty()) {
            ResolvedTransport::Stdio {
                command,
                args: e.args,
            }
        } else {
            continue;
        };
        if is_builtin_orbit_mcp_entry(&name, &transport) {
            continue;
        }
        out.push(ResolvedExternalMcpServer {
            id: make_server_id(McpServerScope::CodexUser, &name),
            name,
            scope: McpServerScope::CodexUser,
            transport,
            env: e.env,
            headers: e.headers,
            enabled: e.enabled.unwrap_or(true),
            editable: false,
            source_path: source_str.clone(),
        });
    }
    out
}

fn strip_toml_comment(line: &str) -> &str {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '#' => return &line[..idx],
                _ => {}
            }
        }
    }
    line
}

fn split_toml_kv(line: &str) -> Option<(&str, &str)> {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if in_double {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '=' => return Some((&line[..idx], &line[idx + 1..])),
                _ => {}
            }
        }
    }
    None
}

fn parse_toml_table_path(header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for ch in header.chars() {
        if in_double {
            if escaped {
                cur.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            } else {
                cur.push(ch);
            }
        } else if in_single {
            if ch == '\'' {
                in_single = false;
            } else {
                cur.push(ch);
            }
        } else {
            match ch {
                '"' => in_double = true,
                '\'' => in_single = true,
                '.' => {
                    out.push(cur.trim().to_string());
                    cur.clear();
                }
                _ => cur.push(ch),
            }
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn unquote_toml_key(raw: &str) -> String {
    parse_toml_string(raw).unwrap_or_else(|| raw.to_string())
}

fn parse_toml_string(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let mut out = String::new();
        let mut chars = s[1..s.len() - 1].chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next()? {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        return Some(out);
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return Some(s[1..s.len() - 1].to_string());
    }
    None
}

fn parse_toml_scalar_as_string(raw: &str) -> Option<String> {
    if let Some(s) = parse_toml_string(raw) {
        return Some(s);
    }
    let trimmed = raw.trim();
    if trimmed == "true" || trimmed == "false" || trimmed.parse::<f64>().is_ok() {
        return Some(trimmed.to_string());
    }
    None
}

fn parse_toml_string_array(raw: &str) -> Option<Vec<String>> {
    let s = raw.trim();
    if !(s.starts_with('[') && s.ends_with(']')) {
        return None;
    }
    let inner = s[1..s.len() - 1].trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for ch in inner.chars() {
        if in_double {
            cur.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            cur.push(ch);
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => {
                    in_double = true;
                    cur.push(ch);
                }
                '\'' => {
                    in_single = true;
                    cur.push(ch);
                }
                ',' => {
                    let t = cur.trim();
                    if !t.is_empty() {
                        items.push(parse_toml_string(t)?);
                    }
                    cur.clear();
                }
                _ => cur.push(ch),
            }
        }
    }
    let tail = cur.trim();
    if !tail.is_empty() {
        items.push(parse_toml_string(tail)?);
    }
    Some(items)
}

fn parse_toml_inline_table(raw: &str) -> Option<BTreeMap<String, String>> {
    let s = raw.trim();
    if !(s.starts_with('{') && s.ends_with('}')) {
        return None;
    }
    let inner = s[1..s.len() - 1].trim();
    let mut out = BTreeMap::new();
    if inner.is_empty() {
        return Some(out);
    }
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    for ch in inner.chars() {
        if in_double {
            cur.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_double = false;
            }
        } else if in_single {
            cur.push(ch);
            if ch == '\'' {
                in_single = false;
            }
        } else {
            match ch {
                '"' => {
                    in_double = true;
                    cur.push(ch);
                }
                '\'' => {
                    in_single = true;
                    cur.push(ch);
                }
                ',' => {
                    parts.push(cur.clone());
                    cur.clear();
                }
                _ => cur.push(ch),
            }
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    for part in parts {
        let Some((k, v)) = split_toml_kv(part.trim()) else {
            continue;
        };
        let key = unquote_toml_key(k.trim());
        if let Some(val) = parse_toml_scalar_as_string(v.trim())
            && !key.is_empty()
        {
            out.insert(key, val);
        }
    }
    Some(out)
}

fn discover_all_resolved(
    orbit_home: &Path,
    user_home: Option<&Path>,
    cwd: Option<&Path>,
) -> Vec<ResolvedExternalMcpServer> {
    let mut servers = Vec::new();

    // 1. Orbit user-managed: $ORBIT_HOME/mcp-servers.json
    let orbit_path = orbit_user_mcp_path(orbit_home);
    if let Some(text) = read_bounded_text(&orbit_path)
        && let Ok(val) = serde_json::from_str::<Value>(&text)
    {
        servers.extend(parse_json_mcp_servers_value(
            &val,
            McpServerScope::OrbitUser,
            &orbit_path,
            false,
        ));
    }

    // 2. Project-scoped: <canonical_cwd>/.mcp.json
    if let Some(cwd_path) = cwd
        && let Ok(project_mcp) = resolve_project_dot_mcp_path(cwd_path, false)
        && let Some(text) = read_bounded_text(&project_mcp)
        && let Ok(val) = serde_json::from_str::<Value>(&text)
    {
        servers.extend(parse_json_mcp_servers_value(
            &val,
            McpServerScope::ProjectDotMcp,
            &project_mcp,
            false,
        ));
    }

    // 3. Read-only external CLI discovery (~/.claude.json, ~/.codex/config.toml, ~/.gemini/antigravity/mcp_config.json)
    if let Some(uhome) = user_home {
        let claude_path = uhome.join(".claude.json");
        if let Some(text) = read_bounded_text(&claude_path)
            && let Ok(val) = serde_json::from_str::<Value>(&text)
        {
            servers.extend(parse_json_mcp_servers_value(
                &val,
                McpServerScope::ClaudeUser,
                &claude_path,
                true,
            ));
        }

        let codex_path = uhome.join(".codex").join("config.toml");
        if let Some(text) = read_bounded_text(&codex_path) {
            servers.extend(parse_codex_toml_mcp_servers(&text, &codex_path));
        }

        let agy_path = uhome
            .join(".gemini")
            .join("antigravity")
            .join("mcp_config.json");
        if let Some(text) = read_bounded_text(&agy_path)
            && let Ok(val) = serde_json::from_str::<Value>(&text)
        {
            servers.extend(parse_json_mcp_servers_value(
                &val,
                McpServerScope::AntigravityUser,
                &agy_path,
                true,
            ));
        }
    }

    servers
}

fn to_wire_record(
    resolved: ResolvedExternalMcpServer,
    canonical_cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> ExternalMcpServerRecord {
    let header_keys: Vec<String> = resolved.headers.keys().cloned().collect();
    let transport = match resolved.transport {
        ResolvedTransport::Stdio { command, args } => McpTransportKind::Stdio { command, args },
        ResolvedTransport::StreamableHttp { url } => {
            McpTransportKind::StreamableHttp { url, header_keys }
        }
    };
    let env_keys: Vec<String> = resolved.env.keys().cloned().collect();
    let masked_env = mask_kv_map(&resolved.env);
    let masked_headers = mask_kv_map(&resolved.headers);
    let cache_key = probe_cache_key(&resolved.id, canonical_cwd);
    let last_probe = probe_cache.get(&cache_key).cloned();

    ExternalMcpServerRecord {
        id: resolved.id,
        name: resolved.name,
        scope: resolved.scope,
        transport,
        env_keys,
        masked_env,
        masked_headers,
        enabled: resolved.enabled,
        editable: resolved.editable,
        source_path: resolved.source_path,
        last_probe,
    }
}

/// List all discovered and Orbit-managed external MCP servers with all secrets masked.
pub fn list_external_mcp_servers(
    orbit_home: &Path,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Vec<ExternalMcpServerRecord> {
    let user_home = resolve_user_home_dir();
    list_external_mcp_servers_with_homes(orbit_home, user_home.as_deref(), cwd, probe_cache)
}

pub(crate) fn list_external_mcp_servers_with_homes(
    orbit_home: &Path,
    user_home: Option<&Path>,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Vec<ExternalMcpServerRecord> {
    let canonical_cwd = cwd.and_then(|dir| canonicalize_workspace_root(dir).ok());
    discover_all_resolved(orbit_home, user_home, cwd)
        .into_iter()
        .map(|item| to_wire_record(item, canonical_cwd.as_deref(), probe_cache))
        .collect()
}

fn load_or_init_json_root(path: &Path) -> Result<Value, String> {
    if !path.exists() {
        return Ok(json!({ "mcpServers": {} }));
    }
    let meta =
        fs::metadata(path).map_err(|e| format!("Failed to inspect `{}`: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("`{}` is not a regular file", path.display()));
    }
    if meta.len() > MAX_CONFIG_FILE_BYTES {
        return Err(format!(
            "`{}` exceeds the {} byte MCP config size cap",
            path.display(),
            MAX_CONFIG_FILE_BYTES
        ));
    }
    let text = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read `{}`: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(json!({ "mcpServers": {} }));
    }
    let mut root: Value = serde_json::from_str(&text)
        .map_err(|e| format!("Invalid JSON in `{}`: {e}", path.display()))?;
    let Some(obj) = root.as_object_mut() else {
        return Err(format!("Expected JSON object in `{}`", path.display()));
    };
    if !obj.contains_key("mcpServers") {
        obj.insert("mcpServers".to_string(), Value::Object(Map::new()));
    } else if !obj.get("mcpServers").is_some_and(Value::is_object) {
        return Err(format!(
            "`mcpServers` in `{}` must be a JSON object",
            path.display()
        ));
    }
    Ok(root)
}

/// Merge incoming env/headers with existing secrets when `keep_existing_secrets` is true.
/// Preserves existing secret values ONLY for keys still present in `incoming` with empty or
/// masked placeholder values — keys explicitly omitted by the caller are deleted cleanly.
fn merge_with_existing_secrets(
    incoming: &BTreeMap<String, String>,
    existing: &BTreeMap<String, String>,
    keep_existing_secrets: bool,
) -> BTreeMap<String, String> {
    if !keep_existing_secrets {
        return incoming.clone();
    }
    let mut merged = BTreeMap::new();
    for (k, v) in incoming {
        if is_secret_key(k) && (v.is_empty() || is_masked_placeholder(v)) {
            if let Some(prev) = existing.get(k) {
                merged.insert(k.clone(), prev.clone());
            }
        } else {
            merged.insert(k.clone(), v.clone());
        }
    }
    merged
}

/// Create or update an external MCP server in either `OrbitUser` (`$ORBIT_HOME/mcp-servers.json`)
/// or `ProjectDotMcp` (`<canonical_cwd>/.mcp.json`), enforcing secret masking/preservation and `0600` permissions.
pub fn upsert_external_mcp_server(
    orbit_home: &Path,
    input: UpsertExternalMcpInput,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let user_home = resolve_user_home_dir();
    upsert_external_mcp_server_with_homes(orbit_home, user_home.as_deref(), input, probe_cache)
}

pub(crate) fn upsert_external_mcp_server_with_homes(
    orbit_home: &Path,
    user_home: Option<&Path>,
    input: UpsertExternalMcpInput,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let name = validate_server_name(&input.name)?;
    validate_transport(&input.transport)?;
    let incoming_env = validate_env_map(&input.env)?;
    let incoming_headers = validate_headers_map(&input.headers, input.keep_existing_secrets)?;

    let (target_path, target_scope, force_private) = match input.scope {
        McpWriteScope::User => (
            orbit_user_mcp_path(orbit_home),
            McpServerScope::OrbitUser,
            true,
        ),
        McpWriteScope::Project => {
            let cwd = input
                .cwd
                .as_deref()
                .ok_or_else(|| "Workspace `cwd` is required for project `.mcp.json`".to_string())?;
            let path = resolve_project_dot_mcp_path(cwd, true)?;
            (path, McpServerScope::ProjectDotMcp, false)
        }
    };

    // Look up existing secrets in target scope first; if absent (e.g. importing a read-only server),
    // also fall back to a same-named read-only discovered server so secrets transfer without leaking to UI.
    let all_existing = discover_all_resolved(orbit_home, user_home, input.cwd.as_deref());
    let existing_entry = all_existing
        .iter()
        .find(|s| s.scope == target_scope && s.name == name)
        .or_else(|| {
            if input.keep_existing_secrets {
                all_existing.iter().find(|s| !s.editable && s.name == name)
            } else {
                None
            }
        });

    let empty_map = BTreeMap::new();
    let final_env = merge_with_existing_secrets(
        &incoming_env,
        existing_entry.map(|s| &s.env).unwrap_or(&empty_map),
        input.keep_existing_secrets,
    );
    let final_headers = merge_with_existing_secrets(
        &incoming_headers,
        existing_entry.map(|s| &s.headers).unwrap_or(&empty_map),
        input.keep_existing_secrets,
    );
    // Validate resolved headers strictly after merging preserved secrets.
    let final_headers = validate_headers_map(&final_headers, false)?;

    let mut root = load_or_init_json_root(&target_path)?;
    let mut entry_obj = Map::new();
    match &input.transport {
        McpTransportKind::Stdio { command, args } => {
            entry_obj.insert(
                "command".to_string(),
                Value::String(command.trim().to_string()),
            );
            if !args.is_empty() {
                entry_obj.insert(
                    "args".to_string(),
                    Value::Array(args.iter().cloned().map(Value::String).collect()),
                );
            }
            if !final_env.is_empty() {
                let env_obj: Map<String, Value> = final_env
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                entry_obj.insert("env".to_string(), Value::Object(env_obj));
            }
        }
        McpTransportKind::StreamableHttp { url, .. } => {
            entry_obj.insert("type".to_string(), Value::String("http".to_string()));
            entry_obj.insert("url".to_string(), Value::String(url.trim().to_string()));
            if !final_headers.is_empty() {
                let hdr_obj: Map<String, Value> = final_headers
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                entry_obj.insert("headers".to_string(), Value::Object(hdr_obj));
            }
            if !final_env.is_empty() {
                let env_obj: Map<String, Value> = final_env
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect();
                entry_obj.insert("env".to_string(), Value::Object(env_obj));
            }
        }
    }
    entry_obj.insert("enabled".to_string(), Value::Bool(input.enabled));

    if let Some(servers_map) = root.get_mut("mcpServers").and_then(Value::as_object_mut) {
        servers_map.insert(name, Value::Object(entry_obj));
    }

    write_json_atomic(
        &target_path,
        &root,
        force_private || json_root_has_secrets(&root),
    )?;

    Ok(list_external_mcp_servers_with_homes(
        orbit_home,
        user_home,
        input.cwd.as_deref(),
        probe_cache,
    ))
}

fn parse_editable_server_id(server_id: &str) -> Result<(McpServerScope, String), String> {
    let Some((prefix, name)) = server_id.split_once(':') else {
        return Err(format!("Invalid MCP server ID `{server_id}`"));
    };
    let scope = match prefix {
        "orbit_user" => McpServerScope::OrbitUser,
        "project_dot_mcp" => McpServerScope::ProjectDotMcp,
        "claude_user" | "codex_user" | "antigravity_user" => {
            return Err(
                "External provider MCP entries are read-only; import into Orbit to edit or toggle"
                    .to_string(),
            );
        }
        _ => return Err(format!("Unknown MCP server scope `{prefix}`")),
    };
    if name.trim().is_empty() {
        return Err(format!("Invalid MCP server ID `{server_id}`"));
    }
    Ok((scope, name.to_string()))
}

/// Toggle `enabled` on an editable (`OrbitUser` or `ProjectDotMcp`) MCP server.
pub fn toggle_external_mcp_server(
    orbit_home: &Path,
    server_id: &str,
    enabled: bool,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let user_home = resolve_user_home_dir();
    toggle_external_mcp_server_with_homes(
        orbit_home,
        user_home.as_deref(),
        server_id,
        enabled,
        cwd,
        probe_cache,
    )
}

pub(crate) fn toggle_external_mcp_server_with_homes(
    orbit_home: &Path,
    user_home: Option<&Path>,
    server_id: &str,
    enabled: bool,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let (scope, name) = parse_editable_server_id(server_id)?;
    let (target_path, force_private) = match scope {
        McpServerScope::OrbitUser => (orbit_user_mcp_path(orbit_home), true),
        McpServerScope::ProjectDotMcp => {
            let dir =
                cwd.ok_or_else(|| "Workspace `cwd` is required to update `.mcp.json`".to_string())?;
            (resolve_project_dot_mcp_path(dir, true)?, false)
        }
        _ => return Err("Cannot toggle read-only external MCP server".to_string()),
    };

    let mut root = load_or_init_json_root(&target_path)?;
    let Some(entry) = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.get_mut(&name))
        .and_then(Value::as_object_mut)
    else {
        return Err(format!("MCP server `{server_id}` not found"));
    };
    entry.insert("enabled".to_string(), Value::Bool(enabled));
    entry.remove("disabled");

    write_json_atomic(
        &target_path,
        &root,
        force_private || json_root_has_secrets(&root),
    )?;
    Ok(list_external_mcp_servers_with_homes(
        orbit_home,
        user_home,
        cwd,
        probe_cache,
    ))
}

/// Delete an editable (`OrbitUser` or `ProjectDotMcp`) MCP server.
pub fn delete_external_mcp_server(
    orbit_home: &Path,
    server_id: &str,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let user_home = resolve_user_home_dir();
    delete_external_mcp_server_with_homes(
        orbit_home,
        user_home.as_deref(),
        server_id,
        cwd,
        probe_cache,
    )
}

pub(crate) fn delete_external_mcp_server_with_homes(
    orbit_home: &Path,
    user_home: Option<&Path>,
    server_id: &str,
    cwd: Option<&Path>,
    probe_cache: &HashMap<String, McpProbeResult>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    let (scope, name) = parse_editable_server_id(server_id)?;
    let (target_path, force_private) = match scope {
        McpServerScope::OrbitUser => (orbit_user_mcp_path(orbit_home), true),
        McpServerScope::ProjectDotMcp => {
            let dir =
                cwd.ok_or_else(|| "Workspace `cwd` is required to update `.mcp.json`".to_string())?;
            (resolve_project_dot_mcp_path(dir, true)?, false)
        }
        _ => return Err("Cannot delete read-only external MCP server".to_string()),
    };

    let mut root = load_or_init_json_root(&target_path)?;
    let removed = root
        .get_mut("mcpServers")
        .and_then(Value::as_object_mut)
        .and_then(|m| m.remove(&name))
        .is_some();
    if !removed {
        return Err(format!("MCP server `{server_id}` not found"));
    }

    write_json_atomic(
        &target_path,
        &root,
        force_private || json_root_has_secrets(&root),
    )?;
    Ok(list_external_mcp_servers_with_homes(
        orbit_home,
        user_home,
        cwd,
        probe_cache,
    ))
}

/// Probe a configured or discovered external MCP server by `server_id` (bounded to 6s).
pub async fn probe_external_mcp_server(
    orbit_home: &Path,
    server_id: &str,
    cwd: Option<&Path>,
) -> McpProbeResult {
    let user_home = resolve_user_home_dir();
    probe_external_mcp_server_with_homes(orbit_home, user_home.as_deref(), server_id, cwd).await
}

pub(crate) async fn probe_external_mcp_server_with_homes(
    orbit_home: &Path,
    user_home: Option<&Path>,
    server_id: &str,
    cwd: Option<&Path>,
) -> McpProbeResult {
    let started = Instant::now();
    let servers = discover_all_resolved(orbit_home, user_home, cwd);
    let Some(server) = servers.into_iter().find(|s| s.id == server_id) else {
        return McpProbeResult {
            ok: false,
            server_name: None,
            server_version: None,
            protocol_version: None,
            tools: Vec::new(),
            latency_ms: started.elapsed().as_millis() as u64,
            error: Some(format!("MCP server `{server_id}` not found on this host")),
            probed_at_ms: now_unix_ms(),
        };
    };

    match &server.transport {
        ResolvedTransport::Stdio { command, args } => {
            probe_stdio_mcp(command, args, &server.env, cwd, started, MCP_PROBE_TIMEOUT).await
        }
        ResolvedTransport::StreamableHttp { url } => {
            match tokio::time::timeout(
                MCP_PROBE_TIMEOUT,
                probe_http_mcp(url, &server.headers, started),
            )
            .await
            {
                Ok(res) => res,
                Err(_) => McpProbeResult {
                    ok: false,
                    server_name: None,
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    latency_ms: started.elapsed().as_millis() as u64,
                    error: Some(format!(
                        "MCP probe timed out after {}s",
                        MCP_PROBE_TIMEOUT.as_secs()
                    )),
                    probed_at_ms: now_unix_ms(),
                },
            }
        }
    }
}

fn resolve_executable_path(cmd: &str) -> Option<PathBuf> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = Path::new(trimmed);
    if trimmed.contains('/') || trimmed.contains('\\') {
        return p.is_file().then(|| p.to_path_buf());
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
        if let Some(home) = resolve_user_home_dir() {
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
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

async fn kill_process_group_and_reap(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let pgid_arg = format!("-{pid}");
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &pgid_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let count = trimmed.chars().count();
    if count <= max_chars {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

fn extract_tools_from_result(result: &Value) -> Vec<McpToolSummary> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .take(MAX_MCP_PROBE_TOOLS)
                .filter_map(|item| {
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())?;
                    let desc = item
                        .get("description")
                        .and_then(Value::as_str)
                        .map(|d| truncate_chars(d, MAX_TOOL_DESC_CHARS))
                        .unwrap_or_default();
                    Some(McpToolSummary {
                        name: name.to_string(),
                        description: desc,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn jsonrpc_error_message(val: &Value) -> Option<(Option<i64>, String)> {
    let err = val.get("error")?;
    let code = err.get("code").and_then(Value::as_i64);
    let msg = err
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("JSON-RPC error")
        .to_string();
    Some((code, msg))
}

fn matches_jsonrpc_id(val: &Value, expected_id: u64) -> bool {
    match val.get("id") {
        Some(Value::Number(n)) => n.as_u64() == Some(expected_id),
        Some(Value::String(s)) => s.parse::<u64>().ok() == Some(expected_id),
        _ => false,
    }
}

async fn read_stdio_jsonrpc_response<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    expected_id: u64,
    total_read: &mut usize,
) -> Result<Value, String> {
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| format!("Failed reading MCP stdout: {e}"))?;
        if n == 0 {
            if *total_read >= MAX_STDIO_PROBE_STDOUT_BYTES {
                return Err("MCP stdio response exceeded 2 MiB limit".to_string());
            }
            return Err("MCP server closed stdout before responding".to_string());
        }
        *total_read = total_read.saturating_add(n);
        if *total_read > MAX_STDIO_PROBE_STDOUT_BYTES {
            return Err("MCP stdio response exceeded 2 MiB limit".to_string());
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Support Content-Length header framing as fallback in addition to standard NDJSON.
        if let Some(rest) = trimmed
            .strip_prefix("Content-Length:")
            .or_else(|| trimmed.strip_prefix("content-length:"))
            && let Ok(content_len) = rest.trim().parse::<usize>()
        {
            if content_len > MAX_STDIO_PROBE_STDOUT_BYTES {
                return Err("MCP Content-Length frame exceeded 2 MiB limit".to_string());
            }
            loop {
                let mut hdr = String::new();
                let hn = reader
                    .read_line(&mut hdr)
                    .await
                    .map_err(|e| format!("Failed reading MCP header: {e}"))?;
                *total_read = total_read.saturating_add(hn);
                if *total_read > MAX_STDIO_PROBE_STDOUT_BYTES {
                    return Err("MCP stdio response exceeded 2 MiB limit".to_string());
                }
                if hn == 0 || hdr.trim().is_empty() {
                    break;
                }
            }
            if total_read.saturating_add(content_len) > MAX_STDIO_PROBE_STDOUT_BYTES {
                return Err("MCP stdio response exceeded 2 MiB limit".to_string());
            }
            let mut body = vec![0u8; content_len];
            reader
                .read_exact(&mut body)
                .await
                .map_err(|e| format!("Failed reading MCP framed body: {e}"))?;
            *total_read = total_read.saturating_add(content_len);
            if let Ok(val) = serde_json::from_slice::<Value>(&body)
                && matches_jsonrpc_id(&val, expected_id)
            {
                return Ok(val);
            }
            continue;
        }

        if let Ok(val) = serde_json::from_str::<Value>(trimmed)
            && matches_jsonrpc_id(&val, expected_id)
        {
            return Ok(val);
        }
    }
}

struct StdioProbeSuccess {
    server_name: Option<String>,
    server_version: Option<String>,
    protocol_version: Option<String>,
    tools: Vec<McpToolSummary>,
}

async fn exchange_stdio_mcp(
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
) -> Result<StdioProbeSuccess, String> {
    // Bound raw stdout bytes BEFORE BufReader so an adversarial single line without `\n`
    // can never allocate unbounded RAM.
    let bounded_stdout = stdout.take((MAX_STDIO_PROBE_STDOUT_BYTES + 1) as u64);
    let mut reader = BufReader::new(bounded_stdout);
    let mut total_read = 0usize;

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    let init_line = format!("{init_req}\n");
    stdin
        .write_all(init_line.as_bytes())
        .await
        .map_err(|e| format!("Failed writing initialize request: {e}"))?;
    let _ = stdin.flush().await;

    let init_resp = read_stdio_jsonrpc_response(&mut reader, 1, &mut total_read).await?;
    if let Some((_, msg)) = jsonrpc_error_message(&init_resp) {
        return Err(format!("initialize failed: {msg}"));
    }

    let init_result = init_resp.get("result").cloned().unwrap_or(Value::Null);
    let protocol_version = init_result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let server_name = init_result
        .pointer("/serverInfo/name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let server_version = init_result
        .pointer("/serverInfo/version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    // Send notifications/initialized
    let notif = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    let notif_line = format!("{notif}\n");
    let _ = stdin.write_all(notif_line.as_bytes()).await;

    // Request tools/list
    let tools_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    let tools_line = format!("{tools_req}\n");
    stdin
        .write_all(tools_line.as_bytes())
        .await
        .map_err(|e| format!("Failed writing tools/list request: {e}"))?;
    let _ = stdin.flush().await;

    let tools_resp = read_stdio_jsonrpc_response(&mut reader, 2, &mut total_read).await?;
    let tools = if let Some((code, msg)) = jsonrpc_error_message(&tools_resp) {
        if code == Some(-32601) {
            Vec::new()
        } else {
            return Err(format!("tools/list failed: {msg}"));
        }
    } else {
        extract_tools_from_result(tools_resp.get("result").unwrap_or(&Value::Null))
    };

    drop(stdin);
    Ok(StdioProbeSuccess {
        server_name,
        server_version,
        protocol_version,
        tools,
    })
}

async fn probe_stdio_mcp(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: Option<&Path>,
    started: Instant,
    timeout_dur: Duration,
) -> McpProbeResult {
    let fail = |msg: String| McpProbeResult {
        ok: false,
        server_name: None,
        server_version: None,
        protocol_version: None,
        tools: Vec::new(),
        latency_ms: started.elapsed().as_millis() as u64,
        error: Some(msg),
        probed_at_ms: now_unix_ms(),
    };

    if command.trim().is_empty() || command.chars().any(|c| c == '\0' || c.is_control()) {
        return fail("Invalid stdio command".to_string());
    }

    let cmd_owned = command.to_string();
    let resolved = tokio::task::spawn_blocking(move || resolve_executable_path(&cmd_owned))
        .await
        .ok()
        .flatten();
    let Some(resolved) = resolved else {
        return fail(format!("Executable `{command}` not found on PATH"));
    };

    let mut cmd = Command::new(&resolved);
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    // Strip any dangerous inherited loader, shell-hook, or internal ORBIT_* env variables.
    for (k_os, _) in std::env::vars_os() {
        if let Some(k_str) = k_os.to_str()
            && is_blocked_mcp_env_key(k_str)
        {
            cmd.env_remove(&k_os);
        }
    }
    cmd.env("NO_COLOR", "1");

    // Apply user env with dangerous keys strictly filtered out.
    for (k, v) in sanitize_stdio_env(env) {
        cmd.env(k, v);
    }
    if let Some(dir) = cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("Failed to spawn `{}`: {e}", resolved.display())),
    };

    let Some(stdin) = child.stdin.take() else {
        kill_process_group_and_reap(&mut child).await;
        return fail("Failed to capture MCP child stdin".to_string());
    };
    let Some(stdout) = child.stdout.take() else {
        kill_process_group_and_reap(&mut child).await;
        return fail("Failed to capture MCP child stdout".to_string());
    };
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr
                .take(MAX_STDIO_PROBE_STDERR_BYTES as u64)
                .read_to_end(&mut buf)
                .await;
            buf
        })
    });

    // Enforce timeout INSIDE probe_stdio_mcp so `kill_process_group_and_reap` and
    // `stderr_task.abort()` ALWAYS run cleanly even on timeout.
    let exchange_res = tokio::time::timeout(timeout_dur, exchange_stdio_mcp(stdin, stdout)).await;

    kill_process_group_and_reap(&mut child).await;

    match exchange_res {
        Ok(Ok(ok)) => {
            if let Some(task) = stderr_task {
                task.abort();
            }
            McpProbeResult {
                ok: true,
                server_name: ok.server_name,
                server_version: ok.server_version,
                protocol_version: ok.protocol_version,
                tools: ok.tools,
                latency_ms: started.elapsed().as_millis() as u64,
                error: None,
                probed_at_ms: now_unix_ms(),
            }
        }
        Ok(Err(err)) => {
            let stderr_note = collect_stderr_note(stderr_task).await;
            fail(match stderr_note {
                Some(note) => format!("{err} (stderr: {note})"),
                None => err,
            })
        }
        Err(_) => {
            if let Some(task) = stderr_task {
                task.abort();
            }
            fail(format!(
                "MCP probe timed out after {}s",
                timeout_dur.as_secs()
            ))
        }
    }
}

async fn collect_stderr_note(task: Option<tokio::task::JoinHandle<Vec<u8>>>) -> Option<String> {
    let handle = task?;
    let bytes = match tokio::time::timeout(Duration::from_millis(250), handle).await {
        Ok(Ok(b)) => b,
        _ => return None,
    };
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(truncate_chars(trimmed, 240))
    }
}

async fn read_bounded_http_body(mut resp: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Failed reading HTTP response body: {e}"))?
    {
        if buf.len().saturating_add(chunk.len()) > MAX_HTTP_PROBE_RESPONSE_BYTES {
            return Err("HTTP MCP response exceeded 2 MiB safety limit".to_string());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

fn try_extract_sse_event_json(
    block: &str,
    expected_id: u64,
    fallback: &mut Option<Value>,
) -> Option<Value> {
    let mut data_lines = Vec::new();
    for raw_line in block.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let joined = data_lines.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }
    let val = serde_json::from_str::<Value>(trimmed).ok()?;
    if let Some(arr) = val.as_array() {
        if let Some(found) = arr
            .iter()
            .find(|item| matches_jsonrpc_id(item, expected_id))
        {
            return Some(found.clone());
        }
    } else if matches_jsonrpc_id(&val, expected_id) {
        return Some(val);
    } else if fallback.is_none() && (val.get("result").is_some() || val.get("error").is_some()) {
        *fallback = Some(val);
    }
    None
}

fn find_sse_event_delimiter(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i + 1 < buf.len() {
        if buf[i] == b'\n' && buf[i + 1] == b'\n' {
            return Some((i, 2));
        }
        if i + 3 < buf.len()
            && buf[i] == b'\r'
            && buf[i + 1] == b'\n'
            && buf[i + 2] == b'\r'
            && buf[i + 3] == b'\n'
        {
            return Some((i, 4));
        }
        i += 1;
    }
    None
}

/// Incrementally read an HTTP MCP response:
/// - For `text/event-stream`, parses each completed SSE event block (`\n\n` or `\r\n\r\n`) as chunks
///   arrive and returns immediately upon matching `expected_id` without waiting for EOF.
/// - For JSON responses, reads up to `MAX_HTTP_PROBE_RESPONSE_BYTES` and parses JSON-RPC.
async fn read_http_jsonrpc_response(
    mut resp: reqwest::Response,
    content_type: &str,
    expected_id: u64,
) -> Result<Value, String> {
    let ct_lower = content_type.to_ascii_lowercase();
    if ct_lower.contains("text/event-stream") {
        let mut total_bytes = 0usize;
        let mut pending = Vec::new();
        let mut fallback: Option<Value> = None;

        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("Failed reading SSE response chunk: {e}"))?
        {
            total_bytes = total_bytes.saturating_add(chunk.len());
            if total_bytes > MAX_HTTP_PROBE_RESPONSE_BYTES {
                return Err("HTTP MCP SSE response exceeded 2 MiB safety limit".to_string());
            }
            pending.extend_from_slice(&chunk);
            while let Some((pos, delim_len)) = find_sse_event_delimiter(&pending) {
                let block_text = String::from_utf8_lossy(&pending[..pos]).into_owned();
                pending.drain(..pos + delim_len);
                if let Some(matched) =
                    try_extract_sse_event_json(&block_text, expected_id, &mut fallback)
                {
                    return Ok(matched);
                }
            }
        }

        if !pending.is_empty() {
            let tail = String::from_utf8_lossy(&pending);
            if let Some(matched) = try_extract_sse_event_json(&tail, expected_id, &mut fallback) {
                return Ok(matched);
            }
        }
        return fallback
            .ok_or_else(|| "No matching JSON-RPC payload found in SSE stream".to_string());
    }

    let body = read_bounded_http_body(resp).await?;
    let val: Value = serde_json::from_slice(&body)
        .map_err(|e| format!("Invalid JSON-RPC HTTP response: {e}"))?;
    if let Some(arr) = val.as_array() {
        arr.iter()
            .find(|item| matches_jsonrpc_id(item, expected_id))
            .cloned()
            .ok_or_else(|| "JSON-RPC batch response did not include matching id".to_string())
    } else {
        Ok(val)
    }
}

async fn probe_http_mcp(
    url: &str,
    headers: &BTreeMap<String, String>,
    started: Instant,
) -> McpProbeResult {
    let fail = |msg: String| McpProbeResult {
        ok: false,
        server_name: None,
        server_version: None,
        protocol_version: None,
        tools: Vec::new(),
        latency_ms: started.elapsed().as_millis() as u64,
        error: Some(msg),
        probed_at_ms: now_unix_ms(),
    };

    let parsed_url = match validate_mcp_http_url(url) {
        Ok(u) => u,
        Err(e) => return fail(e),
    };

    // Resolve DNS upfront, validate EVERY returned IP against SSRF rules, and pin those exact
    // addresses on reqwest while disabling redirects (preventing both DNS rebinding and
    // cross-origin secret header leakage).
    let (host_norm, pinned_addrs) = match resolve_and_validate_http_addrs(&parsed_url).await {
        Ok(res) => res,
        Err(e) => return fail(e),
    };

    let mut builder = reqwest::Client::builder()
        .timeout(MCP_PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none());
    if host_norm.parse::<IpAddr>().is_err() {
        builder = builder.resolve_to_addrs(&host_norm, &pinned_addrs);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => return fail(format!("Failed to build HTTP client: {e}")),
    };

    let mut base_headers = HeaderMap::new();
    base_headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    base_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Ok(ver_val) = HeaderValue::from_str(MCP_PROTOCOL_VERSION) {
        base_headers.insert(HeaderName::from_static("mcp-protocol-version"), ver_val);
    }
    for (k, v) in headers {
        let key_trimmed = k.trim();
        if key_trimmed.is_empty() || is_forbidden_http_header_name(key_trimmed) {
            continue;
        }
        if v.chars().any(|c| c == '\r' || c == '\n' || c == '\0') {
            return fail(format!(
                "HTTP header `{key_trimmed}` contains invalid control characters"
            ));
        }
        let Ok(hn) = HeaderName::from_bytes(key_trimmed.as_bytes()) else {
            return fail(format!("Invalid HTTP header name `{key_trimmed}`"));
        };
        let Ok(hv) = HeaderValue::from_str(v) else {
            return fail(format!("Invalid HTTP header value for `{key_trimmed}`"));
        };
        base_headers.insert(hn, hv);
    }

    // 1. initialize
    let init_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });

    let init_http = match client
        .post(parsed_url.clone())
        .headers(base_headers.clone())
        .json(&init_body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return fail(format!("HTTP request failed: {e}")),
    };

    let init_status = init_http.status();
    let session_id = init_http
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let init_ct = init_http
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    if !init_status.is_success() {
        let err_bytes = read_bounded_http_body(init_http).await.unwrap_or_default();
        let snippet = truncate_chars(&String::from_utf8_lossy(&err_bytes), 200);
        return fail(if snippet.is_empty() {
            format!("HTTP {init_status} during initialize")
        } else {
            format!("HTTP {init_status} during initialize: {snippet}")
        });
    }

    let init_json = match read_http_jsonrpc_response(init_http, &init_ct, 1).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };
    if let Some((_, msg)) = jsonrpc_error_message(&init_json) {
        return fail(format!("initialize failed: {msg}"));
    }

    let init_result = init_json.get("result").cloned().unwrap_or(Value::Null);
    let protocol_version = init_result
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let server_name = init_result
        .pointer("/serverInfo/name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let server_version = init_result
        .pointer("/serverInfo/version")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    if let Some(sid) = session_id.as_deref()
        && let Ok(val) = HeaderValue::from_str(sid)
    {
        base_headers.insert(HeaderName::from_static("mcp-session-id"), val);
    }

    // 2. notifications/initialized (best-effort; servers return 202 Accepted or 204)
    let _ = client
        .post(parsed_url.clone())
        .headers(base_headers.clone())
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }))
        .send()
        .await;

    // 3. tools/list
    let tools_http = match client
        .post(parsed_url)
        .headers(base_headers)
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list",
            "params": {}
        }))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return fail(format!("HTTP tools/list failed: {e}")),
    };

    let tools_status = tools_http.status();
    let tools_ct = tools_http
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    if !tools_status.is_success() {
        let err_bytes = read_bounded_http_body(tools_http).await.unwrap_or_default();
        let snippet = truncate_chars(&String::from_utf8_lossy(&err_bytes), 200);
        return fail(if snippet.is_empty() {
            format!("HTTP {tools_status} during tools/list")
        } else {
            format!("HTTP {tools_status} during tools/list: {snippet}")
        });
    }

    let tools_json = match read_http_jsonrpc_response(tools_http, &tools_ct, 2).await {
        Ok(v) => v,
        Err(e) => return fail(e),
    };

    let tools = if let Some((code, msg)) = jsonrpc_error_message(&tools_json) {
        if code == Some(-32601) {
            Vec::new()
        } else {
            return fail(format!("tools/list failed: {msg}"));
        }
    } else {
        extract_tools_from_result(tools_json.get("result").unwrap_or(&Value::Null))
    };

    McpProbeResult {
        ok: true,
        server_name,
        server_version,
        protocol_version,
        tools,
        latency_ms: started.elapsed().as_millis() as u64,
        error: None,
        probed_at_ms: now_unix_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::StatusCode;
    use axum::response::{IntoResponse, Response};
    use axum::routing::post;
    use futures_util::stream;

    fn make_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit-mcp-test-{label}-{}",
            Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn user_config_crud_0600_permissions_and_secret_masking_roundtrip() {
        let orbit_home = make_temp_dir("crud");
        let user_home = make_temp_dir("uhome");
        let cache = HashMap::new();

        // 1. Create OrbitUser stdio server with two secrets + non-secret env var
        let servers = upsert_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "github".into(),
                transport: McpTransportKind::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
                },
                env: BTreeMap::from([
                    ("GITHUB_TOKEN".into(), "ghp_super_secret_9876".into()),
                    ("OLD_API_SECRET".into(), "old_secret_value_1111".into()),
                    ("LOG_LEVEL".into(), "info".into()),
                ]),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &cache,
        )
        .expect("create github mcp server");

        assert_eq!(servers.len(), 1);
        let rec = &servers[0];
        assert_eq!(rec.id, "orbit_user:github");
        assert_eq!(rec.scope, McpServerScope::OrbitUser);
        assert!(rec.editable);
        assert!(rec.enabled);
        assert_eq!(
            rec.masked_env.get("GITHUB_TOKEN").map(String::as_str),
            Some("••••9876")
        );
        assert_eq!(
            rec.masked_env.get("LOG_LEVEL").map(String::as_str),
            Some("info")
        );

        // Verify 0600 permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = fs::metadata(orbit_home.join("mcp-servers.json")).expect("stat config");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }

        // 2. Edit with keep_existing_secrets=true: preserve GITHUB_TOKEN via empty string,
        // update LOG_LEVEL, and explicitly DELETE OLD_API_SECRET by omitting it!
        let updated = upsert_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "github".into(),
                transport: McpTransportKind::Stdio {
                    command: "npx".into(),
                    args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
                },
                env: BTreeMap::from([
                    ("GITHUB_TOKEN".into(), String::new()),
                    ("LOG_LEVEL".into(), "debug".into()),
                ]),
                headers: BTreeMap::new(),
                keep_existing_secrets: true,
                enabled: true,
            },
            &cache,
        )
        .expect("update github mcp server");
        assert_eq!(
            updated[0].masked_env.get("LOG_LEVEL").map(String::as_str),
            Some("debug")
        );
        assert_eq!(
            updated[0]
                .masked_env
                .get("GITHUB_TOKEN")
                .map(String::as_str),
            Some("••••9876")
        );
        assert!(
            !updated[0].masked_env.contains_key("OLD_API_SECRET"),
            "deleted secret key must not be resurrected"
        );

        // Verify on-disk secret was preserved intact and OLD_API_SECRET was removed
        let raw_on_disk: Value = serde_json::from_str(
            &fs::read_to_string(orbit_home.join("mcp-servers.json")).expect("read raw"),
        )
        .expect("parse raw");
        assert_eq!(
            raw_on_disk
                .pointer("/mcpServers/github/env/GITHUB_TOKEN")
                .and_then(Value::as_str),
            Some("ghp_super_secret_9876")
        );
        assert!(
            raw_on_disk
                .pointer("/mcpServers/github/env/OLD_API_SECRET")
                .is_none()
        );

        // 3. Toggle disabled
        let toggled = toggle_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            "orbit_user:github",
            false,
            None,
            &cache,
        )
        .expect("toggle off");
        assert!(!toggled[0].enabled);

        // 4. Delete
        let after_delete = delete_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            "orbit_user:github",
            None,
            &cache,
        )
        .expect("delete server");
        assert!(after_delete.is_empty());

        let _ = fs::remove_dir_all(&orbit_home);
        let _ = fs::remove_dir_all(&user_home);
    }

    #[test]
    fn edit_and_import_authenticated_streamable_http_preserves_secret_headers() {
        let orbit_home = make_temp_dir("http-auth-edit");
        let user_home = make_temp_dir("http-auth-user");
        let cache = HashMap::new();

        // Seed a read-only Claude HTTP server with an Authorization header
        fs::write(
            user_home.join(".claude.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "linear": {
                        "url": "https://mcp.linear.app/mcp",
                        "headers": {
                            "Authorization": "Bearer lin_real_secret_7788",
                            "X-Team": "core"
                        }
                    }
                }
            }))
            .expect("claude json"),
        )
        .expect("write claude json");

        // 1. Import into OrbitUser sending empty string for Authorization with keep_existing_secrets=true
        let imported = upsert_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "linear".into(),
                transport: McpTransportKind::StreamableHttp {
                    url: "https://mcp.linear.app/mcp".into(),
                    header_keys: vec!["Authorization".into(), "X-Team".into()],
                },
                env: BTreeMap::new(),
                headers: BTreeMap::from([
                    ("Authorization".into(), String::new()),
                    ("X-Team".into(), "core".into()),
                ]),
                keep_existing_secrets: true,
                enabled: true,
            },
            &cache,
        )
        .expect("import authenticated http server");

        let orbit_rec = imported
            .iter()
            .find(|s| s.id == "orbit_user:linear")
            .expect("orbit linear exists");
        assert_eq!(
            orbit_rec
                .masked_headers
                .get("Authorization")
                .map(String::as_str),
            Some("••••7788")
        );

        // 2. Edit existing OrbitUser HTTP server sending masked placeholder "••••7788" + updated X-Team
        let edited = upsert_external_mcp_server_with_homes(
            &orbit_home,
            Some(&user_home),
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "linear".into(),
                transport: McpTransportKind::StreamableHttp {
                    url: "https://mcp.linear.app/v2".into(),
                    header_keys: vec!["Authorization".into(), "X-Team".into()],
                },
                env: BTreeMap::new(),
                headers: BTreeMap::from([
                    ("Authorization".into(), "••••7788".into()),
                    ("X-Team".into(), "platform".into()),
                ]),
                keep_existing_secrets: true,
                enabled: true,
            },
            &cache,
        )
        .expect("edit authenticated http server with masked placeholder");

        let orbit_rec2 = edited
            .iter()
            .find(|s| s.id == "orbit_user:linear")
            .expect("orbit linear edited");
        assert_eq!(
            orbit_rec2
                .masked_headers
                .get("Authorization")
                .map(String::as_str),
            Some("••••7788")
        );
        assert_eq!(
            orbit_rec2.masked_headers.get("X-Team").map(String::as_str),
            Some("platform")
        );

        // Confirm real secret persisted intact on disk
        let raw: Value = serde_json::from_str(
            &fs::read_to_string(orbit_home.join("mcp-servers.json")).expect("read raw"),
        )
        .expect("parse raw");
        assert_eq!(
            raw.pointer("/mcpServers/linear/headers/Authorization")
                .and_then(Value::as_str),
            Some("Bearer lin_real_secret_7788")
        );

        let _ = fs::remove_dir_all(&orbit_home);
        let _ = fs::remove_dir_all(&user_home);
    }

    #[cfg(unix)]
    #[test]
    fn project_dot_mcp_preserves_0600_across_upsert_toggle_and_delete() {
        use std::os::unix::fs::PermissionsExt;

        let orbit_home = make_temp_dir("proj-mode-home");
        let ws = make_temp_dir("proj-mode-ws");
        let cache = HashMap::new();

        // Add server A with a secret token
        upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::Project,
                cwd: Some(ws.clone()),
                name: "secret-srv".into(),
                transport: McpTransportKind::Stdio {
                    command: "echo".into(),
                    args: Vec::new(),
                },
                env: BTreeMap::from([("API_TOKEN".into(), "tok_super_secret_4321".into())]),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &cache,
        )
        .expect("add secret-srv");

        // Add server B without secrets — `.mcp.json` still has secret-srv so MUST stay 0600!
        upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::Project,
                cwd: Some(ws.clone()),
                name: "plain-srv".into(),
                transport: McpTransportKind::Stdio {
                    command: "echo".into(),
                    args: Vec::new(),
                },
                env: BTreeMap::new(),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &cache,
        )
        .expect("add plain-srv");
        assert_eq!(
            fs::metadata(ws.join(".mcp.json"))
                .expect("stat after upsert")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        // Toggle plain-srv — MUST stay 0600!
        toggle_external_mcp_server_with_homes(
            &orbit_home,
            None,
            "project_dot_mcp:plain-srv",
            false,
            Some(&ws),
            &cache,
        )
        .expect("toggle plain-srv");
        assert_eq!(
            fs::metadata(ws.join(".mcp.json"))
                .expect("stat after toggle")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        // Delete plain-srv — secret-srv still exists so MUST stay 0600!
        delete_external_mcp_server_with_homes(
            &orbit_home,
            None,
            "project_dot_mcp:plain-srv",
            Some(&ws),
            &cache,
        )
        .expect("delete plain-srv");
        assert_eq!(
            fs::metadata(ws.join(".mcp.json"))
                .expect("stat after delete")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let _ = fs::remove_dir_all(&orbit_home);
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn dangerous_env_stripping_and_validation() {
        for blocked in [
            "LD_PRELOAD",
            "ld_library_path",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_FRAMEWORK_PATH",
            "ORBIT_HOME",
            "orbit_mcp_token",
            "NODE_OPTIONS",
            "PYTHONSTARTUP",
            "PYTHONPATH",
            "PYTHONHOME",
            "BASH_ENV",
            "ENV",
            "IFS",
            "PROMPT_COMMAND",
            "GIT_SSH_COMMAND",
            "GIT_EXEC_PATH",
            "PERL5OPT",
            "RUBYOPT",
            "JAVA_TOOL_OPTIONS",
            "_JAVA_OPTIONS",
            "GCONV_PATH",
            "GLIBC_TUNABLES",
        ] {
            assert!(
                is_blocked_mcp_env_key(blocked),
                "{blocked} should be blocked"
            );
        }
        assert!(!is_blocked_mcp_env_key("GITHUB_TOKEN"));
        assert!(!is_blocked_mcp_env_key("RUST_LOG"));

        let raw = BTreeMap::from([
            ("LD_PRELOAD".into(), "/tmp/evil.so".into()),
            ("DYLD_INSERT_LIBRARIES".into(), "/tmp/evil.dylib".into()),
            ("ORBIT_HOME".into(), "/tmp/fake".into()),
            ("NODE_OPTIONS".into(), "--require=/tmp/pwn.js".into()),
            ("BASH_ENV".into(), "/tmp/pwn.sh".into()),
            ("SAFE_VAR".into(), "ok".into()),
        ]);
        let sanitized = sanitize_stdio_env(&raw);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized.get("SAFE_VAR").map(String::as_str), Some("ok"));

        let orbit_home = make_temp_dir("env-reject");
        let err = upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "bad-env".into(),
                transport: McpTransportKind::Stdio {
                    command: "echo".into(),
                    args: Vec::new(),
                },
                env: BTreeMap::from([("NODE_OPTIONS".into(), "--inspect".into())]),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &HashMap::new(),
        )
        .expect_err("must reject NODE_OPTIONS");
        assert!(err.contains("blocked"));
        let _ = fs::remove_dir_all(&orbit_home);
    }

    #[test]
    fn project_dot_mcp_traversal_and_symlink_rejection() {
        let base = make_temp_dir("traversal");
        let ws = base.join("repo");
        fs::create_dir_all(&ws).expect("create repo");
        let outside = base.join("outside.json");
        fs::write(&outside, "{}").expect("write outside");

        // Reject raw `..` traversal in cwd
        let traversed = ws.join("..").join("repo");
        let err = resolve_project_dot_mcp_path(&traversed, true).expect_err("reject ..");
        assert!(err.contains(".."));

        // Reject symlinked .mcp.json escaping workspace
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, ws.join(".mcp.json")).expect("create symlink");
            let read_err =
                resolve_project_dot_mcp_path(&ws, false).expect_err("reject escaping symlink read");
            assert!(read_err.contains("escapes"));
            let write_err =
                resolve_project_dot_mcp_path(&ws, true).expect_err("reject symlink write");
            assert!(write_err.contains("symlink"));
        }

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn discovers_claude_codex_multiline_dotted_antigravity_and_skips_builtin_orbit() {
        let orbit_home = make_temp_dir("discover-orbit");
        let user_home = make_temp_dir("discover-user");
        fs::create_dir_all(user_home.join(".codex")).expect("codex dir");
        fs::create_dir_all(user_home.join(".gemini/antigravity")).expect("agy dir");

        fs::write(
            user_home.join(".claude.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "orbit": { "url": "http://127.0.0.1:6868/mcp/agents" },
                    "linear": {
                        "url": "https://mcp.linear.app/mcp",
                        "headers": { "Authorization": "Bearer lin_secret_token_9999" }
                    }
                }
            }))
            .expect("claude json"),
        )
        .expect("write claude");

        fs::write(
            user_home.join(".codex/config.toml"),
            r#"
[mcp_servers.orbit]
url = "http://127.0.0.1:6868/mcp/agents"

[mcp_servers.postgres]
command = "uvx"
args = [
  "mcp-server-postgres",
  "--read-only", # inline comment
  "--timeout=5"
]
env.PG_PASSWORD = "super_secret_password_1234"
env.PG_HOST = "localhost"
enabled = true

[mcp_servers.remote_api]
url = "https://mcp.example.com/mcp"
http_headers."X-Api-Key" = "codex_api_key_5566"
"#,
        )
        .expect("write codex toml");

        fs::write(
            user_home.join(".gemini/antigravity/mcp_config.json"),
            serde_json::to_string_pretty(&json!({
                "mcpServers": {
                    "orbit": { "command": "orbit", "args": ["mcp"] },
                    "filesystem": { "command": "npx", "args": ["-y", "@mcp/fs"] }
                }
            }))
            .expect("agy json"),
        )
        .expect("write agy");

        let list = list_external_mcp_servers_with_homes(
            &orbit_home,
            Some(&user_home),
            None,
            &HashMap::new(),
        );
        assert_eq!(list.len(), 4);
        assert!(list.iter().all(|s| s.name != "orbit"));

        let linear = list
            .iter()
            .find(|s| s.id == "claude_user:linear")
            .expect("linear discovered");
        assert!(!linear.editable);
        assert_eq!(
            linear
                .masked_headers
                .get("Authorization")
                .map(String::as_str),
            Some("••••9999")
        );

        let pg = list
            .iter()
            .find(|s| s.id == "codex_user:postgres")
            .expect("postgres discovered");
        assert_eq!(
            pg.transport,
            McpTransportKind::Stdio {
                command: "uvx".into(),
                args: vec![
                    "mcp-server-postgres".into(),
                    "--read-only".into(),
                    "--timeout=5".into()
                ],
            }
        );
        assert_eq!(
            pg.masked_env.get("PG_PASSWORD").map(String::as_str),
            Some("••••1234")
        );
        assert_eq!(
            pg.masked_env.get("PG_HOST").map(String::as_str),
            Some("localhost")
        );

        let remote_api = list
            .iter()
            .find(|s| s.id == "codex_user:remote_api")
            .expect("remote_api discovered");
        assert_eq!(
            remote_api
                .masked_headers
                .get("X-Api-Key")
                .map(String::as_str),
            Some("••••5566")
        );

        assert!(list.iter().any(|s| s.id == "antigravity_user:filesystem"));

        let _ = fs::remove_dir_all(&orbit_home);
        let _ = fs::remove_dir_all(&user_home);
    }

    #[test]
    fn http_url_and_ipv6_ssrf_validation_blocks_compat_and_nat64() {
        assert!(validate_mcp_http_url("http://127.0.0.1:8080/mcp").is_ok());
        assert!(validate_mcp_http_url("http://[::1]:8080/mcp").is_ok());
        assert!(validate_mcp_http_url("https://mcp.example.com/v1").is_ok());

        assert!(validate_mcp_http_url("file:///etc/passwd").is_err());
        assert!(validate_mcp_http_url("ftp://example.com/mcp").is_err());
        assert!(validate_mcp_http_url("http://user:pass@example.com/mcp").is_err());
        assert!(validate_mcp_http_url("http://169.254.169.254/latest/meta-data/").is_err());
        assert!(validate_mcp_http_url("http://metadata.google.internal/computeMetadata").is_err());
        assert!(validate_mcp_http_url("http://0.0.0.0:8080/mcp").is_err());
        assert!(validate_mcp_http_url("http://[fe80::1]/mcp").is_err());
        assert!(validate_mcp_http_url("http://[::ffff:169.254.169.254]/mcp").is_err());
        assert!(validate_mcp_http_url("http://[::169.254.169.254]/mcp").is_err());
        assert!(validate_mcp_http_url("http://[64:ff9b::169.254.169.254]/mcp").is_err());
        assert!(validate_mcp_http_url("http://[64:ff9b::100.100.100.200]/mcp").is_err());

        assert!(!is_blocked_ssrf_ip("::1".parse().expect("::1")));
        assert!(is_blocked_ssrf_ip(
            "::169.254.169.254".parse().expect("compat metadata")
        ));
        assert!(is_blocked_ssrf_ip(
            "64:ff9b::169.254.169.254".parse().expect("nat64 metadata")
        ));
    }

    #[tokio::test]
    async fn dns_pinning_resolves_localhost_and_probe_disables_redirects() {
        let parsed = validate_mcp_http_url("http://localhost:8080/mcp").expect("localhost url");
        let (host, addrs) = resolve_and_validate_http_addrs(&parsed)
            .await
            .expect("resolve localhost");
        assert_eq!(host, "localhost");
        assert!(!addrs.is_empty());
        assert!(addrs.iter().all(|a| !is_blocked_ssrf_ip(a.ip())));

        // Verify redirects are NOT followed (prevents cross-origin secret header leakage)
        async fn redirect_handler() -> Response {
            (StatusCode::FOUND, [("location", "http://127.0.0.1:1/leak")]).into_response()
        }
        let app = Router::new().route("/redirect", post(redirect_handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind redirect test");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let res = probe_http_mcp(
            &format!("http://{addr}/redirect"),
            &BTreeMap::from([("Authorization".into(), "Bearer secret-1234".into())]),
            Instant::now(),
        )
        .await;
        assert!(!res.ok);
        assert!(
            res.error.as_deref().unwrap_or("").contains("302"),
            "expected HTTP 302 rejection without following redirect, got: {:?}",
            res.error
        );
        server_task.abort();
    }

    #[test]
    fn probe_cache_bounds_evicts_oldest_skips_not_found_and_namespaces_projects() {
        let mut cache = HashMap::new();
        // Skip "not found" errors
        insert_probe_cache(
            &mut cache,
            "orbit_user:missing".into(),
            McpProbeResult {
                ok: false,
                server_name: None,
                server_version: None,
                protocol_version: None,
                tools: Vec::new(),
                latency_ms: 1,
                error: Some("MCP server `orbit_user:missing` not found on this host".into()),
                probed_at_ms: 100,
            },
        );
        assert!(cache.is_empty());

        // Insert 130 entries; oldest 2 must be evicted so len stays 128
        for i in 0..130u64 {
            insert_probe_cache(
                &mut cache,
                format!("orbit_user:srv-{i}"),
                McpProbeResult {
                    ok: true,
                    server_name: Some(format!("srv-{i}")),
                    server_version: None,
                    protocol_version: None,
                    tools: Vec::new(),
                    latency_ms: 5,
                    error: None,
                    probed_at_ms: 1_000 + i,
                },
            );
        }
        assert_eq!(cache.len(), MAX_MCP_PROBE_CACHE_ENTRIES);
        assert!(!cache.contains_key("orbit_user:srv-0"));
        assert!(!cache.contains_key("orbit_user:srv-1"));
        assert!(cache.contains_key("orbit_user:srv-2"));
        assert!(cache.contains_key("orbit_user:srv-129"));

        // Project keys are namespaced by canonical cwd
        let k1 = probe_cache_key("project_dot_mcp:db", Some(Path::new("/repo/a")));
        let k2 = probe_cache_key("project_dot_mcp:db", Some(Path::new("/repo/b")));
        assert_ne!(k1, k2);
        assert_eq!(
            probe_cache_key("orbit_user:db", Some(Path::new("/repo/a"))),
            "orbit_user:db"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn mock_stdio_mcp_probe_initialize_and_tools_list() {
        use std::os::unix::fs::PermissionsExt;

        let dir = make_temp_dir("stdio-probe");
        let script = dir.join("mock_mcp.sh");
        fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*|*'"method": "initialize"'*)
      printf '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"mock-stdio-mcp","version":"1.2.3"}}}\n'
      ;;
    *'"method":"tools/list"'*|*'"method": "tools/list"'*)
      printf '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo_tool","description":"Echoes text"},{"name":"search_docs","description":"Searches docs"}]}}\n'
      exit 0
      ;;
  esac
done
"#,
        )
        .expect("write mock stdio script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod script");

        let orbit_home = make_temp_dir("stdio-home");
        upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "mock-stdio".into(),
                transport: McpTransportKind::Stdio {
                    command: script.to_string_lossy().to_string(),
                    args: Vec::new(),
                },
                env: BTreeMap::from([("SAFE_TEST".into(), "1".into())]),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &HashMap::new(),
        )
        .expect("save mock stdio server");

        let res =
            probe_external_mcp_server_with_homes(&orbit_home, None, "orbit_user:mock-stdio", None)
                .await;
        assert!(res.ok, "probe failed: {:?}", res.error);
        assert_eq!(res.server_name.as_deref(), Some("mock-stdio-mcp"));
        assert_eq!(res.server_version.as_deref(), Some("1.2.3"));
        assert_eq!(res.protocol_version.as_deref(), Some("2025-03-26"));
        assert_eq!(res.tools.len(), 2);
        assert_eq!(res.tools[0].name, "echo_tool");
        assert_eq!(res.tools[1].name, "search_docs");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&orbit_home);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_probe_timeout_kills_child_process_group_cleanly() {
        let res = probe_stdio_mcp(
            "/bin/sleep",
            &["30".to_string()],
            &BTreeMap::new(),
            None,
            Instant::now(),
            Duration::from_millis(250),
        )
        .await;
        assert!(!res.ok);
        assert!(
            res.error.as_deref().unwrap_or("").contains("timed out"),
            "expected timeout error, got {:?}",
            res.error
        );
        assert!(
            res.latency_ms < 2_000,
            "timeout should kill child immediately, took {}ms",
            res.latency_ms
        );
    }

    #[tokio::test]
    async fn mock_streamable_http_json_and_open_ended_sse_mcp_probes() {
        async fn json_handler(
            headers: axum::http::HeaderMap,
            axum::Json(body): axum::Json<Value>,
        ) -> Response {
            let method = body.get("method").and_then(Value::as_str).unwrap_or("");
            match method {
                "initialize" => {
                    let mut resp = (
                        StatusCode::OK,
                        axum::Json(json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "result": {
                                "protocolVersion": "2025-03-26",
                                "serverInfo": { "name": "mock-http-json", "version": "2.0.0" }
                            }
                        })),
                    )
                        .into_response();
                    if let Ok(v) = HeaderValue::from_str("sess-json-42") {
                        resp.headers_mut().insert("mcp-session-id", v);
                    }
                    resp
                }
                "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
                "tools/list" => {
                    let sid = headers
                        .get("mcp-session-id")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if sid != "sess-json-42" {
                        return (StatusCode::BAD_REQUEST, "missing session id").into_response();
                    }
                    (
                        StatusCode::OK,
                        axum::Json(json!({
                            "jsonrpc": "2.0",
                            "id": 2,
                            "result": {
                                "tools": [
                                    { "name": "query_db", "description": "Run SQL query" }
                                ]
                            }
                        })),
                    )
                        .into_response()
                }
                _ => StatusCode::NOT_FOUND.into_response(),
            }
        }

        // Streaming SSE handler that intentionally keeps the HTTP stream open indefinitely
        // after emitting the JSON-RPC event — proving incremental SSE parsing returns immediately!
        async fn open_sse_handler(axum::Json(body): axum::Json<Value>) -> Response {
            let method = body.get("method").and_then(Value::as_str).unwrap_or("");
            match method {
                "initialize" => {
                    let first_event = concat!(
                        "event: message\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-03-26\",\"serverInfo\":{\"name\":\"mock-http-sse\",\"version\":\"3.1.0\"}}}\n\n"
                    )
                    .to_string();
                    let body_stream =
                        stream::once(async move { Ok::<_, std::io::Error>(first_event) })
                            .chain(stream::pending());
                    (
                        StatusCode::OK,
                        [
                            ("content-type", "text/event-stream"),
                            ("mcp-session-id", "sess-sse-99"),
                        ],
                        Body::from_stream(body_stream),
                    )
                        .into_response()
                }
                "notifications/initialized" => StatusCode::ACCEPTED.into_response(),
                "tools/list" => {
                    let first_event = concat!(
                        "event: message\n",
                        "data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"stream_tool\",\"description\":\"Streams data\"},{\"name\":\"ping_tool\",\"description\":\"Pings\"}]}}\n\n"
                    )
                    .to_string();
                    let body_stream =
                        stream::once(async move { Ok::<_, std::io::Error>(first_event) })
                            .chain(stream::pending());
                    (
                        StatusCode::OK,
                        [("content-type", "text/event-stream")],
                        Body::from_stream(body_stream),
                    )
                        .into_response()
                }
                _ => StatusCode::NOT_FOUND.into_response(),
            }
        }

        use futures_util::StreamExt as _;

        let app = Router::new()
            .route("/mcp-json", post(json_handler))
            .route("/mcp-sse", post(open_sse_handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local test port");
        let addr = listener.local_addr().expect("local addr");
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let orbit_home = make_temp_dir("http-probe");

        // Test JSON Streamable HTTP
        upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "http-json".into(),
                transport: McpTransportKind::StreamableHttp {
                    url: format!("http://{addr}/mcp-json"),
                    header_keys: Vec::new(),
                },
                env: BTreeMap::new(),
                headers: BTreeMap::from([(
                    "Authorization".into(),
                    "Bearer test-token-1234".into(),
                )]),
                keep_existing_secrets: false,
                enabled: true,
            },
            &HashMap::new(),
        )
        .expect("save http-json");

        let json_res =
            probe_external_mcp_server_with_homes(&orbit_home, None, "orbit_user:http-json", None)
                .await;
        assert!(json_res.ok, "json probe failed: {:?}", json_res.error);
        assert_eq!(json_res.server_name.as_deref(), Some("mock-http-json"));
        assert_eq!(json_res.server_version.as_deref(), Some("2.0.0"));
        assert_eq!(json_res.tools.len(), 1);
        assert_eq!(json_res.tools[0].name, "query_db");

        // Test open-ended SSE Streamable HTTP (must complete immediately without waiting for EOF!)
        upsert_external_mcp_server_with_homes(
            &orbit_home,
            None,
            UpsertExternalMcpInput {
                scope: McpWriteScope::User,
                cwd: None,
                name: "http-sse".into(),
                transport: McpTransportKind::StreamableHttp {
                    url: format!("http://{addr}/mcp-sse"),
                    header_keys: Vec::new(),
                },
                env: BTreeMap::new(),
                headers: BTreeMap::new(),
                keep_existing_secrets: false,
                enabled: true,
            },
            &HashMap::new(),
        )
        .expect("save http-sse");

        let sse_res =
            probe_external_mcp_server_with_homes(&orbit_home, None, "orbit_user:http-sse", None)
                .await;
        assert!(sse_res.ok, "sse probe failed: {:?}", sse_res.error);
        assert_eq!(sse_res.server_name.as_deref(), Some("mock-http-sse"));
        assert_eq!(sse_res.server_version.as_deref(), Some("3.1.0"));
        assert_eq!(sse_res.tools.len(), 2);
        assert_eq!(sse_res.tools[0].name, "stream_tool");
        assert_eq!(sse_res.tools[1].name, "ping_tool");
        assert!(
            sse_res.latency_ms < 2_000,
            "incremental SSE probe should finish immediately, took {}ms",
            sse_res.latency_ms
        );

        server_task.abort();
        let _ = fs::remove_dir_all(&orbit_home);
    }
}
