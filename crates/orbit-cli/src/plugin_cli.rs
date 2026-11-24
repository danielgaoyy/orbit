use anyhow::{Result, bail};
use clap::Subcommand;
use orbit_client_core::HostSessionHandle;
use orbit_protocol::WireMessage;
use serde_json::Value;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum PluginAction {
    List,
    /// Install from a directory on the target host or a public HTTPS GitHub repository.
    Install {
        source: String,
        #[arg(long)]
        enable: bool,
    },
    Enable {
        plugin: String,
    },
    Disable {
        plugin: String,
    },
    Remove {
        plugin: String,
    },
    /// Invoke one declared operation. Use --cwd to grant access to a registered workspace.
    Invoke {
        plugin: String,
        op: String,
        #[arg(long, default_value = "null")]
        input: Value,
        #[arg(long)]
        cwd: Option<String>,
    },
}

pub async fn run(client: &HostSessionHandle, action: PluginAction, json: bool) -> Result<()> {
    use WireMessage::*;
    let request_id = Uuid::new_v4();
    let req = match action {
        PluginAction::List => ListPlugins { request_id },
        PluginAction::Install { source, enable } => {
            let source = if source.contains("://") {
                source
            } else {
                std::path::absolute(source)?.to_string_lossy().into_owned()
            };
            InstallPlugin {
                request_id,
                source,
                enable,
            }
        }
        PluginAction::Enable { plugin } => SetPluginEnabled {
            request_id,
            plugin_id: plugin,
            enabled: true,
        },
        PluginAction::Disable { plugin } => SetPluginEnabled {
            request_id,
            plugin_id: plugin,
            enabled: false,
        },
        PluginAction::Remove { plugin } => UninstallPlugin {
            request_id,
            plugin_id: plugin,
        },
        PluginAction::Invoke {
            plugin,
            op,
            input,
            cwd,
        } => InvokePluginOp {
            request_id,
            plugin_id: plugin,
            op,
            input,
            cwd: cwd
                .map(std::path::absolute)
                .transpose()?
                .map(|p| p.to_string_lossy().into_owned()),
        },
    };
    let reply = crate::orchestration_cli::send_rpc(client, request_id, req, 30).await?;
    match reply {
        PluginsList { plugins, .. } if !json => {
            if plugins.is_empty() {
                println!("No plugins installed.");
            }
            for p in plugins {
                println!(
                    "{}\t{}\t{}",
                    p.manifest.id,
                    p.manifest.version,
                    if p.enabled { "enabled" } else { "disabled" }
                );
            }
        }
        PluginsList { plugins, .. } => println!("{}", serde_json::to_string_pretty(&plugins)?),
        PluginInstalled { plugin, .. } => println!("{}", serde_json::to_string_pretty(&plugin)?),
        PluginOpResult { result, .. } => println!("{}", serde_json::to_string_pretty(&result)?),
        Ack { .. } => println!("{}", if json { r#"{"ok":true}"# } else { "OK" }),
        _ => bail!("unexpected plugin RPC response"),
    }
    Ok(())
}
