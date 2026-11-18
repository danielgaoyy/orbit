//! Native Rust driver for Google Antigravity (`agy acp --stdio` + `transcript_full.jsonl` monitor + `/usage`).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentStatus, ProviderKind, ProviderQuotaSnapshot, QuestionItem, QuotaWindow, SessionMode,
    ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{ShimFrame, ShimMode, ShimSpawnSpec, spawn_detached_shim};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use crate::driver::{
    DriverCommand, DriverEvent, ProviderSessionHandle, RpcId, SessionLaunchConfig,
};

pub async fn spawn_antigravity_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    let skip_perms = matches!(cfg.mode, SessionMode::FullAccess);
    let mut args = vec!["acp".to_string(), "--stdio".to_string()];
    if skip_perms {
        args.push("--dangerously-skip-permissions".to_string());
    }
    args.push("--add-dir".to_string());
    args.push(cfg.cwd.to_string_lossy().to_string());
    for extra in &cfg.extra_dirs {
        args.push("--add-dir".to_string());
        args.push(extra.to_string_lossy().to_string());
    }

    let mut env = HashMap::new();
    env.insert("NO_COLOR".into(), "1".into());

    let spec = ShimSpawnSpec {
        shim_id: format!("agy_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args,
        cwd: cfg.cwd.clone(),
        env,
    };

    let shim = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
        .await
        .map_err(|e| format!("failed to spawn agy shim: {e}"))?;

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_agy_actor(cfg, shim, cmd_rx, evt_tx));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

struct PendingPerm {
    rpc_id: Value,
    allow_option_id: String,
    allow_always_option_id: String,
    deny_option_id: String,
    is_question: bool,
    raw_questions: Vec<QuestionItem>,
}

struct PendingElicitation {
    rpc_id: Value,
    field_keys: Vec<(String, bool)>, // (key, is_multi_select)
}

