mod service;

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use orbit_client_core::{ClientInboundEvent, connect_direct};
use orbit_daemon::{
    DaemonConfig, DaemonPersistedConfig, DaemonRuntime, discover_lan_ws_endpoints, run_daemon,
};
use orbit_protocol::{
    AgentStatus, ProviderKind, SessionMode, TimelinePayload, ToolCallDetail, WireMessage,
};
use orbit_providers::{discover_installed_providers, probe_all_provider_quotas};
use orbit_relay::{RelayConfig, RelayPersistedConfig, run_relay};
use orbit_shim::{ShimSpawnSpec, run_shim};
use uuid::Uuid;

use crate::service::{
    RelayProbeOutcome, ServiceRole, check_local_health, default_daemon_listen, detect_public_ipv4,
    install_and_start_service, normalize_relay_url, preflight_probe_relay, stop_service,
};

#[derive(Parser)]
#[command(
    name = "orbit",
    version,
    about = "Ultra-fast Multi-Host AI Coding Agent Orchestrator (Antigravity, Codex, Claude Code)"
)]
struct Cli {
    /// Override Orbit state directory (defaults to `~/.orbit`).
    #[arg(long, env = "ORBIT_HOME")]
    home: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliProvider {
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
enum CliMode {
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
    Restart,
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

    /// Attach (or detach with `--clear`) this machine to your E2EE Relay, verify connectivity, restart daemon, and print a fresh Pairing Code.
    UseRelay {
        /// Relay address (e.g. `203.0.113.10:7878`, `ws://203.0.113.10:7878`, or `wss://relay.example.com`).
        relay_url: Option<String>,
        /// Detach Relay and revert this host to LAN/Tailscale direct-only mode.
        #[arg(long)]
        clear: bool,
    },

    /// Print a Multi-Host QR pairing URI (auto-detects LAN/Tailscale IPs & persisted Relay URL).
    Pair {
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
        #[arg(long, default_value = "ws://127.0.0.1:6868/ws", env = "ORBIT_SERVER")]
        server: String,
    },

    /// Create and stream a live coding agent session (`agy`, `codex`, or `claude`).
    Run {
        #[arg(short, long, value_enum, default_value = "antigravity")]
        provider: CliProvider,
        #[arg(short, long, value_enum, default_value = "accept-edits")]
        mode: CliMode,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long, default_value = "ws://127.0.0.1:6868/ws", env = "ORBIT_SERVER")]
        server: String,
        /// Initial prompt to send to the agent.
        prompt: String,
    },

    /// Send a follow-up prompt to an existing (or persisted) agent and stream the turn.
    Send {
        agent_id: String,
        prompt: String,
        #[arg(long, default_value = "ws://127.0.0.1:6868/ws", env = "ORBIT_SERVER")]
        server: String,
    },

