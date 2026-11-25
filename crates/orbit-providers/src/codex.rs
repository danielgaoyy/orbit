//! Native Rust driver for OpenAI Codex (`codex app-server` JSON-RPC 2.0 + `~/.codex/auth.json` Wham Usage).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentStatus, ProviderKind,
    ProviderModelCatalog, ProviderModelSpec, ProviderQuotaSnapshot, ProviderSubagentDescriptor,
    ProviderSubagentStatus, QuestionItem, QuotaWindow, ReasoningEffort, SessionMode,
    ThinkingOptionSpec, TimelinePayload, ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after, spawn_detached_shim,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex as AsyncMutex, mpsc};

use crate::attachments::build_codex_input_items_async;
use crate::driver::{
    DriverCommand, DriverEvent, PromptOrigin, ProviderSessionHandle, QueuedDriverPrompt,
    SessionLaunchConfig, emit_dropped_queued_prompts, emit_prompt_delivery,
};

/// Serialize concurrent `codex app-server` SQLite state runtime startups.
fn codex_startup_mutex() -> &'static AsyncMutex<()> {
    static LOCK: OnceLock<AsyncMutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| AsyncMutex::new(()))
}

fn build_codex_launch_args(fast_mode: bool) -> Vec<String> {
    let mut args = vec!["app-server".to_string()];
    if fast_mode {
        args.push("-c".to_string());
        args.push("service_tier=\"fast\"".to_string());
    }
    args
}

fn build_codex_thread_start_params(
    cfg: &SessionLaunchConfig,
    approval_policy: &str,
    start_sandbox: &str,
    inner_config: &serde_json::Map<String, Value>,
) -> Value {
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
    if cfg.fast_mode {
        params["serviceTier"] = Value::String("fast".to_string());
    }
    if !inner_config.is_empty() {
        params["config"] = Value::Object(inner_config.clone());
    }
    params
}

fn build_codex_thread_resume_params(
    tid: &str,
    model: Option<&str>,
    fast_mode: bool,
    inner_config: &serde_json::Map<String, Value>,
) -> Value {
    let mut params = json!({
        "threadId": tid,
        "excludeTurns": true
    });
    if let Some(m) = model.map(str::trim).filter(|s| !s.is_empty()) {
        params["model"] = Value::String(m.to_string());
    }
    if fast_mode {
        params["serviceTier"] = Value::String("fast".to_string());
    }
    if !inner_config.is_empty() {
        params["config"] = Value::Object(inner_config.clone());
    }
    params
}

#[allow(clippy::too_many_arguments)]
fn build_codex_turn_start_params(
    cfg: &SessionLaunchConfig,
    thread_id: &str,
    mode: &SessionMode,
    explicit_default_collaboration_mode: bool,
    resolved_model: &Option<String>,
    current_effort: Option<ReasoningEffort>,
    fast_mode: bool,
    service_tier_explicitly_controlled: bool,
    input_items: Value,
) -> Value {
    let (pol, sandbox_policy) = codex_turn_policies(mode, &cfg.extra_dirs);
    let mut params = json!({
        "threadId": thread_id,
        "input": input_items,
        "cwd": cfg.cwd.to_string_lossy(),
        "approvalPolicy": pol,
        "approvalsReviewer": "user",
        "sandboxPolicy": sandbox_policy
    });
    let clean_model = resolved_model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let eff_str = current_effort.map(|eff| match eff {
        ReasoningEffort::Off => "none",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max => "max",
        ReasoningEffort::Ultra => "ultra",
    });
    if let Some(m) = clean_model {
        params["model"] = Value::String(m.to_string());
    }
    if let Some(s) = eff_str {
        params["effort"] = Value::String(s.to_string());
    }
    if fast_mode {
        params["serviceTier"] = Value::String("fast".to_string());
    } else if service_tier_explicitly_controlled {
        // Codex app-server uses three-state `Option<Option<ServiceTier>>`:
        // explicit `null` clears `-c service_tier="fast"` / thread / user config.
        params["serviceTier"] = Value::Null;
    }
    // Codex app-server schema requires `CollaborationMode.settings.model` to be a non-empty string.
    // In Plan mode, emit `collaborationMode: "plan"`. When exiting Plan mode into Default/build,
    // explicitly emit `collaborationMode: "default"` so Codex does not inherit Plan mode from the
    // previous turn on the thread.
    if let Some(m) = clean_model
        && (matches!(mode, SessionMode::Plan) || explicit_default_collaboration_mode)
    {
        let mut settings = serde_json::Map::new();
        settings.insert("model".to_string(), Value::String(m.to_string()));
        if let Some(s) = eff_str {
            settings.insert("reasoning_effort".to_string(), Value::String(s.to_string()));
        }
        let collab_mode = if matches!(mode, SessionMode::Plan) {
            "plan"
        } else {
            "default"
        };
        params["collaborationMode"] = json!({
            "mode": collab_mode,
            "settings": Value::Object(settings)
        });
    }
    params
}

