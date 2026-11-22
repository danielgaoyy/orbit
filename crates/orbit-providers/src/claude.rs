//! Native Rust driver for Anthropic Claude Code (`claude` bidirectional `stream-json` Control Protocol
//! + `~/.claude/.credentials.json` / macOS Keychain OAuth Usage).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, BackgroundTaskInfo, ProviderKind, ProviderModelCatalog,
    ProviderModelSpec, ProviderQuotaSnapshot, ProviderSubagentDescriptor, ProviderSubagentStatus,
    QuestionItem, QuotaWindow, ReasoningEffort, SessionMode, ThinkingOptionSpec, TimelinePayload,
    ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after, spawn_detached_shim,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::attachments::build_claude_content_blocks;
use crate::driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};

pub async fn spawn_claude_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    let preassigned_session_id = cfg
        .resume_session_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let (shim, is_reattached) = if let Some(existing) = reattached {
        if let Some(ack_seq) = existing.attach_ack_seq() {
            cfg.shim_seq_tracker.fetch_max(ack_seq, Ordering::Relaxed);
        }
        (existing, true)
    } else {
        let perm_mode = claude_mode_flag(&cfg.mode);

        let mut args = vec![
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--permission-prompt-tool".to_string(),
            "stdio".to_string(),
            "--include-partial-messages".to_string(),
            "--permission-mode".to_string(),
            perm_mode.to_string(),
            "--allow-dangerously-skip-permissions".to_string(),
        ];

        if let Some(ref resume_id) = cfg.resume_session_id {
            args.push(format!("--resume={resume_id}"));
        } else {
            args.push(format!("--session-id={preassigned_session_id}"));
        }

        if let Some(ref m) = cfg.model {
            args.push("--model".to_string());
            args.push(m.clone());
        }
        for extra in &cfg.extra_dirs {
            args.push("--add-dir".to_string());
            args.push(extra.to_string_lossy().to_string());
        }
        if let Some(ref mcp) = cfg.mcp_spec {
            let mcp_json = json!({
                "mcpServers": {
                    mcp.server_name.clone(): {
                        "type": "http",
                        "url": mcp.http_url,
                        "headers": {
                            "Authorization": format!("Bearer {}", mcp.bearer_token)
                        }
                    }
                }
            })
            .to_string();
            args.push("--mcp-config".to_string());
            args.push(mcp_json);
        }
        if let Some(ref sys_append) = cfg.system_prompt_append
            && !sys_append.trim().is_empty()
        {
            args.push("--append-system-prompt".to_string());
            args.push(sys_append.clone());
        }

        let mut env = cfg.extra_env.clone();
        env.insert("CLAUDE_CODE_ENTRYPOINT".into(), "sdk-ts".into());
        env.insert(
            "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING".into(),
            "true".into(),
        );
        env.insert("CLAUDE_CODE_EMIT_SESSION_STATE_EVENTS".into(), "1".into());
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
                env.insert("CLAUDE_CODE_EFFORT_LEVEL".into(), s.into());
            }
        }

        let spec = ShimSpawnSpec {
            shim_id: format!("claude_{}", cfg.agent_id),
            mode: ShimMode::Stdio,
            program: cfg.executable.to_string_lossy().to_string(),
            args,
            cwd: cfg.cwd.clone(),
            env,
        };

        let mut fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
            .await
            .map_err(|e| format!("failed to spawn claude shim: {e}"))?;

        // Send mandatory `initialize` control_request handshake
        let init_req = json!({
            "request_id": "init_1",
            "type": "control_request",
            "request": {
                "subtype": "initialize",
                "forwardSubagentText": true,
                "perTaskStopAffordance": true
            }
        })
        .to_string();

        fresh
            .write_stdin_line(init_req)
            .await
            .map_err(|e| format!("failed to write claude initialize: {e}"))?;

        wait_claude_init_response(&mut fresh, &cfg.shim_seq_tracker).await?;
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_claude_actor(
        cfg,
        preassigned_session_id,
        shim,
        is_reattached,
        cmd_rx,
        evt_tx,
    ));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

struct PendingClaudePerm {
    control_request_id: String,
    tool_use_id: String,
    original_input: Value,
    permission_suggestions: Value,
    is_question: bool,
    question_texts: Vec<String>,
}

