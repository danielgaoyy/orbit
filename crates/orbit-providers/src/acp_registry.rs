//! Curated 30+ ACP Agent Registry catalog, `$ORBIT_HOME/acp_registry.json` custom agent CRUD,
//! optional HTTPS remote registry overlay (`agentclientprotocol/registry`), and live ACP
//! JSON-RPC 2.0 `initialize` handshake probing.

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use orbit_protocol::{AcpAgentAvailability, AcpAgentSource, AcpAgentSpec, AcpProbeResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tracing::warn;

/// Default official ACP Registry CDN URL.
pub const DEFAULT_ACP_REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";

/// Maximum accepted remote registry JSON payload size (5 MiB).
pub const MAX_REMOTE_REGISTRY_BYTES: usize = 5 * 1024 * 1024;

/// Maximum wall-clock timeout for a live ACP `initialize` handshake probe.
pub const ACP_PROBE_TIMEOUT: Duration = Duration::from_secs(8);

/// Upper bound on stdout bytes read during an ACP handshake probe (512 KiB).
const MAX_PROBE_STDOUT_BYTES: usize = 512 * 1024;
/// Upper bound on stderr bytes captured for probe diagnostics (4 KiB).
const MAX_PROBE_STDERR_BYTES: usize = 4 * 1024;

const MAX_CUSTOM_ARGS: usize = 64;
const MAX_CUSTOM_ENV_ENTRIES: usize = 64;
const MAX_ARG_BYTES: usize = 1024;
const MAX_ENV_KEY_BYTES: usize = 128;
const MAX_ENV_VAL_BYTES: usize = 4096;

/// Dynamic-linker, shell-injection, and runtime-hijack environment variables blocked from ACP specs.
const BLOCKED_ACP_ENV_KEYS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
    "DYLD_FALLBACK_LIBRARY_PATH",
    "BASH_ENV",
    "ENV",
    "CDPATH",
    "IFS",
    "NODE_OPTIONS",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PERL5OPT",
    "RUBYOPT",
];

/// Process-wide lock serializing read-modify-write operations on `$ORBIT_HOME/acp_registry.json`.
static REGISTRY_IO_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

pub(crate) fn is_blocked_acp_env_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    upper.starts_with("ORBIT_") || BLOCKED_ACP_ENV_KEYS.contains(&upper.as_str())
}

/// Persisted host-local ACP registry state stored at `$ORBIT_HOME/acp_registry.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedAcpRegistry {
    #[serde(default)]
    pub custom_agents: Vec<AcpAgentSpec>,
    #[serde(default)]
    pub remote_catalog: Vec<AcpAgentSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_refreshed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_registry_url: Option<String>,
}

