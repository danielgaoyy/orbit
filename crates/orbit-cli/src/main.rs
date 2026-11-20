mod mcp_bridge;
mod orchestration_cli;
mod service;
mod ssh_config;

use std::collections::HashSet;
use std::io::{IsTerminal, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use orbit_client_core::{
    ClientInboundEvent, HostConnectionState, HostRouteMode, HostSessionHandle, SavedHostProfile,
    connect_direct, connect_smart,
};
use orbit_crypto::PairingOffer;
use orbit_daemon::{
    DaemonConfig, DaemonPersistedConfig, DaemonRuntime, discover_lan_ws_endpoints, run_daemon,
};
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, ProviderKind, SessionMode, TimelinePayload, ToolCallDetail,
    WireMessage, WorktreeCreateMode,
};
use orbit_providers::{discover_installed_providers, probe_all_provider_quotas};
use orbit_relay::{RelayConfig, RelayPersistedConfig, run_relay};
use orbit_relay_transport::{is_ssh_relay_url, redact_relay_url};
use orbit_shim::{ShimSpawnSpec, run_shim};
use uuid::Uuid;

use crate::orchestration_cli::{
    CliBehavior, CliEffort, ConfigAction, HeartbeatAction, HostAction, PermitAction, ProfileAction,
    ScheduleAction, ScriptAction, SkillAction, TerminalAction, WorkspaceAction, cmd_config,
    cmd_heartbeat, cmd_host, cmd_inspect, cmd_permit, cmd_profile, cmd_schedule, cmd_script,
    cmd_simple_agent_op, cmd_skill, cmd_terminal, cmd_wait, cmd_workspace, resolve_server_url,
};
use crate::service::{
    PortOwnership, RelayProbeOutcome, ServiceRole, check_local_health, default_daemon_listen,
    detect_public_ipv4, find_live_daemon_port_for_host, install_and_start_service,
    normalize_relay_url, preflight_probe_relay, probe_daemon_port, resolve_isolated_daemon_listen,
    self_upgrade_cli, stop_service,
};
use crate::ssh_config::{discover_ssh_hosts, provision_relay_via_ssh};

#[derive(Parser)]
#[command(
    name = "orbit",
    version,
    about = "Ultra-fast Multi-Host AI Coding Agent Orchestrator (Antigravity, Codex, Claude Code)"
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
    Codex,
    #[value(alias = "claude")]
    ClaudeCode,
}

