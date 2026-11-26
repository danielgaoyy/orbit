mod acp_cli;
mod cloud_sync_cli;
mod mcp_bridge;
mod orchestration_cli;
mod plugin_cli;
mod service;
mod ssh_config;
mod voice_cli;
mod web_assets;

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::io::{IsTerminal, Read, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use orbit_client_core::{
    ClientInboundEvent, HostConnectionState, HostRouteMode, HostSessionHandle, SavedHostProfile,
    connect_direct, connect_direct_verified, connect_smart,
};
use orbit_crypto::PairingOffer;
use orbit_daemon::{
    DaemonConfig, DaemonPersistedConfig, DaemonRuntime, discover_lan_ws_endpoints,
    run_daemon_until_with_web,
};
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, GuardianAssessment, ProviderKind, SessionMode,
    TimelinePayload, ToolCallDetail, WireMessage, WorktreeCreateMode,
};
use orbit_providers::{
    discover_installed_providers, probe_all_provider_quotas, resolve_login_shell_env,
    set_snapshot_orbit_home, shell_snapshot_stats,
};
use orbit_relay::{RelayConfig, RelayPersistedConfig, run_relay_with_web};
use orbit_relay_transport::{is_ssh_relay_url, redact_relay_url};
use orbit_shim::read_spawn_spec_from_stdin;
use uuid::Uuid;

use crate::acp_cli::{AcpAction, cmd_acp};
use crate::orchestration_cli::{
    AgentAction, CliBehavior, CliEffort, ConfigAction, HeartbeatAction, HostAction, PermitAction,
    ProfileAction, ScheduleAction, ScriptAction, SkillAction, TerminalAction, WorkspaceAction,
    cmd_agent, cmd_config, cmd_fork_agent, cmd_heartbeat, cmd_host, cmd_inspect, cmd_permit,
    cmd_profile, cmd_schedule, cmd_script, cmd_simple_agent_op, cmd_skill, cmd_terminal, cmd_wait,
    cmd_workspace, resolve_server_url,
};
use crate::service::{
    PortOwnership, RelayProbeOutcome, ServiceRole, check_local_health, connectable_ip_for_bind,
    default_daemon_listen, detect_public_ipv4, find_live_daemon_addr_for_host,
    find_live_daemon_port_for_host, install_and_start_service, normalize_relay_url,
    preflight_probe_relay, probe_daemon_addr, probe_daemon_port, resolve_isolated_daemon_listen,
    self_upgrade_cli, stop_service,
};
use crate::ssh_config::{discover_ssh_hosts, provision_relay_via_ssh};

#[derive(Parser)]
#[command(
    name = "orbit",
    version,
    about = "Ultra-fast Multi-Host AI Coding Agent Orchestrator (13 Native Coding Agents)"
)]
struct Cli {
    /// Override Orbit state directory (defaults to `~/.orbit`).
    #[arg(long, env = "ORBIT_HOME", global = true)]
    home: Option<PathBuf>,

    /// Emit machine-readable JSON to stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Target Orbit Daemon WebSocket URL or `orbit://pair/...` URI (auto-discovers local daemon if omitted).
    #[arg(long, env = "ORBIT_SERVER", global = true)]
    server: Option<String>,

    /// Force connecting via E2EE Relay when `--server` is an `orbit://pair/...` URI.
    #[arg(long, global = true)]
    via_relay: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliProvider {
    #[value(alias = "agy")]
    Antigravity,
    #[value(alias = "openai")]
    Codex,
    #[value(name = "claude_code", alias = "claude-code", alias = "claude")]
    ClaudeCode,
    #[value(name = "opencode", alias = "open-code", alias = "open_code")]
    OpenCode,
    #[value(name = "gemini_cli", alias = "gemini-cli", alias = "gemini")]
    GeminiCli,
    #[value(name = "cursor_agent", alias = "cursor-agent", alias = "cursor")]
    CursorAgent,
    #[value(name = "copilot", alias = "github-copilot", alias = "github_copilot")]
    Copilot,
    Goose,
    #[value(name = "pi", alias = "pi-coding-agent")]
    Pi,
    #[value(name = "kimi", alias = "kimi-cli", alias = "kimi_cli")]
    Kimi,
    Cline,
    #[value(
        name = "mistral_vibe",
        alias = "mistral-vibe",
        alias = "vibe",
        alias = "vibe-acp"
    )]
    MistralVibe,
    #[value(name = "augment", alias = "auggie", alias = "augment-code")]
    Augment,
    Acp,
}

impl From<CliProvider> for ProviderKind {
    fn from(v: CliProvider) -> Self {
        match v {
            CliProvider::Antigravity => ProviderKind::Antigravity,
            CliProvider::Codex => ProviderKind::Codex,
            CliProvider::ClaudeCode => ProviderKind::ClaudeCode,
            CliProvider::OpenCode => ProviderKind::OpenCode,
            CliProvider::GeminiCli => ProviderKind::GeminiCli,
            CliProvider::CursorAgent => ProviderKind::CursorAgent,
            CliProvider::Copilot => ProviderKind::Copilot,
            CliProvider::Goose => ProviderKind::Goose,
            CliProvider::Pi => ProviderKind::Pi,
            CliProvider::Kimi => ProviderKind::Kimi,
            CliProvider::Cline => ProviderKind::Cline,
            CliProvider::MistralVibe => ProviderKind::MistralVibe,
            CliProvider::Augment => ProviderKind::Augment,
            CliProvider::Acp => ProviderKind::Acp,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliMode {
    Default,
    Auto,
    AcceptEdits,
    Plan,
    FullAccess,
}

impl From<CliMode> for SessionMode {
    fn from(v: CliMode) -> Self {
        match v {
            CliMode::Default => SessionMode::Default,
            CliMode::Auto => SessionMode::Auto,
            CliMode::AcceptEdits => SessionMode::AcceptEdits,
            CliMode::Plan => SessionMode::Plan,
            CliMode::FullAccess => SessionMode::FullAccess,
        }
    }
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Install & start (or restart) `orbit daemon` in the background (systemd / launchd / daemon).
    Up {
        /// Install under Windows Service Control Manager.
        #[arg(long)]
        service: bool,
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long)]
        relay: Option<String>,
    },
    /// Show background `orbit daemon` status, bound port, and attached Relay URL.
    Status,
    /// Restart background `orbit daemon` cleanly (running `orbit shim` sessions survive uninterrupted).
    Restart {
        #[arg(long)]
        port: Option<u16>,
    },
    /// Stop and disable background `orbit daemon`.
    Down,
}

#[derive(Subcommand)]
enum RelayAction {
    /// Install & start (or restart) the `< 6MB` zero-knowledge E2EE Relay in the background.
    Up {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long)]
        listen: Option<SocketAddr>,
        /// Authorize one or more host public keys (merged into `~/.orbit/relay.json`).
        #[arg(long = "allow-pubkey")]
        allow_pubkeys: Vec<String>,
        /// Replace the existing allowlist instead of merging.
        #[arg(long)]
        replace_allowlist: bool,
        /// Optional public URL override for printed onboarding instructions.
        #[arg(long)]
        public_url: Option<String>,
        /// Read a shared fleet admission token from stdin (`0o600` persisted in `~/.orbit/relay.json`).
        #[arg(long)]
        fleet_token_stdin: bool,
    },
    /// Expose a zero-inbound-port local Relay (`127.0.0.1:<port>`) over Cloudflare Tunnel (ideal for firewalled / intranet machines).
    Tunnel {
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Authorize one or more host public keys (merged into `~/.orbit/relay.json`).
        #[arg(long = "allow-pubkey")]
        allow_pubkeys: Vec<String>,
        /// Optional custom hostname for a Cloudflare Named Tunnel (paired with `--tunnel-token-stdin`).
        #[arg(long)]
        hostname: Option<String>,
        /// Read a Cloudflare Named Tunnel token from stdin (`0o600` EnvironmentFile, never stored in argv).
        #[arg(long)]
        tunnel_token_stdin: bool,
        /// Do not auto-configure the local Orbit Daemon to attach to the provisioned tunnel URL.
        #[arg(long)]
        no_attach_daemon: bool,
    },
    /// Add host public key(s) to the Relay allowlist (`~/.orbit/relay.json`) and reload Relay immediately.
    Allow {
        #[arg(required = true)]
        pubkeys: Vec<String>,
    },
    /// Show Relay health, bound port, and authorized public keys.
    Status,
    /// Stop and disable background `orbit relay`.
    Down,
}

#[derive(Subcommand)]
enum AuditAction {
    Verify,
    Export {
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
#[derive(Subcommand)]
enum Commands {
    #[cfg(windows)]
    #[command(hide = true)]
    ServiceRun {
        #[arg(value_parser=["daemon","relay"])]
        role: String,
    },
    #[cfg(target_os = "linux")]
    #[command(hide = true)]
    SandboxBridge {
        port: u16,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Inspect host voice settings, manage offline models and transcribe a WAV file.
    Voice {
        #[command(subcommand)]
        action: voice_cli::VoiceAction,
    },
    /// Review a pull request or a local three-dot commit range with Orbit Patrol.
    ReviewPr {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        base: Option<String>,
        #[arg(long)]
        head: Option<String>,
        #[arg(long)]
        pr: Option<u64>,
        /// Submit verified findings as inline comments on this PR.
        #[arg(long, requires = "pr")]
        post: bool,
    },
    /// Inspect current host environment, identity keypair, Relay attachment, and installed AI CLIs.
    Doctor {
        #[arg(long)]
        bundle: Option<PathBuf>,
    },
    /// Preview or apply a migration from a coding CLI or IDE.
    Migrate {
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        apply: bool,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long,default_value="skip",value_parser=["skip","overwrite","rename"])]
        conflict: String,
    },
    /// Verify or export the signed host security history.
    Audit {
        #[command(subcommand)]
        action: AuditAction,
    },

    /// Fetch live 5-hour / weekly usage quotas & remaining credits across Antigravity, Codex, and Claude Code.
    Quotas,

    /// One-click self-upgrade `orbit` binary to the latest GitHub Release (and hot-restart active daemon/relay).
    Upgrade {
        /// Force reinstall even if already on the latest version.
        #[arg(long)]
        force: bool,
        /// Also upgrade all connected remote Orbit hosts in the fleet mesh before upgrading this machine.
        #[arg(long)]
        all: bool,
    },

    /// Auto-discover `~/.ssh/config` or attach to an E2EE Relay, auto-deploy via SSH if given an SSH alias, and print Pairing Code.
    UseRelay {
        /// SSH config host alias (e.g. `devbox`) OR Relay URL (`203.0.113.10:7878` or `ssh+relay://...`). Omit to pick from `~/.ssh/config`.
        relay_url: Option<String>,
        /// Relay TCP port when deploying/connecting via SSH alias (defaults to `7878`).
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Deploy a public-port WebSocket Relay (`0.0.0.0:<port>`) instead of a Zero-Port SSH-tunneled Relay.
        #[arg(long)]
        direct: bool,
        /// Skip automatic SSH relay provisioning when `relay_url` matches an SSH config alias.
        #[arg(long)]
        no_provision: bool,
        /// Detach Relay and revert this host to LAN/Tailscale direct-only mode.
        #[arg(long)]
        clear: bool,
    },

    /// Print a Multi-Host QR pairing URI (auto-detects LAN/Tailscale IPs & persisted Relay URL).
    Pair {
        #[arg(long)]
        port: Option<u16>,
        #[arg(long = "endpoint")]
        endpoints: Vec<String>,
        #[arg(long)]
        relay: Option<String>,
        /// Rotate the host's persisted pairing secret (`$ORBIT_HOME/pairing-secret`) before printing the URI.
        #[arg(long)]
        rotate_secret: bool,
    },

    /// Sign in with GitHub (zero-server E2EE Secret Gist) to sync hosts, projects & settings across all devices.
    Login {
        /// Optional GitHub Personal Access Token (`gist` scope) for non-interactive or fallback sign-in.
        #[arg(long, env = "ORBIT_GITHUB_TOKEN")]
        token: Option<String>,
        /// Optional GitHub OAuth App Client ID for RFC 8628 Device Flow.
        #[arg(long, env = "ORBIT_GITHUB_CLIENT_ID")]
        client_id: Option<String>,
        /// Optional Sync Passphrase to unlock or initialize the encrypted Vault non-interactively.
        #[arg(long, env = "ORBIT_SYNC_PASSPHRASE")]
        passphrase: Option<String>,
        /// Import an `orbit://sync-login?...` Device Link URI from an existing Orbit device without burning a GitHub OAuth token.
        #[arg(long, env = "ORBIT_DEVICE_LINK_URI")]
        device_link: Option<String>,
        /// Read an `orbit://sync-login?...` Device Link URI from stdin (recommended over passing secrets in argv).
        #[arg(long)]
        device_link_stdin: bool,
        /// Force a new GitHub OAuth Device Flow even if a valid cached GitHub session already exists locally.
        #[arg(long)]
        reauth: bool,
        /// Require manual Sync Passphrase entry on new devices instead of Dual-Gist Instant Unlock.
        #[arg(long)]
        manual_passphrase: bool,
        /// Skip registering this machine's local Orbit Daemon into the synced fleet.
        #[arg(long)]
        no_register_host: bool,
        /// Also wire all other synced hosts into this machine's local Daemon Connected Hosts mesh.
        #[arg(long)]
        mesh: bool,
    },

    /// Sync this machine's Daemon & multi-device fleet config with your encrypted GitHub Vault.
    Sync {
        /// Pull/merge without pushing this machine's local Daemon identity.
        #[arg(long)]
        no_register_host: bool,
        /// Also wire all other synced hosts into this machine's local Daemon Connected Hosts mesh.
        #[arg(long)]
        mesh: bool,
        /// Print an `orbit://sync-login?...` 1-scan Device Link URI for onboarding another phone/laptop without consuming a GitHub token slot.
        #[arg(long)]
        export_device_link: bool,
    },

    /// Sign out of GitHub Cloud Sync on this machine (keeps your remote encrypted GitHub Gist intact).
    Logout {
        /// Also clear saved client hosts from local desktop replica SQLite.
        #[arg(long)]
        clear_local_hosts: bool,
    },

    /// Run or manage the host-local Orbit Daemon (`orbit daemon up | status | restart | down`).
    Daemon {
        /// Serve a built Orbit PWA from this directory on the foreground listener.
        #[arg(long, value_name = "DIR")]
        serve_web: Option<PathBuf>,
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long, env = "ORBIT_RELAY_URL")]
        relay: Option<String>,
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },

    /// Run or manage the `< 6MB` zero-knowledge E2EE Relay (`orbit relay up | allow | status | down`).
    Relay {
        /// Serve a built Orbit PWA from this directory on the foreground listener.
        #[arg(long, value_name = "DIR")]
        serve_web: Option<PathBuf>,
        #[arg(long)]
        listen: Option<SocketAddr>,
        /// Restrict relay access strictly to your own Daemon public key(s). Repeatable.
        #[arg(long = "allow-pubkey")]
        allow_pubkeys: Vec<String>,
        #[command(subcommand)]
        action: Option<RelayAction>,
    },

    /// List managed agents on an Orbit Daemon.
    Ls {
        #[arg(short = 'a', long)]
        all: bool,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        parent: Option<String>,
        #[arg(long)]
        include_background: bool,
    },

    /// Create and stream (or asynchronously launch with `--no-wait`) a coding agent session.
    Run {
        #[arg(short, long, value_enum, default_value = "antigravity")]
        provider: CliProvider,
        /// ACP registry agent ID when `--provider acp` is selected (e.g. `opencode`, `gemini-cli`, or custom ID).
        #[arg(long)]
        acp_agent: Option<String>,
        #[arg(short, long, value_enum, default_value = "accept-edits")]
        mode: CliMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum)]
        effort: Option<CliEffort>,
        /// Enable Fast output mode (lower latency, higher quota usage) when supported by the model.
        #[arg(long, conflicts_with = "no_fast")]
        fast: bool,
        /// Explicitly disable Fast output mode (overrides a fast-enabled Agent Profile).
        #[arg(long, conflicts_with = "fast")]
        no_fast: bool,
        /// Apply a saved Agent Profile ID (fills provider/model/effort/mode/system prompt/MCP policy).
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        workspace: Option<String>,
        #[arg(long)]
        title: Option<String>,
        /// Automatically run inside an isolated git worktree branch.
        #[arg(long)]
        worktree: bool,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        base_ref: Option<String>,
        /// Return immediately with the created agent_id instead of streaming stdout.
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        background: bool,
        #[arg(long)]
        auto_archive: bool,
        /// Notify parent agent automatically when the child finishes (defaults on if `ORBIT_AGENT_ID` is set).
        #[arg(long)]
        notify_on_finish: Option<bool>,
        /// Automatically approve tool permission requests during streaming execution.
        #[arg(short = 'y', long = "auto-approve")]
        auto_approve: bool,
        /// Initial prompt to send to the agent.
        prompt: String,
    },

    /// Send a follow-up prompt to an existing (or persisted) agent.
    Send {
        agent_id: String,
        prompt: String,
        #[arg(long, value_enum)]
        behavior: Option<CliBehavior>,
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        notify_on_finish: Option<bool>,
        /// Automatically approve tool permission requests during streaming execution.
        #[arg(short = 'y', long = "auto-approve")]
        auto_approve: bool,
    },

    /// Block until an agent reaches Idle / AwaitingPermission / Error / Closed and print its final reply.
    Wait {
        agent_id: String,
        #[arg(long, default_value_t = 1800)]
        timeout: u64,
    },

    /// Interrupt an agent's currently running turn.
    Stop { agent_id: String },

    /// Inspect an agent's metadata, pending permissions, provider subagents, and recent timeline.
    Inspect { agent_id: String },

    /// Soft-archive an agent (and its managed same-workspace subagents).
    Archive { agent_id: String },

    /// Restore a soft-archived agent.
    Unarchive { agent_id: String },

    /// Detach a subagent from its parent so it becomes a top-level standalone session.
    Detach { agent_id: String },

    /// Permanently delete an agent and its timeline history.
    Delete { agent_id: String },

    /// Change an agent's active session mode.
    Mode {
        agent_id: String,
        #[arg(value_enum)]
        mode: CliMode,
    },

    /// Read timeline history of an agent from an Orbit Daemon.
    Logs { agent_id: String },

    /// Fork/branch an existing agent conversation in the same directory or into a new git worktree.
    Fork {
        agent_id: String,
        #[arg(long = "at-seq")]
        at_seq: Option<u64>,
        #[arg(long)]
        worktree: bool,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long = "base-ref")]
        base_ref: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
    },

    /// Agent conversation & lifecycle utilities (`orbit agent export|fork ...`).
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage workspaces and isolated git worktrees.
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },

    /// Manage workspace scripts & reverse-proxied dev services (`orbit.json` / `paseo.json`).
    Script {
        #[command(subcommand)]
        action: ScriptAction,
    },

    /// Manage detached PTY terminals (list, create, capture text, send-keys, kill).
    Terminal {
        #[command(subcommand)]
        action: TerminalAction,
    },

    /// Manage Cron Schedules (`orbit schedule create | ls | pause | resume | run-once | logs | delete`).
    Schedule {
        #[command(subcommand)]
        action: ScheduleAction,
    },

    /// Manage periodic agent Heartbeats (`orbit heartbeat create | ls | delete`).
    Heartbeat {
        #[command(subcommand)]
        action: HeartbeatAction,
    },

    /// Manage Agent Profiles (`orbit profile ls | create | delete`).
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },

    /// Manage Daemon-to-Daemon Connected Hosts for cross-host orchestration.
    Host {
        #[command(subcommand)]
        action: HostAction,
    },

    /// Inspect and resolve pending tool permissions across agents.
    Permit {
        #[command(subcommand)]
        action: PermitAction,
    },

    /// View or update host Orchestration runtime configuration.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Manage WASM plugins on the selected daemon.
    Plugin {
        #[command(subcommand)]
        action: plugin_cli::PluginAction,
    },

    /// Inspect or install bundled Orbit Orchestration skills (`orbit`, `orbit-handoff`, `orbit-committee`, `orbit-advisor`).
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// Manage curated & custom Universal ACP coding agents (`orbit acp list | add | remove | test | refresh`).
    Acp {
        #[command(subcommand)]
        action: AcpAction,
    },

    /// Stdio-to-HTTP MCP 2025-03-26 JSON-RPC bridge connecting external MCP clients to the local Orbit Daemon.
    Mcp {
        #[arg(long)]
        caller_agent_id: Option<String>,
        #[arg(long)]
        url: Option<String>,
    },

    /// Internal detached process supervisor invoked by `orbit daemon` (reads `ShimSpawnSpec` JSON from stdin).
    #[command(hide = true)]
    Shim {
        #[arg(long)]
        socket: PathBuf,
    },

    /// Internal Claude Code `PreToolUse` hook evaluator forwarding tool calls to the local Daemon.
    #[command(hide = true)]
    InternalPreToolUse {
        #[arg(long)]
        agent_id: String,
    },
}