fn builtin_entry(
    id: &str,
    name: &str,
    description: &str,
    command: &str,
    args: &[&str],
    install_hint: &str,
    homepage: &str,
) -> AcpAgentSpec {
    AcpAgentSpec {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        command: command.to_string(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        env: HashMap::new(),
        install_hint: Some(install_hint.to_string()),
        homepage: Some(homepage.to_string()),
        version: None,
        source: AcpAgentSource::BuiltinCatalog,
        enabled: true,
    }
}

/// Embedded curated catalog of 34 ACP-compatible coding agents aligned with the open
/// `agentclientprotocol/registry` ecosystem.
pub fn builtin_acp_catalog() -> Vec<AcpAgentSpec> {
    vec![
        builtin_entry(
            "claude-acp",
            "Claude Code (ACP)",
            "Anthropic Claude Code exposed via ACP stdio adapter.",
            "claude-code-acp",
            &[],
            "npm install -g @zed-industries/claude-code-acp",
            "https://github.com/zed-industries/claude-code-acp",
        ),
        builtin_entry(
            "codex-acp",
            "Codex (ACP)",
            "OpenAI Codex CLI exposed via ACP stdio adapter.",
            "codex-acp",
            &[],
            "npm install -g @zed-industries/codex-acp",
            "https://github.com/zed-industries/codex-acp",
        ),
        builtin_entry(
            "gemini-cli",
            "Gemini CLI",
            "Google Gemini open-source terminal coding agent with native ACP support.",
            "gemini",
            &["--experimental-acp"],
            "npm install -g @google/gemini-cli",
            "https://github.com/google-gemini/gemini-cli",
        ),
        builtin_entry(
            "opencode",
            "OpenCode",
            "Open-source multi-provider AI coding agent for the terminal.",
            "opencode",
            &["acp"],
            "brew install opencode",
            "https://opencode.ai",
        ),
        builtin_entry(
            "cursor-agent",
            "Cursor Agent",
            "Cursor CLI coding agent running over ACP stdio.",
            "cursor-agent",
            &["acp"],
            "curl https://cursor.com/install -fsS | bash",
            "https://cursor.com/cli",
        ),
        builtin_entry(
            "copilot-cli",
            "GitHub Copilot CLI",
            "GitHub Copilot terminal coding agent with native ACP server mode.",
            "copilot",
            &["--acp"],
            "npm install -g @github/copilot",
            "https://github.com/features/copilot/cli",
        ),
        builtin_entry(
            "goose",
            "Goose",
            "Block's open-source extensible developer agent with ACP support.",
            "goose",
            &["acp"],
            "brew install block-goose-cli",
            "https://block.github.io/goose",
        ),
        builtin_entry(
            "kimi-cli",
            "Kimi CLI",
            "Moonshot AI Kimi terminal coding agent supporting ACP.",
            "kimi",
            &["acp"],
            "uv tool install kimi-cli",
            "https://github.com/MoonshotAI/kimi-cli",
        ),
        builtin_entry(
            "cline",
            "Cline",
            "Autonomous coding agent CLI with ACP integration.",
            "cline",
            &["--acp"],
            "npm install -g cline",
            "https://cline.bot",
        ),
        builtin_entry(
            "mistral-vibe",
            "Mistral Vibe",
            "Mistral AI's minimal terminal coding agent supporting ACP.",
            "vibe-acp",
            &[],
            "uv tool install mistral-vibe",
            "https://github.com/mistralai/mistral-vibe",
        ),
        builtin_entry(
            "auggie",
            "Augment Code",
            "Augment Code CLI powered by codebase context engine over ACP.",
            "auggie",
            &["--acp"],
            "npm install -g @augmentcode/auggie",
            "https://www.augmentcode.com",
        ),
        builtin_entry(
            "qwen-code",
            "Qwen Code",
            "Alibaba Qwen terminal coding agent with ACP mode.",
            "qwen",
            &["--acp"],
            "npm install -g @qwen-code/qwen-code",
            "https://github.com/QwenLM/qwen-code",
        ),
        builtin_entry(
            "amp",
            "Amp",
            "Sourcegraph Amp agentic coding CLI supporting ACP.",
            "amp",
            &["--acp"],
            "npm install -g @sourcegraph/amp",
            "https://ampcode.com",
        ),
        builtin_entry(
            "droid",
            "Factory Droid",
            "Factory AI Droid CLI software engineering agent over ACP.",
            "droid",
            &["acp"],
            "curl -fsSL https://app.factory.ai/cli | sh",
            "https://factory.ai",
        ),
        builtin_entry(
            "aider-acp",
            "Aider (ACP)",
            "AI pair programming in your terminal bridged to ACP.",
            "aider-acp",
            &[],
            "uv tool install aider-acp",
            "https://aider.chat",
        ),
        builtin_entry(
            "roo-code",
            "Roo Code",
            "Roo Code multi-mode coding agent over ACP.",
            "roo-code-acp",
            &[],
            "npm install -g @roocode/acp",
            "https://roocode.com",
        ),
        builtin_entry(
            "kiro",
            "Kiro CLI",
            "AWS Kiro spec-driven terminal coding agent with ACP support.",
            "kiro-cli",
            &["acp"],
            "curl -fsSL https://cli.kiro.dev/install | bash",
            "https://kiro.dev",
        ),
        builtin_entry(
            "amazon-q",
            "Amazon Q Developer",
            "Amazon Q Developer CLI coding assistant over ACP.",
            "q",
            &["acp"],
            "brew install amazon-q",
            "https://aws.amazon.com/q/developer",
        ),
        builtin_entry(
            "openhands",
            "OpenHands",
            "Open-source autonomous software engineering agent with ACP transport.",
            "openhands",
            &["acp"],
            "uv tool install openhands-ai",
            "https://docs.all-hands.dev",
        ),
        builtin_entry(
            "trae",
            "Trae CLI",
            "Trae adaptive AI coding agent CLI supporting ACP.",
            "trae-cli",
            &["acp"],
            "npm install -g @trae-ai/cli",
            "https://www.trae.ai",
        ),
        builtin_entry(
            "windsurf-acp",
            "Windsurf (ACP)",
            "Windsurf Cascade coding agent bridged via ACP.",
            "windsurf-acp",
            &[],
            "npm install -g windsurf-acp",
            "https://windsurf.com",
        ),
        builtin_entry(
            "zed-agent",
            "Zed Agent",
            "Zed's native ACP coding agent runner.",
            "zed-agent",
            &["acp"],
            "brew install zed",
            "https://zed.dev",
        ),
        builtin_entry(
            "junie",
            "JetBrains Junie",
            "JetBrains Junie autonomous coding agent CLI over ACP.",
            "junie",
            &["--acp"],
            "brew install jetbrains/tap/junie",
            "https://www.jetbrains.com/junie",
        ),
        builtin_entry(
            "kilo-code",
            "Kilo Code",
            "Open-source Kilo Code agent CLI with native ACP support.",
            "kilocode",
            &["acp"],
            "npm install -g @kilocode/cli",
            "https://kilocode.ai",
        ),
        builtin_entry(
            "blackbox",
            "Blackbox AI",
            "Blackbox AI CLI coding agent supporting ACP.",
            "blackbox",
            &["acp"],
            "npm install -g blackbox-cli",
            "https://www.blackbox.ai",
        ),
        builtin_entry(
            "warp-oz",
            "Warp Oz",
            "Warp Oz agentic terminal workflow runner over ACP.",
            "oz",
            &["acp"],
            "brew install warp-oz",
            "https://www.warp.dev",
        ),
        builtin_entry(
            "continue-acp",
            "Continue",
            "Continue open-source coding CLI with ACP stdio mode.",
            "cn",
            &["--acp"],
            "npm install -g @continuedev/cli",
            "https://continue.dev",
        ),
        builtin_entry(
            "cody-acp",
            "Sourcegraph Cody",
            "Sourcegraph Cody codebase-aware coding CLI over ACP.",
            "cody",
            &["acp"],
            "npm install -g @sourcegraph/cody",
            "https://sourcegraph.com/cody",
        ),
        builtin_entry(
            "tabby-acp",
            "Tabby",
            "Self-hosted Tabby coding assistant CLI bridged to ACP.",
            "tabby-agent",
            &["--acp"],
            "npm install -g tabby-agent",
            "https://tabby.tabbyml.com",
        ),
        builtin_entry(
            "sweep-acp",
            "Sweep",
            "Sweep AI issue-to-PR coding agent via ACP.",
            "sweep-acp",
            &[],
            "uv tool install sweep-acp",
            "https://sweep.dev",
        ),
        builtin_entry(
            "devin-acp",
            "Devin CLI",
            "Cognition Devin CLI coding agent over ACP.",
            "devin",
            &["acp"],
            "npm install -g @cognition/devin-cli",
            "https://devin.ai",
        ),
        builtin_entry(
            "mentat-acp",
            "Mentat",
            "Mentat multi-file coding assistant bridged to ACP.",
            "mentat-acp",
            &[],
            "uv tool install mentat-acp",
            "https://mentat.ai",
        ),
        builtin_entry(
            "pi-acp",
            "Pi Coding Agent",
            "Minimal extensible Pi coding agent exposed via ACP.",
            "pi-acp",
            &[],
            "npm install -g pi-acp",
            "https://github.com/svkozak/pi-acp",
        ),
        builtin_entry(
            "stakpak",
            "Stakpak",
            "DevOps and infrastructure coding agent supporting ACP.",
            "stakpak",
            &["acp"],
            "brew install stakpak/tap/stakpak",
            "https://stakpak.dev",
        ),
    ]
}

/// Canonical path to the host-local ACP registry JSON file (`$ORBIT_HOME/acp_registry.json`).
pub fn acp_registry_path(home_dir: &Path) -> PathBuf {
    home_dir.join("acp_registry.json")
}

/// Strictly validate an ACP agent specification before persisting or launching it.
pub fn validate_acp_agent_spec(spec: &AcpAgentSpec) -> Result<(), String> {
    let id = spec.id.trim();
    if id.is_empty() || id.len() > 64 {
        return Err("ACP agent id must be 1..=64 characters".to_string());
    }
    let bytes = id.as_bytes();
    if !(bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit()) {
        return Err(
            "ACP agent id must start with a lowercase ASCII letter or digit (`[a-z0-9]`)"
                .to_string(),
        );
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(
            "ACP agent id may only contain lowercase letters, digits, `.`, `_`, and `-`"
                .to_string(),
        );
    }

    let name = spec.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err("ACP agent name must be 1..=128 characters".to_string());
    }
    if name.chars().any(|c| c.is_control()) {
        return Err("ACP agent name must not contain control characters".to_string());
    }

    if spec.description.len() > 1024 {
        return Err("ACP agent description exceeds 1024 bytes".to_string());
    }

    let command = spec.command.trim();
    if command.is_empty() || command.len() > 512 {
        return Err("ACP agent command must be 1..=512 characters".to_string());
    }
    if command.chars().any(char::is_control) {
        return Err("ACP agent command must not contain control characters".to_string());
    }

    if spec.args.len() > MAX_CUSTOM_ARGS {
        return Err(format!(
            "ACP agent args count ({}) exceeds limit ({MAX_CUSTOM_ARGS})",
            spec.args.len()
        ));
    }
    for (idx, arg) in spec.args.iter().enumerate() {
        if arg.len() > MAX_ARG_BYTES {
            return Err(format!(
                "ACP agent arg[{idx}] exceeds {MAX_ARG_BYTES} bytes"
            ));
        }
        if arg.contains('\0') {
            return Err(format!("ACP agent arg[{idx}] must not contain NUL bytes"));
        }
    }

    if spec.source == AcpAgentSource::RemoteRegistry && !spec.env.is_empty() {
        return Err(
            "remote ACP registry entries must not declare environment overrides".to_string(),
        );
    }

    if spec.env.len() > MAX_CUSTOM_ENV_ENTRIES {
        return Err(format!(
            "ACP agent env entry count ({}) exceeds limit ({MAX_CUSTOM_ENV_ENTRIES})",
            spec.env.len()
        ));
    }
    for (k, v) in &spec.env {
        let key = k.trim();
        if key.is_empty() || key.len() > MAX_ENV_KEY_BYTES {
            return Err("ACP agent env variable key must be 1..=128 characters".to_string());
        }
        let kb = key.as_bytes();
        if !(kb[0].is_ascii_alphabetic() || kb[0] == b'_')
            || !kb.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            return Err(format!("invalid ACP agent env variable name `{key}`"));
        }
        if is_blocked_acp_env_key(key) {
            return Err(format!(
                "ACP agent env variable `{key}` is blocked for security"
            ));
        }
        if v.len() > MAX_ENV_VAL_BYTES {
            return Err(format!(
                "ACP agent env value for `{key}` exceeds {MAX_ENV_VAL_BYTES} bytes"
            ));
        }
        if v.contains('\0') {
            return Err(format!(
                "ACP agent env value for `{key}` must not contain NUL bytes"
            ));
        }
    }

    if let Some(ref homepage) = spec.homepage {
        let hp = homepage.trim();
        if hp.len() > 512 {
            return Err("ACP agent homepage URL exceeds 512 bytes".to_string());
        }
        if !hp.is_empty() {
            if hp.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(
                    "ACP agent homepage URL must not contain whitespace or control characters"
                        .to_string(),
                );
            }
            let parsed = reqwest::Url::parse(hp)
                .map_err(|_| "ACP agent homepage must be a valid https:// URL".to_string())?;
            if parsed.scheme() != "https"
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
            {
                return Err("ACP agent homepage must be a valid https:// URL".to_string());
            }
        }
    }

    Ok(())
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    if ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
    {
        return false;
    }
    // 0.0.0.0/8 ("this network")
    if a == 0 {
        return false;
    }
    // CGNAT / Shared Address Space 100.64.0.0/10
    if a == 100 && (b & 0b1100_0000) == 64 {
        return false;
    }
    // IETF Protocol Assignments 192.0.0.0/24
    if a == 192 && b == 0 && c == 0 {
        return false;
    }
    // Benchmarking 198.18.0.0/15
    if a == 198 && (b == 18 || b == 19) {
        return false;
    }
    // Reserved 240.0.0.0/4
    if a >= 240 {
        return false;
    }
    true
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return false;
    }
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    let segs = ip.segments();
    // Deprecated IPv4-compatible ::/96
    if segs[0..6] == [0; 6] {
        let v4 = Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            (segs[6] & 0xff) as u8,
            (segs[7] >> 8) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_public_ipv4(v4);
    }
    // Unique Local Addresses fc00::/7
    if (segs[0] & 0xfe00) == 0xfc00 {
        return false;
    }
    // Link-local fe80::/10 and deprecated site-local fec0::/10
    if (segs[0] & 0xffc0) == 0xfe80 || (segs[0] & 0xffc0) == 0xfec0 {
        return false;
    }
    // Documentation 2001:db8::/32
    if segs[0] == 0x2001 && segs[1] == 0x0db8 {
        return false;
    }
    // 6to4 2002::/16 — check embedded IPv4
    if segs[0] == 0x2002 {
        let v4 = Ipv4Addr::new(
            (segs[1] >> 8) as u8,
            (segs[1] & 0xff) as u8,
            (segs[2] >> 8) as u8,
            (segs[2] & 0xff) as u8,
        );
        return is_public_ipv4(v4);
    }
    // Teredo 2001::/32, Benchmarking 2001:2::/48, Discard 0100::/64, NAT64 64:ff9b::/96
    if (segs[0] == 0x2001 && segs[1] == 0x0000)
        || (segs[0] == 0x2001 && segs[1] == 0x0002 && segs[2] == 0)
        || (segs[0] == 0x0100 && segs[1] == 0 && segs[2] == 0 && segs[3] == 0)
        || (segs[0] == 0x0064 && segs[1] == 0xff9b)
    {
        return false;
    }
    // Require global unicast space (2000::/3)
    (segs[0] & 0xe000) == 0x2000
}