async fn run_agy_actor(
    cfg: SessionLaunchConfig,
    mut shim: orbit_shim::ShimClient,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_rpc_id: i64 = 1;
    let mut pending_rpcs: HashMap<String, oneshot::Sender<Result<Value, Value>>> = HashMap::new();

    // 1. Send `initialize`
    let init_rx = match send_acp_req(
        &shim,
        &mut next_rpc_id,
        &mut pending_rpcs,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "elicitation": { "form": {} }
            },
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
    .await
    {
        Ok(rx) => rx,
        Err(_) => {
            let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
            return;
        }
    };

    if wait_rpc_during_boot(&mut shim, &mut pending_rpcs, init_rx)
        .await
        .is_err()
    {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                message: "Antigravity ACP initialize failed".into(),
                recoverable: false,
            }))
            .await;
        return;
    }

    // 2. `session/load` or `session/new`
    let is_resume = cfg.resume_session_id.is_some();
    let sess_rx = if let Some(ref sid) = cfg.resume_session_id {
        send_acp_req(
            &shim,
            &mut next_rpc_id,
            &mut pending_rpcs,
            "session/load",
            json!({
                "sessionId": sid,
                "cwd": cfg.cwd.to_string_lossy(),
                "mcpServers": []
            }),
        )
        .await
    } else {
        send_acp_req(
            &shim,
            &mut next_rpc_id,
            &mut pending_rpcs,
            "session/new",
            json!({
                "cwd": cfg.cwd.to_string_lossy(),
                "mcpServers": []
            }),
        )
        .await
    };
    let Ok(sess_rx) = sess_rx else {
        return;
    };

    let sess_val = match wait_rpc_during_boot(&mut shim, &mut pending_rpcs, sess_rx).await {
        Ok(v) => v,
        Err(e) => {
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                    message: format!("Antigravity session open failed: {e}"),
                    recoverable: false,
                }))
                .await;
            return;
        }
    };

    // Drain replay notifications on `session/load` until 50ms of quiet
    if is_resume {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            if tokio::time::timeout(Duration::from_millis(50), shim.rx.recv())
                .await
                .is_err()
            {
                break;
            }
        }
    }

    let session_id = sess_val
        .get("sessionId")
        .and_then(|v| v.as_str())
        .or(cfg.resume_session_id.as_deref())
        .unwrap_or("unknown")
        .to_string();

    let mut current_model = sess_val
        .pointer("/models/currentModelId")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| cfg.model.clone());

    // Optional `session/set_model`
    if let Some(ref wanted_model) = cfg.model
        && let Ok(rx) = send_acp_req(
            &shim,
            &mut next_rpc_id,
            &mut pending_rpcs,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": wanted_model
            }),
        )
        .await
        && let Ok(v) = wait_rpc_during_boot(&mut shim, &mut pending_rpcs, rx).await
        && let Some(m) = v.pointer("/models/currentModelId").and_then(|x| x.as_str())
    {
        current_model = Some(m.to_string());
    }

    // Optional `session/set_mode` for `accept-edits` (ignore -32601)
    if matches!(cfg.mode, SessionMode::AcceptEdits)
        && let Ok(rx) = send_acp_req(
            &shim,
            &mut next_rpc_id,
            &mut pending_rpcs,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": "accept-edits"
            }),
        )
        .await
    {
        let _ = wait_rpc_during_boot(&mut shim, &mut pending_rpcs, rx).await;
    }

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: session_id.clone(),
            model: current_model,
        })
        .await;
    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
        .await;

    // Active runtime state
    let mut mode = cfg.mode.clone();
    let mut active_turn_id: Option<String> = None;
    let mut active_prompt_rpc_key: Option<String> = None;
    let mut msg_accum: HashMap<String, String> = HashMap::new();
    let mut thought_accum: HashMap<String, String> = HashMap::new();
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingPerm> = HashMap::new();
    let mut pending_elicits: HashMap<String, PendingElicitation> = HashMap::new();

    // Post-turn autonomous wakeup (`transcript_full.jsonl`) state
    let mut transcript_monitor = AgyTranscriptMonitor::new(&session_id);
    let mut suppress_catchup_until_user_chunk = false;
    let mut synthesized_wakeup_tools: HashSet<String> = HashSet::new();
    let mut autonomous_turn_id: Option<String> = None;
    let mut transcript_poll = tokio::time::interval(Duration::from_millis(200));

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt { turn_id, prompt } => {
                        active_turn_id = Some(turn_id.clone());
                        msg_accum.clear();
                        thought_accum.clear();
                        let is_slash = prompt.trim_start().starts_with('/');
                        if is_slash {
                            suppress_catchup_until_user_chunk = false;
                        }

                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                turn_id: turn_id.clone(),
                            }))
                            .await;

                        let id = next_rpc_id;
                        next_rpc_id += 1;
                        let key = RpcId::Num(id).as_key();
                        active_prompt_rpc_key = Some(key);

                        let req = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "method": "session/prompt",
                            "params": {
                                "sessionId": session_id,
                                "prompt": [{ "type": "text", "text": prompt }]
                            }
                        })
                        .to_string();
                        if shim.write_stdin_line(req).await.is_err() {
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        let cancel = json!({
                            "jsonrpc": "2.0",
                            "method": "session/cancel",
                            "params": { "sessionId": session_id }
                        })
                        .to_string();
                        let _ = shim.write_stdin_line(cancel).await;
                    }
                    DriverCommand::SetMode { mode: new_mode } => {
                        mode = new_mode.clone();
                        if !matches!(mode, SessionMode::FullAccess) {
                            let mode_str = match mode {
                                SessionMode::AcceptEdits => "accept-edits",
                                SessionMode::Plan => "plan",
                                _ => "default",
                            };
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_mode",
                                "params": {
                                    "sessionId": session_id,
                                    "modeId": mode_str
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(req).await;
                        }
                    }
                    DriverCommand::SetModel { model: new_model } => {
                        if let Some(m) = new_model {
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_model",
                                "params": {
                                    "sessionId": session_id,
                                    "modelId": m
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(req).await;
                        }
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(p) = pending_perms.remove(&request_id) {
                            let chosen_opt = match decision.as_str() {
                                "allow_always" => &p.allow_always_option_id,
                                "allow" => &p.allow_option_id,
                                _ => &p.deny_option_id,
                            };
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": {
                                        "outcome": "selected",
                                        "optionId": chosen_opt
                                    }
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                            if let Some(ref tid) = active_turn_id {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                        turn_id: tid.clone(),
                                    }))
                                    .await;
                            }
                        }
                    }
                    DriverCommand::AnswerQuestion { request_id, answers } => {
                        if let Some(p) = pending_perms.remove(&request_id)
                            && p.is_question
                        {
                            let mut answers_map = serde_json::Map::new();
                            let mut content_map = serde_json::Map::new();
                            for (idx, q) in p.raw_questions.iter().enumerate() {
                                let joined = answers
                                    .get(idx)
                                    .map(|v| v.join(", "))
                                    .unwrap_or_default();
                                answers_map.insert(q.question.clone(), Value::String(joined.clone()));
                                content_map.insert(format!("q{idx}"), Value::String(joined));
                            }
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": {
                                        "outcome": "selected",
                                        "optionId": p.allow_option_id,
                                        "updatedInput": { "answers": answers_map }
                                    },
                                    "updatedInput": { "answers": answers_map },
                                    "_meta": {
                                        "antigravity": {
                                            "answers": answers_map,
                                            "content": content_map
                                        }
                                    }
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        } else if let Some(el) = pending_elicits.remove(&request_id) {
                            let mut content = serde_json::Map::new();
                            for (idx, (key, is_multi)) in el.field_keys.iter().enumerate() {
                                let vals = answers.get(idx).cloned().unwrap_or_default();
                                if *is_multi {
                                    content.insert(
                                        key.clone(),
                                        Value::Array(vals.into_iter().map(Value::String).collect()),
                                    );
                                } else {
                                    content.insert(
                                        key.clone(),
                                        Value::String(vals.into_iter().next().unwrap_or_default()),
                                    );
                                }
                            }
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": el.rpc_id,
                                "result": {
                                    "action": "accept",
                                    "content": content
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                        if let Some(ref tid) = active_turn_id {
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                    turn_id: tid.clone(),
                                }))
                                .await;
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
                        let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                        let method = val.get("method").and_then(|v| v.as_str());

                        if has_id && method.is_none() {
                            // Response to our request (e.g. `session/prompt` completion)
                            if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone()) {
                                let key = rid.as_key();
                                if active_prompt_rpc_key.as_deref() == Some(key.as_str()) {
                                    active_prompt_rpc_key = None;
                                    // Finalize any streaming message buffers
                                    for (mid, text) in msg_accum.drain() {
                                        let _ = evt_tx
                                            .send(DriverEvent::AssistantText {
                                                item_id: mid,
                                                text,
                                                streaming: false,
                                            })
                                            .await;
                                    }
                                    active_turn_id = None;
                                    // Fast-forward transcript reader past foreground steps
                                    transcript_monitor.sync_silent();
                                    if let Some(err_obj) = val.get("error") {
                                        let msg = err_obj
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Antigravity turn failed")
                                            .to_string();
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                                message: msg,
                                                recoverable: true,
                                            }))
                                            .await;
                                    } else {
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                            .await;
                                    }
                                }
                            }
                        } else if !has_id && method == Some("session/update") {
                            if let Some(update) = val.pointer("/params/update") {
                                handle_acp_session_update(
                                    update,
                                    &mut suppress_catchup_until_user_chunk,
                                    &synthesized_wakeup_tools,
                                    &mut msg_accum,
                                    &mut thought_accum,
                                    &mut tool_cache,
                                    &evt_tx,
                                )
                                .await;
                            }
                        } else if has_id && method == Some("session/request_permission") {
                            handle_acp_permission_request(
                                &val,
                                &mode,
                                &active_turn_id,
                                &shim,
                                &mut pending_perms,
                                &evt_tx,
                            )
                            .await;
                        } else if has_id && method == Some("elicitation/create") {
                            handle_acp_elicitation_request(
                                &val,
                                &active_turn_id,
                                &mut pending_elicits,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    ShimFrame::Exited { code } => {
                        let _ = evt_tx.send(DriverEvent::Exited { code }).await;
                        break;
                    }
                    _ => {}
                }
            }

            _ = transcript_poll.tick() => {
                // Only monitor post-turn autonomous wakeups when foreground turn is idle
                if active_turn_id.is_none() {
                    for wake_ev in transcript_monitor.poll_post_turn_wakeup() {
                        match wake_ev {
                            WakeupStep::Started { source } => {
                                let tid = format!("wakeup_{}", uuid::Uuid::new_v4().simple());
                                autonomous_turn_id = Some(tid.clone());
                                suppress_catchup_until_user_chunk = true;
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup {
                                        turn_id: tid,
                                        source,
                                    }))
                                    .await;
                            }
                            WakeupStep::Thinking { id, text } => {
                                let _ = evt_tx
                                    .send(DriverEvent::ThinkingText { item_id: id, text })
                                    .await;
                            }
                            WakeupStep::Message { id, text } => {
                                let _ = evt_tx
                                    .send(DriverEvent::AssistantText {
                                        item_id: id,
                                        text,
                                        streaming: false,
                                    })
                                    .await;
                            }
                            WakeupStep::Tool { call_id, status, detail } => {
                                synthesized_wakeup_tools.insert(call_id.clone());
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id,
                                        status,
                                        detail,
                                    })
                                    .await;
                            }
                            WakeupStep::Settled => {
                                if autonomous_turn_id.take().is_some() {
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                        .await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn send_acp_req(
    shim: &orbit_shim::ShimClient,
    next_id: &mut i64,
    pending: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    method: &str,
    params: Value,
) -> Result<oneshot::Receiver<Result<Value, Value>>, orbit_shim::ShimError> {
    let id = *next_id;
    *next_id += 1;
    let (tx, rx) = oneshot::channel();
    pending.insert(RpcId::Num(id).as_key(), tx);
    let payload = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string();
    shim.write_stdin_line(payload).await?;
    Ok(rx)
}

async fn wait_rpc_during_boot(
    shim: &mut orbit_shim::ShimClient,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    mut target_rx: oneshot::Receiver<Result<Value, Value>>,
) -> Result<Value, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("ACP initialization timed out".into());
        }
        tokio::select! {
            res = &mut target_rx => {
                return match res {
                    Ok(Ok(v)) => Ok(v),
                    Ok(Err(e)) => Err(e.to_string()),
                    Err(_) => Err("rpc channel dropped".into()),
                };
            }
            frame = tokio::time::timeout(remaining, shim.rx.recv()) => {
                match frame {
                    Ok(Some(ShimFrame::StdoutLine { line })) => {
                        if let Ok(val) = serde_json::from_str::<Value>(&line)
                            && val.get("method").is_none()
                            && let Some(id_val) = val.get("id")
                            && let Ok(rid) = serde_json::from_value::<RpcId>(id_val.clone())
                            && let Some(tx) = pending_rpcs.remove(&rid.as_key())
                        {
                            if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                                let _ = tx.send(Err(err.clone()));
                            } else {
                                let _ = tx.send(Ok(val.get("result").cloned().unwrap_or(Value::Null)));
                            }
                        }
                    }
                    Ok(Some(ShimFrame::Exited { code })) => {
                        return Err(format!("agy exited early with code {code:?}"));
                    }
                    Ok(Some(_)) => {}
                    _ => return Err("agy shim closed".into()),
                }
            }
        }
    }
}

