//! Native Rust driver for OpenAI Codex (`codex app-server` JSON-RPC 2.0 + `~/.codex/auth.json` Wham Usage).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use orbit_protocol::{
    AgentStatus, ProviderKind, ProviderModelCatalog, ProviderModelSpec, ProviderQuotaSnapshot,
    QuestionItem, QuotaWindow, ReasoningEffort, SessionMode, ThinkingOptionSpec, ToolCallDetail,
    ToolCallStatus,
};
use orbit_shim::{ShimFrame, ShimMode, ShimSpawnSpec, spawn_detached_shim};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};

/// Serialize concurrent `codex app-server` SQLite state runtime startups.
fn codex_startup_mutex() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

pub async fn spawn_codex_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    let startup_guard = codex_startup_mutex().lock().await;

    let mut env = HashMap::new();
    env.insert("NO_COLOR".into(), "1".into());

    let spec = ShimSpawnSpec {
        shim_id: format!("codex_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args: vec!["app-server".to_string()],
        cwd: cfg.cwd.clone(),
        env,
    };

    let mut shim = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
        .await
        .map_err(|e| format!("failed to spawn codex shim: {e}"))?;

    // 1. Handshake: `initialize` -> `initialized` while holding startup lock
    let init_req = json!({
        "id": 1u64,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "codex_app_server_daemon",
                "title": "Orbit Codex Driver",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "experimentalApi": true,
                "mcpServerOpenaiFormElicitation": true
            }
        }
    })
    .to_string();

    shim.write_stdin_line(init_req)
        .await
        .map_err(|e| format!("codex write initialize failed: {e}"))?;

    wait_codex_id_response(&mut shim, 1).await?;

    let initialized_notif = json!({
        "method": "initialized",
        "params": {}
    })
    .to_string();
    shim.write_stdin_line(initialized_notif)
        .await
        .map_err(|e| format!("codex write initialized failed: {e}"))?;

    drop(startup_guard);

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_codex_actor(cfg, shim, cmd_rx, evt_tx));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

struct PendingCodexApproval {
    rpc_id: u64,
}

struct PendingCodexQuestion {
    rpc_id: u64,
    question_ids: Vec<String>,
}