pub async fn spawn_codex_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
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
        let startup_guard = codex_startup_mutex().lock().await;

        let mut env = cfg.extra_env.clone();
        env.insert("NO_COLOR".into(), "1".into());
        crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);

        let spec = ShimSpawnSpec {
            sandbox: cfg.sandbox.clone(),
            egress_proxy_url: None,
            shim_id: format!("codex_{}", cfg.agent_id),
            mode: ShimMode::Stdio,
            program: cfg.executable.to_string_lossy().to_string(),
            args: build_codex_launch_args(cfg.fast_mode),
            cwd: cfg.cwd.clone(),
            env,
            cleanup_paths: Vec::new(),
        };

        let mut fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
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

        if let Err(e) = fresh.write_stdin_line(init_req).await {
            let _ = fresh.kill().await;
            return Err(format!("codex write initialize failed: {e}"));
        }

        if let Err(e) = wait_codex_id_response(&mut fresh, 1, &cfg.shim_seq_tracker).await {
            let _ = fresh.kill().await;
            return Err(e);
        }

        let initialized_notif = json!({
            "method": "initialized",
            "params": {}
        })
        .to_string();
        if let Err(e) = fresh.write_stdin_line(initialized_notif).await {
            let _ = fresh.kill().await;
            return Err(format!("codex write initialized failed: {e}"));
        }

        drop(startup_guard);
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_codex_actor(cfg, shim, is_reattached, cmd_rx, evt_tx));

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
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_id: u64 = if is_reattached { 1000 } else { 2 };
    let mut mode = cfg.mode.clone();

    let mut inject_handoff_on_next_prompt = false;
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();

    let (thread_id, mut resolved_model) = if is_reattached
        && let Some(ref tid) = cfg.resume_session_id
    {
        (tid.clone(), cfg.model.clone())
    } else {
        // Start or resume thread (injecting Orbit MCP server into `config.mcp_servers`)
        let (approval_policy, start_sandbox) = codex_thread_mode_strings(&mode);
        let thread_req_id = next_id;
        next_id += 1;

        let mut inner_config = serde_json::Map::new();
        if let Some(ref mcp) = cfg.mcp_spec {
            inner_config.insert(
                "mcp_servers".into(),
                json!({
                    mcp.server_name.clone(): {
                        "url": mcp.http_url,
                        "http_headers": {
                            "Authorization": format!("Bearer {}", mcp.bearer_token)
                        },
                        "tool_timeout_sec": 120
                    }
                }),
            );
        }
        if let Some(ref sys_append) = cfg.system_prompt_append
            && !sys_append.trim().is_empty()
        {
            inner_config.insert(
                "developer_instructions".into(),
                Value::String(sys_append.clone()),
            );
        }

        enum CodexThreadOpenMode<'a> {
            Start,
            Resume(&'a str),
            Fork(&'a str),
        }

        let build_open_msg = |req_id: u64, open_mode: CodexThreadOpenMode<'_>| match open_mode {
            CodexThreadOpenMode::Fork(tid) => {
                let mut params = json!({
                    "threadId": tid,
                    "cwd": cfg.cwd.to_string_lossy(),
                    "approvalPolicy": approval_policy,
                    "sandbox": start_sandbox,
                    "approvalsReviewer": "user",
                    "ephemeral": false
                });
                if let Some(ref m) = cfg.model {
                    params["model"] = Value::String(m.clone());
                }
                if cfg.fast_mode {
                    params["serviceTier"] = Value::String("fast".to_string());
                }
                if !inner_config.is_empty() {
                    params["config"] = Value::Object(inner_config.clone());
                }
                json!({
                    "id": req_id,
                    "method": "thread/fork",
                    "params": params
                })
            }
            CodexThreadOpenMode::Resume(tid) => {
                let params = build_codex_thread_resume_params(
                    tid,
                    cfg.model.as_deref(),
                    cfg.fast_mode,
                    &inner_config,
                );
                json!({
                    "id": req_id,
                    "method": "thread/resume",
                    "params": params
                })
            }
            CodexThreadOpenMode::Start => {
                let params = build_codex_thread_start_params(
                    &cfg,
                    approval_policy,
                    start_sandbox,
                    &inner_config,
                );
                json!({
                    "id": req_id,
                    "method": "thread/start",
                    "params": params
                })
            }
        };

        let initial_open = if let Some(ref fork_tid) = cfg.fork_from_session_id {
            CodexThreadOpenMode::Fork(fork_tid.as_str())
        } else if let Some(ref resume_tid) = cfg.resume_session_id {
            CodexThreadOpenMode::Resume(resume_tid.as_str())
        } else {
            CodexThreadOpenMode::Start
        };

        let start_msg = build_open_msg(thread_req_id, initial_open);
        if shim.write_stdin_line(start_msg.to_string()).await.is_err() {
            if !is_reattached {
                let _ = shim.kill().await;
            }
            return;
        }

        let thread_res = match wait_codex_id_response(
            &mut shim,
            thread_req_id,
            &cfg.shim_seq_tracker,
        )
        .await
        {
            Ok(v)
                if cfg.fork_from_session_id.is_none()
                    || v.pointer("/thread/id")
                        .and_then(|x| x.as_str())
                        .is_some_and(|tid| {
                            !tid.is_empty() && cfg.fork_from_session_id.as_deref() != Some(tid)
                        }) =>
            {
                if cfg.fork_from_session_id.is_none()
                    && cfg.resume_session_id.is_none()
                    && cfg.fallback_handoff_context.is_some()
                {
                    inject_handoff_on_next_prompt = true;
                }
                v
            }
            first_outcome
                if cfg.fork_from_session_id.is_some() || cfg.resume_session_id.is_some() =>
            {
                let first_err = match first_outcome {
                    Err(e) => e,
                    Ok(_) => "upstream thread/fork did not return a distinct thread id".to_string(),
                };
                let is_fork = cfg.fork_from_session_id.is_some();
                if is_fork && cfg.native_fork_only {
                    tracing::warn!(
                        agent_id = %cfg.agent_id,
                        error = %first_err,
                        "Codex native thread/fork unavailable during eager fork; deferring fresh session until first prompt"
                    );
                    let _ = shim.kill().await;
                    return;
                }
                let fallback_id = next_id;
                next_id += 1;
                let fallback_msg = build_open_msg(fallback_id, CodexThreadOpenMode::Start);
                if shim
                    .write_stdin_line(fallback_msg.to_string())
                    .await
                    .is_err()
                {
                    if !is_reattached {
                        let _ = shim.kill().await;
                    }
                    return;
                }
                match wait_codex_id_response(&mut shim, fallback_id, &cfg.shim_seq_tracker).await {
                    Ok(v) => {
                        inject_handoff_on_next_prompt = true;
                        let old_short: String = cfg
                            .fork_from_session_id
                            .as_deref()
                            .or(cfg.resume_session_id.as_deref())
                            .unwrap_or_default()
                            .chars()
                            .take(8)
                            .collect();
                        let notice_msg = if is_fork {
                            format!(
                                "Native Codex thread fork ({old_short}…) fell back to a fresh session with conversation context restored."
                            )
                        } else {
                            format!(
                                "Previous Codex session ({old_short}…) could not be resumed ({first_err}); started a fresh session with recent conversation context restored."
                            )
                        };
                        let _ = evt_tx
                            .send(DriverEvent::Notice {
                                level: "info".into(),
                                message: notice_msg,
                            })
                            .await;
                        v
                    }
                    Err(e) => {
                        let op_label = if is_fork { "fork" } else { "resume" };
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                message: format!(
                                    "Codex thread {op_label} failed ({first_err}); fresh thread start also failed: {e}"
                                ),
                                recoverable: false,
                            }))
                            .await;
                        if !is_reattached {
                            let _ = shim.kill().await;
                        }
                        return;
                    }
                }
            }
            Err(e) => {
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                        message: format!("Codex thread start failed: {e}"),
                        recoverable: false,
                    }))
                    .await;
                if !is_reattached {
                    let _ = shim.kill().await;
                }
                return;
            }
            Ok(v) => v,
        };

        let tid = thread_res
            .pointer("/thread/id")
            .and_then(|v| v.as_str())
            .or(
                if inject_handoff_on_next_prompt || cfg.fork_from_session_id.is_some() {
                    None
                } else {
                    cfg.resume_session_id.as_deref()
                },
            )
            .unwrap_or("unknown")
            .to_string();
        let requested_model = cfg
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let model = requested_model.or_else(|| {
            thread_res
                .get("model")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        });
        (tid, model)
    };
    let mut current_effort = cfg.effort;
    let mut current_fast_mode = cfg.fast_mode;
    let mut service_tier_explicitly_controlled = cfg.fast_mode;

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: thread_id.clone(),
            model: resolved_model.clone(),
        })
        .await;
    if !is_reattached || cfg.persisted_initializing {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }

    let mut native_turn_id: Option<String> = None;
    let mut client_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut msg_accum: HashMap<String, String> = HashMap::new();
    let mut thought_accum: HashMap<String, String> = HashMap::new();
    let mut tool_detail_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_proposed_plan: Option<String> = None;
    let mut pending_synthetic_plan_gate: Option<String> = None;
    let mut had_plan_collaboration_mode = matches!(mode, SessionMode::Plan);
    let mut pending_approvals: HashMap<String, PendingCodexApproval> = HashMap::new();
    let mut pending_questions: HashMap<String, PendingCodexQuestion> = HashMap::new();
    let mut pending_gate_order: VecDeque<String> = VecDeque::new();
    let mut pending_human_steers: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut queued_prompts: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<QueuedDriverPrompt> = None;
    let mut active_turn_message_ids: Vec<String> = Vec::new();
    let mut pending_turn_start_rpc_id: Option<u64> = None;
    let mut pending_interrupt_on_turn_started = false;
    let mut usage_tracker = CodexTurnUsageTracker::default();

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        mut prompt,
                        behavior,
                        origin,
                        message_ids,
                    } => {
                        if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            if let Some(ctx) = pending_handoff_context.take()
                                && !ctx.trim().is_empty()
                                && !prompt.contains("[Prior Conversation Context in this Workspace]")
                            {
                                prompt = format!("{ctx}\n\n{prompt}");
                            }
                        }
                        let has_pending_gates = !pending_approvals.is_empty()
                            || !pending_questions.is_empty()
                            || pending_synthetic_plan_gate.is_some();
                        if let Some(ref active_nat_tid) = native_turn_id {
                            let blocking_tid = client_turn_id
                                .clone()
                                .or_else(|| Some(active_nat_tid.clone()));
                            match behavior {
                                ActiveTurnBehavior::Steer => {
                                    // Option A: while any approval, question, or synthetic plan
                                    // gate is pending, queue non-destructively instead of sending
                                    // `turn/steer` mid-gate.
                                    if has_pending_gates {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &turn_id,
                                            &message_ids,
                                            AgentMessageDeliveryStatus::Queued,
                                            blocking_tid,
                                        )
                                        .await;
                                        let queued = QueuedDriverPrompt {
                                            turn_id,
                                            prompt,
                                            message_ids,
                                        };
                                        if origin == PromptOrigin::Human {
                                            pending_human_steers.push_back(queued);
                                        } else {
                                            queued_prompts.push_back(queued);
                                        }
                                        continue;
                                    }
                                    let input_items = build_codex_input_items_async(
                                        prompt,
                                        cfg.attachments_dir.clone(),
                                    )
                                    .await;
                                    let req_id = next_id;
                                    next_id += 1;
                                    let steer = json!({
                                        "id": req_id,
                                        "method": "turn/steer",
                                        "params": {
                                            "threadId": thread_id,
                                            "expectedTurnId": active_nat_tid,
                                            "input": input_items
                                        }
                                    });
                                    if shim.write_stdin_line(steer.to_string()).await.is_err() {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            client_turn_id.as_deref().unwrap_or(active_nat_tid),
                                            &message_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                    } else {
                                        active_turn_message_ids.extend(message_ids.iter().cloned());
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            client_turn_id.as_deref().unwrap_or(active_nat_tid),
                                            &message_ids,
                                            AgentMessageDeliveryStatus::Dispatched,
                                            None,
                                        )
                                        .await;
                                    }
                                    continue;
                                }
                                ActiveTurnBehavior::Queue => {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid,
                                    )
                                    .await;
                                    queued_prompts.push_back(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
                                    continue;
                                }
                                ActiveTurnBehavior::Interrupt => {
                                    pending_synthetic_plan_gate = None;
                                    cancel_pending_codex_approvals_and_questions(
                                        &shim,
                                        &mut pending_approvals,
                                        &mut pending_questions,
                                        &mut pending_gate_order,
                                    )
                                    .await;
                                    let req_id = next_id;
                                    next_id += 1;
                                    let req = json!({
                                        "id": req_id,
                                        "method": "turn/interrupt",
                                        "params": {
                                            "threadId": thread_id,
                                            "turnId": active_nat_tid
                                        }
                                    });
                                    let _ = shim.write_stdin_line(req.to_string()).await;
                                    if let Some(prev_rep) = pending_interrupt_replacement.take() {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &prev_rep.turn_id,
                                            &prev_rep.message_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                    }
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid,
                                    )
                                    .await;
                                    pending_interrupt_replacement = Some(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
                                    continue;
                                }
                            }
                        } else if let Some(ref blocking_ctid) = client_turn_id {
                            let blocking_tid = Some(blocking_ctid.clone());
                            match behavior {
                                ActiveTurnBehavior::Queue => {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid,
                                    )
                                    .await;
                                    queued_prompts.push_back(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
                                }
                                ActiveTurnBehavior::Steer => {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid,
                                    )
                                    .await;
                                    let queued = QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    };
                                    if origin == PromptOrigin::Human {
                                        pending_human_steers.push_back(queued);
                                    } else {
                                        queued_prompts.push_back(queued);
                                    }
                                }
                                ActiveTurnBehavior::Interrupt => {
                                    pending_synthetic_plan_gate = None;
                                    cancel_pending_codex_approvals_and_questions(
                                        &shim,
                                        &mut pending_approvals,
                                        &mut pending_questions,
                                        &mut pending_gate_order,
                                    )
                                    .await;
                                    pending_interrupt_on_turn_started = true;
                                    if let Some(prev_rep) = pending_interrupt_replacement.take() {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &prev_rep.turn_id,
                                            &prev_rep.message_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                    }
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid,
                                    )
                                    .await;
                                    pending_interrupt_replacement = Some(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
                                }
                            }
                            continue;
                        } else if has_pending_gates {
                            match behavior {
                                ActiveTurnBehavior::Interrupt => {
                                    // Explicit Interrupt clears any synthetic turn-end plan gate
                                    // or orphaned approval/question gate and immediately falls
                                    // through to dispatch the replacement turn (`turn/start`).
                                    pending_synthetic_plan_gate = None;
                                    cancel_pending_codex_approvals_and_questions(
                                        &shim,
                                        &mut pending_approvals,
                                        &mut pending_questions,
                                        &mut pending_gate_order,
                                    )
                                    .await;
                                }
                                ActiveTurnBehavior::Steer => {
                                    // Option A: hold steer prompts behind open plan/approval gate
                                    // in priority lane until user resolves or explicitly interrupts.
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        None,
                                    )
                                    .await;
                                    let queued = QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    };
                                    if origin == PromptOrigin::Human {
                                        pending_human_steers.push_back(queued);
                                    } else {
                                        queued_prompts.push_back(queued);
                                    }
                                    continue;
                                }
                                ActiveTurnBehavior::Queue => {
                                    // Hold queued prompts while a synthetic turn-end plan gate is
                                    // awaiting explicit user review so queued work never executes
                                    // before plan approval.
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        None,
                                    )
                                    .await;
                                    queued_prompts.push_back(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
                                    continue;
                                }
                            }
                        }

                        pending_interrupt_on_turn_started = false;
                        active_turn_message_ids = message_ids;
                        if dispatch_codex_turn(
                            &cfg,
                            &thread_id,
                            &mode,
                            &mut had_plan_collaboration_mode,
                            &resolved_model,
                            current_effort,
                            current_fast_mode,
                            service_tier_explicitly_controlled,
                            &turn_id,
                            &prompt,
                            &mut next_id,
                            &mut pending_turn_start_rpc_id,
                            &mut client_turn_id,
                            &mut msg_accum,
                            &mut thought_accum,
                            &shim,
                            &evt_tx,
                        )
                        .await
                        .is_err()
                        {
                            let dropped = std::mem::take(&mut active_turn_message_ids);
                            emit_prompt_delivery(
                                &evt_tx,
                                &turn_id,
                                &dropped,
                                AgentMessageDeliveryStatus::Dropped,
                                None,
                            )
                            .await;
                            emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                                .await;
                            emit_dropped_queued_prompts(
                                &evt_tx,
                                pending_interrupt_replacement.take(),
                                &mut queued_prompts,
                            )
                            .await;
                            break;
                        }
                        emit_prompt_delivery(
                            &evt_tx,
                            &turn_id,
                            &active_turn_message_ids,
                            AgentMessageDeliveryStatus::Dispatched,
                            None,
                        )
                        .await;
                    }
                    DriverCommand::Interrupt => {
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut queued_prompts,
                        )
                        .await;
                        let had_rpc_gates =
                            !pending_approvals.is_empty() || !pending_questions.is_empty();
                        cancel_pending_codex_approvals_and_questions(
                            &shim,
                            &mut pending_approvals,
                            &mut pending_questions,
                            &mut pending_gate_order,
                        )
                        .await;
                        let had_synthetic_gate = pending_synthetic_plan_gate.take().is_some();
                        if let Some(ref nat_tid) = native_turn_id {
                            pending_interrupt_on_turn_started = false;
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
                        } else if client_turn_id.is_some() {
                            pending_interrupt_on_turn_started = true;
                        } else if had_synthetic_gate || had_rpc_gates {
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                .await;
                        }
                    }
                    DriverCommand::InterruptAndDrainQueue => {
                        let had_rpc_gates =
                            !pending_approvals.is_empty() || !pending_questions.is_empty();
                        cancel_pending_codex_approvals_and_questions(
                            &shim,
                            &mut pending_approvals,
                            &mut pending_questions,
                            &mut pending_gate_order,
                        )
                        .await;
                        let had_synthetic_gate = pending_synthetic_plan_gate.take().is_some();
                        if let Some(ref nat_tid) = native_turn_id {
                            pending_interrupt_on_turn_started = false;
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
                        } else if client_turn_id.is_some() {
                            pending_interrupt_on_turn_started = true;
                        } else if let Some(next_q) = pending_interrupt_replacement
                            .take()
                            .or_else(|| pending_human_steers.pop_front())
                            .or_else(|| queued_prompts.pop_front())
                        {
                            pending_interrupt_on_turn_started = false;
                            active_turn_message_ids = next_q.message_ids;
                            if dispatch_codex_turn(
                                &cfg,
                                &thread_id,
                                &mode,
                                &mut had_plan_collaboration_mode,
                                &resolved_model,
                                current_effort,
                                current_fast_mode,
                                service_tier_explicitly_controlled,
                                &next_q.turn_id,
                                &next_q.prompt,
                                &mut next_id,
                                &mut pending_turn_start_rpc_id,
                                &mut client_turn_id,
                                &mut msg_accum,
                                &mut thought_accum,
                                &shim,
                                &evt_tx,
                            )
                            .await
                            .is_err()
                            {
                                let dropped = std::mem::take(&mut active_turn_message_ids);
                                emit_prompt_delivery(
                                    &evt_tx,
                                    &next_q.turn_id,
                                    &dropped,
                                    AgentMessageDeliveryStatus::Dropped,
                                    None,
                                )
                                .await;
                                emit_dropped_queued_prompts(
                                    &evt_tx,
                                    None,
                                    &mut pending_human_steers,
                                )
                                .await;
                                emit_dropped_queued_prompts(
                                    &evt_tx,
                                    pending_interrupt_replacement.take(),
                                    &mut queued_prompts,
                                )
                                .await;
                                break;
                            }
                            emit_prompt_delivery(
                                &evt_tx,
                                &next_q.turn_id,
                                &active_turn_message_ids,
                                AgentMessageDeliveryStatus::Dispatched,
                                None,
                            )
                            .await;
                        } else if had_synthetic_gate || had_rpc_gates {
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                .await;
                        }
                    }
                    DriverCommand::SetMode { mode: new_mode } => {
                        if matches!(mode, SessionMode::Plan) || matches!(new_mode, SessionMode::Plan)
                        {
                            had_plan_collaboration_mode = true;
                        }
                        mode = new_mode;
                    }
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode,
                    } => {
                        if let Some(m) = model {
                            if effort.is_none()
                                && m.to_ascii_lowercase().contains("luna")
                                && current_effort == Some(ReasoningEffort::Ultra)
                            {
                                current_effort = Some(ReasoningEffort::Max);
                            }
                            resolved_model = Some(m);
                        }
                        if effort.is_some() {
                            current_effort = effort;
                        }
                        if let Some(fm) = fast_mode {
                            current_fast_mode = fm;
                            service_tier_explicitly_controlled = true;
                        }
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
                            emit_codex_post_gate_status(
                                &pending_approvals,
                                &pending_questions,
                                &pending_synthetic_plan_gate,
                                &mut pending_gate_order,
                                &client_turn_id,
                                &evt_tx,
                            )
                            .await;
                        } else if pending_synthetic_plan_gate.as_deref() == Some(request_id.as_str()) {
                            pending_synthetic_plan_gate = None;
                            emit_codex_post_gate_status(
                                &pending_approvals,
                                &pending_questions,
                                &pending_synthetic_plan_gate,
                                &mut pending_gate_order,
                                &client_turn_id,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    DriverCommand::ResolvePlanApproval {
                        request_id,
                        action,
                        edited_plan_markdown,
                        target_mode,
                        ..
                    } => {
                        let rpc_app = pending_approvals.remove(&request_id);
                        let cleared_synthetic =
                            pending_synthetic_plan_gate.as_deref() == Some(request_id.as_str());
                        if cleared_synthetic {
                            pending_synthetic_plan_gate = None;
                        }
                        if rpc_app.is_none() && !cleared_synthetic {
                            continue;
                        }
                        if matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild) {
                            if matches!(mode, SessionMode::Plan) {
                                had_plan_collaboration_mode = true;
                            }
                            mode = target_mode;
                        }
                        if let Some(app) = rpc_app {
                            // Codex RPC plan approval (`accept`/`decline`) carries no inline edited
                            // plan field; when the user edited the plan, decline the stale gate so
                            // the daemon launches one clean build turn with the approved edits.
                            let has_edits = edited_plan_markdown
                                .as_deref()
                                .is_some_and(|s| !s.trim().is_empty());
                            let wire_dec = match action {
                                orbit_protocol::PlanApprovalAction::ApproveAndBuild
                                    if !has_edits =>
                                {
                                    "accept"
                                }
                                _ => "decline",
                            };
                            let resp = json!({
                                "id": app.rpc_id,
                                "result": { "decision": wire_dec }
                            });
                            let _ = shim.write_stdin_line(resp.to_string()).await;
                            emit_codex_post_gate_status(
                                &pending_approvals,
                                &pending_questions,
                                &pending_synthetic_plan_gate,
                                &mut pending_gate_order,
                                &client_turn_id,
                                &evt_tx,
                            )
                            .await;
                        } else if matches!(action, orbit_protocol::PlanApprovalAction::Dismiss) {
                            // Synthetic turn-end gate dismissed: resume any queued prompts held
                            // behind the gate when no other gates remain, or emit post-gate status.
                            if pending_approvals.is_empty()
                                && pending_questions.is_empty()
                                && let Some(next_q) = pending_interrupt_replacement
                                    .take()
                                    .or_else(|| pending_human_steers.pop_front())
                                    .or_else(|| queued_prompts.pop_front())
                            {
                                active_turn_message_ids = next_q.message_ids;
                                if dispatch_codex_turn(
                                    &cfg,
                                    &thread_id,
                                    &mode,
                                    &mut had_plan_collaboration_mode,
                                    &resolved_model,
                                    current_effort,
                                    current_fast_mode,
                                    service_tier_explicitly_controlled,
                                    &next_q.turn_id,
                                    &next_q.prompt,
                                    &mut next_id,
                                    &mut pending_turn_start_rpc_id,
                                    &mut client_turn_id,
                                    &mut msg_accum,
                                    &mut thought_accum,
                                    &shim,
                                    &evt_tx,
                                )
                                .await
                                .is_err()
                                {
                                    let dropped = std::mem::take(&mut active_turn_message_ids);
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &next_q.turn_id,
                                        &dropped,
                                        AgentMessageDeliveryStatus::Dropped,
                                        None,
                                    )
                                    .await;
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        None,
                                        &mut pending_human_steers,
                                    )
                                    .await;
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        pending_interrupt_replacement.take(),
                                        &mut queued_prompts,
                                    )
                                    .await;
                                    break;
                                }
                                emit_prompt_delivery(
                                    &evt_tx,
                                    &next_q.turn_id,
                                    &active_turn_message_ids,
                                    AgentMessageDeliveryStatus::Dispatched,
                                    None,
                                )
                                .await;
                            } else {
                                emit_codex_post_gate_status(
                                    &pending_approvals,
                                    &pending_questions,
                                    &pending_synthetic_plan_gate,
                                    &mut pending_gate_order,
                                    &client_turn_id,
                                    &evt_tx,
                                )
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
                            emit_codex_post_gate_status(
                                &pending_approvals,
                                &pending_questions,
                                &pending_synthetic_plan_gate,
                                &mut pending_gate_order,
                                &client_turn_id,
                                &evt_tx,
                            )
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
                let Some(frame) = maybe_frame else {
                    emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers).await;
                    emit_dropped_queued_prompts(
                        &evt_tx,
                        pending_interrupt_replacement.take(),
                        &mut queued_prompts,
                    )
                    .await;
                    if let Some(ref tid) = client_turn_id.clone().or_else(|| native_turn_id.clone()) {
                        let active_ids = std::mem::take(&mut active_turn_message_ids);
                        emit_prompt_delivery(
                            &evt_tx,
                            tid,
                            &active_ids,
                            AgentMessageDeliveryStatus::Dropped,
                            None,
                        )
                        .await;
                    }
                    break;
                };
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
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
                                &tool_detail_cache,
                                &shim,
                                &mut pending_approvals,
                                &mut pending_questions,
                                &mut pending_gate_order,
                                &evt_tx,
                            )
                            .await;
                        } else if rpc_id.is_none()
                            && let Some(m) = method
                        {
                            if m == "turn/completed" {
                                cancel_pending_codex_approvals_and_questions(
                                    &shim,
                                    &mut pending_approvals,
                                    &mut pending_questions,
                                    &mut pending_gate_order,
                                )
                                .await;
                                if let Some(ref tid) =
                                    client_turn_id.clone().or_else(|| native_turn_id.clone())
                                {
                                    let settled_ids = std::mem::take(&mut active_turn_message_ids);
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        tid,
                                        &settled_ids,
                                        AgentMessageDeliveryStatus::Settled,
                                        None,
                                    )
                                    .await;
                                }
                            }
                            let turn_completed = handle_codex_notification(
                                &cfg.agent_id,
                                m,
                                &val["params"],
                                &mode,
                                &mut native_turn_id,
                                &mut client_turn_id,
                                &mut msg_accum,
                                &mut thought_accum,
                                &mut tool_detail_cache,
                                &mut pending_proposed_plan,
                                &mut pending_synthetic_plan_gate,
                                &mut pending_gate_order,
                                &mut usage_tracker,
                                &evt_tx,
                            )
                            .await;
                            if m == "turn/started"
                                && pending_interrupt_on_turn_started
                                && let Some(ref nat_tid) = native_turn_id
                            {
                                pending_interrupt_on_turn_started = false;
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
                            if turn_completed {
                                pending_interrupt_on_turn_started = false;
                                pending_turn_start_rpc_id = None;
                                active_turn_message_ids.clear();
                                if pending_synthetic_plan_gate.is_none()
                                    && let Some(next_q) = pending_interrupt_replacement
                                        .take()
                                        .or_else(|| pending_human_steers.pop_front())
                                        .or_else(|| queued_prompts.pop_front())
                                {
                                    active_turn_message_ids = next_q.message_ids;
                                    if dispatch_codex_turn(
                                        &cfg,
                                        &thread_id,
                                        &mode,
                                        &mut had_plan_collaboration_mode,
                                        &resolved_model,
                                        current_effort,
                                        current_fast_mode,
                                        service_tier_explicitly_controlled,
                                        &next_q.turn_id,
                                        &next_q.prompt,
                                        &mut next_id,
                                        &mut pending_turn_start_rpc_id,
                                        &mut client_turn_id,
                                        &mut msg_accum,
                                        &mut thought_accum,
                                        &shim,
                                        &evt_tx,
                                    )
                                    .await
                                    .is_err()
                                    {
                                        let dropped =
                                            std::mem::take(&mut active_turn_message_ids);
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &next_q.turn_id,
                                            &dropped,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                        emit_dropped_queued_prompts(
                                            &evt_tx,
                                            None,
                                            &mut pending_human_steers,
                                        )
                                        .await;
                                        emit_dropped_queued_prompts(
                                            &evt_tx,
                                            pending_interrupt_replacement.take(),
                                            &mut queued_prompts,
                                        )
                                        .await;
                                        break;
                                    }
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &next_q.turn_id,
                                        &active_turn_message_ids,
                                        AgentMessageDeliveryStatus::Dispatched,
                                        None,
                                    )
                                    .await;
                                }
                            }
                        } else if rpc_id.is_some() && method.is_none() {
                            let is_turn_start_resp =
                                pending_turn_start_rpc_id.is_some() && pending_turn_start_rpc_id == rpc_id;
                            if is_turn_start_resp {
                                pending_turn_start_rpc_id = None;
                            }
                            if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                                let msg = err
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Codex RPC error")
                                    .to_string();
                                if is_turn_start_resp {
                                    let failed_tid = client_turn_id.take();
                                    native_turn_id = None;
                                    pending_interrupt_on_turn_started = false;
                                    msg_accum.clear();
                                    thought_accum.clear();
                                    let dropped_ids = std::mem::take(&mut active_turn_message_ids);
                                    if let Some(ref tid) = failed_tid {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            tid,
                                            &dropped_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                    }
                                    if pending_synthetic_plan_gate.is_none()
                                        && let Some(next_q) = pending_interrupt_replacement
                                            .take()
                                            .or_else(|| pending_human_steers.pop_front())
                                            .or_else(|| queued_prompts.pop_front())
                                    {
                                        active_turn_message_ids = next_q.message_ids;
                                        if dispatch_codex_turn(
                                            &cfg,
                                            &thread_id,
                                            &mode,
                                            &mut had_plan_collaboration_mode,
                                            &resolved_model,
                                            current_effort,
                                            current_fast_mode,
                                            service_tier_explicitly_controlled,
                                            &next_q.turn_id,
                                            &next_q.prompt,
                                            &mut next_id,
                                            &mut pending_turn_start_rpc_id,
                                            &mut client_turn_id,
                                            &mut msg_accum,
                                            &mut thought_accum,
                                            &shim,
                                            &evt_tx,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            let dropped =
                                                std::mem::take(&mut active_turn_message_ids);
                                            emit_prompt_delivery(
                                                &evt_tx,
                                                &next_q.turn_id,
                                                &dropped,
                                                AgentMessageDeliveryStatus::Dropped,
                                                None,
                                            )
                                            .await;
                                            emit_dropped_queued_prompts(
                                                &evt_tx,
                                                None,
                                                &mut pending_human_steers,
                                            )
                                            .await;
                                            emit_dropped_queued_prompts(
                                                &evt_tx,
                                                pending_interrupt_replacement.take(),
                                                &mut queued_prompts,
                                            )
                                            .await;
                                            break;
                                        }
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &next_q.turn_id,
                                            &active_turn_message_ids,
                                            AgentMessageDeliveryStatus::Dispatched,
                                            None,
                                        )
                                        .await;
                                    } else {
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                                message: msg,
                                                recoverable: true,
                                            }))
                                            .await;
                                    }
                                } else if !msg.contains("no active turn") {
                                    let _ = evt_tx
                                        .send(DriverEvent::Notice {
                                            level: "error".into(),
                                            message: msg,
                                        })
                                        .await;
                                }
                            } else if is_turn_start_resp
                                && native_turn_id.is_none()
                                && let Some(tid) =
                                    val.pointer("/result/turn/id").and_then(|v| v.as_str())
                            {
                                native_turn_id = Some(tid.to_string());
                                if pending_interrupt_on_turn_started {
                                    pending_interrupt_on_turn_started = false;
                                    let req_id = next_id;
                                    next_id += 1;
                                    let req = json!({
                                        "id": req_id,
                                        "method": "turn/interrupt",
                                        "params": {
                                            "threadId": thread_id,
                                            "turnId": tid
                                        }
                                    });
                                    let _ = shim.write_stdin_line(req.to_string()).await;
                                }
                            }
                        }
                    }
                    ShimFrame::Exited { code } => {
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut queued_prompts,
                        )
                        .await;
                        if let Some(ref tid) =
                            client_turn_id.clone().or_else(|| native_turn_id.clone())
                        {
                            let active_ids = std::mem::take(&mut active_turn_message_ids);
                            emit_prompt_delivery(
                                &evt_tx,
                                tid,
                                &active_ids,
                                AgentMessageDeliveryStatus::Dropped,
                                None,
                            )
                            .await;
                        }
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
    shim_seq_tracker: &std::sync::atomic::AtomicU64,
) -> Result<Value, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
        if rem.is_zero() {
            return Err(format!("Codex RPC #{expected_id} timed out"));
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
                        if val.get("method").is_none()
                            && val.get("id").and_then(|v| v.as_u64()) == Some(expected_id)
                        {
                            if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                                return Err(err.to_string());
                            }
                            return Ok(val.get("result").cloned().unwrap_or(Value::Null));
                        }
                    }
                    ShimFrame::Exited { code } => {
                        return Err(format!("codex app-server exited early ({code:?})"));
                    }
                    _ => {}
                }
            }
            _ => return Err("codex shim closed".into()),
        }
    }
}

