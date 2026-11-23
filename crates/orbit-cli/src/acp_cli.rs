//! CLI subcommands for Universal ACP Agent Registry management & live handshake testing:
//! `orbit acp list | add | remove | test <id> | refresh`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Subcommand;
use orbit_protocol::{
    AcpAgentAvailability, AcpAgentSource, AcpAgentSpec, AcpProbeResult, WireMessage,
};
use uuid::Uuid;

use crate::connect_cli_server;

#[derive(Subcommand)]
pub enum AcpAction {
    /// List curated, remote-synced, and custom ACP agents with live host availability.
    List {
        /// Show only agents whose binary is installed on PATH.
        #[arg(long)]
        installed: bool,
        /// Show only user-defined custom ACP agents.
        #[arg(long)]
        custom: bool,
    },
    /// Add or update a custom stdio ACP agent in `$ORBIT_HOME/acp_registry.json`.
    Add {
        /// Stable slug identifier (`[a-z0-9][a-z0-9._-]{0,63}`, e.g. `my-coder`).
        id: String,
        /// Human-friendly display name (defaults to `id`).
        #[arg(long)]
        name: Option<String>,
        /// Executable name or path (e.g. `opencode` or `/usr/local/bin/my-acp`).
        #[arg(long, short = 'c')]
        command: String,
        /// Command-line argument passed to the executable (repeatable).
        #[arg(long = "arg", short = 'a', allow_hyphen_values = true)]
        args: Vec<String>,
        /// Environment variable override in `KEY=VALUE` format (repeatable).
        #[arg(long = "env", short = 'e')]
        env: Vec<String>,
        /// Short description of this ACP agent.
        #[arg(long, default_value = "")]
        description: String,
        /// Optional install command hint.
        #[arg(long)]
        install_hint: Option<String>,
        /// Optional homepage URL.
        #[arg(long)]
        homepage: Option<String>,
    },
    /// Remove a custom ACP agent by ID.
    Remove {
        /// Custom ACP agent ID to remove.
        id: String,
    },
    /// Spawn an ACP agent briefly and run a live JSON-RPC 2.0 `initialize` handshake probe.
    Test {
        /// ACP agent ID from catalog or custom registry.
        id: String,
        /// Optional working directory for the handshake probe.
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Refresh the cached remote ACP registry overlay (with graceful offline fallback).
    Refresh {
        /// Optional HTTPS registry JSON URL override.
        #[arg(long)]
        url: Option<String>,
    },
}

fn parse_env_pairs(raw_pairs: &[String]) -> anyhow::Result<HashMap<String, String>> {
    let mut map = HashMap::new();
    for item in raw_pairs {
        let Some((k, v)) = item.split_once('=') else {
            anyhow::bail!("invalid --env entry `{item}` (expected `KEY=VALUE`)");
        };
        let key = k.trim();
        if key.is_empty() {
            anyhow::bail!("empty env key in `{item}`");
        }
        map.insert(key.to_string(), v.to_string());
    }
    Ok(map)
}

async fn try_connect_daemon(
    home_dir: &Path,
    explicit_server: Option<&str>,
    resolved_server: &str,
    via_relay: bool,
) -> anyhow::Result<Option<orbit_client_core::HostSessionHandle>> {
    // Only talk to the implicit local daemon when this ORBIT_HOME actually owns a host identity.
    if explicit_server.is_none() && !home_dir.join("identity.key").exists() {
        return Ok(None);
    }
    match connect_cli_server(home_dir, resolved_server, via_relay).await {
        Ok(client) => Ok(Some(client)),
        Err(err) => {
            if explicit_server.is_some() {
                Err(err)
            } else {
                Ok(None)
            }
        }
    }
}

async fn request_or_fallback_local<F>(
    client: &orbit_client_core::HostSessionHandle,
    explicit_server: Option<&str>,
    msg: WireMessage,
    timeout: Duration,
    matcher: F,
) -> anyhow::Result<Option<WireMessage>>
where
    F: Fn(&WireMessage) -> bool + Send + Sync + 'static,
{
    match client.request(msg, timeout, matcher).await {
        Ok(resp) => Ok(Some(resp)),
        Err(err) => {
            if explicit_server.is_some() {
                Err(err.into())
            } else {
                // Local daemon may be an older version that does not implement ACP registry RPCs yet.
                Ok(None)
            }
        }
    }
}

fn print_acp_list(
    agents: &[AcpAgentAvailability],
    installed_only: bool,
    custom_only: bool,
    json_mode: bool,
) -> anyhow::Result<()> {
    let filtered: Vec<&AcpAgentAvailability> = agents
        .iter()
        .filter(|a| !installed_only || a.installed)
        .filter(|a| !custom_only || a.spec.source == AcpAgentSource::Custom)
        .collect();

    if json_mode {
        println!("{}", serde_json::to_string_pretty(&filtered)?);
        return Ok(());
    }

    if filtered.is_empty() {
        println!("No matching ACP agents found.");
        return Ok(());
    }

    let ready_count = filtered.iter().filter(|a| a.installed).count();
    println!("ACP Agents ({ready_count}/{} ready):", filtered.len());
    for item in filtered {
        let status = if item.installed { "ready" } else { "missing" };
        let source = match item.spec.source {
            AcpAgentSource::BuiltinCatalog => "catalog",
            AcpAgentSource::RemoteRegistry => "remote",
            AcpAgentSource::Custom => "custom",
        };
        let cmd_str = if item.spec.args.is_empty() {
            item.spec.command.clone()
        } else {
            format!("{} {}", item.spec.command, item.spec.args.join(" "))
        };
        println!(
            "  [{status:<7}] {:<18} {:<20} ({source:<7})  {cmd_str}",
            item.spec.id, item.spec.name
        );
    }
    Ok(())
}

fn print_probe_result(result: &AcpProbeResult, json_mode: bool) -> anyhow::Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string_pretty(result)?);
    } else if result.ok {
        let server = match (&result.server_name, &result.server_version) {
            (Some(n), Some(v)) => format!("{n} {v}"),
            (Some(n), None) => n.clone(),
            _ => result.agent_id.clone(),
        };
        let proto = result.protocol_version.as_deref().unwrap_or("?");
        println!(
            "[✓] ACP handshake ok: {server} (protocol v{proto}, {}ms)",
            result.latency_ms
        );
        if !result.capabilities_summary.is_empty() {
            println!(
                "    Capabilities: {}",
                result.capabilities_summary.join(", ")
            );
        }
        if !result.auth_methods.is_empty() {
            println!("    Auth methods: {}", result.auth_methods.join(", "));
        }
    } else {
        let err = result.error.as_deref().unwrap_or("unknown error");
        println!(
            "[✗] ACP handshake failed for `{}` ({}ms): {err}",
            result.agent_id, result.latency_ms
        );
    }
    if !result.ok {
        anyhow::bail!(
            "{}",
            result
                .error
                .clone()
                .unwrap_or_else(|| "ACP handshake failed".to_string())
        );
    }
    Ok(())
}

