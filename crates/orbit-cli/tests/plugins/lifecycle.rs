//! Verify the installed CLI and the authenticated WS and HTTP entry points together.
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{OrbitPluginUiSlot, WireMessage};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use uuid::Uuid;

async fn cli(home: &Path, server: &str, args: &[&str]) -> Result<Value> {
    let out = tokio::process::Command::new(env!("CARGO_BIN_EXE_orbit"))
        .arg("--home")
        .arg(home)
        .args(["--server", server, "--json", "plugin"])
        .args(args)
        .kill_on_drop(true)
        .output()
        .await?;
    ensure!(
        out.status.success(),
        "plugin {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(serde_json::from_slice(&out.stdout)?)
}

async fn mcp(url: &str, token: &str, method: &str, params: Value) -> Result<Value> {
    Ok(reqwest::Client::new()
        .post(url)
        .bearer_auth(token)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?["result"]
        .clone())
}

async fn catalog(url: &str, token: &str) -> Result<Vec<Value>> {
    mcp(url, token, "tools/list", json!({})).await?["tools"]
        .as_array()
        .cloned()
        .context("missing MCP catalog")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_cli_wire_and_conditional_mcp_catalog() -> Result<()> {
    let home = tempfile::tempdir()?;
    let workspace = tempfile::tempdir()?;
    let cwd = workspace.path().canonicalize()?.display().to_string();
    std::fs::write(workspace.path().join("README.md"), "Hello Orbit")?;
    let (addr, rt) = spawn_embedded_daemon(home.path().to_owned(), None).await?;
    let ws = format!("ws://{addr}/ws");
    let http = format!("http://{addr}/mcp/agents?caller_agent_id=ag_plugin");
    let token = &rt.mcp_token;
    rt.store.upsert_workspace(&serde_json::from_value(json!({
        "workspace_id":"ws_plugin","title":"Plugin workspace","cwd":cwd,"is_worktree":false,"updated_at":Utc::now()
    }))?)?;
    rt.store.upsert_agent(&serde_json::from_value(json!({
        "agent_id":"ag_plugin","workspace_id":"ws_plugin","title":"Plugin caller","cwd":cwd,
        "provider":"codex","mode":"default","status":{"state":"idle"},"latest_seq":0,"created_at":Utc::now(),"updated_at":Utc::now()
    }))?, None)?;
    let initial = catalog(&http, token).await?;
    assert!(initial.iter().all(|t| t["name"] != "orbit_plugin"));
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins");
    assert_eq!(cli(home.path(), &ws, &["list"]).await?, json!([]));
    for id in ["project-health", "pr-dashboard"] {
        cli(
            home.path(),
            &ws,
            &["install", &examples.join(id).display().to_string()],
        )
        .await?;
    }
    assert_eq!(catalog(&http, token).await?, initial);
    for id in ["project-health", "pr-dashboard"] {
        cli(home.path(), &ws, &["enable", id]).await?;
        let defs = catalog(&http, token).await?;
        assert_eq!(
            defs.iter().filter(|t| t["name"] == "orbit_plugin").count(),
            1
        );
        assert_eq!(defs.len(), initial.len() + 1);
    }
    let ops = mcp(
        &http,
        token,
        "tools/call",
        json!({"name":"orbit_plugin","arguments":{"action":"list"}}),
    )
    .await?;
    assert_eq!(
        ops["structuredContent"]["plugins"]
            .as_array()
            .context("expected catalog")?
            .len(),
        2
    );
    let result = cli(
        home.path(),
        &ws,
        &[
            "invoke",
            "project-health",
            "check_health",
            "--cwd",
            &cwd,
            "--input",
            "{}",
        ],
    )
    .await?;
    assert_eq!(result["checks"][2]["nonempty"], true);
    let result_mcp = mcp(
        &http,
        token,
        "tools/call",
        json!({"name":"orbit_plugin","arguments":{
            "action":"call","plugin":"project-health","op":"check_health","input":{}
        }}),
    )
    .await?;
    assert_eq!(result_mcp["structuredContent"], result);
    let client = orbit_client_core::connect_direct(&ws, &rt.pairing_secret, None).await?;
    for (id, slot, cwd) in [
        ("project-health", OrbitPluginUiSlot::StageCard, Some(cwd)),
        ("pr-dashboard", OrbitPluginUiSlot::SidePanelTab, None),
    ] {
        let req = Uuid::new_v4();
        let msg = client
            .request(
                WireMessage::RenderPluginUi {
                    request_id: req,
                    plugin_id: id.into(),
                    slot,
                    cwd,
                    action_id: None,
                    payload: None,
                },
                Duration::from_secs(10),
                |m| {
                    matches!(m, WireMessage::PluginUiRendered{request_id,..} if *request_id==req)
                        || matches!(m,WireMessage::RpcError{request_id:Some(r),..} if *r==req)
                },
            )
            .await?;
        let WireMessage::PluginUiRendered { spec, .. } = msg else {
            anyhow::bail!("render {id}: {msg:?}")
        };
        spec.validate().map_err(anyhow::Error::msg)?;
    }
    cli(home.path(), &ws, &["disable", "project-health"]).await?;
    let denied = mcp(
        &http,
        token,
        "tools/call",
        json!({"name":"orbit_plugin","arguments":{
            "action":"call","plugin":"project-health","op":"check_health"
        }}),
    )
    .await?;
    assert_eq!(denied["isError"], true);
    cli(home.path(), &ws, &["remove", "pr-dashboard"]).await?;
    assert_eq!(catalog(&http, token).await?, initial);
    cli(home.path(), &ws, &["remove", "project-health"]).await?;
    assert_eq!(cli(home.path(), &ws, &["list"]).await?, json!([]));
    Ok(())
}