pub(crate) fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

fn parse_and_validate_remote_registry_url(raw_url: &str) -> Result<reqwest::Url, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() || trimmed.len() > 1024 {
        return Err("registry URL must be 1..=1024 characters".to_string());
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(
            "remote ACP registry URL must not contain whitespace or control characters".to_string(),
        );
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|_| "invalid remote ACP registry URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("remote ACP registry URL must use https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("remote ACP registry URL must not include credentials".to_string());
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "remote ACP registry URL missing host".to_string())?;
    let unbracketed = host_raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host_raw);
    if let Ok(ip) = unbracketed.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err("remote ACP registry URL must point to a public HTTPS host".to_string());
        }
        return Ok(parsed);
    }
    let host = unbracketed.to_ascii_lowercase();
    if host.is_empty()
        || !host.contains('.')
        || host.starts_with('.')
        || host.ends_with('.')
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
    {
        return Err("remote ACP registry URL must point to a public HTTPS host".to_string());
    }
    Ok(parsed)
}

/// Strictly validate a remote ACP registry URL (HTTPS-only, public host, no credentials/spaces).
pub fn validate_remote_registry_url(raw_url: &str) -> Result<String, String> {
    parse_and_validate_remote_registry_url(raw_url).map(|u| u.to_string())
}

fn load_persisted_acp_registry_strict_unlocked(
    home_dir: &Path,
) -> Result<PersistedAcpRegistry, String> {
    let path = acp_registry_path(home_dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PersistedAcpRegistry::default());
        }
        Err(e) => {
            return Err(format!("failed to read `{}`: {e}", path.display()));
        }
    };
    serde_json::from_slice(&bytes).map_err(|e| format!("failed to parse `{}`: {e}", path.display()))
}

/// Load persisted custom + cached remote ACP registry state from `$ORBIT_HOME/acp_registry.json`.
/// Returns defaults on missing/corrupt file for read-only listing resilience.
pub fn load_persisted_acp_registry(home_dir: &Path) -> PersistedAcpRegistry {
    let _guard = REGISTRY_IO_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match load_persisted_acp_registry_strict_unlocked(home_dir) {
        Ok(reg) => reg,
        Err(e) => {
            warn!(path = %acp_registry_path(home_dir).display(), error = %e, "Failed to load acp_registry.json; using defaults");
            PersistedAcpRegistry::default()
        }
    }
}

fn save_persisted_acp_registry_unlocked(
    home_dir: &Path,
    reg: &PersistedAcpRegistry,
) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create orbit home `{}`: {e}", home_dir.display()))?;
    let target = acp_registry_path(home_dir);
    let tmp = home_dir.join(format!(
        ".acp_registry.json.tmp.{}",
        uuid::Uuid::new_v4().simple()
    ));
    let payload = serde_json::to_vec_pretty(reg)
        .map_err(|e| format!("failed to serialize acp_registry.json: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("failed to create temp registry file: {e}"))?;
        f.write_all(&payload)
            .and_then(|_| f.sync_all())
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("failed to write temp registry file: {e}")
            })?;
    }
    #[cfg(not(unix))]
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| format!("failed to create temp registry file: {e}"))?;
        f.write_all(&payload)
            .and_then(|_| f.sync_all())
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("failed to write temp registry file: {e}")
            })?;
    }

    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("failed to atomically replace acp_registry.json: {e}")
    })
}

/// Atomically persist `$ORBIT_HOME/acp_registry.json` with restrictive `0600` Unix permissions.
pub fn save_persisted_acp_registry(
    home_dir: &Path,
    reg: &PersistedAcpRegistry,
) -> Result<(), String> {
    let _guard = REGISTRY_IO_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    save_persisted_acp_registry_unlocked(home_dir, reg)
}