async fn run_codex_actor(
    cfg: SessionLaunchConfig,
    mut shim: orbit_shim::ShimClient,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_id: u64 = 2;
    let mut mode = cfg.mode.clone();

    // Start or resume thread
    let (approval_policy, start_sandbox) = codex_thread_mode_strings(&mode);
    let thread_req_id = next_id;
    next_id += 1;

    let start_msg = if let Some(ref tid) = cfg.resume_session_id {
        json!({
            "id": thread_req_id,
            "method": "thread/resume",
            "params": {
                "threadId": tid,
                "excludeTurns": true
            }
        })
    } else {
        let mut params = json!({
            "cwd": cfg.cwd.to_string_lossy(),
            "approvalPolicy": approval_policy,
            "sandbox": start_sandbox,
            "approvalsReviewer": "user",
            "ephemeral": false
        });
        if let Some(ref m) = cfg.model {
            params["model"] = Value::String(m.clone());
        }
        json!({
            "id": thread_req_id,
            "method": "thread/start",
            "params": params
        })
    };

    if shim.write_stdin_line(start_msg.to_string()).await.is_err() {
        return;
    }

    let thread_res = match wait_codex_id_response(&mut shim, thread_req_id).await {
        Ok(v) => v,
        Err(e) => {
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                    message: format!("Codex thread start failed: {e}"),
                    recoverable: false,
                }))
                .await;
            return;
        }
    };

    let thread_id = thread_res
        .pointer("/thread/id")
        .and_then(|v| v.as_str())
        .or(cfg.resume_session_id.as_deref())
        .unwrap_or("unknown")
        .to_string();
    let mut resolved_model = thread_res
        .get("model")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| cfg.model.clone());
    let mut current_effort = cfg.effort;

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: thread_id.clone(),
            model: resolved_model.clone(),
        })
        .await;
    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
        .await;

    let mut native_turn_id: Option<String> = None;
    let mut client_turn_id: Option<String> = None;
    let mut msg_accum: HashMap<String, String> = HashMap::new();
    let mut thought_accum: HashMap<String, String> = HashMap::new();
    let mut file_change_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_approvals: HashMap<String, PendingCodexApproval> = HashMap::new();
    let mut pending_questions: HashMap<String, PendingCodexQuestion> = HashMap::new();

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt { turn_id, prompt } => {
                        // If a turn is already active and we know `native_turn_id`, steer in-place!
                        if let Some(ref active_nat_tid) = native_turn_id {
                            let req_id = next_id;
                            next_id += 1;
                            let steer = json!({
                                "id": req_id,
                                "method": "turn/steer",
                                "params": {
                                    "threadId": thread_id,
                                    "expectedTurnId": active_nat_tid,
                                    "input": [{
                                        "type": "text",
                                        "text": prompt,
                                        "text_elements": []
                                    }]
                                }
                            });
                            let _ = shim.write_stdin_line(steer.to_string()).await;
                            continue;
                        }

                        client_turn_id = Some(turn_id.clone());
                        msg_accum.clear();
                        thought_accum.clear();
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                turn_id: turn_id.clone(),
                            }))
                            .await;

                        let (pol, sandbox_policy) = codex_turn_policies(&mode);
                        let req_id = next_id;
                        next_id += 1;
                        let mut params = json!({
                            "threadId": thread_id,
                            "input": [{
                                "type": "text",
                                "text": prompt,
                                "text_elements": []
                            }],
                            "cwd": cfg.cwd.to_string_lossy(),
                            "approvalPolicy": pol,
                            "approvalsReviewer": "user",
                            "sandboxPolicy": sandbox_policy
                        });
                        if let Some(ref m) = resolved_model {
                            params["model"] = Value::String(m.clone());
                        }
                        if let Some(eff) = current_effort {
                            let eff_str = match eff {
                                ReasoningEffort::Off => Some("none"),
                                ReasoningEffort::Low => Some("low"),
                                ReasoningEffort::Medium => Some("medium"),
                                ReasoningEffort::High => Some("high"),
                                ReasoningEffort::Xhigh => Some("xhigh"),
                                ReasoningEffort::Max => Some("max"),
                                ReasoningEffort::Ultra => Some("ultra"),
                            };
                            if let Some(s) = eff_str {
                                params["effort"] = Value::String(s.to_string());
                            }
                        }
                        if matches!(mode, SessionMode::Plan) {
                            params["collaborationMode"] = json!({
                                "mode": "plan",
                                "settings": {}
                            });
                        }

                        let req = json!({
                            "id": req_id,
                            "method": "turn/start",
                            "params": params
                        });
                        if shim.write_stdin_line(req.to_string()).await.is_err() {
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        if let Some(ref nat_tid) = native_turn_id {
                            let req_id = next_id;
                            next_id += 1;
                            let req = json!({
                                "id": req_id,
                                "method": "turn/interrupt",
                                "params": {
                                    "threadId": thread_id,
                                    "turnId": nat_tid
                                }
                            });
                            let _ = shim.write_stdin_line(req.to_string()).await;
                        }
                    }
                    DriverCommand::SetMode { mode: new_mode } => {
                        mode = new_mode;
                    }
                    DriverCommand::SetConfig { model, effort } => {
                        resolved_model = model;
                        current_effort = effort;
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(app) = pending_approvals.remove(&request_id) {
                            let wire_dec = match decision.as_str() {
                                "allow" | "allow_always" => "accept",
                                _ => "decline",
                            };
                            let resp = json!({
                                "id": app.rpc_id,
                                "result": { "decision": wire_dec }
                            });
                            let _ = shim.write_stdin_line(resp.to_string()).await;
                            if let Some(ref tid) = client_turn_id {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                        turn_id: tid.clone(),
                                    }))
                                    .await;
                            }
                        }
                    }
                    DriverCommand::AnswerQuestion { request_id, answers } => {
                        if let Some(pq) = pending_questions.remove(&request_id) {
                            let mut ans_map = serde_json::Map::new();
                            for (idx, qid) in pq.question_ids.iter().enumerate() {
                                let selected = answers.get(idx).cloned().unwrap_or_default();
                                ans_map.insert(qid.clone(), json!({ "answers": selected }));
                            }
                            let resp = json!({
                                "id": pq.rpc_id,
                                "result": { "answers": ans_map }
                            });
                            let _ = shim.write_stdin_line(resp.to_string()).await;
                            if let Some(ref tid) = client_turn_id {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                        turn_id: tid.clone(),
                                    }))
                                    .await;
                            }
                        }
                    }
                    DriverCommand::Shutdown => {
                        let _ = shim.kill().await;
                        break;
                    }
                }
            }

            maybe_frame = shim.rx.recv() => {
                let Some(frame) = maybe_frame else { break; };
                match frame {
                    ShimFrame::StdoutLine { line } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let rpc_id = val.get("id").and_then(|v| v.as_u64());
                        let method = val.get("method").and_then(|v| v.as_str());

                        if let (Some(id), Some(m)) = (rpc_id, method) {
                            // Server -> Client interactive RPC request
                            handle_codex_server_request(
                                id,
                                m,
                                &val["params"],
                                &mode,
                                &client_turn_id,
                                &file_change_cache,
                                &shim,
                                &mut pending_approvals,
                                &mut pending_questions,
                                &evt_tx,
                            )
                            .await;
                        } else if rpc_id.is_none()
                            && let Some(m) = method
                        {
                            handle_codex_notification(
                                m,
                                &val["params"],
                                &mut native_turn_id,
                                &mut client_turn_id,
                                &mut msg_accum,
                                &mut thought_accum,
                                &mut file_change_cache,
                                &evt_tx,
                            )
                            .await;
                        } else if rpc_id.is_some()
                            && method.is_none()
                            && let Some(err) = val.get("error").filter(|v| !v.is_null())
                        {
                            let msg = err
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Codex RPC error")
                                .to_string();
                            if !msg.contains("no active turn") {
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "error".into(),
                                        message: msg,
                                    })
                                    .await;
                            }
                        }
                    }
                    ShimFrame::Exited { code } => {
                        let _ = evt_tx.send(DriverEvent::Exited { code }).await;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
}