async fn handle_acp_session_update(
    update: &Value,
    suppress_catchup: &mut bool,
    synthesized_wakeup_tools: &HashSet<String>,
    msg_accum: &mut HashMap<String, String>,
    thought_accum: &mut HashMap<String, String>,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let Some(kind) = update.get("sessionUpdate").and_then(|v| v.as_str()) else {
        return;
    };

    if kind == "user_message_chunk" {
        *suppress_catchup = false;
        return;
    }
    if *suppress_catchup {
        return;
    }

    match kind {
        "agent_message_chunk" => {
            let chunk = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            let mid = update
                .get("messageId")
                .and_then(|v| v.as_str())
                .unwrap_or("agy_msg_default")
                .to_string();
            let entry = msg_accum.entry(mid.clone()).or_default();
            entry.push_str(chunk);
            let _ = evt_tx
                .send(DriverEvent::AssistantText {
                    item_id: mid,
                    text: entry.clone(),
                    streaming: true,
                })
                .await;
        }
        "agent_thought_chunk" => {
            let chunk = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            let tid = update
                .get("messageId")
                .and_then(|v| v.as_str())
                .unwrap_or("agy_thought_default")
                .to_string();
            let entry = thought_accum.entry(tid.clone()).or_default();
            entry.push_str(chunk);
            let _ = evt_tx
                .send(DriverEvent::ThinkingText {
                    item_id: tid,
                    text: entry.clone(),
                })
                .await;
        }
        "tool_call" | "tool_call_update" => {
            let Some(call_id) = update.get("toolCallId").and_then(|v| v.as_str()) else {
                return;
            };
            if synthesized_wakeup_tools.contains(call_id) {
                return;
            }
            let status = match update.get("status").and_then(|v| v.as_str()) {
                Some("completed") => ToolCallStatus::Completed,
                Some("failed") => ToolCallStatus::Failed,
                _ => ToolCallStatus::Running,
            };
            let detail = map_agy_tool_detail(update, tool_cache.get(call_id));
            tool_cache.insert(call_id.to_string(), detail.clone());
            let _ = evt_tx
                .send(DriverEvent::ToolCall {
                    call_id: call_id.to_string(),
                    status,
                    detail,
                })
                .await;
        }
        _ => {}
    }
}