async fn run_claude_actor(
    cfg: SessionLaunchConfig,
    mut session_id: String,
    mut shim: orbit_shim::ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: session_id.clone(),
            model: cfg.model.clone(),
        })
        .await;
    if !is_reattached {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }

    let mut ctrl_seq: u64 = if is_reattached { 1000 } else { 2 };
    let mut active_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut active_msg_id: String = "claude_msg_0".into();
    let mut streamed_blocks: HashMap<u64, (String, String)> = HashMap::new(); // index -> (kind, accum)
    let mut emitted_text_in_turn = false;
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingClaudePerm> = HashMap::new();
    let mut queued_prompts: VecDeque<(String, String)> = VecDeque::new();
    let mut declared_tasks: HashMap<String, ProviderSubagentDescriptor> = HashMap::new();
    let mut tool_use_to_task: HashMap<String, String> = HashMap::new();
    let mut bg_bash_tasks: HashMap<String, (String, ToolCallDetail, BackgroundTaskInfo)> =
        HashMap::new();
    let mut bg_tool_use_to_task: HashMap<String, String> = HashMap::new();
    let mut bg_tool_uses: HashMap<String, BackgroundTaskInfo> = HashMap::new();

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt { turn_id, prompt, behavior } => {
                        if active_turn_id.is_some() {
                            match behavior {
                                ActiveTurnBehavior::Queue => {
                                    queued_prompts.push_back((turn_id, prompt));
                                    continue;
                                }
                                ActiveTurnBehavior::Interrupt => {
                                    let rid = format!("ctrl_{ctrl_seq}");
                                    ctrl_seq += 1;
                                    let req = json!({
                                        "request_id": rid,
                                        "type": "control_request",
                                        "request": {
                                            "subtype": "interrupt",
                                            "cancel_queued": true
                                        }
                                    });
                                    let _ = shim.write_stdin_line(req.to_string()).await;
                                    queued_prompts.push_front((turn_id, prompt));
                                    continue;
                                }
                                ActiveTurnBehavior::Steer => {
                                    let content_blocks = build_claude_content_blocks(
                                        &prompt,
                                        cfg.attachments_dir.as_deref(),
                                    );
                                    let user_frame = json!({
                                        "type": "user",
                                        "session_id": session_id,
                                        "parent_tool_use_id": null,
                                        "uuid": Uuid::new_v4().to_string(),
                                        "priority": "next",
                                        "message": {
                                            "role": "user",
                                            "content": content_blocks
                                        }
                                    });
                                    if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }

                        active_turn_id = Some(turn_id.clone());
                        streamed_blocks.clear();
                        emitted_text_in_turn = false;
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                turn_id: turn_id.clone(),
                            }))
                            .await;

                        let content_blocks =
                            build_claude_content_blocks(&prompt, cfg.attachments_dir.as_deref());
                        let user_frame = json!({
                            "type": "user",
                            "session_id": session_id,
                            "parent_tool_use_id": null,
                            "uuid": Uuid::new_v4().to_string(),
                            "message": {
                                "role": "user",
                                "content": content_blocks
                            }
                        });
                        if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        let rid = format!("ctrl_{ctrl_seq}");
                        ctrl_seq += 1;
                        let req = json!({
                            "request_id": rid,
                            "type": "control_request",
                            "request": {
                                "subtype": "interrupt",
                                "cancel_queued": true
                            }
                        });
                        let _ = shim.write_stdin_line(req.to_string()).await;
                    }
                    DriverCommand::SetMode { mode } => {
                        let rid = format!("ctrl_{ctrl_seq}");
                        ctrl_seq += 1;
                        let req = json!({
                            "request_id": rid,
                            "type": "control_request",
                            "request": {
                                "subtype": "set_permission_mode",
                                "mode": claude_mode_flag(&mode)
                            }
                        });
                        let _ = shim.write_stdin_line(req.to_string()).await;
                    }
                    DriverCommand::SetConfig { model, effort } => {
                        if let Some(m) = model {
                            let rid = format!("ctrl_{ctrl_seq}");
                            ctrl_seq += 1;
                            let req = json!({
                                "request_id": rid,
                                "type": "control_request",
                                "request": {
                                    "subtype": "set_model",
                                    "model": m
                                }
                            });
                            let _ = shim.write_stdin_line(req.to_string()).await;
                        }
                        if let Some(eff) = effort {
                            let max_tokens = match eff {
                                ReasoningEffort::Off => 0,
                                ReasoningEffort::Low => 4_000,
                                ReasoningEffort::Medium => 10_000,
                                ReasoningEffort::High => 16_000,
                                ReasoningEffort::Xhigh => 24_000,
                                ReasoningEffort::Max | ReasoningEffort::Ultra => 31_999,
                            };
                            let rid = format!("ctrl_{ctrl_seq}");
                            ctrl_seq += 1;
                            let req = json!({
                                "request_id": rid,
                                "type": "control_request",
                                "request": {
                                    "subtype": "set_max_thinking_tokens",
                                    "max_thinking_tokens": max_tokens
                                }
                            });
                            let _ = shim.write_stdin_line(req.to_string()).await;
                        }
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(p) = pending_perms.remove(&request_id) {
                            let resp = if decision == "allow" || decision == "allow_always" {
                                let (updated_perms, classification) = if decision == "allow_always" {
                                    (p.permission_suggestions, "user_permanent")
                                } else {
                                    (json!([]), "user_temporary")
                                };
                                json!({
                                    "type": "control_response",
                                    "response": {
                                        "subtype": "success",
                                        "request_id": p.control_request_id,
                                        "response": {
                                            "behavior": "allow",
                                            "updatedInput": p.original_input,
                                            "updatedPermissions": updated_perms,
                                            "toolUseID": p.tool_use_id,
                                            "decisionClassification": classification
                                        }
                                    }
                                })
                            } else {
                                json!({
                                    "type": "control_response",
                                    "response": {
                                        "subtype": "success",
                                        "request_id": p.control_request_id,
                                        "response": {
                                            "behavior": "deny",
                                            "message": "User denied permission from Orbit",
                                            "interrupt": false,
                                            "toolUseID": p.tool_use_id,
                                            "decisionClassification": "user_reject"
                                        }
                                    }
                                })
                            };
                            let _ = shim.write_stdin_line(resp.to_string()).await;
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
                            let mut answers_obj = serde_json::Map::new();
                            for (idx, q_text) in p.question_texts.iter().enumerate() {
                                let ans_str = answers
                                    .get(idx)
                                    .map(|v| v.join(", "))
                                    .unwrap_or_default();
                                answers_obj.insert(q_text.clone(), Value::String(ans_str));
                            }
                            let mut updated = p.original_input.clone();
                            if let Some(obj) = updated.as_object_mut() {
                                obj.insert("answers".into(), Value::Object(answers_obj));
                            }
                            let resp = json!({
                                "type": "control_response",
                                "response": {
                                    "subtype": "success",
                                    "request_id": p.control_request_id,
                                    "response": {
                                        "behavior": "allow",
                                        "updatedInput": updated,
                                        "toolUseID": p.tool_use_id
                                    }
                                }
                            });
                            let _ = shim.write_stdin_line(resp.to_string()).await;
                            if let Some(ref tid) = active_turn_id {
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
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let Some(frame_type) = val.get("type").and_then(|v| v.as_str()) else {
                            continue;
                        };
                        match frame_type {
                            "system" => {
                                let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
                                match subtype {
                                    "init" => {
                                        if let Some(sid) = val.get("session_id").and_then(|v| v.as_str())
                                            && !sid.is_empty()
                                            && sid != session_id
                                        {
                                            session_id = sid.to_string();
                                            let model = val
                                                .get("model")
                                                .and_then(|v| v.as_str())
                                                .map(ToOwned::to_owned);
                                            let _ = evt_tx
                                                .send(DriverEvent::SessionInitialized {
                                                    persistence_ref: session_id.clone(),
                                                    model,
                                                })
                                                .await;
                                        }
                                    }
                                    "task_started" => {
                                        let task_type = val.get("task_type").and_then(|v| v.as_str()).unwrap_or("local_agent");
                                        let skip_transcript = val.get("skip_transcript").and_then(|v| v.as_bool()).unwrap_or(false);
                                        if task_type == "local_bash" {
                                            let task_id = val
                                                .get("task_id")
                                                .or_else(|| val.get("tool_use_id"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            if !task_id.is_empty() {
                                                let call_id = val
                                                    .get("tool_use_id")
                                                    .and_then(|v| v.as_str())
                                                    .filter(|s| !s.trim().is_empty())
                                                    .map(ToOwned::to_owned)
                                                    .unwrap_or_else(|| format!("claude_bg_{task_id}"));
                                                let desc = val
                                                    .get("description")
                                                    .or_else(|| val.get("summary"))
                                                    .and_then(|v| v.as_str())
                                                    .map(str::trim)
                                                    .filter(|s| !s.is_empty())
                                                    .map(ToOwned::to_owned);
                                                let log_path = val
                                                    .get("output_file")
                                                    .and_then(|v| v.as_str())
                                                    .map(str::trim)
                                                    .filter(|s| !s.is_empty())
                                                    .map(ToOwned::to_owned);
                                                let bg_info = BackgroundTaskInfo {
                                                    task_id: Some(task_id.clone()),
                                                    label: desc.clone(),
                                                    log_path,
                                                };
                                                let detail = tool_cache
                                                    .get(&call_id)
                                                    .cloned()
                                                    .unwrap_or_else(|| ToolCallDetail::Shell {
                                                        command: desc
                                                            .clone()
                                                            .unwrap_or_else(|| "Background command".into()),
                                                        cwd: None,
                                                        exit_code: None,
                                                        output_preview: None,
                                                    });
                                                tool_cache.insert(call_id.clone(), detail.clone());
                                                bg_tool_use_to_task.insert(call_id.clone(), task_id.clone());
                                                bg_tool_uses.insert(call_id.clone(), bg_info.clone());
                                                bg_bash_tasks.insert(
                                                    task_id,
                                                    (call_id.clone(), detail.clone(), bg_info.clone()),
                                                );
                                                let _ = evt_tx
                                                    .send(DriverEvent::ToolCall {
                                                        call_id,
                                                        status: ToolCallStatus::Running,
                                                        detail,
                                                        background_task: Some(bg_info),
                                                    })
                                                    .await;
                                            }
                                        } else if !skip_transcript {
                                            let task_id = val
                                                .get("task_id")
                                                .or_else(|| val.get("tool_use_id"))
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("")
                                                .to_string();
                                            if !task_id.is_empty() {
                                                if let Some(tuid) = val.get("tool_use_id").and_then(|v| v.as_str()) {
                                                    tool_use_to_task.insert(tuid.to_string(), task_id.clone());
                                                }
                                                let title = val
                                                    .get("description")
                                                    .or_else(|| val.get("summary"))
                                                    .and_then(|v| v.as_str())
                                                    .filter(|s| !s.trim().is_empty())
                                                    .unwrap_or(if task_type == "local_workflow" { "Workflow" } else { "Subagent" })
                                                    .to_string();
                                                let parent_sub = val
                                                    .get("parent_tool_use_id")
                                                    .and_then(|v| v.as_str())
                                                    .and_then(|p| tool_use_to_task.get(p).cloned());
                                                let now = Utc::now();
                                                let desc = ProviderSubagentDescriptor {
                                                    subagent_id: task_id.clone(),
                                                    parent_agent_id: cfg.agent_id.clone(),
                                                    parent_subagent_id: parent_sub,
                                                    title,
                                                    subtitle: val.get("model").and_then(|v| v.as_str()).map(ToOwned::to_owned),
                                                    status: ProviderSubagentStatus::Running,
                                                    created_at: now,
                                                    updated_at: now,
                                                };
                                                declared_tasks.insert(task_id, desc.clone());
                                                let _ = evt_tx.send(DriverEvent::ProviderSubagentUpsert(desc)).await;
                                            }
                                        }
                                    }
                                    "task_updated" | "task_progress" => {
                                        let lookup_id = val
                                            .get("task_id")
                                            .and_then(|v| v.as_str())
                                            .map(ToOwned::to_owned)
                                            .or_else(|| {
                                                val.get("tool_use_id")
                                                    .and_then(|v| v.as_str())
                                                    .and_then(|id| tool_use_to_task.get(id).cloned())
                                            });
                                        if let Some(tid) = lookup_id
                                            && let Some(desc) = declared_tasks.get_mut(&tid)
                                        {
                                            if let Some(title) = val
                                                .pointer("/patch/description")
                                                .or_else(|| val.get("description"))
                                                .and_then(|v| v.as_str())
                                                .filter(|s| !s.trim().is_empty())
                                            {
                                                desc.title = title.to_string();
                                            }
                                            desc.updated_at = Utc::now();
                                            let _ = evt_tx
                                                .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
                                                .await;
                                        }
                                    }
                                    "task_notification" => {
                                        let raw_task_id = val
                                            .get("task_id")
                                            .and_then(|v| v.as_str())
                                            .map(ToOwned::to_owned);
                                        let raw_tool_use_id = val
                                            .get("tool_use_id")
                                            .and_then(|v| v.as_str())
                                            .map(ToOwned::to_owned);
                                        let bg_tid = raw_task_id
                                            .clone()
                                            .filter(|id| bg_bash_tasks.contains_key(id))
                                            .or_else(|| {
                                                raw_tool_use_id
                                                    .as_deref()
                                                    .and_then(|id| bg_tool_use_to_task.get(id).cloned())
                                            });
                                        if let Some(tid) = bg_tid
                                            && let Some((call_id, mut detail, mut bg_info)) =
                                                bg_bash_tasks.remove(&tid)
                                        {
                                            bg_tool_use_to_task.remove(&call_id);
                                            bg_tool_uses.remove(&call_id);
                                            tool_cache.remove(&call_id);
                                            if bg_info.log_path.is_none() {
                                                bg_info.log_path = val
                                                    .get("output_file")
                                                    .and_then(|v| v.as_str())
                                                    .map(str::trim)
                                                    .filter(|s| !s.is_empty())
                                                    .map(ToOwned::to_owned);
                                            }
                                            let st = val
                                                .get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("completed");
                                            let is_failed = matches!(
                                                st,
                                                "failed" | "error" | "stopped" | "canceled" | "cancelled"
                                            );
                                            if let Some(summary) = val
                                                .get("summary")
                                                .and_then(|v| v.as_str())
                                                .map(str::trim)
                                                .filter(|s| !s.is_empty())
                                                && let ToolCallDetail::Shell {
                                                    ref mut output_preview,
                                                    ref mut exit_code,
                                                    ..
                                                } = detail
                                            {
                                                *output_preview = Some(truncate_claude(summary, 800));
                                                if exit_code.is_none() {
                                                    *exit_code = Some(if is_failed { 1 } else { 0 });
                                                }
                                            }
                                            let _ = evt_tx
                                                .send(DriverEvent::ToolCall {
                                                    call_id,
                                                    status: if is_failed {
                                                        ToolCallStatus::Failed
                                                    } else {
                                                        ToolCallStatus::Completed
                                                    },
                                                    detail,
                                                    background_task: Some(bg_info),
                                                })
                                                .await;
                                            continue;
                                        }
                                        let lookup_id = raw_task_id.or_else(|| {
                                            raw_tool_use_id
                                                .as_deref()
                                                .and_then(|id| tool_use_to_task.get(id).cloned())
                                        });
                                        if let Some(tid) = lookup_id
                                            && let Some(desc) = declared_tasks.get_mut(&tid)
                                        {
                                            let st = val.get("status").and_then(|v| v.as_str()).unwrap_or("completed");
                                            desc.status = match st {
                                                "failed" | "error" => ProviderSubagentStatus::Failed,
                                                "stopped" | "canceled" | "cancelled" => ProviderSubagentStatus::Canceled,
                                                _ => ProviderSubagentStatus::Completed,
                                            };
                                            desc.updated_at = Utc::now();
                                            let _ = evt_tx
                                                .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
                                                .await;
                                            if let Some(summary) = val.get("summary").and_then(|v| v.as_str())
                                                && !summary.trim().is_empty()
                                            {
                                                let _ = evt_tx
                                                    .send(DriverEvent::ProviderSubagentTimeline {
                                                        subagent_id: tid,
                                                        payload: TimelinePayload::AssistantMessage {
                                                            item_id: format!("task_notif_{}", Uuid::new_v4().simple()),
                                                            text: summary.to_string(),
                                                            streaming: false,
                                                        },
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "stream_event" => {
                                // Only stream foreground root events on main timeline
                                if val.get("parent_tool_use_id").is_some_and(|v| !v.is_null()) {
                                    continue;
                                }
                                let Some(ev) = val.get("event") else { continue; };
                                let ev_type = ev.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                match ev_type {
                                    "message_start" => {
                                        if let Some(mid) = ev.pointer("/message/id").and_then(|v| v.as_str()) {
                                            active_msg_id = mid.to_string();
                                            streamed_blocks.clear();
                                        }
                                    }
                                    "content_block_start" => {
                                        let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                        let btype = ev
                                            .pointer("/content_block/type")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string();
                                        streamed_blocks.insert(idx, (btype, String::new()));
                                    }
                                    "content_block_delta" => {
                                        let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                        let dtype = ev.pointer("/delta/type").and_then(|v| v.as_str()).unwrap_or("");
                                        if dtype == "text_delta" {
                                            let chunk = ev.pointer("/delta/text").and_then(|v| v.as_str()).unwrap_or("");
                                            if !chunk.is_empty() {
                                                let entry = streamed_blocks
                                                    .entry(idx)
                                                    .or_insert_with(|| ("text".into(), String::new()));
                                                entry.1.push_str(chunk);
                                                emitted_text_in_turn = true;
                                                let _ = evt_tx
                                                    .send(DriverEvent::AssistantText {
                                                        item_id: format!("{active_msg_id}_{idx}"),
                                                        text: entry.1.clone(),
                                                        streaming: true,
                                                    })
                                                    .await;
                                            }
                                        } else if dtype == "thinking_delta" {
                                            let chunk = ev
                                                .pointer("/delta/thinking")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("");
                                            if !chunk.is_empty() {
                                                let entry = streamed_blocks
                                                    .entry(idx)
                                                    .or_insert_with(|| ("thinking".into(), String::new()));
                                                entry.1.push_str(chunk);
                                                let _ = evt_tx
                                                    .send(DriverEvent::ThinkingText {
                                                        item_id: format!("{active_msg_id}_th_{idx}"),
                                                        text: entry.1.clone(),
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                    "content_block_stop" => {
                                        let idx = ev.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                                        if let Some((btype, full_txt)) = streamed_blocks.get(&idx)
                                            && btype == "text"
                                            && !full_txt.is_empty()
                                        {
                                            let _ = evt_tx
                                                .send(DriverEvent::AssistantText {
                                                    item_id: format!("{active_msg_id}_{idx}"),
                                                    text: full_txt.clone(),
                                                    streaming: false,
                                                })
                                                .await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            "assistant" => {
                                if let Some(parent_tuid) = val.get("parent_tool_use_id").and_then(|v| v.as_str()) {
                                    if let Some(sub_id) = tool_use_to_task.get(parent_tuid).cloned()
                                        && let Some(blocks) = val.pointer("/message/content").and_then(|v| v.as_array())
                                    {
                                        for (i, b) in blocks.iter().enumerate() {
                                            let btype = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                            if btype == "text"
                                                && let Some(txt) = b.get("text").and_then(|v| v.as_str())
                                                && !txt.trim().is_empty()
                                            {
                                                let _ = evt_tx
                                                    .send(DriverEvent::ProviderSubagentTimeline {
                                                        subagent_id: sub_id.clone(),
                                                        payload: TimelinePayload::AssistantMessage {
                                                            item_id: format!("sub_{i}_{}", Uuid::new_v4().simple()),
                                                            text: txt.to_string(),
                                                            streaming: false,
                                                        },
                                                    })
                                                    .await;
                                            } else if btype == "tool_use" {
                                                let cid = b.get("id").and_then(|v| v.as_str()).unwrap_or("sub_t").to_string();
                                                let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("Tool");
                                                let detail = map_claude_tool_detail(name, &b["input"], None);
                                                let _ = evt_tx
                                                    .send(DriverEvent::ProviderSubagentTimeline {
                                                        subagent_id: sub_id.clone(),
                                                        payload: TimelinePayload::ToolCall {
                                                            call_id: cid,
                                                            status: ToolCallStatus::Completed,
                                                            detail,
                                                            background_task: None,
                                                        },
                                                    })
                                                    .await;
                                            }
                                        }
                                    }
                                    continue;
                                }
                                let mid = val
                                    .pointer("/message/id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("claude_msg");
                                if let Some(blocks) = val.pointer("/message/content").and_then(|v| v.as_array()) {
                                    for (i, b) in blocks.iter().enumerate() {
                                        let btype = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                                        if btype == "tool_use" {
                                            let call_id = b
                                                .get("id")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("toolu")
                                                .to_string();
                                            let name = b
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("Tool");
                                            let input = &b["input"];
                                            let detail = map_claude_tool_detail(name, input, None);
                                            let bg_info = if name == "Bash"
                                                && input
                                                    .get("run_in_background")
                                                    .and_then(|v| v.as_bool())
                                                    == Some(true)
                                            {
                                                let info = BackgroundTaskInfo {
                                                    task_id: None,
                                                    label: input
                                                        .get("description")
                                                        .and_then(|v| v.as_str())
                                                        .map(str::trim)
                                                        .filter(|s| !s.is_empty())
                                                        .map(ToOwned::to_owned),
                                                    log_path: None,
                                                };
                                                bg_tool_uses.insert(call_id.clone(), info.clone());
                                                Some(info)
                                            } else {
                                                None
                                            };
                                            tool_cache.insert(call_id.clone(), detail.clone());
                                            let _ = evt_tx
                                                .send(DriverEvent::ToolCall {
                                                    call_id,
                                                    status: ToolCallStatus::Running,
                                                    detail,
                                                    background_task: bg_info,
                                                })
                                                .await;
                                        } else if btype == "text"
                                            && streamed_blocks.is_empty()
                                            && let Some(txt) = b.get("text").and_then(|v| v.as_str())
                                            && !txt.is_empty()
                                        {
                                            emitted_text_in_turn = true;
                                            let _ = evt_tx
                                                .send(DriverEvent::AssistantText {
                                                    item_id: format!("{mid}_{i}"),
                                                    text: txt.to_string(),
                                                    streaming: false,
                                                })
                                                .await;
                                        }
                                    }
                                }
                            }
                            "user" => {
                                if val.get("isReplay").and_then(|v| v.as_bool()) == Some(true) {
                                    continue;
                                }
                                if let Some(blocks) = val.pointer("/message/content").and_then(|v| v.as_array()) {
                                    for b in blocks {
                                        if b.get("type").and_then(|v| v.as_str()) == Some("tool_result")
                                            && let Some(call_id) = b.get("tool_use_id").and_then(|v| v.as_str())
                                        {
                                            let is_err = b
                                                .get("is_error")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false);
                                            let out_text = extract_claude_tool_result_text(&b["content"]);
                                            let keep_running_bg = !is_err
                                                && (bg_tool_use_to_task.contains_key(call_id)
                                                    || (bg_tool_uses.contains_key(call_id)
                                                        && looks_like_claude_bg_launch(&out_text)));
                                            let existing_bg = if keep_running_bg {
                                                bg_tool_uses.get(call_id).cloned()
                                            } else {
                                                bg_tool_uses.remove(call_id)
                                            };
                                            let cached_detail = if keep_running_bg {
                                                tool_cache.get(call_id).cloned()
                                            } else {
                                                tool_cache.remove(call_id)
                                            };
                                            let detail = match cached_detail {
                                                Some(ToolCallDetail::Shell { command, cwd, .. }) => {
                                                    ToolCallDetail::Shell {
                                                        command,
                                                        cwd,
                                                        exit_code: if keep_running_bg {
                                                            None
                                                        } else {
                                                            Some(if is_err { 1 } else { 0 })
                                                        },
                                                        output_preview: Some(out_text),
                                                    }
                                                }
                                                Some(other) => other,
                                                None => ToolCallDetail::Generic {
                                                    tool_name: "tool".into(),
                                                    arguments_json: out_text,
                                                },
                                            };
                                            if keep_running_bg {
                                                tool_cache.insert(call_id.to_string(), detail.clone());
                                            }
                                            let _ = evt_tx
                                                .send(DriverEvent::ToolCall {
                                                    call_id: call_id.to_string(),
                                                    status: if is_err {
                                                        ToolCallStatus::Failed
                                                    } else if keep_running_bg {
                                                        ToolCallStatus::Running
                                                    } else {
                                                        ToolCallStatus::Completed
                                                    },
                                                    detail,
                                                    background_task: existing_bg,
                                                })
                                                .await;
                                        }
                                    }
                                }
                            }
                            "control_request" => {
                                handle_claude_control_request(
                                    &val,
                                    &active_turn_id,
                                    &mut pending_perms,
                                    &evt_tx,
                                )
                                .await;
                            }
                            "result" => {
                                active_turn_id = None;
                                let is_err = val
                                    .get("is_error")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let subtype = val
                                    .get("subtype")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("success");

                                if is_err || subtype != "success" {
                                    let err_msg = val
                                        .get("result")
                                        .and_then(|v| v.as_str())
                                        .map(ToOwned::to_owned)
                                        .or_else(|| {
                                            val.get("errors")
                                                .and_then(|v| v.as_array())
                                                .and_then(|a| a.first())
                                                .and_then(|x| x.as_str())
                                                .map(ToOwned::to_owned)
                                        })
                                        .unwrap_or_else(|| format!("Claude turn failed ({subtype})"));
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                            message: err_msg,
                                            recoverable: true,
                                        }))
                                        .await;
                                } else {
                                    if !emitted_text_in_turn
                                        && let Some(res_txt) = val.get("result").and_then(|v| v.as_str())
                                        && !res_txt.trim().is_empty()
                                    {
                                        let _ = evt_tx
                                            .send(DriverEvent::AssistantText {
                                                item_id: format!("slash_{}", Uuid::new_v4().simple()),
                                                text: res_txt.to_string(),
                                                streaming: false,
                                            })
                                            .await;
                                    }
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                        .await;
                                }

                                // Drain queued or interrupted-replacement prompt immediately on turn settle
                                if let Some((next_tid, next_prompt)) = queued_prompts.pop_front() {
                                    active_turn_id = Some(next_tid.clone());
                                    streamed_blocks.clear();
                                    emitted_text_in_turn = false;
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: next_tid,
                                        }))
                                        .await;
                                    let content_blocks = build_claude_content_blocks(
                                        &next_prompt,
                                        cfg.attachments_dir.as_deref(),
                                    );
                                    let user_frame = json!({
                                        "type": "user",
                                        "session_id": session_id,
                                        "parent_tool_use_id": null,
                                        "uuid": Uuid::new_v4().to_string(),
                                        "message": {
                                            "role": "user",
                                            "content": content_blocks
                                        }
                                    });
                                    if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            _ => {}
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

async fn wait_claude_init_response(
    shim: &mut orbit_shim::ShimClient,
    shim_seq_tracker: &std::sync::atomic::AtomicU64,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            return Err("Claude Code control_request initialize timed out".into());
        }
        match tokio::time::timeout(rem, shim.rx.recv()).await {
            Ok(Some(frame)) => {
                if let Some(seq) = frame.seq() {
                    shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if val.get("type").and_then(|v| v.as_str()) == Some("control_response")
                            && val.pointer("/response/request_id").and_then(|v| v.as_str())
                                == Some("init_1")
                        {
                            return Ok(());
                        }
                    }
                    ShimFrame::Exited { code } => {
                        return Err(format!("claude exited early during init ({code:?})"));
                    }
                    _ => {}
                }
            }
            _ => return Err("claude shim closed during init".into()),
        }
    }
}

fn claude_mode_flag(mode: &SessionMode) -> &'static str {
    match mode {
        SessionMode::Default => "default",
        SessionMode::AcceptEdits => "acceptEdits",
        SessionMode::Plan => "plan",
        SessionMode::FullAccess => "bypassPermissions",
        SessionMode::Custom(_) => "default",
    }
}

async fn handle_claude_control_request(
    val: &Value,
    active_turn_id: &Option<String>,
    pending_perms: &mut HashMap<String, PendingClaudePerm>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let Some(ctrl_req_id) = val.get("request_id").and_then(|v| v.as_str()) else {
        return;
    };
    let req = &val["request"];
    if req.get("subtype").and_then(|v| v.as_str()) != Some("can_use_tool") {
        return;
    }

    let tool_name = req
        .get("tool_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Tool");
    let tool_use_id = req
        .get("tool_use_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let input = req.get("input").cloned().unwrap_or_else(|| json!({}));
    let suggestions = req
        .get("permission_suggestions")
        .cloned()
        .unwrap_or_else(|| json!([]));

    if tool_name == "AskUserQuestion" {
        let q_arr = input
            .get("questions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        let mut q_texts = Vec::new();
        for (i, q) in q_arr.iter().enumerate() {
            let qt = q
                .get("question")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let opts = q
                .get("options")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|o| {
                            o.get("label")
                                .and_then(|l| l.as_str())
                                .map(ToOwned::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            q_texts.push(qt.clone());
            items.push(QuestionItem {
                id: format!("cq_{i}"),
                header: q
                    .get("header")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                question: qt,
                options: opts,
                is_multi_select: q
                    .get("multiSelect")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
        }
        let req_id = format!("clq_{}", Uuid::new_v4().simple());
        pending_perms.insert(
            req_id.clone(),
            PendingClaudePerm {
                control_request_id: ctrl_req_id.to_string(),
                tool_use_id,
                original_input: input,
                permission_suggestions: suggestions,
                is_question: true,
                question_texts: q_texts,
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
                questions: items,
            })
            .await;
        return;
    }

    let req_id = format!("clp_{}", Uuid::new_v4().simple());
    let title = req
        .get("title")
        .or_else(|| req.get("description"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Claude wants to use {tool_name}"));
    let detail = map_claude_tool_detail(tool_name, &input, None);

    pending_perms.insert(
        req_id.clone(),
        PendingClaudePerm {
            control_request_id: ctrl_req_id.to_string(),
            tool_use_id,
            original_input: input,
            permission_suggestions: suggestions,
            is_question: false,
            question_texts: Vec::new(),
        },
    );

    let _ = evt_tx
        .send(DriverEvent::PermissionRequest {
            request_id: req_id.clone(),
            title,
            detail,
            allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
        })
        .await;
    let _ = evt_tx
        .send(DriverEvent::StatusChanged(
            AgentStatus::AwaitingPermission {
                turn_id: active_turn_id.clone().unwrap_or_default(),
                request_id: req_id,
            },
        ))
        .await;
}

fn map_claude_tool_detail(
    name: &str,
    input: &Value,
    output_preview: Option<String>,
) -> ToolCallDetail {
    match name {
        "Bash" => ToolCallDetail::Shell {
            command: input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            cwd: None,
            exit_code: None,
            output_preview,
        },
        "Read" => ToolCallDetail::FileRead {
            path: input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            start_line: input
                .get("offset")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
            end_line: input
                .get("limit")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32),
        },
        "Edit" | "Write" | "MultiEdit" => {
            let path = input
                .get("file_path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let diff = match (
                input.get("old_string").and_then(|v| v.as_str()),
                input.get("new_string").and_then(|v| v.as_str()),
            ) {
                (Some(o), Some(n)) => crate::build_compact_unified_diff(&path, o, n),
                _ => None,
            };
            ToolCallDetail::FileEdit {
                path,
                unified_diff: diff,
            }
        }
        "Grep" | "Glob" => ToolCallDetail::Search {
            query: input
                .get("pattern")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            scope: input
                .get("path")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned),
        },
        "Task" | "Agent" => ToolCallDetail::Subagent {
            child_agent_id: None,
            role: input
                .get("description")
                .or_else(|| input.get("subagent_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("subagent")
                .to_string(),
            prompt_preview: input
                .get("prompt")
                .and_then(|v| v.as_str())
                .map(|s| truncate_claude(s, 240))
                .unwrap_or_default(),
        },
        _ => ToolCallDetail::Generic {
            tool_name: name.to_string(),
            arguments_json: input.to_string(),
        },
    }
}

fn looks_like_claude_bg_launch(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("running in the background")
        || lower.contains("running in background")
        || lower.contains("background task")
        || lower.contains("background shell")
}

fn extract_claude_tool_result_text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return truncate_claude(s, 800);
    }
    if let Some(arr) = v.as_array() {
        let joined: String = arr
            .iter()
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        return truncate_claude(&joined, 800);
    }
    String::new()
}

// --- Claude Code OAuth Quota Probe (`~/.claude/.credentials.json` + macOS Keychain) ---

#[derive(Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOAuthEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeOAuthEntry {
    access_token: Option<String>,
    subscription_type: Option<String>,
    rate_limit_tier: Option<String>,
}

pub async fn probe_claude_quota() -> ProviderQuotaSnapshot {
    let raw_creds = load_claude_credentials_json().await;
    let Some(raw) = raw_creds else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("Claude OAuth credentials not found".into()),
            updated_at: Utc::now(),
        };
    };

    let Ok(parsed) = serde_json::from_str::<ClaudeCredentialsFile>(&raw) else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("invalid Claude credentials JSON".into()),
            updated_at: Utc::now(),
        };
    };

    let Some(oauth) = parsed.claude_ai_oauth else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("Claude configured via API key (no subscription window)".into()),
            updated_at: Utc::now(),
        };
    };
    let Some(token) = oauth.access_token else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label: None,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("missing Claude accessToken".into()),
            updated_at: Utc::now(),
        };
    };

    let account_label = match (
        oauth.subscription_type.as_deref(),
        oauth.rate_limit_tier.as_deref(),
    ) {
        (Some(sub), Some(tier)) => {
            let suffix = tier.split('_').next_back().unwrap_or("");
            Some(
                format!("Claude {} {}", sub.to_uppercase(), suffix)
                    .trim()
                    .to_string(),
            )
        }
        (Some(sub), None) => Some(format!("Claude {}", sub.to_uppercase())),
        _ => None,
    };

    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    else {
        return ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some("http client init failed".into()),
            updated_at: Utc::now(),
        };
    };

    let resp = client
        .get("https://api.anthropic.com/api/oauth/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            if let Ok(val) = r.json::<Value>().await {
                let mut windows = Vec::new();
                for (key, title) in [
                    ("five_hour", "Claude · 5-Hour Window"),
                    ("seven_day", "Claude · Weekly Window"),
                    ("seven_day_opus", "Claude · Weekly Opus"),
                ] {
                    if let Some(w) = val.get(key)
                        && let Some(util) = w.get("utilization").and_then(|v| v.as_f64())
                    {
                        let resets_at = w
                            .get("resets_at")
                            .and_then(|v| v.as_str())
                            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                            .map(|d| d.with_timezone(&Utc));
                        windows.push(QuotaWindow {
                            label: title.to_string(),
                            used_percent: util as f32,
                            resets_at,
                        });
                    }
                }
                ProviderQuotaSnapshot {
                    provider: ProviderKind::ClaudeCode,
                    account_label,
                    windows,
                    credits_remaining: None,
                    error: None,
                    updated_at: Utc::now(),
                }
            } else {
                ProviderQuotaSnapshot {
                    provider: ProviderKind::ClaudeCode,
                    account_label,
                    windows: Vec::new(),
                    credits_remaining: None,
                    error: Some("invalid usage JSON from Anthropic".into()),
                    updated_at: Utc::now(),
                }
            }
        }
        Ok(r) => ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some(format!("HTTP {}", r.status())),
            updated_at: Utc::now(),
        },
        Err(e) => ProviderQuotaSnapshot {
            provider: ProviderKind::ClaudeCode,
            account_label,
            windows: Vec::new(),
            credits_remaining: None,
            error: Some(e.to_string()),
            updated_at: Utc::now(),
        },
    }
}

async fn load_claude_credentials_json() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = tokio::process::Command::new("security")
            .args([
                "find-generic-password",
                "-w",
                "-s",
                "Claude Code-credentials",
            ])
            .output()
            .await
            && out.status.success()
        {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
    }

    let cfg_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
        });
    tokio::fs::read_to_string(cfg_dir.join(".credentials.json"))
        .await
        .ok()
}

fn truncate_claude(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

// --- Paseo-Aligned Claude Code First-Party Model Manifest (`model-manifest.ts`) ---

struct ClaudeManifestRow {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    min_version: Option<(u32, u32, u32)>,
    default_priority: u8,
    default_effort: Option<ReasoningEffort>,
    efforts: &'static [ReasoningEffort],
}

const EFFORTS_XHIGH_WITH_OFF: &[ReasoningEffort] = &[
    ReasoningEffort::Off,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

const EFFORTS_XHIGH: &[ReasoningEffort] = &[
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Xhigh,
    ReasoningEffort::Max,
];

const EFFORTS_STANDARD_WITH_OFF: &[ReasoningEffort] = &[
    ReasoningEffort::Off,
    ReasoningEffort::Low,
    ReasoningEffort::Medium,
    ReasoningEffort::High,
    ReasoningEffort::Max,
];

const CLAUDE_MANIFEST: &[ClaudeManifestRow] = &[
    ClaudeManifestRow {
        id: "claude-opus-5-5",
        label: "Opus 5.5",
        description: "Opus 5.5 · Latest release",
        min_version: Some((2, 1, 280)),
        default_priority: 3,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH,
    },
    ClaudeManifestRow {
        id: "claude-fable-5-2",
        label: "Fable 5.2",
        description: "Fable 5.2 · Most powerful model",
        min_version: Some((2, 1, 321)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-5-5",
        label: "Sonnet 5.5",
        description: "Sonnet 5.5 · Best for everyday tasks",
        min_version: Some((2, 1, 284)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH,
    },
    ClaudeManifestRow {
        id: "claude-haiku-5-5",
        label: "Haiku 5.5",
        description: "Haiku 5.5 · Fastest for quick answers",
        min_version: Some((2, 1, 293)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-opus-5",
        label: "Opus 5",
        description: "Opus 5 · Previous release",
        min_version: Some((2, 1, 219)),
        default_priority: 2,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-fable-5-1",
        label: "Fable 5.1",
        description: "Fable 5.1 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-5",
        label: "Sonnet 5",
        description: "Sonnet 5 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-opus-4-8",
        label: "Opus 4.8",
        description: "Opus 4.8 · Previous release",
        min_version: None,
        default_priority: 1,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-opus-4-6",
        label: "Opus 4.6",
        description: "Opus 4.6 · Most capable for complex work",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_STANDARD_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-4-6",
        label: "Sonnet 4.6",
        description: "Sonnet 4.6 · Best for everyday tasks",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_STANDARD_WITH_OFF,
    },
    ClaudeManifestRow {
        id: "claude-haiku-4-5",
        label: "Haiku 4.5",
        description: "Haiku 4.5 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: None,
        efforts: &[],
    },
];

fn parse_semver_prefix(raw: &str) -> Option<(u32, u32, u32)> {
    for tok in raw.split_whitespace() {
        let cleaned = tok.trim_start_matches('v');
        let mut parts = cleaned.split(['.', '-']);
        if let (Some(a), Some(b), Some(c)) = (parts.next(), parts.next(), parts.next())
            && let (Ok(maj), Ok(min), Ok(pat)) =
                (a.parse::<u32>(), b.parse::<u32>(), c.parse::<u32>())
        {
            return Some((maj, min, pat));
        }
    }
    None
}

fn build_claude_catalog_for_version(version: Option<(u32, u32, u32)>) -> ProviderModelCatalog {
    let eligible: Vec<&ClaudeManifestRow> = CLAUDE_MANIFEST
        .iter()
        .filter(|row| match (row.min_version, version) {
            (Some(min_v), Some(cur_v)) => cur_v >= min_v,
            _ => true,
        })
        .collect();

    let best_default_id = eligible
        .iter()
        .max_by_key(|row| row.default_priority)
        .map(|row| row.id);

    let models = eligible
        .into_iter()
        .map(|row| {
            let thinking_options = row
                .efforts
                .iter()
                .map(|eff| ThinkingOptionSpec {
                    effort: *eff,
                    label: eff.display_label().to_string(),
                    description: None,
                    is_default: Some(*eff) == row.default_effort,
                })
                .collect();

            ProviderModelSpec {
                id: row.id.to_string(),
                label: row.label.to_string(),
                description: row.description.to_string(),
                is_default: Some(row.id) == best_default_id,
                default_effort: row.default_effort,
                thinking_options,
            }
        })
        .collect();

    ProviderModelCatalog {
        provider: ProviderKind::ClaudeCode,
        models,
        configured_default_model: best_default_id.map(ToOwned::to_owned),
        configured_default_effort: Some(ReasoningEffort::Medium),
    }
}

pub fn fallback_claude_models() -> ProviderModelCatalog {
    build_claude_catalog_for_version(None)
}

pub async fn probe_claude_models(executable: Option<&Path>) -> ProviderModelCatalog {
    let Some(exe) = executable else {
        return fallback_claude_models();
    };
    let ver = tokio::time::timeout(Duration::from_secs(2), async {
        let child = tokio::process::Command::new(exe)
            .arg("--version")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let out = child.wait_with_output().await.ok()?;
        parse_semver_prefix(&String::from_utf8_lossy(&out.stdout))
    })
    .await
    .ok()
    .flatten();

    build_claude_catalog_for_version(ver)
}
