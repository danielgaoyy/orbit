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
use crate::hooks::{HOOK_ORIGIN_TOKEN_HTTP_HEADER, HookOriginContext};
use crate::tools::{
    execute_tool_with_context, list_tool_definitions_for_caller, resolve_caller_policy,
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

    let hook_origin = extract_verified_hook_origin(&runtime, &headers);

    // Handle JSON-RPC batch or single request
    if let Some(batch) = body.as_array() {
        let mut responses = Vec::new();
        for item in batch {
            if let Some(resp) =
                handle_single_rpc(&runtime, caller_agent_id, &hook_origin, item).await
            {
                responses.push(resp);
            }
        }
        if responses.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        return (StatusCode::OK, Json(Value::Array(responses))).into_response();
    }

    match handle_single_rpc(&runtime, caller_agent_id, &hook_origin, &body).await {
        Some(resp) => (StatusCode::OK, Json(resp)).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    }
}

/// Extract authenticated hook-origin provenance from MCP request headers.
///
/// Only accepts a live, daemon-issued 256-bit `x-orbit-hook-origin-token` registered for a
/// currently running hook rule so normal agents cannot forge `x-orbit-hook-origin` headers to
/// bypass `PreToolUse` security rules or silence `PostToolUse` auditing.
pub(crate) fn extract_verified_hook_origin(
    runtime: &DaemonRuntime,
    headers: &HeaderMap,
) -> HookOriginContext {
    headers
        .get(HOOK_ORIGIN_TOKEN_HTTP_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|token| runtime.hooks.verify_hook_origin_token(token))
        .unwrap_or_default()
}

async fn handle_single_rpc(
    runtime: &Arc<DaemonRuntime>,
    caller_agent_id: Option<&str>,
    hook_origin: &HookOriginContext,
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
                    "instructions": "Orbit built-in multi-agent & Mission Workspace orchestration server. Inside a Mission Workspace, coordinate with sibling peers using `workspace_peers` (pass `handle` to peek context), `message_peer` (`@handle` or `all`), `workspace_board` (`read`/`update` with optional `expected_revision` CAS lock), `spawn_workspace_peer` (`worktree=true` for parallel edits), `orbit_agent(action=\"fork\", worktree=true, ...)` (isolates onto a git worktree while staying in the same Mission Workspace), `handoff_worktree` (pass `agent_id`/`@handle` to merge peer worktrees back cleanly), and `orbit_timer` (supports `all_peers` and `sender:<id|@handle>` barriers). Parallel-first: release order != hard block; unless strictly hard-blocked, self-isolate onto a git worktree via `orbit_agent(action=\"fork\", worktree=true, ...)` or `spawn_workspace_peer(worktree=true, ...)` and develop in parallel, then rebase/merge via `handoff_worktree`. Never go idle silently when waiting on a peer — arm `orbit_timer(condition=\"all_peers\"|\"sender:@handle\")` or `message_peer(notify_on_finish=true)`. Prefer reactive finish notifications over polling. For capabilities outside loaded packs (schedules, browser, terminals, workflows, artifacts, LSP), run `orbit <subcommand>` via shell (`orbit --help`)."
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
            let exec =
                execute_tool_with_context(runtime, caller_agent_id, tool_name, args, hook_origin)
                    .await;
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