fn map_agy_tool_detail(update: &Value, prev: Option<&ToolCallDetail>) -> ToolCallDetail {
    let kind = update.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let raw_in = update.get("rawInput");
    let raw_out = update.get("rawOutput");

    let out_preview = raw_out.and_then(|o| {
        if let Some(s) = o.as_str() {
            Some(truncate_preview(s, 800))
        } else {
            o.get("output")
                .or_else(|| o.get("content"))
                .or_else(|| o.get("result"))
                .and_then(|v| v.as_str())
                .map(|s| truncate_preview(s, 800))
        }
    });

    if kind == "execute"
        || raw_in
            .and_then(|i| i.get("CommandLine").or_else(|| i.get("command")))
            .is_some()
    {
        let cmd = raw_in
            .and_then(|i| i.get("CommandLine").or_else(|| i.get("command")))
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { command, .. }) => Some(command.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());
        let exit_code = raw_out
            .and_then(|o| o.get("exitCode"))
            .and_then(|v| v.as_i64())
            .map(|c| c as i32);
        return ToolCallDetail::Shell {
            command: cmd,
            cwd: None,
            exit_code,
            output_preview: out_preview,
        };
    }

    if kind == "edit" || kind == "write" {
        let path = raw_in
            .and_then(|i| i.get("Path").or_else(|| i.get("TargetFile")))
            .and_then(|v| v.as_str())
            .or_else(|| update.pointer("/locations/0/path").and_then(|v| v.as_str()))
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::FileEdit { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());

        let diff = update
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("type").and_then(|t| t.as_str()) == Some("diff") {
                        let old_t = item.get("oldText").and_then(|v| v.as_str()).unwrap_or("");
                        let new_t = item.get("newText").and_then(|v| v.as_str()).unwrap_or("");
                        Some(format!("--- old\n+++ new\n-{old_t}\n+{new_t}"))
                    } else {
                        None
                    }
                })
            });

        return ToolCallDetail::FileEdit {
            path,
            unified_diff: diff,
        };
    }

    if kind == "read" {
        let path = raw_in
            .and_then(|i| i.get("Path").or_else(|| i.get("AbsolutePath")))
            .and_then(|v| v.as_str())
            .or_else(|| update.pointer("/locations/0/path").and_then(|v| v.as_str()))
            .unwrap_or(title)
            .to_string();
        return ToolCallDetail::FileRead {
            path,
            start_line: None,
            end_line: None,
        };
    }

    if title.contains("subagent")
        || raw_in
            .and_then(|i| i.get("Role").or_else(|| i.get("TypeName")))
            .is_some()
    {
        let role = raw_in
            .and_then(|i| i.get("Role").or_else(|| i.get("TypeName")))
            .and_then(|v| v.as_str())
            .unwrap_or("subagent")
            .to_string();
        let prompt = raw_in
            .and_then(|i| i.get("Prompt"))
            .and_then(|v| v.as_str())
            .map(|s| truncate_preview(s, 240))
            .unwrap_or_default();
        let child_id = raw_out
            .and_then(|o| o.get("conversationId"))
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned);
        return ToolCallDetail::Subagent {
            child_agent_id: child_id,
            role,
            prompt_preview: prompt,
        };
    }

    if let Some(existing) = prev {
        return existing.clone();
    }

    ToolCallDetail::Generic {
        tool_name: title.to_string(),
        arguments_json: raw_in.map(|v| v.to_string()).unwrap_or_default(),
    }
}