fn codex_thread_mode_strings(mode: &SessionMode) -> (&'static str, &'static str) {
    match mode {
        SessionMode::FullAccess => ("never", "danger-full-access"),
        SessionMode::Plan => ("on-request", "read-only"),
        // Auto (Guardian) keeps approval policy `on-request` so Orbit's daemon classifies requests.
        SessionMode::Auto => ("on-request", "workspace-write"),
        _ => ("on-request", "workspace-write"),
    }
}

fn codex_turn_policies(mode: &SessionMode, extra_dirs: &[PathBuf]) -> (&'static str, Value) {
    let writable_roots: Vec<String> = extra_dirs
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    match mode {
        SessionMode::FullAccess => ("never", json!({ "type": "dangerFullAccess" })),
        SessionMode::Plan => ("on-request", json!({ "type": "readOnly" })),
        SessionMode::Auto
        | SessionMode::Default
        | SessionMode::AcceptEdits
        | SessionMode::Custom(_) => (
            "on-request",
            json!({
                "type": "workspaceWrite",
                "networkAccess": false,
                "writableRoots": writable_roots,
                "excludeSlashTmp": false,
                "excludeTmpdirEnvVar": false
            }),
        ),
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_codex_turn(
    cfg: &SessionLaunchConfig,
    thread_id: &str,
    mode: &SessionMode,
    had_plan_collaboration_mode: &mut bool,
    resolved_model: &Option<String>,
    current_effort: Option<ReasoningEffort>,
    fast_mode: bool,
    service_tier_explicitly_controlled: bool,
    turn_id: &str,
    prompt: &str,
    next_id: &mut u64,
    pending_turn_start_rpc_id: &mut Option<u64>,
    client_turn_id: &mut Option<String>,
    msg_accum: &mut HashMap<String, String>,
    thought_accum: &mut HashMap<String, String>,
    shim: &orbit_shim::ShimClient,
    evt_tx: &mpsc::Sender<DriverEvent>,
) -> Result<(), ()> {
    let input_items =
        build_codex_input_items_async(prompt.to_string(), cfg.attachments_dir.clone()).await;
    *client_turn_id = Some(turn_id.to_string());
    msg_accum.clear();
    thought_accum.clear();
    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Running {
            turn_id: turn_id.to_string(),
        }))
        .await;

    let explicit_default_collab =
        *had_plan_collaboration_mode && !matches!(mode, SessionMode::Plan);
    if matches!(mode, SessionMode::Plan) {
        *had_plan_collaboration_mode = true;
    }

    let req_id = *next_id;
    *next_id += 1;
    *pending_turn_start_rpc_id = Some(req_id);
    if prompt.trim() == "/compact" {
        return shim.write_stdin_line(json!({"id":req_id,"method":"thread/compact/start","params":{"threadId":thread_id}}).to_string()).await.map_err(|_|());
    }
    let params = build_codex_turn_start_params(
        cfg,
        thread_id,
        mode,
        explicit_default_collab,
        resolved_model,
        current_effort,
        fast_mode,
        service_tier_explicitly_controlled,
        Value::Array(input_items),
    );
    if explicit_default_collab && params.get("collaborationMode").is_some() {
        *had_plan_collaboration_mode = false;
    }

    let req = json!({
        "id": req_id,
        "method": "turn/start",
        "params": params
    });
    shim.write_stdin_line(req.to_string()).await.map_err(|_| ())
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CodexUsageBreakdown {
    /// Total prompt/input tokens reported by Codex (includes cached input tokens).
    raw_input_tokens: u64,
    /// Output tokens reported by Codex (already includes reasoning output tokens).
    output_tokens: u64,
    /// Cached portion of `raw_input_tokens`.
    cache_read_tokens: u64,
}

impl CodexUsageBreakdown {
    fn from_value(u: &Value) -> Option<Self> {
        let raw_input_tokens = u
            .get("inputTokens")
            .or_else(|| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let output_tokens = u
            .get("outputTokens")
            .or_else(|| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let cache_read_tokens = u
            .get("cachedInputTokens")
            .or_else(|| u.get("cached_input_tokens"))
            .or_else(|| u.get("cache_read_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if raw_input_tokens == 0 && output_tokens == 0 && cache_read_tokens == 0 {
            return None;
        }
        Some(Self {
            raw_input_tokens,
            output_tokens,
            cache_read_tokens,
        })
    }

    fn is_non_zero(self) -> bool {
        self.raw_input_tokens > 0 || self.output_tokens > 0 || self.cache_read_tokens > 0
    }

    fn saturating_add(self, rhs: Self) -> Self {
        Self {
            raw_input_tokens: self.raw_input_tokens.saturating_add(rhs.raw_input_tokens),
            output_tokens: self.output_tokens.saturating_add(rhs.output_tokens),
            cache_read_tokens: self.cache_read_tokens.saturating_add(rhs.cache_read_tokens),
        }
    }

    fn delta_since(self, prev: Self) -> Self {
        if self.raw_input_tokens < prev.raw_input_tokens || self.output_tokens < prev.output_tokens
        {
            return self;
        }
        Self {
            raw_input_tokens: self.raw_input_tokens.saturating_sub(prev.raw_input_tokens),
            output_tokens: self.output_tokens.saturating_sub(prev.output_tokens),
            cache_read_tokens: self
                .cache_read_tokens
                .saturating_sub(prev.cache_read_tokens),
        }
    }

    fn uncached_input_tokens(self) -> u64 {
        self.raw_input_tokens.saturating_sub(self.cache_read_tokens)
    }
}

/// Tracks Codex per-turn token usage across multi-step tool loops so exactly one
/// `DriverEvent::TurnUsage` is emitted per completed turn with accurate turn delta
/// and instantaneous single-call prompt occupancy.
#[derive(Debug, Default)]
struct CodexTurnUsageTracker {
    prev_total: Option<CodexUsageBreakdown>,
    latest_total: Option<CodexUsageBreakdown>,
    accum_last: CodexUsageBreakdown,
    last_call_prompt_tokens: Option<u64>,
    context_window_tokens: Option<u64>,
    model: Option<String>,
}

impl CodexTurnUsageTracker {
    fn on_token_usage_updated(&mut self, params: &Value) {
        if let Some(m) = params
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
        {
            self.model = Some(m.to_string());
        }
        if let Some(window) = params
            .pointer("/tokenUsage/modelContextWindow")
            .or_else(|| params.pointer("/token_usage/model_context_window"))
            .or_else(|| params.pointer("/info/model_context_window"))
            .or_else(|| params.get("modelContextWindow"))
            .or_else(|| params.get("model_context_window"))
            .and_then(|v| v.as_u64())
            .filter(|&w| w > 0)
        {
            self.context_window_tokens = Some(window);
        }
        let total = params
            .pointer("/tokenUsage/total")
            .or_else(|| params.pointer("/token_usage/total"))
            .or_else(|| params.get("total"))
            .and_then(CodexUsageBreakdown::from_value);
        let last = params
            .pointer("/tokenUsage/last")
            .or_else(|| params.pointer("/token_usage/last"))
            .or_else(|| params.get("last"))
            .and_then(CodexUsageBreakdown::from_value);

        if let Some(l) = last {
            if l.raw_input_tokens > 0 {
                self.last_call_prompt_tokens = Some(l.raw_input_tokens);
            }
            if total.is_none() {
                self.accum_last = self.accum_last.saturating_add(l);
            }
        }
        if let Some(t) = total {
            self.latest_total = Some(t);
        }
    }

    fn finalize_turn(&mut self, params: &Value) -> Option<DriverEvent> {
        let completed_usage = params
            .pointer("/turn/usage")
            .or_else(|| params.get("usage"))
            .and_then(CodexUsageBreakdown::from_value);
        let model = params
            .pointer("/turn/model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.model.take());

        let latest_total = self.latest_total.take();
        let accum_last = std::mem::take(&mut self.accum_last);
        let last_call_prompt_tokens = self.last_call_prompt_tokens.take();
        let context_window_tokens = self.context_window_tokens;

        let breakdown = if let Some(u) = completed_usage {
            if let Some(t) = latest_total {
                self.prev_total = Some(t);
            }
            Some(u)
        } else if let Some(t) = latest_total {
            let delta = match self.prev_total {
                Some(prev) => t.delta_since(prev),
                None => t,
            };
            self.prev_total = Some(t);
            delta.is_non_zero().then_some(delta)
        } else if accum_last.is_non_zero() {
            Some(accum_last)
        } else {
            None
        }?;

        Some(DriverEvent::TurnUsage {
            input_tokens: breakdown.uncached_input_tokens(),
            output_tokens: breakdown.output_tokens,
            cache_read_tokens: breakdown.cache_read_tokens,
            cache_write_tokens: 0,
            last_call_prompt_tokens,
            context_window_tokens,
            model,
        })
    }
}

async fn cancel_pending_codex_approvals_and_questions(
    shim: &orbit_shim::ShimClient,
    pending_approvals: &mut HashMap<String, PendingCodexApproval>,
    pending_questions: &mut HashMap<String, PendingCodexQuestion>,
    pending_gate_order: &mut VecDeque<String>,
) {
    pending_gate_order.clear();
    for (_, app) in pending_approvals.drain() {
        let resp = json!({
            "id": app.rpc_id,
            "result": { "decision": "decline" }
        });
        let _ = shim.write_stdin_line(resp.to_string()).await;
    }
    for (_, pq) in pending_questions.drain() {
        let resp = json!({
            "id": pq.rpc_id,
            "result": { "answers": {} }
        });
        let _ = shim.write_stdin_line(resp.to_string()).await;
    }
}

async fn emit_codex_post_gate_status(
    pending_approvals: &HashMap<String, PendingCodexApproval>,
    pending_questions: &HashMap<String, PendingCodexQuestion>,
    pending_synthetic_plan_gate: &Option<String>,
    pending_gate_order: &mut VecDeque<String>,
    client_turn_id: &Option<String>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    while let Some(front) = pending_gate_order.front() {
        let still_pending = pending_approvals.contains_key(front)
            || pending_questions.contains_key(front)
            || pending_synthetic_plan_gate.as_deref() == Some(front.as_str());
        if still_pending {
            break;
        }
        pending_gate_order.pop_front();
    }
    let next_req_id = pending_gate_order
        .front()
        .cloned()
        .or_else(|| pending_approvals.keys().next().cloned())
        .or_else(|| pending_questions.keys().next().cloned())
        .or_else(|| pending_synthetic_plan_gate.clone());
    if let Some(req_id) = next_req_id {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(
                AgentStatus::AwaitingPermission {
                    turn_id: client_turn_id.clone().unwrap_or_default(),
                    request_id: req_id,
                },
            ))
            .await;
    } else if let Some(tid) = client_turn_id {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                turn_id: tid.clone(),
            }))
            .await;
    } else {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_codex_notification(
    parent_agent_id: &str,
    method: &str,
    params: &Value,
    mode: &SessionMode,
    native_turn_id: &mut Option<String>,
    client_turn_id: &mut Option<String>,
    msg_accum: &mut HashMap<String, String>,
    thought_accum: &mut HashMap<String, String>,
    tool_detail_cache: &mut HashMap<String, ToolCallDetail>,
    pending_proposed_plan: &mut Option<String>,
    pending_synthetic_plan_gate: &mut Option<String>,
    pending_gate_order: &mut VecDeque<String>,
    usage_tracker: &mut CodexTurnUsageTracker,
    evt_tx: &mpsc::Sender<DriverEvent>,
) -> bool {
    match method {
        "turn/started" => {
            *pending_proposed_plan = None;
            if let Some(tid) = params.pointer("/turn/id").and_then(|v| v.as_str()) {
                *native_turn_id = Some(tid.to_string());
                if client_turn_id.is_none() {
                    *client_turn_id = Some(tid.to_string());
                }
            }
        }
        "item/agentMessage/delta" => {
            let turn_scope = native_turn_id
                .as_deref()
                .or(client_turn_id.as_deref())
                .unwrap_or("turn0");
            let item_id = params
                .get("itemId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("codex_msg_{turn_scope}"));
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
            let turn_scope = native_turn_id
                .as_deref()
                .or(client_turn_id.as_deref())
                .unwrap_or("turn0");
            let item_id = params
                .get("itemId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("codex_rsn_{turn_scope}"));
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
                return false;
            };
            let turn_scope = native_turn_id
                .as_deref()
                .or(client_turn_id.as_deref())
                .unwrap_or("turn0");
            let item_id = item
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("codex_msg_{turn_scope}"));
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
                            if matches!(mode, SessionMode::Plan)
                                && let Some(proposed) =
                                    orbit_protocol::extract_codex_proposed_plan(&final_text)
                            {
                                *pending_proposed_plan = Some(proposed);
                            }
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
                "plan" | "Plan" | "proposedPlan" | "ProposedPlan" => {
                    if is_done
                        && matches!(mode, SessionMode::Plan)
                        && let Some(plan_text) = item
                            .get("text")
                            .or_else(|| item.get("plan"))
                            .or_else(|| item.get("markdown"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    {
                        *pending_proposed_plan = Some(plan_text.to_string());
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
                            background_task: None,
                        })
                        .await;
                }
                "fileChange" | "FileChange" => {
                    let changes = item
                        .get("changes")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let detail = build_codex_file_change_detail(&changes);
                    tool_detail_cache.insert(item_id.clone(), detail.clone());
                    let status =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status,
                            detail,
                            background_task: None,
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
                            background_task: None,
                        })
                        .await;
                }
                "mcpToolCall" | "McpToolCall" => {
                    let server = item.get("server").and_then(|v| v.as_str()).unwrap_or("mcp");
                    let tool = item.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
                    let previous = tool_detail_cache.remove(&item_id);
                    let detail = if matches!(previous, Some(ToolCallDetail::Subagent { .. }))
                        || ((server.eq_ignore_ascii_case("orbit")
                            || tool.starts_with("mcp_orbit_")
                            || tool.starts_with("mcp__orbit__"))
                            && crate::managed_dispatch::is_launch(tool))
                    {
                        let dispatch = crate::managed_dispatch::detail(
                            &item["arguments"],
                            &item["result"],
                            previous.as_ref(),
                        );
                        if !is_done {
                            tool_detail_cache.insert(item_id.clone(), dispatch.clone());
                        }
                        dispatch
                    } else {
                        ToolCallDetail::Generic {
                            tool_name: format!("{server}/{tool}"),
                            arguments_json: truncate_str(&item["arguments"].to_string(), 800),
                        }
                    };
                    let status =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status,
                            detail,
                            background_task: None,
                        })
                        .await;
                }
                "collabAgentToolCall" | "CollabAgentToolCall" => {
                    let sub_id = item
                        .get("receiverThreadId")
                        .or_else(|| item.get("threadId"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(&item_id)
                        .to_string();
                    let title = item
                        .get("prompt")
                        .or_else(|| item.get("tool"))
                        .and_then(|v| v.as_str())
                        .map(|s| truncate_str(s, 72))
                        .unwrap_or_else(|| "Codex Subagent".to_string());
                    let st =
                        map_codex_item_status(item.get("status").and_then(|v| v.as_str()), is_done);
                    let pstatus = match st {
                        ToolCallStatus::Running => ProviderSubagentStatus::Running,
                        ToolCallStatus::Completed => ProviderSubagentStatus::Completed,
                        ToolCallStatus::Failed | ToolCallStatus::Denied => {
                            ProviderSubagentStatus::Failed
                        }
                    };
                    let now = Utc::now();
                    let _ = evt_tx
                        .send(DriverEvent::ProviderSubagentUpsert(
                            ProviderSubagentDescriptor {
                                subagent_id: sub_id.clone(),
                                parent_agent_id: parent_agent_id.to_string(),
                                parent_subagent_id: None,
                                title: title.clone(),
                                subtitle: Some("Codex".into()),
                                status: pstatus,
                                created_at: now,
                                updated_at: now,
                            },
                        ))
                        .await;
                    if is_done
                        && let Some(out) = item.get("result").and_then(|v| v.as_str())
                        && !out.trim().is_empty()
                    {
                        let _ = evt_tx
                            .send(DriverEvent::ProviderSubagentTimeline {
                                subagent_id: sub_id,
                                payload: TimelinePayload::AssistantMessage {
                                    item_id: format!("codex_sub_{item_id}"),
                                    text: out.to_string(),
                                    streaming: false,
                                },
                            })
                            .await;
                    }
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: item_id,
                            status: st,
                            detail: ToolCallDetail::Subagent {
                                child_agent_id: item
                                    .get("receiverThreadId")
                                    .and_then(|v| v.as_str())
                                    .map(ToOwned::to_owned),
                                role: "Codex Subagent".into(),
                                prompt_preview: title,
                            },
                            background_task: None,
                        })
                        .await;
                }
                _ => {}
            }
        }
        "thread/tokenUsage/updated" => {
            usage_tracker.on_token_usage_updated(params);
        }
        "turn/completed" => {
            let completed_turn_id = client_turn_id
                .clone()
                .or_else(|| native_turn_id.clone())
                .unwrap_or_default();
            *native_turn_id = None;
            *client_turn_id = None;
            let proposed_plan = pending_proposed_plan.take();
            if let Some(usage_evt) = usage_tracker.finalize_turn(params) {
                let _ = evt_tx.send(usage_evt).await;
            }
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
            } else if status_str == "completed"
                && matches!(mode, SessionMode::Plan)
                && let Some(plan_markdown) = proposed_plan
            {
                let req_id = format!("cplan_{}", uuid::Uuid::new_v4().simple());
                *pending_synthetic_plan_gate = Some(req_id.clone());
                pending_gate_order.push_back(req_id.clone());
                let plan_title = orbit_protocol::extract_plan_title(&plan_markdown);
                let title = plan_title
                    .clone()
                    .unwrap_or_else(|| "Review implementation plan".to_string());
                let payload = orbit_protocol::PlanApprovalPayload {
                    plan_markdown: plan_markdown.clone(),
                    plan_title,
                    plan_file_path: None,
                    summary: None,
                    plan_file_backed: false,
                    allowed_prompts: Vec::new(),
                    source: Some("codex_plan".to_string()),
                };
                let _ = evt_tx
                    .send(DriverEvent::PermissionRequest {
                        request_id: req_id.clone(),
                        title,
                        detail: ToolCallDetail::Generic {
                            tool_name: "ExitPlanMode".into(),
                            arguments_json: json!({ "plan": plan_markdown }).to_string(),
                        },
                        allowed_decisions: vec!["allow".into(), "deny".into()],
                        plan_approval: Some(payload),
                    })
                    .await;
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(
                        AgentStatus::AwaitingPermission {
                            turn_id: completed_turn_id,
                            request_id: req_id,
                        },
                    ))
                    .await;
            } else {
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                    .await;
            }
            return true;
        }
        _ => {}
    }
    false
}