    /// Read timeline history of an agent from an Orbit Daemon.
    Logs {
        agent_id: String,
        #[arg(long, default_value = "ws://127.0.0.1:6868/ws", env = "ORBIT_SERVER")]
        server: String,
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let home_dir = resolve_home(cli.home);

    match cli.command {
        Commands::Doctor => {
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let persisted = DaemonPersistedConfig::load(&home_dir);
            let listen = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
            let daemon_live = check_local_health(listen.port()).await;

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
                listen.port()
            );
            match &persisted.relay_url {
                Some(r) => println!("Relay Route  : {r} (Noise NK E2EE)"),
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
            println!("\nQuick Relay Setup (on your cloud VM / devbox):");
            println!(
                "  orbit relay up --allow-pubkey {}",
                runtime.host_info.public_key_b64
            );
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

        Commands::UseRelay { relay_url, clear } => {
            let mut persisted = DaemonPersistedConfig::load(&home_dir);
            let listen = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
            persisted.listen_addr = Some(listen);

            if clear {
                persisted.relay_url = None;
                persisted.save(&home_dir)?;
                let mgr = install_and_start_service(ServiceRole::Daemon, &home_dir, listen.port())
                    .await?;
                println!("[✓] Detached Relay configuration and restarted daemon via {mgr}.");
                print_pairing_summary(&home_dir, Vec::new(), None).await?;
                return Ok(());
            }

            let raw = relay_url.ok_or_else(|| {
                anyhow::anyhow!(
                    "Usage: `orbit use-relay <HOST:PORT>` (or `orbit use-relay --clear` to detach)"
                )
            })?;
            let normalized = normalize_relay_url(&raw)?;
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let pubkey = runtime.keypair.public_key_b64.clone();
            drop(runtime);

            println!("Probing Relay {normalized} with host pubkey `{pubkey}`...");
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
                    println!("[!] Could not reach {normalized} right now: {reason}");
                    println!(
                        "    Ensure `orbit relay up` is running on the server and its TCP port is open in your firewall / Security Group."
                    );
                    println!(
                        "    (Saving Relay config now — local daemon will auto-connect in background as soon as reachable.)"
                    );
                }
            }

            persisted.relay_url = Some(normalized.clone());
            persisted.save(&home_dir)?;

            let mgr =
                install_and_start_service(ServiceRole::Daemon, &home_dir, listen.port()).await?;
            println!("[✓] Orbit Daemon active via {mgr} -> attached to {normalized}");
            println!(
                "\nCopy & import this updated Pairing Code in Orbit App (Mac / iOS / Android):"
            );
            print_pairing_summary(&home_dir, Vec::new(), Some(normalized)).await?;
        }

        Commands::Pair { endpoints, relay } => {
            print_pairing_summary(&home_dir, endpoints, relay).await?;
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
                let mut persisted = DaemonPersistedConfig::load(&home_dir);
                let addr = if let Some(l) = sub_listen.or(listen) {
                    l
                } else if let Some(p) = port {
                    SocketAddr::from(([0, 0, 0, 0], p))
                } else {
                    persisted.listen_addr.unwrap_or_else(default_daemon_listen)
                };
                persisted.listen_addr = Some(addr);
                if let Some(r) = sub_relay.or(relay) {
                    persisted.relay_url = Some(normalize_relay_url(&r)?);
                }
                persisted.save(&home_dir)?;

                let mgr =
                    install_and_start_service(ServiceRole::Daemon, &home_dir, addr.port()).await?;
                println!("[✓] Orbit Daemon running in background via {mgr}");
                println!("    Listen : {addr}");
                println!(
                    "    Relay  : {}",
                    persisted
                        .relay_url
                        .as_deref()
                        .unwrap_or("(none — LAN/Tailscale direct)")
                );
                println!("\nPairing Code for Orbit App:");
                print_pairing_summary(&home_dir, Vec::new(), persisted.relay_url).await?;
            }
            Some(DaemonAction::Restart) => {
                let persisted = DaemonPersistedConfig::load(&home_dir);
                let addr = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
                let mgr =
                    install_and_start_service(ServiceRole::Daemon, &home_dir, addr.port()).await?;
                println!("[✓] Orbit Daemon restarted via {mgr} (existing agent shims preserved).");
            }
            Some(DaemonAction::Status) => {
                let persisted = DaemonPersistedConfig::load(&home_dir);
                let addr = persisted.listen_addr.unwrap_or_else(default_daemon_listen);
                let healthy = check_local_health(addr.port()).await;
                println!(
                    "Orbit Daemon : {}",
                    if healthy { "RUNNING" } else { "STOPPED" }
                );
                println!("Listen Addr  : {addr}");
                println!(
                    "Relay URL    : {}",
                    persisted.relay_url.as_deref().unwrap_or("(not configured)")
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
                    install_and_start_service(ServiceRole::Relay, &home_dir, bound_port).await?;

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

        Commands::Ls { server } => {
            let client = connect_direct(&server, None).await?;
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::ListAgents {
                        request_id: req_id,
                        workspace_id: None,
                    },
                    Duration::from_secs(10),
                    |w| matches!(w, WireMessage::AgentsSnapshot { request_id: r, .. } if *r == req_id),
                )
                .await?;
            if let WireMessage::AgentsSnapshot { agents, .. } = resp {
                if agents.is_empty() {
                    println!("No agents registered on {server}.");
                } else {
                    for a in agents {
                        println!(
                            "{:<36} | {:<12} | {:<16} | {}",
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
            cwd,
            server,
            prompt,
        } => {
            let target_cwd = cwd
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("."));
            let client = connect_direct(&server, None).await?;
            let mut sub_rx = client.subscribe();

            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::CreateAgent {
                        request_id: req_id,
                        cwd: target_cwd.to_string_lossy().to_string(),
                        title: None,
                        provider: provider.into(),
                        model,
                        mode: mode.into(),
                        initial_prompt: prompt,
                    },
                    Duration::from_secs(35),
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

            let agent_id = match resp {
                WireMessage::AgentCreated { agent, .. } => {
                    println!(
                        "Started {} session `{}` in {}",
                        agent.provider.display_name(),
                        agent.agent_id,
                        agent.cwd
                    );
                    agent.agent_id
                }
                WireMessage::RpcError { code, message, .. } => {
                    anyhow::bail!("[{code}] {message}");
                }
                _ => anyhow::bail!("unexpected response"),
            };

            stream_turn_to_stdout(&client, &mut sub_rx, &agent_id).await?;
        }

        Commands::Send {
            agent_id,
            prompt,
            server,
        } => {
            let client = connect_direct(&server, None).await?;
            let mut sub_rx = client.subscribe();
            let req_id = Uuid::new_v4();
            let resp = client
                .request(
                    WireMessage::SendPrompt {
                        request_id: req_id,
                        agent_id: agent_id.clone(),
                        prompt,
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
            stream_turn_to_stdout(&client, &mut sub_rx, &agent_id).await?;
        }

        Commands::Logs { agent_id, server } => {
            let client = connect_direct(&server, None).await?;
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
                for ev in events {
                    print_timeline_payload(ev.seq, &ev.payload);
                }
            }
        }

        Commands::Shim { socket, spec_json } => {
            let spec: ShimSpawnSpec = serde_json::from_str(&spec_json)?;
            run_shim(&socket, spec).await?;
        }
    }

    Ok(())
}

async fn print_pairing_summary(
    home_dir: &std::path::Path,
    explicit_endpoints: Vec<String>,
    explicit_relay: Option<String>,
) -> anyhow::Result<()> {
    let persisted = DaemonPersistedConfig::load(home_dir);
    let port = persisted
        .listen_addr
        .unwrap_or_else(default_daemon_listen)
        .port();
    let endpoints = if explicit_endpoints.is_empty() {
        discover_lan_ws_endpoints(port)
    } else {
        explicit_endpoints
    };
    let relay = match explicit_relay {
        Some(r) => Some(normalize_relay_url(&r)?),
        None => persisted.relay_url,
    };

    let runtime = DaemonRuntime::bootstrap(home_dir).await?;
    let offer = runtime.build_pairing_offer(endpoints.clone(), relay.clone());
    eprintln!(
        "Host `{}` ({}) | Direct: {} | Relay: {}",
        runtime.host_info.hostname,
        runtime.host_info.host_id,
        endpoints.join(", "),
        relay
            .as_deref()
            .unwrap_or("none (LAN only — run `orbit use-relay <HOST:PORT>` for remote)")
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

fn print_timeline_payload(seq: u64, payload: &TimelinePayload) {
    match payload {
        TimelinePayload::UserPrompt { text, .. } => println!("#{seq:04} [USER] {text}"),
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

fn format_status(s: &AgentStatus) -> &'static str {
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