async fn handle_acp_permission_request(
    val: &Value,
    mode: &SessionMode,
    active_turn_id: &Option<String>,
    shim: &orbit_shim::ShimClient,
    pending_perms: &mut HashMap<String, PendingPerm>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val["id"].clone();
    let params = &val["params"];
    let tool_call = &params["toolCall"];
    let options = params["options"].as_array().cloned().unwrap_or_default();

    let allow_opt = select_option_id(&options, "allow_once", "allow");
    let allow_always_opt = select_option_id(&options, "allow_always", "allow");
    let deny_opt = select_option_id(&options, "reject_once", "deny");

    // Check if this is `ask_question` disguised as `session/request_permission`
    let raw_questions_arr = tool_call
        .pointer("/rawInput/questions")
        .and_then(|v| v.as_array());
    if let Some(q_arr) = raw_questions_arr
        && !q_arr.is_empty()
    {
        let mut parsed_questions = Vec::new();
        for (i, q) in q_arr.iter().enumerate() {
            let question = q
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let opts = q
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            item.as_str()
                                .or_else(|| item.get("label").and_then(|l| l.as_str()))
                                .map(ToOwned::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let multi = q
                .get("is_multi_select")
                .or_else(|| q.get("multiSelect"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            parsed_questions.push(QuestionItem {
                id: format!("q{i}"),
                header: q
                    .get("header")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                question,
                options: opts,
                is_multi_select: multi,
            });
        }
        let req_id = format!("q_{}", uuid::Uuid::new_v4().simple());
        pending_perms.insert(
            req_id.clone(),
            PendingPerm {
                rpc_id,
                allow_option_id: allow_opt,
                allow_always_option_id: allow_always_opt,
                deny_option_id: deny_opt,
                is_question: true,
                raw_questions: parsed_questions.clone(),
            },
        );
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(
                AgentStatus::AwaitingPermission {
                    turn_id: active_turn_id.clone().unwrap_or_default(),
                    request_id: req_id.clone(),
                },
            ))
            .await;
        let _ = evt_tx
            .send(DriverEvent::QuestionRequest {
                request_id: req_id,
                questions: parsed_questions,
            })
            .await;
        return;
    }

    // Auto-approve logic for `FullAccess` or native file edits in `AcceptEdits`
    let kind_str = tool_call.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let auto_approve = matches!(mode, SessionMode::FullAccess)
        || (matches!(mode, SessionMode::AcceptEdits)
            && matches!(
                kind_str,
                "edit" | "write" | "delete" | "move" | "read" | "search"
            ));

    if auto_approve {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": rpc_id,
            "result": {
                "outcome": {
                    "outcome": "selected",
                    "optionId": allow_opt
                }
            }
        })
        .to_string();
        let _ = shim.write_stdin_line(resp).await;
        return;
    }

    let req_id = format!("perm_{}", uuid::Uuid::new_v4().simple());
    let title = tool_call
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Approve tool execution")
        .to_string();
    let detail = map_agy_tool_detail(tool_call, None);

    pending_perms.insert(
        req_id.clone(),
        PendingPerm {
            rpc_id,
            allow_option_id: allow_opt,
            allow_always_option_id: allow_always_opt,
            deny_option_id: deny_opt,
            is_question: false,
            raw_questions: Vec::new(),
        },
    );

    let _ = evt_tx
        .send(DriverEvent::StatusChanged(
            AgentStatus::AwaitingPermission {
                turn_id: active_turn_id.clone().unwrap_or_default(),
                request_id: req_id.clone(),
            },
        ))
        .await;
    let _ = evt_tx
        .send(DriverEvent::PermissionRequest {
            request_id: req_id,
            title,
            detail,
            allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
        })
        .await;
}