fn resolve_home(override_home: Option<PathBuf>) -> PathBuf {
    if let Some(h) = override_home {
        return h;
    }
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".orbit")
}

fn main() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        // Windows MSVC defaults to a 1 MiB main thread stack (vs 8 MiB on macOS/Linux),
        // which can overflow (`0xc00000fd`) in `clap::Parser` or large async futures in debug builds.
        let handle = std::thread::Builder::new()
            .name("orbit-main".into())
            .stack_size(8 * 1024 * 1024)
            .spawn(main_impl)?;
        match handle.join() {
            Ok(res) => res,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
    #[cfg(not(windows))]
    {
        main_impl()
    }
}

fn main_impl() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let rt = match &cli.command {
        Commands::Daemon { action: None, .. } | Commands::Relay { .. } => {
            let workers = std::thread::available_parallelism()
                .map(|n| n.get().clamp(2, 4))
                .unwrap_or(4);
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .thread_stack_size(8 * 1024 * 1024)
                .enable_all()
                .build()?
        }
        _ => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?,
    };
    rt.block_on(Box::pin(run_cli(cli)))
}

fn setup_logging(
    home_dir: &std::path::Path,
    use_rotating_daemon_log: bool,
) -> anyhow::Result<Option<std::sync::Arc<orbit_daemon::telemetry::Exporter>>> {
    let mut exporter = None;
    let mut env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    // Cap WebSocket dependency crates at INFO so RUST_LOG=trace can never dump signed
    // Feishu/Slack WSS URLs or Discord Gateway IDENTIFY bot tokens into daemon logs.
    for directive in ["tungstenite=info", "tokio_tungstenite=info"] {
        if let Ok(parsed) = directive.parse() {
            env_filter = env_filter.add_directive(parsed);
        }
    }
    use tracing_subscriber::{fmt::writer::BoxMakeWriter, prelude::*};
    let writer = if use_rotating_daemon_log {
        orbit_client_core::logging::RotatingLogWriter::open(
            home_dir.join("logs/daemon.log"),
            orbit_client_core::logging::DEFAULT_LOG_MAX_BYTES,
            orbit_client_core::logging::DEFAULT_LOG_MAX_BACKUPS,
            std::io::stderr().is_terminal(),
        )
        .ok()
    } else {
        None
    };
    let writer = match writer {
        Some(w) => BoxMakeWriter::new(move || w.clone()),
        None => BoxMakeWriter::new(std::io::stderr),
    };
    let endpoint = use_rotating_daemon_log
        .then(|| {
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    DaemonPersistedConfig::load(home_dir)
                        .telemetry_endpoint
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
        })
        .flatten();
    let log_layer = if let Some(endpoint) = endpoint {
        match orbit_daemon::telemetry::start(&endpoint) {
            Ok((layer, handle)) => {
                exporter = Some(handle);
                Some(layer)
            }
            Err(err) => {
                eprintln!("warning: ignoring invalid OTLP telemetry endpoint `{endpoint}`: {err}");
                None
            }
        }
    } else {
        None
    };
    let base = tracing_subscriber::registry()
        .with(env_filter)
        .with(log_layer);
    let fmt = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .with_ansi(false);
    if std::env::var("ORBIT_LOG_FORMAT").as_deref() == Ok("json") {
        base.with(fmt.json()).init()
    } else {
        base.with(fmt).init()
    }
    Ok(exporter)
}

