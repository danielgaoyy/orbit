use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand, ValueEnum};
use orbit_client_core::{ClientInboundEvent, connect_direct};
use orbit_daemon::{DaemonConfig, DaemonRuntime, run_daemon};
use orbit_protocol::{
    AgentStatus, ProviderKind, SessionMode, TimelinePayload, ToolCallDetail, WireMessage,
};
use orbit_providers::{discover_installed_providers, probe_all_provider_quotas};
use orbit_relay::{RelayConfig, run_relay};
use orbit_shim::{ShimSpawnSpec, run_shim};
use uuid::Uuid;

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
enum Commands {
    /// Inspect current host environment, identity keypair, and installed AI CLIs (`agy`, `codex`, `claude`).
    Doctor,

    /// Fetch live 5-hour / weekly usage quotas & remaining credits across Antigravity, Codex, and Claude Code.
    Quotas,

    /// Print a Multi-Host QR pairing URI for mobile onboarding.
    Pair {
        #[arg(long = "endpoint", default_value = "ws://127.0.0.1:6868/ws")]
        endpoints: Vec<String>,
        #[arg(long)]
        relay: Option<String>,
    },

    /// Start the host-local Orbit Daemon.
    Daemon {
        #[arg(long, default_value = "127.0.0.1:6868")]
        listen: SocketAddr,
        #[arg(long, env = "ORBIT_RELAY_URL")]
        relay: Option<String>,
    },

    /// Start the `< 5MB` zero-knowledge E2EE Relay server (supports `--allow-pubkey` allowlisting).
    Relay {
        #[arg(long, default_value = "0.0.0.0:7878")]
        listen: SocketAddr,
        /// Restrict relay access strictly to your own Daemon public key(s). Repeatable.
        #[arg(long = "allow-pubkey")]
        allow_pubkeys: Vec<String>,
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
            println!("=== Orbit Host Diagnostics ===");
            println!("Host ID      : {}", runtime.host_info.host_id);
            println!("Hostname     : {}", runtime.host_info.hostname);
            println!(
                "Platform     : {}/{}",
                runtime.host_info.os, runtime.host_info.arch
            );
            println!("Orbit Home   : {}", home_dir.display());
            println!("Public Key   : {}", runtime.host_info.public_key_b64);
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
        Commands::Pair { endpoints, relay } => {
            let runtime = DaemonRuntime::bootstrap(&home_dir).await?;
            let offer = runtime.build_pairing_offer(endpoints, relay);
            println!("{}", offer.to_qr_uri()?);
        }
        Commands::Daemon { listen, relay } => {
            run_daemon(DaemonConfig {
                home_dir,
                listen_addr: listen,
                relay_url: relay,
            })
            .await?;
        }
        Commands::Relay {
            listen,
            allow_pubkeys,
        } => {
            let allowed: HashSet<String> = allow_pubkeys.into_iter().collect();
            run_relay(RelayConfig {
                listen_addr: listen,
                allowed_pubkeys: Arc::new(allowed),
            })
            .await?;
        }
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