async fn wait_codex_id_response(
    shim: &mut orbit_shim::ShimClient,
    expected_id: u64,
) -> Result<Value, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            return Err(format!("Codex RPC #{expected_id} timed out"));
        }
        match tokio::time::timeout(rem, shim.rx.recv()).await {
            Ok(Some(ShimFrame::StdoutLine { line })) => {
                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if val.get("method").is_none()
                    && val.get("id").and_then(|v| v.as_u64()) == Some(expected_id)
                {
                    if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                        return Err(err.to_string());
                    }
                    return Ok(val.get("result").cloned().unwrap_or(Value::Null));
                }
            }
            Ok(Some(ShimFrame::Exited { code })) => {
                return Err(format!("codex app-server exited early ({code:?})"));
            }
            Ok(Some(_)) => {}
            _ => return Err("codex shim closed".into()),
        }
    }
}

fn codex_thread_mode_strings(mode: &SessionMode) -> (&'static str, &'static str) {
    match mode {
        SessionMode::FullAccess => ("never", "danger-full-access"),
        SessionMode::Plan => ("on-request", "read-only"),
        _ => ("on-request", "workspace-write"),
    }
}

fn codex_turn_policies(mode: &SessionMode) -> (&'static str, Value) {
    match mode {
        SessionMode::FullAccess => ("never", json!({ "type": "dangerFullAccess" })),
        SessionMode::Plan => ("on-request", json!({ "type": "readOnly" })),
        _ => (
            "on-request",
            json!({
                "type": "workspaceWrite",
                "networkAccess": false,
                "writableRoots": [],
                "excludeSlashTmp": false,
                "excludeTmpdirEnvVar": false
            }),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_codex_notification(
    method: &str,
    params: &Value,
    native_turn_id: &mut Option<String>,
    client_turn_id: &mut Option<String>,
    msg_accum: &mut HashMap<String, String>,
    thought_accum: &mut HashMap<String, String>,
    file_change_cache: &mut HashMap<String, ToolCallDetail>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    match method {
        "turn/started" => {
            if let Some(tid) = params.pointer("/turn/id").and_then(|v| v.as_str()) {
                *native_turn_id = Some(tid.to_string());
                if client_turn_id.is_none() {
                    *client_turn_id = Some(tid.to_string());
                }
            }
        }
        "item/agentMessage/delta" => {
            let item_id = params
                .get("itemId")
                .and_then(|v| v.as_str())
                .unwrap_or("codex_msg")
                .to_string();
            let delta = params.get("delta").and_then(|v| v.as_str()).unwrap_or("");
            if !delta.is_empty() {
                let buf = msg_accum.entry(item_id.clone()).or_default();
                buf.push_str(delta);
                let _ = evt_tx
                    .send(DriverEvent::AssistantText {
                        item_id,
                        text: buf.clone(),
                        streaming: true,
                    })
                    .await;
            }
        }
        "item/reasoning/summaryTextDelta" => {
            let item_id = params
                .get("itemId")
                .and_then(|v| v.as_str())
                .unwrap_or("codex_rsn")
                .to_string();
            let delta = params.get("delta").and_then(|v| v.as_str()).unwrap_or("");
            if !delta.is_empty() {
                let buf = thought_accum.entry(item_id.clone()).or_default();
                buf.push_str(delta);
                let _ = evt_tx
                    .send(DriverEvent::ThinkingText {
                        item_id,
                        text: buf.clone(),
                    })
                    .await;
            }
        }
        "item/started" | "item/completed" => {
            let is_done = method == "item/completed";
            let Some(item) = params.get("item") else {
                return;
            };
            let item_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("item")
                .to_string();
            let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match item_type {
                "agentMessage" | "AgentMessage" => {
                    if is_done {
                        let final_text = item
                            .get("text")
                            .and_then(|v| v.as_str())
                            .map(ToOwned::to_owned)
                            .or_else(|| msg_accum.remove(&item_id))
                            .unwrap_or_default();
                        if !final_text.is_empty() {
                            let _ = evt_tx
                                .send(DriverEvent::AssistantText {
                                    item_id,
                                    text: final_text,
                                    streaming: false,
                                })
                                .await;
                        }
                    }
                }
                "commandExecution" | "CommandExecution" => {
                    let cmd_str = format_codex_cmd(&item["command"]);
                    let cwd = item
                        .get("cwd")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    let exit_code = item
                        .get("exitCode")
                        .and_then(|v| v.as_i64())
                        .map(|c| c as i32);
                    let out_prev = item
                        .get("aggregatedOutput")
                        .and_then(|v| v.as_str())
                        .map(|s| truncate_str(s, 800));
                    let status =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status,
                            detail: ToolCallDetail::Shell {
                                command: cmd_str,
                                cwd,
                                exit_code,
                                output_preview: out_prev,
                            },
                        })
                        .await;
                }
                "fileChange" | "FileChange" => {
                    let changes = item
                        .get("changes")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let first_path = changes
                        .first()
                        .and_then(|c| c.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("files")
                        .to_string();
                    let combined_diff: String = changes
                        .iter()
                        .filter_map(|c| c.get("diff").and_then(|d| d.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    let detail = ToolCallDetail::FileEdit {
                        path: first_path,
                        unified_diff: if combined_diff.is_empty() {
                            None
                        } else {
                            Some(combined_diff)
                        },
                    };
                    file_change_cache.insert(item_id.clone(), detail.clone());
                    let status =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status,
                            detail,
                        })
                        .await;
                }
                "webSearch" | "WebSearch" => {
                    let query = item
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let status =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status,
                            detail: ToolCallDetail::Search { query, scope: None },
                        })
                        .await;
                }
                _ => {}
            }
        }
        "turn/completed" => {
            *native_turn_id = None;
            *client_turn_id = None;
            let status_str = params
                .pointer("/turn/status")
                .and_then(|v| v.as_str())
                .unwrap_or("completed");
            if status_str == "failed" {
                let err_msg = params
                    .pointer("/turn/error/message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Codex turn failed")
                    .to_string();
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                        message: err_msg,
                        recoverable: true,
                    }))
                    .await;
            } else {
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                    .await;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_codex_server_request(
    rpc_id: u64,
    method: &str,
    params: &Value,
    mode: &SessionMode,
    client_turn_id: &Option<String>,
    file_change_cache: &HashMap<String, ToolCallDetail>,
    shim: &orbit_shim::ShimClient,
    pending_approvals: &mut HashMap<String, PendingCodexApproval>,
    pending_questions: &mut HashMap<String, PendingCodexQuestion>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    match method {
        "item/commandExecution/requestApproval" => {
            if matches!(mode, SessionMode::FullAccess) {
                let _ = shim
                    .write_stdin_line(
                        json!({ "id": rpc_id, "result": { "decision": "accept" } }).to_string(),
                    )
                    .await;
                return;
            }
            let req_id = format!("cperm_{rpc_id}");
            let cmd = format_codex_cmd(&params["command"]);
            let cwd = params
                .get("cwd")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned);
            pending_approvals.insert(req_id.clone(), PendingCodexApproval { rpc_id });
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id.clone(),
                    },
                ))
                .await;
            let _ = evt_tx
                .send(DriverEvent::PermissionRequest {
                    request_id: req_id,
                    title: format!("Run command: {cmd}"),
                    detail: ToolCallDetail::Shell {
                        command: cmd,
                        cwd,
                        exit_code: None,
                        output_preview: None,
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                })
                .await;
        }
        "item/fileChange/requestApproval" => {
            if matches!(mode, SessionMode::FullAccess | SessionMode::AcceptEdits) {
                let _ = shim
                    .write_stdin_line(
                        json!({ "id": rpc_id, "result": { "decision": "accept" } }).to_string(),
                    )
                    .await;
                return;
            }
            let req_id = format!("cfile_{rpc_id}");
            let item_id = params.get("itemId").and_then(|v| v.as_str()).unwrap_or("");
            let detail = file_change_cache.get(item_id).cloned().unwrap_or_else(|| {
                ToolCallDetail::FileEdit {
                    path: "workspace files".into(),
                    unified_diff: None,
                }
            });
            pending_approvals.insert(req_id.clone(), PendingCodexApproval { rpc_id });
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id.clone(),
                    },
                ))
                .await;
            let _ = evt_tx
                .send(DriverEvent::PermissionRequest {
                    request_id: req_id,
                    title: "Approve file modifications".into(),
                    detail,
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                })
                .await;
        }
        "item/tool/requestUserInput" | "tool/requestUserInput" => {
            let Some(q_arr) = params.get("questions").and_then(|v| v.as_array()) else {
                let _ = shim
                    .write_stdin_line(
                        json!({ "id": rpc_id, "result": { "answers": {} } }).to_string(),
                    )
                    .await;
                return;
            };
            let mut items = Vec::new();
            let mut q_ids = Vec::new();
            for (idx, q) in q_arr.iter().enumerate() {
                let qid = q
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| format!("q{idx}"));
                let opts = q
                    .get("options")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|o| {
                                o.get("label")
                                    .and_then(|l| l.as_str())
                                    .or_else(|| o.as_str())
                                    .map(ToOwned::to_owned)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                q_ids.push(qid.clone());
                items.push(QuestionItem {
                    id: qid,
                    header: q
                        .get("header")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned),
                    question: q
                        .get("question")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    options: opts,
                    is_multi_select: q
                        .get("multiSelect")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                });
            }
            let req_id = format!("cq_{rpc_id}");
            pending_questions.insert(
                req_id.clone(),
                PendingCodexQuestion {
                    rpc_id,
                    question_ids: q_ids,
                },
            );
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id.clone(),
                    },
                ))
                .await;
            let _ = evt_tx
                .send(DriverEvent::QuestionRequest {
                    request_id: req_id,
                    questions: items,
                })
                .await;
        }
        _ => {
            let _ = shim
                .write_stdin_line(json!({ "id": rpc_id, "result": {} }).to_string())
                .await;
        }
    }
}

