//! Native Rust driver for Google Antigravity (`agy acp --stdio` + `transcript_full.jsonl` monitor + `/usage`).

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
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
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use crate::attachments::{build_agy_prompt_blocks, is_path_within_dir};
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
    if let Some(eff) = cfg.effort {
        let eff_str = match eff {
            ReasoningEffort::Off => None,
            ReasoningEffort::Low => Some("low"),
            ReasoningEffort::Medium => Some("medium"),
            ReasoningEffort::High => Some("high"),
            ReasoningEffort::Xhigh => Some("xhigh"),
            ReasoningEffort::Max | ReasoningEffort::Ultra => Some("max"),
        };
        if let Some(s) = eff_str {
            env.insert("AGY_EFFORT".into(), s.into());
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ElicitationOptionMapping {
    label: String,
    wire_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ElicitationFieldMeta {
    key: String,
    is_multi: bool,
    options: Vec<ElicitationOptionMapping>,
    other_option_value: Option<String>,
    other_field_key: Option<String>,
}

struct PendingElicitation {
    rpc_id: Value,
    fields: Vec<ElicitationFieldMeta>,
}

#[derive(Default)]
struct AgyStreamSegmentState {
    turn_tag: String,
    step_seq: u64,
    active_msg_id: Option<String>,
    active_thought_id: Option<String>,
    msg_accum: HashMap<String, String>,
    thought_accum: HashMap<String, String>,
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
        warn!(agent_id = %cfg.agent_id, "Antigravity ACP initialize failed");
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
            warn!(agent_id = %cfg.agent_id, error = %e, "Antigravity session open failed");
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

    let available_model_ids: HashSet<String> = sess_val
        .pointer("/models/availableModels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("modelId").and_then(|s| s.as_str()))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let canonical_base_ids: HashSet<String> = sess_val
        .get("configOptions")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|opt| opt.get("id").and_then(|x| x.as_str()) == Some("model"))
        })
        .and_then(|opt| opt.get("options"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("value").and_then(|s| s.as_str()))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let raw_initial_model = sess_val
        .pointer("/models/currentModelId")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .or_else(|| cfg.model.clone());

    let mut active_effort = cfg.effort;
    let mut current_base_model = raw_initial_model
        .as_deref()
        .map(|m| normalize_agy_base_model_id(m, &canonical_base_ids))
        .or_else(|| cfg.model.clone());

    // Optional `session/set_model` when explicit model or effort was requested
    if (cfg.model.is_some() || cfg.effort.is_some())
        && let Some(ref base_m) = cfg.model.as_ref().or(current_base_model.as_ref()).cloned()
    {
        let wire_model_id = resolve_agy_wire_model_id(
            base_m,
            active_effort,
            &available_model_ids,
            &canonical_base_ids,
        );
        if let Ok(rx) = send_acp_req(
            &shim,
            &mut next_rpc_id,
            &mut pending_rpcs,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": wire_model_id
            }),
        )
        .await
            && let Ok(v) = wait_rpc_during_boot(&mut shim, &mut pending_rpcs, rx).await
            && let Some(m) = v.pointer("/models/currentModelId").and_then(|x| x.as_str())
        {
            current_base_model = Some(normalize_agy_base_model_id(m, &canonical_base_ids));
        }
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
            model: current_base_model.clone(),
        })
        .await;
    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
        .await;

    // Active runtime state
    let mut mode = cfg.mode.clone();
    let mut active_turn_id: Option<String> = None;
    let mut active_prompt_rpc_key: Option<String> = None;
    let mut seg_state = AgyStreamSegmentState::default();
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingPerm> = HashMap::new();
    let mut pending_elicits: HashMap<String, PendingElicitation> = HashMap::new();

    // Post-turn autonomous wakeup (`transcript_full.jsonl`) state
    let mut transcript_monitor = AgyTranscriptMonitor::new(&session_id);
    // Skip any historical steps already recorded on disk when attaching/resuming
    transcript_monitor.sync_silent();
    let mut synthesized_wakeup_tools: HashSet<String> = HashSet::new();
    let mut autonomous_turn_id: Option<String> = None;
    let mut deferred_prompt: Option<(String, String)> = None;
    let mut last_ask_question_raw: Option<Vec<QuestionItem>> = None;
    let mut transcript_poll = tokio::time::interval(Duration::from_millis(200));

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt { turn_id, prompt } => {
                        if active_prompt_rpc_key.is_some() {
                            if !pending_perms.is_empty() || !pending_elicits.is_empty() {
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "warning".into(),
                                        message: "Waiting for your permission or question response above. Please answer it (or stop the current turn) before sending a new prompt.".into(),
                                    })
                                    .await;
                            } else {
                                deferred_prompt = Some((turn_id, prompt));
                            }
                        } else if dispatch_agy_prompt(
                            &shim,
                            &session_id,
                            cfg.attachments_dir.as_deref(),
                            &mut next_rpc_id,
                            &mut active_turn_id,
                            &mut active_prompt_rpc_key,
                            &mut autonomous_turn_id,
                            &mut transcript_monitor,
                            &mut seg_state,
                            &mut last_ask_question_raw,
                            &evt_tx,
                            turn_id,
                            prompt,
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        for (_, p) in pending_perms.drain() {
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": {
                                        "outcome": "cancelled"
                                    }
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                        for (_, el) in pending_elicits.drain() {
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": el.rpc_id,
                                "result": {
                                    "action": "cancel"
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                        last_ask_question_raw = None;
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
                    DriverCommand::SetConfig { model, effort } => {
                        if let Some(m) = model {
                            current_base_model =
                                Some(normalize_agy_base_model_id(&m, &canonical_base_ids));
                        }
                        if effort.is_some() {
                            active_effort = effort;
                        }
                        if let Some(ref base_m) = current_base_model {
                            let wire_model_id = resolve_agy_wire_model_id(
                                base_m,
                                active_effort,
                                &available_model_ids,
                                &canonical_base_ids,
                            );
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_model",
                                "params": {
                                    "sessionId": session_id,
                                    "modelId": wire_model_id
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
                            let content = build_elicitation_response_content(&el.fields, &answers);
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
                                    pending_perms.clear();
                                    pending_elicits.clear();
                                    last_ask_question_raw = None;
                                    seg_state.active_msg_id = None;
                                    seg_state.active_thought_id = None;
                                    // Finalize any streaming message buffers
                                    for (mid, text) in seg_state.msg_accum.drain() {
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
                                    if let Some((next_turn_id, next_prompt)) = deferred_prompt.take() {
                                        if dispatch_agy_prompt(
                                            &shim,
                                            &session_id,
                                            cfg.attachments_dir.as_deref(),
                                            &mut next_rpc_id,
                                            &mut active_turn_id,
                                            &mut active_prompt_rpc_key,
                                            &mut autonomous_turn_id,
                                            &mut transcript_monitor,
                                            &mut seg_state,
                                            &mut last_ask_question_raw,
                                            &evt_tx,
                                            next_turn_id,
                                            next_prompt,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            break;
                                        }
                                    } else if let Some(err_obj) = val.get("error") {
                                        let msg = err_obj
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("Antigravity turn failed")
                                            .to_string();
                                        warn!(
                                            agent_id = %cfg.agent_id,
                                            session_id = %session_id,
                                            error = %msg,
                                            "Antigravity ACP session/prompt failed"
                                        );
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
                            if !(active_turn_id.is_none() && autonomous_turn_id.is_some())
                                && let Some(update) = val.pointer("/params/update")
                            {
                                handle_acp_session_update(
                                    update,
                                    &synthesized_wakeup_tools,
                                    &mut seg_state,
                                    &mut tool_cache,
                                    &mut last_ask_question_raw,
                                    &evt_tx,
                                )
                                .await;
                            }
                        } else if has_id && method == Some("session/request_permission") {
                            handle_acp_permission_request(
                                &val,
                                &mode,
                                cfg.attachments_dir.as_deref(),
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
                                last_ask_question_raw.as_deref(),
                                &mut pending_elicits,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    ShimFrame::Exited { code } => {
                        if code != Some(0) {
                            warn!(
                                agent_id = %cfg.agent_id,
                                session_id = %session_id,
                                exit_code = ?code,
                                "Antigravity shim process exited"
                            );
                        }
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
                            WakeupStep::Notice { level, message } => {
                                warn!(
                                    agent_id = %cfg.agent_id,
                                    session_id = %session_id,
                                    level = %level,
                                    notice = %message,
                                    "Antigravity post-turn wakeup reported runtime warning"
                                );
                                let _ = evt_tx
                                    .send(DriverEvent::Notice { level, message })
                                    .await;
                            }
                            WakeupStep::Failed { message } => {
                                warn!(
                                    agent_id = %cfg.agent_id,
                                    session_id = %session_id,
                                    error = %message,
                                    "Antigravity post-turn wakeup failed after retries"
                                );
                                autonomous_turn_id = None;
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                        message,
                                        recoverable: true,
                                    }))
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

#[allow(clippy::too_many_arguments)]
async fn dispatch_agy_prompt(
    shim: &orbit_shim::ShimClient,
    session_id: &str,
    attachments_dir: Option<&Path>,
    next_rpc_id: &mut i64,
    active_turn_id: &mut Option<String>,
    active_prompt_rpc_key: &mut Option<String>,
    autonomous_turn_id: &mut Option<String>,
    transcript_monitor: &mut AgyTranscriptMonitor,
    seg_state: &mut AgyStreamSegmentState,
    last_ask_question_raw: &mut Option<Vec<QuestionItem>>,
    evt_tx: &mpsc::Sender<DriverEvent>,
    turn_id: String,
    prompt: String,
) -> Result<(), ()> {
    if autonomous_turn_id.take().is_some() {
        transcript_monitor.sync_silent();
    }
    *active_turn_id = Some(turn_id.clone());
    *seg_state = AgyStreamSegmentState {
        turn_tag: turn_id.clone(),
        ..AgyStreamSegmentState::default()
    };
    *last_ask_question_raw = None;

    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Running { turn_id }))
        .await;

    let id = *next_rpc_id;
    *next_rpc_id += 1;
    let key = RpcId::Num(id).as_key();
    *active_prompt_rpc_key = Some(key);

    let prompt_blocks = build_agy_prompt_blocks(&prompt, attachments_dir);
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": prompt_blocks
        }
    })
    .to_string();
    shim.write_stdin_line(req).await.map_err(|_| ())
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
    synthesized_wakeup_tools: &HashSet<String>,
    seg_state: &mut AgyStreamSegmentState,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    last_ask_question_raw: &mut Option<Vec<QuestionItem>>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let Some(kind) = update.get("sessionUpdate").and_then(|v| v.as_str()) else {
        return;
    };

    if kind == "user_message_chunk" {
        return;
    }

    let turn_tag = if seg_state.turn_tag.is_empty() {
        "init".to_string()
    } else {
        seg_state.turn_tag.clone()
    };

    match kind {
        "agent_message_chunk" => {
            let chunk = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            let (warnings, cleaned_text) = split_acp_runtime_warnings(chunk);
            for warning_msg in warnings {
                warn!(notice = %warning_msg, "Antigravity ACP stream warning");
                let _ = evt_tx
                    .send(DriverEvent::Notice {
                        level: "warning".into(),
                        message: warning_msg,
                    })
                    .await;
            }
            if cleaned_text.is_empty() {
                return;
            }
            seg_state.active_thought_id = None;
            let mid = if let Some(explicit) = update
                .get("messageId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                format!("{turn_tag}_{explicit}")
            } else if let Some(ref cur) = seg_state.active_msg_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("agy_msg_{turn_tag}_{}", seg_state.step_seq);
                seg_state.active_msg_id = Some(id.clone());
                id
            };
            let entry = seg_state.msg_accum.entry(mid.clone()).or_default();
            entry.push_str(&cleaned_text);
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
            if let Some(prev_mid) = seg_state.active_msg_id.take()
                && let Some(prev_text) = seg_state.msg_accum.remove(&prev_mid)
            {
                let _ = evt_tx
                    .send(DriverEvent::AssistantText {
                        item_id: prev_mid,
                        text: prev_text,
                        streaming: false,
                    })
                    .await;
            }
            let tid = if let Some(explicit) = update
                .get("messageId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                format!("{turn_tag}_th_{explicit}")
            } else if let Some(ref cur) = seg_state.active_thought_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("agy_th_{turn_tag}_{}", seg_state.step_seq);
                seg_state.active_thought_id = Some(id.clone());
                id
            };
            let entry = seg_state.thought_accum.entry(tid.clone()).or_default();
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
            seg_state.active_thought_id = None;
            if let Some(prev_mid) = seg_state.active_msg_id.take()
                && let Some(prev_text) = seg_state.msg_accum.remove(&prev_mid)
            {
                let _ = evt_tx
                    .send(DriverEvent::AssistantText {
                        item_id: prev_mid,
                        text: prev_text,
                        streaming: false,
                    })
                    .await;
            }
            if let Some(q_arr) = update
                .pointer("/rawInput/questions")
                .and_then(|v| v.as_array())
                && !q_arr.is_empty()
            {
                *last_ask_question_raw = Some(parse_raw_question_items(q_arr));
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
                        crate::build_compact_unified_diff(&path, old_t, new_t)
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

/// Parses structured `ask_question` raw input items shared by permission-wrapped
/// questions and `session/update` tool-call fallbacks.
fn parse_raw_question_items(q_arr: &[Value]) -> Vec<QuestionItem> {
    let mut parsed_questions = Vec::with_capacity(q_arr.len());
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
                            .or_else(|| item.get("value").and_then(|l| l.as_str()))
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
        let header = q
            .get("header")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|h| !h.is_empty() && *h != question.trim())
            .map(ToOwned::to_owned);
        parsed_questions.push(QuestionItem {
            id: format!("q{i}"),
            header,
            question,
            options: opts,
            is_multi_select: multi,
        });
    }
    parsed_questions
}

async fn handle_acp_permission_request(
    val: &Value,
    mode: &SessionMode,
    attachments_dir: Option<&Path>,
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
        let parsed_questions = parse_raw_question_items(q_arr);
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

    // Auto-approve logic for staged attachment reads, `FullAccess`, or native file edits in `AcceptEdits`
    let kind_str = tool_call.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let is_attachment_read = kind_str == "read"
        && attachments_dir.is_some_and(|dir| {
            tool_call
                .get("rawInput")
                .and_then(|i| {
                    i.get("Path")
                        .or_else(|| i.get("AbsolutePath"))
                        .or_else(|| i.get("path"))
                })
                .and_then(|v| v.as_str())
                .or_else(|| {
                    tool_call
                        .pointer("/locations/0/path")
                        .and_then(|v| v.as_str())
                })
                .is_some_and(|p| is_path_within_dir(Path::new(p), dir))
        });
    let auto_approve = is_attachment_read
        || matches!(mode, SessionMode::FullAccess)
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

fn schema_scalar_to_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parses MCP/ACP `oneOf` / `anyOf` option entries into `(label, wire_value)` pairs.
fn parse_schema_choice_entries(arr: &[Value]) -> Vec<ElicitationOptionMapping> {
    arr.iter()
        .filter_map(|item| {
            let wire_val = item
                .get("const")
                .and_then(schema_scalar_to_string)
                .or_else(|| {
                    item.get("enum")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(schema_scalar_to_string)
                })?;
            let label = item
                .get("title")
                .or_else(|| item.get("description"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| wire_val.clone());
            Some(ElicitationOptionMapping {
                label,
                wire_value: wire_val,
            })
        })
        .collect()
}

/// Parses JSON Schema `enum` + optional legacy `enumNames` arrays into option mappings.
fn parse_schema_enum_entries(
    enum_arr: &[Value],
    names_arr: Option<&[Value]>,
) -> Vec<ElicitationOptionMapping> {
    enum_arr
        .iter()
        .enumerate()
        .filter_map(|(idx, val)| {
            let wire_val = schema_scalar_to_string(val)?;
            let label = names_arr
                .and_then(|names| names.get(idx))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| wire_val.clone());
            Some(ElicitationOptionMapping {
                label,
                wire_value: wire_val,
            })
        })
        .collect()
}

/// Extracts selectable options from either multi-select (`items.anyOf` / `items.oneOf` / `items.enum`)
/// or single-select (`oneOf` / `anyOf` / `enum`) JSON Schema property definitions.
fn extract_elicitation_options(prop: &Value, is_multi: bool) -> Vec<ElicitationOptionMapping> {
    if is_multi {
        if let Some(arr) = prop
            .pointer("/items/anyOf")
            .or_else(|| prop.pointer("/items/oneOf"))
            .and_then(|v| v.as_array())
        {
            let parsed = parse_schema_choice_entries(arr);
            if !parsed.is_empty() {
                return parsed;
            }
        }
        if let Some(arr) = prop.pointer("/items/enum").and_then(|v| v.as_array()) {
            let names = prop
                .pointer("/items/enumNames")
                .and_then(|v| v.as_array())
                .map(Vec::as_slice);
            let parsed = parse_schema_enum_entries(arr, names);
            if !parsed.is_empty() {
                return parsed;
            }
        }
    }

    if let Some(arr) = prop
        .get("oneOf")
        .or_else(|| prop.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        let parsed = parse_schema_choice_entries(arr);
        if !parsed.is_empty() {
            return parsed;
        }
    }

    if let Some(arr) = prop.get("enum").and_then(|v| v.as_array()) {
        let names = prop
            .get("enumNames")
            .and_then(|v| v.as_array())
            .map(Vec::as_slice);
        return parse_schema_enum_entries(arr, names);
    }

    Vec::new()
}

/// Computes a natural sort key (`q1`, `q2`, ..., `q10`) so `BTreeMap`-backed schema maps keep field order.
fn natural_field_sort_key(key: &str) -> (&str, Option<u64>, &str) {
    let digit_start = key
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let prefix = &key[..digit_start];
    let num = (!key[digit_start..].is_empty())
        .then(|| key[digit_start..].parse::<u64>().ok())
        .flatten();
    (prefix, num, key)
}

/// Parses an ACP `elicitation/create` parameter object into Orbit `QuestionItem`s plus field metadata
/// needed to map user selections and custom answers back to the exact schema wire format.
fn parse_elicitation_request(
    params: &Value,
    fallback_questions: Option<&[QuestionItem]>,
) -> Option<(Vec<QuestionItem>, Vec<ElicitationFieldMeta>)> {
    let props = params
        .pointer("/requestedSchema/properties")
        .and_then(|v| v.as_object())?;

    let mut required_order: HashMap<&str, usize> = HashMap::new();
    if let Some(req_arr) = params
        .pointer("/requestedSchema/required")
        .and_then(|v| v.as_array())
    {
        for (idx, item) in req_arr.iter().enumerate() {
            if let Some(k) = item.as_str() {
                required_order.entry(k).or_insert(idx);
            }
        }
    }

    let mut base_keys: Vec<&str> = props
        .keys()
        .map(String::as_str)
        .filter(|k| !k.ends_with("_other"))
        .collect();
    base_keys.sort_by_key(|k| {
        (
            required_order.get(k).copied().unwrap_or(usize::MAX),
            natural_field_sort_key(k),
        )
    });

    let mut questions = Vec::with_capacity(base_keys.len());
    let mut fields = Vec::with_capacity(base_keys.len());

    for (idx, key) in base_keys.iter().enumerate() {
        let Some(prop) = props.get(*key) else {
            continue;
        };
        let is_multi = prop.get("type").and_then(|v| v.as_str()) == Some("array");
        let title = prop
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let desc = prop
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let (header, q_text) = match (title, desc) {
            (Some(t), Some(d)) if t != d => (Some(t.to_string()), d.to_string()),
            _ => (None, desc.or(title).unwrap_or(key).to_string()),
        };

        let mut raw_opts = extract_elicitation_options(prop, is_multi);
        if raw_opts.is_empty()
            && let Some(fb) = fallback_questions
            && let Some(matched) = fb
                .iter()
                .find(|item| !item.question.trim().is_empty() && item.question.trim() == q_text)
                .or_else(|| (fb.len() == base_keys.len()).then(|| &fb[idx]))
        {
            raw_opts = matched
                .options
                .iter()
                .map(|opt| ElicitationOptionMapping {
                    label: opt.clone(),
                    wire_value: opt.clone(),
                })
                .collect();
        }

        let other_key = format!("{key}_other");
        let mut other_option_value = None;
        let mut other_field_key = None;
        if props.contains_key(&other_key) {
            other_field_key = Some(other_key);
            let mut filtered = Vec::with_capacity(raw_opts.len());
            for opt in raw_opts {
                let is_other_sentinel = opt.wire_value.eq_ignore_ascii_case("other")
                    || opt.wire_value == "__other__"
                    || opt.label.eq_ignore_ascii_case("other");
                if is_other_sentinel {
                    if other_option_value.is_none() {
                        other_option_value = Some(opt.wire_value);
                    }
                } else {
                    filtered.push(opt);
                }
            }
            raw_opts = filtered;
        }

        let display_options = raw_opts.iter().map(|o| o.label.clone()).collect();
        fields.push(ElicitationFieldMeta {
            key: (*key).to_string(),
            is_multi,
            options: raw_opts,
            other_option_value,
            other_field_key,
        });
        questions.push(QuestionItem {
            id: (*key).to_string(),
            header,
            question: q_text,
            options: display_options,
            is_multi_select: is_multi,
        });
    }

    (!questions.is_empty()).then_some((questions, fields))
}

/// Converts user UI answers (labels or free-text) into the wire `content` object expected by ACP
/// `elicitation/create`, routing free-text through `<key>_other` when declared in the schema.
fn build_elicitation_response_content(
    fields: &[ElicitationFieldMeta],
    answers: &[Vec<String>],
) -> serde_json::Map<String, Value> {
    let mut content = serde_json::Map::new();
    for (idx, field) in fields.iter().enumerate() {
        let raw_vals = answers.get(idx).map(Vec::as_slice).unwrap_or(&[]);
        let mut mapped_values: Vec<String> = Vec::new();
        let mut custom_texts: Vec<String> = Vec::new();

        for ans in raw_vals {
            let trimmed = ans.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(found) = field
                .options
                .iter()
                .find(|o| o.wire_value == trimmed || o.label == trimmed)
            {
                if !mapped_values.contains(&found.wire_value) {
                    mapped_values.push(found.wire_value.clone());
                }
            } else {
                custom_texts.push(trimmed.to_string());
            }
        }

        if !custom_texts.is_empty() {
            let joined_custom = custom_texts.join(", ");
            if let (Some(other_val), Some(other_key)) =
                (&field.other_option_value, &field.other_field_key)
            {
                if !field.is_multi {
                    mapped_values.clear();
                }
                if !mapped_values.contains(other_val) {
                    mapped_values.push(other_val.clone());
                }
                content.insert(other_key.clone(), Value::String(joined_custom));
            } else if field.is_multi {
                for c in custom_texts {
                    if !mapped_values.contains(&c) {
                        mapped_values.push(c);
                    }
                }
            } else {
                mapped_values.clear();
                mapped_values.push(joined_custom);
            }
        }

        if field.is_multi {
            content.insert(
                field.key.clone(),
                Value::Array(mapped_values.into_iter().map(Value::String).collect()),
            );
        } else {
            content.insert(
                field.key.clone(),
                Value::String(mapped_values.into_iter().next().unwrap_or_default()),
            );
        }
    }
    content
}

async fn handle_acp_elicitation_request(
    val: &Value,
    active_turn_id: &Option<String>,
    fallback_questions: Option<&[QuestionItem]>,
    pending_elicits: &mut HashMap<String, PendingElicitation>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val["id"].clone();
    let Some((questions, fields)) = parse_elicitation_request(&val["params"], fallback_questions)
    else {
        return;
    };

    let req_id = format!("elicit_{}", uuid::Uuid::new_v4().simple());
    pending_elicits.insert(req_id.clone(), PendingElicitation { rpc_id, fields });

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
    Notice {
        level: String,
        message: String,
    },
    Failed {
        message: String,
    },
    Settled,
}

const WAKEUP_ERROR_STALL_TIMEOUT: Duration = Duration::from_secs(90);
const WAKEUP_ERROR_EXHAUSTED_TIMEOUT: Duration = Duration::from_secs(15);
const WAKEUP_ERROR_EXHAUSTED_COUNT: u32 = 8;

/// Split provider-injected runtime warning lines (e.g. `\n⚠ API error (attempt 1): ...` or
/// `\n⚠ The stream was interrupted...`) out of an ACP `agent_message_chunk` so they surface as
/// visible `Notice` alerts rather than getting merged/folded as assistant prose.
fn split_acp_runtime_warnings(chunk: &str) -> (Vec<String>, String) {
    if !chunk.contains('⚠') {
        return (Vec::new(), chunk.to_string());
    }

    let mut warnings = Vec::new();
    let mut kept_lines = Vec::new();

    for segment in chunk.split_inclusive('\n') {
        let trimmed = segment.trim();
        if let Some(rest) = trimmed.strip_prefix('⚠') {
            let clean = rest.trim();
            if is_acp_runtime_warning_body(clean) {
                warnings.push(clean.to_string());
                continue;
            }
        }
        kept_lines.push(segment);
    }

    if warnings.is_empty() {
        return (Vec::new(), chunk.to_string());
    }

    let kept = kept_lines.concat();
    if kept.trim().is_empty() {
        (warnings, String::new())
    } else {
        (warnings, kept)
    }
}

fn is_acp_runtime_warning_body(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.starts_with("api error")
        || lower.starts_with("the stream was interrupted")
        || lower.starts_with("rate limit")
        || lower.starts_with("request failed")
        || lower.starts_with("connection ")
        || lower.starts_with("network ")
        || lower.starts_with("retrying")
        || lower.starts_with("error:")
}

/// Unwrap double-encoded JSON string arguments emitted in `transcript.jsonl` `tool_calls[].args`.
fn unquote_transcript_arg(val: &Value) -> Option<String> {
    if let Some(s) = val.as_str() {
        let trimmed = s.trim();
        if trimmed.starts_with('"')
            && trimmed.ends_with('"')
            && trimmed.len() >= 2
            && let Ok(parsed) = serde_json::from_str::<String>(trimmed)
        {
            return Some(parsed);
        }
        return Some(s.to_string());
    }
    if val.is_number() || val.is_boolean() {
        return Some(val.to_string());
    }
    None
}

fn transcript_arg_str(args: Option<&Value>, keys: &[&str]) -> Option<String> {
    let obj = args?.as_object()?;
    for key in keys {
        if let Some(val) = obj.get(*key).and_then(unquote_transcript_arg)
            && !val.trim().is_empty()
        {
            return Some(val);
        }
    }
    None
}

fn transcript_arg_u32(args: Option<&Value>, keys: &[&str]) -> Option<u32> {
    let obj = args?.as_object()?;
    for key in keys {
        if let Some(v) = obj.get(*key) {
            if let Some(n) = v.as_u64() {
                return u32::try_from(n).ok();
            }
            if let Some(s) = unquote_transcript_arg(v)
                && let Ok(n) = s.trim().parse::<u32>()
            {
                return Some(n);
            }
        }
    }
    None
}

fn map_transcript_tool_detail(tool_name: &str, args: Option<&Value>) -> ToolCallDetail {
    let lower = tool_name.to_ascii_lowercase();

    if matches!(
        lower.as_str(),
        "run_command" | "execute_command" | "bash" | "shell" | "terminal"
    ) {
        let cmd = transcript_arg_str(args, &["CommandLine", "command", "cmd"])
            .unwrap_or_else(|| tool_name.to_string());
        let cwd = transcript_arg_str(args, &["Cwd", "cwd"]);
        return ToolCallDetail::Shell {
            command: cmd,
            cwd,
            exit_code: None,
            output_preview: None,
        };
    }

    if matches!(lower.as_str(), "read_file" | "view_file" | "open_file") {
        let path = transcript_arg_str(args, &["Path", "AbsolutePath", "file_path", "path"])
            .unwrap_or_else(|| tool_name.to_string());
        let start_line = transcript_arg_u32(args, &["StartLine", "start_line", "offset"]);
        let end_line = transcript_arg_u32(args, &["EndLine", "end_line"]);
        return ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        };
    }

    if matches!(
        lower.as_str(),
        "edit_file"
            | "write_file"
            | "replace_file_content"
            | "multi_replace_file_content"
            | "write_to_file"
    ) {
        let path = transcript_arg_str(args, &["Path", "TargetFile", "file_path", "path"])
            .unwrap_or_else(|| tool_name.to_string());
        let unified_diff = match (
            transcript_arg_str(args, &["Target", "old_string"]),
            transcript_arg_str(args, &["Replacement", "new_string", "Content"]),
        ) {
            (Some(old_t), Some(new_t)) => crate::build_compact_unified_diff(&path, &old_t, &new_t),
            (None, Some(new_t)) if lower.starts_with("write") => {
                crate::build_compact_unified_diff(&path, "", &new_t)
            }
            _ => None,
        };
        return ToolCallDetail::FileEdit { path, unified_diff };
    }

    if matches!(
        lower.as_str(),
        "search_web" | "grep_search" | "find_by_name" | "grep" | "glob" | "search"
    ) {
        let query = transcript_arg_str(args, &["query", "Query", "Pattern", "pattern"])
            .unwrap_or_else(|| tool_name.to_string());
        let scope = transcript_arg_str(
            args,
            &["domain", "SearchPath", "SearchDirectory", "path", "scope"],
        );
        return ToolCallDetail::Search { query, scope };
    }

    if lower.contains("subagent") {
        let role = transcript_arg_str(args, &["Role", "TypeName", "role"])
            .unwrap_or_else(|| "subagent".into());
        let prompt = transcript_arg_str(args, &["Prompt", "prompt"]).unwrap_or_default();
        return ToolCallDetail::Subagent {
            child_agent_id: None,
            role,
            prompt_preview: truncate_preview(&prompt, 240),
        };
    }

    ToolCallDetail::Generic {
        tool_name: tool_name.to_string(),
        arguments_json: args
            .map(|v| truncate_preview(&v.to_string(), 400))
            .unwrap_or_default(),
    }
}

fn attach_transcript_tool_result(mut detail: ToolCallDetail, content: &str) -> ToolCallDetail {
    let preview = (!content.trim().is_empty()).then(|| truncate_preview(content.trim(), 800));
    match &mut detail {
        ToolCallDetail::Shell {
            output_preview,
            exit_code,
            ..
        } => {
            if preview.is_some() {
                *output_preview = preview;
            }
            if let Some(code_line) = content
                .lines()
                .find(|l| l.starts_with("Command exited with code "))
                && let Some(num_str) = code_line
                    .strip_prefix("Command exited with code ")
                    .map(|s| s.trim_end_matches('.').trim())
                && let Ok(code) = num_str.parse::<i32>()
            {
                *exit_code = Some(code);
            }
        }
        ToolCallDetail::Generic { arguments_json, .. } => {
            if arguments_json.is_empty()
                && let Some(p) = preview
            {
                *arguments_json = p;
            }
        }
        _ => {}
    }
    detail
}

struct AgyTranscriptMonitor {
    full_path: PathBuf,
    compact_path: PathBuf,
    byte_offset: u64,
    seen_steps: HashSet<u64>,
    in_wakeup: bool,
    pending_tools: Vec<(String, ToolCallDetail)>,
    consecutive_errors: u32,
    last_error_at: Option<Instant>,
    last_error_message: Option<String>,
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
            consecutive_errors: 0,
            last_error_at: None,
            last_error_message: None,
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
        self.consecutive_errors = 0;
        self.last_error_at = None;
        self.last_error_message = None;
    }

    fn poll_post_turn_wakeup(&mut self) -> Vec<WakeupStep> {
        let mut out = self.read_new_lines(false);
        self.check_error_stall(&mut out, Instant::now());
        out
    }

    fn check_error_stall(&mut self, out: &mut Vec<WakeupStep>, now: Instant) {
        if !self.in_wakeup || self.consecutive_errors == 0 {
            return;
        }
        let Some(last_err) = self.last_error_at else {
            return;
        };
        let elapsed = now.saturating_duration_since(last_err);
        let timed_out = elapsed >= WAKEUP_ERROR_STALL_TIMEOUT
            || (self.consecutive_errors >= WAKEUP_ERROR_EXHAUSTED_COUNT
                && elapsed >= WAKEUP_ERROR_EXHAUSTED_TIMEOUT);
        if timed_out {
            let reason = self
                .last_error_message
                .take()
                .unwrap_or_else(|| "Background task stalled after repeated API errors".into());
            let attempts = self.consecutive_errors;
            self.in_wakeup = false;
            self.pending_tools.clear();
            self.consecutive_errors = 0;
            self.last_error_at = None;
            out.push(WakeupStep::Failed {
                message: format!("Background wakeup stopped after {attempts} error(s): {reason}"),
            });
        }
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
                    self.consecutive_errors = 0;
                    self.last_error_at = None;
                    self.last_error_message = None;
                    out.push(WakeupStep::Started {
                        source: "background_task".into(),
                    });
                }
            } else if step_type == "ERROR_MESSAGE" {
                let err_msg = step
                    .get("error")
                    .or_else(|| step.get("content"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Upstream API request failed during background task");
                self.consecutive_errors = self.consecutive_errors.saturating_add(1);
                self.last_error_at = Some(Instant::now());
                self.last_error_message = Some(err_msg.to_string());
                out.push(WakeupStep::Notice {
                    level: if self.consecutive_errors >= WAKEUP_ERROR_EXHAUSTED_COUNT {
                        "error".into()
                    } else {
                        "warning".into()
                    },
                    message: err_msg.to_string(),
                });
            } else if self.in_wakeup && step_type == "PLANNER_RESPONSE" {
                self.consecutive_errors = 0;
                self.last_error_at = None;
                self.last_error_message = None;
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
                    self.pending_tools.clear();
                    out.push(WakeupStep::Settled);
                } else {
                    for (call_idx, c) in calls.into_iter().enumerate() {
                        let cid = c
                            .get("id")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.trim().is_empty())
                            .map(ToOwned::to_owned)
                            .unwrap_or_else(|| format!("wake_tool_{step_idx}_{call_idx}"));
                        let tname = c.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                        let detail = map_transcript_tool_detail(tname, c.get("args"));
                        self.pending_tools.push((cid.clone(), detail.clone()));
                        out.push(WakeupStep::Tool {
                            call_id: cid,
                            status: ToolCallStatus::Running,
                            detail,
                        });
                    }
                }
            } else if self.in_wakeup && step_type == "GENERIC" && !self.pending_tools.is_empty() {
                self.consecutive_errors = 0;
                self.last_error_at = None;
                self.last_error_message = None;
                let (cid, orig_detail) = self.pending_tools.remove(0);
                let detail = attach_transcript_tool_result(orig_detail, content);
                out.push(WakeupStep::Tool {
                    call_id: cid,
                    status: if status == "ERROR" {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    },
                    detail,
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

// --- Antigravity ACP Model & Thinking Catalog Probing (`agy acp --stdio` `session/new`) ---

fn strip_trailing_effort_suffix(model_id: &str) -> (&str, Option<ReasoningEffort>) {
    for (suffix, eff) in [
        ("-xhigh", ReasoningEffort::Xhigh),
        ("-ultra", ReasoningEffort::Ultra),
        ("-medium", ReasoningEffort::Medium),
        ("-high", ReasoningEffort::High),
        ("-low", ReasoningEffort::Low),
        ("-max", ReasoningEffort::Max),
    ] {
        if let Some(base) = model_id.strip_suffix(suffix) {
            return (base, Some(eff));
        }
    }
    (model_id, None)
}

fn normalize_agy_base_model_id(raw_id: &str, canonical_bases: &HashSet<String>) -> String {
    if canonical_bases.contains(raw_id) {
        return raw_id.to_string();
    }
    let (stripped, _) = strip_trailing_effort_suffix(raw_id);
    if canonical_bases.is_empty() || canonical_bases.contains(stripped) {
        return stripped.to_string();
    }
    raw_id.to_string()
}

fn resolve_agy_wire_model_id(
    wanted_model: &str,
    effort: Option<ReasoningEffort>,
    available_ids: &HashSet<String>,
    canonical_bases: &HashSet<String>,
) -> String {
    if available_ids.is_empty() {
        return wanted_model.to_string();
    }
    let base = normalize_agy_base_model_id(wanted_model, canonical_bases);
    if let Some(eff) = effort {
        let candidate_suffixes: &[&str] = match eff {
            ReasoningEffort::Off | ReasoningEffort::Low => &["low", "medium"],
            ReasoningEffort::Medium => &["medium", "high", "low"],
            ReasoningEffort::High => &["high", "medium"],
            ReasoningEffort::Xhigh | ReasoningEffort::Max | ReasoningEffort::Ultra => {
                &["xhigh", "max", "high", "medium"]
            }
        };
        for suf in candidate_suffixes {
            let candidate = format!("{base}-{suf}");
            if available_ids.contains(&candidate) {
                return candidate;
            }
        }
    }
    if available_ids.contains(wanted_model) {
        return wanted_model.to_string();
    }
    for fallback_suf in ["medium", "high", "low"] {
        let candidate = format!("{base}-{fallback_suf}");
        if available_ids.contains(&candidate) {
            return candidate;
        }
    }
    wanted_model.to_string()
}

struct AgyFallbackRow<'a> {
    id: &'static str,
    label: &'static str,
    desc: &'static str,
    is_default: bool,
    default_effort: Option<ReasoningEffort>,
    efforts: &'a [ReasoningEffort],
}

pub fn fallback_antigravity_models() -> ProviderModelCatalog {
    let std_efforts = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];
    let xhigh_efforts = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
        ReasoningEffort::Xhigh,
    ];

    let rows = [
        AgyFallbackRow {
            id: "gemini-4-carbon",
            label: "Gemini 4 Carbon",
            desc: "Flagship reasoning & multi-file software engineering",
            is_default: true,
            default_effort: Some(ReasoningEffort::Medium),
            efforts: &std_efforts,
        },
        AgyFallbackRow {
            id: "gemini-4-argon",
            label: "Gemini 4 Argon",
            desc: "Balanced coding, architecture & tool orchestration",
            is_default: false,
            default_effort: Some(ReasoningEffort::Medium),
            efforts: &std_efforts,
        },
        AgyFallbackRow {
            id: "gemini-4-hydrogen",
            label: "Gemini 4 Hydrogen",
            desc: "Extended deep reasoning with Extra High budget",
            is_default: false,
            default_effort: Some(ReasoningEffort::High),
            efforts: &xhigh_efforts,
        },
        AgyFallbackRow {
            id: "gemini-pro-orchestrator",
            label: "Orchestrator (Carbon)",
            desc: "Multi-agent coordinator powered by Carbon",
            is_default: false,
            default_effort: None,
            efforts: &[],
        },
        AgyFallbackRow {
            id: "gemini-pro-orchestrator-argon",
            label: "Orchestrator (Argon)",
            desc: "Multi-agent coordinator powered by Argon",
            is_default: false,
            default_effort: None,
            efforts: &[],
        },
        AgyFallbackRow {
            id: "gemini-3.8-flash",
            label: "Gemini 3.8 Flash",
            desc: "High-speed interactive coding & rapid iteration",
            is_default: false,
            default_effort: Some(ReasoningEffort::Medium),
            efforts: &std_efforts,
        },
        AgyFallbackRow {
            id: "claude-opus-5-5",
            label: "Claude Opus 5.5",
            desc: "Anthropic flagship model on Antigravity",
            is_default: false,
            default_effort: Some(ReasoningEffort::Medium),
            efforts: &std_efforts,
        },
        AgyFallbackRow {
            id: "claude-sonnet-5-5",
            label: "Claude Sonnet 5.5",
            desc: "Fast everyday coding & refactoring",
            is_default: false,
            default_effort: Some(ReasoningEffort::Medium),
            efforts: &std_efforts,
        },
    ];

    let models = rows
        .iter()
        .map(|row| ProviderModelSpec {
            id: row.id.to_string(),
            label: row.label.to_string(),
            description: row.desc.to_string(),
            is_default: row.is_default,
            default_effort: row.default_effort,
            thinking_options: row
                .efforts
                .iter()
                .map(|e| ThinkingOptionSpec {
                    effort: *e,
                    label: e.display_label().to_string(),
                    description: None,
                    is_default: Some(*e) == row.default_effort,
                })
                .collect(),
        })
        .collect();

    ProviderModelCatalog {
        provider: ProviderKind::Antigravity,
        models,
        configured_default_model: Some("gemini-4-carbon".to_string()),
        configured_default_effort: Some(ReasoningEffort::Medium),
    }
}

pub async fn probe_antigravity_models(executable: Option<&Path>) -> ProviderModelCatalog {
    let Some(exe) = executable else {
        return fallback_antigravity_models();
    };

    let probe_cwd = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let rpc_res = tokio::time::timeout(Duration::from_secs(6), async {
        let mut child = Command::new(exe)
            .args(["acp", "--stdio"])
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;

        let Some(mut stdin) = child.stdin.take() else {
            return Err("agy stdin unavailable".to_string());
        };
        let Some(stdout) = child.stdout.take() else {
            return Err("agy stdout unavailable".to_string());
        };
        let mut lines = BufReader::new(stdout).lines();

        let init_req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientCapabilities": {},
                "clientInfo": {
                    "name": "orbit_model_probe",
                    "version": env!("CARGO_PKG_VERSION")
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
                && v.get("id").and_then(|x| x.as_i64()) == Some(1)
            {
                break;
            }
        }

        let new_req = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/new",
            "params": {
                "cwd": probe_cwd.to_string_lossy(),
                "mcpServers": []
            }
        });
        stdin
            .write_all(format!("{new_req}\n").as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())?;

        let mut sess_result: Option<Value> = None;
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && v.get("id").and_then(|x| x.as_i64()) == Some(2)
            {
                sess_result = v.get("result").cloned();
                break;
            }
        }

        // Best-effort close temporary probe session cleanly before terminating
        if let Some(sid) = sess_result
            .as_ref()
            .and_then(|r| r.get("sessionId"))
            .and_then(|v| v.as_str())
        {
            let close_req = json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "session/close",
                "params": { "sessionId": sid }
            });
            let _ = stdin.write_all(format!("{close_req}\n").as_bytes()).await;
            let _ = stdin.flush().await;
        }

        let _ = child.kill().await;
        Ok::<Option<Value>, String>(sess_result)
    })
    .await;

    let Ok(Ok(Some(res_val))) = rpc_res else {
        return fallback_antigravity_models();
    };

    let available_ids: Vec<String> = res_val
        .pointer("/models/availableModels")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("modelId").and_then(|x| x.as_str()))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();

    let config_options = res_val
        .get("configOptions")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let model_opt = config_options
        .iter()
        .find(|o| o.get("id").and_then(|x| x.as_str()) == Some("model"));
    let thought_opt = config_options
        .iter()
        .find(|o| o.get("id").and_then(|x| x.as_str()) == Some("thought_level"));

    let default_model_id = model_opt
        .and_then(|o| o.get("currentValue"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);
    let default_effort = thought_opt
        .and_then(|o| o.get("currentValue"))
        .and_then(|v| v.as_str())
        .and_then(ReasoningEffort::from_slug)
        .or(Some(ReasoningEffort::Medium));

    let mut models = Vec::new();
    if let Some(opts) = model_opt
        .and_then(|o| o.get("options"))
        .and_then(|v| v.as_array())
    {
        for opt in opts {
            let Some(base_id) = opt
                .get("value")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
            let label = opt
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(base_id)
                .to_string();
            let description = opt
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            // Discover exact supported efforts for this base model from `availableModels`
            let mut supported_efforts = Vec::new();
            for (suf, eff) in [
                ("low", ReasoningEffort::Low),
                ("medium", ReasoningEffort::Medium),
                ("high", ReasoningEffort::High),
                ("xhigh", ReasoningEffort::Xhigh),
                ("max", ReasoningEffort::Max),
            ] {
                let candidate = format!("{base_id}-{suf}");
                if available_ids.iter().any(|aid| aid == &candidate) {
                    supported_efforts.push(eff);
                }
            }

            let model_def_eff = if supported_efforts.is_empty() {
                None
            } else if let Some(de) = default_effort
                && supported_efforts.contains(&de)
            {
                Some(de)
            } else {
                supported_efforts.first().copied()
            };

            let thinking_options = supported_efforts
                .into_iter()
                .map(|eff| ThinkingOptionSpec {
                    effort: eff,
                    label: eff.display_label().to_string(),
                    description: None,
                    is_default: Some(eff) == model_def_eff,
                })
                .collect();

            models.push(ProviderModelSpec {
                id: base_id.to_string(),
                label,
                description,
                is_default: default_model_id.as_deref() == Some(base_id),
                default_effort: model_def_eff,
                thinking_options,
            });
        }
    }

    if models.is_empty() {
        return fallback_antigravity_models();
    }
    if !models.iter().any(|m| m.is_default)
        && let Some(first) = models.first_mut()
    {
        first.is_default = true;
    }

    ProviderModelCatalog {
        provider: ProviderKind::Antigravity,
        models,
        configured_default_model: default_model_id,
        configured_default_effort: default_effort,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_elicitation_supports_one_of_any_of_enum_names_natural_order_and_fallback() {
        let params = json!({
            "requestedSchema": {
                "type": "object",
                "required": ["q2", "q1", "q10"],
                "properties": {
                    "q10": {
                        "type": "string",
                        "title": "Same text",
                        "description": "Same text",
                        "enum": ["alpha", "beta"],
                        "enumNames": ["Alpha Option", "Beta Option"]
                    },
                    "q1": {
                        "type": "string",
                        "title": "Layout Style",
                        "description": "Pick layout style",
                        "oneOf": [
                            { "const": "opt_a", "title": "(Recommended) Single group" },
                            { "const": "opt_b", "title": "Split groups" },
                            { "const": "__other__", "title": "Other" }
                        ]
                    },
                    "q1_other": {
                        "type": "string",
                        "title": "Custom answer"
                    },
                    "q2": {
                        "type": "array",
                        "description": "Select targets",
                        "items": {
                            "anyOf": [
                                { "const": "desktop", "title": "Desktop" },
                                { "const": "ios", "title": "iOS" },
                                { "const": "other", "title": "Other" }
                            ]
                        }
                    },
                    "q2_other": {
                        "type": "string"
                    }
                }
            }
        });

        let (questions, fields) = parse_elicitation_request(&params, None).expect("parse ok");
        let ids: Vec<&str> = questions.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(ids, vec!["q2", "q1", "q10"]);

        // Multi-select `items.anyOf` with sibling `_other` sentinel stripped from visible options
        assert!(questions[0].is_multi_select);
        assert_eq!(questions[0].options, vec!["Desktop", "iOS"]);
        assert_eq!(fields[0].other_option_value.as_deref(), Some("other"));
        assert_eq!(fields[0].other_field_key.as_deref(), Some("q2_other"));

        // Single-select `oneOf` with header != question and `__other__` stripped
        assert!(!questions[1].is_multi_select);
        assert_eq!(questions[1].header.as_deref(), Some("Layout Style"));
        assert_eq!(questions[1].question, "Pick layout style");
        assert_eq!(
            questions[1].options,
            vec!["(Recommended) Single group", "Split groups"]
        );
        assert_eq!(fields[1].other_option_value.as_deref(), Some("__other__"));

        // Identical title & description deduplicates `header`
        assert_eq!(questions[2].header, None);
        assert_eq!(questions[2].question, "Same text");
        assert_eq!(questions[2].options, vec!["Alpha Option", "Beta Option"]);

        // Fallback when schema properties omit options
        let no_opts_params = json!({
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "q1": { "type": "string", "title": "Choose strategy" }
                }
            }
        });
        let fb = vec![QuestionItem {
            id: "q0".into(),
            header: None,
            question: "Choose strategy".into(),
            options: vec!["Fast".into(), "Safe".into()],
            is_multi_select: false,
        }];
        let (fb_qs, _) =
            parse_elicitation_request(&no_opts_params, Some(&fb)).expect("fallback parse ok");
        assert_eq!(fb_qs[0].options, vec!["Fast", "Safe"]);

        // Natural numeric sort (`q1`, `q2`, `q10`) when `required` is absent
        let natural_params = json!({
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "q10": { "type": "string" },
                    "q2": { "type": "string" },
                    "q1": { "type": "string" }
                }
            }
        });
        let (nat_qs, _) = parse_elicitation_request(&natural_params, None).expect("natural order");
        let nat_ids: Vec<&str> = nat_qs.iter().map(|q| q.id.as_str()).collect();
        assert_eq!(nat_ids, vec!["q1", "q2", "q10"]);
    }

    #[test]
    fn build_elicitation_response_maps_labels_and_routes_custom_other() {
        let fields = vec![
            ElicitationFieldMeta {
                key: "q1".into(),
                is_multi: false,
                options: vec![
                    ElicitationOptionMapping {
                        label: "Option A".into(),
                        wire_value: "const_a".into(),
                    },
                    ElicitationOptionMapping {
                        label: "Option B".into(),
                        wire_value: "const_b".into(),
                    },
                ],
                other_option_value: Some("__other__".into()),
                other_field_key: Some("q1_other".into()),
            },
            ElicitationFieldMeta {
                key: "q2".into(),
                is_multi: true,
                options: vec![
                    ElicitationOptionMapping {
                        label: "Desktop".into(),
                        wire_value: "desktop".into(),
                    },
                    ElicitationOptionMapping {
                        label: "iOS".into(),
                        wire_value: "ios".into(),
                    },
                ],
                other_option_value: Some("other".into()),
                other_field_key: Some("q2_other".into()),
            },
            ElicitationFieldMeta {
                key: "q3".into(),
                is_multi: false,
                options: vec![],
                other_option_value: None,
                other_field_key: None,
            },
        ];

        // Preset label maps to const `wire_value`; multi-select mixes preset + custom; plain free text
        let content_mixed = build_elicitation_response_content(
            &fields,
            &[
                vec!["Option A".into()],
                vec!["Desktop".into(), "HarmonyOS".into()],
                vec!["Keep concise".into()],
            ],
        );
        assert_eq!(content_mixed.get("q1"), Some(&json!("const_a")));
        assert!(!content_mixed.contains_key("q1_other"));
        assert_eq!(content_mixed.get("q2"), Some(&json!(["desktop", "other"])));
        assert_eq!(content_mixed.get("q2_other"), Some(&json!("HarmonyOS")));
        assert_eq!(content_mixed.get("q3"), Some(&json!("Keep concise")));

        // Single-select custom answer routes sentinel into `q1` and text into `q1_other`
        let content_custom_single = build_elicitation_response_content(
            &fields,
            &[vec!["Use custom folding".into()], vec![], vec![]],
        );
        assert_eq!(content_custom_single.get("q1"), Some(&json!("__other__")));
        assert_eq!(
            content_custom_single.get("q1_other"),
            Some(&json!("Use custom folding"))
        );
    }

    #[tokio::test]
    async fn acp_session_update_streams_without_user_message_chunk_and_skips_wakeup_dups() {
        let mut synthesized_wakeup_tools = HashSet::new();
        synthesized_wakeup_tools.insert("call_wakeup_dup".to_string());
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_test".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        handle_acp_session_update(
            &json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": { "text": "Inspecting code" }
            }),
            &synthesized_wakeup_tools,
            &mut seg_state,
            &mut tool_cache,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_wakeup_dup",
                "title": "Duplicate tool",
                "status": "completed"
            }),
            &synthesized_wakeup_tools,
            &mut seg_state,
            &mut tool_cache,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_live",
                "kind": "read",
                "title": "Read file",
                "status": "completed",
                "rawInput": { "Path": "src/main.rs" }
            }),
            &synthesized_wakeup_tools,
            &mut seg_state,
            &mut tool_cache,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "text": "Done!" }
            }),
            &synthesized_wakeup_tools,
            &mut seg_state,
            &mut tool_cache,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        let ev1 = evt_rx.try_recv().expect("thinking emitted");
        assert!(
            matches!(ev1, DriverEvent::ThinkingText { ref text, .. } if text == "Inspecting code")
        );

        let ev2 = evt_rx.try_recv().expect("live tool emitted");
        assert!(matches!(ev2, DriverEvent::ToolCall { ref call_id, .. } if call_id == "call_live"));

        let ev3 = evt_rx.try_recv().expect("assistant text emitted");
        assert!(
            matches!(ev3, DriverEvent::AssistantText { ref text, streaming: true, .. } if text == "Done!")
        );

        assert!(evt_rx.try_recv().is_err());
    }

    #[test]
    fn compact_unified_diff_shrinks_large_file_edits_and_formats_multiline_hunks() {
        let mut lines: Vec<String> = (1..=1500).map(|i| format!("line_{i}")).collect();
        let old_doc = lines.join("\n") + "\n";
        lines[749] = "line_750_updated_a".into();
        lines.insert(750, "line_750_inserted_b".into());
        let new_doc = lines.join("\n") + "\n";

        let diff = crate::build_compact_unified_diff("crates/demo/lib.rs", &old_doc, &new_doc)
            .expect("non-empty diff");
        assert!(
            diff.len() < 1024,
            "expected compact diff, got {} bytes",
            diff.len()
        );
        assert!(diff.contains("--- a/crates/demo/lib.rs"));
        assert!(diff.contains("+++ b/crates/demo/lib.rs"));
        assert!(diff.contains("@@"));
        assert!(diff.contains("-line_750\n"));
        assert!(diff.contains("+line_750_updated_a\n+line_750_inserted_b\n"));
    }

    #[tokio::test]
    async fn acp_runtime_warning_chunks_route_to_notice_not_assistant_text() {
        let synthesized_wakeup_tools = HashSet::new();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_net".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        handle_acp_session_update(
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "text": "\n⚠ API error (attempt 1): request failed: read: connection reset by peer\n\n⚠ The stream was interrupted. Please continue the task you were working on.\n"
                }
            }),
            &synthesized_wakeup_tools,
            &mut seg_state,
            &mut tool_cache,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        let ev1 = evt_rx.try_recv().expect("first warning notice");
        assert!(matches!(
            ev1,
            DriverEvent::Notice { ref level, ref message }
                if level == "warning" && message.starts_with("API error (attempt 1):")
        ));
        let ev2 = evt_rx.try_recv().expect("second warning notice");
        assert!(matches!(
            ev2,
            DriverEvent::Notice { ref level, ref message }
                if level == "warning" && message.starts_with("The stream was interrupted")
        ));
        assert!(evt_rx.try_recv().is_err());
        assert!(seg_state.msg_accum.is_empty());
    }

    #[test]
    fn transcript_monitor_maps_structured_wakeup_tools_and_surfaces_network_errors() {
        let dir = std::env::temp_dir().join(format!("orbit_wake_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let transcript_path = dir.join("transcript.jsonl");
        let lines = [
            json!({
                "step_index": 10,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "<SYSTEM_MESSAGE>background task done</SYSTEM_MESSAGE>"
            }),
            json!({
                "step_index": 11,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "thinking": "Running check",
                "content": "",
                "tool_calls": [
                    {
                        "name": "run_command",
                        "args": { "CommandLine": "\"cargo test --workspace\"" }
                    },
                    {
                        "name": "read_file",
                        "args": { "Path": "\"src/lib.rs\"", "StartLine": "1", "EndLine": "50" }
                    }
                ]
            }),
            json!({
                "step_index": 12,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Command exited with code 0.\nok"
            }),
            json!({
                "step_index": 13,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Showing lines 1-50"
            }),
            json!({
                "step_index": 14,
                "type": "ERROR_MESSAGE",
                "status": "DONE",
                "error": "API error (attempt 1): dial tcp 1.2.3.4:443: i/o timeout"
            }),
        ];
        let payload = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&transcript_path, payload).expect("write transcript");

        let mut monitor = AgyTranscriptMonitor {
            full_path: dir.join("missing.jsonl"),
            compact_path: transcript_path,
            byte_offset: 0,
            seen_steps: HashSet::new(),
            in_wakeup: false,
            pending_tools: Vec::new(),
            consecutive_errors: 0,
            last_error_at: None,
            last_error_message: None,
        };

        let steps = monitor.poll_post_turn_wakeup();
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, WakeupStep::Started { .. }))
        );
        assert!(steps.iter().any(|s| matches!(
            s,
            WakeupStep::Tool {
                call_id,
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::Shell { command, exit_code: Some(0), .. }
            } if call_id == "wake_tool_11_0" && command == "cargo test --workspace"
        )));
        assert!(steps.iter().any(|s| matches!(
            s,
            WakeupStep::Tool {
                call_id,
                status: ToolCallStatus::Completed,
                detail: ToolCallDetail::FileRead { path, start_line: Some(1), end_line: Some(50) }
            } if call_id == "wake_tool_11_1" && path == "src/lib.rs"
        )));
        assert!(steps.iter().any(|s| matches!(
            s,
            WakeupStep::Notice { level, message }
                if level == "warning" && message.contains("i/o timeout")
        )));

        // Simulate stall past WAKEUP_ERROR_STALL_TIMEOUT -> should emit Failed and exit in_wakeup
        let mut stall_out = Vec::new();
        let future_now = Instant::now() + WAKEUP_ERROR_STALL_TIMEOUT + Duration::from_secs(1);
        monitor.check_error_stall(&mut stall_out, future_now);
        assert!(!monitor.in_wakeup);
        assert!(stall_out.iter().any(|s| matches!(
            s,
            WakeupStep::Failed { message } if message.contains("i/o timeout")
        )));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
