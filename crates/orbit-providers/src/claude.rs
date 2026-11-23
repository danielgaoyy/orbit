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

use crate::attachments::build_claude_content_blocks_async;
use crate::driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};

enum ClaudeSessionStartMode<'a> {
    Fresh(&'a str),
    Resume(&'a str),
    ForkFrom(&'a str),
}

fn apply_claude_settings_json(args: &mut Vec<String>, key: &str, value: Value) {
    let mut idx = 0;
    while idx < args.len() {
        if args[idx] == "--settings"
            && let Some(raw) = args.get(idx + 1)
        {
            if let Some(mut obj) = serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|v| v.as_object().cloned())
            {
                obj.insert(key.to_string(), value);
                args[idx + 1] = Value::Object(obj).to_string();
                return;
            }
            // Do not clobber a file path argument such as `--settings /path/to/settings.json`.
            idx += 2;
            continue;
        }
        if let Some(raw) = args[idx].strip_prefix("--settings=")
            && let Some(mut obj) = serde_json::from_str::<Value>(raw)
                .ok()
                .and_then(|v| v.as_object().cloned())
        {
            obj.insert(key.to_string(), value);
            args[idx] = format!("--settings={}", Value::Object(obj));
            return;
        }
        idx += 1;
    }
    let mut obj = serde_json::Map::new();
    obj.insert(key.to_string(), value);
    args.push("--settings".to_string());
    args.push(Value::Object(obj).to_string());
}

fn extract_claude_control_error(val: &Value) -> Option<String> {
    let candidates = [
        val.pointer("/response/error"),
        val.pointer("/response/error/message"),
        val.get("error"),
        val.pointer("/error/message"),
    ];
    for cand in candidates.into_iter().flatten() {
        if let Some(s) = cand.as_str() {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn is_claude_unsupported_control_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("unknown control")
        || lower.contains("unknown subtype")
        || lower.contains("unrecognized subtype")
        || lower.contains("unrecognized control")
        || lower.contains("unsupported control")
        || lower.contains("unsupported subtype")
        || lower.contains("invalid subtype")
        || lower.contains("method not found")
        || (lower.contains("apply_flag_settings")
            && (lower.contains("unknown")
                || lower.contains("unsupported")
                || lower.contains("unrecognized")
                || lower.contains("not found")
                || lower.contains("invalid")))
}

pub fn claude_model_supports_fast_mode(model_id: &str) -> bool {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(row) = CLAUDE_MANIFEST
        .iter()
        .find(|r| r.id.eq_ignore_ascii_case(&lower))
    {
        return row.supports_fast_mode;
    }
    if lower == "opus" {
        return true;
    }
    if lower.starts_with("claude-opus-") {
        return lower.starts_with("claude-opus-4-6")
            || lower.starts_with("claude-opus-4.6")
            || lower.starts_with("claude-opus-4-8")
            || lower.starts_with("claude-opus-4.8")
            || lower.starts_with("claude-opus-5")
            || lower.starts_with("claude-opus-6");
    }
    false
}

fn build_claude_launch_args(
    cfg: &SessionLaunchConfig,
    start_mode: ClaudeSessionStartMode<'_>,
) -> Vec<String> {
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

    match start_mode {
        ClaudeSessionStartMode::ForkFrom(source_sid) => {
            args.push(format!("--resume={source_sid}"));
            args.push("--fork-session".to_string());
        }
        ClaudeSessionStartMode::Resume(resume_id) => {
            args.push(format!("--resume={resume_id}"));
        }
        ClaudeSessionStartMode::Fresh(session_id) => {
            args.push(format!("--session-id={session_id}"));
        }
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
    if cfg.fast_mode {
        apply_claude_settings_json(&mut args, "fastMode", Value::Bool(true));
    }
    args
}

fn build_claude_fast_mode_control_request(request_id: &str, enabled: bool) -> Value {
    json!({
        "request_id": request_id,
        "type": "control_request",
        "request": {
            "subtype": "apply_flag_settings",
            "settings": {
                "fastMode": enabled
            }
        }
    })
}

async fn spawn_claude_shim_and_init(
    cfg: &SessionLaunchConfig,
    start_mode: ClaudeSessionStartMode<'_>,
) -> Result<orbit_shim::ShimClient, String> {
    let args = build_claude_launch_args(cfg, start_mode);

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
    crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);

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

    if let Err(e) = fresh.write_stdin_line(init_req).await {
        let _ = fresh.kill().await;
        return Err(format!("failed to write claude initialize: {e}"));
    }

    if let Err(e) = wait_claude_init_response(&mut fresh, &cfg.shim_seq_tracker).await {
        let _ = fresh.kill().await;
        return Err(e);
    }
    Ok(fresh)
}

pub async fn spawn_claude_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    let mut preassigned_session_id = cfg
        .resume_session_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let mut forked_natively = false;
    let mut inject_handoff_on_next_prompt = false;
    let mut fork_fallback_notice: Option<String> = None;

    let (shim, is_reattached) = if let Some(existing) = reattached {
        (existing, true)
    } else if let Some(ref fork_sid) = cfg.fork_from_session_id {
        match spawn_claude_shim_and_init(&cfg, ClaudeSessionStartMode::ForkFrom(fork_sid)).await {
            Ok(fresh) => {
                forked_natively = true;
                // Do not leak a fabricated random session_id before Claude's `system/init`
                // confirms the newly allocated forked session ID.
                preassigned_session_id.clear();
                (fresh, false)
            }
            Err(first_err) if cfg.native_fork_only => {
                return Err(format!(
                    "Claude native fork failed ({first_err}); deferring fresh session until first prompt"
                ));
            }
            Err(first_err) => {
                let short_sid: String = fork_sid.chars().take(8).collect();
                let fresh = spawn_claude_shim_and_init(
                    &cfg,
                    ClaudeSessionStartMode::Fresh(&preassigned_session_id),
                )
                .await
                .map_err(|e| {
                    format!("Claude native fork failed ({first_err}); fresh session fallback also failed: {e}")
                })?;
                inject_handoff_on_next_prompt = true;
                fork_fallback_notice = Some(format!(
                    "Native Claude session fork ({short_sid}…) fell back to a fresh session with conversation context restored."
                ));
                (fresh, false)
            }
        }
    } else if let Some(ref resume_id) = cfg.resume_session_id {
        match spawn_claude_shim_and_init(&cfg, ClaudeSessionStartMode::Resume(resume_id)).await {
            Ok(fresh) => (fresh, false),
            Err(first_err) if cfg.fallback_handoff_context.is_some() => {
                let short_sid: String = resume_id.chars().take(8).collect();
                let fresh_id = Uuid::new_v4().to_string();
                let fresh = spawn_claude_shim_and_init(
                    &cfg,
                    ClaudeSessionStartMode::Fresh(&fresh_id),
                )
                .await
                .map_err(|e| {
                    format!("Claude resume failed ({first_err}); fresh session fallback also failed: {e}")
                })?;
                preassigned_session_id = fresh_id;
                inject_handoff_on_next_prompt = true;
                fork_fallback_notice = Some(format!(
                    "Previous Claude session ({short_sid}…) could not be resumed; started a fresh session with recent conversation context restored."
                ));
                (fresh, false)
            }
            Err(e) => return Err(e),
        }
    } else {
        let fresh = spawn_claude_shim_and_init(
            &cfg,
            ClaudeSessionStartMode::Fresh(&preassigned_session_id),
        )
        .await?;
        if cfg.fallback_handoff_context.is_some() {
            inject_handoff_on_next_prompt = true;
        }
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_claude_actor(
        cfg,
        preassigned_session_id,
        shim,
        is_reattached,
        forked_natively,
        inject_handoff_on_next_prompt,
        fork_fallback_notice,
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

fn hydrate_reattached_claude_bg_tasks(
    persisted: &[crate::PersistedBackgroundTask],
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    bg_bash_tasks: &mut HashMap<String, (String, ToolCallDetail, BackgroundTaskInfo)>,
    bg_tool_use_to_task: &mut HashMap<String, String>,
    bg_tool_uses: &mut HashMap<String, BackgroundTaskInfo>,
) {
    for item in persisted {
        let task_id = item
            .info
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| item.call_id.clone());
        tool_cache.insert(item.call_id.clone(), item.detail.clone());
        bg_tool_use_to_task.insert(item.call_id.clone(), task_id.clone());
        bg_tool_uses.insert(item.call_id.clone(), item.info.clone());
        bg_bash_tasks.insert(
            task_id,
            (item.call_id.clone(), item.detail.clone(), item.info.clone()),
        );
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_claude_task_notification(
    parent_agent_id: &str,
    is_reattached: bool,
    val: &Value,
    bg_bash_tasks: &mut HashMap<String, (String, ToolCallDetail, BackgroundTaskInfo)>,
    bg_tool_use_to_task: &mut HashMap<String, String>,
    bg_tool_uses: &mut HashMap<String, BackgroundTaskInfo>,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    declared_tasks: &mut HashMap<String, ProviderSubagentDescriptor>,
    tool_use_to_task: &HashMap<String, String>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
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
        && let Some((call_id, mut detail, mut bg_info)) = bg_bash_tasks.remove(&tid)
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
        return;
    }
    let lookup_id = raw_task_id.or_else(|| {
        raw_tool_use_id
            .as_deref()
            .and_then(|id| tool_use_to_task.get(id).cloned())
    });
    if let Some(tid) = lookup_id {
        let st = val
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("completed");
        let terminal_status = match st {
            "failed" | "error" => ProviderSubagentStatus::Failed,
            "stopped" | "canceled" | "cancelled" => ProviderSubagentStatus::Canceled,
            _ => ProviderSubagentStatus::Completed,
        };
        let now = Utc::now();
        let desc = if let Some(existing) = declared_tasks.get_mut(&tid) {
            existing.status = terminal_status;
            existing.updated_at = now;
            existing.clone()
        } else {
            let task_type = val.get("task_type").and_then(|v| v.as_str());
            let skip_transcript = val
                .get("skip_transcript")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            // Only synthesize an undeclared terminal subagent after a daemon hot-restart
            // (`is_reattached == true`), and never for background bash or skipped transcripts.
            if !is_reattached || task_type == Some("local_bash") || skip_transcript {
                return;
            }
            let title = val
                .get("description")
                .or_else(|| val.get("title"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(tid.as_str())
                .to_string();
            let synthesized = ProviderSubagentDescriptor {
                subagent_id: tid.clone(),
                parent_agent_id: parent_agent_id.to_string(),
                parent_subagent_id: None,
                title,
                subtitle: val
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned),
                status: terminal_status,
                created_at: now,
                updated_at: now,
            };
            declared_tasks.insert(tid.clone(), synthesized.clone());
            synthesized
        };
        let _ = evt_tx.send(DriverEvent::ProviderSubagentUpsert(desc)).await;
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

fn build_claude_user_frame(
    session_id: &str,
    content_blocks: Vec<Value>,
    priority_next: bool,
) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("type".into(), Value::String("user".into()));
    if !session_id.is_empty() {
        obj.insert("session_id".into(), Value::String(session_id.to_string()));
    }
    obj.insert("parent_tool_use_id".into(), Value::Null);
    obj.insert("uuid".into(), Value::String(Uuid::new_v4().to_string()));
    if priority_next {
        obj.insert("priority".into(), Value::String("next".into()));
    }
    obj.insert(
        "message".into(),
        json!({
            "role": "user",
            "content": content_blocks
        }),
    );
    Value::Object(obj)
}

#[allow(clippy::too_many_arguments)]
async fn run_claude_actor(
    cfg: SessionLaunchConfig,
    mut session_id: String,
    mut shim: orbit_shim::ShimClient,
    is_reattached: bool,
    mut forked_natively: bool,
    mut inject_handoff_on_next_prompt: bool,
    fork_fallback_notice: Option<String>,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut emitted_session_init = false;
    if !forked_natively {
        let _ = evt_tx
            .send(DriverEvent::SessionInitialized {
                persistence_ref: session_id.clone(),
                model: cfg.model.clone(),
            })
            .await;
        emitted_session_init = true;
    }
    if let Some(notice) = fork_fallback_notice {
        let _ = evt_tx
            .send(DriverEvent::Notice {
                level: "info".into(),
                message: notice,
            })
            .await;
    }
    if !is_reattached {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();
    let mut pending_fork_first_prompt: Option<(String, String)> = None;

    let mut ctrl_seq: u64 = if is_reattached { 1000 } else { 2 };
    let mut active_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut active_msg_id: String = "claude_msg_0".into();
    let mut streamed_blocks: HashMap<u64, (String, String)> = HashMap::new(); // index -> (kind, accum)
    let mut emitted_text_in_turn = false;
    let mut last_call_prompt_tokens: Option<u64> = None;
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingClaudePerm> = HashMap::new();
    let mut queued_prompts: VecDeque<(String, String)> = VecDeque::new();
    let mut declared_tasks: HashMap<String, ProviderSubagentDescriptor> = HashMap::new();
    let mut tool_use_to_task: HashMap<String, String> = HashMap::new();
    let mut bg_bash_tasks: HashMap<String, (String, ToolCallDetail, BackgroundTaskInfo)> =
        HashMap::new();
    let mut bg_tool_use_to_task: HashMap<String, String> = HashMap::new();
    let mut bg_tool_uses: HashMap<String, BackgroundTaskInfo> = HashMap::new();
    if is_reattached {
        hydrate_reattached_claude_bg_tasks(
            &cfg.persisted_running_bg_tasks,
            &mut tool_cache,
            &mut bg_bash_tasks,
            &mut bg_tool_use_to_task,
            &mut bg_tool_uses,
        );
    }
    let mut confirmed_fast_mode: bool = cfg.fast_mode;
    let mut active_fast_mode: bool = cfg.fast_mode;
    let mut pending_fast_mode_reqs: HashMap<String, bool> = HashMap::new();

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt { turn_id, mut prompt, behavior } => {
                        if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            if let Some(ctx) = pending_handoff_context.take()
                                && !ctx.trim().is_empty()
                                && !prompt.contains("[Prior Conversation Context in this Workspace]")
                            {
                                prompt = format!("{ctx}\n\n{prompt}");
                            }
                        }
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
                                    let content_blocks = build_claude_content_blocks_async(
                                        prompt.clone(),
                                        cfg.attachments_dir.clone(),
                                    )
                                    .await;
                                    let user_frame = build_claude_user_frame(
                                        &session_id,
                                        content_blocks,
                                        true,
                                    );
                                    if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }

                        if forked_natively && !emitted_session_init {
                            pending_fork_first_prompt = Some((turn_id.clone(), prompt.clone()));
                        }

                        active_turn_id = Some(turn_id.clone());
                        streamed_blocks.clear();
                        emitted_text_in_turn = false;
                        last_call_prompt_tokens = None;
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                turn_id: turn_id.clone(),
                            }))
                            .await;

                        let content_blocks = build_claude_content_blocks_async(
                            prompt,
                            cfg.attachments_dir.clone(),
                        )
                        .await;
                        let user_frame =
                            build_claude_user_frame(&session_id, content_blocks, false);
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
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode,
                    } => {
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
                        if let Some(enabled) = fast_mode
                            && enabled != active_fast_mode
                        {
                            active_fast_mode = enabled;
                            let rid = format!("ctrl_{ctrl_seq}");
                            ctrl_seq += 1;
                            let req = build_claude_fast_mode_control_request(&rid, enabled);
                            pending_fast_mode_reqs.insert(rid, enabled);
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
                                        {
                                            if forked_natively
                                                && cfg.fork_from_session_id.as_deref() == Some(sid)
                                            {
                                                tracing::warn!(
                                                    agent_id = %cfg.agent_id,
                                                    parent_session_id = %sid,
                                                    "Claude --fork-session returned parent session_id; killing shim to protect parent session"
                                                );
                                                let _ = shim.kill().await;
                                                if cfg.native_fork_only {
                                                    break;
                                                }
                                                let fresh_id = Uuid::new_v4().to_string();
                                                match spawn_claude_shim_and_init(
                                                    &cfg,
                                                    ClaudeSessionStartMode::Fresh(&fresh_id),
                                                )
                                                .await
                                                {
                                                    Ok(fresh_shim) => {
                                                        shim = fresh_shim;
                                                        forked_natively = false;
                                                        session_id = fresh_id;
                                                        emitted_session_init = true;
                                                        inject_handoff_on_next_prompt = true;
                                                        let _ = evt_tx
                                                            .send(DriverEvent::SessionInitialized {
                                                                persistence_ref: session_id.clone(),
                                                                model: cfg.model.clone(),
                                                            })
                                                            .await;
                                                        let _ = evt_tx
                                                            .send(DriverEvent::Notice {
                                                                level: "info".into(),
                                                                message: "Native Claude session fork returned parent session ID; started a fresh session with conversation context restored.".into(),
                                                            })
                                                            .await;
                                                        if let Some((_tid, orig_prompt)) =
                                                            pending_fork_first_prompt.take()
                                                        {
                                                            inject_handoff_on_next_prompt = false;
                                                            let wire_prompt = if let Some(ctx) =
                                                                pending_handoff_context.take()
                                                                && !ctx.trim().is_empty()
                                                                && !orig_prompt.contains(
                                                                    "[Prior Conversation Context in this Workspace]",
                                                                )
                                                            {
                                                                format!("{ctx}\n\n{orig_prompt}")
                                                            } else {
                                                                orig_prompt
                                                            };
                                                            let blocks = build_claude_content_blocks_async(
                                                                wire_prompt,
                                                                cfg.attachments_dir.clone(),
                                                            )
                                                            .await;
                                                            let frame = build_claude_user_frame(
                                                                &session_id,
                                                                blocks,
                                                                false,
                                                            );
                                                            if shim
                                                                .write_stdin_line(frame.to_string())
                                                                .await
                                                                .is_err()
                                                            {
                                                                break;
                                                            }
                                                        }
                                                    }
                                                    Err(e) => {
                                                        let _ = evt_tx
                                                            .send(DriverEvent::StatusChanged(
                                                                AgentStatus::Error {
                                                                    message: format!(
                                                                        "Claude native fork returned parent session ID; fresh session fallback failed: {e}"
                                                                    ),
                                                                    recoverable: true,
                                                                },
                                                            ))
                                                            .await;
                                                        break;
                                                    }
                                                }
                                            } else if !emitted_session_init || sid != session_id {
                                                session_id = sid.to_string();
                                                emitted_session_init = true;
                                                pending_fork_first_prompt = None;
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
                                        handle_claude_task_notification(
                                            &cfg.agent_id,
                                            is_reattached,
                                            &val,
                                            &mut bg_bash_tasks,
                                            &mut bg_tool_use_to_task,
                                            &mut bg_tool_uses,
                                            &mut tool_cache,
                                            &mut declared_tasks,
                                            &tool_use_to_task,
                                            &evt_tx,
                                        )
                                        .await;
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
                                if let Some(call_prompt) = extract_claude_message_prompt_tokens(&val) {
                                    last_call_prompt_tokens = Some(call_prompt);
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
                            "control_response" => {
                                if let Some(resp_rid) = val
                                    .pointer("/response/request_id")
                                    .and_then(|v| v.as_str())
                                    && let Some(enabled) = pending_fast_mode_reqs.remove(resp_rid)
                                {
                                    let was_err = val
                                        .pointer("/response/subtype")
                                        .and_then(|v| v.as_str())
                                        == Some("error");
                                    if !was_err {
                                        confirmed_fast_mode = enabled;
                                    } else {
                                        if pending_fast_mode_reqs.is_empty() {
                                            active_fast_mode = confirmed_fast_mode;
                                        }
                                        let state_label = if enabled { "on" } else { "off" };
                                        let raw_err = extract_claude_control_error(&val);
                                        let (level, message) = match raw_err {
                                            Some(err_msg)
                                                if !is_claude_unsupported_control_error(&err_msg) =>
                                            {
                                                (
                                                    "error",
                                                    format!(
                                                        "Fast mode ({state_label}) rejected by Claude: {err_msg}"
                                                    ),
                                                )
                                            }
                                            _ => (
                                                "info",
                                                format!(
                                                    "Fast mode ({state_label}) saved; takes effect on next Claude session restart."
                                                ),
                                            ),
                                        };
                                        let _ = evt_tx
                                            .send(DriverEvent::Notice {
                                                level: level.into(),
                                                message,
                                            })
                                            .await;
                                    }
                                }
                            }
                            "result" => {
                                active_turn_id = None;
                                let single_call_prompt = last_call_prompt_tokens.take();
                                if let Some(usage) = val.get("usage") {
                                    let input_tokens = usage
                                        .get("input_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    let output_tokens = usage
                                        .get("output_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    let cache_read_tokens = usage
                                        .get("cache_read_input_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    let cache_write_tokens = usage
                                        .get("cache_creation_input_tokens")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    if input_tokens > 0
                                        || output_tokens > 0
                                        || cache_read_tokens > 0
                                        || cache_write_tokens > 0
                                    {
                                        let reported_model = select_dominant_claude_model(&val);
                                        let _ = evt_tx
                                            .send(DriverEvent::TurnUsage {
                                                input_tokens,
                                                output_tokens,
                                                cache_read_tokens,
                                                cache_write_tokens,
                                                last_call_prompt_tokens: single_call_prompt,
                                                model: reported_model,
                                            })
                                            .await;
                                    }
                                }
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
                                    last_call_prompt_tokens = None;
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: next_tid,
                                        }))
                                        .await;
                                    let content_blocks = build_claude_content_blocks_async(
                                        next_prompt,
                                        cfg.attachments_dir.clone(),
                                    )
                                    .await;
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

/// Extract instantaneous single-call prompt token count from a top-level Claude `"assistant"` event.
fn extract_claude_message_prompt_tokens(val: &Value) -> Option<u64> {
    let usage = val.pointer("/message/usage")?;
    let input = usage
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_read = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache_write = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total = input.saturating_add(cache_read).saturating_add(cache_write);
    (total > 0).then_some(total)
}

/// Select the dominant model from Claude Code's `result.modelUsage` map by highest total token volume
/// (never alphabetical key order, so an Opus turn with a small Haiku helper is priced as Opus).
fn select_dominant_claude_model(val: &Value) -> Option<String> {
    if let Some(map) = val.get("modelUsage").and_then(|v| v.as_object()) {
        let best = map
            .iter()
            .filter(|(k, _)| !k.trim().is_empty())
            .max_by(|(k_a, v_a), (k_b, v_b)| {
                let total = |v: &Value| -> u64 {
                    let inp = v
                        .get("inputTokens")
                        .or_else(|| v.get("input_tokens"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    let out = v
                        .get("outputTokens")
                        .or_else(|| v.get("output_tokens"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    let cr = v
                        .get("cacheReadInputTokens")
                        .or_else(|| v.get("cache_read_input_tokens"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    let cw = v
                        .get("cacheCreationInputTokens")
                        .or_else(|| v.get("cache_creation_input_tokens"))
                        .and_then(|x| x.as_u64())
                        .unwrap_or(0);
                    inp.saturating_add(out)
                        .saturating_add(cr)
                        .saturating_add(cw)
                };
                total(v_a).cmp(&total(v_b)).then_with(|| k_a.cmp(k_b))
            })
            .map(|(k, _)| k.clone());
        if best.is_some() {
            return best;
        }
    }
    val.get("model")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
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
                            if val.pointer("/response/subtype").and_then(|v| v.as_str())
                                == Some("error")
                            {
                                let err_msg = val
                                    .pointer("/response/error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Claude Code initialize rejected");
                                return Err(err_msg.to_string());
                            }
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
        // Auto (Guardian) maps to Claude's approval-gating mode so Orbit's daemon receives
        // `can_use_tool` control requests and evaluates them through Guardian.
        SessionMode::Auto => "default",
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
                .filter(|h| !h.is_empty())
                .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
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
    supports_fast_mode: bool,
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
        supports_fast_mode: true,
    },
    ClaudeManifestRow {
        id: "claude-fable-5-2",
        label: "Fable 5.2",
        description: "Fable 5.2 · Most powerful model",
        min_version: Some((2, 1, 321)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-5-5",
        label: "Sonnet 5.5",
        description: "Sonnet 5.5 · Best for everyday tasks",
        min_version: Some((2, 1, 284)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-haiku-5-5",
        label: "Haiku 5.5",
        description: "Haiku 5.5 · Fastest for quick answers",
        min_version: Some((2, 1, 293)),
        default_priority: 0,
        default_effort: Some(ReasoningEffort::Medium),
        efforts: EFFORTS_XHIGH_WITH_OFF,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-opus-5",
        label: "Opus 5",
        description: "Opus 5 · Previous release",
        min_version: Some((2, 1, 219)),
        default_priority: 2,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
        supports_fast_mode: true,
    },
    ClaudeManifestRow {
        id: "claude-fable-5-1",
        label: "Fable 5.1",
        description: "Fable 5.1 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-5",
        label: "Sonnet 5",
        description: "Sonnet 5 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-opus-4-8",
        label: "Opus 4.8",
        description: "Opus 4.8 · Previous release",
        min_version: None,
        default_priority: 1,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_XHIGH_WITH_OFF,
        supports_fast_mode: true,
    },
    ClaudeManifestRow {
        id: "claude-opus-4-6",
        label: "Opus 4.6",
        description: "Opus 4.6 · Most capable for complex work",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_STANDARD_WITH_OFF,
        supports_fast_mode: true,
    },
    ClaudeManifestRow {
        id: "claude-sonnet-4-6",
        label: "Sonnet 4.6",
        description: "Sonnet 4.6 · Best for everyday tasks",
        min_version: None,
        default_priority: 0,
        default_effort: Some(ReasoningEffort::High),
        efforts: EFFORTS_STANDARD_WITH_OFF,
        supports_fast_mode: false,
    },
    ClaudeManifestRow {
        id: "claude-haiku-4-5",
        label: "Haiku 4.5",
        description: "Haiku 4.5 · Previous release",
        min_version: None,
        default_priority: 0,
        default_effort: None,
        efforts: &[],
        supports_fast_mode: false,
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
                supports_fast_mode: row.supports_fast_mode,
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
        let mut cmd = tokio::process::Command::new(exe);
        cmd.arg("--version")
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::shell_snapshot::apply_cached_shell_env(&mut cmd);
        let child = cmd.spawn().ok()?;
        let out = child.wait_with_output().await.ok()?;
        parse_semver_prefix(&String::from_utf8_lossy(&out.stdout))
    })
    .await
    .ok()
    .flatten();

    build_claude_catalog_for_version(ver)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    fn test_claude_cfg(fork_sid: Option<&str>, native_fork_only: bool) -> SessionLaunchConfig {
        SessionLaunchConfig {
            agent_id: "ag_claude_fork_test".into(),
            provider: ProviderKind::ClaudeCode,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: std::env::temp_dir().join("orbit_claude_unused.sock"),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("sonnet".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            resume_session_id: None,
            fork_from_session_id: fork_sid.map(ToOwned::to_owned),
            native_fork_only,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: Some(
                "[Prior Conversation Context in this Workspace]\nUser: hi".into(),
            ),
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        }
    }

    fn sample_cfg(fast_mode: bool) -> SessionLaunchConfig {
        SessionLaunchConfig {
            agent_id: "ag_claude_test".into(),
            provider: ProviderKind::ClaudeCode,
            executable: PathBuf::from("/usr/bin/claude"),
            orbit_exe: PathBuf::from("/usr/bin/orbit"),
            shim_socket: PathBuf::from("/tmp/claude.sock"),
            cwd: PathBuf::from("/repo"),
            extra_dirs: Vec::new(),
            attachments_dir: None,
            model: Some("claude-opus-4-6".into()),
            effort: Some(ReasoningEffort::High),
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
            acp_agent_id: None,
            acp_spec: None,
        }
    }

    #[test]
    fn build_claude_launch_args_uses_equals_resume_and_fork_session() {
        let cfg = test_claude_cfg(Some("parent-sess-123"), false);
        let args =
            build_claude_launch_args(&cfg, ClaudeSessionStartMode::ForkFrom("parent-sess-123"));
        assert!(args.iter().any(|a| a == "--resume=parent-sess-123"));
        assert!(args.iter().any(|a| a == "--fork-session"));
        assert!(!args.iter().any(|a| a.starts_with("--session-id")));
    }

    #[test]
    fn build_claude_user_frame_omits_empty_session_id() {
        let frame_empty = build_claude_user_frame("", Vec::new(), false);
        assert!(frame_empty.get("session_id").is_none());

        let frame_set = build_claude_user_frame("forked-sess-456", Vec::new(), true);
        assert_eq!(
            frame_set.get("session_id").and_then(|v| v.as_str()),
            Some("forked-sess-456")
        );
        assert_eq!(
            frame_set.get("priority").and_then(|v| v.as_str()),
            Some("next")
        );
    }

    #[tokio::test]
    async fn claude_fork_aborts_when_init_returns_parent_session_id() {
        use orbit_shim::ShimListener;
        use tokio::io::AsyncWriteExt;

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_claude_fork_guard_{}.sock",
            Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock claude shim");

        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("accept");
            let frames = [
                ShimFrame::AttachAck { latest_seq: 0 },
                ShimFrame::StdoutLine {
                    seq: Some(1),
                    line: json!({
                        "type": "system",
                        "subtype": "init",
                        "session_id": "parent-sess-999",
                        "model": "sonnet"
                    })
                    .to_string(),
                },
            ];
            for f in frames {
                let mut enc = serde_json::to_string(&f).expect("encode");
                enc.push('\n');
                let _ = stream.write_all(enc.as_bytes()).await;
            }
            let _ = stream.flush().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (_cmd_tx, cmd_rx) = mpsc::channel(8);
        let (evt_tx, mut evt_rx) = mpsc::channel(16);
        let mut cfg = test_claude_cfg(Some("parent-sess-999"), true);
        cfg.shim_socket = sock_path.clone();

        let actor = tokio::spawn(run_claude_actor(
            cfg,
            String::new(),
            shim,
            false,
            true,
            false,
            None,
            cmd_rx,
            evt_tx,
        ));

        let mut saw_session_init = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await {
            if matches!(ev, DriverEvent::SessionInitialized { .. }) {
                saw_session_init = true;
            }
        }
        assert!(
            !saw_session_init,
            "Claude fork must NEVER emit SessionInitialized when init returns parent session_id"
        );
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn launch_args_include_settings_only_when_fast_mode_enabled() {
        let off_args =
            build_claude_launch_args(&sample_cfg(false), ClaudeSessionStartMode::Fresh("sess-1"));
        assert!(!off_args.iter().any(|a| a.starts_with("--settings")));

        let on_args =
            build_claude_launch_args(&sample_cfg(true), ClaudeSessionStartMode::Fresh("sess-1"));
        let idx = on_args
            .iter()
            .position(|a| a == "--settings")
            .expect("--settings flag");
        let parsed: Value = serde_json::from_str(&on_args[idx + 1]).expect("valid settings json");
        assert_eq!(parsed, json!({ "fastMode": true }));
    }

    #[test]
    fn apply_claude_settings_merges_existing_json_cleanly() {
        let mut args = vec![
            "--verbose".to_string(),
            "--settings".to_string(),
            r#"{"theme":"dark"}"#.to_string(),
        ];
        apply_claude_settings_json(&mut args, "fastMode", Value::Bool(true));
        let idx = args
            .iter()
            .position(|a| a == "--settings")
            .expect("settings flag");
        let merged: Value = serde_json::from_str(&args[idx + 1]).expect("merged json");
        assert_eq!(merged["theme"], json!("dark"));
        assert_eq!(merged["fastMode"], json!(true));
        assert_eq!(args.iter().filter(|a| *a == "--settings").count(), 1);

        let mut eq_args = vec![r#"--settings={"a":1}"#.to_string()];
        apply_claude_settings_json(&mut eq_args, "fastMode", Value::Bool(true));
        let raw = eq_args[0]
            .strip_prefix("--settings=")
            .expect("inline settings");
        let merged_eq: Value = serde_json::from_str(raw).expect("merged eq");
        assert_eq!(merged_eq["a"], json!(1));
        assert_eq!(merged_eq["fastMode"], json!(true));
    }

    #[test]
    fn fast_mode_control_request_and_catalog_flags() {
        let req = build_claude_fast_mode_control_request("ctrl_7", true);
        assert_eq!(req["type"], json!("control_request"));
        assert_eq!(req["request_id"], json!("ctrl_7"));
        assert_eq!(req["request"]["subtype"], json!("apply_flag_settings"));
        assert_eq!(req["request"]["settings"]["fastMode"], json!(true));

        let cat = fallback_claude_models();
        let opus = cat
            .models
            .iter()
            .find(|m| m.id == "claude-opus-4-6")
            .expect("opus 4.6");
        let haiku = cat
            .models
            .iter()
            .find(|m| m.id == "claude-haiku-4-5")
            .expect("haiku 4.5");
        assert!(opus.supports_fast_mode);
        assert!(!haiku.supports_fast_mode);
        assert!(claude_model_supports_fast_mode("opus"));
        assert!(claude_model_supports_fast_mode("claude-opus-4-6"));
        assert!(!claude_model_supports_fast_mode("claude-sonnet-4-6"));
        assert!(!claude_model_supports_fast_mode("sonnet"));
    }

    #[test]
    fn apply_claude_settings_preserves_file_path_argument() {
        let mut args = vec![
            "--verbose".to_string(),
            "--settings".to_string(),
            "/path/to/settings.json".to_string(),
        ];
        apply_claude_settings_json(&mut args, "fastMode", Value::Bool(true));
        assert_eq!(args[1], "--settings");
        assert_eq!(args[2], "/path/to/settings.json");
        assert_eq!(args[3], "--settings");
        let parsed: Value = serde_json::from_str(&args[4]).expect("appended settings json");
        assert_eq!(parsed, json!({ "fastMode": true }));

        let mut eq_file_args = vec!["--settings=/etc/claude/settings.json".to_string()];
        apply_claude_settings_json(&mut eq_file_args, "fastMode", Value::Bool(true));
        assert_eq!(eq_file_args[0], "--settings=/etc/claude/settings.json");
        assert_eq!(eq_file_args[1], "--settings");
        let parsed2: Value = serde_json::from_str(&eq_file_args[2]).expect("appended json");
        assert_eq!(parsed2, json!({ "fastMode": true }));
    }

    #[tokio::test]
    async fn claude_live_fast_mode_tracks_multiple_requests_and_surfaces_policy_errors() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_claude_fast_ctrl_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_reqs: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed_bg = Arc::clone(&observed_reqs);

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

            let mut count = 0usize;
            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if val.get("type").and_then(|v| v.as_str()) == Some("control_request")
                    && val.pointer("/request/subtype").and_then(|v| v.as_str())
                        == Some("apply_flag_settings")
                {
                    let rid = val
                        .get("request_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    observed_bg.lock().await.push(rid.clone());
                    count += 1;
                    match count {
                        1 => {
                            // Real policy error -> must emit level="error" and revert active_fast_mode
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "type": "control_response",
                                    "response": {
                                        "subtype": "error",
                                        "request_id": rid,
                                        "error": "Fast mode is disabled by organization policy"
                                    }
                                }),
                            )
                            .await;
                        }
                        2 => {
                            // Retry after revert -> unsupported subtype -> info notice
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "type": "control_response",
                                    "response": {
                                        "subtype": "error",
                                        "request_id": rid,
                                        "error": "Unknown control request subtype: apply_flag_settings"
                                    }
                                }),
                            )
                            .await;
                            break;
                        }
                        _ => {}
                    }
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

        let actor = tokio::spawn(run_claude_actor(
            cfg,
            "sess_ctrl_1".into(),
            shim,
            false,
            false,
            false,
            None,
            cmd_rx,
            evt_tx,
        ));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // First toggle ON -> rejected by org policy
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(true),
            })
            .await
            .expect("toggle 1");
        let notice1 = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
            .await
            .expect("notice 1 timeout")
            .expect("ev");
        match notice1 {
            DriverEvent::Notice { level, message } => {
                assert_eq!(level, "error");
                assert!(
                    message.contains("organization policy"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected error Notice, got {other:?}"),
        }

        // Because active_fast_mode was reverted to false on rejection, retrying `Some(true)`
        // MUST emit a second control_request rather than being deduplicated!
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(true),
            })
            .await
            .expect("toggle 2");
        let notice2 = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
            .await
            .expect("notice 2 timeout")
            .expect("ev");
        match notice2 {
            DriverEvent::Notice { level, message } => {
                assert_eq!(level, "info");
                assert!(
                    message.contains("takes effect on next Claude session restart"),
                    "unexpected message: {message}"
                );
            }
            other => panic!("expected info Notice, got {other:?}"),
        }

        assert_eq!(observed_reqs.lock().await.len(), 2);

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn claude_post_restart_hydrates_bg_bash_and_settles_undeclared_subagent() {
        let mut tool_cache = HashMap::new();
        let mut bg_bash_tasks = HashMap::new();
        let mut bg_tool_use_to_task = HashMap::new();
        let mut bg_tool_uses = HashMap::new();
        let mut declared_tasks = HashMap::new();
        let tool_use_to_task = HashMap::new();
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        // 1. Hydrate a persisted running background bash task after daemon hot-restart
        let persisted = vec![crate::PersistedBackgroundTask {
            call_id: "toolu_bg_1".into(),
            detail: ToolCallDetail::Shell {
                command: "cargo test --workspace".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
            info: BackgroundTaskInfo {
                task_id: Some("bash_task_42".into()),
                label: Some("Run workspace tests".into()),
                log_path: Some("/tmp/bash_42.log".into()),
            },
        }];
        hydrate_reattached_claude_bg_tasks(
            &persisted,
            &mut tool_cache,
            &mut bg_bash_tasks,
            &mut bg_tool_use_to_task,
            &mut bg_tool_uses,
        );

        let bg_notif = json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "bash_task_42",
            "status": "completed",
            "summary": "All 500 tests passed"
        });
        handle_claude_task_notification(
            "ag_claude_parent",
            true,
            &bg_notif,
            &mut bg_bash_tasks,
            &mut bg_tool_use_to_task,
            &mut bg_tool_uses,
            &mut tool_cache,
            &mut declared_tasks,
            &tool_use_to_task,
            &evt_tx,
        )
        .await;

        let ev1 = evt_rx.try_recv().expect("bg tool call settlement");
        match ev1 {
            DriverEvent::ToolCall {
                call_id,
                status,
                detail:
                    ToolCallDetail::Shell {
                        exit_code,
                        output_preview,
                        ..
                    },
                ..
            } => {
                assert_eq!(call_id, "toolu_bg_1");
                assert_eq!(status, ToolCallStatus::Completed);
                assert_eq!(exit_code, Some(0));
                assert!(
                    output_preview
                        .as_deref()
                        .unwrap_or("")
                        .contains("All 500 tests passed")
                );
            }
            other => panic!("expected settled Shell ToolCall, got {other:?}"),
        }

        // 2. Fresh session (`is_reattached = false`) MUST NOT synthesize undeclared subagents,
        // and `local_bash` / `skip_transcript` MUST NOT synthesize fake subagents even on reattach.
        for (reattached, payload) in [
            (
                false,
                json!({
                    "type": "system",
                    "subtype": "task_notification",
                    "task_id": "sub_fresh_ignored",
                    "status": "completed"
                }),
            ),
            (
                true,
                json!({
                    "type": "system",
                    "subtype": "task_notification",
                    "task_id": "bash_untracked",
                    "task_type": "local_bash",
                    "status": "completed"
                }),
            ),
            (
                true,
                json!({
                    "type": "system",
                    "subtype": "task_notification",
                    "task_id": "internal_hook_task",
                    "skip_transcript": true,
                    "status": "completed"
                }),
            ),
        ] {
            handle_claude_task_notification(
                "ag_claude_parent",
                reattached,
                &payload,
                &mut bg_bash_tasks,
                &mut bg_tool_use_to_task,
                &mut bg_tool_uses,
                &mut tool_cache,
                &mut declared_tasks,
                &tool_use_to_task,
                &evt_tx,
            )
            .await;
            assert!(
                evt_rx.try_recv().is_err(),
                "must not synthesize fake subagent for {payload}"
            );
        }

        // 3. Post-restart (`is_reattached = true`) `task_notification` for an undeclared pre-restart subagent MUST emit terminal upsert
        let sub_notif = json!({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "sub_pre_restart_99",
            "description": "Code Reviewer Subagent",
            "status": "completed",
            "summary": "Review complete with 0 issues."
        });
        handle_claude_task_notification(
            "ag_claude_parent",
            true,
            &sub_notif,
            &mut bg_bash_tasks,
            &mut bg_tool_use_to_task,
            &mut bg_tool_uses,
            &mut tool_cache,
            &mut declared_tasks,
            &tool_use_to_task,
            &evt_tx,
        )
        .await;

        let ev2 = evt_rx.try_recv().expect("subagent upsert");
        match ev2 {
            DriverEvent::ProviderSubagentUpsert(desc) => {
                assert_eq!(desc.subagent_id, "sub_pre_restart_99");
                assert_eq!(desc.parent_agent_id, "ag_claude_parent");
                assert_eq!(desc.title, "Code Reviewer Subagent");
                assert_eq!(desc.status, ProviderSubagentStatus::Completed);
            }
            other => panic!("expected ProviderSubagentUpsert, got {other:?}"),
        }
        let ev3 = evt_rx.try_recv().expect("subagent summary message");
        assert!(matches!(
            ev3,
            DriverEvent::ProviderSubagentTimeline {
                subagent_id,
                payload: TimelinePayload::AssistantMessage { text, .. }
            } if subagent_id == "sub_pre_restart_99" && text.contains("Review complete")
        ));
    }

    #[test]
    fn select_dominant_claude_model_picks_highest_token_volume_not_alphabetical() {
        // "claude-haiku-4-5" sorts alphabetically before "claude-opus-4-6",
        // but Opus dominates token volume and must be chosen for pricing.
        let result_val = json!({
            "type": "result",
            "modelUsage": {
                "claude-haiku-4-5": {
                    "inputTokens": 400,
                    "outputTokens": 120,
                    "cacheReadInputTokens": 0,
                    "cacheCreationInputTokens": 0
                },
                "claude-opus-4-6": {
                    "inputTokens": 12000,
                    "outputTokens": 3500,
                    "cacheReadInputTokens": 45000,
                    "cacheCreationInputTokens": 2000
                }
            }
        });
        assert_eq!(
            select_dominant_claude_model(&result_val).as_deref(),
            Some("claude-opus-4-6")
        );

        let assistant_ev = json!({
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": 1200,
                    "cache_read_input_tokens": 38000,
                    "cache_creation_input_tokens": 800
                }
            }
        });
        assert_eq!(
            extract_claude_message_prompt_tokens(&assistant_ev),
            Some(40_000)
        );
    }
}