/// Normalize & upsert a user-defined custom ACP agent in `$ORBIT_HOME/acp_registry.json`.
pub fn upsert_custom_acp_agent(
    home_dir: &Path,
    mut spec: AcpAgentSpec,
) -> Result<AcpAgentSpec, String> {
    spec.id = spec.id.trim().to_string();
    spec.name = spec.name.trim().to_string();
    spec.description = spec.description.trim().to_string();
    spec.command = spec.command.trim().to_string();
    spec.install_hint = spec
        .install_hint
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    spec.homepage = spec
        .homepage
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    spec.version = spec
        .version
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    spec.source = AcpAgentSource::Custom;

    validate_acp_agent_spec(&spec)?;

    let _guard = REGISTRY_IO_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut persisted = load_persisted_acp_registry_strict_unlocked(home_dir)?;
    if let Some(slot) = persisted
        .custom_agents
        .iter_mut()
        .find(|existing| existing.id == spec.id)
    {
        *slot = spec.clone();
    } else {
        persisted.custom_agents.push(spec.clone());
    }
    save_persisted_acp_registry_unlocked(home_dir, &persisted)?;
    Ok(spec)
}

/// Delete a user-defined custom ACP agent from `$ORBIT_HOME/acp_registry.json`.
/// Returns `true` if a custom agent with `agent_id` existed and was removed.
pub fn delete_custom_acp_agent(home_dir: &Path, agent_id: &str) -> Result<bool, String> {
    let clean_id = agent_id.trim();
    if clean_id.is_empty() {
        return Err("ACP agent id must not be empty".to_string());
    }
    let _guard = REGISTRY_IO_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut persisted = load_persisted_acp_registry_strict_unlocked(home_dir)?;
    let before = persisted.custom_agents.len();
    persisted.custom_agents.retain(|a| a.id != clean_id);
    let removed = persisted.custom_agents.len() != before;
    if removed {
        save_persisted_acp_registry_unlocked(home_dir, &persisted)?;
    }
    Ok(removed)
}

/// Resolve an ACP agent command to a concrete executable path on this host.
pub fn resolve_acp_agent_command(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rel) = trimmed.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
            .map(PathBuf::from)?;
        let candidate = home.join(rel);
        return crate::is_executable_file(&candidate).then_some(candidate);
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        let candidate = PathBuf::from(trimmed);
        return crate::is_executable_file(&candidate).then_some(candidate);
    }
    crate::find_executable(trimmed)
}

/// Merge built-in catalog, cached remote registry overlay, and user custom agents (custom wins).
pub fn merged_acp_specs(home_dir: &Path) -> Vec<AcpAgentSpec> {
    let persisted = load_persisted_acp_registry(home_dir);
    let mut by_id: BTreeMap<String, AcpAgentSpec> = BTreeMap::new();

    for spec in builtin_acp_catalog() {
        by_id.insert(spec.id.clone(), spec);
    }
    for mut remote in persisted.remote_catalog {
        remote.source = AcpAgentSource::RemoteRegistry;
        if validate_acp_agent_spec(&remote).is_ok() {
            by_id.insert(remote.id.clone(), remote);
        }
    }
    for mut custom in persisted.custom_agents {
        custom.source = AcpAgentSource::Custom;
        if validate_acp_agent_spec(&custom).is_ok() {
            by_id.insert(custom.id.clone(), custom);
        }
    }

    by_id.into_values().collect()
}

/// Lookup a single merged `AcpAgentSpec` by ID on this host.
pub fn get_acp_agent_spec(home_dir: &Path, agent_id: &str) -> Option<AcpAgentSpec> {
    let clean = agent_id.trim();
    if clean.is_empty() {
        return None;
    }
    merged_acp_specs(home_dir)
        .into_iter()
        .find(|s| s.id == clean)
}

/// List all known ACP agents with live host binary availability resolution.
/// Sorted with installed agents first (custom before catalog, then alphabetical).
pub fn list_acp_agents(home_dir: &Path) -> Vec<AcpAgentAvailability> {
    let mut items: Vec<AcpAgentAvailability> = merged_acp_specs(home_dir)
        .into_iter()
        .map(|spec| {
            let resolved = resolve_acp_agent_command(&spec.command);
            AcpAgentAvailability {
                installed: resolved.is_some(),
                resolved_path: resolved.map(|p| p.to_string_lossy().to_string()),
                spec,
            }
        })
        .collect();

    items.sort_by(|a, b| {
        b.installed
            .cmp(&a.installed)
            .then_with(|| {
                let a_custom = a.spec.source == AcpAgentSource::Custom;
                let b_custom = b.spec.source == AcpAgentSource::Custom;
                b_custom.cmp(&a_custom)
            })
            .then_with(|| {
                a.spec
                    .name
                    .to_ascii_lowercase()
                    .cmp(&b.spec.name.to_ascii_lowercase())
            })
            .then_with(|| a.spec.id.cmp(&b.spec.id))
    });
    items
}

/// Return `true` when at least one enabled ACP agent is installed on this host.
pub fn has_any_installed_acp_agent(home_dir: &Path) -> bool {
    list_acp_agents(home_dir)
        .into_iter()
        .any(|a| a.installed && a.spec.enabled)
}

fn current_platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}

/// Parse an official `agentclientprotocol/registry` JSON document (or flat array of specs) into
/// validated `AcpAgentSpec` entries tagged with `AcpAgentSource::RemoteRegistry`.
pub fn parse_remote_registry_json(bytes: &[u8]) -> Result<Vec<AcpAgentSpec>, String> {
    if bytes.len() > MAX_REMOTE_REGISTRY_BYTES {
        return Err(format!(
            "remote ACP registry exceeds {} MiB size cap",
            MAX_REMOTE_REGISTRY_BYTES / (1024 * 1024)
        ));
    }
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|e| format!("failed to parse remote ACP registry JSON: {e}"))?;

    let entries = if let Some(arr) = root.as_array() {
        arr.as_slice()
    } else if let Some(arr) = root.get("agents").and_then(Value::as_array) {
        arr.as_slice()
    } else {
        return Err("remote ACP registry JSON missing `agents` array".to_string());
    };

    let plat = current_platform_key();
    let mut parsed = Vec::new();
    for item in entries {
        let Some(id) = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(id);
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let version = item
            .get("version")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let homepage = item
            .get("homepage")
            .or_else(|| item.get("repository"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let mut command: Option<String> = item
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let mut args: Vec<String> = item
            .get("args")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        // Untrusted remote registry entries are never allowed to inject environment overrides.
        let env: HashMap<String, String> = HashMap::new();
        let mut install_hint: Option<String> = item
            .get("install_hint")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        // Parse official ACP Registry `distribution` schema (`binary`, `npx`, `uvx`)
        if command.is_none()
            && let Some(dist) = item.get("distribution").and_then(Value::as_object)
        {
            if let Some(bin_map) = dist.get("binary").and_then(Value::as_object)
                && let Some(plat_obj) = bin_map
                    .get(&plat)
                    .or_else(|| bin_map.values().next())
                    .and_then(Value::as_object)
                && let Some(cmd) = plat_obj
                    .get("cmd")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                let bin_name = Path::new(cmd)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(cmd);
                command = Some(bin_name.to_string());
                if let Some(a) = plat_obj.get("args").and_then(Value::as_array) {
                    args = a
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned)
                        .collect();
                }
            } else if let Some(npx_obj) = dist.get("npx").and_then(Value::as_object)
                && let Some(pkg) = npx_obj
                    .get("package")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                command = Some("npx".to_string());
                args = vec!["-y".to_string(), pkg.to_string()];
                if let Some(extra) = npx_obj.get("args").and_then(Value::as_array) {
                    args.extend(
                        extra
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
                install_hint = install_hint.or_else(|| Some(format!("npx -y {pkg}")));
            } else if let Some(uvx_obj) = dist.get("uvx").and_then(Value::as_object)
                && let Some(pkg) = uvx_obj
                    .get("package")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            {
                command = Some("uvx".to_string());
                args = vec![pkg.to_string()];
                if let Some(extra) = uvx_obj.get("args").and_then(Value::as_array) {
                    args.extend(
                        extra
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToOwned::to_owned),
                    );
                }
                install_hint = install_hint.or_else(|| Some(format!("uvx {pkg}")));
            }
        }

        let Some(cmd) = command else {
            continue;
        };

        let spec = AcpAgentSpec {
            id: id.to_string(),
            name: name.to_string(),
            description,
            command: cmd,
            args,
            env,
            install_hint,
            homepage,
            version,
            source: AcpAgentSource::RemoteRegistry,
            enabled: true,
        };
        if validate_acp_agent_spec(&spec).is_ok() {
            parsed.push(spec);
        }
    }
    Ok(parsed)
}