fn format_codex_cmd(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(arr) = v.as_array() {
        let strs: Vec<&str> = arr.iter().filter_map(|x| x.as_str()).collect();
        if strs.len() == 3
            && (strs[0].ends_with("bash") || strs[0].ends_with("sh"))
            && strs[1] == "-lc"
        {
            return strs[2].to_string();
        }
        return strs.join(" ");
    }
    String::new()
}

fn map_codex_item_status(raw: Option<&str>, is_completed_notif: bool) -> ToolCallStatus {
    match raw {
        Some("completed") => ToolCallStatus::Completed,
        Some("failed") => ToolCallStatus::Failed,
        Some("declined") => ToolCallStatus::Denied,
        _ if is_completed_notif => ToolCallStatus::Completed,
        _ => ToolCallStatus::Running,
    }
}

// --- Codex Wham Usage Quota Probe (`~/.codex/auth.json`) ---

#[derive(Deserialize)]
struct CodexAuthFile {
    tokens: Option<CodexAuthTokens>,
}

#[derive(Deserialize)]
struct CodexAuthTokens {
    access_token: Option<String>,
    account_id: Option<String>,
}

pub async fn probe_codex_quota() -> ProviderQuotaSnapshot {
    let codex_home = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        });
    let auth_path: &Path = &codex_home.join("auth.json");
    let Ok(raw) = tokio::fs::read_to_string(auth_path).await else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("~/.codex/auth.json not found".into()),
            updated_at: Utc::now(),
        };
    };

    let Ok(parsed) = serde_json::from_str::<CodexAuthFile>(&raw) else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("invalid ~/.codex/auth.json".into()),
            updated_at: Utc::now(),
        };
    };

    let Some(tokens) = parsed.tokens else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("Codex using API key (no subscription quota)".into()),
            updated_at: Utc::now(),
        };
    };
    let Some(access_token) = tokens.access_token else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("missing access_token in ~/.codex/auth.json".into()),
            updated_at: Utc::now(),
        };
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ProviderQuotaSnapshot {
                provider: ProviderKind::Codex,
                account_label: None,
                windows: Vec::new(),
                credits_remaining: None,
                error: Some(e.to_string()),
                updated_at: Utc::now(),
            };
        }
    };

    let mut req = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        );
    if let Some(ref acc_id) = tokens.account_id {
        req = req.header("ChatGPT-Account-Id", acc_id);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(val) = resp.json::<Value>().await {
                let plan = val
                    .get("plan_type")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_uppercase());
                let email = val.get("email").and_then(|v| v.as_str());
                let account_label = match (email, plan.as_deref()) {
                    (Some(e), Some(p)) => Some(format!("{e} ({p})")),
                    (Some(e), None) => Some(e.to_string()),
                    (None, Some(p)) => Some(p.to_string()),
                    _ => None,
                };

                let mut windows = Vec::new();
                for ptr in ["/rate_limit/primary_window", "/rate_limit/secondary_window"] {
                    if let Some(w) = val.pointer(ptr)
                        && let Some(used) = coerce_f64(w.get("used_percent"))
                    {
                        let secs = coerce_f64(w.get("limit_window_seconds")).unwrap_or(0.0);
                        let label = if (secs - 18_000.0).abs() < 1.0 {
                            "Codex · 5-Hour Window".to_string()
                        } else if (secs - 604_800.0).abs() < 1.0 {
                            "Codex · Weekly Window".to_string()
                        } else {
                            "Codex · Rate Window".to_string()
                        };
                        let resets_at = coerce_f64(w.get("reset_at"))
                            .and_then(|ts| Utc.timestamp_opt(ts as i64, 0).single());
                        windows.push(QuotaWindow {
                            label,
                            used_percent: used as f32,
                            resets_at,
                        });
                    }
                }
                let credits_remaining =
                    coerce_f64(val.pointer("/credits/balance")).filter(|c| *c > 0.0);
                ProviderQuotaSnapshot {
                    provider: ProviderKind::Codex,
                    account_label,
                    windows,
                    credits_remaining,
                    error: None,
                    updated_at: Utc::now(),
                }
            } else {
                ProviderQuotaSnapshot {
                    provider: ProviderKind::Codex,
                    account_label: None,
                    windows: Vec::new(),
                    credits_remaining: None,
                    error: Some("failed to parse Codex usage JSON".into()),
                    updated_at: Utc::now(),
                }
            }
        }
        Ok(resp) => ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some(format!("HTTP {}", resp.status())),
            updated_at: Utc::now(),
        },
        Err(e) => ProviderQuotaSnapshot {
            provider: ProviderKind::Codex,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some(e.to_string()),
            updated_at: Utc::now(),
        },
    }
}