async fn handle_acp_elicitation_request(
    val: &Value,
    active_turn_id: &Option<String>,
    pending_elicits: &mut HashMap<String, PendingElicitation>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val["id"].clone();
    let Some(props) = val
        .pointer("/params/requestedSchema/properties")
        .and_then(|v| v.as_object())
    else {
        return;
    };

    let mut questions = Vec::new();
    let mut field_keys = Vec::new();
    for (key, prop) in props {
        if key.ends_with("_other") {
            continue;
        }
        let is_multi = prop.get("type").and_then(|v| v.as_str()) == Some("array");
        let enum_arr = if is_multi {
            prop.pointer("/items/enum").and_then(|v| v.as_array())
        } else {
            prop.get("enum").and_then(|v| v.as_array())
        };
        let options: Vec<String> = enum_arr
            .map(|arr| {
                arr.iter()
                    .filter_map(|x| x.as_str())
                    .filter(|s| *s != "Other")
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let q_text = prop
            .get("description")
            .or_else(|| prop.get("title"))
            .and_then(|v| v.as_str())
            .unwrap_or(key)
            .to_string();

        field_keys.push((key.clone(), is_multi));
        questions.push(QuestionItem {
            id: key.clone(),
            header: prop
                .get("title")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned),
            question: q_text,
            options,
            is_multi_select: is_multi,
        });
    }

    let req_id = format!("elicit_{}", uuid::Uuid::new_v4().simple());
    pending_elicits.insert(req_id.clone(), PendingElicitation { rpc_id, field_keys });

    let _ = evt_tx
        .send(DriverEvent::StatusChanged(
            AgentStatus::AwaitingPermission {
                turn_id: active_turn_id.clone().unwrap_or_default(),
                request_id: req_id.clone(),
            },
        ))
        .await;
    let _ = evt_tx
        .send(DriverEvent::QuestionRequest {
            request_id: req_id,
            questions,
        })
        .await;
}

fn select_option_id(options: &[Value], exact_kind: &str, fallback_sub: &str) -> String {
    for opt in options {
        if opt.get("kind").and_then(|v| v.as_str()) == Some(exact_kind)
            && let Some(id) = opt.get("optionId").and_then(|v| v.as_str())
        {
            return id.to_string();
        }
    }
    for opt in options {
        if let Some(id) = opt.get("optionId").and_then(|v| v.as_str())
            && id.to_ascii_lowercase().contains(fallback_sub)
        {
            return id.to_string();
        }
    }
    fallback_sub.to_string()
}

// --- Post-turn `transcript_full.jsonl` incremental reader ---

enum WakeupStep {
    Started {
        source: String,
    },
    Thinking {
        id: String,
        text: String,
    },
    Message {
        id: String,
        text: String,
    },
    Tool {
        call_id: String,
        status: ToolCallStatus,
        detail: ToolCallDetail,
    },
    Settled,
}

struct AgyTranscriptMonitor {
    full_path: PathBuf,
    compact_path: PathBuf,
    byte_offset: u64,
    seen_steps: HashSet<u64>,
    in_wakeup: bool,
    pending_tools: Vec<String>,
}

impl AgyTranscriptMonitor {
    fn new(session_id: &str) -> Self {
        let data_dir = resolve_agy_data_dir();
        let logs_dir = data_dir
            .join("brain")
            .join(session_id)
            .join(".system_generated/logs");
        Self {
            full_path: logs_dir.join("transcript_full.jsonl"),
            compact_path: logs_dir.join("transcript.jsonl"),
            byte_offset: 0,
            seen_steps: HashSet::new(),
            in_wakeup: false,
            pending_tools: Vec::new(),
        }
    }

    fn active_path(&self) -> Option<&Path> {
        if self.full_path.exists() {
            Some(&self.full_path)
        } else if self.compact_path.exists() {
            Some(&self.compact_path)
        } else {
            None
        }
    }

    fn sync_silent(&mut self) {
        let _ = self.read_new_lines(true);
        self.in_wakeup = false;
        self.pending_tools.clear();
    }

    fn poll_post_turn_wakeup(&mut self) -> Vec<WakeupStep> {
        self.read_new_lines(false)
    }

    fn read_new_lines(&mut self, silent: bool) -> Vec<WakeupStep> {
        let Some(path) = self.active_path().map(Path::to_path_buf) else {
            return Vec::new();
        };
        let Ok(mut file) = std::fs::File::open(&path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        if meta.len() < self.byte_offset {
            self.byte_offset = 0;
        }
        if self.byte_offset > 0 {
            if file.seek(SeekFrom::Start(self.byte_offset - 1)).is_ok() {
                let mut b = [0u8; 1];
                if file.read_exact(&mut b).is_err() || b[0] != b'\n' {
                    self.byte_offset = 0;
                }
            } else {
                self.byte_offset = 0;
            }
        }
        if file.seek(SeekFrom::Start(self.byte_offset)).is_err() {
            return Vec::new();
        }

        let mut buf = String::new();
        if file.read_to_string(&mut buf).is_err() || buf.is_empty() {
            return Vec::new();
        }

        let mut out = Vec::new();
        let mut consumed_bytes = 0u64;

        for chunk in buf.split_inclusive('\n') {
            if !chunk.ends_with('\n') {
                break;
            }
            let Ok(step) = serde_json::from_str::<Value>(chunk) else {
                consumed_bytes += chunk.len() as u64;
                continue;
            };
            let step_idx = step
                .get("step_index")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            let step_type = step.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let status = step
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_uppercase();

            // Defer unfinished steps (agy rewrites them in place)
            if step_type == "PLANNER_RESPONSE"
                && matches!(
                    status.as_str(),
                    "PENDING" | "RUNNING" | "GENERATING" | "WAITING" | "WAITING_FOR_USER"
                )
            {
                break;
            }

            consumed_bytes += chunk.len() as u64;
            if self.seen_steps.contains(&step_idx) {
                continue;
            }
            self.seen_steps.insert(step_idx);

            if silent {
                continue;
            }

            let content = step.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if step_type == "SYSTEM_MESSAGE" || content.contains("<SYSTEM_MESSAGE>") {
                if !self.in_wakeup {
                    self.in_wakeup = true;
                    out.push(WakeupStep::Started {
                        source: "background_task".into(),
                    });
                }
            } else if self.in_wakeup && step_type == "PLANNER_RESPONSE" {
                if let Some(th) = step.get("thinking").and_then(|v| v.as_str())
                    && !th.trim().is_empty()
                {
                    out.push(WakeupStep::Thinking {
                        id: format!("wake_th_{step_idx}"),
                        text: th.to_string(),
                    });
                }
                if !content.trim().is_empty() {
                    out.push(WakeupStep::Message {
                        id: format!("wake_msg_{step_idx}"),
                        text: content.to_string(),
                    });
                }
                let calls = step
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if calls.is_empty() && status == "DONE" {
                    self.in_wakeup = false;
                    out.push(WakeupStep::Settled);
                } else {
                    for c in calls {
                        let cid = c
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("wake_tool")
                            .to_string();
                        let tname = c
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("tool")
                            .to_string();
                        self.pending_tools.push(cid.clone());
                        out.push(WakeupStep::Tool {
                            call_id: cid,
                            status: ToolCallStatus::Running,
                            detail: ToolCallDetail::Generic {
                                tool_name: tname,
                                arguments_json: c
                                    .get("args")
                                    .map(|v| v.to_string())
                                    .unwrap_or_default(),
                            },
                        });
                    }
                }
            } else if self.in_wakeup && step_type == "GENERIC" && !self.pending_tools.is_empty() {
                let cid = self.pending_tools.remove(0);
                out.push(WakeupStep::Tool {
                    call_id: cid,
                    status: if status == "ERROR" {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    },
                    detail: ToolCallDetail::Generic {
                        tool_name: "result".into(),
                        arguments_json: truncate_preview(content, 400),
                    },
                });
            }
        }

        self.byte_offset += consumed_bytes;
        out
    }
}

pub fn resolve_agy_data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ANTIGRAVITY_EXECUTABLE_DATA_DIR")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir);
    }
    let rel = std::env::var("ANTIGRAVITY_CONFIG_REL_DIR")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "antigravity-cli".into());
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".gemini")
        .join(rel)
}