/// Fetch and cache the remote ACP registry overlay with strict HTTPS/SSRF validation, DNS pinning,
/// no-redirect enforcement, timeout, 5 MiB cap, and graceful offline fallback (preserving existing
/// cached overlay + builtin catalog).
pub async fn refresh_acp_registry(
    home_dir: &Path,
    registry_url: Option<&str>,
) -> Result<(Vec<AcpAgentAvailability>, Option<String>), String> {
    let raw_url = match registry_url.map(str::trim).filter(|s| !s.is_empty()) {
        Some(custom) => custom,
        None => DEFAULT_ACP_REGISTRY_URL,
    };
    let parsed_url = parse_and_validate_remote_registry_url(raw_url)?;
    let url_str = parsed_url.to_string();
    let host_raw = parsed_url
        .host_str()
        .ok_or_else(|| "remote ACP registry URL missing host".to_string())?
        .to_string();
    let host_unbracketed = host_raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(&host_raw)
        .to_string();
    let port = parsed_url.port_or_known_default().unwrap_or(443);

    // Resolve DNS up-front and verify EVERY resolved IP is globally public before connecting,
    // pinning the validated address(es) to prevent DNS rebinding and disabling HTTP redirects.
    let mut warning: Option<String> = None;
    let pinned_addrs: Option<Vec<SocketAddr>> = if let Ok(ip) = host_unbracketed.parse::<IpAddr>() {
        if !is_public_ip(ip) {
            return Err("remote ACP registry host is not a public IP address".to_string());
        }
        Some(vec![SocketAddr::new(ip, port)])
    } else {
        match tokio::time::timeout(
            Duration::from_secs(5),
            tokio::net::lookup_host((host_unbracketed.as_str(), port)),
        )
        .await
        {
            Ok(Ok(iter)) => {
                let addrs: Vec<SocketAddr> = iter.collect();
                if addrs.is_empty() {
                    warning = Some(
                        "Offline fallback: DNS lookup returned no addresses for remote registry."
                            .to_string(),
                    );
                    None
                } else if addrs.iter().any(|a| !is_public_ip(a.ip())) {
                    return Err(
                        "remote ACP registry host resolved to a non-public IP address".to_string(),
                    );
                } else {
                    Some(addrs)
                }
            }
            Ok(Err(e)) => {
                warn!(url = %url_str, error = %e, "DNS lookup failed for remote ACP registry; using cached/builtin catalog");
                warning = Some(format!(
                    "Offline fallback: could not resolve remote registry host ({e})."
                ));
                None
            }
            Err(_) => {
                warn!(url = %url_str, "DNS lookup timed out for remote ACP registry; using cached/builtin catalog");
                warning =
                    Some("Offline fallback: DNS lookup timed out for remote registry.".to_string());
                None
            }
        }
    };

    if let Some(addrs) = pinned_addrs {
        let mut builder = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(format!("orbit/{}", env!("CARGO_PKG_VERSION")));
        if host_unbracketed.parse::<IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(&host_unbracketed, &addrs);
        }
        let client = builder
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;

        match client.get(parsed_url).send().await {
            Ok(mut resp) => {
                if resp.status().is_redirection() {
                    let status = resp.status();
                    warn!(url = %url_str, %status, "Remote ACP registry returned redirect; refused for SSRF protection");
                    warning = Some(format!(
                        "Remote registry returned redirect HTTP {status} (redirects disabled); using cached/builtin catalog."
                    ));
                } else if !resp.status().is_success() {
                    let status = resp.status();
                    warn!(url = %url_str, %status, "Remote ACP registry returned non-200 status; using cached/builtin catalog");
                    warning = Some(format!(
                        "Remote registry returned HTTP {status}; using cached/builtin catalog."
                    ));
                } else {
                    let mut body = Vec::new();
                    let mut exceeded = false;
                    let mut stream_err: Option<String> = None;
                    loop {
                        match resp.chunk().await {
                            Ok(Some(chunk)) => {
                                if body.len().saturating_add(chunk.len())
                                    > MAX_REMOTE_REGISTRY_BYTES
                                {
                                    exceeded = true;
                                    break;
                                }
                                body.extend_from_slice(&chunk);
                            }
                            Ok(None) => break,
                            Err(e) => {
                                stream_err = Some(e.to_string());
                                break;
                            }
                        }
                    }
                    if let Some(err) = stream_err {
                        warn!(url = %url_str, error = %err, "Remote ACP registry stream interrupted; keeping cached/builtin catalog");
                        warning = Some(format!(
                            "Remote registry download interrupted ({err}); keeping cached/builtin catalog."
                        ));
                    } else if exceeded {
                        warn!(url = %url_str, "Remote ACP registry exceeded 5 MiB cap; keeping cached/builtin catalog");
                        warning = Some(
                            "Remote registry exceeded 5 MiB cap; keeping cached/builtin catalog."
                                .to_string(),
                        );
                    } else {
                        match parse_remote_registry_json(&body) {
                            Ok(remote_agents) if !remote_agents.is_empty() => {
                                let home_owned = home_dir.to_path_buf();
                                let url_owned = url_str.clone();
                                let save_res = tokio::task::spawn_blocking(move || {
                                    let _guard =
                                        REGISTRY_IO_LOCK.lock().unwrap_or_else(|p| p.into_inner());
                                    let mut persisted =
                                        load_persisted_acp_registry_strict_unlocked(&home_owned)?;
                                    persisted.remote_catalog = remote_agents;
                                    persisted.remote_refreshed_at = Some(Utc::now());
                                    persisted.remote_registry_url = Some(url_owned);
                                    save_persisted_acp_registry_unlocked(&home_owned, &persisted)
                                })
                                .await
                                .map_err(|e| format!("registry save task panicked: {e}"))?;
                                if let Err(e) = save_res {
                                    warn!(error = %e, "Failed to cache refreshed remote ACP registry");
                                    warning = Some(format!("Failed to cache remote registry: {e}"));
                                }
                            }
                            Ok(_) => {
                                warning = Some(
                                    "Remote registry contained no valid ACP agents; keeping cached/builtin catalog."
                                        .to_string(),
                                );
                            }
                            Err(e) => {
                                warn!(url = %url_str, error = %e, "Failed to parse remote ACP registry; keeping cached/builtin catalog");
                                warning = Some(format!(
                                    "Failed to parse remote registry ({e}); keeping cached/builtin catalog."
                                ));
                            }
                        }
                    }
                }
            }
            Err(e) => {
                warn!(url = %url_str, error = %e, "Offline or unreachable remote ACP registry; using cached/builtin catalog");
                warning = Some(format!(
                    "Offline fallback: could not reach remote registry ({e})."
                ));
            }
        }
    }

    let home_owned = home_dir.to_path_buf();
    let agents = tokio::task::spawn_blocking(move || list_acp_agents(&home_owned))
        .await
        .map_err(|e| format!("failed to list ACP agents: {e}"))?;
    Ok((agents, warning))
}