fn coerce_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

// --- Paseo-Aligned Dynamic Model & Thinking Catalog Probing (`codex app-server` `model/list`) ---

fn resolve_codex_home_dir() -> PathBuf {
    std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".codex")
        })
}

pub fn format_codex_display_name(raw_display: &str, raw_id: &str) -> String {
    let base = if raw_display.trim().is_empty() {
        raw_id.trim()
    } else {
        raw_display.trim()
    };
    let lower = base.to_ascii_lowercase();
    if lower.starts_with("gpt-") && !lower.starts_with("gpt-oss") {
        let rest = &base[4..];
        let cleaned_rest = rest
            .split('-')
            .filter(|seg| !seg.is_empty())
            .map(|seg| {
                let mut chars = seg.chars();
                match chars.next() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        format!("{}{}", c.to_ascii_uppercase(), chars.as_str())
                    }
                    _ => seg.to_string(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        return format!("GPT {cleaned_rest}");
    }
    base.to_string()
}

async fn read_codex_config_toml_defaults() -> (Option<String>, Option<ReasoningEffort>) {
    let cfg_path = resolve_codex_home_dir().join("config.toml");
    let Ok(raw) = tokio::fs::read_to_string(cfg_path).await else {
        return (None, None);
    };
    let mut cfg_model = None;
    let mut cfg_effort = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.starts_with('[') {
            if trimmed.starts_with('[') && cfg_model.is_some() {
                break;
            }
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim();
            let val = v
                .split('#')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            if key == "model" && !val.is_empty() && cfg_model.is_none() {
                cfg_model = Some(val.to_string());
            } else if (key == "model_reasoning_effort" || key == "reasoning_effort")
                && cfg_effort.is_none()
            {
                cfg_effort = ReasoningEffort::from_slug(val);
            }
        }
    }
    (cfg_model, cfg_effort)
}

fn build_codex_thinking_options(
    entries: &[(ReasoningEffort, Option<String>)],
    resolved_default: Option<ReasoningEffort>,
) -> Vec<ThinkingOptionSpec> {
    let mut out = Vec::new();
    for (eff, desc) in entries {
        if out
            .iter()
            .any(|existing: &ThinkingOptionSpec| existing.effort == *eff)
        {
            continue;
        }
        out.push(ThinkingOptionSpec {
            effort: *eff,
            label: eff.display_label().to_string(),
            description: desc.clone(),
            is_default: Some(*eff) == resolved_default,
        });
    }
    if let Some(def) = resolved_default
        && !out.iter().any(|x| x.effort == def)
    {
        out.push(ThinkingOptionSpec {
            effort: def,
            label: def.display_label().to_string(),
            description: None,
            is_default: true,
        });
    }
    out
}

pub async fn fallback_codex_models() -> ProviderModelCatalog {
    let (cfg_model, cfg_effort) = read_codex_config_toml_defaults().await;

    // 1. Try reading `~/.codex/models_cache.json` written by Codex CLI
    let cache_path = resolve_codex_home_dir().join("models_cache.json");
    if let Ok(raw) = tokio::fs::read_to_string(cache_path).await
        && let Ok(val) = serde_json::from_str::<Value>(&raw)
        && let Some(arr) = val.get("models").and_then(|v| v.as_array())
    {
        let mut parsed_models = Vec::new();
        for item in arr {
            let visibility = item
                .get("visibility")
                .and_then(|v| v.as_str())
                .unwrap_or("list");
            let hidden = item
                .get("hidden")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if visibility == "hide" || hidden {
                continue;
            }
            let Some(id) = item
                .get("slug")
                .or_else(|| item.get("id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
            let display_raw = item
                .get("display_name")
                .or_else(|| item.get("displayName"))
                .and_then(|v| v.as_str())
                .unwrap_or(id);
            let description = item
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let model_default_eff = item
                .get("default_reasoning_level")
                .or_else(|| item.get("defaultReasoningEffort"))
                .and_then(|v| v.as_str())
                .and_then(ReasoningEffort::from_slug);
            let resolved_eff = cfg_effort.or(model_default_eff);

            let mut effort_entries = Vec::new();
            if let Some(levels) = item
                .get("supported_reasoning_levels")
                .or_else(|| item.get("supportedReasoningEfforts"))
                .and_then(|v| v.as_array())
            {
                for lvl in levels {
                    if let Some(slug) = lvl
                        .get("effort")
                        .or_else(|| lvl.get("reasoningEffort"))
                        .and_then(|v| v.as_str())
                        && let Some(eff) = ReasoningEffort::from_slug(slug)
                    {
                        let desc = lvl
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(ToOwned::to_owned);
                        effort_entries.push((eff, desc));
                    }
                }
            }

            parsed_models.push(ProviderModelSpec {
                id: id.to_string(),
                label: format_codex_display_name(display_raw, id),
                description,
                is_default: false,
                default_effort: resolved_eff,
                thinking_options: build_codex_thinking_options(&effort_entries, resolved_eff),
            });
        }

        if !parsed_models.is_empty() {
            let has_cfg = cfg_model
                .as_deref()
                .is_some_and(|m| parsed_models.iter().any(|pm| pm.id == m));
            for (idx, pm) in parsed_models.iter_mut().enumerate() {
                pm.is_default = if has_cfg {
                    cfg_model.as_deref() == Some(pm.id.as_str())
                } else {
                    idx == 0
                };
            }
            return ProviderModelCatalog {
                provider: ProviderKind::Codex,
                models: parsed_models,
                configured_default_model: cfg_model,
                configured_default_effort: cfg_effort,
            };
        }
    }

    // 2. Built-in static catalog matching Codex 0.169+ when CLI is not installed on host
    let full_efforts = vec![
        (
            ReasoningEffort::Low,
            Some("Fast responses with lighter reasoning".to_string()),
        ),
        (
            ReasoningEffort::Medium,
            Some("Balances speed and reasoning depth".to_string()),
        ),
        (
            ReasoningEffort::High,
            Some("Greater reasoning depth for complex problems".to_string()),
        ),
        (
            ReasoningEffort::Xhigh,
            Some("Extra high reasoning depth".to_string()),
        ),
        (
            ReasoningEffort::Max,
            Some("Maximum reasoning depth for the hardest problems".to_string()),
        ),
        (
            ReasoningEffort::Ultra,
            Some("Maximum reasoning with automatic task delegation".to_string()),
        ),
    ];
    let presets = [
        (
            "gpt-6.2-sol",
            "GPT 6.2 Sol",
            "Fast, go-to model for coding and everyday work.",
            ReasoningEffort::Low,
            true,
        ),
        (
            "gpt-6.2-astra",
            "GPT 6.2 Astra",
            "Most capable model for complex coding and professional work.",
            ReasoningEffort::Low,
            false,
        ),
        (
            "gpt-6.1-terra",
            "GPT 6.1 Terra",
            "Efficient model for visual understanding and computer use.",
            ReasoningEffort::Low,
            false,
        ),
        (
            "gpt-6.1-sol",
            "GPT 6.1 Sol",
            "Previous generation model for coding and everyday work.",
            ReasoningEffort::Low,
            false,
        ),
        (
            "gpt-6-astra",
            "GPT 6 Astra",
            "Previous generation model for complex coding and professional work.",
            ReasoningEffort::Medium,
            false,
        ),
        (
            "gpt-6-sol",
            "GPT 6 Sol",
            "Legacy model for coding and everyday work.",
            ReasoningEffort::Medium,
            false,
        ),
        (
            "gpt-6-luna",
            "GPT 6 Luna",
            "Fast and affordable model for easier tasks.",
            ReasoningEffort::Medium,
            false,
        ),
    ];

    let has_cfg = cfg_model
        .as_deref()
        .is_some_and(|m| presets.iter().any(|(id, ..)| *id == m));
    let models = presets
        .into_iter()
        .map(|(id, label, desc, def_eff, is_def)| {
            let resolved_eff = cfg_effort.unwrap_or(def_eff);
            ProviderModelSpec {
                id: id.to_string(),
                label: label.to_string(),
                description: desc.to_string(),
                is_default: if has_cfg {
                    cfg_model.as_deref() == Some(id)
                } else {
                    is_def
                },
                default_effort: Some(resolved_eff),
                thinking_options: build_codex_thinking_options(&full_efforts, Some(resolved_eff)),
            }
        })
        .collect();

    ProviderModelCatalog {
        provider: ProviderKind::Codex,
        models,
        configured_default_model: cfg_model,
        configured_default_effort: cfg_effort,
    }
}

pub async fn probe_codex_models(executable: Option<&Path>) -> ProviderModelCatalog {
    let Some(exe) = executable else {
        return fallback_codex_models().await;
    };

    let rpc_res = tokio::time::timeout(Duration::from_secs(6), async {
        let _startup_guard = codex_startup_mutex().lock().await;
        let mut child = Command::new(exe)
            .arg("app-server")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err("codex stdin unavailable".to_string());
        };
        let Some(stdout) = child.stdout.take() else {
            return Err("codex stdout unavailable".to_string());
        };
        let mut lines = BufReader::new(stdout).lines();

        let init_req = json!({
            "id": 1u64,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "orbit_model_probe",
                    "title": "Orbit Codex Catalog Probe",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        });
        stdin
            .write_all(format!("{init_req}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && v.get("id").and_then(|x| x.as_u64()) == Some(1)
            {
                break;
            }
        }
        drop(_startup_guard);

        let batch = format!(
            "{}\n{}\n{}\n",
            json!({ "method": "initialized", "params": {} }),
            json!({ "id": 2u64, "method": "model/list", "params": { "includeHidden": false } }),
            json!({ "id": 3u64, "method": "config/read", "params": {} }),
        );
        stdin
            .write_all(batch.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        let mut model_list_res: Option<Value> = None;
        let mut config_read_res: Option<Value> = None;

        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v.get("id").and_then(|x| x.as_u64()) {
                Some(2) => model_list_res = v.get("result").cloned(),
                Some(3) => config_read_res = v.get("result").cloned(),
                _ => {}
            }
            if model_list_res.is_some() && config_read_res.is_some() {
                break;
            }
        }

        let _ = child.kill().await;
        Ok::<(Option<Value>, Option<Value>), String>((model_list_res, config_read_res))
    })
    .await;

    let Ok(Ok((Some(list_val), cfg_val))) = rpc_res else {
        return fallback_codex_models().await;
    };

    let (toml_model, toml_effort) = read_codex_config_toml_defaults().await;
    let cfg_obj = cfg_val.as_ref().and_then(|v| v.get("config"));
    let configured_model = cfg_obj
        .and_then(|c| c.get("model"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
        .or(toml_model);
    let configured_effort = cfg_obj
        .and_then(|c| {
            c.get("model_reasoning_effort")
                .or_else(|| c.get("modelReasoningEffort"))
        })
        .and_then(|v| v.as_str())
        .and_then(ReasoningEffort::from_slug)
        .or(toml_effort);

    let Some(data) = list_val.get("data").and_then(|v| v.as_array()) else {
        return fallback_codex_models().await;
    };

    let has_cfg_model = configured_model.as_deref().is_some_and(|cm| {
        data.iter()
            .any(|item| item.get("id").and_then(|v| v.as_str()) == Some(cm))
    });

    let mut models = Vec::new();
    for item in data {
        if item
            .get("hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let Some(id) = item
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        else {
            continue;
        };
        let display_name = item
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        let description = item
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let item_is_default = item
            .get("isDefault")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let model_def_effort = item
            .get("defaultReasoningEffort")
            .and_then(|v| v.as_str())
            .and_then(ReasoningEffort::from_slug);
        let resolved_default_effort = configured_effort.or(model_def_effort);

        let mut effort_entries = Vec::new();
        if let Some(arr) = item
            .get("supportedReasoningEfforts")
            .and_then(|v| v.as_array())
        {
            for entry in arr {
                if let Some(slug) = entry.get("reasoningEffort").and_then(|v| v.as_str())
                    && let Some(eff) = ReasoningEffort::from_slug(slug)
                {
                    let desc = entry
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned);
                    effort_entries.push((eff, desc));
                }
            }
        }

        models.push(ProviderModelSpec {
            id: id.to_string(),
            label: format_codex_display_name(display_name, id),
            description,
            is_default: if has_cfg_model {
                configured_model.as_deref() == Some(id)
            } else {
                item_is_default
            },
            default_effort: resolved_default_effort,
            thinking_options: build_codex_thinking_options(
                &effort_entries,
                resolved_default_effort,
            ),
        });
    }

    if models.is_empty() {
        return fallback_codex_models().await;
    }

    if !models.iter().any(|m| m.is_default)
        && let Some(first) = models.first_mut()
    {
        first.is_default = true;
    }

    ProviderModelCatalog {
        provider: ProviderKind::Codex,
        models,
        configured_default_model: configured_model,
        configured_default_effort: configured_effort,
    }
}
