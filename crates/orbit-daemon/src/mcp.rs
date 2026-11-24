//! Built-in Streamable HTTP MCP Server (`POST /mcp/agents`, MCP 2025-03-26 JSON-RPC 2.0).
//!
//! Zero external MCP SDK runtime dependencies — runs directly inside `orbit-daemon`'s Axum server,
//! authenticated with `$ORBIT_HOME/mcp-token` (`0600`) and caller-scoped via `?caller_agent_id=...`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::DaemonRuntime;
use crate::tools::{
    McpCallerAuthority, execute_tool, list_tool_definitions_for_caller, resolve_caller_authority,
    resolve_caller_policy,
};

const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// Load or create the 256-bit local MCP Bearer token (`$ORBIT_HOME/mcp-token`, mode `0600`).
pub fn load_or_create_mcp_token(home_dir: &Path) -> anyhow::Result<String> {
    let token_path = home_dir.join("mcp-token");
    if token_path.is_file()
        && let Ok(existing) = std::fs::read_to_string(&token_path)
    {
        let trimmed = existing.trim().to_string();
        if trimmed.len() >= 32 {
            return Ok(trimmed);
        }
    }
    std::fs::create_dir_all(home_dir)?;
    let generated = format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
    std::fs::write(&token_path, &generated)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(generated)
}