fn summarize_agent_capabilities(caps: Option<&Value>) -> Vec<String> {
    let Some(obj) = caps.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if obj.get("loadSession").and_then(Value::as_bool) == Some(true) {
        out.push("loadSession".to_string());
    }
    if let Some(prompt_caps) = obj.get("promptCapabilities").and_then(Value::as_object) {
        if prompt_caps.get("image").and_then(Value::as_bool) == Some(true) {
            out.push("prompt.image".to_string());
        }
        if prompt_caps.get("audio").and_then(Value::as_bool) == Some(true) {
            out.push("prompt.audio".to_string());
        }
        if prompt_caps.get("embeddedContext").and_then(Value::as_bool) == Some(true) {
            out.push("prompt.embeddedContext".to_string());
        }
    }
    if let Some(mcp_caps) = obj.get("mcpCapabilities").and_then(Value::as_object) {
        if mcp_caps.get("http").and_then(Value::as_bool) == Some(true) {
            out.push("mcp.http".to_string());
        }
        if mcp_caps.get("sse").and_then(Value::as_bool) == Some(true) {
            out.push("mcp.sse".to_string());
        }
    }
    if let Some(sess_caps) = obj.get("sessionCapabilities").and_then(Value::as_object) {
        for (k, v) in sess_caps {
            if v.is_object() || v.as_bool() == Some(true) {
                out.push(format!("session.{k}"));
            }
        }
    }
    out
}