impl From<CliProvider> for ProviderKind {
    fn from(v: CliProvider) -> Self {
        match v {
            CliProvider::Antigravity => ProviderKind::Antigravity,
            CliProvider::Codex => ProviderKind::Codex,
            CliProvider::ClaudeCode => ProviderKind::ClaudeCode,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliMode {
    Default,
    AcceptEdits,
    Plan,
    FullAccess,
}

impl From<CliMode> for SessionMode {
    fn from(v: CliMode) -> Self {
        match v {
            CliMode::Default => SessionMode::Default,
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
enum Commands {
    /// Inspect current host environment, identity keypair, Relay attachment, and installed AI CLIs.
    Doctor,

    /// Fetch live 5-hour / weekly usage quotas & remaining credits across Antigravity, Codex, and Claude Code.
    Quotas,

    /// One-click self-upgrade `orbit` binary to the latest GitHub Release (and hot-restart active daemon/relay).
    Upgrade {
        /// Force reinstall even if already on the latest version.
        #[arg(long)]
        force: bool,
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
    },

    /// Run or manage the host-local Orbit Daemon (`orbit daemon up | status | restart | down`).
    Daemon {
        #[arg(long)]
        listen: Option<SocketAddr>,
        #[arg(long, env = "ORBIT_RELAY_URL")]
        relay: Option<String>,
        #[command(subcommand)]
        action: Option<DaemonAction>,
    },

    /// Run or manage the `< 6MB` zero-knowledge E2EE Relay (`orbit relay up | allow | status | down`).
    Relay {
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
        #[arg(short, long, value_enum, default_value = "accept-edits")]
        mode: CliMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum)]
        effort: Option<CliEffort>,
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
        /// Initial prompt to send to the agent.
        prompt: String,
    },

    /// Send a follow-up prompt to an existing (or persisted) agent.
    Send {
        agent_id: String,
        prompt: String,
        #[arg(long, value_enum, default_value = "steer")]
        behavior: CliBehavior,
        #[arg(long)]
        no_wait: bool,
        #[arg(long)]
        notify_on_finish: Option<bool>,
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

    /// Inspect or install bundled Orbit Orchestration skills (`orbit`, `orbit-handoff`, `orbit-committee`, `orbit-advisor`).
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },

    /// Stdio-to-HTTP MCP 2025-03-26 JSON-RPC bridge connecting external MCP clients to the local Orbit Daemon.
    Mcp {
        #[arg(long)]
        caller_agent_id: Option<String>,
        #[arg(long)]
        url: Option<String>,
    },

    /// Internal detached process supervisor invoked by `orbit daemon`.
    #[command(hide = true)]
    Shim {
        #[arg(long)]
        socket: PathBuf,
        #[arg(long)]
        spec_json: String,
    },
}

fn resolve_home(override_home: Option<PathBuf>) -> PathBuf {
    if let Some(h) = override_home {
        return h;
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".orbit")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let home_dir = resolve_home(cli.home);
    let json_mode = cli.json;
    let via_relay = cli.via_relay;
    let server = resolve_server_url(&home_dir, cli.server);

    let is_mcp_stdio = matches!(cli.command, Commands::Mcp { .. });
    if !is_mcp_stdio {
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
        let use_rotating_daemon_log = matches!(cli.command, Commands::Daemon { action: None, .. });
        if use_rotating_daemon_log
            && let Ok(log_writer) = orbit_client_core::logging::RotatingLogWriter::open(
                home_dir.join("logs").join("daemon.log"),
                orbit_client_core::logging::DEFAULT_LOG_MAX_BYTES,
                orbit_client_core::logging::DEFAULT_LOG_MAX_BACKUPS,
                std::io::stderr().is_terminal(),
            )
        {
            tracing_subscriber::fmt()
                .with_ansi(false)
                .with_env_filter(env_filter)
                .with_writer(move || log_writer.clone())
                .init();
        } else {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(env_filter)
                .init();
        }
    }

    match cli.command {
        Commands::Doctor => {
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let persisted = DaemonPersistedConfig::load(&home_dir);
            let fallback_port = persisted
                .listen_addr
                .unwrap_or_else(default_daemon_listen)
                .port();
            let active_port =
                find_live_daemon_port_for_host(&runtime.host_info.host_id, fallback_port).await;
            let daemon_live = probe_daemon_port(active_port, &runtime.host_info.host_id).await
                == PortOwnership::Mine;

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
            match &persisted.relay_url {
                Some(r) => println!("Relay Route  : {} (Noise NK E2EE)", redact_relay_url(r)),
                None => println!(
                    "Relay Route  : (not configured — LAN/Tailscale only; run `orbit use-relay <HOST:PORT>`)"
                ),
            }

            println!("\nDiscovered Coding Agent CLIs:");
            let providers = discover_installed_providers();
            if providers.is_empty() {
                println!("  (none detected in PATH / ~/.local/bin)");
            } else {
                for p in providers {
                    println!(
                        "  [✓] {:<13} -> {} ({})",
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

        Commands::Upgrade { force } => {
            self_upgrade_cli(&home_dir, force).await?;
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
                persisted.save(&home_dir)?;
                let mgr = install_and_start_service(
                    ServiceRole::Daemon,
                    &home_dir,
                    listen.port(),
                    Some(&expected_host_id),
                )
                .await?;
                println!("[✓] Detached Relay configuration and restarted daemon via {mgr}.");
                print_pairing_summary(&home_dir, Some(listen.port()), Vec::new(), None).await?;
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
            persisted.save(&home_dir)?;

            let mgr = install_and_start_service(
                ServiceRole::Daemon,
                &home_dir,
                listen.port(),
                Some(&expected_host_id),
            )
            .await?;
            println!("[✓] Orbit Daemon active via {mgr} -> attached to {display_relay}");
            println!(
                "\nCopy & import this updated Pairing Code in Orbit App (Mac / iOS / Android):"
            );
            print_pairing_summary(&home_dir, Some(listen.port()), Vec::new(), Some(normalized))
                .await?;
        }

        Commands::Pair {
            port,
            endpoints,
            relay,
        } => {
            print_pairing_summary(&home_dir, port, endpoints, relay).await?;
        }

        Commands::Daemon {
            listen,
            relay,
            action,
        } => match action {
            None => {
                let persisted = DaemonPersistedConfig::load(&home_dir);
                let effective_listen = listen
                    .or(persisted.listen_addr)
                    .unwrap_or_else(default_daemon_listen);
                let effective_relay = relay.or(persisted.relay_url);
                run_daemon(DaemonConfig {
                    home_dir,
                    listen_addr: effective_listen,
                    relay_url: effective_relay,
                })
                .await?;
            }
            Some(DaemonAction::Up {
                port,
                listen: sub_listen,
                relay: sub_relay,
            }) => {
                let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                let expected_host_id = runtime.host_info.host_id.clone();
                drop(runtime);

                let mut persisted = DaemonPersistedConfig::load(&home_dir);
                let preferred = if let Some(l) = sub_listen.or(listen) {
                    l
                } else if let Some(p) = port {
                    SocketAddr::from(([0, 0, 0, 0], p))
                } else {
                    persisted.listen_addr.unwrap_or_else(default_daemon_listen)
                };
                let addr = resolve_isolated_daemon_listen(preferred, &expected_host_id).await?;
                persisted.listen_addr = Some(addr);
                if let Some(r) = sub_relay.or(relay) {
                    persisted.relay_url = Some(normalize_relay_url(&r)?);
                }
                persisted.save(&home_dir)?;

                let mgr = install_and_start_service(
                    ServiceRole::Daemon,
                    &home_dir,
                    addr.port(),
                    Some(&expected_host_id),
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
                println!("\nPairing Code for Orbit App:");
                print_pairing_summary(
                    &home_dir,
                    Some(addr.port()),
                    Vec::new(),
                    persisted.relay_url,
                )
                .await?;
            }
            Some(DaemonAction::Restart { port }) => {
                let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                let expected_host_id = runtime.host_info.host_id.clone();
                drop(runtime);

                let mut persisted = DaemonPersistedConfig::load(&home_dir);
                let preferred = if let Some(p) = port {
                    SocketAddr::from(([0, 0, 0, 0], p))
                } else {
                    persisted.listen_addr.unwrap_or_else(default_daemon_listen)
                };
                let addr = resolve_isolated_daemon_listen(preferred, &expected_host_id).await?;
                persisted.listen_addr = Some(addr);
                persisted.save(&home_dir)?;

                let mgr = install_and_start_service(
                    ServiceRole::Daemon,
                    &home_dir,
                    addr.port(),
                    Some(&expected_host_id),
                )
                .await?;
                println!("[✓] Orbit Daemon restarted via {mgr} (existing agent shims preserved).");
            }
            Some(DaemonAction::Status) => {
                let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
                let persisted = DaemonPersistedConfig::load(&home_dir);
                let base_addr = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
                let live_port =
                    find_live_daemon_port_for_host(&runtime.host_info.host_id, base_addr.port())
                        .await;
                let healthy = probe_daemon_port(live_port, &runtime.host_info.host_id).await
                    == PortOwnership::Mine;
                println!(
                    "Orbit Daemon : {}",
                    if healthy { "RUNNING" } else { "STOPPED" }
                );
                println!(
                    "Listen Addr  : {}",
                    SocketAddr::new(base_addr.ip(), live_port)
                );
                println!(
                    "Relay URL    : {}",
                    persisted
                        .relay_url
                        .as_deref()
                        .map(redact_relay_url)
                        .unwrap_or_else(|| "(not configured)".to_string())
                );
            }
            Some(DaemonAction::Down) => {
                stop_service(ServiceRole::Daemon, &home_dir)?;
                println!("[✓] Orbit Daemon stopped.");
            }
        },

        Commands::Relay {
            listen,
            allow_pubkeys,
            action,
        } => match action {
            None => {
                let persisted = RelayPersistedConfig::load(&home_dir);
                let effective_listen = listen.unwrap_or(persisted.listen_addr);
                let mut allowed: HashSet<String> = if allow_pubkeys.is_empty() {
                    persisted.allowed_pubkeys.into_iter().collect()
                } else {
                    allow_pubkeys.into_iter().collect()
                };
                allowed.retain(|k| !k.trim().is_empty());
                run_relay(RelayConfig {
                    listen_addr: effective_listen,
                    allowed_pubkeys: Arc::new(allowed),
                })
                .await?;
            }
            Some(RelayAction::Up {
                port,
                listen: sub_listen,
                allow_pubkeys: sub_keys,
                replace_allowlist,
                public_url,
            }) => {
                let mut persisted = RelayPersistedConfig::load(&home_dir);
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

                println!("[✓] Orbit Zero-Knowledge E2EE Relay running in background via {mgr}!");
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
                println!("Allowed Keys : {}", persisted.allowed_pubkeys.len());
                for k in &persisted.allowed_pubkeys {
                    println!("  - {k}");
                }
            }
            Some(RelayAction::Down) => {
                stop_service(ServiceRole::Relay, &home_dir)?;
                println!("[✓] Orbit Relay stopped.");
            }
        },

        Commands::Ls {
            all,
            workspace,
            parent,
            include_background,
        } => {
            let client = connect_cli_server(&server, via_relay).await?;
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
            mode,
            model,
            effort,
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

            let client = connect_cli_server(&server, via_relay).await?;
            let mut sub_rx = client.subscribe();

            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::CreateAgent {
                        request_id: req_id,
                        cwd: target_cwd.to_string_lossy().to_string(),
                        extra_dirs: Vec::new(),
                        title,
                        provider: provider.into(),
                        model,
                        effort: effort.map(Into::into),
                        mode: mode.into(),
                        initial_prompt: prompt,
                        initial_display_prompt: None,
                        initial_badges: Vec::new(),
                        worktree,
                        workspace_id: workspace,
                        parent_agent_id: env_caller,
                        parent_host_id: env_host,
                        profile_id: profile,
                        background,
                        auto_archive,
                        notify_on_finish: effective_notify,
                        idempotency_key: None,
                        base_ref,
                        worktree_mode: WorktreeCreateMode::BranchOff,
                        branch,
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
                stream_turn_to_stdout(&client, &mut sub_rx, &agent.agent_id).await?;
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
        } => {
            let env_caller = std::env::var("ORBIT_AGENT_ID")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let client = connect_cli_server(&server, via_relay).await?;
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
                        active_turn_behavior: ActiveTurnBehavior::from(behavior),
                        message_id: None,
                        caller_agent_id: env_caller,
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
                stream_turn_to_stdout(&client, &mut sub_rx, &agent_id).await?;
            }
        }

        Commands::Wait { agent_id, timeout } => {
            cmd_wait(&server, via_relay, json_mode, &agent_id, timeout).await?;
        }
        Commands::Stop { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
                &server,
                via_relay,
                json_mode,
                WireMessage::InterruptTurn {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                },
                req_id,
                &format!("Stopped `{agent_id}`"),
            )
            .await?;
        }
        Commands::Inspect { agent_id } => {
            cmd_inspect(&server, via_relay, json_mode, &agent_id).await?;
        }
        Commands::Archive { agent_id } => {
            let req_id = Uuid::new_v4();
            cmd_simple_agent_op(
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
            let client = connect_cli_server(&server, via_relay).await?;
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::SubscribeTimeline {
                        request_id: req_id,
                        agent_id,
                        since_seq: 0,
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

        Commands::Workspace { action } => {
            cmd_workspace(&server, via_relay, json_mode, action).await?;
        }
        Commands::Script { action } => {
            cmd_script(&server, via_relay, json_mode, action).await?;
        }
        Commands::Terminal { action } => {
            cmd_terminal(&server, via_relay, json_mode, action).await?;
        }
        Commands::Schedule { action } => {
            cmd_schedule(&server, via_relay, json_mode, action).await?;
        }
        Commands::Heartbeat { action } => {
            cmd_heartbeat(&server, via_relay, json_mode, action).await?;
        }
        Commands::Profile { action } => {
            cmd_profile(&server, via_relay, json_mode, action).await?;
        }
        Commands::Host { action } => {
            cmd_host(&server, via_relay, json_mode, action).await?;
        }
        Commands::Permit { action } => {
            cmd_permit(&server, via_relay, json_mode, action).await?;
        }
        Commands::Config { action } => {
            cmd_config(&server, via_relay, json_mode, action).await?;
        }
        Commands::Skill { action } => {
            cmd_skill(&server, via_relay, json_mode, action).await?;
        }
        Commands::Mcp {
            caller_agent_id,
            url,
        } => {
            mcp_bridge::run_mcp_stdio_bridge(&home_dir, caller_agent_id, url).await?;
        }

        Commands::Shim { socket, spec_json } => {
            let spec: ShimSpawnSpec = serde_json::from_str(&spec_json)?;
            run_shim(&socket, spec).await?;
        }
    }

    Ok(())
}

pub(crate) async fn connect_cli_server(
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
            state: HostConnectionState::Disconnected,
        };
        Ok(connect_smart(&profile, None).await?)
    } else {
        Ok(connect_direct(trimmed, None).await?)
    }
}

async fn print_pairing_summary(
    home_dir: &std::path::Path,
    port_override: Option<u16>,
    explicit_endpoints: Vec<String>,
    explicit_relay: Option<String>,
) -> anyhow::Result<()> {
    let runtime = DaemonRuntime::bootstrap(home_dir).await?;
    let persisted = DaemonPersistedConfig::load(home_dir);
    let fallback_port = persisted
        .listen_addr
        .unwrap_or_else(default_daemon_listen)
        .port();
    let port = match port_override {
        Some(p) => p,
        None => find_live_daemon_port_for_host(&runtime.host_info.host_id, fallback_port).await,
    };
    let endpoints = if explicit_endpoints.is_empty() {
        discover_lan_ws_endpoints(port)
    } else {
        explicit_endpoints
    };
    let relay = match explicit_relay {
        Some(r) => Some(normalize_relay_url(&r)?),
        None => persisted.relay_url,
    };

    let offer = runtime.build_pairing_offer(endpoints.clone(), relay.clone());
    let relay_summary = relay
        .as_deref()
        .map(redact_relay_url)
        .unwrap_or_else(|| "none (LAN only — run `orbit use-relay <HOST:PORT>` for remote)".into());
    eprintln!(
        "Host `{}` ({}) | Direct: {} | Relay: {}",
        runtime.host_info.hostname,
        runtime.host_info.host_id,
        endpoints.join(", "),
        relay_summary
    );
    println!("{}", offer.to_qr_uri()?);
    Ok(())
}

async fn stream_turn_to_stdout(
    client: &orbit_client_core::HostSessionHandle,
    sub_rx: &mut tokio::sync::broadcast::Receiver<ClientInboundEvent>,
    agent_id: &str,
) -> anyhow::Result<()> {
    let mut saw_running = false;
    while let Ok(ev) = sub_rx.recv().await {
        match ev {
            ClientInboundEvent::Wire(WireMessage::TimelineLive { event })
                if event.agent_id == agent_id =>
            {
                match event.payload {
                    TimelinePayload::AssistantMessage {
                        ref text,
                        streaming: false,
                        ..
                    } => {
                        println!("\n{text}");
                    }
                    TimelinePayload::ToolCall { status, detail, .. } => {
                        println!("  [tool:{status:?}] {}", summarize_tool(&detail));
                    }
                    TimelinePayload::PermissionRequest {
                        request_id, title, ..
                    } => {
                        println!("  [permission requested] {title} -> auto-allowing from CLI");
                        let _ = client
                            .send_wire(WireMessage::ResolvePermission {
                                request_id: Uuid::new_v4(),
                                agent_id: agent_id.to_string(),
                                permission_request_id: request_id,
                                decision: "allow".into(),
                            })
                            .await;
                    }
                    TimelinePayload::StatusChanged { status } => match status {
                        AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. } => {
                            saw_running = true;
                        }
                        AgentStatus::Idle if saw_running => {
                            break;
                        }
                        AgentStatus::Error { message, .. } => {
                            eprintln!("\n[Agent Error] {message}");
                            break;
                        }
                        AgentStatus::Closed => break,
                        _ => {}
                    },
                    _ => {}
                }
            }
            ClientInboundEvent::Disconnected => break,
            _ => {}
        }
    }
    Ok(())
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