// --- Quota & Credits Probing (`agy -p /usage --output-format json`) ---

#[derive(Deserialize)]
struct AgyUsageEnvelope {
    status: Option<String>,
    command: Option<AgyUsageCmd>,
}

#[derive(Deserialize)]
struct AgyUsageCmd {
    data: Option<AgyUsageData>,
}

#[derive(Deserialize)]
struct AgyUsageData {
    tier: Option<String>,
    plan: Option<String>,
    remaining_credits: Option<f64>,
    groups: Option<Vec<AgyUsageGroup>>,
}

#[derive(Deserialize)]
struct AgyUsageGroup {
    name: String,
    buckets: Option<Vec<AgyUsageBucket>>,
}

#[derive(Deserialize)]
struct AgyUsageBucket {
    name: String,
    remaining_fraction: Option<f32>,
    reset_time: Option<String>,
}

pub async fn probe_antigravity_quota(executable: &Path) -> ProviderQuotaSnapshot {
    let usage_fut = Command::new(executable)
        .args(["-p", "/usage", "--output-format", "json"])
        .env("NO_COLOR", "1")
        .output();
    let credits_fut = Command::new(executable)
        .args(["-p", "/credits", "--output-format", "json"])
        .env("NO_COLOR", "1")
        .output();

    let (u_res, c_res) = tokio::time::timeout(Duration::from_secs(18), async {
        tokio::join!(usage_fut, credits_fut)
    })
    .await
    .unwrap_or_else(|_| {
        (
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "agy /usage timed out",
            )),
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "agy /credits timed out",
            )),
        )
    });

    let mut account_label = None;
    let mut windows = Vec::new();
    let mut error = None;

    match u_res {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Ok(env) = serde_json::from_str::<AgyUsageEnvelope>(&stdout)
                && env.status.as_deref() == Some("SUCCESS")
                && let Some(data) = env.command.and_then(|c| c.data)
            {
                account_label = data.tier.or(data.plan);
                for grp in data.groups.unwrap_or_default() {
                    for b in grp.buckets.unwrap_or_default() {
                        let rem = b.remaining_fraction.unwrap_or(1.0).clamp(0.0, 1.0);
                        let used_pct = ((1.0 - rem) * 1000.0).round() / 10.0;
                        let resets_at = b
                            .reset_time
                            .as_deref()
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&Utc));
                        windows.push(QuotaWindow {
                            label: format!("{} · {}", grp.name, b.name),
                            used_percent: used_pct,
                            resets_at,
                        });
                    }
                }
            } else if !out.status.success() {
                error = Some(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
        }
        Err(e) => error = Some(e.to_string()),
    }

    let mut credits_remaining = None;
    if let Ok(out) = c_res
        && let Ok(env) = serde_json::from_slice::<AgyUsageEnvelope>(&out.stdout)
        && let Some(c) = env
            .command
            .and_then(|cmd| cmd.data)
            .and_then(|d| d.remaining_credits)
        && c > 0.0
    {
        credits_remaining = Some(c);
    }

    ProviderQuotaSnapshot {
        provider: ProviderKind::Antigravity,
        account_label,
        windows,
        credits_remaining,
        error,
        updated_at: Utc::now(),
    }
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}