fn extract_auth_methods(val: Option<&Value>) -> Vec<String> {
    val.and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("name")
                        .or_else(|| item.get("id"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn kill_process_group_and_reap(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id()
        && pid > 1
    {
        // Because the child was spawned with `process_group(0)`, signaling `-{pid}` cleanly
        // terminates the entire process tree (e.g. `npx`/`uvx` grandchildren) without unsafe code.
        let pgid_arg = format!("-{pid}");
        let _ = Command::new("/bin/kill")
            .args(["-s", "KILL", "--", &pgid_arg])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let pid_str = pid.to_string();
        let mut kill_cmd = Command::new("taskkill");
        kill_cmd
            .args(["/PID", &pid_str, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let _ = kill_cmd.status().await;
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

fn format_stderr_tail(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let max_chars = 320usize;
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        Some(trimmed.to_string())
    } else {
        let tail: String = chars[chars.len() - max_chars..].iter().collect();
        Some(format!("…{tail}"))
    }
}

/// Spawn an ACP agent briefly in its own process group with `.kill_on_drop(true)` and an 8-second
/// timeout, perform an ACP JSON-RPC 2.0 `initialize` handshake with bounded stdout/stderr caps,
/// extract server info/capabilities/auth methods, and terminate cleanly with zero leaked children.
pub async fn probe_acp_agent(spec: &AcpAgentSpec, cwd: Option<&Path>) -> AcpProbeResult {
    let started = Instant::now();
    let elapsed_ms = || started.elapsed().as_millis() as u64;

    if let Err(err) = validate_acp_agent_spec(spec) {
        return AcpProbeResult {
            agent_id: spec.id.clone(),
            ok: false,
            protocol_version: None,
            server_name: None,
            server_version: None,
            capabilities_summary: Vec::new(),
            auth_methods: Vec::new(),
            latency_ms: elapsed_ms(),
            error: Some(err),
        };
    }

    let cmd_str = spec.command.clone();
    let resolved = tokio::task::spawn_blocking(move || resolve_acp_agent_command(&cmd_str))
        .await
        .ok()
        .flatten();
    let Some(resolved) = resolved else {
        return AcpProbeResult {
            agent_id: spec.id.clone(),
            ok: false,
            protocol_version: None,
            server_name: None,
            server_version: None,
            capabilities_summary: Vec::new(),
            auth_methods: Vec::new(),
            latency_ms: elapsed_ms(),
            error: Some(format!("Executable `{}` not found on PATH", spec.command)),
        };
    };

    let mut cmd = Command::new(&resolved);
    cmd.args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        cmd.creation_flags(0x0000_0200 | 0x0800_0000);
    }
    crate::shell_snapshot::apply_cached_shell_env(&mut cmd);
    cmd.env("NO_COLOR", "1");
    if spec.source != AcpAgentSource::RemoteRegistry {
        for (k, v) in &spec.env {
            if !is_blocked_acp_env_key(k) {
                cmd.env(k, v);
            }
        }
    }
    if let Some(dir) = cwd
        && dir.is_dir()
    {
        cmd.current_dir(dir);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return AcpProbeResult {
                agent_id: spec.id.clone(),
                ok: false,
                protocol_version: None,
                server_name: None,
                server_version: None,
                capabilities_summary: Vec::new(),
                auth_methods: Vec::new(),
                latency_ms: elapsed_ms(),
                error: Some(format!("Failed to spawn `{}`: {e}", resolved.display())),
            };
        }
    };

    let Some(mut stdin) = child.stdin.take() else {
        kill_process_group_and_reap(&mut child).await;
        return AcpProbeResult {
            agent_id: spec.id.clone(),
            ok: false,
            protocol_version: None,
            server_name: None,
            server_version: None,
            capabilities_summary: Vec::new(),
            auth_methods: Vec::new(),
            latency_ms: elapsed_ms(),
            error: Some("Failed to capture child stdin".to_string()),
        };
    };
    let Some(stdout) = child.stdout.take() else {
        kill_process_group_and_reap(&mut child).await;
        return AcpProbeResult {
            agent_id: spec.id.clone(),
            ok: false,
            protocol_version: None,
            server_name: None,
            server_version: None,
            capabilities_summary: Vec::new(),
            auth_methods: Vec::new(),
            latency_ms: elapsed_ms(),
            error: Some("Failed to capture child stdout".to_string()),
        };
    };
    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut buf = Vec::new();
            let _ = stderr
                .take(MAX_PROBE_STDERR_BYTES as u64)
                .read_to_end(&mut buf)
                .await;
            buf
        })
    });

    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    });
    let mut wire = init_req.to_string();
    wire.push('\n');

    let probe_outcome = tokio::time::timeout(ACP_PROBE_TIMEOUT, async {
        stdin
            .write_all(wire.as_bytes())
            .await
            .map_err(|e| format!("Failed writing ACP initialize request: {e}"))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed flushing ACP initialize request: {e}"))?;

        let bounded_stdout = stdout.take(MAX_PROBE_STDOUT_BYTES as u64);
        let mut reader = BufReader::new(bounded_stdout).lines();
        let mut total_bytes: usize = 0;
        while let Some(line) = reader
            .next_line()
            .await
            .map_err(|e| format!("Failed reading ACP stdout: {e}"))?
        {
            total_bytes = total_bytes.saturating_add(line.len().saturating_add(1));
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
                continue;
            };
            let matches_id = val
                .get("id")
                .is_some_and(|id| id == &json!(1) || id == &json!("1"));
            if !matches_id {
                continue;
            }
            if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                let msg = err
                    .get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| err.to_string());
                return Err(format!("ACP initialize error: {msg}"));
            }
            let res = val.get("result").cloned().unwrap_or(Value::Null);
            let protocol_version = res.get("protocolVersion").and_then(|v| {
                v.as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| v.as_i64().map(|n| n.to_string()))
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
            let server_name = res
                .pointer("/agentInfo/name")
                .or_else(|| res.pointer("/agentInfo/title"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
            let server_version = res
                .pointer("/agentInfo/version")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned);
            let capabilities_summary = summarize_agent_capabilities(res.get("agentCapabilities"));
            let auth_methods = extract_auth_methods(res.get("authMethods"));
            return Ok((
                protocol_version,
                server_name,
                server_version,
                capabilities_summary,
                auth_methods,
            ));
        }

        if total_bytes >= MAX_PROBE_STDOUT_BYTES {
            Err("ACP agent stdout exceeded 512 KiB before responding to initialize".to_string())
        } else {
            Err("ACP agent exited before responding to initialize".to_string())
        }
    })
    .await;

    drop(stdin);
    kill_process_group_and_reap(&mut child).await;

    let stderr_tail = if let Some(task) = stderr_task {
        tokio::time::timeout(Duration::from_millis(250), task)
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|bytes| format_stderr_tail(&bytes))
    } else {
        None
    };

    match probe_outcome {
        Ok(Ok((
            protocol_version,
            server_name,
            server_version,
            capabilities_summary,
            auth_methods,
        ))) => AcpProbeResult {
            agent_id: spec.id.clone(),
            ok: true,
            protocol_version,
            server_name,
            server_version,
            capabilities_summary,
            auth_methods,
            latency_ms: elapsed_ms(),
            error: None,
        },
        Ok(Err(err)) => {
            let full_err = match stderr_tail {
                Some(tail) => format!("{err} (stderr: {tail})"),
                None => err,
            };
            AcpProbeResult {
                agent_id: spec.id.clone(),
                ok: false,
                protocol_version: None,
                server_name: None,
                server_version: None,
                capabilities_summary: Vec::new(),
                auth_methods: Vec::new(),
                latency_ms: elapsed_ms(),
                error: Some(full_err),
            }
        }
        Err(_) => {
            let base = format!(
                "ACP initialize timed out after {}s",
                ACP_PROBE_TIMEOUT.as_secs()
            );
            let full_err = match stderr_tail {
                Some(tail) => format!("{base} (stderr: {tail})"),
                None => base,
            };
            AcpProbeResult {
                agent_id: spec.id.clone(),
                ok: false,
                protocol_version: None,
                server_name: None,
                server_version: None,
                capabilities_summary: Vec::new(),
                auth_methods: Vec::new(),
                latency_ms: elapsed_ms(),
                error: Some(full_err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn builtin_catalog_has_at_least_30_unique_valid_entries() {
        let catalog = builtin_acp_catalog();
        assert!(
            catalog.len() >= 30,
            "expected >= 30 curated ACP agents, got {}",
            catalog.len()
        );
        let mut seen = HashSet::new();
        for spec in &catalog {
            assert!(
                seen.insert(spec.id.clone()),
                "duplicate ACP catalog id `{}`",
                spec.id
            );
            validate_acp_agent_spec(spec)
                .unwrap_or_else(|e| panic!("invalid builtin spec `{}`: {e}", spec.id));
            assert_eq!(spec.source, AcpAgentSource::BuiltinCatalog);
            assert!(spec.enabled);
        }
        for required in [
            "claude-acp",
            "codex-acp",
            "gemini-cli",
            "opencode",
            "cursor-agent",
            "copilot-cli",
            "goose",
            "kimi-cli",
            "cline",
            "mistral-vibe",
            "auggie",
            "qwen-code",
            "amp",
            "droid",
            "aider-acp",
            "roo-code",
            "kiro",
            "amazon-q",
            "openhands",
            "trae",
            "windsurf-acp",
            "zed-agent",
            "junie",
            "kilo-code",
            "blackbox",
            "warp-oz",
            "continue-acp",
            "cody-acp",
            "tabby-acp",
            "sweep-acp",
            "devin-acp",
            "mentat-acp",
        ] {
            assert!(
                seen.contains(required),
                "expected builtin catalog to contain `{required}`"
            );
        }
    }

    #[test]
    fn custom_acp_agent_validation_and_atomic_crud() {
        let tmp_home =
            std::env::temp_dir().join(format!("orbit_acp_reg_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");

        // Reject invalid IDs, empty commands, invalid env keys, oversized args
        let bad_id = AcpAgentSpec {
            id: "Bad Space ID!".into(),
            name: "Bad".into(),
            description: String::new(),
            command: "echo".into(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        assert!(upsert_custom_acp_agent(&tmp_home, bad_id).is_err());

        let bad_cmd = AcpAgentSpec {
            id: "my-agent".into(),
            name: "My Agent".into(),
            description: String::new(),
            command: "   ".into(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        assert!(upsert_custom_acp_agent(&tmp_home, bad_cmd).is_err());

        let bad_env = AcpAgentSpec {
            id: "my-agent".into(),
            name: "My Agent".into(),
            description: String::new(),
            command: "my-acp".into(),
            args: vec![],
            env: HashMap::from([("123-BAD".into(), "val".into())]),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        assert!(upsert_custom_acp_agent(&tmp_home, bad_env).is_err());

        // Reject non-HTTPS / local remote registry URLs
        assert!(validate_remote_registry_url("http://example.com/reg.json").is_err());
        assert!(validate_remote_registry_url("file:///etc/passwd").is_err());
        assert!(validate_remote_registry_url("https://localhost/reg.json").is_err());
        assert!(validate_remote_registry_url(DEFAULT_ACP_REGISTRY_URL).is_ok());

        // Valid custom upsert + permissions + reload + delete
        let valid = AcpAgentSpec {
            id: "acme-coder".into(),
            name: "Acme Coder".into(),
            description: "Internal ACP agent".into(),
            command: "/usr/local/bin/acme-acp".into(),
            args: vec!["--stdio".into()],
            env: HashMap::from([("ACME_ENV".into(), "prod".into())]),
            install_hint: None,
            homepage: Some("https://acme.example.com".into()),
            version: Some("0.2.0".into()),
            source: AcpAgentSource::BuiltinCatalog, // should be forced to Custom
            enabled: true,
        };
        let saved = upsert_custom_acp_agent(&tmp_home, valid).expect("upsert valid");
        assert_eq!(saved.source, AcpAgentSource::Custom);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(acp_registry_path(&tmp_home)).expect("metadata");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }

        let found = get_acp_agent_spec(&tmp_home, "acme-coder").expect("found custom");
        assert_eq!(found.name, "Acme Coder");
        assert_eq!(found.env.get("ACME_ENV").map(String::as_str), Some("prod"));

        // Update existing custom agent
        let mut updated = found.clone();
        updated.name = "Acme Coder v2".into();
        upsert_custom_acp_agent(&tmp_home, updated).expect("update custom");
        assert_eq!(
            get_acp_agent_spec(&tmp_home, "acme-coder")
                .expect("reloaded")
                .name,
            "Acme Coder v2"
        );

        // Delete custom agent
        assert!(delete_custom_acp_agent(&tmp_home, "acme-coder").expect("delete"));
        assert!(!delete_custom_acp_agent(&tmp_home, "acme-coder").expect("delete twice"));
        assert!(get_acp_agent_spec(&tmp_home, "acme-coder").is_none());

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[test]
    fn parse_official_remote_registry_format() {
        let sample = json!({
            "version": "1.0.0",
            "agents": [
                {
                    "id": "remote-demo",
                    "name": "Remote Demo Agent",
                    "version": "2.1.0",
                    "description": "Fetched from remote registry",
                    "repository": "https://github.com/example/remote-demo",
                    "distribution": {
                        "npx": {
                            "package": "@example/remote-demo-acp",
                            "args": ["--stdio"]
                        }
                    }
                }
            ]
        });
        let parsed =
            parse_remote_registry_json(sample.to_string().as_bytes()).expect("parse remote");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "remote-demo");
        assert_eq!(parsed[0].command, "npx");
        assert_eq!(
            parsed[0].args,
            vec!["-y", "@example/remote-demo-acp", "--stdio"]
        );
        assert_eq!(parsed[0].source, AcpAgentSource::RemoteRegistry);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_acp_agent_handshake_success_and_cleanup() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("orbit_acp_probe_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("mock-acp-agent");
        std::fs::write(
            &script,
            r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1,"agentInfo":{"name":"MockACP","version":"0.9.1"},"agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true},"mcpCapabilities":{"http":true}},"authMethods":[{"id":"api-key","name":"API Key"}]}}'
      ;;
  esac
done
"#,
        )
        .expect("write script");
        let mut perms = std::fs::metadata(&script).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod");

        let spec = AcpAgentSpec {
            id: "mock-acp".into(),
            name: "Mock ACP".into(),
            description: "Deterministic mock".into(),
            command: script.to_string_lossy().to_string(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };

        let result = probe_acp_agent(&spec, Some(&dir)).await;
        assert!(result.ok, "probe failed: {:?}", result.error);
        assert_eq!(result.protocol_version.as_deref(), Some("1"));
        assert_eq!(result.server_name.as_deref(), Some("MockACP"));
        assert_eq!(result.server_version.as_deref(), Some("0.9.1"));
        assert!(
            result
                .capabilities_summary
                .contains(&"loadSession".to_string())
        );
        assert!(
            result
                .capabilities_summary
                .contains(&"mcp.http".to_string())
        );
        assert_eq!(result.auth_methods, vec!["API Key".to_string()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_registry_ssrf_and_security_validation() {
        // SSRF / non-public IP / credential / scheme rejections
        for bad_url in [
            "http://cdn.agentclientprotocol.com/registry.json",
            "file:///etc/passwd",
            "https://169.254.169.254/latest/meta-data/",
            "https://10.0.0.1/registry.json",
            "https://172.16.0.5/registry.json",
            "https://192.168.1.1/registry.json",
            "https://100.64.0.1/registry.json",
            "https://127.0.0.2/registry.json",
            "https://0.0.0.0/registry.json",
            "https://[::1]/registry.json",
            "https://[fe80::1]/registry.json",
            "https://[fc00::1]/registry.json",
            "https://[::ffff:127.0.0.1]/registry.json",
            "https://[::ffff:169.254.169.254]/registry.json",
            "https://user:pass@cdn.agentclientprotocol.com/registry.json",
            "https://token@cdn.agentclientprotocol.com/registry.json",
            "https://localhost/registry.json",
            "https://meta.local/registry.json",
            "https://intranet.internal/registry.json",
            "https://cdn.example.com/reg\n.json",
        ] {
            assert!(
                validate_remote_registry_url(bad_url).is_err(),
                "expected SSRF/URL rejection for `{bad_url}`"
            );
        }

        // Valid public HTTPS URLs pass
        assert!(validate_remote_registry_url(DEFAULT_ACP_REGISTRY_URL).is_ok());
        assert!(validate_remote_registry_url("https://1.1.1.1/registry.json").is_ok());

        // Control chars in command rejected
        let ctrl_cmd = AcpAgentSpec {
            id: "ctrl-cmd".into(),
            name: "Ctrl".into(),
            description: String::new(),
            command: "my-acp\x1b[31m".into(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        assert!(validate_acp_agent_spec(&ctrl_cmd).is_err());

        // Dangerous env vars and ORBIT_* keys rejected
        for blocked_key in [
            "LD_PRELOAD",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
            "DYLD_LIBRARY_PATH",
            "BASH_ENV",
            "NODE_OPTIONS",
            "PYTHONSTARTUP",
            "PYTHONPATH",
            "PERL5OPT",
            "RUBYOPT",
            "ORBIT_HOME",
            "ORBIT_SERVER",
            "orbit_token",
        ] {
            let bad_env = AcpAgentSpec {
                id: "bad-env".into(),
                name: "Bad Env".into(),
                description: String::new(),
                command: "echo".into(),
                args: vec![],
                env: HashMap::from([(blocked_key.to_string(), "evil".to_string())]),
                install_hint: None,
                homepage: None,
                version: None,
                source: AcpAgentSource::Custom,
                enabled: true,
            };
            assert!(
                validate_acp_agent_spec(&bad_env).is_err(),
                "expected env key `{blocked_key}` to be rejected"
            );
        }

        // Non-HTTPS or credentialed homepage rejected
        for bad_hp in [
            "http://example.com",
            "javascript:alert(1)",
            "https://user:pass@example.com",
            "https://example.com/with space",
        ] {
            let bad = AcpAgentSpec {
                id: "bad-hp".into(),
                name: "Bad HP".into(),
                description: String::new(),
                command: "echo".into(),
                args: vec![],
                env: HashMap::new(),
                install_hint: None,
                homepage: Some(bad_hp.to_string()),
                version: None,
                source: AcpAgentSource::Custom,
                enabled: true,
            };
            assert!(
                validate_acp_agent_spec(&bad).is_err(),
                "expected homepage `{bad_hp}` to be rejected"
            );
        }

        // RemoteRegistry specs with env are rejected by validate_acp_agent_spec AND stripped during parse
        let remote_with_env = AcpAgentSpec {
            id: "remote-env".into(),
            name: "Remote Env".into(),
            description: String::new(),
            command: "echo".into(),
            args: vec![],
            env: HashMap::from([("SAFE_KEY".into(), "1".into())]),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::RemoteRegistry,
            enabled: true,
        };
        assert!(validate_acp_agent_spec(&remote_with_env).is_err());

        let untrusted_remote = json!({
            "agents": [{
                "id": "remote-strip-env",
                "name": "Remote Strip",
                "command": "echo",
                "env": { "LD_PRELOAD": "/tmp/evil.so", "FOO": "bar" }
            }]
        });
        let parsed = parse_remote_registry_json(untrusted_remote.to_string().as_bytes())
            .expect("parse strips untrusted remote env");
        assert_eq!(parsed.len(), 1);
        assert!(parsed[0].env.is_empty());
    }

    #[test]
    fn corrupt_registry_file_never_silently_wiped_on_write() {
        let tmp_home =
            std::env::temp_dir().join(format!("orbit_acp_corrupt_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        let path = acp_registry_path(&tmp_home);
        std::fs::write(&path, b"{ not valid json").expect("write corrupt");

        let spec = AcpAgentSpec {
            id: "safe-agent".into(),
            name: "Safe".into(),
            description: String::new(),
            command: "echo".into(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        assert!(upsert_custom_acp_agent(&tmp_home, spec).is_err());
        assert!(delete_custom_acp_agent(&tmp_home, "safe-agent").is_err());
        // Original corrupt file remains untouched for manual recovery
        assert_eq!(
            std::fs::read(&path).expect("still present"),
            b"{ not valid json"
        );

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_acp_agent_captures_stderr_and_enforces_stdout_cap() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("orbit_acp_stderr_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let script = dir.join("failing-acp");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'missing API_KEY in env' >&2\nexit 1\n",
        )
        .expect("write script");
        let mut perms = std::fs::metadata(&script).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod");

        let spec = AcpAgentSpec {
            id: "failing-acp".into(),
            name: "Failing ACP".into(),
            description: String::new(),
            command: script.to_string_lossy().to_string(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        let res = probe_acp_agent(&spec, Some(&dir)).await;
        assert!(!res.ok);
        let err = res.error.unwrap_or_default();
        assert!(
            err.contains("missing API_KEY in env"),
            "expected stderr tail in probe error, got: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