/// Axum handler for `POST /mcp/agents`.
pub async fn mcp_agents_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    // 1. Enforce anti-CSWSH / CSRF Origin validation
    if let Err(status) = crate::validate_request_origin(&headers) {
        return (
            status,
            Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32003, "message": "Forbidden Origin" }
            })),
        )
            .into_response();
    }

    // 2. Validate Bearer token in constant time
    if !crate::verify_bearer_token(&headers, &runtime.mcp_token) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "jsonrpc": "2.0",
                "id": body.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32001, "message": "Unauthorized Orbit MCP token" }
            })),
        )
            .into_response();
    }

    let caller_agent_id = query
        .get("caller_agent_id")
        .or_else(|| query.get("callerAgentId"))
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty());

    // Handle JSON-RPC batch or single request
    if let Some(batch) = body.as_array() {
        let mut responses = Vec::new();
        for item in batch {
            if let Some(resp) = handle_single_rpc(&runtime, caller_agent_id, item).await {
                responses.push(resp);
            }
        }
        if responses.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return (StatusCode::OK, Json(Value::Array(responses))).into_response();
    }

    match handle_single_rpc(&runtime, caller_agent_id, &body).await {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

async fn handle_single_rpc(
    runtime: &Arc<DaemonRuntime>,
    caller_agent_id: Option<&str>,
    req: &Value,
) -> Option<Value> {
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");

    // Notifications (no `id`) receive HTTP 202 Accepted with no body
    if id.is_none() || method.starts_with("notifications/") {
        return None;
    }
    let id_val = id.unwrap_or(Value::Null);

    match method {
        "initialize" => {
            let client_ver = req
                .pointer("/params/protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(MCP_PROTOCOL_VERSION);
            let instructions = match resolve_caller_authority(runtime, caller_agent_id) {
                McpCallerAuthority::PersonalRouter => {
                    "Orbit built-in multi-agent & Mission Workspace orchestration server. Use `create_agent` to launch a child/remote Worker conversation (`worktree=true` supported; does not promote standalone workspaces into a Group), `spawn_workspace_peer` to add a top-level sibling peer inside a Mission Workspace Group (`worktree=true` for parallel edits; visible inside the sidebar Group), and `orbit_workspace`, `orbit_agent` (`fork`, `move_workspace`, `list`, `kill`), `send_agent_prompt`, `orbit_control` (`list_projects`, `inspect_project_context`, `list_profiles`, `list_hosts`, approvals) to orchestrate across the fleet. Inside a Mission Workspace Group, coordinate peers using `workspace_peers`, `message_peer` (`@handle` or `all`), `workspace_board` (`read`/`update` with optional `expected_revision` CAS lock), `handoff_worktree`, and `orbit_timer` (`all_peers`, `sender:<id|@handle>`). Never go idle silently when waiting on a peer or child — rely on reactive finish notifications or arm `orbit_timer`."
                }
                McpCallerAuthority::Worker => {
                    "Orbit session helper server for a coding Agent Harness. Only the Personal Router can spawn, fork, move, or organize Orbit conversations via MCP — this session does NOT have session-spawning or workspace-management tools. For subagent delegation, ALWAYS use your own harness-native subagent tool (e.g. Claude Agent/Task, Codex native subagent, Antigravity invoke_subagent) rather than opening new Orbit sessions. When collaborating inside a Mission Workspace Group (`<orbit-workspace>`), coordinate with existing peers using `workspace_peers`, `message_peer`, and `workspace_board`, and use `handoff_worktree` / `orbit_timer` for your own worktree handoff and wakeup timers."
                }
            };
            Some(json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "protocolVersion": client_ver,
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "orbit-orchestrator",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "instructions": instructions
                }
            }))
        }
        "ping" => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": {}
        })),
        "tools/list" => {
            let policy = resolve_caller_policy(runtime, caller_agent_id);
            let tools: Vec<Value> =
                list_tool_definitions_for_caller(runtime, caller_agent_id, &policy)
                    .into_iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": t.input_schema
                        })
                    })
                    .collect();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": { "tools": tools }
            }))
        }
        "tools/call" => {
            let tool_name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let exec = execute_tool(runtime, caller_agent_id, tool_name, args).await;
            Some(json!({
                "jsonrpc": "2.0",
                "id": id_val,
                "result": {
                    "content": [{ "type": "text", "text": exec.text }],
                    "structuredContent": exec.structured,
                    "isError": exec.is_error
                }
            }))
        }
        other => Some(json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {other}")
            }
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use orbit_protocol::{AgentRole, AgentStatus, AttentionState, ProviderKind, SessionMode};

    #[tokio::test]
    async fn mcp_initialize_instructions_match_caller_authority() {
        let tmp = std::env::temp_dir().join(format!("orbit-mcp-init-{}", Uuid::new_v4()));
        let rt = DaemonRuntime::bootstrap(&tmp).await.expect("bootstrap");
        let now = Utc::now();
        rt.store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "Test Workspace".into(),
                cwd: tmp.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert workspace");

        let mk_agent = |id: &str, role: AgentRole| {
            let mut ag = orbit_protocol::AgentSummary {
                agent_id: id.into(),
                workspace_id: "ws_1".into(),
                cwd: tmp.to_string_lossy().to_string(),
                title: id.into(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::AcceptEdits,
                status: AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 0,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: AttentionState::None,
                last_activity_at: Some(now),
                queued_prompts: Vec::new(),
                read_seq: 0,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
            };
            ag.set_role(role);
            ag
        };
        rt.store
            .upsert_agent(&mk_agent("ag_worker", AgentRole::Worker), None)
            .expect("upsert worker");
        rt.store
            .upsert_agent(&mk_agent("ag_router", AgentRole::PersonalRouter), None)
            .expect("upsert router");

        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2025-03-26" }
        });

        let worker_resp = handle_single_rpc(&rt, Some("ag_worker"), &init_req)
            .await
            .expect("worker init");
        let worker_inst = worker_resp["result"]["instructions"].as_str().unwrap_or("");
        assert!(worker_inst.contains("harness-native subagent"));
        assert!(!worker_inst.contains("spawn_workspace_peer"));
        assert!(!worker_inst.contains("action=\"fork\""));

        let missing_caller_resp = handle_single_rpc(&rt, None, &init_req)
            .await
            .expect("missing caller init");
        let missing_inst = missing_caller_resp["result"]["instructions"]
            .as_str()
            .unwrap_or("");
        assert!(missing_inst.contains("harness-native subagent"));
        assert!(!missing_inst.contains("spawn_workspace_peer"));

        let router_resp = handle_single_rpc(&rt, Some("ag_router"), &init_req)
            .await
            .expect("router init");
        let router_inst = router_resp["result"]["instructions"].as_str().unwrap_or("");
        assert!(router_inst.contains("create_agent"));
        assert!(router_inst.contains("spawn_workspace_peer"));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