#[allow(clippy::too_many_arguments)]
async fn handle_codex_server_request(
    rpc_id: u64,
    method: &str,
    params: &Value,
    mode: &SessionMode,
    client_turn_id: &Option<String>,
    tool_detail_cache: &HashMap<String, ToolCallDetail>,
    shim: &orbit_shim::ShimClient,
    pending_approvals: &mut HashMap<String, PendingCodexApproval>,
    pending_questions: &mut HashMap<String, PendingCodexQuestion>,
    pending_gate_order: &mut VecDeque<String>,
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
            pending_gate_order.push_back(req_id.clone());
            let _ = evt_tx
                .send(DriverEvent::PermissionRequest {
                    request_id: req_id.clone(),
                    title: format!("Run command: {cmd}"),
                    detail: ToolCallDetail::Shell {
                        command: cmd,
                        cwd,
                        exit_code: None,
                        output_preview: None,
                    },
                    allowed_decisions: vec!["allow".into(), "allow_always".into(), "deny".into()],
                    plan_approval: None,
                })
                .await;
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id,
                    },
                ))
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
            let cached_detail = tool_detail_cache.get(item_id).cloned();
            let allow_always_eligible = cached_detail
                .as_ref()
                .is_some_and(codex_file_change_single_concrete_path);
            let detail = cached_detail.unwrap_or_else(|| ToolCallDetail::FileEdit {
                path: "workspace files".into(),
                unified_diff: None,
            });
            let allowed_decisions = if allow_always_eligible {
                vec!["allow".into(), "allow_always".into(), "deny".into()]
            } else {
                vec!["allow".into(), "deny".into()]
            };
            pending_approvals.insert(req_id.clone(), PendingCodexApproval { rpc_id });
            pending_gate_order.push_back(req_id.clone());
            let _ = evt_tx
                .send(DriverEvent::PermissionRequest {
                    request_id: req_id.clone(),
                    title: "Approve file modifications".into(),
                    detail,
                    allowed_decisions,
                    plan_approval: None,
                })
                .await;
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id,
                    },
                ))
                .await;
        }
        "item/plan/requestApproval" | "plan/requestApproval" => {
            let req_id = format!("cplan_rpc_{rpc_id}");
            let plan_markdown = params
                .get("plan")
                .or_else(|| params.get("text"))
                .or_else(|| params.get("markdown"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let plan_title = params
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .or_else(|| orbit_protocol::extract_plan_title(&plan_markdown));
            let title = plan_title
                .clone()
                .unwrap_or_else(|| "Review implementation plan".to_string());
            let payload = orbit_protocol::PlanApprovalPayload {
                plan_markdown: plan_markdown.clone(),
                plan_title,
                plan_file_path: orbit_protocol::extract_plan_file_path(params),
                summary: None,
                plan_file_backed: false,
                allowed_prompts: Vec::new(),
                source: Some("codex_plan_rpc".to_string()),
            };
            pending_approvals.insert(req_id.clone(), PendingCodexApproval { rpc_id });
            pending_gate_order.push_back(req_id.clone());
            let _ = evt_tx
                .send(DriverEvent::PermissionRequest {
                    request_id: req_id.clone(),
                    title,
                    detail: ToolCallDetail::Generic {
                        tool_name: "ExitPlanMode".into(),
                        arguments_json: params.to_string(),
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    plan_approval: Some(payload),
                })
                .await;
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(
                    AgentStatus::AwaitingPermission {
                        turn_id: client_turn_id.clone().unwrap_or_default(),
                        request_id: req_id,
                    },
                ))
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
            pending_gate_order.push_back(req_id.clone());
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

fn build_codex_file_change_detail(changes: &[Value]) -> ToolCallDetail {
    let first_path = changes
        .first()
        .and_then(|c| c.get("path"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("files")
        .to_string();

    let mut sections: Vec<String> = Vec::with_capacity(changes.len());
    for change in changes {
        let path_opt = change
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let diff_str = change.get("diff").and_then(Value::as_str).unwrap_or("");
        let has_header = diff_str.lines().any(|line| {
            line.starts_with("--- ")
                || line.starts_with("+++ ")
                || line.starts_with("diff --git ")
                || line.starts_with("*** Update File: ")
                || line.starts_with("*** Add File: ")
                || line.starts_with("*** Delete File: ")
        });
        if let Some(p) = path_opt {
            if has_header {
                sections.push(diff_str.to_string());
            } else if diff_str.is_empty() {
                sections.push(format!("--- a/{p}\n+++ b/{p}"));
            } else {
                sections.push(format!("--- a/{p}\n+++ b/{p}\n{diff_str}"));
            }
        } else if !diff_str.is_empty() {
            sections.push(diff_str.to_string());
        }
    }

    let combined_diff = sections.join("\n");
    ToolCallDetail::FileEdit {
        path: first_path,
        unified_diff: if combined_diff.is_empty() {
            None
        } else {
            Some(crate::compact_unified_diff(
                &combined_diff,
                crate::MAX_UNIFIED_DIFF_BYTES,
            ))
        },
    }
}

fn codex_file_change_single_concrete_path(detail: &ToolCallDetail) -> bool {
    let ToolCallDetail::FileEdit { path, unified_diff } = detail else {
        return false;
    };
    let trimmed = path.trim();
    if trimmed.is_empty()
        || matches!(
            trimmed.to_ascii_lowercase().as_str(),
            "files" | "workspace files" | "multiple files" | "workspace"
        )
    {
        return false;
    }
    let Some(diff) = unified_diff.as_deref() else {
        return true;
    };
    if diff.contains("[diff truncated")
        || diff.contains("...(truncated")
        || diff.contains("... (truncated")
    {
        return false;
    }
    let mut distinct: Vec<String> = vec![path.clone()];
    for line in diff.lines() {
        let candidate = if let Some(rest) = line.strip_prefix("+++ ") {
            let p = rest.split('\t').next().unwrap_or(rest);
            Some(p.strip_prefix("b/").unwrap_or(p))
        } else if let Some(rest) = line.strip_prefix("--- ") {
            let p = rest.split('\t').next().unwrap_or(rest);
            Some(p.strip_prefix("a/").unwrap_or(p))
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            Some(rest)
        } else {
            line.strip_prefix("rename from ")
        };
        if let Some(p) = candidate
            && !p.is_empty()
            && p != "/dev/null"
            && p != "dev/null"
            && !distinct.iter().any(|existing| existing == p)
        {
            distinct.push(p.to_string());
            if distinct.len() > 1 {
                return false;
            }
        }
    }
    distinct.len() == 1
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
                .filter(|h| !h.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
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
                .filter(|h| !h.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
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

fn resolve_codex_model_default_effort(
    effort_entries: &[(ReasoningEffort, Option<String>)],
    configured_effort: Option<ReasoningEffort>,
    model_def_effort: Option<ReasoningEffort>,
) -> Option<ReasoningEffort> {
    if effort_entries.is_empty() {
        return configured_effort.or(model_def_effort);
    }
    let supports = |target: ReasoningEffort| effort_entries.iter().any(|(e, _)| *e == target);
    configured_effort
        .filter(|e| supports(*e))
        .or_else(|| model_def_effort.filter(|e| supports(*e)))
        .or_else(|| effort_entries.first().map(|(e, _)| *e))
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
    if out.is_empty()
        && let Some(def) = resolved_default
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

fn codex_tier_entry_supports_fast(entry: &Value) -> bool {
    if let Some(s) = entry.as_str() {
        return s.eq_ignore_ascii_case("fast") || s.eq_ignore_ascii_case("priority");
    }
    if let Some(obj) = entry.as_object() {
        let id_fast = obj
            .get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("fast") || s.eq_ignore_ascii_case("priority"));
        let name_fast = obj
            .get("name")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("fast"));
        return id_fast || name_fast;
    }
    false
}

pub fn codex_model_supports_fast_mode(model_id: &str, item: Option<&Value>) -> bool {
    if let Some(obj) = item {
        if let Some(explicit) = obj
            .get("supportsFastMode")
            .or_else(|| obj.get("supports_fast_mode"))
            .and_then(|v| v.as_bool())
        {
            return explicit;
        }
        let mut saw_tier_metadata = false;
        if let Some(speed_tiers) = obj
            .get("additionalSpeedTiers")
            .or_else(|| obj.get("additional_speed_tiers"))
            .and_then(|v| v.as_array())
        {
            saw_tier_metadata = true;
            if speed_tiers
                .iter()
                .filter_map(|v| v.as_str())
                .any(|t| t.eq_ignore_ascii_case("fast"))
            {
                return true;
            }
        }
        if let Some(tiers) = obj
            .get("supportedServiceTiers")
            .or_else(|| obj.get("supported_service_tiers"))
            .or_else(|| obj.get("serviceTiers"))
            .or_else(|| obj.get("service_tiers"))
            .and_then(|v| v.as_array())
        {
            saw_tier_metadata = true;
            if tiers.iter().any(codex_tier_entry_supports_fast) {
                return true;
            }
        }
        if saw_tier_metadata {
            return false;
        }
    }
    let lower = model_id.trim().to_ascii_lowercase();
    if lower.starts_with("gpt-oss") {
        return false;
    }
    lower.starts_with("gpt-5") || lower.starts_with("gpt-6") || lower.contains("codex")
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
            let resolved_eff =
                resolve_codex_model_default_effort(&effort_entries, cfg_effort, model_default_eff);

            parsed_models.push(ProviderModelSpec {
                id: id.to_string(),
                label: format_codex_display_name(display_raw, id),
                description,
                is_default: false,
                default_effort: resolved_eff,
                thinking_options: build_codex_thinking_options(&effort_entries, resolved_eff),
                supports_fast_mode: codex_model_supports_fast_mode(id, Some(item)),
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
    let luna_efforts: Vec<(ReasoningEffort, Option<String>)> = full_efforts
        .iter()
        .filter(|(eff, _)| *eff != ReasoningEffort::Ultra)
        .cloned()
        .collect();
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
            let entries = if id.contains("luna") {
                &luna_efforts
            } else {
                &full_efforts
            };
            let resolved_eff =
                resolve_codex_model_default_effort(entries, cfg_effort, Some(def_eff))
                    .unwrap_or(def_eff);
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
                thinking_options: build_codex_thinking_options(entries, Some(resolved_eff)),
                supports_fast_mode: codex_model_supports_fast_mode(id, None),
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
        let mut cmd = Command::new(exe);
        cmd.arg("app-server")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::shell_snapshot::apply_cached_shell_env(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;

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
        let resolved_default_effort = resolve_codex_model_default_effort(
            &effort_entries,
            configured_effort,
            model_def_effort,
        );

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
            supports_fast_mode: codex_model_supports_fast_mode(id, Some(item)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::PromptOrigin;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    fn sample_cfg(fast_mode: bool) -> SessionLaunchConfig {
        SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_codex_test".into(),
            provider: ProviderKind::Codex,
            executable: PathBuf::from("/usr/bin/codex"),
            orbit_exe: PathBuf::from("/usr/bin/orbit"),
            shim_socket: PathBuf::from("/tmp/codex.sock"),
            runtime_dir: std::env::temp_dir(),
            cwd: PathBuf::from("/repo"),
            extra_dirs: Vec::new(),
            attachments_dir: None,
            model: Some("gpt-5.4".into()),
            effort: Some(ReasoningEffort::Medium),
            fast_mode,
            mode: SessionMode::AcceptEdits,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: Vec::new(),
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: None,
            acp_spec: None,
        }
    }

    #[test]
    fn launch_args_and_rpc_params_respect_fast_mode() {
        assert_eq!(build_codex_launch_args(false), vec!["app-server"]);
        assert_eq!(
            build_codex_launch_args(true),
            vec!["app-server", "-c", "service_tier=\"fast\""]
        );

        let empty_cfg = serde_json::Map::new();
        let off_thread = build_codex_thread_start_params(
            &sample_cfg(false),
            "on-request",
            "workspace-write",
            &empty_cfg,
        );
        assert!(off_thread.get("serviceTier").is_none());
        assert!(off_thread.get("service_tier").is_none());

        let on_thread = build_codex_thread_start_params(
            &sample_cfg(true),
            "on-request",
            "workspace-write",
            &empty_cfg,
        );
        assert_eq!(on_thread["serviceTier"], json!("fast"));
        assert!(on_thread.get("service_tier").is_none());

        let resume_off = build_codex_thread_resume_params("thr_old", None, false, &empty_cfg);
        assert!(resume_off.get("model").is_none());
        assert!(resume_off.get("serviceTier").is_none());
        let resume_on =
            build_codex_thread_resume_params("thr_old", Some("gpt-6.1-terra"), true, &empty_cfg);
        assert_eq!(resume_on["model"], json!("gpt-6.1-terra"));
        assert_eq!(resume_on["serviceTier"], json!("fast"));

        let cfg = sample_cfg(false);
        let turn_uncontrolled_off = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::AcceptEdits,
            false,
            &Some("gpt-5.4".into()),
            Some(ReasoningEffort::High),
            false,
            false,
            json!([]),
        );
        assert!(turn_uncontrolled_off.get("serviceTier").is_none());

        let turn_on = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::AcceptEdits,
            false,
            &Some("gpt-5.4".into()),
            Some(ReasoningEffort::High),
            true,
            true,
            json!([]),
        );
        assert_eq!(turn_on["serviceTier"], json!("fast"));

        let turn_explicit_reset = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::AcceptEdits,
            false,
            &Some("gpt-5.4".into()),
            Some(ReasoningEffort::High),
            false,
            true,
            json!([]),
        );
        assert_eq!(
            turn_explicit_reset
                .as_object()
                .expect("object")
                .get("serviceTier"),
            Some(&Value::Null)
        );
    }

    #[test]
    fn codex_catalog_marks_gpt5_and_codex_models_as_fast_capable() {
        assert!(codex_model_supports_fast_mode("gpt-5.4", None));
        assert!(codex_model_supports_fast_mode("gpt-5.3-codex", None));
        assert!(codex_model_supports_fast_mode("gpt-6.2-sol", None));
        assert!(!codex_model_supports_fast_mode("gpt-oss-120b", None));
        assert!(!codex_model_supports_fast_mode("o3-mini", None));
        assert!(codex_model_supports_fast_mode(
            "custom-model",
            Some(&json!({ "supportedServiceTiers": ["standard", "fast"] }))
        ));
        assert!(codex_model_supports_fast_mode(
            "custom-model",
            Some(&json!({ "additionalSpeedTiers": ["fast"] }))
        ));
        assert!(codex_model_supports_fast_mode(
            "custom-model",
            Some(&json!({ "serviceTiers": [{ "id": "priority", "name": "Fast" }] }))
        ));
        assert!(!codex_model_supports_fast_mode(
            "gpt-6.1-terra",
            Some(&json!({ "additionalSpeedTiers": [], "serviceTiers": [] }))
        ));
        assert!(!codex_model_supports_fast_mode(
            "gpt-5.4",
            Some(&json!({ "supportsFastMode": false }))
        ));
    }

    #[tokio::test]
    async fn codex_live_fast_mode_toggle_updates_subsequent_turn_start() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_codex_fast_toggle_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_turns: Arc<tokio::sync::Mutex<Vec<Value>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed_bg = Arc::clone(&observed_turns);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            let mut ack =
                serde_json::to_string(&ShimFrame::AttachAck { latest_seq: 0 }).expect("ack");
            ack.push('\n');
            let _ = wh.write_all(ack.as_bytes()).await;
            let _ = wh.flush().await;

            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(rpc) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let id = rpc.get("id").cloned().unwrap_or(Value::Null);
                let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "thread/start" => {
                        assert!(rpc["params"].get("serviceTier").is_none());
                        assert!(rpc["params"].get("service_tier").is_none());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": id,
                                "result": {
                                    "thread": { "id": "thr_live_1" },
                                    "model": "gpt-5.4"
                                }
                            }),
                        )
                        .await;
                    }
                    "turn/start" => {
                        let turn_idx = {
                            let mut guard = observed_bg.lock().await;
                            guard.push(rpc["params"].clone());
                            guard.len()
                        };
                        let nat_tid = format!("nat_turn_{turn_idx}");
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": id,
                                "result": { "turn": { "id": nat_tid } }
                            }),
                        )
                        .await;
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "method": "turn/completed",
                                "params": { "turn": { "id": nat_tid, "status": "completed" } }
                            }),
                        )
                        .await;
                        if turn_idx >= 2 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let mut cfg = sample_cfg(false);
        cfg.shim_socket = sock_path.clone();

        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // Toggle fast mode ON without specifying model/effort -> must preserve existing model & effort!
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(true),
            })
            .await
            .expect("toggle on");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "first".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("prompt 1");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn 1 timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // Toggle fast mode OFF -> next turn omits service_tier
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(false),
            })
            .await
            .expect("toggle off");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t2".into(),
                prompt: "second".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("prompt 2");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn 2 timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        let turns = observed_turns.lock().await;
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0]["serviceTier"], json!("fast"));
        assert!(turns[0].get("service_tier").is_none());
        assert_eq!(turns[0]["model"], json!("gpt-5.4"));
        assert_eq!(turns[0]["effort"], json!("medium"));
        assert_eq!(
            turns[1].as_object().expect("turn 2 obj").get("serviceTier"),
            Some(&Value::Null)
        );
        assert_eq!(turns[1]["model"], json!("gpt-5.4"));
        assert_eq!(turns[1]["effort"], json!("medium"));

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn codex_start_fast_true_then_toggle_false_sends_null_service_tier() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_codex_fast_reset_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_thread_start: Arc<tokio::sync::Mutex<Option<Value>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let observed_turns: Arc<tokio::sync::Mutex<Vec<Value>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed_ts_bg = Arc::clone(&observed_thread_start);
        let observed_turns_bg = Arc::clone(&observed_turns);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            let mut ack =
                serde_json::to_string(&ShimFrame::AttachAck { latest_seq: 0 }).expect("ack");
            ack.push('\n');
            let _ = wh.write_all(ack.as_bytes()).await;
            let _ = wh.flush().await;

            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(rpc) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let id = rpc.get("id").cloned().unwrap_or(Value::Null);
                let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "thread/start" => {
                        *observed_ts_bg.lock().await = Some(rpc["params"].clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": id,
                                "result": {
                                    "thread": { "id": "thr_fast_init" },
                                    "model": "gpt-5.4"
                                }
                            }),
                        )
                        .await;
                    }
                    "turn/start" => {
                        observed_turns_bg.lock().await.push(rpc["params"].clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": id,
                                "result": { "turn": { "id": "nat_turn_reset" } }
                            }),
                        )
                        .await;
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "method": "turn/completed",
                                "params": { "turn": { "id": "nat_turn_reset", "status": "completed" } }
                            }),
                        )
                        .await;
                        break;
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let mut cfg = sample_cfg(true);
        cfg.shim_socket = sock_path.clone();

        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // Disable fast mode on a session that booted with `fast_mode: true`
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(false),
            })
            .await
            .expect("toggle off");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_reset".into(),
                prompt: "standard tier please".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("prompt");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        let ts = observed_thread_start
            .lock()
            .await
            .clone()
            .expect("thread/start observed");
        assert_eq!(ts["serviceTier"], json!("fast"));

        let turns = observed_turns.lock().await;
        assert_eq!(turns.len(), 1);
        assert_eq!(
            turns[0].as_object().expect("turn obj").get("serviceTier"),
            Some(&Value::Null),
            "toggling true -> false must emit explicit null serviceTier to override -c service_tier=fast"
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn codex_turn_usage_tracker_emits_once_per_turn_without_reasoning_double_count() {
        let mut tracker = CodexTurnUsageTracker::default();

        // Turn 1: 3 tool-call steps. `outputTokens` ALREADY includes `reasoningOutputTokens`.
        // `tokenUsage.total` is cumulative across the thread; `tokenUsage.last` is the single call.
        tracker.on_token_usage_updated(&json!({
            "model": "gpt-5.4",
            "tokenUsage": {
                "modelContextWindow": 272000,
                "last": { "inputTokens": 20000, "cachedInputTokens": 12000, "outputTokens": 800, "reasoningOutputTokens": 500 },
                "total": { "inputTokens": 20000, "cachedInputTokens": 12000, "outputTokens": 800, "reasoningOutputTokens": 500 }
            }
        }));
        tracker.on_token_usage_updated(&json!({
            "tokenUsage": {
                "last": { "inputTokens": 28000, "cachedInputTokens": 20000, "outputTokens": 1100, "reasoningOutputTokens": 700 },
                "total": { "inputTokens": 48000, "cachedInputTokens": 32000, "outputTokens": 1900, "reasoningOutputTokens": 1200 }
            }
        }));
        tracker.on_token_usage_updated(&json!({
            "tokenUsage": {
                "last": { "inputTokens": 35000, "cachedInputTokens": 30000, "outputTokens": 900, "reasoningOutputTokens": 400 },
                "total": { "inputTokens": 83000, "cachedInputTokens": 62000, "outputTokens": 2800, "reasoningOutputTokens": 1600 }
            }
        }));

        // Emit once on `turn/completed` (without inline turn.usage -> uses total delta)
        let ev1 = tracker
            .finalize_turn(&json!({ "turn": { "status": "completed" } }))
            .expect("turn 1 usage");
        match ev1 {
            DriverEvent::TurnUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_write_tokens,
                last_call_prompt_tokens,
                context_window_tokens,
                model,
            } => {
                // Uncached input = 83000 - 62000 = 21000
                assert_eq!(input_tokens, 21_000);
                // Output is 2800 (NOT 2800 + 1600!)
                assert_eq!(output_tokens, 2_800);
                assert_eq!(cache_read_tokens, 62_000);
                assert_eq!(cache_write_tokens, 0);
                // Instantaneous single-call prompt occupancy is 35000 (NOT 83000!)
                assert_eq!(last_call_prompt_tokens, Some(35_000));
                assert_eq!(context_window_tokens, Some(272_000));
                assert_eq!(model.as_deref(), Some("gpt-5.4"));
            }
            other => panic!("expected TurnUsage, got {other:?}"),
        }

        // Turn 2: `total` continues growing cumulatively; tracker must emit ONLY the delta for Turn 2.
        tracker.on_token_usage_updated(&json!({
            "tokenUsage": {
                "last": { "inputTokens": 42000, "cachedInputTokens": 36000, "outputTokens": 600, "reasoningOutputTokens": 200 },
                "total": { "inputTokens": 125000, "cachedInputTokens": 98000, "outputTokens": 3400, "reasoningOutputTokens": 1800 }
            }
        }));
        let ev2 = tracker
            .finalize_turn(&json!({ "turn": { "status": "completed" } }))
            .expect("turn 2 usage");
        match ev2 {
            DriverEvent::TurnUsage {
                input_tokens,
                output_tokens,
                cache_read_tokens,
                last_call_prompt_tokens,
                ..
            } => {
                // Delta total input = 125000 - 83000 = 42000; delta cache = 98000 - 62000 = 36000; uncached = 6000
                assert_eq!(input_tokens, 6_000);
                assert_eq!(output_tokens, 600);
                assert_eq!(cache_read_tokens, 36_000);
                assert_eq!(last_call_prompt_tokens, Some(42_000));
            }
            other => panic!("expected TurnUsage, got {other:?}"),
        }
    }

    #[test]
    fn codex_plan_mode_turn_start_params_require_model_and_include_effort() {
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_plan".into(),
            provider: ProviderKind::Codex,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: PathBuf::from("/tmp/plan.sock"),
            runtime_dir: std::env::temp_dir(),
            cwd: PathBuf::from("/tmp/repo"),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gpt-5.4".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::Plan,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: None,
            acp_spec: None,
        };

        // With model + effort: collaborationMode includes required `settings.model` + `reasoning_effort`
        let with_model = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::Plan,
            false,
            &Some("gpt-5.4".to_string()),
            Some(ReasoningEffort::High),
            false,
            false,
            json!([]),
        );
        assert_eq!(with_model["collaborationMode"]["mode"], json!("plan"));
        assert_eq!(
            with_model["collaborationMode"]["settings"]["model"],
            json!("gpt-5.4")
        );
        assert_eq!(
            with_model["collaborationMode"]["settings"]["reasoning_effort"],
            json!("high")
        );
        assert_eq!(with_model["sandboxPolicy"]["type"], json!("readOnly"));

        // Exiting Plan mode -> Default MUST explicitly emit `collaborationMode: "default"` so
        // Codex app-server does not keep the thread stuck in Plan collaboration mode.
        let exiting_plan = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::Default,
            true,
            &Some("gpt-5.4".to_string()),
            Some(ReasoningEffort::High),
            false,
            false,
            json!([]),
        );
        assert_eq!(exiting_plan["collaborationMode"]["mode"], json!("default"));
        assert_eq!(
            exiting_plan["collaborationMode"]["settings"]["model"],
            json!("gpt-5.4")
        );
        assert_eq!(
            exiting_plan["collaborationMode"]["settings"]["reasoning_effort"],
            json!("high")
        );
        assert_eq!(
            exiting_plan["sandboxPolicy"]["type"],
            json!("workspaceWrite")
        );

        // With model but no effort: omits `reasoning_effort`, keeps `model`
        let with_model_no_effort = build_codex_turn_start_params(
            &cfg,
            "thr_1",
            &SessionMode::Plan,
            false,
            &Some("gpt-5.4".to_string()),
            None,
            false,
            false,
            json!([]),
        );
        assert_eq!(
            with_model_no_effort["collaborationMode"]["settings"]["model"],
            json!("gpt-5.4")
        );
        assert!(
            with_model_no_effort["collaborationMode"]["settings"]
                .get("reasoning_effort")
                .is_none()
        );

        // Without model (None or empty): omit `collaborationMode` so app-server never rejects with missing `model`
        for empty_model in [None, Some(String::new()), Some("   ".to_string())] {
            let without_model = build_codex_turn_start_params(
                &cfg,
                "thr_1",
                &SessionMode::Plan,
                false,
                &empty_model,
                Some(ReasoningEffort::Medium),
                false,
                false,
                json!([]),
            );
            assert!(
                without_model.get("collaborationMode").is_none(),
                "must omit collaborationMode when model is unset/empty"
            );
            assert_eq!(without_model["sandboxPolicy"]["type"], json!("readOnly"));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_turn_start_rpc_error_clears_running_and_startup_interrupt_fires_on_turn_started()
    {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let sock_path = std::env::temp_dir().join(format!("o_cdx_err_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let interrupted_turn_ids: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let int_bg = Arc::clone(&interrupted_turn_ids);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
            }

            let mut turn_start_count = 0u32;
            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(frame) = serde_json::from_str::<ShimFrame>(&raw_line) else {
                    continue;
                };
                let ShimFrame::WriteStdin { line } = frame else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "thread/start" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": req["id"],
                                "result": { "thread": { "id": "thr_err_test" } }
                            }),
                        )
                        .await;
                    }
                    "turn/start" => {
                        turn_start_count += 1;
                        if turn_start_count == 1 {
                            // Reject first turn/start with RPC error
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "id": req["id"],
                                    "error": {
                                        "code": -32602,
                                        "message": "Invalid request: missing field `model`"
                                    }
                                }),
                            )
                            .await;
                        } else if turn_start_count == 2 {
                            // Delay `turn/started` slightly so Interrupt arrives during startup window
                            tokio::time::sleep(Duration::from_millis(60)).await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "id": req["id"],
                                    "result": {}
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "method": "turn/started",
                                    "params": { "turn": { "id": "nat_turn_2" } }
                                }),
                            )
                            .await;
                        }
                    }
                    "turn/interrupt" => {
                        let tid = req
                            .pointer("/params/turnId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        int_bg.lock().await.push(tid.clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "method": "turn/completed",
                                "params": { "turn": { "id": tid, "status": "interrupted" } }
                            }),
                        )
                        .await;
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_cdx_err".into(),
            provider: ProviderKind::Codex,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gpt-5.4".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::Plan,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: None,
            acp_spec: None,
        };

        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // 1. First `turn/start` returns RPC error -> driver must transition out of Running to Error!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_err_1".into(),
                prompt: "bad turn".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send 1");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("err wait")
                .expect("ev");
            if let DriverEvent::StatusChanged(AgentStatus::Error {
                message,
                recoverable,
            }) = ev
            {
                assert!(recoverable);
                assert!(message.contains("missing field"));
                break;
            }
        }

        // 2. Second `turn/start` can now dispatch cleanly (not stuck in Running!), and Interrupt
        // during the `turn/start`..`turn/started` startup window must fire once `turn/started` arrives!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_win_2".into(),
                prompt: "slow startup".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send 2");
        // Wait for Running status (meaning `turn/start` was sent, before `turn/started` arrives)
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("running wait")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Running { .. })) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("interrupt during startup window");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("idle wait")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }
        assert_eq!(
            *interrupted_turn_ids.lock().await,
            vec!["nat_turn_2".to_string()],
            "startup-window Interrupt must fire turn/interrupt as soon as turn/started delivers native_turn_id"
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_queue_steer_and_interrupt_and_drain_queue_delivery_lifecycle() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::atomic::AtomicUsize;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let sock_path = std::env::temp_dir().join(format!("o_cdx_deliv_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let interrupt_count = Arc::new(AtomicUsize::new(0));
        let int_bg = Arc::clone(&interrupt_count);
        let steer_count = Arc::new(AtomicUsize::new(0));
        let steer_bg = Arc::clone(&steer_count);
        let started_turns: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let turns_bg = Arc::clone(&started_turns);
        let (finish_t1_tx, mut finish_t1_rx) = mpsc::channel::<()>(2);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
            }

            let mut active_nat_tid: Option<String> = None;
            let mut turn_num = 0usize;
            loop {
                tokio::select! {
                    _ = finish_t1_rx.recv() => {
                        if let Some(tid) = active_nat_tid.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "method": "turn/completed",
                                    "params": { "turn": { "id": tid, "status": "completed" } }
                                }),
                            )
                            .await;
                        }
                    }
                    maybe_line = lines.next_line() => {
                        let Ok(Some(raw_line)) = maybe_line else { break; };
                        let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw_line) else {
                            continue;
                        };
                        let Ok(req) = serde_json::from_str::<Value>(&line) else { continue; };
                        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        match method {
                            "thread/start" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "id": req["id"],
                                        "result": { "thread": { "id": "thr_deliv" } }
                                    }),
                                )
                                .await;
                            }
                            "turn/start" => {
                                turn_num += 1;
                                let nat_tid = format!("nat_{turn_num}");
                                let text = req
                                    .pointer("/params/input/0/text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                turns_bg.lock().await.push(text.clone());
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "id": req["id"],
                                        "result": { "turn": { "id": nat_tid } }
                                    }),
                                )
                                .await;
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "method": "turn/started",
                                        "params": { "turn": { "id": nat_tid } }
                                    }),
                                )
                                .await;
                                if text.starts_with("SLOW_") {
                                    active_nat_tid = Some(nat_tid);
                                } else {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "method": "turn/completed",
                                            "params": { "turn": { "id": nat_tid, "status": "completed" } }
                                        }),
                                    )
                                    .await;
                                }
                            }
                            "turn/steer" => {
                                steer_bg.fetch_add(1, Ordering::SeqCst);
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "id": req["id"],
                                        "result": {}
                                    }),
                                )
                                .await;
                            }
                            "turn/interrupt" => {
                                int_bg.fetch_add(1, Ordering::SeqCst);
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "id": req["id"],
                                        "result": {}
                                    }),
                                )
                                .await;
                                if let Some(tid) = active_nat_tid.take() {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "method": "turn/completed",
                                            "params": { "turn": { "id": tid, "status": "interrupted" } }
                                        }),
                                    )
                                    .await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let mut cfg = sample_cfg(false);
        cfg.agent_id = "ag_cdx_deliv".into();
        cfg.shim_socket = sock_path.clone();
        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));

        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("boot")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::Idle)
            ) {
                break;
            }
        }

        // 1. Slow turn -> Queue (0 interrupts) + native Steer (0 interrupts) -> natural settle drains Queue
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_s1".into(),
                prompt: "SLOW_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("slow 1");
        tokio::time::sleep(Duration::from_millis(60)).await;

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q1".into(),
                prompt: "Q1".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_q1".into()],
            })
            .await
            .expect("queue");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_steer".into(),
                prompt: "NATIVE_STEER".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_steer".into()],
            })
            .await
            .expect("steer");
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(interrupt_count.load(Ordering::SeqCst), 0);
        assert_eq!(steer_count.load(Ordering::SeqCst), 1);

        finish_t1_tx.send(()).await.expect("finish t1");

        let mut q1_events = Vec::new();
        let mut steer_events = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase 1 settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } => {
                    if message_ids == vec!["msg_q1".to_string()] {
                        q1_events.push(status);
                    } else if message_ids == vec!["msg_steer".to_string()] {
                        steer_events.push(status);
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if q1_events.contains(&AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(interrupt_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            steer_events,
            vec![
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );
        assert_eq!(
            q1_events,
            vec![
                AgentMessageDeliveryStatus::Queued,
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );

        // 2. InterruptAndDrainQueue interrupts active turn and preserves FIFO queue
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_s2".into(),
                prompt: "SLOW_2".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("slow 2");
        tokio::time::sleep(Duration::from_millis(60)).await;
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q2".into(),
                prompt: "Q2".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_q2".into()],
            })
            .await
            .expect("q2");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q3".into(),
                prompt: "Q3".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_q3".into()],
            })
            .await
            .expect("q3");
        cmd_tx
            .send(DriverCommand::InterruptAndDrainQueue)
            .await
            .expect("interrupt drain");

        let mut q3_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase 2 settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } => {
                    assert_ne!(status, AgentMessageDeliveryStatus::Dropped);
                    if message_ids == vec!["msg_q3".to_string()]
                        && status == AgentMessageDeliveryStatus::Settled
                    {
                        q3_settled = true;
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if q3_settled => break,
                _ => {}
            }
        }
        assert_eq!(interrupt_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            *started_turns.lock().await,
            vec!["SLOW_1", "Q1", "SLOW_2", "Q2", "Q3"]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn codex_file_change_detail_preserves_all_multi_file_paths_and_gates_allow_always() {
        // Multi-file change where raw per-file diffs omit `---`/`+++` headers
        let multi = build_codex_file_change_detail(&[
            json!({ "path": "src/main.rs", "diff": "@@ -1 +1 @@\n-old\n+new" }),
            json!({ "path": ".env", "diff": "@@ -1 +1 @@\n-SECRET=1\n+SECRET=2" }),
        ]);
        match &multi {
            ToolCallDetail::FileEdit { path, unified_diff } => {
                assert_eq!(path, "src/main.rs");
                let diff = unified_diff.as_deref().expect("diff present");
                assert!(diff.contains("+++ b/src/main.rs"), "diff:\n{diff}");
                assert!(diff.contains("+++ b/.env"), "diff:\n{diff}");
            }
            other => panic!("expected FileEdit, got {other:?}"),
        }
        assert!(
            !codex_file_change_single_concrete_path(&multi),
            "multi-file change must NOT advertise allow_always"
        );

        // Single concrete file change IS eligible for allow_always
        let single = build_codex_file_change_detail(&[
            json!({ "path": "src/main.rs", "diff": "@@ -1 +1 @@\n-old\n+new" }),
        ]);
        assert!(codex_file_change_single_concrete_path(&single));

        // Placeholder fallback on cache miss or empty changes is NOT eligible
        let placeholder = ToolCallDetail::FileEdit {
            path: "workspace files".into(),
            unified_diff: None,
        };
        assert!(!codex_file_change_single_concrete_path(&placeholder));
        let empty = build_codex_file_change_detail(&[]);
        assert!(!codex_file_change_single_concrete_path(&empty));
    }

    #[test]
    fn codex_default_effort_clamps_configured_ultra_on_luna_without_fabricating_option() {
        let luna_entries = vec![
            (ReasoningEffort::Low, None),
            (ReasoningEffort::Medium, None),
            (ReasoningEffort::High, None),
            (ReasoningEffort::Xhigh, None),
            (ReasoningEffort::Max, None),
        ];
        let resolved = resolve_codex_model_default_effort(
            &luna_entries,
            Some(ReasoningEffort::Ultra),
            Some(ReasoningEffort::Medium),
        );
        assert_eq!(resolved, Some(ReasoningEffort::Medium));
        let opts = build_codex_thinking_options(&luna_entries, resolved);
        assert!(
            !opts.iter().any(|o| o.effort == ReasoningEffort::Ultra),
            "unsupported Ultra must NOT be fabricated onto luna thinking_options"
        );
        assert!(
            opts.iter()
                .any(|o| o.effort == ReasoningEffort::Medium && o.is_default)
        );
    }

    #[tokio::test]
    async fn codex_resume_preserves_requested_model_over_stale_thread_model() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_codex_resume_model_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_resume_params: Arc<tokio::sync::Mutex<Option<Value>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let observed_turn_model: Arc<tokio::sync::Mutex<Option<String>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let resume_bg = Arc::clone(&observed_resume_params);
        let turn_bg = Arc::clone(&observed_turn_model);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            let mut ack =
                serde_json::to_string(&ShimFrame::AttachAck { latest_seq: 0 }).expect("ack");
            ack.push('\n');
            let _ = wh.write_all(ack.as_bytes()).await;
            let _ = wh.flush().await;

            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(rpc) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let id = rpc.get("id").cloned().unwrap_or(Value::Null);
                let method = rpc.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"id": id, "result": {"userAgent": "codex/test"}}),
                        )
                        .await;
                    }
                    "thread/resume" => {
                        *resume_bg.lock().await = rpc.get("params").cloned();
                        // Stale thread returns old model `gpt-6.1-sol` even when `gpt-6.1-terra` was requested.
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "id": id,
                                "result": {
                                    "thread": { "id": "thr_stale_1" },
                                    "model": "gpt-6.1-sol"
                                }
                            }),
                        )
                        .await;
                    }
                    "turn/start" => {
                        let m = rpc
                            .pointer("/params/model")
                            .and_then(|v| v.as_str())
                            .map(ToOwned::to_owned);
                        *turn_bg.lock().await = m;
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"id": id, "result": {"turn": {"id": "t_res_1"}}}),
                        )
                        .await;
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "method": "turn/completed",
                                "params": { "turn": { "id": "t_res_1", "status": "completed" } }
                            }),
                        )
                        .await;
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let mut cfg = sample_cfg(false);
        cfg.shim_socket = sock_path.clone();
        cfg.resume_session_id = Some("thr_stale_1".into());
        cfg.model = Some("gpt-6.1-terra".into());

        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        let mut init_model: Option<String> = None;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            match ev {
                DriverEvent::SessionInitialized { model, .. } => init_model = model,
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert_eq!(init_model.as_deref(), Some("gpt-6.1-terra"));

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_res_1".into(),
                prompt: "hello".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send prompt");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        let resume_p = observed_resume_params
            .lock()
            .await
            .clone()
            .expect("thread/resume params");
        assert_eq!(resume_p["model"], json!("gpt-6.1-terra"));
        assert_eq!(
            observed_turn_model.lock().await.as_deref(),
            Some("gpt-6.1-terra")
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_steer_during_gates_startup_priority_multi_gate_and_interrupt_cancels_gates() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "o_cdx_gates_{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let started_turns: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let turns_bg = Arc::clone(&started_turns);
        let steer_count = Arc::new(AtomicUsize::new(0));
        let steer_bg = Arc::clone(&steer_count);
        let gate_replies: Arc<tokio::sync::Mutex<Vec<(u64, Value)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let gate_bg = Arc::clone(&gate_replies);
        let (release_startup_tx, mut release_startup_rx) = mpsc::channel::<()>(2);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut next_turn = 0u64;
            let mut held_startup_rpc: Option<u64> = None;
            let mut active_gated_turn: Option<String> = None;
            let mut gates_answered = 0usize;
            let mut holding_int_turn: Option<String> = None;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            loop {
                tokio::select! {
                    Some(()) = release_startup_rx.recv() => {
                        if let Some(rid) = held_startup_rpc.take() {
                            next_turn += 1;
                            let nt = format!("nt_{next_turn}");
                            send_stdout(&mut wh, &mut seq, json!({"id": rid, "result": {"turn": {"id": nt}}})).await;
                            send_stdout(&mut wh, &mut seq, json!({"method": "turn/started", "params": {"turn": {"id": nt}}})).await;
                            send_stdout(&mut wh, &mut seq, json!({"method": "turn/completed", "params": {"turn": {"id": nt, "status": "completed"}}})).await;
                        }
                    }
                    maybe_line = lines.next_line() => {
                        let Ok(Some(raw)) = maybe_line else { break; };
                        let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw) else { continue; };
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let method = val.get("method").and_then(|v| v.as_str());
                        if method.is_none()
                            && let Some(rid) = val.get("id").and_then(|v| v.as_u64())
                        {
                            gate_bg.lock().await.push((rid, val["result"].clone()));
                            if rid == 501 || rid == 502 {
                                gates_answered += 1;
                                if gates_answered == 2
                                    && let Some(nt) = active_gated_turn.take()
                                {
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/completed", "params": {"turn": {"id": nt, "status": "completed"}}})).await;
                                }
                            }
                            continue;
                        }
                        match method.unwrap_or("") {
                            "initialize" => {
                                send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {}})).await;
                            }
                            "thread/start" => {
                                send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {"thread": {"id": "thr_gates"}, "model": "gpt-5.4"}})).await;
                            }
                            "turn/start" => {
                                let text = val.pointer("/params/input/0/text").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                turns_bg.lock().await.push(text.clone());
                                if text == "STARTUP_HOLD" {
                                    held_startup_rpc = val["id"].as_u64();
                                } else if text == "TWO_GATES" {
                                    next_turn += 1;
                                    let nt = format!("nt_{next_turn}");
                                    active_gated_turn = Some(nt.clone());
                                    send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/started", "params": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({
                                        "id": 501,
                                        "method": "item/commandExecution/requestApproval",
                                        "params": { "command": "echo gated" }
                                    })).await;
                                    send_stdout(&mut wh, &mut seq, json!({
                                        "id": 502,
                                        "method": "item/tool/requestUserInput",
                                        "params": {
                                            "questions": [{
                                                "id": "q_env",
                                                "question": "Pick env?",
                                                "options": [{"label": "staging"}]
                                            }]
                                        }
                                    })).await;
                                } else if text == "GATE_THEN_INTERRUPT" {
                                    next_turn += 1;
                                    let nt = format!("nt_{next_turn}");
                                    holding_int_turn = Some(nt.clone());
                                    send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/started", "params": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({
                                        "id": 601,
                                        "method": "item/commandExecution/requestApproval",
                                        "params": { "command": "echo int_cmd" }
                                    })).await;
                                    send_stdout(&mut wh, &mut seq, json!({
                                        "id": 602,
                                        "method": "item/tool/requestUserInput",
                                        "params": {
                                            "questions": [{
                                                "id": "q_int",
                                                "question": "Cancel me?",
                                                "options": [{"label": "yes"}]
                                            }]
                                        }
                                    })).await;
                                } else {
                                    next_turn += 1;
                                    let nt = format!("nt_{next_turn}");
                                    send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/started", "params": {"turn": {"id": nt}}})).await;
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/completed", "params": {"turn": {"id": nt, "status": "completed"}}})).await;
                                }
                            }
                            "turn/steer" => {
                                steer_bg.fetch_add(1, Ordering::SeqCst);
                                send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {}})).await;
                            }
                            "turn/interrupt" => {
                                send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {}})).await;
                                if let Some(nt) = holding_int_turn.take() {
                                    send_stdout(&mut wh, &mut seq, json!({"method": "turn/completed", "params": {"turn": {"id": nt, "status": "interrupted"}}})).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let mut cfg = sample_cfg(false);
        cfg.agent_id = "ag_cdx_gates".into();
        cfg.mode = SessionMode::Default;
        cfg.shim_socket = sock_path.clone();
        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("boot")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::Idle)
            ) {
                break;
            }
        }

        // 1. Startup window: Human Steer must drain AHEAD of earlier Agent Queue!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_s0".into(),
                prompt: "STARTUP_HOLD".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("startup hold");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_ag0".into(),
                prompt: "AGENT_STARTUP_Q".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["m_ag0".into()],
            })
            .await
            .expect("agent q");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_hu0".into(),
                prompt: "HUMAN_STARTUP_STEER".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_hu0".into()],
            })
            .await
            .expect("human steer");
        tokio::time::sleep(Duration::from_millis(40)).await;
        release_startup_tx.send(()).await.expect("release startup");
        let mut saw_ag0_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("startup drain")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_ag0".to_string()] => {
                    saw_ag0_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_ag0_settled => break,
                _ => {}
            }
        }

        // 2. Two gates (approval + question): Steer must queue (ZERO turn/steer), resolving only
        //    approval must keep AwaitingPermission(cq_502), resolving question resumes & drains!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_two".into(),
                prompt: "TWO_GATES".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("two gates");
        let mut saw_perm = false;
        let mut saw_q = false;
        while !(saw_perm && saw_q) {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("two gates arrival")
                .expect("ev");
            match ev {
                DriverEvent::PermissionRequest { request_id, .. } if request_id == "cperm_501" => {
                    saw_perm = true;
                }
                DriverEvent::QuestionRequest { request_id, .. } if request_id == "cq_502" => {
                    saw_q = true;
                }
                _ => {}
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_gated_steer".into(),
                prompt: "HUMAN_STEER_BEHIND_GATES".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_gs".into()],
            })
            .await
            .expect("gated steer");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("gs queued")
                .expect("ev");
            if let DriverEvent::PromptDeliveryUpdated {
                message_ids,
                status,
                ..
            } = ev
                && message_ids == vec!["m_gs".to_string()]
            {
                assert_eq!(status, AgentMessageDeliveryStatus::Queued);
                break;
            }
        }
        assert_eq!(
            steer_count.load(Ordering::SeqCst),
            0,
            "Codex must NOT send turn/steer while approvals/questions are pending"
        );

        cmd_tx
            .send(DriverCommand::ResolvePermission {
                request_id: "cperm_501".into(),
                decision: "allow".into(),
            })
            .await
            .expect("resolve 501");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("post 501 status")
                .expect("ev");
            match ev {
                DriverEvent::StatusChanged(AgentStatus::Running { .. }) => {
                    panic!("Codex must NOT emit Running while cq_502 question is still pending");
                }
                DriverEvent::StatusChanged(AgentStatus::AwaitingPermission {
                    request_id, ..
                }) if request_id == "cq_502" => break,
                _ => {}
            }
        }

        cmd_tx
            .send(DriverCommand::AnswerQuestion {
                request_id: "cq_502".into(),
                answers: vec![vec!["staging".into()]],
            })
            .await
            .expect("answer 502");
        let mut saw_running_after_502 = false;
        let mut saw_gs_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("drain gs")
                .expect("ev");
            match ev {
                DriverEvent::StatusChanged(AgentStatus::Running { turn_id })
                    if turn_id == "t_two" =>
                {
                    saw_running_after_502 = true;
                }
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_gs".to_string()] => {
                    saw_gs_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_gs_settled => break,
                _ => {}
            }
        }
        assert!(saw_running_after_502);
        assert_eq!(steer_count.load(Ordering::SeqCst), 0);

        // 3. Interrupt while approval + question are open MUST decline approval & answer empty to question!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_int".into(),
                prompt: "GATE_THEN_INTERRUPT".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("gate int");
        let mut p601 = false;
        let mut q602 = false;
        while !(p601 && q602) {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("601+602")
                .expect("ev");
            match ev {
                DriverEvent::PermissionRequest { request_id, .. } if request_id == "cperm_601" => {
                    p601 = true;
                }
                DriverEvent::QuestionRequest { request_id, .. } if request_id == "cq_602" => {
                    q602 = true;
                }
                _ => {}
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_int_rep".into(),
                prompt: "AFTER_GATE_INTERRUPT".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_int_rep".into()],
            })
            .await
            .expect("interrupt gates");
        let mut saw_int_rep_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("int rep settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_int_rep".to_string()] => {
                    saw_int_rep_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_int_rep_settled => break,
                _ => {}
            }
        }
        let replies = gate_replies.lock().await;
        assert!(
            replies
                .iter()
                .any(|(id, res)| *id == 601 && res["decision"] == "decline"),
            "Interrupt must decline lingering approval 601: {replies:?}"
        );
        assert!(
            replies
                .iter()
                .any(|(id, res)| *id == 602 && res["answers"] == json!({})),
            "Interrupt must cancel lingering question 602: {replies:?}"
        );
        assert_eq!(
            *started_turns.lock().await,
            vec![
                "STARTUP_HOLD".to_string(),
                "HUMAN_STARTUP_STEER".to_string(),
                "AGENT_STARTUP_Q".to_string(),
                "TWO_GATES".to_string(),
                "HUMAN_STEER_BEHIND_GATES".to_string(),
                "GATE_THEN_INTERRUPT".to_string(),
                "AFTER_GATE_INTERRUPT".to_string(),
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_synthetic_plan_gate_interrupt_and_interrupt_and_drain_queue_never_deadlock() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "o_cdx_plangate_{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let started_turns: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let turns_bg = Arc::clone(&started_turns);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut next_turn = 0u64;

            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, v: Value) {
                let frame = ShimFrame::StdoutLine {
                    seq: Some(*seq),
                    line: v.to_string(),
                };
                *seq += 1;
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                match val.get("method").and_then(|v| v.as_str()).unwrap_or("") {
                    "initialize" => {
                        send_stdout(&mut wh, &mut seq, json!({"id": val["id"], "result": {}}))
                            .await;
                    }
                    "thread/start" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"id": val["id"], "result": {"thread": {"id": "thr_plan"}, "model": "gpt-5.4"}}),
                        )
                        .await;
                    }
                    "turn/start" => {
                        next_turn += 1;
                        let nt = format!("nt_{next_turn}");
                        let text = val
                            .pointer("/params/input/0/text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        turns_bg.lock().await.push(text.clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"id": val["id"], "result": {"turn": {"id": nt}}}),
                        )
                        .await;
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"method": "turn/started", "params": {"turn": {"id": nt}}}),
                        )
                        .await;
                        if text.starts_with("MAKE_PLAN") {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "method": "item/completed",
                                    "params": {
                                        "item": {
                                            "id": format!("plan_{next_turn}"),
                                            "type": "proposedPlan",
                                            "text": "# Plan\n\n- Step 1"
                                        }
                                    }
                                }),
                            )
                            .await;
                        }
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"method": "turn/completed", "params": {"turn": {"id": nt, "status": "completed"}}}),
                        )
                        .await;
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let mut cfg = sample_cfg(false);
        cfg.agent_id = "ag_cdx_plan".into();
        cfg.mode = SessionMode::Plan;
        cfg.shim_socket = sock_path.clone();
        let actor = tokio::spawn(run_codex_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("boot")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::Idle)
            ) {
                break;
            }
        }

        // Scenario 1: Open synthetic plan gate -> Steer queues (Option A), then explicit Interrupt
        // clears the synthetic plan gate and immediately dispatches `turn/start` (NO deadlock!).
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "tp1".into(),
                prompt: "MAKE_PLAN_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("tp1");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("synthetic gate 1")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::AwaitingPermission { .. })
            ) {
                break;
            }
        }

        // Steer queues behind synthetic gate without cancelling it.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_steer_behind".into(),
                prompt: "STEER_QUEUED_BEHIND_PLAN".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_sb".into()],
            })
            .await
            .expect("steer behind plan");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("sb queued")
                .expect("ev");
            if let DriverEvent::PromptDeliveryUpdated {
                message_ids,
                status,
                ..
            } = ev
                && message_ids == vec!["m_sb".to_string()]
            {
                assert_eq!(status, AgentMessageDeliveryStatus::Queued);
                break;
            }
        }

        // Explicit Interrupt clears synthetic gate and immediately dispatches replacement prompt,
        // and once that finishes, STEER_QUEUED_BEHIND_PLAN also drains cleanly!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_flush_now".into(),
                prompt: "FLUSH_CLEARS_PLAN_GATE".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_flush".into()],
            })
            .await
            .expect("flush interrupt");
        let mut saw_flush_dispatched = false;
        let mut saw_sb_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("flush + drain settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Dispatched,
                    ..
                } if message_ids == vec!["m_flush".to_string()] => {
                    saw_flush_dispatched = true;
                }
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_sb".to_string()] => {
                    saw_sb_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_sb_settled => break,
                _ => {}
            }
        }
        assert!(saw_flush_dispatched);

        // Scenario 2: DriverCommand::Interrupt clears synthetic plan gate and emits Idle.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "tp2".into(),
                prompt: "MAKE_PLAN_2".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("tp2");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("synthetic gate 2")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::AwaitingPermission { .. })
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("hard interrupt");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("idle after plan interrupt")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::Idle)
            ) {
                break;
            }
        }

        // Scenario 3: DriverCommand::InterruptAndDrainQueue clears synthetic plan gate and drains queued prompt!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "tp3".into(),
                prompt: "MAKE_PLAN_3".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("tp3");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("synthetic gate 3")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::AwaitingPermission { .. })
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_drain_3".into(),
                prompt: "DRAIN_BEHIND_PLAN_3".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_d3".into()],
            })
            .await
            .expect("queue behind plan 3");
        cmd_tx
            .send(DriverCommand::InterruptAndDrainQueue)
            .await
            .expect("interrupt and drain plan 3");
        let mut saw_d3_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("d3 settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_d3".to_string()] => {
                    saw_d3_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_d3_settled => break,
                _ => {}
            }
        }
        assert_eq!(
            *started_turns.lock().await,
            vec![
                "MAKE_PLAN_1".to_string(),
                "FLUSH_CLEARS_PLAN_GATE".to_string(),
                "STEER_QUEUED_BEHIND_PLAN".to_string(),
                "MAKE_PLAN_2".to_string(),
                "MAKE_PLAN_3".to_string(),
                "DRAIN_BEHIND_PLAN_3".to_string(),
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }
}