pub async fn cmd_acp(
    home_dir: &Path,
    explicit_server: Option<&str>,
    resolved_server: &str,
    via_relay: bool,
    json_mode: bool,
    action: AcpAction,
) -> anyhow::Result<()> {
    let maybe_client =
        try_connect_daemon(home_dir, explicit_server, resolved_server, via_relay).await?;
    let quick_rpc_timeout = if explicit_server.is_some() {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(3)
    };

    match action {
        AcpAction::List { installed, custom } => {
            let rpc_resp = if let Some(client) = maybe_client.as_ref() {
                let req_id = Uuid::new_v4();
                request_or_fallback_local(
                    client,
                    explicit_server,
                    WireMessage::ListAcpAgents { request_id: req_id },
                    quick_rpc_timeout,
                    move |w| {
                        matches!(
                            w,
                            WireMessage::AcpAgentsSnapshot { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?
            } else {
                None
            };
            let agents = match rpc_resp {
                Some(WireMessage::AcpAgentsSnapshot { agents, .. }) => agents,
                Some(WireMessage::RpcError { code, message, .. }) => {
                    anyhow::bail!("[{code}] {message}")
                }
                Some(_) => anyhow::bail!("unexpected response to ListAcpAgents"),
                None => orbit_providers::list_acp_agents(home_dir),
            };
            print_acp_list(&agents, installed, custom, json_mode)?;
        }

        AcpAction::Add {
            id,
            name,
            command,
            args,
            env,
            description,
            install_hint,
            homepage,
        } => {
            let env_map = parse_env_pairs(&env)?;
            let spec = AcpAgentSpec {
                id: id.clone(),
                name: name.unwrap_or_else(|| id.clone()),
                description,
                command,
                args,
                env: env_map,
                install_hint,
                homepage,
                version: None,
                source: AcpAgentSource::Custom,
                enabled: true,
            };
            orbit_providers::validate_acp_agent_spec(&spec).map_err(|e| anyhow::anyhow!("{e}"))?;

            let rpc_resp = if let Some(client) = maybe_client.as_ref() {
                let req_id = Uuid::new_v4();
                request_or_fallback_local(
                    client,
                    explicit_server,
                    WireMessage::UpsertCustomAcpAgent {
                        request_id: req_id,
                        spec: spec.clone(),
                    },
                    quick_rpc_timeout,
                    move |w| {
                        matches!(
                            w,
                            WireMessage::AcpAgentsSnapshot { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?
            } else {
                None
            };
            let agents = match rpc_resp {
                Some(WireMessage::AcpAgentsSnapshot { agents, .. }) => agents,
                Some(WireMessage::RpcError { code, message, .. }) => {
                    anyhow::bail!("[{code}] {message}")
                }
                Some(_) => anyhow::bail!("unexpected response to UpsertCustomAcpAgent"),
                None => {
                    orbit_providers::upsert_custom_acp_agent(home_dir, spec.clone())
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                    orbit_providers::list_acp_agents(home_dir)
                }
            };

            let saved = agents.iter().find(|a| a.spec.id == spec.id);
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&saved)?);
            } else {
                let status = if saved.is_some_and(|s| s.installed) {
                    "ready"
                } else {
                    "binary not found on PATH"
                };
                println!("[✓] Saved custom ACP agent `{}` ({status})", spec.id);
            }
        }

        AcpAction::Remove { id } => {
            let rpc_resp = if let Some(client) = maybe_client.as_ref() {
                let req_id = Uuid::new_v4();
                request_or_fallback_local(
                    client,
                    explicit_server,
                    WireMessage::DeleteCustomAcpAgent {
                        request_id: req_id,
                        agent_id: id.clone(),
                    },
                    quick_rpc_timeout,
                    move |w| {
                        matches!(
                            w,
                            WireMessage::AcpAgentsSnapshot { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?
            } else {
                None
            };
            let removed = match rpc_resp {
                Some(WireMessage::AcpAgentsSnapshot { .. }) => true,
                Some(WireMessage::RpcError { code, .. }) if code == "not_found" => false,
                Some(WireMessage::RpcError { code, message, .. }) => {
                    anyhow::bail!("[{code}] {message}")
                }
                Some(_) => anyhow::bail!("unexpected response to DeleteCustomAcpAgent"),
                None => orbit_providers::delete_custom_acp_agent(home_dir, &id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "ok": removed,
                        "removed": if removed { Some(&id) } else { None },
                        "not_found": !removed,
                    }))?
                );
            } else if removed {
                println!("[✓] Removed custom ACP agent `{id}`");
            } else {
                println!("Custom ACP agent `{id}` not found.");
            }
            if !removed {
                anyhow::bail!("custom ACP agent `{id}` not found");
            }
        }

        AcpAction::Test { id, cwd } => {
            let rpc_resp = if let Some(client) = maybe_client.as_ref() {
                let req_id = Uuid::new_v4();
                request_or_fallback_local(
                    client,
                    explicit_server,
                    WireMessage::TestAcpAgent {
                        request_id: req_id,
                        agent_id: id.clone(),
                        cwd: cwd.as_ref().map(|p| p.to_string_lossy().to_string()),
                    },
                    Duration::from_secs(if explicit_server.is_some() { 20 } else { 12 }),
                    move |w| {
                        matches!(
                            w,
                            WireMessage::AcpAgentProbeResult { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?
            } else {
                None
            };
            let result = match rpc_resp {
                Some(WireMessage::AcpAgentProbeResult { result, .. }) => result,
                Some(WireMessage::RpcError { code, message, .. }) => {
                    anyhow::bail!("[{code}] {message}")
                }
                Some(_) => anyhow::bail!("unexpected response to TestAcpAgent"),
                None => {
                    let spec = orbit_providers::get_acp_agent_spec(home_dir, &id)
                        .ok_or_else(|| anyhow::anyhow!("ACP agent `{id}` not found in registry"))?;
                    orbit_providers::probe_acp_agent(&spec, cwd.as_deref()).await
                }
            };
            print_probe_result(&result, json_mode)?;
        }

        AcpAction::Refresh { url } => {
            let rpc_resp = if let Some(client) = maybe_client.as_ref() {
                let req_id = Uuid::new_v4();
                request_or_fallback_local(
                    client,
                    explicit_server,
                    WireMessage::RefreshAcpRegistry {
                        request_id: req_id,
                        registry_url: url.clone(),
                    },
                    Duration::from_secs(if explicit_server.is_some() { 25 } else { 12 }),
                    move |w| {
                        matches!(
                            w,
                            WireMessage::AcpAgentsSnapshot { request_id: r, .. }
                                | WireMessage::RpcError { request_id: Some(r), .. }
                                if *r == req_id
                        )
                    },
                )
                .await?
            } else {
                None
            };
            let (agents, warning) = match rpc_resp {
                Some(WireMessage::AcpAgentsSnapshot {
                    agents, warning, ..
                }) => (agents, warning),
                Some(WireMessage::RpcError { code, message, .. }) => {
                    anyhow::bail!("[{code}] {message}")
                }
                Some(_) => anyhow::bail!("unexpected response to RefreshAcpRegistry"),
                None => orbit_providers::refresh_acp_registry(home_dir, url.as_deref())
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
            };
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "total": agents.len(),
                        "installed": agents.iter().filter(|a| a.installed).count(),
                        "warning": warning,
                        "agents": agents,
                    }))?
                );
            } else {
                let installed = agents.iter().filter(|a| a.installed).count();
                println!(
                    "[✓] ACP registry refreshed ({installed}/{} ready)",
                    agents.len()
                );
                if let Some(w) = warning {
                    println!("    Note: {w}");
                }
            }
        }
    }

    Ok(())
}