async fn run_cli(cli: Cli) -> anyhow::Result<()> {
    let home_dir = resolve_home(cli.home);
    let json_mode = cli.json;
    let via_relay = cli.via_relay;
    let explicit_server = cli.server.clone();
    let server = resolve_server_url(&home_dir, cli.server);

    let initialize_logging = !matches!(
        cli.command,
        Commands::Mcp { .. } | Commands::InternalPreToolUse { .. }
    );
    #[cfg(target_os = "linux")]
    let initialize_logging =
        initialize_logging && !matches!(cli.command, Commands::SandboxBridge { .. });
    #[cfg(windows)]
    let initialize_logging =
        initialize_logging && !matches!(cli.command, Commands::ServiceRun { .. });
    let exporter = if initialize_logging {
        setup_logging(
            &home_dir,
            matches!(cli.command, Commands::Daemon { action: None, .. }),
        )?
    } else {
        None
    };

    match cli.command {
        #[cfg(windows)]
        Commands::ServiceRun { role } => service::dispatch(
            if role == "relay" {
                ServiceRole::Relay
            } else {
                ServiceRole::Daemon
            },
            home_dir,
        )?,
        #[cfg(target_os = "linux")]
        Commands::SandboxBridge { port, command } => {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
            let mut child = tokio::process::Command::new(&command[0])
                .args(&command[1..])
                .kill_on_drop(true)
                .spawn()?;
            let mut clients = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    status=child.wait()=>std::process::exit(status?.code().unwrap_or(1)),
                    Some(_)=clients.join_next(),if !clients.is_empty()=>{},
                    item=listener.accept(),if clients.len()<64=>{
                        let (mut src,_)=item?;clients.spawn(async move{if let Ok(mut dst)=tokio::net::UnixStream::connect("/tmp/.orbit-egress.sock").await{let _=tokio::io::copy_bidirectional(&mut src,&mut dst).await;}});
                    }
                }
            }
        }
        Commands::Migrate {
            from,
            apply,
            cwd,
            conflict,
        } => {
            let user = orbit_daemon::platform::resolve_user_home_dir()
                .ok_or_else(|| anyhow::anyhow!("user home unavailable"))?;
            let policy = serde_json::from_value(serde_json::Value::String(conflict))?;
            if apply {
                orbit_shim::ensure_private_dir(&home_dir)?;
            }
            let cwd = cwd.or_else(|| std::env::current_dir().ok());
            let items = orbit_daemon::ide_migrate::migrate(
                &home_dir,
                &user,
                cwd.as_deref(),
                from.as_deref(),
                apply,
                policy,
            )?;
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        Commands::Audit { action } => {
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let store = &runtime.store;
            match action {
                AuditAction::Verify => {
                    let result = store.verify_audit_chain(store.audit_verifying_key().as_ref())?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                    anyhow::ensure!(result.broken_seq.is_none(), "audit verification failed");
                }
                AuditAction::Export { output } => {
                    let dump = |writer: &mut dyn std::io::Write| -> anyhow::Result<()> {
                        let mut seq = 0;
                        loop {
                            let (s, next) = store.export_audit_chain_jsonl(seq)?;
                            writer.write_all(s.as_bytes())?;
                            if let Some(n) = next { seq = n } else { break }
                        }
                        Ok(())
                    };
                    if let Some(p) = output {
                        orbit_daemon::diagnostics::write_private(&p, |f| dump(f))?;
                    } else {
                        dump(&mut std::io::stdout().lock())?;
                    }
                }
            }
        }
        Commands::Voice { action } => {
            let session = connect_cli_server(&home_dir, &server, via_relay).await?;
            voice_cli::run(&session, action, json_mode).await?;
        }
        Commands::ReviewPr {
            cwd,
            base,
            head,
            pr,
            post,
        } => {
            let report = if explicit_server.is_some() || via_relay {
                let client = connect_cli_server(&home_dir, &server, via_relay).await?;
                let id = Uuid::new_v4();
                let response = client
                    .request_for(
                        WireMessage::RunPrPatrol {
                            request_id: id,
                            cwd: cwd.display().to_string(),
                            base_ref: base,
                            head_ref: head,
                            pr_number: pr,
                            post_github_comments: post,
                        },
                        Duration::from_secs(130),
                        Some(id),
                        |w| matches!(w, WireMessage::PrPatrolCompleted { request_id, .. } if *request_id == id),
                    )
                    .await?;
                let WireMessage::PrPatrolCompleted { report, .. } = response else {
                    anyhow::bail!("Unexpected PR Patrol response");
                };
                report
            } else {
                orbit_daemon::pr_patrol::run(&cwd, base.as_deref(), head.as_deref(), pr, post)
                    .await?
            };
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", report.summary);
                for f in report.findings {
                    println!(
                        "[{:?}] {}:{} {}\n  {}",
                        f.severity, f.file_path, f.start_line, f.title, f.suggested_fix
                    );
                }
            }
        }
        Commands::Doctor { bundle } => {
            set_snapshot_orbit_home(&home_dir);
            let _ = resolve_login_shell_env().await;
            let snap_stats = shell_snapshot_stats();

            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let persisted = DaemonPersistedConfig::load(&home_dir);
            let live_addr = orbit_daemon::read_live_daemon_addr(&home_dir);
            let fallback_addr = live_addr
                .or(persisted.listen_addr)
                .unwrap_or_else(default_daemon_listen);
            let active_addr =
                find_live_daemon_addr_for_host(&runtime.host_info.host_id, fallback_addr).await;
            let active_port = active_addr.port();
            let daemon_live = probe_daemon_addr(active_addr, &runtime.host_info.host_id).await
                == PortOwnership::Mine;

            if let Some(path) = bundle {
                let bundle = if daemon_live {
                    let sock =
                        SocketAddr::new(connectable_ip_for_bind(active_addr.ip()), active_port);
                    let addr = format!("ws://{sock}/ws");
                    let client = connect_cli_server(&home_dir, &addr, false).await?;
                    let id = Uuid::new_v4();
                    match client.request(WireMessage::ExportDiagnosticBundle{request_id:id},Duration::from_secs(60),|w|matches!(w,WireMessage::DiagnosticBundleReady{request_id,..} if *request_id==id)).await?{
                        WireMessage::DiagnosticBundleReady{bundle,..}=>bundle,_=>anyhow::bail!("unexpected diagnostic response")
                    }
                } else {
                    orbit_daemon::diagnostics::bundle(&runtime).await?
                };
                orbit_daemon::diagnostics::write_private(&path, |f| {
                    Ok(serde_json::to_writer_pretty(f, &bundle)?)
                })?;
                println!("Wrote diagnostics to {}", path.display());
                return Ok(());
            }
            let probe = orbit_shim::probe_sandbox_support();
            println!(
                "Sandbox      : {} (available {}, kernel enforced {})",
                probe.backend, probe.available, probe.kernel_enforced
            );
            println!("=== Orbit Host Diagnostics ===");
            println!("Host ID      : {}", runtime.host_info.host_id);
            println!("Hostname     : {}", runtime.host_info.hostname);
            println!(
                "Platform     : {}/{}",
                runtime.host_info.os, runtime.host_info.arch
            );
            println!("Orbit Home   : {}", home_dir.display());
            println!("Public Key   : {}", runtime.host_info.public_key_b64);
            println!(
                "Daemon State : {} (port {})",
                if daemon_live {
                    "running"
                } else {
                    "stopped (run `orbit daemon up`)"
                },
                active_port
            );
            let effective_relay = persisted
                .relay_url
                .or_else(|| orbit_daemon::read_live_daemon_relay(&home_dir));
            match &effective_relay {
                Some(r) => println!("Relay Route  : {} (Noise NK E2EE)", redact_relay_url(r)),
                None => println!(
                    "Relay Route  : (not configured — LAN/Tailscale only; run `orbit use-relay <HOST:PORT>`)"
                ),
            }
            if snap_stats.warm {
                println!(
                    "Shell Cache  : {} ({} vars, age {}s, hits {}, misses {})",
                    snap_stats.shell,
                    snap_stats.var_count,
                    snap_stats.age_secs.unwrap_or(0),
                    snap_stats.hits,
                    snap_stats.misses
                );
            } else {
                println!(
                    "Shell Cache  : {} (cold, errors {}, hits {}, misses {})",
                    snap_stats.shell, snap_stats.probe_errors, snap_stats.hits, snap_stats.misses
                );
            }

            println!("\nDiscovered Coding Agent CLIs:");
            let providers = discover_installed_providers();
            if providers.is_empty() {
                println!("  (none detected in PATH / ~/.local/bin)");
            } else {
                for p in providers {
                    println!(
                        "  [✓] {:<15} -> {} ({})",
                        p.kind.display_name(),
                        p.executable_path.display(),
                        p.protocol_transport
                    );
                }
            }
            let ssh_hosts = discover_ssh_hosts();
            if !ssh_hosts.is_empty() {
                println!("\nDiscovered SSH Config Servers (1-Command Auto-Deploy & Attach):");
                for h in &ssh_hosts {
                    println!(
                        "  orbit use-relay {:<14} # auto-deploys Relay on {}",
                        h.alias,
                        h.display_target()
                    );
                }
            } else {
                println!("\nQuick Relay Setup (on your cloud VM / devbox):");
                println!(
                    "  orbit relay up --allow-pubkey {}",
                    runtime.host_info.public_key_b64
                );
            }
        }

        Commands::Upgrade { force, all } => {
            self_upgrade_cli(&home_dir, force, all).await?;
        }

        Commands::Quotas => {
            println!("Probing real-time quotas across installed providers...");
            let snapshots = probe_all_provider_quotas().await;
            for snap in snapshots {
                println!(
                    "\n[{}] {}",
                    snap.provider.display_name(),
                    snap.account_label.as_deref().unwrap_or("")
                );
                if let Some(err) = snap.error {
                    println!("  ! Notice: {err}");
                }
                for w in snap.windows {
                    let reset_str = w
                        .resets_at
                        .map(|t| format!(" (resets {})", t.format("%Y-%m-%d %H:%M UTC")))
                        .unwrap_or_default();
                    println!(
                        "  - {:<36}: {:>5.1}% used{}",
                        w.label, w.used_percent, reset_str
                    );
                }
                if let Some(cred) = snap.credits_remaining {
                    println!("  - Credits Remaining                   : {cred:.1}");
                }
            }
        }

        Commands::UseRelay {
            relay_url,
            port,
            direct,
            no_provision,
            clear,
        } => {
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let expected_host_id = runtime.host_info.host_id.clone();
            let pubkey = runtime.keypair.public_key_b64.clone();
            drop(runtime);

            let mut persisted = DaemonPersistedConfig::load(&home_dir);
            let preferred = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
            let listen = resolve_isolated_daemon_listen(preferred, &expected_host_id).await?;
            persisted.listen_addr = Some(listen);

            if clear {
                persisted.relay_url = None;
                DaemonPersistedConfig::update(&home_dir, |cfg| {
                    cfg.relay_url = persisted.relay_url.clone();
                    cfg.listen_addr = persisted.listen_addr;
                })?;
                let mgr = install_and_start_service(
                    ServiceRole::Daemon,
                    &home_dir,
                    listen.port(),
                    Some(&expected_host_id),
                )
                .await?;
                println!("[✓] Detached Relay configuration and restarted daemon via {mgr}.");
                println!("    Run `orbit pair` to display an updated pairing URI.");
                return Ok(());
            }

            let ssh_hosts = discover_ssh_hosts();
            let chosen_input = match relay_url {
                Some(v) => v,
                None => {
                    if ssh_hosts.is_empty() {
                        anyhow::bail!(
                            "No SSH servers found in ~/.ssh/config. Usage: `orbit use-relay <SSH_ALIAS | HOST:PORT>`"
                        );
                    }
                    println!("Discovered SSH servers in ~/.ssh/config:");
                    for (idx, h) in ssh_hosts.iter().enumerate() {
                        println!(
                            "  [{}] {:<16} ({} -> ws://{}:{port})",
                            idx + 1,
                            h.alias,
                            h.display_target(),
                            h.host_name
                        );
                    }
                    if !std::io::stdin().is_terminal() {
                        anyhow::bail!(
                            "Run `orbit use-relay {}` to 1-click deploy & attach via SSH.",
                            ssh_hosts[0].alias
                        );
                    }
                    print!(
                        "\nSelect SSH host to auto-deploy & attach Relay [1-{}] (Enter = 1): ",
                        ssh_hosts.len()
                    );
                    std::io::stdout().flush()?;
                    let mut line = String::new();
                    std::io::stdin().read_line(&mut line)?;
                    let trimmed = line.trim();
                    let choice: usize = if trimmed.is_empty() {
                        1
                    } else {
                        trimmed
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid selection"))?
                    };
                    let entry = ssh_hosts
                        .get(choice.saturating_sub(1))
                        .ok_or_else(|| anyhow::anyhow!("selection out of range"))?;
                    entry.alias.clone()
                }
            };

            let normalized = if let Some(ssh_entry) =
                ssh_hosts.iter().find(|h| h.alias == chosen_input.trim())
            {
                if no_provision {
                    format!("ws://{}:{port}", ssh_entry.host_name)
                } else {
                    if direct {
                        println!(
                            "Auto-deploying Orbit E2EE Relay (direct TCP port {port}) onto SSH host `{}` ({})...",
                            ssh_entry.alias,
                            ssh_entry.display_target()
                        );
                    } else {
                        println!(
                            "Auto-deploying Zero-Port SSH Relay onto SSH host `{}` ({})...",
                            ssh_entry.alias,
                            ssh_entry.display_target()
                        );
                    }
                    let deployed_url = provision_relay_via_ssh(
                        &home_dir,
                        ssh_entry,
                        port,
                        &pubkey,
                        &expected_host_id,
                        direct,
                    )?;
                    println!(
                        "[✓] Relay deployed & authorized on `{}` -> {}",
                        ssh_entry.alias,
                        redact_relay_url(&deployed_url)
                    );
                    normalize_relay_url(&deployed_url)?
                }
            } else {
                normalize_relay_url(&chosen_input)?
            };

            let display_relay = redact_relay_url(&normalized);
            println!("Probing Relay {display_relay} with host pubkey `{pubkey}`...");
            match preflight_probe_relay(&normalized, &pubkey).await {
                RelayProbeOutcome::Authorized => {
                    println!("[✓] Relay reachable & host public key authorized!");
                }
                RelayProbeOutcome::Forbidden403 => {
                    println!(
                        "[!] Relay is reachable, but rejected this host's public key (HTTP 403 Forbidden)."
                    );
                    println!(
                        "    Run this command on your Relay server to authorize this machine:"
                    );
                    println!("      orbit relay allow {pubkey}");
                    println!(
                        "    (Saving Relay config now — local daemon will auto-connect within 2s once authorized.)"
                    );
                }
                RelayProbeOutcome::Unreachable(reason) => {
                    println!("[!] Could not reach {display_relay} right now: {reason}");
                    if is_ssh_relay_url(&normalized) {
                        println!(
                            "    Ensure SSH TCP forwarding (`AllowTcpForwarding yes`) is enabled on the SSH server."
                        );
                    } else {
                        println!(
                            "    Ensure TCP port {port} is open in your cloud firewall / AWS Security Group."
                        );
                    }
                    println!(
                        "    (Saving Relay config now — local daemon will auto-connect in background as soon as reachable.)"
                    );
                }
            }

            persisted.relay_url = Some(normalized.clone());
            DaemonPersistedConfig::update(&home_dir, |cfg| {
                cfg.relay_url = persisted.relay_url.clone();
                cfg.listen_addr = persisted.listen_addr;
            })?;

            let mgr = install_and_start_service(
                ServiceRole::Daemon,
                &home_dir,
                listen.port(),
                Some(&expected_host_id),
            )
            .await?;
            println!("[✓] Orbit Daemon active via {mgr} -> attached to {display_relay}");
            println!("\nRun `orbit pair` to display a pairing URI.");
        }

        Commands::Pair {
            port,
            endpoints,
            relay,
            rotate_secret,
        } => {
            print_pairing_summary(&home_dir, port, endpoints, relay, rotate_secret, json_mode)
                .await?;
        }

        Commands::Login {
            token,
            client_id,
            passphrase,
            device_link,
            device_link_stdin,
            reauth,
            manual_passphrase,
            no_register_host,
            mesh,
        } => {
            cloud_sync_cli::cmd_login(
                &home_dir,
                cloud_sync_cli::LoginOptions {
                    token,
                    client_id,
                    passphrase,
                    device_link,
                    device_link_stdin,
                    reauth,
                    manual_passphrase,
                    no_register_host,
                    sync_mesh: mesh,
                    json_mode,
                },
            )
            .await?;
        }

        Commands::Sync {
            no_register_host,
            mesh,
            export_device_link,
        } => {
            cloud_sync_cli::cmd_sync(
                &home_dir,
                cloud_sync_cli::SyncOptions {
                    no_register_host,
                    sync_mesh: mesh,
                    export_device_link,
                    json_mode,
                },
            )
            .await?;
        }

        Commands::Logout { clear_local_hosts } => {
            cloud_sync_cli::cmd_logout(&home_dir, clear_local_hosts, json_mode).await?;
        }

        Commands::Daemon {
            serve_web,
            listen,
            relay,
            action,
        } => {
            if action.is_some() {
                anyhow::ensure!(
                    serve_web.is_none(),
                    "--serve-web is only supported when running `orbit daemon` in the foreground"
                );
            }
            match action {
                None => {
                    let persisted = DaemonPersistedConfig::load(&home_dir);
                    let effective_listen = listen
                        .or(persisted.listen_addr)
                        .unwrap_or_else(default_daemon_listen);
                    let effective_relay = relay.or(persisted.relay_url);
                    run_daemon_until_with_web(
                        DaemonConfig {
                            home_dir,
                            listen_addr: effective_listen,
                            relay_url: effective_relay,
                        },
                        exporter,
                        serve_web.map(web_assets::router).transpose()?,
                        orbit_daemon::wait_daemon_shutdown_signal(),
                    )
                    .await?;
                }
                Some(DaemonAction::Up {
                    service,
                    port,
                    listen: sub_listen,
                    relay: sub_relay,
                }) => {
                    let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                    let expected_host_id = runtime.host_info.host_id.clone();
                    drop(runtime);

                    let mut persisted = DaemonPersistedConfig::load(&home_dir);
                    let live_addr = orbit_daemon::read_live_daemon_addr(&home_dir);
                    let preferred = if let Some(l) = sub_listen.or(listen) {
                        l
                    } else if let Some(p) = port {
                        SocketAddr::new(
                            live_addr
                                .map(|a| a.ip())
                                .or_else(|| persisted.listen_addr.map(|a| a.ip()))
                                .unwrap_or_else(|| default_daemon_listen().ip()),
                            p,
                        )
                    } else {
                        persisted
                            .listen_addr
                            .or(live_addr)
                            .unwrap_or_else(default_daemon_listen)
                    };
                    let addr = resolve_isolated_daemon_listen(preferred, &expected_host_id).await?;
                    persisted.listen_addr = Some(addr);
                    if let Some(r) = sub_relay.or(relay) {
                        persisted.relay_url = Some(normalize_relay_url(&r)?);
                    }
                    DaemonPersistedConfig::update(&home_dir, |cfg| {
                        cfg.relay_url = persisted.relay_url.clone();
                        cfg.listen_addr = persisted.listen_addr;
                    })?;

                    let mgr = service::install_supervised(
                        ServiceRole::Daemon,
                        &home_dir,
                        addr.port(),
                        Some(&expected_host_id),
                        service,
                    )
                    .await?;
                    println!("[✓] Orbit Daemon running in background via {mgr}");
                    println!("    Listen : {addr}");
                    println!(
                        "    Relay  : {}",
                        persisted
                            .relay_url
                            .as_deref()
                            .map(redact_relay_url)
                            .unwrap_or_else(|| "(none — LAN/Tailscale direct)".to_string())
                    );
                    println!("\nRun `orbit pair` to display a pairing URI.");
                }
                Some(DaemonAction::Restart { port }) => {
                    let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                    let expected_host_id = runtime.host_info.host_id.clone();
                    drop(runtime);

                    let mut persisted = DaemonPersistedConfig::load(&home_dir);
                    let preferred = if let Some(p) = port {
                        SocketAddr::new(
                            persisted
                                .listen_addr
                                .map(|a| a.ip())
                                .unwrap_or_else(|| default_daemon_listen().ip()),
                            p,
                        )
                    } else {
                        persisted.listen_addr.unwrap_or_else(default_daemon_listen)
                    };
                    let addr = resolve_isolated_daemon_listen(preferred, &expected_host_id).await?;
                    persisted.listen_addr = Some(addr);
                    DaemonPersistedConfig::update(&home_dir, |cfg| {
                        cfg.listen_addr = persisted.listen_addr
                    })?;

                    let mgr = install_and_start_service(
                        ServiceRole::Daemon,
                        &home_dir,
                        addr.port(),
                        Some(&expected_host_id),
                    )
                    .await?;
                    println!(
                        "[✓] Orbit Daemon restarted via {mgr} (existing agent shims preserved)."
                    );
                }
                Some(DaemonAction::Status) => {
                    let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                    let persisted = DaemonPersistedConfig::load(&home_dir);
                    let base_addr = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
                    let live_addr = orbit_daemon::read_live_daemon_addr(&home_dir);
                    let fallback_addr = live_addr.unwrap_or(base_addr);
                    let active_addr =
                        find_live_daemon_addr_for_host(&runtime.host_info.host_id, fallback_addr)
                            .await;
                    let live_port = active_addr.port();
                    let healthy = probe_daemon_addr(active_addr, &runtime.host_info.host_id).await
                        == PortOwnership::Mine;
                    let effective_ip = if healthy
                        && let Some(la) = live_addr
                        && la.port() == live_port
                    {
                        la.ip()
                    } else {
                        base_addr.ip()
                    };
                    let effective_relay = persisted
                        .relay_url
                        .or_else(|| orbit_daemon::read_live_daemon_relay(&home_dir));
                    println!(
                        "Orbit Daemon : {}",
                        if healthy { "RUNNING" } else { "STOPPED" }
                    );
                    println!(
                        "Listen Addr  : {}",
                        SocketAddr::new(effective_ip, live_port)
                    );
                    println!(
                        "Relay URL    : {}",
                        effective_relay
                            .as_deref()
                            .map(redact_relay_url)
                            .unwrap_or_else(|| "(not configured)".to_string())
                    );
                }
                Some(DaemonAction::Down) => {
                    stop_service(ServiceRole::Daemon, &home_dir)?;
                    println!("[✓] Orbit Daemon stopped.");
                }
            }
        }

        Commands::Relay {
            serve_web,
            listen,
            allow_pubkeys,
            action,
        } => {
            if action.is_some() {
                anyhow::ensure!(
                    serve_web.is_none(),
                    "--serve-web is only supported when running `orbit relay` in the foreground"
                );
            }
            match action {
                None => {
                    let persisted = RelayPersistedConfig::load(&home_dir);
                    let effective_listen = listen.unwrap_or(persisted.listen_addr);
                    let mut allowed: HashSet<String> = if allow_pubkeys.is_empty() {
                        persisted.allowed_pubkeys.into_iter().collect()
                    } else {
                        allow_pubkeys.into_iter().collect()
                    };
                    allowed.retain(|k| !k.trim().is_empty());
                    run_relay_with_web(
                        RelayConfig {
                            listen_addr: effective_listen,
                            allowed_pubkeys: Arc::new(allowed),
                            fleet_token: persisted.fleet_token.clone(),
                        },
                        serve_web.map(web_assets::router).transpose()?,
                    )
                    .await?;
                }
                Some(RelayAction::Up {
                    port,
                    listen: sub_listen,
                    allow_pubkeys: sub_keys,
                    replace_allowlist,
                    public_url,
                    fleet_token_stdin,
                }) => {
                    let mut persisted = RelayPersistedConfig::load(&home_dir);
                    if fleet_token_stdin {
                        let mut raw = String::new();
                        std::io::stdin().read_to_string(&mut raw)?;
                        let tok = raw
                            .lines()
                            .next()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "--fleet-token-stdin expected non-empty token on stdin"
                                )
                            })?
                            .to_string();
                        persisted.fleet_token = Some(tok);
                    }
                    if let Some(l) = sub_listen.or(listen) {
                        persisted.listen_addr = l;
                    } else if let Some(p) = port {
                        persisted.listen_addr = SocketAddr::from(([0, 0, 0, 0], p));
                    }
                    if replace_allowlist {
                        persisted.allowed_pubkeys.clear();
                    }
                    for k in allow_pubkeys.into_iter().chain(sub_keys) {
                        let trimmed = k.trim().to_string();
                        if !trimmed.is_empty() && !persisted.allowed_pubkeys.contains(&trimmed) {
                            persisted.allowed_pubkeys.push(trimmed);
                        }
                    }
                    persisted.save(&home_dir)?;

                    let bound_port = persisted.listen_addr.port();
                    let mgr =
                        install_and_start_service(ServiceRole::Relay, &home_dir, bound_port, None)
                            .await?;

                    let advertised_url = match public_url {
                        Some(u) => normalize_relay_url(&u)?,
                        None => {
                            let host_part = detect_public_ipv4()
                                .await
                                .unwrap_or_else(|| "<YOUR_SERVER_IP>".to_string());
                            format!("ws://{host_part}:{bound_port}")
                        }
                    };

                    println!(
                        "[✓] Orbit Zero-Knowledge E2EE Relay running in background via {mgr}!"
                    );
                    println!("    Listen      : {}", persisted.listen_addr);
                    if persisted.allowed_pubkeys.is_empty() {
                        println!(
                            "    Allowlist   : (open — run `orbit relay allow <PUBKEY>` to lock strictly to your hosts)"
                        );
                    } else {
                        println!(
                            "    Allowlist   : {} authorized host pubkey(s) (HTTP 403 to everyone else)",
                            persisted.allowed_pubkeys.len()
                        );
                    }
                    println!("    Relay URL   : {advertised_url}");
                    println!(
                        "\nNext step — run this on each machine (workstation / Mac mini) to attach:"
                    );
                    println!("  orbit use-relay {advertised_url}");
                    println!(
                        "\n(Tip: Make sure inbound TCP port {bound_port} is open in your cloud firewall / AWS Security Group)"
                    );
                }
                Some(RelayAction::Allow { pubkeys }) => {
                    let mut persisted = RelayPersistedConfig::load(&home_dir);
                    let mut added = 0usize;
                    for k in pubkeys {
                        let trimmed = k.trim().to_string();
                        if !trimmed.is_empty() && !persisted.allowed_pubkeys.contains(&trimmed) {
                            persisted.allowed_pubkeys.push(trimmed);
                            added += 1;
                        }
                    }
                    persisted.save(&home_dir)?;
                    let mgr = install_and_start_service(
                        ServiceRole::Relay,
                        &home_dir,
                        persisted.listen_addr.port(),
                        None,
                    )
                    .await?;
                    println!(
                        "[✓] Authorized {added} new key(s) ({} total in allowlist) and reloaded Relay via {mgr}.",
                        persisted.allowed_pubkeys.len()
                    );
                }
                Some(RelayAction::Status) => {
                    let persisted = RelayPersistedConfig::load(&home_dir);
                    let healthy = check_local_health(persisted.listen_addr.port()).await;
                    println!(
                        "Orbit Relay  : {}",
                        if healthy { "RUNNING" } else { "STOPPED" }
                    );
                    println!("Listen Addr  : {}", persisted.listen_addr);
                    if persisted.fleet_token.is_some() {
                        println!("Fleet Token  : configured (zero-config fleet admission enabled)");
                    }
                    println!("Allowed Keys : {}", persisted.allowed_pubkeys.len());
                    for k in &persisted.allowed_pubkeys {
                        println!("  - {k}");
                    }
                }
                Some(RelayAction::Tunnel {
                    port,
                    allow_pubkeys: sub_keys,
                    hostname,
                    tunnel_token_stdin,
                    no_attach_daemon,
                }) => {
                    let tunnel_token = if tunnel_token_stdin {
                        let mut raw = String::new();
                        std::io::stdin().read_to_string(&mut raw)?;
                        let tok = raw
                        .lines()
                        .next()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "--tunnel-token-stdin expected non-empty Cloudflare token on stdin"
                            )
                        })?
                        .to_string();
                        Some(tok)
                    } else {
                        None
                    };
                    let clean_hostname = hostname
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| {
                            s.trim_start_matches("https://")
                                .trim_start_matches("http://")
                                .trim_start_matches("wss://")
                                .trim_start_matches("ws://")
                                .trim_end_matches('/')
                                .to_string()
                        });
                    if clean_hostname.is_some() ^ tunnel_token.is_some() {
                        anyhow::bail!(
                            "--hostname and --tunnel-token-stdin must be supplied together for a Named Cloudflare Tunnel"
                        );
                    }

                    let fleet_token =
                        orbit_client_core::desktop_ops::load_or_create_fleet_relay_token(&home_dir)
                            .map_err(|e| anyhow::anyhow!(e))?;

                    let mut persisted = RelayPersistedConfig::load(&home_dir);
                    persisted.listen_addr = SocketAddr::from(([127, 0, 0, 1], port));
                    persisted.fleet_token = Some(fleet_token.clone());
                    if let Some((_, local_pk)) = read_local_host_identity(&home_dir)
                        && !persisted.allowed_pubkeys.contains(&local_pk)
                    {
                        persisted.allowed_pubkeys.push(local_pk);
                    }
                    for k in allow_pubkeys.into_iter().chain(sub_keys) {
                        let trimmed = k.trim().to_string();
                        if !trimmed.is_empty() && !persisted.allowed_pubkeys.contains(&trimmed) {
                            persisted.allowed_pubkeys.push(trimmed);
                        }
                    }
                    persisted.save(&home_dir)?;

                    let mgr = install_and_start_service(ServiceRole::Relay, &home_dir, port, None)
                        .await?;
                    let cf_bin = ensure_cloudflared_binary().await?;
                    let tunnel_mgr = start_cloudflare_tunnel_service(
                        &home_dir,
                        &cf_bin,
                        port,
                        clean_hostname.as_deref(),
                        tunnel_token.as_deref(),
                    )?;
                    let cf_host = if let Some(host) = clean_hostname {
                        host
                    } else {
                        wait_for_trycloudflare_host(&home_dir.join("cloudflared-tunnel.log"))
                            .await?
                    };
                    let canonical_url = format!("wss://{cf_host}?ft={fleet_token}");

                    let mut attached_local = false;
                    if !no_attach_daemon && let Some((hid, _)) = read_local_host_identity(&home_dir)
                    {
                        let mut d_cfg = DaemonPersistedConfig::load(&home_dir);
                        d_cfg.relay_url = Some(canonical_url.clone());
                        if d_cfg.save(&home_dir).is_ok() {
                            attached_local = true;
                            let live_port = find_live_daemon_port_for_host(
                                &hid,
                                d_cfg
                                    .listen_addr
                                    .unwrap_or_else(default_daemon_listen)
                                    .port(),
                            )
                            .await;
                            if probe_daemon_port(live_port, &hid).await == PortOwnership::Mine {
                                let ws_url = format!("ws://127.0.0.1:{live_port}/ws");
                                if let Ok(client) =
                                    connect_cli_server(&home_dir, &ws_url, false).await
                                {
                                    let req_id = Uuid::new_v4();
                                    let _ = client
                                    .request(
                                        WireMessage::ConfigureHostRelay {
                                            request_id: req_id,
                                            relay_url: Some(canonical_url.clone()),
                                        },
                                        Duration::from_secs(5),
                                        |w| {
                                            matches!(
                                                w,
                                                WireMessage::HostRelayConfigured { request_id: r, .. }
                                                    if *r == req_id
                                            )
                                        },
                                    )
                                    .await;
                                } else {
                                    let _ = install_and_start_service(
                                        ServiceRole::Daemon,
                                        &home_dir,
                                        live_port,
                                        Some(&hid),
                                    )
                                    .await;
                                }
                            }
                        }
                    }

                    println!(
                        "[✓] Orbit Zero-Port Cloudflare Tunnel Relay running ({mgr} + {tunnel_mgr})!"
                    );
                    println!("    Local Relay : {}", persisted.listen_addr);
                    println!(
                        "    Public URL  : https://{cf_host} (zero inbound firewall ports needed)"
                    );
                    if attached_local {
                        println!("    Local Daemon: attached and reloaded cleanly");
                    }
                    println!(
                        "\nAttach any other machine (keeping the `?ft=` fleet admission token secret):"
                    );
                    println!("  orbit use-relay '{canonical_url}'");
                }
                Some(RelayAction::Down) => {
                    stop_service(ServiceRole::Relay, &home_dir)?;
                    println!("[✓] Orbit Relay stopped.");
                }
            }
        }

        Commands::Ls {
            all,
            workspace,
            parent,
            include_background,
        } => {
            let client = connect_cli_server(&home_dir, &server, via_relay).await?;
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::ListAgents {
                        request_id: req_id,
                        workspace_id: workspace,
                        include_archived: all,
                        include_background,
                        parent_agent_id: parent,
                    },
                    Duration::from_secs(10),
                    |w| matches!(w, WireMessage::AgentsSnapshot { request_id: r, .. } if *r == req_id),
                )
                .await?;
            if let WireMessage::AgentsSnapshot { agents, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&agents)?);
                } else if agents.is_empty() {
                    println!("No agents registered on {server}.");
                } else {
                    for a in agents {
                        println!(
                            "{:<36} | {:<12} | {:<14} | {}",
                            a.agent_id,
                            a.provider.display_name(),
                            format_status(&a.status),
                            a.title
                        );
                    }
                }
            }
        }

        Commands::Run {
            provider,
            acp_agent,
            mode,
            model,
            effort,
            fast,
            no_fast,
            profile,
            cwd,
            workspace,
            title,
            worktree,
            branch,
            base_ref,
            no_wait,
            background,
            auto_archive,
            notify_on_finish,
            auto_approve,
            prompt,
        } => {
            let target_cwd = cwd
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let env_caller = std::env::var("ORBIT_AGENT_ID")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let env_host = std::env::var("ORBIT_HOST_ID")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let effective_notify = notify_on_finish.or_else(|| env_caller.as_ref().map(|_| true));
            let effective_provider = if acp_agent.is_some() {
                ProviderKind::Acp
            } else {
                provider.into()
            };

            let client = connect_cli_server(&home_dir, &server, via_relay).await?;
            let mut sub_rx = client.subscribe();

            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::CreateAgent {
                        request_id: req_id,
                        cwd: target_cwd.to_string_lossy().to_string(),
                        extra_dirs: Vec::new(),
                        title,
                        provider: effective_provider,
                        model,
                        effort: effort.map(Into::into),
                        fast_mode: if no_fast {
                            Some(false)
                        } else if fast {
                            Some(true)
                        } else {
                            None
                        },
                        mode: mode.into(),
                        initial_prompt: prompt,
                        initial_display_prompt: None,
                        initial_badges: Vec::new(),
                        worktree,
                        workspace_id: workspace,
                        parent_agent_id: env_caller,
                        parent_host_id: env_host,
                        parent_title: None,
                        profile_id: profile,
                        background,
                        auto_archive,
                        notify_on_finish: effective_notify,
                        idempotency_key: None,
                        base_ref,
                        worktree_mode: WorktreeCreateMode::BranchOff,
                        branch,
                        handle: None,
                        auto_cluster: None,
                        acp_agent_id: acp_agent,
                        labels: std::collections::HashMap::new(),
                    },
                    Duration::from_secs(45),
                    |w| {
                        matches!(
                            w,
                            WireMessage::AgentCreated { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?;

            let agent = match resp {
                WireMessage::AgentCreated { agent, .. } => agent,
                WireMessage::RpcError { code, message, .. } => {
                    anyhow::bail!("[{code}] {message}");
                }
                _ => anyhow::bail!("unexpected response"),
            };

            if no_wait {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&agent)?);
                } else {
                    println!(
                        "Started {} session `{}` in {}",
                        agent.provider.display_name(),
                        agent.agent_id,
                        agent.cwd
                    );
                }
            } else {
                if !json_mode {
                    println!(
                        "Started {} session `{}` in {}",
                        agent.provider.display_name(),
                        agent.agent_id,
                        agent.cwd
                    );
                }
                stream_turn_to_stdout(&client, &mut sub_rx, &agent.agent_id, auto_approve).await?;
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&agent)?);
                }
            }
        }

        Commands::Send {
            agent_id,
            prompt,
            behavior,
            no_wait,
            notify_on_finish,
            auto_approve,
        } => {
            let env_caller = std::env::var("ORBIT_AGENT_ID")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let resolved_behavior = resolve_cli_send_behavior(behavior, env_caller.is_some());
            let client = connect_cli_server(&home_dir, &server, via_relay).await?;
            let mut sub_rx = client.subscribe();
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::SendPrompt {
                        request_id: req_id,
                        agent_id: agent_id.clone(),
                        prompt,
                        display_prompt: None,
                        badges: Vec::new(),
                        active_turn_behavior: resolved_behavior,
                        message_id: None,
                        caller_agent_id: env_caller,
                        caller_host_id: None,
                        caller_title: None,
                        notify_on_finish,
                    },
                    Duration::from_secs(35),
                    |w| {
                        matches!(
                            w,
                            WireMessage::Ack { request_id: r }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?;
            if let WireMessage::RpcError { code, message, .. } = resp {
                anyhow::bail!("[{code}] {message}");
            }
            if no_wait {
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "ok": true,
                            "agent_id": agent_id
                        }))?
                    );
                } else {
                    println!("[✓] Dispatched prompt to `{agent_id}`");
                }
            } else {
                stream_turn_to_stdout(&client, &mut sub_rx, &agent_id, auto_approve).await?;
            }
        }

        Commands::Wait { agent_id, timeout } => {
            cmd_wait(&home_dir, &server, via_relay, json_mode, &agent_id, timeout).await?;
        }
        Commands::Stop { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::InterruptTurn {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                    preserve_queue: false,
                },
                req_id,
                &format!("Stopped `{agent_id}`"),
            )
            .await?;
        }
        Commands::Inspect { agent_id } => {
            cmd_inspect(&home_dir, &server, via_relay, json_mode, &agent_id).await?;
        }
        Commands::Archive { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::ArchiveAgent {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                },
                req_id,
                &format!("Archived `{agent_id}`"),
            )
            .await?;
        }
        Commands::Unarchive { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::UnarchiveAgent {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                },
                req_id,
                &format!("Unarchived `{agent_id}`"),
            )
            .await?;
        }
        Commands::Detach { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::DetachAgent {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                },
                req_id,
                &format!("Detached `{agent_id}`"),
            )
            .await?;
        }
        Commands::Delete { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::DeleteAgent {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                },
                req_id,
                &format!("Deleted `{agent_id}`"),
            )
            .await?;
        }
        Commands::Mode { agent_id, mode } => {
            let req_id = Uuid::new_v4();
            let sm: SessionMode = mode.into();
            cmd_simple_agent_op(
                &home_dir,
                &server,
                via_relay,
                json_mode,
                WireMessage::SetSessionMode {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                    mode: sm,
                },
                req_id,
                &format!("Updated mode for `{agent_id}`"),
            )
            .await?;
        }

        Commands::Logs { agent_id } => {
            let client = connect_cli_server(&home_dir, &server, via_relay).await?;
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::SubscribeTimeline {
                        request_id: req_id,
                        agent_id,
                        since_seq: 0,
                        tail_limit: None,
                    },
                    Duration::from_secs(10),
                    |w| matches!(w, WireMessage::TimelineCatchup { request_id: r, .. } if *r == req_id),
                )
                .await?;
            if let WireMessage::TimelineCatchup { events, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&events)?);
                } else {
                    for ev in events {
                        print_timeline_payload(ev.seq, &ev.payload);
                    }
                }
            }
        }

        Commands::Fork {
            agent_id,
            at_seq,
            worktree,
            branch,
            base_ref,
            title,
            prompt,
        } => {
            cmd_fork_agent(
                &home_dir, &server, via_relay, json_mode, agent_id, at_seq, worktree, branch,
                base_ref, title, prompt,
            )
            .await?;
        }
        Commands::Agent { action } => {
            cmd_agent(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Workspace { action } => {
            cmd_workspace(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Script { action } => {
            cmd_script(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Terminal { action } => {
            cmd_terminal(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Schedule { action } => {
            cmd_schedule(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Heartbeat { action } => {
            cmd_heartbeat(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Profile { action } => {
            cmd_profile(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Host { action } => {
            cmd_host(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Permit { action } => {
            cmd_permit(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Config { action } => {
            cmd_config(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Plugin { action } => {
            let client = connect_cli_server(&home_dir, &server, via_relay).await?;
            plugin_cli::run(&client, action, json_mode).await?;
        }
        Commands::Skill { action } => {
            cmd_skill(&home_dir, &server, via_relay, json_mode, action).await?;
        }
        Commands::Acp { action } => {
            cmd_acp(
                &home_dir,
                explicit_server.as_deref(),
                &server,
                via_relay,
                json_mode,
                action,
            )
            .await?;
        }
        Commands::Mcp {
            caller_agent_id,
            url,
        } => {
            mcp_bridge::run_mcp_stdio_bridge(&home_dir, caller_agent_id, url).await?;
        }

        Commands::Shim { socket } => {
            let err_sidecar = socket.with_extension("err");
            let res: anyhow::Result<()> = match read_spawn_spec_from_stdin() {
                Ok(spec) => orbit_daemon::run_tool_shim(&socket, spec).await,
                Err(e) => Err(e.into()),
            };
            if let Err(ref e) = res {
                let _ = std::fs::write(&err_sidecar, format!("{e:#}"));
            }
            res?;
        }
        Commands::InternalPreToolUse { agent_id } => {
            run_internal_pre_tool_use(&home_dir, &agent_id).await?;
        }
    }

    Ok(())
}

async fn run_internal_pre_tool_use(
    home_dir: &std::path::Path,
    agent_id: &str,
) -> anyhow::Result<()> {
    let mut stdin_buf = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut stdin_buf);
    let trimmed = stdin_buf.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Ok(());
    };
    let port = orbit_daemon::read_live_daemon_port(home_dir);
    if port == 0 {
        return Ok(());
    }
    let token = std::fs::read_to_string(home_dir.join("mcp-token"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let Some(token) = token else {
        return Ok(());
    };
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .no_proxy()
        .build()
    else {
        return Ok(());
    };
    let url = format!("http://127.0.0.1:{port}/internal/pre-tool-use");
    let body = serde_json::json!({
        "agent_id": agent_id,
        "tool_name": payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or(""),
        "tool_input": payload.get("tool_input").cloned().unwrap_or_else(|| serde_json::json!({})),
        "tool_use_id": payload.get("tool_use_id").and_then(|v| v.as_str()),
    });
    let Ok(resp) = client
        .post(&url)
        .bearer_auth(&token)
        .json(&body)
        .send()
        .await
    else {
        return Ok(());
    };
    if !resp.status().is_success() {
        return Ok(());
    }
    if let Ok(val) = resp.json::<serde_json::Value>().await
        && val.get("decision").and_then(|v| v.as_str()) == Some("deny")
    {
        let reason = val
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("Blocked by Orbit policy or PreToolUse hook");
        let out = serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        });
        println!("{out}");
    }
    Ok(())
}

pub(crate) fn read_local_host_identity(home_dir: &std::path::Path) -> Option<(String, String)> {
    let raw = std::fs::read_to_string(home_dir.join("host-keypair.json")).ok()?;
    let kp: orbit_crypto::HostIdentityKeypair = serde_json::from_str(&raw).ok()?;
    let host_id = kp.host_id().ok()?;
    Some((host_id, kp.public_key_b64))
}

async fn ensure_cloudflared_binary() -> anyhow::Result<PathBuf> {
    if let Some(path_os) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_os) {
            let candidate = dir.join("cloudflared");
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    let home = orbit_client_core::desktop_ops::resolve_user_home()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve user home directory for cloudflared"))?;
    let bin_dir = home.join(".local/bin");
    std::fs::create_dir_all(&bin_dir)?;
    let target = bin_dir.join("cloudflared");
    if target.is_file() {
        return Ok(target);
    }

    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => anyhow::bail!("Unsupported architecture for auto-installing cloudflared: {other}"),
    };
    let os = std::env::consts::OS;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("orbit-cli")
        .build()?;
    match os {
        "linux" => {
            let url = format!(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-{arch}"
            );
            let bytes = client
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            std::fs::write(&target, &bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(target)
        }
        "macos" => {
            let url = format!(
                "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-darwin-{arch}.tgz"
            );
            let bytes = client
                .get(&url)
                .send()
                .await?
                .error_for_status()?
                .bytes()
                .await?;
            let tmp_tgz = bin_dir.join(format!("cloudflared-{}.tgz", std::process::id()));
            std::fs::write(&tmp_tgz, &bytes)?;
            let extract_ok = std::process::Command::new("tar")
                .args(["-xzf"])
                .arg(&tmp_tgz)
                .arg("-C")
                .arg(&bin_dir)
                .status()
                .is_ok_and(|s| s.success());
            let _ = std::fs::remove_file(&tmp_tgz);
            if !extract_ok || !target.is_file() {
                anyhow::bail!(
                    "Failed to extract cloudflared archive; install manually with `brew install cloudflared`"
                );
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))?;
            }
            Ok(target)
        }
        other => anyhow::bail!("Unsupported OS `{other}` for auto-installing cloudflared"),
    }
}

fn start_cloudflare_tunnel_service(
    home_dir: &std::path::Path,
    cf_bin: &std::path::Path,
    relay_port: u16,
    hostname: Option<&str>,
    tunnel_token: Option<&str>,
) -> anyhow::Result<&'static str> {
    std::fs::create_dir_all(home_dir)?;
    let env_path = home_dir.join("cloudflared-tunnel.env");
    let log_path = home_dir.join("cloudflared-tunnel.log");
    let pid_path = home_dir.join("cloudflared-tunnel.pid");
    let _ = std::fs::write(&log_path, "");

    if let Some(tok) = tunnel_token {
        std::fs::write(&env_path, format!("TUNNEL_TOKEN={tok}\n"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))?;
        }
    } else if env_path.exists() {
        let _ = std::fs::remove_file(&env_path);
    }

    let exec_cmd = if hostname.is_some() && tunnel_token.is_some() {
        format!(
            "\"{}\" tunnel --no-autoupdate --logfile \"{}\" run",
            cf_bin.display(),
            log_path.display()
        )
    } else {
        format!(
            "\"{}\" tunnel --no-autoupdate --url http://127.0.0.1:{relay_port} --logfile \"{}\"",
            cf_bin.display(),
            log_path.display()
        )
    };

    if cfg!(target_os = "linux")
        && let Some(user_home) = orbit_client_core::desktop_ops::resolve_user_home()
        && std::process::Command::new("systemctl")
            .args(["--user", "status"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    {
        let systemd_dir = user_home.join(".config/systemd/user");
        std::fs::create_dir_all(&systemd_dir)?;
        let env_line = if env_path.exists() {
            format!("EnvironmentFile={}\n", env_path.display())
        } else {
            String::new()
        };
        let unit = format!(
            "[Unit]\n\
             Description=Orbit Zero-Port Cloudflare Tunnel Relay\n\
             After=network-online.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             {env_line}\
             ExecStart={exec_cmd}\n\
             Restart=always\n\
             RestartSec=3\n\
             StandardOutput=append:{log}\n\
             StandardError=append:{log}\n\
             \n\
             [Install]\n\
             WantedBy=default.target\n",
            log = log_path.display()
        );
        std::fs::write(systemd_dir.join("orbit-relay-tunnel.service"), unit)?;
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let _ = std::process::Command::new("systemctl")
            .args(["--user", "enable", "orbit-relay-tunnel.service"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "restart", "orbit-relay-tunnel.service"])
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return Ok("systemd --user");
        }
    }

    if let Ok(old_pid_str) = std::fs::read_to_string(&pid_path)
        && let Ok(old_pid) = old_pid_str.trim().parse::<u32>()
    {
        let _ = std::process::Command::new("kill")
            .arg(old_pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }

    let out_file = std::fs::File::options()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let err_file = out_file.try_clone()?;

    let mut cmd = std::process::Command::new(cf_bin);
    cmd.args(["tunnel", "--no-autoupdate"]);
    if hostname.is_some()
        && let Some(tok) = tunnel_token
    {
        cmd.arg("--logfile").arg(&log_path).arg("run");
        cmd.env("TUNNEL_TOKEN", tok);
    } else {
        cmd.arg("--url")
            .arg(format!("http://127.0.0.1:{relay_port}"))
            .arg("--logfile")
            .arg(&log_path);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(out_file))
        .stderr(std::process::Stdio::from(err_file));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let child = cmd.spawn()?;
    let _ = std::fs::write(&pid_path, format!("{}\n", child.id()));
    Ok("background process")
}

async fn wait_for_trycloudflare_host(log_path: &std::path::Path) -> anyhow::Result<String> {
    for _ in 0..50 {
        if let Ok(content) = std::fs::read_to_string(log_path) {
            for token in content.split(|c: char| {
                !c.is_ascii_alphanumeric() && c != '-' && c != '.' && c != ':' && c != '/'
            }) {
                if let Some(host) = token.strip_prefix("https://")
                    && host.ends_with(".trycloudflare.com")
                    && host.len() > ".trycloudflare.com".len()
                {
                    return Ok(host.to_string());
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    anyhow::bail!(
        "Timed out waiting for trycloudflare.com URL in {}",
        log_path.display()
    )
}

pub(crate) fn advertised_direct_endpoints(bind_ip: std::net::IpAddr, port: u16) -> Vec<String> {
    if bind_ip.is_unspecified() {
        discover_lan_ws_endpoints(port)
    } else if bind_ip.is_ipv6() {
        vec![format!("ws://[{bind_ip}]:{port}/ws")]
    } else {
        vec![format!("ws://{bind_ip}:{port}/ws")]
    }
}

/// Extract `(host, port)` from a plain `ws://` URL (`127.0.0.1`, `localhost`, `[::1]`, or explicit IP).
fn parse_direct_ws_host_port(raw_url: &str) -> Option<(String, u16)> {
    let trimmed = raw_url.trim();
    if trimmed.len() < 5 || !trimmed[..5].eq_ignore_ascii_case("ws://") {
        return None;
    }
    let rest = &trimmed[5..];
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() || authority.contains('@') || authority.contains('\\') {
        return None;
    }
    if let Some(bracketed) = authority.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        let _: std::net::Ipv6Addr = host.parse().ok()?;
        if after.is_empty() {
            return Some((host.to_string(), 80));
        }
        let port: u16 = after.strip_prefix(':')?.parse().ok()?;
        return (port > 0).then_some((host.to_string(), port));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            if h.contains(':') {
                return None;
            }
            let parsed: u16 = p.parse().ok()?;
            if parsed == 0 {
                return None;
            }
            (h, parsed)
        }
        None => (authority, 80),
    };
    Some((host.to_string(), port))
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Extract the TCP port if `raw_url` is a plain `ws://` URL targeting a local loopback host
/// (`127.0.0.0/8`, `localhost`, or `[::1]`). Rejects remote hosts, TLS URLs, userinfo (`@`), and
/// parser-differential characters so local daemon credentials are never sent to remote targets.
fn parse_loopback_ws_port(raw_url: &str) -> Option<u16> {
    let (host, port) = parse_direct_ws_host_port(raw_url)?;
    is_loopback_host(&host).then_some(port)
}

async fn resolve_cli_direct_secret(
    home_dir: &std::path::Path,
    server_url: &str,
    explicit_secret: Option<&str>,
) -> anyhow::Result<(String, Option<String>)> {
    if let Some(secret) = explicit_secret.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok((secret.to_string(), None));
    }
    let live_addr = orbit_daemon::read_live_daemon_addr(home_dir);
    let probe_addr = if let Some(port) = parse_loopback_ws_port(server_url) {
        SocketAddr::from(([127, 0, 0, 1], port))
    } else if let Some((host, port)) = parse_direct_ws_host_port(server_url)
        && let Ok(ip) = host.parse::<std::net::IpAddr>()
        && let Some(la) = live_addr
        && la.port() == port
        && (la.ip() == ip || la.ip().is_unspecified())
    {
        SocketAddr::new(ip, port)
    } else {
        anyhow::bail!(
            "refusing to send local daemon pairing secret to remote server `{server_url}`; pass `--server orbit://pair/...` or set `ORBIT_PAIRING_SECRET` for this remote target"
        );
    };
    let Some((local_host_id, _)) = read_local_host_identity(home_dir) else {
        anyhow::bail!(
            "missing local host identity at {} (start the local daemon or pass `--server orbit://pair/...` / set `ORBIT_PAIRING_SECRET`)",
            home_dir.join("host-keypair.json").display()
        );
    };
    if probe_daemon_addr(probe_addr, &local_host_id).await != PortOwnership::Mine {
        anyhow::bail!(
            "refusing to send local daemon pairing secret to `{server_url}`: {probe_addr} is not owned by local daemon `{local_host_id}` (pass `--server orbit://pair/...` or set `ORBIT_PAIRING_SECRET`)"
        );
    }
    let secret = orbit_daemon::read_pairing_secret(home_dir).ok_or_else(|| {
        anyhow::anyhow!(
            "missing pairing secret at {} (start the local daemon or pass `--server orbit://pair/...`)",
            home_dir.join(orbit_daemon::PAIRING_SECRET_FILENAME).display()
        )
    })?;
    Ok((secret, Some(local_host_id)))
}

pub(crate) async fn connect_cli_server(
    home_dir: &std::path::Path,
    server: &str,
    via_relay: bool,
) -> anyhow::Result<HostSessionHandle> {
    let trimmed = server.trim();
    if trimmed.starts_with("orbit://pair/") {
        let offer = PairingOffer::from_qr_uri(trimmed)?;
        let profile = SavedHostProfile {
            host_id: offer.host_id,
            hostname: offer.hostname,
            custom_name: None,
            ssh_alias: None,
            route_mode: if via_relay {
                HostRouteMode::RelayOnly
            } else {
                HostRouteMode::Auto
            },
            remote_daemon_port: 6868,
            custom_direct_endpoint: None,
            daemon_pubkey_b64: offer.daemon_pubkey_b64,
            direct_endpoints: offer.direct_endpoints,
            relay_url: offer.relay_url,
            pairing_secret: offer.pairing_secret,
            pairing_secret_updated_at_ms: 0,
            state: HostConnectionState::Disconnected,
        };
        Ok(connect_smart(&profile, None).await?)
    } else {
        let env_secret = std::env::var("ORBIT_PAIRING_SECRET").ok();
        let (secret, expected_host_id) =
            resolve_cli_direct_secret(home_dir, trimmed, env_secret.as_deref()).await?;
        if let Some(host_id) = expected_host_id {
            Ok(connect_direct_verified(&host_id, trimmed, &secret, None).await?)
        } else {
            Ok(connect_direct(trimmed, &secret, None).await?)
        }
    }
}

async fn print_pairing_summary(
    home_dir: &std::path::Path,
    port_override: Option<u16>,
    explicit_endpoints: Vec<String>,
    explicit_relay: Option<String>,
    rotate_secret: bool,
    json_mode: bool,
) -> anyhow::Result<()> {
    let runtime = DaemonRuntime::bootstrap(home_dir).await?;
    if rotate_secret {
        let _ = runtime.rotate_pairing_secret()?;
        if !json_mode {
            eprintln!(
                "[✓] Rotated host pairing secret at {} (active immediately for new connections).",
                home_dir
                    .join(orbit_daemon::PAIRING_SECRET_FILENAME)
                    .display()
            );
        }
    }
    let persisted = DaemonPersistedConfig::load(home_dir);
    let configured_listen = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
    let live_addr = orbit_daemon::read_live_daemon_addr(home_dir);
    let fallback_addr = match port_override {
        Some(p) => SocketAddr::new(
            live_addr
                .map(|a| a.ip())
                .unwrap_or_else(|| configured_listen.ip()),
            p,
        ),
        None => live_addr.unwrap_or(configured_listen),
    };
    let active_addr = if port_override.is_some() {
        fallback_addr
    } else {
        find_live_daemon_addr_for_host(&runtime.host_info.host_id, fallback_addr).await
    };
    let port = active_addr.port();
    let bind_ip = if let Some(la) = live_addr
        && la.port() == port
        && probe_daemon_addr(la, &runtime.host_info.host_id).await == PortOwnership::Mine
    {
        la.ip()
    } else {
        active_addr.ip()
    };
    let endpoints: Vec<String> = if explicit_endpoints.is_empty() {
        advertised_direct_endpoints(bind_ip, port)
    } else {
        explicit_endpoints
            .into_iter()
            .map(|ep| orbit_crypto::normalize_ws_endpoint(&ep))
            .filter(|ep| !ep.is_empty())
            .collect()
    };
    let relay = match explicit_relay {
        Some(r) => Some(normalize_relay_url(&r)?),
        None => persisted
            .relay_url
            .or_else(|| orbit_daemon::read_live_daemon_relay(home_dir)),
    };

    let offer = runtime.build_pairing_offer(endpoints.clone(), relay.clone())?;
    let pairing_uri = offer.to_qr_uri()?;
    if json_mode {
        let payload = serde_json::json!({
            "host_id": runtime.host_info.host_id,
            "hostname": runtime.host_info.hostname,
            "daemon_pubkey_b64": offer.daemon_pubkey_b64,
            "direct_endpoints": endpoints,
            "relay_url": relay.as_deref().map(redact_relay_url),
            "pairing_uri": pairing_uri,
            "secret_rotated": rotate_secret,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        let relay_summary = relay.as_deref().map(redact_relay_url).unwrap_or_else(|| {
            "none (LAN only — run `orbit use-relay <HOST:PORT>` for remote)".into()
        });
        eprintln!(
            "Host `{}` ({}) | Direct: {} | Relay: {}",
            runtime.host_info.hostname,
            runtime.host_info.host_id,
            endpoints.join(", "),
            relay_summary
        );
        println!("{pairing_uri}");
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AllowedPermissionChoices {
    allow: bool,
    allow_always: bool,
    deny: bool,
}

impl AllowedPermissionChoices {
    fn from_wire(allowed_decisions: &[String]) -> Self {
        Self {
            allow: allowed_decisions.iter().any(|d| d == "allow"),
            allow_always: allowed_decisions.iter().any(|d| d == "allow_always"),
            deny: allowed_decisions.iter().any(|d| d == "deny"),
        }
    }

    fn prompt_line(self) -> String {
        let mut opts = Vec::new();
        if self.allow {
            opts.push("[a] allow");
        }
        if self.allow_always {
            opts.push("[A] allow always");
        }
        if self.deny || opts.is_empty() {
            opts.push("[d] deny");
        }
        format!("  {} (default: deny): ", opts.join(" / "))
    }

    fn parse_input(self, raw: &str) -> &'static str {
        match raw.trim() {
            "a" if self.allow => "allow",
            "A" if self.allow_always => "allow_always",
            _ => "deny",
        }
    }
}

/// Render text safely on a single terminal line without truncating security-relevant content:
/// backslashes, newlines, tabs, and all control characters are visibly escaped so literal `\n`
/// cannot be confused with an actual newline.
fn escape_terminal_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let code = c as u32;
                if code <= 0xFF {
                    let _ = write!(out, "\\x{code:02x}");
                } else {
                    let _ = write!(out, "\\u{{{code:x}}}");
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn format_permission_summary(title: &str, detail: &ToolCallDetail) -> String {
    let (tool_label, detail_summary) = match detail {
        ToolCallDetail::Shell { command, .. } => (
            "shell".to_string(),
            format!("$ {}", escape_terminal_text(command)),
        ),
        ToolCallDetail::FileRead { path, .. } => (
            "file_read".to_string(),
            format!("read {}", escape_terminal_text(path)),
        ),
        ToolCallDetail::FileEdit { path, .. } => (
            "file_edit".to_string(),
            format!("edit {}", escape_terminal_text(path)),
        ),
        ToolCallDetail::Search { query, .. } => (
            "search".to_string(),
            format!("search `{}`", escape_terminal_text(query)),
        ),
        ToolCallDetail::Subagent {
            role,
            prompt_preview,
            ..
        } => {
            let role_esc = escape_terminal_text(role.trim());
            let preview_esc = escape_terminal_text(prompt_preview.trim());
            if preview_esc.is_empty() {
                ("subagent".to_string(), format!("subagent ({role_esc})"))
            } else {
                (
                    "subagent".to_string(),
                    format!("subagent ({role_esc}): {preview_esc}"),
                )
            }
        }
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => {
            let name = escape_terminal_text(tool_name.trim());
            let args = escape_terminal_text(arguments_json.trim());
            if args.is_empty() || args == "{}" || args == "null" {
                (name, String::new())
            } else {
                (name, args)
            }
        }
    };
    let title_clean = escape_terminal_text(title.trim());
    let mut parts = Vec::new();
    if !tool_label.is_empty() {
        parts.push(tool_label.clone());
    }
    if !title_clean.is_empty() && !title_clean.eq_ignore_ascii_case(&tool_label) {
        parts.push(title_clean);
    }
    if !detail_summary.is_empty() && !parts.iter().any(|p| p == &detail_summary) {
        parts.push(detail_summary);
    }
    if parts.is_empty() {
        "tool permission".to_string()
    } else {
        parts.join(" — ")
    }
}

fn write_guardian_notice(guardian: Option<&GuardianAssessment>, err_writer: &mut impl Write) {
    let Some(g) = guardian else {
        return;
    };
    let reason = escape_terminal_text(g.reason.trim());
    if reason.is_empty() {
        let _ = writeln!(
            err_writer,
            "  [guardian] {} ({})",
            g.risk_level.display_label(),
            g.decision.as_str()
        );
    } else {
        let _ = writeln!(
            err_writer,
            "  [guardian] {} ({}): {reason}",
            g.risk_level.display_label(),
            g.decision.as_str()
        );
    }
}

struct PendingPermission {
    request_id: String,
    summary: String,
    choices: AllowedPermissionChoices,
    guardian: Option<GuardianAssessment>,
}

struct StdinPromptReply {
    request_id: String,
    line: Option<String>,
}

type StdinWorkerChannels = (
    tokio::sync::mpsc::UnboundedSender<String>,
    tokio::sync::mpsc::UnboundedReceiver<StdinPromptReply>,
);

struct StdinLineSource {
    ask_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    resp_rx: Option<tokio::sync::mpsc::UnboundedReceiver<StdinPromptReply>>,
    starter: Option<Box<dyn FnOnce() -> StdinWorkerChannels + Send>>,
    in_flight: usize,
    eof: bool,
}

impl StdinLineSource {
    fn lazy_os_stdin() -> Self {
        Self {
            ask_tx: None,
            resp_rx: None,
            starter: Some(Box::new(|| {
                let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel::<StdinPromptReply>();
                let _ = std::thread::Builder::new()
                    .name("orbit-cli-perm-stdin".into())
                    .spawn(move || {
                        let stdin = std::io::stdin();
                        let mut locked = stdin.lock();
                        while let Some(request_id) = ask_rx.blocking_recv() {
                            let mut line = String::new();
                            match std::io::BufRead::read_line(&mut locked, &mut line) {
                                Ok(0) | Err(_) => {
                                    let _ = resp_tx.send(StdinPromptReply {
                                        request_id,
                                        line: None,
                                    });
                                    break;
                                }
                                Ok(_) => {
                                    if resp_tx
                                        .send(StdinPromptReply {
                                            request_id,
                                            line: Some(line),
                                        })
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    });
                (ask_tx, resp_rx)
            })),
            in_flight: 0,
            eof: false,
        }
    }

    #[cfg(test)]
    fn from_channels(
        ask_tx: tokio::sync::mpsc::UnboundedSender<String>,
        resp_rx: tokio::sync::mpsc::UnboundedReceiver<StdinPromptReply>,
    ) -> Self {
        Self {
            ask_tx: Some(ask_tx),
            resp_rx: Some(resp_rx),
            starter: None,
            in_flight: 0,
            eof: false,
        }
    }

    fn request_line_for(&mut self, request_id: &str) {
        if self.resp_rx.is_none()
            && let Some(starter) = self.starter.take()
        {
            let (tx, rx) = starter();
            self.ask_tx = Some(tx);
            self.resp_rx = Some(rx);
        }
        if let Some(tx) = self.ask_tx.as_ref()
            && tx.send(request_id.to_string()).is_ok()
        {
            self.in_flight += 1;
        }
    }

    fn drain_stale_replies(&mut self) {
        if let Some(rx) = self.resp_rx.as_mut() {
            while let Ok(reply) = rx.try_recv() {
                self.in_flight = self.in_flight.saturating_sub(1);
                if reply.line.is_none() {
                    self.eof = true;
                }
            }
        }
    }

    fn has_in_flight(&self) -> bool {
        self.in_flight > 0
    }

    async fn next_reply(&mut self) -> Option<StdinPromptReply> {
        match self.resp_rx.as_mut() {
            Some(rx) => match rx.recv().await {
                Some(reply) => {
                    self.in_flight = self.in_flight.saturating_sub(1);
                    if reply.line.is_none() {
                        self.eof = true;
                    }
                    Some(reply)
                }
                None => {
                    self.in_flight = 0;
                    self.eof = true;
                    None
                }
            },
            None => std::future::pending().await,
        }
    }
}

fn display_interactive_prompt(prompt: &PendingPermission, err_writer: &mut impl Write) {
    let _ = writeln!(err_writer, "\n  [permission] {}", prompt.summary);
    write_guardian_notice(prompt.guardian.as_ref(), err_writer);
    let _ = write!(err_writer, "{}", prompt.choices.prompt_line());
    let _ = err_writer.flush();
}

async fn send_permission_resolution<F, Fut, WErr>(
    send_wire: &mut F,
    agent_id: &str,
    permission_request_id: &str,
    decision: &str,
    in_flight_resolves: &mut HashMap<Uuid, String>,
    err_writer: &mut WErr,
) -> anyhow::Result<()>
where
    F: FnMut(WireMessage) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
    WErr: Write,
{
    let rpc_req_id = Uuid::new_v4();
    let msg = WireMessage::ResolvePermission {
        request_id: rpc_req_id,
        agent_id: agent_id.to_string(),
        permission_request_id: permission_request_id.to_string(),
        decision: decision.to_string(),
        agent_originated: false,
    };
    if let Err(e) = send_wire(msg).await {
        let _ = writeln!(
            err_writer,
            "  [permission error] failed to send resolution for `{permission_request_id}`: {e}"
        );
        let _ = err_writer.flush();
        return Err(anyhow::anyhow!(
            "failed to send permission resolution for `{permission_request_id}`: {e}"
        ));
    }
    in_flight_resolves.insert(rpc_req_id, permission_request_id.to_string());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn stream_turn_loop<F, Fut, WOut, WErr>(
    sub_rx: &mut tokio::sync::broadcast::Receiver<ClientInboundEvent>,
    agent_id: &str,
    auto_approve: bool,
    is_interactive_tty: bool,
    stdin: &mut StdinLineSource,
    send_wire: &mut F,
    out_writer: &mut WOut,
    err_writer: &mut WErr,
) -> anyhow::Result<()>
where
    F: FnMut(WireMessage) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
    WOut: Write,
    WErr: Write,
{
    let mut saw_running = false;
    let mut terminal_seen = false;
    let mut pending_ack_deadline: Option<tokio::time::Instant> = None;
    let mut pending_permissions: VecDeque<PendingPermission> = VecDeque::new();
    let mut active_prompt: Option<PendingPermission> = None;
    let mut in_flight_resolves: HashMap<Uuid, String> = HashMap::new();

    loop {
        if terminal_seen && in_flight_resolves.is_empty() {
            break;
        }
        let ack_sleep = async {
            match pending_ack_deadline {
                Some(deadline) => tokio::time::sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            recv_res = sub_rx.recv() => {
                match recv_res {
                    Ok(ClientInboundEvent::Wire(WireMessage::Ack { request_id })) => {
                        in_flight_resolves.remove(&request_id);
                    }
                    Ok(ClientInboundEvent::Wire(WireMessage::RpcError {
                        request_id: Some(req_id),
                        code,
                        message,
                    })) => {
                        if let Some(perm_id) = in_flight_resolves.remove(&req_id) {
                            let _ = writeln!(
                                err_writer,
                                "  [permission error] daemon rejected resolution for `{perm_id}`: [{code}] {message}"
                            );
                            let _ = err_writer.flush();
                            anyhow::bail!(
                                "daemon rejected permission resolution for `{perm_id}`: [{code}] {message}"
                            );
                        }
                    }
                    Ok(ClientInboundEvent::Wire(WireMessage::TimelineLive { event }))
                        if event.agent_id == agent_id && !terminal_seen =>
                    {
                        match event.payload {
                            TimelinePayload::AssistantMessage {
                                ref text,
                                streaming: false,
                                ..
                            } => {
                                let _ = writeln!(out_writer, "\n{text}");
                                let _ = out_writer.flush();
                            }
                            TimelinePayload::ToolCall { status, detail, .. } => {
                                let _ = writeln!(
                                    out_writer,
                                    "  [tool:{status:?}] {}",
                                    summarize_tool(&detail)
                                );
                                let _ = out_writer.flush();
                            }
                            TimelinePayload::PermissionRequest {
                                request_id,
                                title,
                                detail,
                                allowed_decisions,
                                guardian_assessment,
                                ..
                            } => {
                                let summary = format_permission_summary(&title, &detail);
                                let choices = AllowedPermissionChoices::from_wire(&allowed_decisions);
                                if auto_approve {
                                    let decision = if choices.allow {
                                        let _ = writeln!(
                                            err_writer,
                                            "  [permission] {summary} -> approved (--auto-approve)"
                                        );
                                        write_guardian_notice(
                                            guardian_assessment.as_ref(),
                                            err_writer,
                                        );
                                        "allow"
                                    } else {
                                        let _ = writeln!(
                                            err_writer,
                                            "  [permission] {summary} -> denied (provider does not permit `allow`)"
                                        );
                                        write_guardian_notice(
                                            guardian_assessment.as_ref(),
                                            err_writer,
                                        );
                                        "deny"
                                    };
                                    let _ = err_writer.flush();
                                    send_permission_resolution(
                                        send_wire,
                                        agent_id,
                                        &request_id,
                                        decision,
                                        &mut in_flight_resolves,
                                        err_writer,
                                    )
                                    .await?;
                                } else if !is_interactive_tty {
                                    let _ = writeln!(err_writer, "  [permission] {summary}");
                                    write_guardian_notice(
                                        guardian_assessment.as_ref(),
                                        err_writer,
                                    );
                                    let _ = writeln!(
                                        err_writer,
                                        "  Permission denied in non-interactive mode. Pass --auto-approve (-y) to allow unattended execution."
                                    );
                                    let _ = err_writer.flush();
                                    send_permission_resolution(
                                        send_wire,
                                        agent_id,
                                        &request_id,
                                        "deny",
                                        &mut in_flight_resolves,
                                        err_writer,
                                    )
                                    .await?;
                                } else if stdin.eof {
                                    let _ = writeln!(
                                        err_writer,
                                        "  [permission] {summary} -> denied (stdin EOF)"
                                    );
                                    write_guardian_notice(
                                        guardian_assessment.as_ref(),
                                        err_writer,
                                    );
                                    let _ = err_writer.flush();
                                    send_permission_resolution(
                                        send_wire,
                                        agent_id,
                                        &request_id,
                                        "deny",
                                        &mut in_flight_resolves,
                                        err_writer,
                                    )
                                    .await?;
                                } else {
                                    let pending = PendingPermission {
                                        request_id,
                                        summary,
                                        choices,
                                        guardian: guardian_assessment,
                                    };
                                    if active_prompt.is_none() {
                                        stdin.request_line_for(&pending.request_id);
                                        display_interactive_prompt(&pending, err_writer);
                                        active_prompt = Some(pending);
                                    } else {
                                        pending_permissions.push_back(pending);
                                    }
                                }
                            }
                            TimelinePayload::PermissionResolved {
                                request_id,
                                decision,
                            } => {
                                if active_prompt
                                    .as_ref()
                                    .is_some_and(|p| p.request_id == request_id)
                                {
                                    active_prompt = None;
                                    stdin.drain_stale_replies();
                                    let _ = writeln!(
                                        err_writer,
                                        "\n  [permission] `{request_id}` resolved ({decision})"
                                    );
                                    let _ = err_writer.flush();
                                    if let Some(next) = pending_permissions.pop_front() {
                                        if stdin.eof {
                                            let _ = writeln!(
                                                err_writer,
                                                "  [permission] {} -> denied (stdin EOF)",
                                                next.summary
                                            );
                                            let _ = err_writer.flush();
                                            send_permission_resolution(
                                                send_wire,
                                                agent_id,
                                                &next.request_id,
                                                "deny",
                                                &mut in_flight_resolves,
                                                err_writer,
                                            )
                                            .await?;
                                        } else {
                                            stdin.request_line_for(&next.request_id);
                                            display_interactive_prompt(&next, err_writer);
                                            active_prompt = Some(next);
                                        }
                                    }
                                } else {
                                    pending_permissions.retain(|p| p.request_id != request_id);
                                }
                            }
                            TimelinePayload::StatusChanged { status } => match status {
                                AgentStatus::Running { .. }
                                | AgentStatus::AutonomousWakeup { .. } => {
                                    saw_running = true;
                                }
                                AgentStatus::Idle if saw_running => {
                                    terminal_seen = true;
                                    active_prompt = None;
                                    if !in_flight_resolves.is_empty() {
                                        pending_ack_deadline =
                                            Some(tokio::time::Instant::now() + Duration::from_secs(5));
                                    }
                                }
                                AgentStatus::Error { message, .. } => {
                                    let _ = writeln!(err_writer, "\n[Agent Error] {message}");
                                    let _ = err_writer.flush();
                                    terminal_seen = true;
                                    active_prompt = None;
                                    if !in_flight_resolves.is_empty() {
                                        pending_ack_deadline =
                                            Some(tokio::time::Instant::now() + Duration::from_secs(5));
                                    }
                                }
                                AgentStatus::Closed => {
                                    terminal_seen = true;
                                    active_prompt = None;
                                    if !in_flight_resolves.is_empty() {
                                        pending_ack_deadline =
                                            Some(tokio::time::Instant::now() + Duration::from_secs(5));
                                    }
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                    Ok(ClientInboundEvent::Disconnected) => {
                        anyhow::bail!("disconnected from Orbit daemon while streaming `{agent_id}`");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        anyhow::bail!(
                            "timeline event stream lagged by {skipped} event(s) while streaming `{agent_id}`"
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        anyhow::bail!(
                            "daemon event stream closed unexpectedly while streaming `{agent_id}`"
                        );
                    }
                    _ => {}
                }
            }
            reply_opt = stdin.next_reply(), if !terminal_seen && (active_prompt.is_some() || stdin.has_in_flight()) => {
                match reply_opt {
                    Some(StdinPromptReply { request_id, line: Some(line) }) => {
                        if active_prompt
                            .as_ref()
                            .is_some_and(|p| p.request_id == request_id)
                            && let Some(prompt) = active_prompt.take()
                        {
                            let decision = prompt.choices.parse_input(&line);
                            send_permission_resolution(
                                send_wire,
                                agent_id,
                                &prompt.request_id,
                                decision,
                                &mut in_flight_resolves,
                                err_writer,
                            )
                            .await?;
                            if let Some(next) = pending_permissions.pop_front() {
                                stdin.request_line_for(&next.request_id);
                                display_interactive_prompt(&next, err_writer);
                                active_prompt = Some(next);
                            }
                        } else if let Some(ref current) = active_prompt {
                            // Stale line typed for a previously dismissed request_id: never apply to `current`.
                            let _ = writeln!(
                                err_writer,
                                "\n  [permission] ignored stale input for `{request_id}`; awaiting `{}`",
                                current.request_id
                            );
                            let _ = write!(err_writer, "{}", current.choices.prompt_line());
                            let _ = err_writer.flush();
                        }
                    }
                    Some(StdinPromptReply { line: None, .. }) | None => {
                        if let Some(prompt) = active_prompt.take() {
                            let _ = writeln!(err_writer);
                            let _ = err_writer.flush();
                            send_permission_resolution(
                                send_wire,
                                agent_id,
                                &prompt.request_id,
                                "deny",
                                &mut in_flight_resolves,
                                err_writer,
                            )
                            .await?;
                        }
                        while let Some(queued) = pending_permissions.pop_front() {
                            let _ = writeln!(
                                err_writer,
                                "  [permission] {} -> denied (stdin EOF)",
                                queued.summary
                            );
                            let _ = err_writer.flush();
                            send_permission_resolution(
                                send_wire,
                                agent_id,
                                &queued.request_id,
                                "deny",
                                &mut in_flight_resolves,
                                err_writer,
                            )
                            .await?;
                        }
                    }
                }
            }
            () = ack_sleep, if terminal_seen && !in_flight_resolves.is_empty() => {
                anyhow::bail!(
                    "timed out waiting for daemon acknowledgement of {} permission resolution(s)",
                    in_flight_resolves.len()
                );
            }
        }
    }
    Ok(())
}

async fn stream_turn_to_stdout(
    client: &orbit_client_core::HostSessionHandle,
    sub_rx: &mut tokio::sync::broadcast::Receiver<ClientInboundEvent>,
    agent_id: &str,
    auto_approve: bool,
) -> anyhow::Result<()> {
    let is_tty = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    let mut stdin = StdinLineSource::lazy_os_stdin();
    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut send_wire = |msg: WireMessage| async move {
        client
            .send_wire(msg)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    };
    stream_turn_loop(
        sub_rx,
        agent_id,
        auto_approve,
        is_tty,
        &mut stdin,
        &mut send_wire,
        &mut stdout,
        &mut stderr,
    )
    .await
}

pub(crate) fn print_timeline_payload(seq: u64, payload: &TimelinePayload) {
    match payload {
        TimelinePayload::UserPrompt { text, .. } => println!("#{seq:04} [USER] {text}"),
        TimelinePayload::AgentMessage {
            kind,
            source_title,
            text,
            ..
        } => println!(
            "#{seq:04} [AGENT:{kind:?}:{}] {text}",
            source_title.as_deref().unwrap_or("system")
        ),
        TimelinePayload::AssistantMessage {
            text,
            streaming: false,
            ..
        } => println!("#{seq:04} [ASSISTANT] {text}"),
        TimelinePayload::ToolCall { status, detail, .. } => {
            println!("#{seq:04} [TOOL:{status:?}] {}", summarize_tool(detail));
        }
        TimelinePayload::StatusChanged { status } => {
            println!("#{seq:04} [STATUS] {}", format_status(status));
        }
        _ => {}
    }
}

fn summarize_tool(detail: &ToolCallDetail) -> String {
    match detail {
        ToolCallDetail::Shell { command, .. } => format!("$ {command}"),
        ToolCallDetail::FileRead { path, .. } => format!("read {path}"),
        ToolCallDetail::FileEdit { path, .. } => format!("edit {path}"),
        ToolCallDetail::Search { query, .. } => format!("search `{query}`"),
        ToolCallDetail::Subagent { role, .. } => format!("subagent ({role})"),
        ToolCallDetail::Generic { tool_name, .. } => tool_name.clone(),
    }
}

pub(crate) fn format_status(s: &AgentStatus) -> &'static str {
    match s {
        AgentStatus::Initializing => "initializing",
        AgentStatus::Idle => "idle",
        AgentStatus::Running { .. } => "running",
        AgentStatus::AutonomousWakeup { .. } => "wakeup",
        AgentStatus::AwaitingPermission { .. } => "awaiting_perm",
        AgentStatus::Error { .. } => "error",
        AgentStatus::Closed => "closed",
    }
}

fn resolve_cli_send_behavior(
    explicit: Option<CliBehavior>,
    has_agent_caller: bool,
) -> ActiveTurnBehavior {
    match explicit {
        Some(b) => ActiveTurnBehavior::from(b),
        None if has_agent_caller => ActiveTurnBehavior::Queue,
        None => ActiveTurnBehavior::Steer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::{GuardianDecision, GuardianRiskLevel, TimelineEvent};

    fn sample_shell_detail() -> ToolCallDetail {
        ToolCallDetail::Shell {
            command: "cargo test --workspace".into(),
            cwd: None,
            exit_code: None,
            output_preview: None,
        }
    }

    fn timeline_event(agent_id: &str, payload: TimelinePayload) -> ClientInboundEvent {
        ClientInboundEvent::Wire(WireMessage::TimelineLive {
            event: TimelineEvent {
                seq: 1,
                epoch: 0,
                agent_id: agent_id.to_string(),
                turn_id: Some("t1".into()),
                timestamp: chrono::Utc::now(),
                payload,
            },
        })
    }

    #[test]
    fn cli_parses_auto_approve_flags_on_run_and_send() {
        let run_default = Cli::try_parse_from(["orbit", "run", "hello"]).expect("run default");
        assert!(matches!(
            run_default.command,
            Commands::Run {
                auto_approve: false,
                ..
            }
        ));

        let run_long =
            Cli::try_parse_from(["orbit", "run", "--auto-approve", "hello"]).expect("run long");
        assert!(matches!(
            run_long.command,
            Commands::Run {
                auto_approve: true,
                ..
            }
        ));

        let run_short = Cli::try_parse_from(["orbit", "run", "-y", "hello"]).expect("run short");
        assert!(matches!(
            run_short.command,
            Commands::Run {
                auto_approve: true,
                ..
            }
        ));

        let send_default =
            Cli::try_parse_from(["orbit", "send", "ag_1", "continue"]).expect("send default");
        assert!(matches!(
            send_default.command,
            Commands::Send {
                auto_approve: false,
                ..
            }
        ));

        let send_long =
            Cli::try_parse_from(["orbit", "send", "--auto-approve", "ag_1", "continue"])
                .expect("send long");
        assert!(matches!(
            send_long.command,
            Commands::Send {
                auto_approve: true,
                ..
            }
        ));

        let send_short =
            Cli::try_parse_from(["orbit", "send", "-y", "ag_1", "continue"]).expect("send short");
        assert!(matches!(
            send_short.command,
            Commands::Send {
                auto_approve: true,
                ..
            }
        ));
    }

    #[test]
    fn allowed_choices_enforces_strict_grammar_and_provider_advertised_options() {
        let all = AllowedPermissionChoices::from_wire(&[
            "allow".into(),
            "allow_always".into(),
            "deny".into(),
        ]);
        assert_eq!(
            all.prompt_line(),
            "  [a] allow / [A] allow always / [d] deny (default: deny): "
        );
        assert_eq!(all.parse_input("a\n"), "allow");
        assert_eq!(all.parse_input(" a \r\n"), "allow");
        assert_eq!(all.parse_input("A\n"), "allow_always");
        assert_eq!(all.parse_input("d\n"), "deny");
        assert_eq!(all.parse_input("\n"), "deny");
        assert_eq!(all.parse_input("allow\n"), "deny");
        assert_eq!(all.parse_input("always\n"), "deny");

        // Codex-style provider: only `allow` and `deny` advertised -> `[A]` omitted & rejected.
        let codex = AllowedPermissionChoices::from_wire(&["allow".into(), "deny".into()]);
        assert_eq!(
            codex.prompt_line(),
            "  [a] allow / [d] deny (default: deny): "
        );
        assert_eq!(codex.parse_input("a\n"), "allow");
        assert_eq!(codex.parse_input("A\n"), "deny");
        assert_eq!(codex.parse_input("\n"), "deny");

        // Deny-only permission -> both `a` and `A` fail closed to `deny`.
        let deny_only = AllowedPermissionChoices::from_wire(&["deny".into()]);
        assert_eq!(deny_only.prompt_line(), "  [d] deny (default: deny): ");
        assert_eq!(deny_only.parse_input("a\n"), "deny");
        assert_eq!(deny_only.parse_input("A\n"), "deny");
    }

    #[test]
    fn permission_summary_preserves_full_command_and_escapes_backslashes_control_chars_and_tool_names()
     {
        // Literal `\n` must escape to `\\n`, distinct from an actual newline (`\n`).
        assert_eq!(escape_terminal_text(r"lit\nreal"), r"lit\\nreal");
        assert_eq!(escape_terminal_text("lit\nreal"), r"lit\nreal");

        let long_prefix = "echo ".to_string() + &"x".repeat(200);
        let full_cmd = format!("{long_prefix}\\nliteral\n\x1b[31mrm -rf /tmp/evil_suffix\r");
        let shell_summary = format_permission_summary(
            "Run\x07Cmd",
            &ToolCallDetail::Shell {
                command: full_cmd,
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
        );
        assert!(
            shell_summary.contains("evil_suffix"),
            "must never truncate command suffix"
        );
        assert!(shell_summary.contains(r"\\nliteral\n"));
        assert!(shell_summary.contains("\\r"));
        assert!(shell_summary.contains("\\x1b[31m"));
        assert!(shell_summary.contains("\\x07"));
        assert!(!shell_summary.contains('\x1b'));
        assert!(!shell_summary.contains('\n'));

        let generic_summary = format_permission_summary(
            "Generic",
            &ToolCallDetail::Generic {
                tool_name: "mcp\x1b[2J_tool\n".into(),
                arguments_json: r#"{"cmd":"a\nb"}"#.into(),
            },
        );
        assert!(generic_summary.contains("mcp\\x1b[2J_tool"));
        assert!(generic_summary.contains(r"a\\nb"));
        assert!(!generic_summary.contains('\x1b'));
    }

    #[tokio::test]
    async fn stream_auto_approve_respects_allowed_decisions_and_displays_guardian_reason() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let tx_for_ack = tx.clone();
        let sent = Arc::new(tokio::sync::Mutex::new(Vec::<WireMessage>::new()));
        let sent_ref = Arc::clone(&sent);
        let (ask_tx, _ask_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut stdin = StdinLineSource::from_channels(ask_tx, resp_rx);
        let mut out = Vec::new();
        let mut err = Vec::new();

        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::StatusChanged {
                status: AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            },
        ))
        .expect("send running");
        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::PermissionRequest {
                request_id: "perm_allow".into(),
                title: "Run tests".into(),
                detail: sample_shell_detail(),
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: Some(GuardianAssessment {
                    risk_level: GuardianRiskLevel::Medium,
                    decision: GuardianDecision::RequireHuman,
                    confidence: 0.8,
                    classifier_tier: "deterministic".into(),
                    reason: "writes outside repo\ncheck target".into(),
                    signals: Vec::new(),
                    latency_ms: 4,
                }),
                plan_approval: None,
            },
        ))
        .expect("send perm_allow");
        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::PermissionRequest {
                request_id: "perm_deny_only".into(),
                title: "Blocked op".into(),
                detail: sample_shell_detail(),
                allowed_decisions: vec!["deny".into()],
                guardian_assessment: None,
                plan_approval: None,
            },
        ))
        .expect("send perm_deny_only");
        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::StatusChanged {
                status: AgentStatus::Idle,
            },
        ))
        .expect("send idle");

        let mut send_wire = |msg: WireMessage| {
            let sent = Arc::clone(&sent_ref);
            let tx = tx_for_ack.clone();
            async move {
                if let WireMessage::ResolvePermission { request_id, .. } = &msg {
                    let _ = tx.send(ClientInboundEvent::Wire(WireMessage::Ack {
                        request_id: *request_id,
                    }));
                }
                sent.lock().await.push(msg);
                Ok(())
            }
        };
        stream_turn_loop(
            &mut rx,
            "ag_1",
            true,
            false,
            &mut stdin,
            &mut send_wire,
            &mut out,
            &mut err,
        )
        .await
        .expect("stream loop");

        let recorded = sent.lock().await;
        assert_eq!(recorded.len(), 2);
        assert!(matches!(
            &recorded[0],
            WireMessage::ResolvePermission {
                permission_request_id,
                decision,
                ..
            } if permission_request_id == "perm_allow" && decision == "allow"
        ));
        assert!(matches!(
            &recorded[1],
            WireMessage::ResolvePermission {
                permission_request_id,
                decision,
                ..
            } if permission_request_id == "perm_deny_only" && decision == "deny"
        ));

        let err_text = String::from_utf8(err).expect("utf8 err");
        assert!(err_text.contains("--auto-approve"));
        assert!(err_text.contains("writes outside repo\\ncheck target"));
        assert!(err_text.contains("provider does not permit `allow`"));
    }

    #[tokio::test]
    async fn stream_non_tty_fails_closed_and_errors_on_correlated_rpc_error_even_after_idle() {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let tx_for_send = tx.clone();
        let (ask_tx, _ask_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut stdin = StdinLineSource::from_channels(ask_tx, resp_rx);
        let mut out = Vec::new();
        let mut err = Vec::new();

        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::StatusChanged {
                status: AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            },
        ))
        .expect("send running");
        tx.send(timeline_event(
            "ag_1",
            TimelinePayload::PermissionRequest {
                request_id: "perm_1".into(),
                title: "Run shell".into(),
                detail: sample_shell_detail(),
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
            },
        ))
        .expect("send perm");

        let mut send_wire = |msg: WireMessage| {
            let tx = tx_for_send.clone();
            async move {
                if let WireMessage::ResolvePermission {
                    request_id,
                    decision,
                    ..
                } = msg
                {
                    assert_eq!(decision, "deny");
                    // Send terminal Idle BEFORE RpcError to prove we await pending resolution
                    // and still fail on correlated RpcError instead of prematurely returning Ok(()).
                    let _ = tx.send(timeline_event(
                        "ag_1",
                        TimelinePayload::StatusChanged {
                            status: AgentStatus::Idle,
                        },
                    ));
                    let _ = tx.send(ClientInboundEvent::Wire(WireMessage::RpcError {
                        request_id: Some(request_id),
                        code: "resolve_permission_failed".into(),
                        message: "already resolved".into(),
                    }));
                }
                Ok(())
            }
        };

        let res = stream_turn_loop(
            &mut rx,
            "ag_1",
            false,
            false,
            &mut stdin,
            &mut send_wire,
            &mut out,
            &mut err,
        )
        .await;
        let rpc_err = res.expect_err("correlated RpcError must return Err");
        assert!(rpc_err.to_string().contains("already resolved"));

        let err_text = String::from_utf8(err).expect("utf8 err");
        assert!(err_text.contains(
            "Permission denied in non-interactive mode. Pass --auto-approve (-y) to allow unattended execution."
        ));
        assert!(err_text.contains("daemon rejected resolution for `perm_1`"));
        assert!(err_text.contains("already resolved"));
    }

    #[tokio::test]
    async fn interactive_loop_binds_stdin_to_request_id_and_never_applies_stale_input_to_next_prompt()
     {
        let (tx, mut rx) = tokio::sync::broadcast::channel(16);
        let tx_for_ack = tx.clone();
        let (ask_tx, mut ask_rx) = tokio::sync::mpsc::unbounded_channel();
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut stdin = StdinLineSource::from_channels(ask_tx, resp_rx);
        let sent = Arc::new(tokio::sync::Mutex::new(Vec::<(String, String)>::new()));
        let sent_ref = Arc::clone(&sent);
        let mut out = Vec::new();
        let mut err = Vec::new();

        // Driver reproduces the exact race:
        // 1. P1 (`perm_ext`) and P2 (`perm_local`) arrive.
        // 2. While P1 is active, another client resolves P1 AND a stale `"a\n"` reply bound to
        //    `perm_ext` arrives AFTER P2 has been activated -> MUST NOT approve P2!
        // 3. Operator then explicitly answers `"d\n"` for P2 -> P2 resolves as `"deny"`.
        let driver = tokio::spawn(async move {
            let _ = tx.send(timeline_event(
                "ag_1",
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
                },
            ));
            let _ = tx.send(timeline_event(
                "ag_1",
                TimelinePayload::PermissionRequest {
                    request_id: "perm_ext".into(),
                    title: "First".into(),
                    detail: sample_shell_detail(),
                    allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                },
            ));
            let _ = tx.send(timeline_event(
                "ag_1",
                TimelinePayload::PermissionRequest {
                    request_id: "perm_local".into(),
                    title: "Second".into(),
                    detail: sample_shell_detail(),
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                },
            ));
            assert_eq!(ask_rx.recv().await.as_deref(), Some("perm_ext"));

            // Externally resolve P1 -> activates P2 and requests stdin for `perm_local`.
            let _ = tx.send(timeline_event(
                "ag_1",
                TimelinePayload::PermissionResolved {
                    request_id: "perm_ext".into(),
                    decision: "allow".into(),
                },
            ));
            assert_eq!(ask_rx.recv().await.as_deref(), Some("perm_local"));

            // Late stale `"a\n"` arrives tagged for dismissed `perm_ext` -> MUST be ignored!
            let _ = resp_tx.send(StdinPromptReply {
                request_id: "perm_ext".into(),
                line: Some("a\n".into()),
            });
            // Now actual decision for `perm_local` arrives -> `"deny"`.
            let _ = resp_tx.send(StdinPromptReply {
                request_id: "perm_local".into(),
                line: Some("d\n".into()),
            });
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = tx.send(timeline_event(
                "ag_1",
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Idle,
                },
            ));
        });

        let mut send_wire = |msg: WireMessage| {
            let sent = Arc::clone(&sent_ref);
            let tx = tx_for_ack.clone();
            async move {
                if let WireMessage::ResolvePermission {
                    request_id,
                    permission_request_id,
                    decision,
                    ..
                } = msg
                {
                    sent.lock().await.push((permission_request_id, decision));
                    let _ = tx.send(ClientInboundEvent::Wire(WireMessage::Ack { request_id }));
                }
                Ok(())
            }
        };

        stream_turn_loop(
            &mut rx,
            "ag_1",
            false,
            true,
            &mut stdin,
            &mut send_wire,
            &mut out,
            &mut err,
        )
        .await
        .expect("interactive loop");
        driver.await.expect("driver");

        let recorded = sent.lock().await;
        assert_eq!(
            *recorded,
            vec![("perm_local".to_string(), "deny".to_string())]
        );
        let err_text = String::from_utf8(err).expect("utf8 err");
        assert!(err_text.contains("`perm_ext` resolved (allow)"));
        assert!(err_text.contains("ignored stale input for `perm_ext`"));
        assert!(err_text.contains("[a] allow / [d] deny (default: deny): "));
    }

    #[tokio::test]
    async fn stream_errors_explicitly_on_lagged_broadcast_and_transport_failure() {
        // 1. Lagged broadcast channel MUST return Err instead of silent Ok(()).
        let (tx, mut rx) = tokio::sync::broadcast::channel(1);
        let (ask_tx, _ask_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut stdin = StdinLineSource::from_channels(ask_tx, resp_rx);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let _ = tx.send(timeline_event(
            "ag_1",
            TimelinePayload::StatusChanged {
                status: AgentStatus::Idle,
            },
        ));
        let _ = tx.send(timeline_event(
            "ag_1",
            TimelinePayload::StatusChanged {
                status: AgentStatus::Idle,
            },
        ));
        let mut noop_send = |_msg: WireMessage| async { Ok(()) };
        let lag_err = stream_turn_loop(
            &mut rx,
            "ag_1",
            false,
            false,
            &mut stdin,
            &mut noop_send,
            &mut out,
            &mut err,
        )
        .await
        .expect_err("lagged stream must error");
        assert!(lag_err.to_string().contains("lagged"));

        // 2. Transport send error on ResolvePermission MUST propagate and report to stderr.
        let (tx2, mut rx2) = tokio::sync::broadcast::channel(4);
        let (ask_tx2, _ask_rx2) = tokio::sync::mpsc::unbounded_channel();
        let (_resp_tx2, resp_rx2) = tokio::sync::mpsc::unbounded_channel();
        let mut stdin2 = StdinLineSource::from_channels(ask_tx2, resp_rx2);
        let mut out2 = Vec::new();
        let mut err2 = Vec::new();
        let _ = tx2.send(timeline_event(
            "ag_1",
            TimelinePayload::PermissionRequest {
                request_id: "perm_fail".into(),
                title: "Run".into(),
                detail: sample_shell_detail(),
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
            },
        ));
        let mut failing_send = |_msg: WireMessage| async { Err(anyhow::anyhow!("ws broken")) };
        let send_err = stream_turn_loop(
            &mut rx2,
            "ag_1",
            true,
            false,
            &mut stdin2,
            &mut failing_send,
            &mut out2,
            &mut err2,
        )
        .await
        .expect_err("send failure must error");
        assert!(send_err.to_string().contains("ws broken"));
        let err2_text = String::from_utf8(err2).expect("utf8 err2");
        assert!(err2_text.contains("failed to send resolution for `perm_fail`"));
    }

    #[test]
    fn cli_parses_pair_rotate_secret_flag() {
        let parsed = Cli::try_parse_from(["orbit", "pair", "--rotate-secret"])
            .expect("parse orbit pair --rotate-secret");
        match parsed.command {
            Commands::Pair { rotate_secret, .. } => assert!(rotate_secret),
            _ => panic!("expected Commands::Pair"),
        }

        let default_pair = Cli::try_parse_from(["orbit", "pair"]).expect("parse orbit pair");
        match default_pair.command {
            Commands::Pair { rotate_secret, .. } => assert!(!rotate_secret),
            _ => panic!("expected Commands::Pair"),
        }
    }

    #[test]
    fn parse_loopback_ws_port_accepts_only_plain_loopback() {
        assert_eq!(parse_loopback_ws_port("ws://127.0.0.1:6868/ws"), Some(6868));
        assert_eq!(
            parse_loopback_ws_port("WS://LOCALHOST:6870/ws?x=1"),
            Some(6870)
        );
        assert_eq!(parse_loopback_ws_port("ws://[::1]:6868/ws"), Some(6868));
        assert_eq!(parse_loopback_ws_port("ws://127.0.0.2:6868/ws"), Some(6868));

        // Remote hosts, TLS, userinfo spoofs, and parser differentials must be rejected
        assert_eq!(parse_loopback_ws_port("ws://192.168.1.10:6868/ws"), None);
        assert_eq!(parse_loopback_ws_port("ws://evil.example:6868/ws"), None);
        assert_eq!(parse_loopback_ws_port("wss://127.0.0.1:6868/ws"), None);
        assert_eq!(
            parse_loopback_ws_port("ws://127.0.0.1@evil.example:6868/ws"),
            None
        );
        assert_eq!(
            parse_loopback_ws_port("ws://127.0.0.1.evil.example:6868/ws"),
            None
        );
        assert_eq!(
            parse_loopback_ws_port("ws://127.0.0.1\\@evil.example:6868/ws"),
            None
        );
    }

    #[tokio::test]
    async fn resolve_cli_direct_secret_refuses_sending_local_secret_to_remote_server() {
        let tmp = std::env::temp_dir().join(format!("orbit_cli_sec_test_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&tmp).expect("mkdir");
        let _rt = DaemonRuntime::bootstrap(&tmp).await.expect("bootstrap");

        // Even with a valid local pairing-secret on disk, targeting a remote server without
        // explicit credential MUST fail closed instead of leaking the local secret.
        let err = resolve_cli_direct_secret(&tmp, "ws://remote.example:6868/ws", None)
            .await
            .expect_err("must refuse sending local secret to remote URL");
        assert!(
            err.to_string()
                .contains("refusing to send local daemon pairing secret"),
            "unexpected error: {err}"
        );

        // Explicit secret is still honored for intentional remote targets.
        let (sec, expected_hid) = resolve_cli_direct_secret(
            &tmp,
            "ws://remote.example:6868/ws",
            Some("explicit-remote-secret"),
        )
        .await
        .expect("explicit secret allowed");
        assert_eq!(sec, "explicit-remote-secret");
        assert!(expected_hid.is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn advertised_direct_endpoints_respects_bind_address() {
        assert_eq!(
            advertised_direct_endpoints(std::net::IpAddr::from([127, 0, 0, 1]), 6868),
            vec!["ws://127.0.0.1:6868/ws".to_string()]
        );
        assert_eq!(
            advertised_direct_endpoints(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST), 6868),
            vec!["ws://[::1]:6868/ws".to_string()]
        );
        assert_eq!(
            advertised_direct_endpoints(std::net::IpAddr::from([10, 20, 30, 40]), 6900),
            vec!["ws://10.20.30.40:6900/ws".to_string()]
        );
        // Wildcard bind enumerates LAN endpoints (at least one endpoint returned).
        let wildcard = advertised_direct_endpoints(std::net::IpAddr::from([0, 0, 0, 0]), 6868);
        assert!(!wildcard.is_empty());
    }

    #[test]
    fn shim_subcommand_accepts_socket_and_rejects_legacy_spec_json_argv() {
        let ok = Cli::try_parse_from(["orbit", "shim", "--socket", "/tmp/test.sock"]);
        assert!(ok.is_ok(), "orbit shim --socket <path> must parse cleanly");

        let rejected = Cli::try_parse_from([
            "orbit",
            "shim",
            "--socket",
            "/tmp/test.sock",
            "--spec-json",
            "{}",
        ]);
        assert!(
            rejected.is_err(),
            "legacy insecure --spec-json CLI flag must be removed"
        );
    }

    #[test]
    fn resolve_cli_send_behavior_decouples_agent_and_human_defaults() {
        assert_eq!(
            resolve_cli_send_behavior(None, false),
            ActiveTurnBehavior::Steer
        );
        assert_eq!(
            resolve_cli_send_behavior(None, true),
            ActiveTurnBehavior::Queue
        );
        assert_eq!(
            resolve_cli_send_behavior(Some(CliBehavior::Steer), true),
            ActiveTurnBehavior::Steer
        );
        assert_eq!(
            resolve_cli_send_behavior(Some(CliBehavior::Interrupt), false),
            ActiveTurnBehavior::Interrupt
        );
    }
}
