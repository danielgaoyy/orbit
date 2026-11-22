//! Native Rust driver for Google Antigravity (`agy acp --stdio` + `transcript_full.jsonl` monitor + `/usage`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

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
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::attachments::{build_agy_prompt_blocks, is_path_within_dir};
use crate::driver::{
    DriverCommand, DriverEvent, ProviderSessionHandle, RpcId, SessionLaunchConfig,
};

pub async fn spawn_antigravity_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let (shim, is_reattached) = if let Some(existing) = reattached {
        (existing, true)
    } else {
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

        let mut env = cfg.extra_env.clone();
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
        crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);

        let spec = ShimSpawnSpec {
            shim_id: format!("agy_{}", cfg.agent_id),
            mode: ShimMode::Stdio,
            program: cfg.executable.to_string_lossy().to_string(),
            args,
            cwd: cfg.cwd.clone(),
            env,
        };

        let fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
            .await
            .map_err(|e| format!("failed to spawn agy shim: {e}"))?;
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_agy_actor(cfg, shim, is_reattached, cmd_rx, evt_tx));

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

fn build_agy_mcp_servers_http(cfg: &SessionLaunchConfig) -> Value {
    match &cfg.mcp_spec {
        Some(mcp) => json!([{
            "type": "http",
            "name": mcp.server_name,
            "url": mcp.http_url,
            "headers": [
                { "name": "Authorization", "value": format!("Bearer {}", mcp.bearer_token) }
            ]
        }]),
        None => json!([]),
    }
}

fn build_agy_mcp_servers_stdio(cfg: &SessionLaunchConfig) -> Value {
    match &cfg.mcp_spec {
        Some(mcp) => json!([{
            "name": mcp.server_name,
            "command": mcp.stdio_command.to_string_lossy(),
            "args": mcp.stdio_args,
            "env": []
        }]),
        None => json!([]),
    }
}

fn build_agy_session_meta(cfg: &SessionLaunchConfig) -> Option<Value> {
    let append = cfg
        .system_prompt_append
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(json!({
        "antigravity": {
            "systemPrompt": {
                "append": append
            }
        }
    }))
}

struct OpenedAgySession {
    session_id: String,
    available_model_ids: HashSet<String>,
    canonical_base_ids: HashSet<String>,
    current_base_model: Option<String>,
}

fn is_agy_missing_trajectory_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("trajectory not found")
        || (lower.contains("not_found") && lower.contains("trajectory"))
        || (lower.contains("trajectory") && lower.contains("does not exist"))
        || lower.contains("session not found")
}

fn prepend_handoff_context(ctx: Option<String>, prompt: &str) -> String {
    let Some(ctx) = ctx.filter(|c| !c.trim().is_empty()) else {
        return prompt.to_string();
    };
    if prompt.contains("[Prior Conversation Context in this Workspace]") {
        return prompt.to_string();
    }
    format!("{ctx}\n\n{prompt}")
}

#[derive(Debug, Clone, Copy)]
enum AgySessionOpenMode<'a> {
    New,
    Load(&'a str),
    Fork(&'a str),
}

fn agy_supports_session_fork(init_val: &Value) -> bool {
    for ptr in [
        "/agentCapabilities/sessionCapabilities/fork",
        "/agentCapabilities/session/fork",
        "/capabilities/session/fork",
    ] {
        if let Some(v) = init_val.pointer(ptr)
            && (v.is_object() || v.as_bool() == Some(true))
        {
            return true;
        }
    }
    false
}

#[allow(clippy::too_many_arguments)]
async fn open_agy_session(
    cfg: &SessionLaunchConfig,
    shim: &mut orbit_shim::ShimClient,
    next_rpc_id: &mut i64,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    open_mode: AgySessionOpenMode<'_>,
    requested_model: Option<&str>,
    requested_effort: Option<ReasoningEffort>,
    mode: &SessionMode,
) -> Result<OpenedAgySession, String> {
    let drains_replay = matches!(
        open_mode,
        AgySessionOpenMode::Load(_) | AgySessionOpenMode::Fork(_)
    );
    let open_session_with = |mcp_val: Value| {
        let (method, mut params) = match open_mode {
            AgySessionOpenMode::Fork(sid) => (
                "session/fork",
                json!({
                    "sessionId": sid,
                    "cwd": cfg.cwd.to_string_lossy(),
                    "mcpServers": mcp_val
                }),
            ),
            AgySessionOpenMode::Load(sid) => (
                "session/load",
                json!({
                    "sessionId": sid,
                    "cwd": cfg.cwd.to_string_lossy(),
                    "mcpServers": mcp_val
                }),
            ),
            AgySessionOpenMode::New => (
                "session/new",
                json!({
                    "cwd": cfg.cwd.to_string_lossy(),
                    "mcpServers": mcp_val
                }),
            ),
        };
        if let Some(meta) = build_agy_session_meta(cfg) {
            params["_meta"] = meta;
        }
        (method, params)
    };

    let (method, params) = open_session_with(build_agy_mcp_servers_http(cfg));
    let sess_rx = send_acp_req(shim, next_rpc_id, pending_rpcs, method, params)
        .await
        .map_err(|e| format!("failed to send {method}: {e}"))?;

    let sess_val =
        match wait_rpc_during_boot(shim, pending_rpcs, sess_rx, &cfg.shim_seq_tracker).await {
            Ok(v) => v,
            Err(first_err) if cfg.mcp_spec.is_some() => {
                // Fallback to stdio `orbit mcp` if ACP binary rejects HTTP MCP descriptor
                let (fb_method, fb_params) = open_session_with(build_agy_mcp_servers_stdio(cfg));
                let fb_rx = send_acp_req(shim, next_rpc_id, pending_rpcs, fb_method, fb_params)
                    .await
                    .map_err(|e| format!("failed to send stdio fallback {fb_method}: {e}"))?;
                wait_rpc_during_boot(shim, pending_rpcs, fb_rx, &cfg.shim_seq_tracker)
                    .await
                    .map_err(|e| format!("{first_err} (stdio fallback: {e})"))?
            }
            Err(e) => return Err(e),
        };

    // Drain replay notifications on `session/load` and `session/fork` until 50ms of quiet
    if drains_replay {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(50), shim.rx.recv()).await {
                Ok(Some(frame)) => {
                    if let Some(seq) = frame.seq() {
                        cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                    }
                }
                _ => break,
            }
        }
    }

    let fallback_sid = match open_mode {
        AgySessionOpenMode::Load(sid) => Some(sid),
        AgySessionOpenMode::Fork(_) | AgySessionOpenMode::New => None,
    };
    let session_id = sess_val
        .get("sessionId")
        .and_then(|v| v.as_str())
        .or(fallback_sid)
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
        .or_else(|| requested_model.map(ToOwned::to_owned));

    let mut current_base_model = raw_initial_model
        .as_deref()
        .map(|m| normalize_agy_base_model_id(m, &canonical_base_ids))
        .or_else(|| requested_model.map(ToOwned::to_owned));

    // Optional `session/set_model` when explicit model or effort was requested
    if (requested_model.is_some() || requested_effort.is_some())
        && let Some(base_m) = requested_model
            .map(ToOwned::to_owned)
            .or_else(|| current_base_model.clone())
    {
        let wire_model_id = resolve_agy_wire_model_id(
            &base_m,
            requested_effort,
            &available_model_ids,
            &canonical_base_ids,
        );
        if let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": wire_model_id
            }),
        )
        .await
            && let Ok(v) = wait_rpc_during_boot(shim, pending_rpcs, rx, &cfg.shim_seq_tracker).await
            && let Some(m) = v.pointer("/models/currentModelId").and_then(|x| x.as_str())
        {
            current_base_model = Some(normalize_agy_base_model_id(m, &canonical_base_ids));
        }
    }

    // Optional `session/set_mode` for `accept-edits` (ignore -32601)
    if matches!(mode, SessionMode::AcceptEdits)
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": "accept-edits"
            }),
        )
        .await
    {
        let _ = wait_rpc_during_boot(shim, pending_rpcs, rx, &cfg.shim_seq_tracker).await;
    }

    Ok(OpenedAgySession {
        session_id,
        available_model_ids,
        canonical_base_ids,
        current_base_model,
    })
}

async fn run_agy_actor(
    cfg: SessionLaunchConfig,
    mut shim: orbit_shim::ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_rpc_id: i64 = if is_reattached { 100_000 } else { 1 };
    let mut pending_rpcs: HashMap<String, oneshot::Sender<Result<Value, Value>>> = HashMap::new();
    let mut inject_handoff_on_next_prompt = false;
    let mut recovered_missing_trajectory = false;
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();

    let (
        mut session_id,
        mut available_model_ids,
        mut canonical_base_ids,
        mut active_effort,
        mut current_base_model,
    ) = if is_reattached && let Some(ref sid) = cfg.resume_session_id {
        (
            sid.clone(),
            HashSet::new(),
            HashSet::new(),
            cfg.effort,
            cfg.model.clone(),
        )
    } else {
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

        let init_val = match wait_rpc_during_boot(
            &mut shim,
            &mut pending_rpcs,
            init_rx,
            &cfg.shim_seq_tracker,
        )
        .await
        {
            Ok(v) => v,
            Err(_) => {
                warn!(agent_id = %cfg.agent_id, "Antigravity ACP initialize failed");
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                        message: "Antigravity ACP initialize failed".into(),
                        recoverable: false,
                    }))
                    .await;
                return;
            }
        };

        // 2. `session/fork`, `session/load`, or `session/new` with Orbit MCP servers (HTTP first, stdio fallback)
        let opened = if let Some(ref fork_sid) = cfg.fork_from_session_id {
            let native_attempt = if agy_supports_session_fork(&init_val) {
                open_agy_session(
                    &cfg,
                    &mut shim,
                    &mut next_rpc_id,
                    &mut pending_rpcs,
                    AgySessionOpenMode::Fork(fork_sid),
                    cfg.model.as_deref(),
                    cfg.effort,
                    &cfg.mode,
                )
                .await
            } else {
                Err("session/fork not advertised by Antigravity ACP".to_string())
            };
            match native_attempt {
                Ok(s)
                    if !s.session_id.is_empty()
                        && s.session_id != "unknown"
                        && s.session_id != *fork_sid =>
                {
                    s
                }
                other => {
                    let reason = match other {
                        Err(e) => e,
                        Ok(s) => format!(
                            "session/fork returned non-distinct or empty sessionId `{}`",
                            s.session_id
                        ),
                    };
                    warn!(
                        agent_id = %cfg.agent_id,
                        source_session_id = %fork_sid,
                        reason = %reason,
                        native_fork_only = cfg.native_fork_only,
                        "Antigravity native session/fork unavailable; falling back"
                    );
                    if cfg.native_fork_only {
                        let _ = shim.kill().await;
                        return;
                    }
                    match open_agy_session(
                        &cfg,
                        &mut shim,
                        &mut next_rpc_id,
                        &mut pending_rpcs,
                        AgySessionOpenMode::New,
                        cfg.model.as_deref(),
                        cfg.effort,
                        &cfg.mode,
                    )
                    .await
                    {
                        Ok(fresh) => {
                            inject_handoff_on_next_prompt = true;
                            fresh
                        }
                        Err(e) => {
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                    message: format!(
                                        "Antigravity fork fresh session open failed: {e}"
                                    ),
                                    recoverable: false,
                                }))
                                .await;
                            return;
                        }
                    }
                }
            }
        } else {
            let open_mode = match cfg.resume_session_id.as_deref() {
                Some(sid) => AgySessionOpenMode::Load(sid),
                None => AgySessionOpenMode::New,
            };
            match open_agy_session(
                &cfg,
                &mut shim,
                &mut next_rpc_id,
                &mut pending_rpcs,
                open_mode,
                cfg.model.as_deref(),
                cfg.effort,
                &cfg.mode,
            )
            .await
            {
                Ok(s) => {
                    if cfg.resume_session_id.is_none() && cfg.fallback_handoff_context.is_some() {
                        inject_handoff_on_next_prompt = true;
                    }
                    s
                }
                Err(first_err)
                    if cfg.resume_session_id.is_some()
                        && is_agy_missing_trajectory_error(&first_err) =>
                {
                    let old_short: String = cfg
                        .resume_session_id
                        .as_deref()
                        .unwrap_or_default()
                        .chars()
                        .take(8)
                        .collect();
                    warn!(
                        agent_id = %cfg.agent_id,
                        old_session_id = ?cfg.resume_session_id,
                        error = %first_err,
                        "Antigravity session/load failed with missing trajectory; transparently creating fresh session"
                    );
                    match open_agy_session(
                        &cfg,
                        &mut shim,
                        &mut next_rpc_id,
                        &mut pending_rpcs,
                        AgySessionOpenMode::New,
                        cfg.model.as_deref(),
                        cfg.effort,
                        &cfg.mode,
                    )
                    .await
                    {
                        Ok(fresh) => {
                            inject_handoff_on_next_prompt = true;
                            recovered_missing_trajectory = true;
                            let _ = evt_tx
                                .send(DriverEvent::Notice {
                                    level: "info".into(),
                                    message: format!(
                                        "Previous Antigravity session ({old_short}…) was evicted from local cache; automatically started a fresh session with recent conversation context restored."
                                    ),
                                })
                                .await;
                            fresh
                        }
                        Err(e) => {
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                    message: format!(
                                        "Antigravity session open failed: {first_err} (fresh session fallback: {e})"
                                    ),
                                    recoverable: false,
                                }))
                                .await;
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = evt_tx
                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                            message: format!("Antigravity session open failed: {e}"),
                            recoverable: false,
                        }))
                        .await;
                    return;
                }
            }
        };

        (
            opened.session_id,
            opened.available_model_ids,
            opened.canonical_base_ids,
            cfg.effort,
            opened.current_base_model,
        )
    };

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: session_id.clone(),
            model: current_base_model.clone(),
        })
        .await;
    if !is_reattached {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }

    // Active runtime state
    let mut mode = cfg.mode.clone();
    let mut active_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut reattached_turn_in_flight = is_reattached && active_turn_id.is_some();
    let mut active_prompt_rpc_key: Option<String> = None;
    let mut post_reattach_non_prompt_rpcs: HashSet<String> = HashSet::new();
    let mut last_dispatched_prompt: Option<(String, String)> = None;
    let mut seg_state = if let Some(ref tid) = active_turn_id {
        AgyStreamSegmentState {
            turn_tag: tid.clone(),
            step_seq: cfg.persisted_turn_step_seq,
            ..AgyStreamSegmentState::default()
        }
    } else {
        AgyStreamSegmentState::default()
    };
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingPerm> = HashMap::new();
    let mut pending_elicits: HashMap<String, PendingElicitation> = HashMap::new();
    let mut pending_prompts: VecDeque<(String, String)> = VecDeque::new();
    let mut subagent_monitors: HashMap<String, (ProviderSubagentDescriptor, AgyTranscriptMonitor)> =
        HashMap::new();

    // Post-turn autonomous wakeup (`transcript_full.jsonl`) + first-class background-task tracking
    let mut transcript_monitor = AgyTranscriptMonitor::new(&session_id);
    // Skip any historical steps already recorded on disk when attaching/resuming, while:
    // 1. Remembering already-finished background task IDs so shim ring-buffer replay can never
    //    resurrect them,
    // 2. Reconciling stale persisted `Running` background tasks in `orbit.db` in place when the
    //    transcript confirms they already finished (or the shim was not reattached),
    // 3. Re-seeding genuinely still-running background tasks across daemon hot-upgrades so their
    //    canonical `call_id` closes cleanly when they finish later, and
    // 4. Registering historical native subagents so Overview / SubagentsTrack reflect real state.
    let historical_bg_deltas = transcript_monitor.sync_silent_with_bg_tasks();
    let mut wakeup_tools = WakeupToolRegistry::default();
    let mut bg_tracker = BackgroundTaskTracker::default();
    struct HistoricalBgStart {
        description: Option<String>,
        log_path: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    }
    let mut historical_started: HashMap<String, HistoricalBgStart> = HashMap::new();
    let mut historical_finished: HashMap<String, bool> = HashMap::new();
    for delta in historical_bg_deltas {
        match delta {
            BgTaskDelta::Started {
                task_id,
                description,
                log_path,
                expires_at,
                ..
            } => {
                historical_started.insert(
                    task_id,
                    HistoricalBgStart {
                        description,
                        log_path,
                        expires_at,
                    },
                );
            }
            BgTaskDelta::Completed { task_id, canceled } => {
                historical_finished.insert(task_id.clone(), canceled);
                bg_tracker.mark_finished(task_id);
            }
            BgTaskDelta::SubagentSpawned {
                subagent_id,
                role,
                prompt_preview,
            } => {
                register_agy_subagent_monitor(
                    &cfg.agent_id,
                    &subagent_id,
                    &role,
                    &prompt_preview,
                    true,
                    &mut subagent_monitors,
                    &evt_tx,
                )
                .await;
            }
        }
    }
    let now_utc = Utc::now();
    let mut stale_settlements: Vec<(String, ToolCallStatus)> = Vec::new();
    for persisted in &cfg.persisted_running_bg_tasks {
        let tid_opt = persisted
            .info
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        if let Some(ref tid) = tid_opt
            && let Some(&canceled) = historical_finished.get(tid)
        {
            stale_settlements.push((
                persisted.call_id.clone(),
                if canceled {
                    ToolCallStatus::Failed
                } else {
                    ToolCallStatus::Completed
                },
            ));
        } else if !is_reattached {
            stale_settlements.push((persisted.call_id.clone(), ToolCallStatus::Failed));
        } else if let Some(tid) = tid_opt
            && let Some(hist) = historical_started.get(&tid)
        {
            if is_one_shot_timer_expired(hist.expires_at, now_utc) {
                bg_tracker.mark_finished(tid);
                stale_settlements.push((persisted.call_id.clone(), ToolCallStatus::Completed));
            } else {
                wakeup_tools.register_synthetic_bg_task(
                    &persisted.call_id,
                    persisted.detail.clone(),
                    persisted.info.clone(),
                );
                tool_cache.insert(persisted.call_id.clone(), persisted.detail.clone());
                bg_tracker.record_started_with_expiry(
                    tid,
                    Some(persisted.call_id.clone()),
                    persisted.info.log_path.clone(),
                    Some(persisted.info.clone()),
                    hist.expires_at,
                );
            }
        } else {
            stale_settlements.push((persisted.call_id.clone(), ToolCallStatus::Completed));
        }
    }
    if is_reattached {
        for (tid, hist) in historical_started {
            if historical_finished.contains_key(&tid) || bg_tracker.tasks.contains_key(&tid) {
                continue;
            }
            if is_one_shot_timer_expired(hist.expires_at, now_utc) {
                bg_tracker.mark_finished(tid);
                continue;
            }
            let bg_info = BackgroundTaskInfo {
                task_id: Some(tid.clone()),
                label: hist.description,
                log_path: hist.log_path.clone(),
            };
            bg_tracker.record_started_with_expiry(
                tid,
                None,
                hist.log_path,
                Some(bg_info),
                hist.expires_at,
            );
        }
    }
    let reconciled_stale = !stale_settlements.is_empty();
    if reconciled_stale {
        let _ = evt_tx
            .send(DriverEvent::ReconcileBackgroundTasks(stale_settlements))
            .await;
    }
    let mut autonomous_turn_id: Option<String> = if is_reattached && !bg_tracker.is_empty() {
        let wait_tid = format!("bgwait_{}", uuid::Uuid::new_v4().simple());
        if cfg.persisted_bg_wait || reconciled_stale {
            let label = bg_tracker.waiting_source_label();
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup {
                    turn_id: wait_tid.clone(),
                    source: label,
                }))
                .await;
        }
        Some(wait_tid)
    } else {
        if is_reattached && (cfg.persisted_bg_wait || reconciled_stale) {
            let _ = evt_tx
                .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                .await;
        }
        None
    };
    let mut user_interrupted = false;
    let mut pending_self_heal: Option<PendingSelfHeal> = None;
    let mut consecutive_self_heals: u32 = 0;
    let mut last_ask_question_raw: Option<Vec<QuestionItem>> = None;
    let mut transcript_poll = tokio::time::interval(Duration::from_millis(200));
    let mut idle_poll_tick: u8 = 0;

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                idle_poll_tick = 0;
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        prompt,
                        behavior,
                    } => {
                        user_interrupted = false;
                        pending_self_heal = None;
                        consecutive_self_heals = 0;
                        if active_prompt_rpc_key.is_some() || active_turn_id.is_some() {
                            if !pending_perms.is_empty() || !pending_elicits.is_empty() {
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "warning".into(),
                                        message: "Waiting for your permission or question response above. Please answer it (or stop the current turn) before sending a new prompt.".into(),
                                    })
                                    .await;
                            } else {
                                match behavior {
                                    ActiveTurnBehavior::Queue => {
                                        pending_prompts.push_back((turn_id, prompt));
                                    }
                                    ActiveTurnBehavior::Steer => {
                                        pending_prompts.push_front((turn_id, prompt));
                                    }
                                    ActiveTurnBehavior::Interrupt => {
                                        let cancel = json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/cancel",
                                            "params": { "sessionId": session_id }
                                        })
                                        .to_string();
                                        let _ = shim.write_stdin_line(cancel).await;
                                        pending_prompts.push_front((turn_id, prompt));
                                    }
                                }
                            }
                            continue;
                        }

                        let effective_prompt = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };
                        last_dispatched_prompt =
                            Some((turn_id.clone(), effective_prompt.clone()));
                        wakeup_tools.begin_turn();
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
                            turn_id,
                            effective_prompt,
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        user_interrupted = true;
                        pending_self_heal = None;
                        bg_tracker.clear();
                        if autonomous_turn_id.take().is_some() {
                            transcript_monitor.sync_silent();
                            for (cid, detail, bg_info) in
                                wakeup_tools.settle_open_tools(&HashSet::new())
                            {
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id: cid,
                                        status: ToolCallStatus::Failed,
                                        detail,
                                        background_task: bg_info,
                                    })
                                    .await;
                            }
                            if active_turn_id.is_none() {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                    .await;
                            }
                        }
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
                            post_reattach_non_prompt_rpcs.insert(RpcId::Num(id).as_key());
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
                            post_reattach_non_prompt_rpcs.insert(RpcId::Num(id).as_key());
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
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                        let method = val.get("method").and_then(|v| v.as_str());

                        if has_id && method.is_none() {
                            // Response to our request (e.g. `session/prompt` completion)
                            if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone()) {
                                let key = rid.as_key();
                                if post_reattach_non_prompt_rpcs.remove(&key) {
                                    continue;
                                }
                                let is_reattached_completion = reattached_turn_in_flight
                                    && active_prompt_rpc_key.is_none()
                                    && (val.pointer("/result/stopReason").is_some()
                                        || val.get("error").is_some());
                                if active_prompt_rpc_key.as_deref() == Some(key.as_str())
                                    || is_reattached_completion
                                {
                                    reattached_turn_in_flight = false;
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

                                    // Transparently recover when `session/load` silently accepted an
                                    // evicted Antigravity session ID and `session/prompt` then fails
                                    // with `trajectory not found`: open a fresh `session/new`, update
                                    // `persistence_ref`, carry over recent conversation context, and
                                    // re-dispatch the user's prompt seamlessly.
                                    if let Some(err_obj) = val.get("error") {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if !recovered_missing_trajectory
                                            && is_agy_missing_trajectory_error(err_msg)
                                            && let Some((failed_turn_id, failed_prompt)) =
                                                last_dispatched_prompt.take()
                                        {
                                            recovered_missing_trajectory = true;
                                            let old_short: String =
                                                session_id.chars().take(8).collect();
                                            warn!(
                                                agent_id = %cfg.agent_id,
                                                old_session_id = %session_id,
                                                error = %err_msg,
                                                "Antigravity trajectory missing on disk; transparently recreating session with conversation handoff"
                                            );
                                            match open_agy_session(
                                                &cfg,
                                                &mut shim,
                                                &mut next_rpc_id,
                                                &mut pending_rpcs,
                                                AgySessionOpenMode::New,
                                                current_base_model.as_deref(),
                                                active_effort,
                                                &mode,
                                            )
                                            .await
                                            {
                                                Ok(fresh) => {
                                                    session_id = fresh.session_id;
                                                    available_model_ids = fresh.available_model_ids;
                                                    canonical_base_ids = fresh.canonical_base_ids;
                                                    current_base_model = fresh.current_base_model;
                                                    transcript_monitor =
                                                        AgyTranscriptMonitor::new(&session_id);
                                                    transcript_monitor.sync_silent();
                                                    let _ = evt_tx
                                                        .send(DriverEvent::SessionInitialized {
                                                            persistence_ref: session_id.clone(),
                                                            model: current_base_model.clone(),
                                                        })
                                                        .await;
                                                    let _ = evt_tx
                                                        .send(DriverEvent::Notice {
                                                            level: "info".into(),
                                                            message: format!(
                                                                "Previous Antigravity session ({old_short}…) was evicted from local cache; automatically started a fresh session with recent conversation context restored."
                                                            ),
                                                        })
                                                        .await;
                                                    let recovered_prompt = prepend_handoff_context(
                                                        pending_handoff_context.take(),
                                                        &failed_prompt,
                                                    );
                                                    last_dispatched_prompt = Some((
                                                        failed_turn_id.clone(),
                                                        recovered_prompt.clone(),
                                                    ));
                                                    wakeup_tools.begin_turn();
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
                                                        failed_turn_id,
                                                        recovered_prompt,
                                                    )
                                                    .await
                                                    .is_err()
                                                    {
                                                        break;
                                                    }
                                                    continue;
                                                }
                                                Err(reopen_err) => {
                                                    warn!(
                                                        agent_id = %cfg.agent_id,
                                                        error = %reopen_err,
                                                        "Failed to create fresh Antigravity session after missing trajectory"
                                                    );
                                                }
                                            }
                                        }
                                    }
                                    last_dispatched_prompt = None;

                                    // Fast-forward transcript reader past foreground steps while still
                                    // capturing any newly spawned/completed background task IDs.
                                    let bg_deltas = transcript_monitor.sync_silent_with_bg_tasks();
                                    apply_foreground_bg_deltas(
                                        &cfg.agent_id,
                                        bg_deltas,
                                        &mut wakeup_tools,
                                        &mut bg_tracker,
                                        &mut tool_cache,
                                        &mut subagent_monitors,
                                        &evt_tx,
                                    )
                                    .await;
                                    // Settle any non-background foreground tools left open when the turn ended
                                    let active_bg_calls = bg_tracker.active_call_ids();
                                    let settle_status = if val.get("error").is_some() {
                                        ToolCallStatus::Failed
                                    } else {
                                        ToolCallStatus::Completed
                                    };
                                    for (cid, detail, bg_info) in
                                        wakeup_tools.settle_open_tools(&active_bg_calls)
                                    {
                                        let _ = evt_tx
                                            .send(DriverEvent::ToolCall {
                                                call_id: cid,
                                                status: settle_status,
                                                detail,
                                                background_task: bg_info,
                                            })
                                            .await;
                                    }
                                    // Drain queued / steered / interrupted-replacement prompt
                                    if let Some((next_turn_id, next_prompt)) =
                                        pending_prompts.pop_front()
                                    {
                                        last_dispatched_prompt =
                                            Some((next_turn_id.clone(), next_prompt.clone()));
                                        wakeup_tools.begin_turn();
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
                                    } else if !user_interrupted && !bg_tracker.is_empty() {
                                        // First-class background task state: remain active until all
                                        // tracked background tasks finish and their follow-up settles.
                                        let wait_tid =
                                            format!("bgwait_{}", uuid::Uuid::new_v4().simple());
                                        let label = bg_tracker.waiting_source_label();
                                        autonomous_turn_id = Some(wait_tid.clone());
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(
                                                AgentStatus::AutonomousWakeup {
                                                    turn_id: wait_tid,
                                                    source: label,
                                                },
                                            ))
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
                                let update_kind = update
                                    .get("sessionUpdate")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                // During transcript-driven post-turn wakeup, still process tool_call /
                                // tool_call_update so ACP native IDs unify with `wake_tool_*` and
                                // background-task signal cancellations are detected immediately.
                                let in_post_turn_wakeup = active_turn_id.is_none()
                                    && autonomous_turn_id.is_some();
                                // Before streaming the first chunk of a foreground reply, flush any
                                // background-task metadata already written to `transcript_full.jsonl`
                                // (e.g. `schedule` timers whose ACP update omits task IDs) so those
                                // tool calls keep their chronological `seq` BEFORE the reply in SQLite
                                // instead of spawning a detached step group below it.
                                if !in_post_turn_wakeup
                                    && update_kind == "agent_message_chunk"
                                    && seg_state.active_msg_id.is_none()
                                {
                                    let bg_deltas =
                                        transcript_monitor.poll_foreground_bg_deltas();
                                    if !bg_deltas.is_empty() {
                                        apply_foreground_bg_deltas(
                                            &cfg.agent_id,
                                            bg_deltas,
                                            &mut wakeup_tools,
                                            &mut bg_tracker,
                                            &mut tool_cache,
                                            &mut subagent_monitors,
                                            &evt_tx,
                                        )
                                        .await;
                                    }
                                }
                                let allow_update = !in_post_turn_wakeup
                                    || matches!(update_kind, "tool_call" | "tool_call_update");
                                if allow_update
                                    && let Some(sig_cancel) = handle_acp_session_update(
                                        &cfg.agent_id,
                                        &session_id,
                                        update,
                                        in_post_turn_wakeup,
                                        &mut wakeup_tools,
                                        &mut bg_tracker,
                                        &mut seg_state,
                                        &mut tool_cache,
                                        &mut subagent_monitors,
                                        &mut last_ask_question_raw,
                                        &evt_tx,
                                    )
                                    .await
                                    && !user_interrupted
                                    && pending_self_heal.is_none()
                                {
                                    pending_self_heal = Some(PendingSelfHeal {
                                        task_id: sig_cancel.task_id,
                                        reason: sig_cancel.reason,
                                        log_path: sig_cancel.log_path,
                                        output_tail: sig_cancel.output_tail,
                                        detected_at: Instant::now(),
                                        transcript_step_at_detect: transcript_monitor
                                            .max_seen_step(),
                                    });
                                }
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
                let has_running_subagent = subagent_monitors
                    .values()
                    .any(|(desc, _)| desc.status == ProviderSubagentStatus::Running);
                let is_fully_idle = active_turn_id.is_none()
                    && autonomous_turn_id.is_none()
                    && bg_tracker.is_empty()
                    && pending_self_heal.is_none()
                    && !transcript_monitor.in_wakeup
                    && !has_running_subagent;
                if is_fully_idle {
                    idle_poll_tick = idle_poll_tick.wrapping_add(1);
                    if !idle_poll_tick.is_multiple_of(5) {
                        continue;
                    }
                } else {
                    idle_poll_tick = 0;
                }

                // Poll native Antigravity subagent transcripts for read-only Subagents Track timelines
                for (sub_id, (desc, sub_mon)) in subagent_monitors.iter_mut() {
                    if desc.status != ProviderSubagentStatus::Running {
                        continue;
                    }
                    poll_agy_subagent_monitor(sub_id, desc, sub_mon, &evt_tx).await;
                }

                // Only monitor post-turn autonomous wakeups & self-healing when foreground turn is idle
                if active_turn_id.is_none() {
                    for wake_ev in transcript_monitor.poll_post_turn_wakeup() {
                        match wake_ev {
                            WakeupStep::Started { source } => {
                                // Native `agy` wakeup occurred — cancel any staged self-heal!
                                pending_self_heal = None;
                                wakeup_tools.begin_turn();
                                let tid = format!("wakeup_{}", uuid::Uuid::new_v4().simple());
                                autonomous_turn_id = Some(tid.clone());
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup {
                                        turn_id: tid,
                                        source,
                                    }))
                                    .await;
                            }
                            WakeupStep::BackgroundTaskStarted {
                                task_id,
                                call_id,
                                description,
                                log_path,
                                expires_at,
                            } => {
                                let bg_info = BackgroundTaskInfo {
                                    task_id: Some(task_id.clone()),
                                    label: description.clone(),
                                    log_path: log_path.clone(),
                                };
                                let canon_cid = call_id
                                    .map(|cid| {
                                        wakeup_tools.canonical_id_for_wakeup(&cid).to_string()
                                    })
                                    .or_else(|| {
                                        bg_tracker.call_id_for(&task_id).map(ToOwned::to_owned)
                                    })
                                    .or_else(|| {
                                        wakeup_tools
                                            .find_matching_call_id_for_bg_task(description.as_deref())
                                    });
                                if let Some(ref cid) = canon_cid {
                                    wakeup_tools.mark_background_task(cid, bg_info.clone());
                                }
                                bg_tracker.record_started_with_expiry(
                                    task_id,
                                    canon_cid,
                                    log_path,
                                    Some(bg_info),
                                    expires_at,
                                );
                            }
                            WakeupStep::BackgroundTaskCompleted { task_id, canceled } => {
                                if let Some(info) = bg_tracker.remove(&task_id)
                                    && let Some(ref cid) = info.call_id
                                {
                                    let final_st = if canceled {
                                        ToolCallStatus::Failed
                                    } else {
                                        ToolCallStatus::Completed
                                    };
                                    if let Some((detail, bg_info)) =
                                        wakeup_tools.finish_tool(cid, final_st)
                                    {
                                        let _ = evt_tx
                                            .send(DriverEvent::ToolCall {
                                                call_id: cid.clone(),
                                                status: final_st,
                                                detail,
                                                background_task: bg_info,
                                            })
                                            .await;
                                    }
                                }
                                if !canceled {
                                    consecutive_self_heals = 0;
                                }
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
                                if let WakeupToolResolution::Emit {
                                    call_id: canon_id,
                                    status: emit_status,
                                    detail: emit_detail,
                                    background_task,
                                } = wakeup_tools.resolve_wakeup_tool(&call_id, status, detail)
                                {
                                    if let ToolCallDetail::Subagent {
                                        child_agent_id: Some(ref child_conv_id),
                                        ref role,
                                        ref prompt_preview,
                                    } = emit_detail
                                    {
                                        register_agy_subagent_monitor(
                                            &cfg.agent_id,
                                            child_conv_id,
                                            role,
                                            prompt_preview,
                                            false,
                                            &mut subagent_monitors,
                                            &evt_tx,
                                        )
                                        .await;
                                    }
                                    tool_cache.insert(canon_id.clone(), emit_detail.clone());
                                    let _ = evt_tx
                                        .send(DriverEvent::ToolCall {
                                            call_id: canon_id,
                                            status: emit_status,
                                            detail: emit_detail,
                                            background_task,
                                        })
                                        .await;
                                }
                            }
                            WakeupStep::SubagentSpawned {
                                subagent_id,
                                role,
                                prompt_preview,
                            } => {
                                register_agy_subagent_monitor(
                                    &cfg.agent_id,
                                    &subagent_id,
                                    &role,
                                    &prompt_preview,
                                    false,
                                    &mut subagent_monitors,
                                    &evt_tx,
                                )
                                .await;
                            }
                            WakeupStep::Notice { level, message } => {
                                if level != "info" {
                                    warn!(
                                        agent_id = %cfg.agent_id,
                                        session_id = %session_id,
                                        level = %level,
                                        notice = %message,
                                        "Antigravity post-turn wakeup reported runtime notice"
                                    );
                                }
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
                                for (cid, detail, bg_info) in
                                    wakeup_tools.settle_open_tools(&bg_tracker.active_call_ids())
                                {
                                    let _ = evt_tx
                                        .send(DriverEvent::ToolCall {
                                            call_id: cid,
                                            status: ToolCallStatus::Failed,
                                            detail,
                                            background_task: bg_info,
                                        })
                                        .await;
                                }
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                        message,
                                        recoverable: true,
                                    }))
                                    .await;
                            }
                            WakeupStep::Settled => {
                                // Close any open wakeup/ACP tools not tied to still-running background tasks
                                let active_bg_calls = bg_tracker.active_call_ids();
                                for (cid, detail, bg_info) in
                                    wakeup_tools.settle_open_tools(&active_bg_calls)
                                {
                                    let _ = evt_tx
                                        .send(DriverEvent::ToolCall {
                                            call_id: cid,
                                            status: ToolCallStatus::Completed,
                                            detail,
                                            background_task: bg_info,
                                        })
                                        .await;
                                }
                                if !user_interrupted && !bg_tracker.is_empty() {
                                    // Still waiting for background task(s): remain active, do NOT drop to Idle!
                                    let wait_tid = format!(
                                        "bgwait_{}",
                                        uuid::Uuid::new_v4().simple()
                                    );
                                    let label = bg_tracker.waiting_source_label();
                                    autonomous_turn_id = Some(wait_tid.clone());
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(
                                            AgentStatus::AutonomousWakeup {
                                                turn_id: wait_tid,
                                                source: label,
                                            },
                                        ))
                                        .await;
                                } else if pending_self_heal.is_none()
                                    && autonomous_turn_id.take().is_some()
                                {
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                        .await;
                                }
                            }
                        }
                    }

                    // Reap one-shot timers that are past their deadline + grace period without
                    // firing (e.g. silently cancelled by Antigravity) so they can never wedge the
                    // waiting UI permanently.
                    let expired_timers = bg_tracker.reap_expired_timers(Utc::now());
                    if !expired_timers.is_empty() {
                        for info in expired_timers {
                            if let Some(ref cid) = info.call_id
                                && let Some((detail, bg_info)) =
                                    wakeup_tools.finish_tool(cid, ToolCallStatus::Completed)
                            {
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id: cid.clone(),
                                        status: ToolCallStatus::Completed,
                                        detail,
                                        background_task: bg_info,
                                    })
                                    .await;
                            }
                        }
                        if !transcript_monitor.in_wakeup {
                            if bg_tracker.is_empty()
                                && pending_self_heal.is_none()
                                && autonomous_turn_id.take().is_some()
                            {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                    .await;
                            } else if !bg_tracker.is_empty()
                                && let Some(ref wait_tid) = autonomous_turn_id
                            {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(
                                        AgentStatus::AutonomousWakeup {
                                            turn_id: wait_tid.clone(),
                                            source: bg_tracker.waiting_source_label(),
                                        },
                                    ))
                                    .await;
                            }
                        }
                    }

                    // Scan `.system_generated/messages/undelivered/` for MESSAGE_PRIORITY_LOW
                    // background task cancellations (when user did NOT click Stop).
                    if !user_interrupted {
                        for cancel_ev in transcript_monitor.poll_undelivered_cancellations(&bg_tracker)
                        {
                            let tracked_info = bg_tracker.remove(&cancel_ev.task_id);
                            let log_path = cancel_ev
                                .log_path
                                .or_else(|| tracked_info.as_ref().and_then(|i| i.log_path.clone()));
                            let output_tail = log_path
                                .as_deref()
                                .and_then(|p| read_log_tail(Path::new(p), 1500));
                            if let Some(ref info) = tracked_info
                                && let Some(ref cid) = info.call_id
                                && let Some((detail, bg_info)) =
                                    wakeup_tools.finish_tool(cid, ToolCallStatus::Failed)
                            {
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id: cid.clone(),
                                        status: ToolCallStatus::Failed,
                                        detail,
                                        background_task: bg_info,
                                    })
                                    .await;
                            }
                            if pending_self_heal.is_none() {
                                pending_self_heal = Some(PendingSelfHeal {
                                    task_id: cancel_ev.task_id,
                                    reason: cancel_ev.reason,
                                    log_path,
                                    output_tail,
                                    detected_at: Instant::now(),
                                    transcript_step_at_detect: transcript_monitor.max_seen_step(),
                                });
                            }
                        }
                    }

                    // Evaluate staged Self-Healing Wakeup after grace period (4s)
                    if let Some(ref heal) = pending_self_heal {
                        if user_interrupted
                            || transcript_monitor.max_seen_step() > heal.transcript_step_at_detect
                        {
                            // User stopped or `agy` woke up on its own — cancel self-heal
                            pending_self_heal = None;
                        } else if heal.detected_at.elapsed() >= SELF_HEAL_GRACE_PERIOD {
                            let heal = pending_self_heal.take().expect("checked Some");
                            if consecutive_self_heals >= MAX_CONSECUTIVE_SELF_HEALS {
                                warn!(
                                    agent_id = %cfg.agent_id,
                                    task_id = %heal.task_id,
                                    attempts = consecutive_self_heals,
                                    "Self-healing wakeup loop guard triggered; handing back to user"
                                );
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "warning".into(),
                                        message: format!(
                                            "Background task {} was terminated by a signal ({MAX_CONSECUTIVE_SELF_HEALS} consecutive auto-resume attempts reached). Stopping automatic resume so you can inspect the failure.",
                                            heal.task_id
                                        ),
                                    })
                                    .await;
                                if bg_tracker.is_empty() && autonomous_turn_id.take().is_some() {
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                        .await;
                                }
                            } else {
                                consecutive_self_heals = consecutive_self_heals.saturating_add(1);
                                let attempt = consecutive_self_heals;
                                info!(
                                    agent_id = %cfg.agent_id,
                                    task_id = %heal.task_id,
                                    attempt,
                                    "Dispatching Orbit self-healing wakeup after unexpected signal termination"
                                );
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "info".into(),
                                        message: format!(
                                            "Background task {} ended unexpectedly due to an OS signal (not user-canceled); automatically resuming agent (attempt {attempt}/{MAX_CONSECUTIVE_SELF_HEALS})…",
                                            heal.task_id
                                        ),
                                    })
                                    .await;
                                let recovery_turn =
                                    format!("selfheal_{}", uuid::Uuid::new_v4().simple());
                                let recovery_prompt = build_self_heal_prompt(&heal, attempt);
                                wakeup_tools.begin_turn();
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
                                    recovery_turn,
                                    recovery_prompt,
                                )
                                .await
                                .is_err()
                                {
                                    break;
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
async fn apply_foreground_bg_deltas(
    parent_agent_id: &str,
    bg_deltas: Vec<BgTaskDelta>,
    wakeup_tools: &mut WakeupToolRegistry,
    bg_tracker: &mut BackgroundTaskTracker,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    subagent_monitors: &mut HashMap<String, (ProviderSubagentDescriptor, AgyTranscriptMonitor)>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    for delta in bg_deltas {
        match delta {
            BgTaskDelta::Started {
                task_id,
                call_id,
                description,
                log_path,
                expires_at,
            } => {
                if bg_tracker.is_finished(&task_id) {
                    continue;
                }
                let bg_info = BackgroundTaskInfo {
                    task_id: Some(task_id.clone()),
                    label: description.clone(),
                    log_path: log_path.clone(),
                };
                let resolved_cid = call_id
                    .or_else(|| bg_tracker.call_id_for(&task_id).map(ToOwned::to_owned))
                    .or_else(|| {
                        wakeup_tools.find_matching_call_id_for_bg_task(description.as_deref())
                    });
                let (final_cid, emit_detail, emit_bg_info) = if let Some(cid) = resolved_cid {
                    match wakeup_tools.mark_background_task(&cid, bg_info.clone()) {
                        Some((det, merged_bg, true)) => (cid, Some(det), merged_bg),
                        Some((_, merged_bg, false)) => (cid, None, merged_bg),
                        None => (cid, None, bg_info.clone()),
                    }
                } else {
                    let synth_id = format!("bg_task_{}", sanitize_bg_task_id(&task_id));
                    let desc_text = description
                        .clone()
                        .unwrap_or_else(|| "Background task".to_string());
                    let is_schedule_bg = is_schedule_bg_task_description(description.as_deref());
                    let det = if is_schedule_bg {
                        ToolCallDetail::Generic {
                            tool_name: "schedule".to_string(),
                            arguments_json: desc_text,
                        }
                    } else {
                        ToolCallDetail::Shell {
                            command: desc_text,
                            cwd: None,
                            exit_code: None,
                            output_preview: None,
                        }
                    };
                    wakeup_tools.register_synthetic_bg_task(
                        &synth_id,
                        det.clone(),
                        bg_info.clone(),
                    );
                    (synth_id, Some(det), bg_info.clone())
                };
                bg_tracker.record_started_with_expiry(
                    task_id,
                    Some(final_cid.clone()),
                    log_path,
                    Some(emit_bg_info.clone()),
                    expires_at,
                );
                if let Some(detail) = emit_detail {
                    tool_cache.insert(final_cid.clone(), detail.clone());
                    let _ = evt_tx
                        .send(DriverEvent::ToolCall {
                            call_id: final_cid,
                            status: ToolCallStatus::Running,
                            detail,
                            background_task: Some(emit_bg_info),
                        })
                        .await;
                }
            }
            BgTaskDelta::Completed { task_id, canceled } => {
                if let Some(info) = bg_tracker.remove(&task_id)
                    && let Some(ref cid) = info.call_id
                {
                    let final_st = if canceled {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    };
                    if let Some((detail, bg_info)) = wakeup_tools.finish_tool(cid, final_st) {
                        let _ = evt_tx
                            .send(DriverEvent::ToolCall {
                                call_id: cid.clone(),
                                status: final_st,
                                detail,
                                background_task: bg_info,
                            })
                            .await;
                    }
                }
            }
            BgTaskDelta::SubagentSpawned {
                subagent_id,
                role,
                prompt_preview,
            } => {
                register_agy_subagent_monitor(
                    parent_agent_id,
                    &subagent_id,
                    &role,
                    &prompt_preview,
                    false,
                    subagent_monitors,
                    evt_tx,
                )
                .await;
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
    shim_seq_tracker: &std::sync::atomic::AtomicU64,
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
                    Ok(Some(f)) => {
                        if let Some(seq) = f.seq() {
                            shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                        }
                        match f {
                            ShimFrame::StdoutLine { line, .. } => {
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
                            ShimFrame::Exited { code } => {
                                return Err(format!("agy exited early with code {code:?}"));
                            }
                            _ => {}
                        }
                    }
                    _ => return Err("agy shim closed".into()),
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_acp_session_update(
    parent_agent_id: &str,
    session_id: &str,
    update: &Value,
    in_post_turn_wakeup: bool,
    wakeup_tools: &mut WakeupToolRegistry,
    bg_tracker: &mut BackgroundTaskTracker,
    seg_state: &mut AgyStreamSegmentState,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    subagent_monitors: &mut HashMap<String, (ProviderSubagentDescriptor, AgyTranscriptMonitor)>,
    last_ask_question_raw: &mut Option<Vec<QuestionItem>>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) -> Option<AcpSignalCancelEvent> {
    let kind = update.get("sessionUpdate").and_then(|v| v.as_str())?;

    if kind == "user_message_chunk" {
        return None;
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
                return None;
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
                return None;
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
            None
        }
        "agent_thought_chunk" => {
            let chunk = update
                .pointer("/content/text")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if chunk.is_empty() {
                return None;
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
            None
        }
        "tool_call" | "tool_call_update" => {
            let raw_call_id = update.get("toolCallId").and_then(|v| v.as_str())?;
            let status = match update.get("status").and_then(|v| v.as_str()) {
                Some("completed") => ToolCallStatus::Completed,
                Some("failed") => ToolCallStatus::Failed,
                _ => ToolCallStatus::Running,
            };
            let cached_prev = wakeup_tools
                .lookup_canonical_id(raw_call_id)
                .and_then(|cid| tool_cache.get(cid))
                .or_else(|| tool_cache.get(raw_call_id));
            let incoming_detail = map_agy_tool_detail(update, cached_prev);
            let raw_output_text = extract_acp_raw_output_text(update.get("rawOutput"));
            let parsed_bg_start = if status == ToolCallStatus::Failed {
                None
            } else {
                raw_output_text
                    .as_deref()
                    .and_then(|out| parse_bg_task_started(out, session_id))
                    .filter(|(tid, _, _)| !bg_tracker.is_finished(tid))
            };
            // Do not let an ACP dispatch-ack `completed` update (e.g. `Run schedule` returning
            // immediately after registering a background timer without an exit code or completion
            // notice) prematurely settle an active background task!
            let preserve_active_bg_running = status == ToolCallStatus::Completed
                && parsed_bg_start.is_none()
                && wakeup_tools.is_active_bg_task_entry(raw_call_id, &incoming_detail, bg_tracker)
                && !acp_output_indicates_bg_completion(
                    update.get("rawOutput"),
                    raw_output_text.as_deref(),
                );
            let effective_status = if parsed_bg_start.is_some() || preserve_active_bg_running {
                ToolCallStatus::Running
            } else {
                status
            };
            let incoming_bg_info =
                parsed_bg_start
                    .as_ref()
                    .map(|(tid, desc, lp)| BackgroundTaskInfo {
                        task_id: Some(tid.clone()),
                        label: desc.clone(),
                        log_path: lp.clone(),
                    });

            let resolved = wakeup_tools.resolve_acp_tool(
                raw_call_id,
                effective_status,
                incoming_detail,
                incoming_bg_info,
                in_post_turn_wakeup,
            );
            let (call_id, detail, background_task) = match resolved {
                AcpToolResolution::SkipDuplicate => {
                    // Even if duplicate card suppressed, still inspect signal-cancellation telemetry
                    let canon_or_raw = wakeup_tools
                        .lookup_canonical_id(raw_call_id)
                        .unwrap_or(raw_call_id);
                    return detect_acp_signal_cancel(
                        canon_or_raw,
                        effective_status,
                        raw_output_text.as_deref(),
                        bg_tracker,
                    );
                }
                AcpToolResolution::Emit {
                    call_id,
                    detail,
                    background_task,
                } => (call_id, detail, background_task),
            };

            // Track background tasks launched / completed / signal-terminated via ACP
            let mut signal_cancel = None;
            if let Some((task_id, _desc, log_path)) = parsed_bg_start {
                bg_tracker.record_started(
                    task_id,
                    Some(call_id.clone()),
                    log_path,
                    background_task.clone(),
                );
            } else if matches!(
                effective_status,
                ToolCallStatus::Completed | ToolCallStatus::Failed
            ) {
                if let Some(ref out_str) = raw_output_text {
                    signal_cancel = detect_acp_signal_cancel(
                        &call_id,
                        effective_status,
                        Some(out_str.as_str()),
                        bg_tracker,
                    );
                    if signal_cancel.is_none() {
                        bg_tracker.remove_by_call_id(&call_id);
                    }
                } else {
                    bg_tracker.remove_by_call_id(&call_id);
                }
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
            if let ToolCallDetail::Subagent {
                child_agent_id: Some(ref child_conv_id),
                ref role,
                ref prompt_preview,
            } = detail
            {
                register_agy_subagent_monitor(
                    parent_agent_id,
                    child_conv_id,
                    role,
                    prompt_preview,
                    false,
                    subagent_monitors,
                    evt_tx,
                )
                .await;
            }
            tool_cache.insert(call_id.clone(), detail.clone());
            let _ = evt_tx
                .send(DriverEvent::ToolCall {
                    call_id,
                    status: effective_status,
                    detail,
                    background_task,
                })
                .await;
            signal_cancel
        }
        _ => None,
    }
}

async fn poll_agy_subagent_monitor(
    sub_id: &str,
    desc: &mut ProviderSubagentDescriptor,
    sub_mon: &mut AgyTranscriptMonitor,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    for wake_ev in sub_mon.poll_post_turn_wakeup() {
        match wake_ev {
            WakeupStep::Thinking { id, text } => {
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentTimeline {
                        subagent_id: sub_id.to_string(),
                        payload: TimelinePayload::AssistantThinking { item_id: id, text },
                    })
                    .await;
            }
            WakeupStep::Message { id, text } => {
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentTimeline {
                        subagent_id: sub_id.to_string(),
                        payload: TimelinePayload::AssistantMessage {
                            item_id: id,
                            text,
                            streaming: false,
                        },
                    })
                    .await;
            }
            WakeupStep::Tool {
                call_id,
                status,
                detail,
            } => {
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentTimeline {
                        subagent_id: sub_id.to_string(),
                        payload: TimelinePayload::ToolCall {
                            call_id,
                            status,
                            detail,
                            background_task: None,
                        },
                    })
                    .await;
            }
            WakeupStep::Settled => {
                desc.status = ProviderSubagentStatus::Completed;
                desc.updated_at = Utc::now();
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
                    .await;
            }
            WakeupStep::Failed { message } => {
                desc.status = ProviderSubagentStatus::Failed;
                desc.subtitle = Some(message);
                desc.updated_at = Utc::now();
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
                    .await;
            }
            WakeupStep::Notice { level, message } => {
                let _ = evt_tx
                    .send(DriverEvent::ProviderSubagentTimeline {
                        subagent_id: sub_id.to_string(),
                        payload: TimelinePayload::Notice { level, message },
                    })
                    .await;
            }
            WakeupStep::Started { .. }
            | WakeupStep::BackgroundTaskStarted { .. }
            | WakeupStep::BackgroundTaskCompleted { .. }
            | WakeupStep::SubagentSpawned { .. } => {}
        }
    }
}

async fn register_agy_subagent_monitor(
    parent_agent_id: &str,
    child_conv_id: &str,
    role: &str,
    prompt_preview: &str,
    is_historical: bool,
    subagent_monitors: &mut HashMap<String, (ProviderSubagentDescriptor, AgyTranscriptMonitor)>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let clean_id = child_conv_id.trim();
    if clean_id.is_empty() || subagent_monitors.contains_key(clean_id) {
        return;
    }
    let now = Utc::now();
    let title = if role.trim().is_empty() || role.trim().eq_ignore_ascii_case("subagent") {
        "Subagent".to_string()
    } else {
        role.trim().to_string()
    };
    let subtitle = if !prompt_preview.trim().is_empty() {
        Some(truncate_preview(prompt_preview.trim(), 240))
    } else {
        Some("Antigravity".into())
    };
    let mut desc = ProviderSubagentDescriptor {
        subagent_id: clean_id.to_string(),
        parent_agent_id: parent_agent_id.to_string(),
        parent_subagent_id: None,
        title,
        subtitle,
        status: ProviderSubagentStatus::Running,
        created_at: now,
        updated_at: now,
    };
    let mut sub_mon = AgyTranscriptMonitor::new_subagent(clean_id);

    if is_historical {
        if sub_mon.active_path().is_none() {
            // Historical subagent whose transcript is no longer on disk: mark completed so
            // reattaching never leaves a stale permanent spinner.
            desc.status = ProviderSubagentStatus::Completed;
            let _ = evt_tx
                .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
                .await;
            subagent_monitors.insert(clean_id.to_string(), (desc, sub_mon));
            return;
        }
        // Poll historical transcript first so already-settled subagents emit their final status
        // immediately without flashing `Running`.
        let initial_steps = sub_mon.poll_post_turn_wakeup();
        let mut initial_timeline = Vec::new();
        for wake_ev in initial_steps {
            match wake_ev {
                WakeupStep::Thinking { id, text } => {
                    initial_timeline.push(TimelinePayload::AssistantThinking { item_id: id, text });
                }
                WakeupStep::Message { id, text } => {
                    initial_timeline.push(TimelinePayload::AssistantMessage {
                        item_id: id,
                        text,
                        streaming: false,
                    });
                }
                WakeupStep::Tool {
                    call_id,
                    status,
                    detail,
                } => {
                    initial_timeline.push(TimelinePayload::ToolCall {
                        call_id,
                        status,
                        detail,
                        background_task: None,
                    });
                }
                WakeupStep::Notice { level, message } => {
                    initial_timeline.push(TimelinePayload::Notice { level, message });
                }
                WakeupStep::Settled => {
                    desc.status = ProviderSubagentStatus::Completed;
                    desc.updated_at = Utc::now();
                }
                WakeupStep::Failed { message } => {
                    desc.status = ProviderSubagentStatus::Failed;
                    desc.subtitle = Some(message);
                    desc.updated_at = Utc::now();
                }
                WakeupStep::Started { .. }
                | WakeupStep::BackgroundTaskStarted { .. }
                | WakeupStep::BackgroundTaskCompleted { .. }
                | WakeupStep::SubagentSpawned { .. } => {}
            }
        }
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
            .await;
        for payload in initial_timeline {
            let _ = evt_tx
                .send(DriverEvent::ProviderSubagentTimeline {
                    subagent_id: clean_id.to_string(),
                    payload,
                })
                .await;
        }
        subagent_monitors.insert(clean_id.to_string(), (desc, sub_mon));
        return;
    }

    let _ = evt_tx
        .send(DriverEvent::ProviderSubagentUpsert(desc.clone()))
        .await;
    poll_agy_subagent_monitor(clean_id, &mut desc, &mut sub_mon, evt_tx).await;
    subagent_monitors.insert(clean_id.to_string(), (desc, sub_mon));
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

    let prev_subagent = match prev {
        Some(ToolCallDetail::Subagent {
            child_agent_id,
            role,
            prompt_preview,
        }) => Some((child_agent_id.clone(), role.clone(), prompt_preview.clone())),
        _ => None,
    };

    if is_subagent_launch_tool(title, raw_in) || prev_subagent.is_some() {
        let (prev_child_id, prev_role, prev_prompt) =
            prev_subagent.unwrap_or((None, String::new(), String::new()));
        let role = raw_in
            .and_then(|i| {
                i.get("Role")
                    .or_else(|| i.get("TypeName"))
                    .or_else(|| i.get("role"))
            })
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| (!prev_role.trim().is_empty()).then_some(prev_role))
            .unwrap_or_else(|| "Subagent".to_string());
        let prompt = raw_in
            .and_then(|i| i.get("Prompt").or_else(|| i.get("prompt")))
            .and_then(|v| v.as_str())
            .map(|s| truncate_preview(s, 240))
            .filter(|s| !s.is_empty())
            .unwrap_or(prev_prompt);
        let child_id = extract_subagent_conversation_id(raw_out, None).or(prev_child_id);
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

// --- Post-turn `transcript_full.jsonl` incremental reader, unified tool reconciliation & self-healing ---

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
    BackgroundTaskStarted {
        task_id: String,
        call_id: Option<String>,
        description: Option<String>,
        log_path: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    BackgroundTaskCompleted {
        task_id: String,
        canceled: bool,
    },
    SubagentSpawned {
        subagent_id: String,
        role: String,
        prompt_preview: String,
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
const SELF_HEAL_GRACE_PERIOD: Duration = Duration::from_secs(4);
const MAX_CONSECUTIVE_SELF_HEALS: u32 = 3;
const IN_PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const ONE_SHOT_TIMER_EXPIRY_GRACE_SECS: i64 = 90;

fn is_one_shot_timer_expired(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    expires_at
        .is_some_and(|exp| now > exp + chrono::Duration::seconds(ONE_SHOT_TIMER_EXPIRY_GRACE_SECS))
}

#[derive(Debug, Clone)]
struct BgTaskInfo {
    task_id: String,
    call_id: Option<String>,
    log_path: Option<String>,
    meta: Option<BackgroundTaskInfo>,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
enum BgTaskDelta {
    Started {
        task_id: String,
        call_id: Option<String>,
        description: Option<String>,
        log_path: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    Completed {
        task_id: String,
        canceled: bool,
    },
    SubagentSpawned {
        subagent_id: String,
        role: String,
        prompt_preview: String,
    },
}

#[derive(Debug, Default)]
struct BackgroundTaskTracker {
    tasks: HashMap<String, BgTaskInfo>,
    finished_tasks: HashSet<String>,
}

impl BackgroundTaskTracker {
    fn record_started(
        &mut self,
        task_id: String,
        call_id: Option<String>,
        log_path: Option<String>,
        meta: Option<BackgroundTaskInfo>,
    ) {
        self.record_started_with_expiry(task_id, call_id, log_path, meta, None);
    }

    fn record_started_with_expiry(
        &mut self,
        task_id: String,
        call_id: Option<String>,
        log_path: Option<String>,
        meta: Option<BackgroundTaskInfo>,
        expires_at: Option<DateTime<Utc>>,
    ) {
        if self.finished_tasks.contains(&task_id) {
            return;
        }
        let entry = self
            .tasks
            .entry(task_id.clone())
            .or_insert_with(|| BgTaskInfo {
                task_id,
                call_id: None,
                log_path: None,
                meta: None,
                expires_at: None,
            });
        if call_id.is_some() {
            entry.call_id = call_id;
        }
        if log_path.is_some() {
            entry.log_path = log_path;
        }
        if meta.is_some() {
            entry.meta = meta;
        }
        if expires_at.is_some() {
            entry.expires_at = expires_at;
        }
    }

    fn reap_expired_timers(&mut self, now: DateTime<Utc>) -> Vec<BgTaskInfo> {
        let expired_ids: Vec<String> = self
            .tasks
            .iter()
            .filter(|(_, info)| is_one_shot_timer_expired(info.expires_at, now))
            .map(|(k, _)| k.clone())
            .collect();
        let mut out = Vec::with_capacity(expired_ids.len());
        for tid in expired_ids {
            self.finished_tasks.insert(tid.clone());
            if let Some(info) = self.tasks.remove(&tid) {
                out.push(info);
            }
        }
        out
    }

    fn mark_finished(&mut self, task_id: String) {
        self.tasks.remove(&task_id);
        self.finished_tasks.insert(task_id);
    }

    fn is_finished(&self, task_id: &str) -> bool {
        self.finished_tasks.contains(task_id)
    }

    fn call_id_for(&self, task_id: &str) -> Option<&str> {
        self.tasks.get(task_id).and_then(|t| t.call_id.as_deref())
    }

    fn remove(&mut self, task_id: &str) -> Option<BgTaskInfo> {
        self.finished_tasks.insert(task_id.to_string());
        self.tasks.remove(task_id)
    }

    fn remove_by_call_id(&mut self, call_id: &str) -> Option<BgTaskInfo> {
        let key = self
            .tasks
            .iter()
            .find(|(_, info)| info.call_id.as_deref() == Some(call_id))
            .map(|(k, _)| k.clone())?;
        self.finished_tasks.insert(key.clone());
        self.tasks.remove(&key)
    }

    fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    fn clear(&mut self) {
        self.tasks.clear();
    }

    fn active_call_ids(&self) -> HashSet<String> {
        self.tasks
            .values()
            .filter_map(|t| t.call_id.clone())
            .collect()
    }

    fn waiting_source_label(&self) -> String {
        let n = self.tasks.len();
        if n == 1 {
            "waiting:1 background task".to_string()
        } else {
            format!("waiting:{n} background tasks")
        }
    }
}

#[derive(Debug, Clone)]
struct AcpSignalCancelEvent {
    task_id: String,
    reason: String,
    log_path: Option<String>,
    output_tail: Option<String>,
}

#[derive(Debug, Clone)]
struct UndeliveredCancelEvent {
    task_id: String,
    reason: String,
    log_path: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingSelfHeal {
    task_id: String,
    reason: String,
    log_path: Option<String>,
    output_tail: Option<String>,
    detected_at: Instant,
    transcript_step_at_detect: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ToolSignature {
    kind: &'static str,
    target: String,
}

fn normalize_tool_signature(detail: &ToolCallDetail) -> ToolSignature {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    match detail {
        ToolCallDetail::Shell { command, .. } => ToolSignature {
            kind: "shell",
            target: norm(command),
        },
        ToolCallDetail::FileRead { path, .. } => ToolSignature {
            kind: "read",
            target: path.trim().to_string(),
        },
        ToolCallDetail::FileEdit { path, .. } => ToolSignature {
            kind: "edit",
            target: path.trim().to_string(),
        },
        ToolCallDetail::Search { query, .. } => ToolSignature {
            kind: "search",
            target: norm(query),
        },
        ToolCallDetail::Subagent { role, .. } => ToolSignature {
            kind: "subagent",
            target: role.trim().to_ascii_lowercase(),
        },
        ToolCallDetail::Generic { tool_name, .. } => {
            let lower = tool_name.trim().to_ascii_lowercase();
            let canonical = lower
                .strip_prefix("run ")
                .unwrap_or(&lower)
                .trim()
                .to_string();
            ToolSignature {
                kind: "generic",
                target: canonical,
            }
        }
    }
}

fn merge_tool_details(existing: &ToolCallDetail, incoming: ToolCallDetail) -> ToolCallDetail {
    match (existing, incoming) {
        (
            ToolCallDetail::Shell {
                command: old_cmd,
                cwd: old_cwd,
                exit_code: old_code,
                output_preview: old_out,
            },
            ToolCallDetail::Shell {
                command: new_cmd,
                cwd: new_cwd,
                exit_code: new_code,
                output_preview: new_out,
            },
        ) => ToolCallDetail::Shell {
            command: if new_cmd.trim().is_empty() || new_cmd == "tool" {
                old_cmd.clone()
            } else {
                new_cmd
            },
            cwd: new_cwd.or_else(|| old_cwd.clone()),
            exit_code: new_code.or(*old_code),
            output_preview: new_out.or_else(|| old_out.clone()),
        },
        (
            ToolCallDetail::Subagent {
                child_agent_id: old_child,
                role: old_role,
                prompt_preview: old_prompt,
            },
            ToolCallDetail::Subagent {
                child_agent_id: new_child,
                role: new_role,
                prompt_preview: new_prompt,
            },
        ) => ToolCallDetail::Subagent {
            child_agent_id: new_child.or_else(|| old_child.clone()),
            role: if new_role.trim().is_empty() || new_role.trim().eq_ignore_ascii_case("subagent")
            {
                old_role.clone()
            } else {
                new_role
            },
            prompt_preview: if new_prompt.trim().is_empty() {
                old_prompt.clone()
            } else {
                new_prompt
            },
        },
        (_, other) => other,
    }
}

#[derive(Debug, Clone)]
struct WakeupToolEntry {
    call_id: String,
    signature: ToolSignature,
    detail: ToolCallDetail,
    status: ToolCallStatus,
    has_bg_task: bool,
    bg_info: Option<BackgroundTaskInfo>,
    in_wakeup_acp: bool,
    claimed_by_wake: bool,
    claimed_by_acp: bool,
    turn_gen: u64,
}

#[derive(Debug)]
enum AcpToolResolution {
    SkipDuplicate,
    Emit {
        call_id: String,
        detail: ToolCallDetail,
        background_task: Option<BackgroundTaskInfo>,
    },
}

#[derive(Debug)]
enum WakeupToolResolution {
    SkipDuplicate,
    Emit {
        call_id: String,
        status: ToolCallStatus,
        detail: ToolCallDetail,
        background_task: Option<BackgroundTaskInfo>,
    },
}

/// Bidirectionally reconciles synthetic `wake_tool_{step}_{idx}` IDs from `transcript_full.jsonl`
/// with native ACP `call_*` IDs (whichever arrives first) so tool updates always converge onto one
/// canonical card and never leave zombie `running` items or duplicate background-task entries.
#[derive(Debug, Default)]
struct WakeupToolRegistry {
    entries: HashMap<String, WakeupToolEntry>,
    order: Vec<String>,
    acp_to_canonical: HashMap<String, String>,
    wake_to_canonical: HashMap<String, String>,
    current_turn_gen: u64,
}

impl WakeupToolRegistry {
    fn begin_turn(&mut self) {
        self.current_turn_gen = self.current_turn_gen.saturating_add(1);
    }

    fn canonical_id_for_wakeup<'a>(&'a self, wake_call_id: &'a str) -> &'a str {
        self.wake_to_canonical
            .get(wake_call_id)
            .map(String::as_str)
            .unwrap_or(wake_call_id)
    }

    fn resolve_wakeup_tool(
        &mut self,
        wake_call_id: &str,
        status: ToolCallStatus,
        incoming_detail: ToolCallDetail,
    ) -> WakeupToolResolution {
        let incoming_sig = normalize_tool_signature(&incoming_detail);

        // 1. Already mapped to an existing canonical (e.g. ACP `call_*`) entry
        if let Some(canon_id) = self.wake_to_canonical.get(wake_call_id).cloned()
            && let Some(entry) = self.entries.get_mut(&canon_id)
        {
            return Self::apply_wakeup_update(entry, status, incoming_detail, incoming_sig);
        }

        // 2. Direct ID match
        if let Some(entry) = self.entries.get_mut(wake_call_id) {
            return Self::apply_wakeup_update(entry, status, incoming_detail, incoming_sig);
        }

        // 3. Reverse-reconcile synthetic `wake_tool_*` against an already-emitted ACP `call_*`
        //    from the current turn (FIFO scan so parallel identical calls map cleanly 1-to-1).
        if wake_call_id.starts_with("wake_tool_") {
            let current_gen = self.current_turn_gen;
            let matched_id = self
                .order
                .iter()
                .find(|cid| {
                    self.entries.get(*cid).is_some_and(|e| {
                        !e.call_id.starts_with("wake_tool_")
                            && !e.claimed_by_wake
                            && e.turn_gen == current_gen
                            && (e.status == ToolCallStatus::Running
                                || e.has_bg_task
                                || e.in_wakeup_acp)
                            && signatures_match(&e.signature, &incoming_sig)
                    })
                })
                .cloned();

            if let Some(canon_id) = matched_id
                && let Some(entry) = self.entries.get_mut(&canon_id)
            {
                entry.claimed_by_wake = true;
                self.wake_to_canonical
                    .insert(wake_call_id.to_string(), canon_id);
                return Self::apply_wakeup_update(entry, status, incoming_detail, incoming_sig);
            }
        }

        // 4. First time seeing this wakeup tool -> register as canonical
        self.order.push(wake_call_id.to_string());
        self.entries.insert(
            wake_call_id.to_string(),
            WakeupToolEntry {
                call_id: wake_call_id.to_string(),
                signature: incoming_sig,
                detail: incoming_detail.clone(),
                status,
                has_bg_task: false,
                bg_info: None,
                in_wakeup_acp: false,
                claimed_by_wake: true,
                claimed_by_acp: false,
                turn_gen: self.current_turn_gen,
            },
        );
        WakeupToolResolution::Emit {
            call_id: wake_call_id.to_string(),
            status,
            detail: incoming_detail,
            background_task: None,
        }
    }

    fn apply_wakeup_update(
        entry: &mut WakeupToolEntry,
        incoming_status: ToolCallStatus,
        incoming_detail: ToolCallDetail,
        incoming_sig: ToolSignature,
    ) -> WakeupToolResolution {
        // Never regress an already-settled (Completed/Failed) non-background entry back to Running
        if entry.status != ToolCallStatus::Running
            && incoming_status == ToolCallStatus::Running
            && !entry.has_bg_task
        {
            return WakeupToolResolution::SkipDuplicate;
        }
        let had_exit_or_out = tool_detail_has_result(&entry.detail);
        let merged = merge_tool_details(&entry.detail, incoming_detail);
        let now_has_exit_or_out = tool_detail_has_result(&merged);
        let same_status = entry.status == incoming_status;
        entry.detail = merged.clone();
        entry.status = incoming_status;
        if !incoming_sig.target.is_empty() {
            entry.signature = incoming_sig;
        }
        if same_status
            && incoming_status == ToolCallStatus::Running
            && !entry.has_bg_task
            && had_exit_or_out == now_has_exit_or_out
        {
            return WakeupToolResolution::SkipDuplicate;
        }
        WakeupToolResolution::Emit {
            call_id: entry.call_id.clone(),
            status: entry.status,
            detail: merged,
            background_task: entry.bg_info.clone(),
        }
    }

    fn register_synthetic_bg_task(
        &mut self,
        call_id: &str,
        detail: ToolCallDetail,
        bg_info: BackgroundTaskInfo,
    ) {
        let sig = normalize_tool_signature(&detail);
        if let Some(entry) = self.entries.get_mut(call_id) {
            entry.has_bg_task = true;
            entry.bg_info = Some(bg_info);
            entry.status = ToolCallStatus::Running;
            entry.detail = merge_tool_details(&entry.detail, detail);
        } else {
            self.order.push(call_id.to_string());
            self.entries.insert(
                call_id.to_string(),
                WakeupToolEntry {
                    call_id: call_id.to_string(),
                    signature: sig,
                    detail,
                    status: ToolCallStatus::Running,
                    has_bg_task: true,
                    bg_info: Some(bg_info),
                    in_wakeup_acp: false,
                    claimed_by_wake: true,
                    claimed_by_acp: false,
                    turn_gen: self.current_turn_gen,
                },
            );
        }
    }

    /// Marks `call_id` as an active background task. Returns `(detail, merged_bg_info, changed)`
    /// where `changed` is `false` when the entry was already `Running` with identical metadata,
    /// preventing redundant post-reply re-emissions that would move the tool card below the reply.
    fn mark_background_task(
        &mut self,
        call_id: &str,
        info: BackgroundTaskInfo,
    ) -> Option<(ToolCallDetail, BackgroundTaskInfo, bool)> {
        let canon = self.canonical_id_for_wakeup(call_id).to_string();
        let entry = self.entries.get_mut(&canon)?;
        let merged = merge_bg_task_info(entry.bg_info.as_ref(), info);
        let changed = !entry.has_bg_task
            || entry.status != ToolCallStatus::Running
            || entry.bg_info.as_ref() != Some(&merged);
        entry.has_bg_task = true;
        entry.status = ToolCallStatus::Running;
        entry.bg_info = Some(merged.clone());
        Some((entry.detail.clone(), merged, changed))
    }

    fn find_matching_call_id_for_bg_task(&self, description: Option<&str>) -> Option<String> {
        let current_gen = self.current_turn_gen;
        let is_unbound_in_turn = |e: &WakeupToolEntry| {
            !e.has_bg_task && e.bg_info.is_none() && e.turn_gen == current_gen
        };

        if let Some(desc) = description.map(str::trim).filter(|s| !s.is_empty()) {
            let norm_desc = desc.split_whitespace().collect::<Vec<_>>().join(" ");
            let lower_desc = norm_desc.to_ascii_lowercase();
            // Match unbound `schedule` tool calls (Timer / Cron) from this turn even if ACP already
            // emitted a dispatch-ack `completed` status before `transcript_full.jsonl` exposed the task ID.
            // Never fall through to shell-matching for timers/crons, otherwise a parallel `run_command`
            // shell gets hijacked with the timer's `task_id` and label!
            if lower_desc.starts_with("timer:") || lower_desc.starts_with("cron:") {
                return self
                    .order
                    .iter()
                    .rev()
                    .find(|cid| {
                        self.entries.get(*cid).is_some_and(|e| {
                            is_unbound_in_turn(e)
                                && e.signature.kind == "generic"
                                && e.signature.target == "schedule"
                        })
                    })
                    .cloned();
            }
            let desc_sig = ToolSignature {
                kind: "shell",
                target: norm_desc.clone(),
            };
            let shell_matches = |e: &WakeupToolEntry| {
                e.signature.kind == "shell"
                    && (signatures_match(&e.signature, &desc_sig)
                        || (!e.signature.target.is_empty()
                            && (norm_desc.contains(&e.signature.target)
                                || e.signature.target.contains(&norm_desc))))
            };
            // FIFO scan across unbound running shells first so parallel commands sharing a common
            // prefix (e.g. `set -a && source runtime.env && ...`) map cleanly 1-to-1 without overwriting.
            if let Some(cid) = self.order.iter().find(|cid| {
                self.entries.get(*cid).is_some_and(|e| {
                    is_unbound_in_turn(e) && e.status == ToolCallStatus::Running && shell_matches(e)
                })
            }) {
                return Some(cid.clone());
            }
            // Fallback: unbound shell in the current turn whose ACP update already emitted a
            // dispatch-ack `completed` status without `Tool is running as a background task`.
            if let Some(cid) = self.order.iter().find(|cid| {
                self.entries
                    .get(*cid)
                    .is_some_and(|e| is_unbound_in_turn(e) && shell_matches(e))
            }) {
                return Some(cid.clone());
            }
        }
        // Fallback: if there is exactly one currently-running unbound shell tool in this turn, bind to it
        let open_shells: Vec<&String> = self
            .order
            .iter()
            .filter(|cid| {
                self.entries.get(*cid).is_some_and(|e| {
                    is_unbound_in_turn(e)
                        && e.status == ToolCallStatus::Running
                        && e.signature.kind == "shell"
                })
            })
            .collect();
        if open_shells.len() == 1 {
            return Some(open_shells[0].clone());
        }
        None
    }

    fn is_active_bg_task_entry(
        &self,
        acp_call_id: &str,
        incoming_detail: &ToolCallDetail,
        bg_tracker: &BackgroundTaskTracker,
    ) -> bool {
        let entry_is_live_bg = |e: &WakeupToolEntry| {
            e.has_bg_task
                && e.bg_info
                    .as_ref()
                    .and_then(|bg| bg.task_id.as_deref())
                    .is_some_and(|tid| bg_tracker.tasks.contains_key(tid))
        };
        if let Some(canon_id) = self.lookup_canonical_id(acp_call_id)
            && let Some(entry) = self.entries.get(canon_id)
        {
            return entry_is_live_bg(entry);
        }
        let incoming_sig = normalize_tool_signature(incoming_detail);
        self.order.iter().rev().any(|cid| {
            self.entries.get(cid).is_some_and(|e| {
                e.call_id.starts_with("wake_tool_")
                    && entry_is_live_bg(e)
                    && signatures_match(&e.signature, &incoming_sig)
            })
        })
    }

    fn finish_tool(
        &mut self,
        call_id: &str,
        final_status: ToolCallStatus,
    ) -> Option<(ToolCallDetail, Option<BackgroundTaskInfo>)> {
        let canon = self.canonical_id_for_wakeup(call_id).to_string();
        let entry = self.entries.get_mut(&canon)?;
        if entry.status != ToolCallStatus::Running {
            return None;
        }
        entry.status = final_status;
        entry.has_bg_task = false;
        Some((entry.detail.clone(), entry.bg_info.clone()))
    }

    fn lookup_canonical_id(&self, acp_call_id: &str) -> Option<&str> {
        self.acp_to_canonical
            .get(acp_call_id)
            .map(String::as_str)
            .or_else(|| self.entries.get(acp_call_id).map(|e| e.call_id.as_str()))
    }

    fn resolve_acp_tool(
        &mut self,
        acp_call_id: &str,
        status: ToolCallStatus,
        incoming_detail: ToolCallDetail,
        incoming_bg: Option<BackgroundTaskInfo>,
        in_post_turn_wakeup: bool,
    ) -> AcpToolResolution {
        // 1. Already mapped from an earlier ACP update to a canonical wakeup tool ID
        if let Some(canon_id) = self.acp_to_canonical.get(acp_call_id).cloned()
            && let Some(entry) = self.entries.get_mut(&canon_id)
        {
            return Self::apply_acp_update(entry, status, incoming_detail, incoming_bg);
        }

        // 2. Direct ID match (ACP `call_*` already registered when it started, or transcript recorded native call ID)
        if let Some(entry) = self.entries.get_mut(acp_call_id) {
            entry.claimed_by_acp = true;
            if in_post_turn_wakeup {
                entry.in_wakeup_acp = true;
            }
            return Self::apply_acp_update(entry, status, incoming_detail, incoming_bg);
        }

        // 3. Reconcile ACP native `call_*` against an unclaimed, currently-running `wake_tool_*`
        //    by signature. Restrict matching to the current turn (except when a genuinely running
        //    background shell from an earlier turn reports its terminal Completed/Failed update),
        //    so a new turn's `Run schedule` can never hijack an old zombie `wake_tool_*` timer!
        let incoming_sig = normalize_tool_signature(&incoming_detail);
        let current_gen = self.current_turn_gen;
        let is_terminal = matches!(status, ToolCallStatus::Completed | ToolCallStatus::Failed);
        let matched_id = self
            .order
            .iter()
            .rev()
            .find(|cid| {
                self.entries.get(*cid).is_some_and(|e| {
                    let turn_ok = e.turn_gen == current_gen
                        || (is_terminal && e.has_bg_task && incoming_sig.kind == "shell");
                    e.call_id.starts_with("wake_tool_")
                        && !e.claimed_by_acp
                        && e.status == ToolCallStatus::Running
                        && turn_ok
                        && signatures_match(&e.signature, &incoming_sig)
                })
            })
            .cloned()
            .or_else(|| {
                // Also match a single open unclaimed background-task shell `wake_tool_*` when
                // a terminal ACP update omits command text (never hijack `schedule`/generic timers).
                if is_terminal && incoming_sig.kind == "shell" {
                    let open_bg: Vec<&String> = self
                        .order
                        .iter()
                        .filter(|cid| {
                            self.entries.get(*cid).is_some_and(|e| {
                                e.call_id.starts_with("wake_tool_")
                                    && !e.claimed_by_acp
                                    && e.status == ToolCallStatus::Running
                                    && e.has_bg_task
                                    && e.signature.kind == "shell"
                            })
                        })
                        .collect();
                    if open_bg.len() == 1 {
                        return Some(open_bg[0].clone());
                    }
                }
                None
            });

        if let Some(canon_id) = matched_id
            && let Some(entry) = self.entries.get_mut(&canon_id)
        {
            entry.claimed_by_acp = true;
            self.acp_to_canonical
                .insert(acp_call_id.to_string(), canon_id);
            return Self::apply_acp_update(entry, status, incoming_detail, incoming_bg);
        }

        // 4. First time seeing this ACP `call_*`: register it as canonical so subsequent
        //    ACP updates AND delayed `transcript_full.jsonl` `wake_tool_*` steps converge onto it!
        let has_bg = incoming_bg.is_some();
        self.order.push(acp_call_id.to_string());
        self.entries.insert(
            acp_call_id.to_string(),
            WakeupToolEntry {
                call_id: acp_call_id.to_string(),
                signature: incoming_sig,
                detail: incoming_detail.clone(),
                status,
                has_bg_task: has_bg,
                bg_info: incoming_bg.clone(),
                in_wakeup_acp: in_post_turn_wakeup,
                claimed_by_wake: false,
                claimed_by_acp: true,
                turn_gen: self.current_turn_gen,
            },
        );
        AcpToolResolution::Emit {
            call_id: acp_call_id.to_string(),
            detail: incoming_detail,
            background_task: incoming_bg,
        }
    }

    fn apply_acp_update(
        entry: &mut WakeupToolEntry,
        status: ToolCallStatus,
        incoming_detail: ToolCallDetail,
        incoming_bg: Option<BackgroundTaskInfo>,
    ) -> AcpToolResolution {
        let bg_newly_attached = if let Some(bg) = incoming_bg {
            let prev = entry.bg_info.clone();
            let merged_bg = merge_bg_task_info(prev.as_ref(), bg);
            let changed = prev.as_ref() != Some(&merged_bg);
            entry.has_bg_task = true;
            entry.bg_info = Some(merged_bg);
            changed
        } else {
            false
        };
        let merged = merge_tool_details(&entry.detail, incoming_detail);
        let same_terminal =
            entry.status == status && status != ToolCallStatus::Running && !bg_newly_attached;
        entry.detail = merged.clone();
        entry.status = status;
        if status != ToolCallStatus::Running {
            entry.has_bg_task = false;
        }
        if same_terminal {
            return AcpToolResolution::SkipDuplicate;
        }
        AcpToolResolution::Emit {
            call_id: entry.call_id.clone(),
            detail: merged,
            background_task: entry.bg_info.clone(),
        }
    }

    /// Close all still-open entries (both `wake_tool_*` and ACP `call_*`) except those tied to active background tasks.
    fn settle_open_tools(
        &mut self,
        active_bg_call_ids: &HashSet<String>,
    ) -> Vec<(String, ToolCallDetail, Option<BackgroundTaskInfo>)> {
        let mut closed = Vec::new();
        for cid in &self.order {
            if active_bg_call_ids.contains(cid) {
                continue;
            }
            if let Some(entry) = self.entries.get_mut(cid)
                && entry.status == ToolCallStatus::Running
            {
                entry.status = ToolCallStatus::Completed;
                entry.has_bg_task = false;
                closed.push((
                    entry.call_id.clone(),
                    entry.detail.clone(),
                    entry.bg_info.clone(),
                ));
            }
        }
        closed
    }
}

fn tool_detail_has_result(detail: &ToolCallDetail) -> bool {
    match detail {
        ToolCallDetail::Shell {
            exit_code,
            output_preview,
            ..
        } => exit_code.is_some() || output_preview.as_ref().is_some_and(|s| !s.is_empty()),
        _ => false,
    }
}

fn merge_bg_task_info(
    prev: Option<&BackgroundTaskInfo>,
    incoming: BackgroundTaskInfo,
) -> BackgroundTaskInfo {
    match prev {
        Some(p) => BackgroundTaskInfo {
            task_id: incoming.task_id.or_else(|| p.task_id.clone()),
            label: incoming.label.or_else(|| p.label.clone()),
            log_path: incoming.log_path.or_else(|| p.log_path.clone()),
        },
        None => incoming,
    }
}

fn sanitize_bg_task_id(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn signatures_match(a: &ToolSignature, b: &ToolSignature) -> bool {
    if a.kind == b.kind && !a.target.is_empty() && a.target == b.target {
        return true;
    }
    if a.kind == "shell" && b.kind == "shell" && !a.target.is_empty() && !b.target.is_empty() {
        return a.target.contains(&b.target) || b.target.contains(&a.target);
    }
    false
}

fn extract_acp_raw_output_text(raw_out: Option<&Value>) -> Option<String> {
    let o = raw_out?;
    if let Some(s) = o.as_str() {
        return Some(s.to_string());
    }
    o.get("output")
        .or_else(|| o.get("content"))
        .or_else(|| o.get("result"))
        .or_else(|| o.get("error"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
}

fn extract_log_path_from_text(text: &str) -> Option<String> {
    for marker in ["Task logs are available at: file://", "Log: file://"] {
        if let Some(idx) = text.find(marker) {
            let rest = &text[idx + marker.len()..];
            let path = rest
                .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
                .next()
                .unwrap_or("")
                .trim_end_matches('.');
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

fn parse_bg_task_started(
    text: &str,
    expected_session_id: &str,
) -> Option<(String, Option<String>, Option<String>)> {
    let expected_session = expected_session_id.trim();
    if expected_session.is_empty() {
        return None;
    }
    const MARKER: &str = "Tool is running as a background task with task id:";

    // Anchor strictly to the top metadata header emitted by `agy` when a tool transitions to a
    // background task: only leading blank lines and `Created At:` lines may precede the marker
    // at column 0. Completed tool outputs (`Completed At:`, `Command exited with code`,
    // `Showing lines `, `Command output:`), source code, or JSON quotes are immediately rejected.
    let mut lines = text.lines().enumerate();
    let mut marker_rest: Option<&str> = None;
    let mut marker_idx: usize = 0;

    for (idx, raw_line) in lines.by_ref() {
        let line = raw_line.trim_end_matches('\r');
        if line.trim().is_empty() {
            if idx >= 4 {
                return None;
            }
            continue;
        }
        if line.starts_with("Created At:") {
            if idx >= 4 {
                return None;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(MARKER) {
            marker_rest = Some(rest);
            marker_idx = idx;
        }
        break;
    }

    let rest = marker_rest?.trim();
    let task_id = rest.trim_end_matches('.');
    if task_id.is_empty()
        || !task_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | ':' | '.'))
    {
        return None;
    }

    // Strict session ownership check: reject foreign session task IDs and test literals.
    let expected_prefix = format!("{expected_session}/");
    if !task_id.starts_with(&expected_prefix) || task_id.len() <= expected_prefix.len() {
        return None;
    }

    // Extract optional metadata headers immediately following the marker, stopping before `Log output:`.
    let mut description: Option<String> = None;
    let mut log_path: Option<String> = None;
    for (idx, raw_line) in lines {
        if idx > marker_idx + 10 {
            break;
        }
        let line = raw_line.trim_end_matches('\r');
        if line.starts_with("Log output:") {
            break;
        }
        if description.is_none()
            && let Some(desc) = line.strip_prefix("Task Description:")
        {
            let trimmed = desc.trim();
            if !trimmed.is_empty() {
                description = Some(truncate_preview(trimmed, 240));
            }
        } else if log_path.is_none() {
            for log_marker in ["Task logs are available at: file://", "Log: file://"] {
                if let Some(lp) = line.strip_prefix(log_marker) {
                    let path = lp
                        .split(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ')')
                        .next()
                        .unwrap_or("")
                        .trim_end_matches('.');
                    if !path.is_empty() {
                        log_path = Some(path.to_string());
                    }
                    break;
                }
            }
        }
    }

    Some((task_id.to_string(), description, log_path))
}

fn is_system_message_step(step_type: &str, content: &str) -> bool {
    if step_type == "SYSTEM_MESSAGE" {
        return true;
    }
    // Never treat tool results (`GENERIC`), planner replies, user inputs, checkpoints, or API
    // errors as system wakeup messages just because their body quotes `<SYSTEM_MESSAGE>`.
    if matches!(
        step_type,
        "GENERIC" | "PLANNER_RESPONSE" | "USER_INPUT" | "CHECKPOINT" | "ERROR_MESSAGE"
    ) {
        return false;
    }
    for raw_line in content.lines().take(4) {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with("Created At:") {
            continue;
        }
        return trimmed.starts_with("<SYSTEM_MESSAGE>");
    }
    false
}

fn parse_bg_task_completed_or_canceled(text: &str) -> Vec<(String, bool, Option<String>)> {
    let mut out = Vec::new();
    let log_path = extract_log_path_from_text(text);
    let mut cursor = text;
    while let Some(idx) = cursor.find("Task id \"") {
        let after = &cursor[idx + "Task id \"".len()..];
        let Some(end_q) = after.find('"') else {
            break;
        };
        let task_id = after[..end_q].trim().to_string();
        let tail = &after[end_q + 1..];
        let trimmed_tail = tail.trim_start();
        if trimmed_tail.starts_with("finished") && !task_id.is_empty() {
            out.push((task_id, false, log_path.clone()));
        } else if (trimmed_tail.starts_with("was canceled")
            || trimmed_tail.starts_with("was cancelled"))
            && !task_id.is_empty()
        {
            out.push((task_id, true, log_path.clone()));
        }
        cursor = tail;
    }
    out
}

/// Extracts `sender=<id>` tokens from Antigravity `[Message]` system lines, e.g.:
/// `[Message] timestamp=2026-11-20T10:18:46Z sender=e17d47cc-.../task-46 priority=MESSAGE_PRIORITY_HIGH content=...`
fn parse_system_message_senders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_start();
        if !line.starts_with("[Message]") {
            continue;
        }
        if let Some(idx) = line.find("sender=") {
            let rest = &line[idx + "sender=".len()..];
            let token = rest
                .split(|c: char| c.is_ascii_whitespace())
                .next()
                .unwrap_or("")
                .trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';'));
            if !token.is_empty()
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | ':' | '.'))
            {
                out.push(token.to_string());
            }
        }
    }
    out
}

/// Distinguishes recurring `Cron:` schedules from one-shot `Timer:` schedules or background shells.
fn is_recurring_bg_task_description(desc: Option<&str>) -> bool {
    let Some(trimmed) = desc.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("cron:") || lower.starts_with("recurring:")
}

fn is_schedule_bg_task_description(desc: Option<&str>) -> bool {
    let Some(trimmed) = desc.map(str::trim).filter(|s| !s.is_empty()) else {
        return false;
    };
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("timer:") || lower.starts_with("cron:") || lower.starts_with("recurring:")
}

/// Parses one-shot timer duration from Antigravity `Task Description:` lines such as
/// `Timer: 8m, Prompt: ...`, `Timer: 90s, ...`, `Timer: 1h30m, ...`.
fn parse_timer_duration_secs(desc: &str) -> Option<u64> {
    let trimmed = desc.trim();
    let lower = trimmed.to_ascii_lowercase();
    let after = lower.strip_prefix("timer:")?.trim_start();
    let token = after
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .next()
        .unwrap_or("")
        .trim();
    if token.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    let mut num: u64 = 0;
    let mut has_digit = false;
    let mut matched_unit = false;
    for ch in token.chars() {
        if let Some(d) = ch.to_digit(10) {
            num = num.saturating_mul(10).saturating_add(u64::from(d));
            has_digit = true;
        } else if has_digit {
            let mult = match ch {
                'h' => 3600,
                'm' => 60,
                's' => 1,
                _ => return None,
            };
            total = total.saturating_add(num.saturating_mul(mult));
            num = 0;
            has_digit = false;
            matched_unit = true;
        } else {
            return None;
        }
    }
    if has_digit && !matched_unit {
        total = num;
        matched_unit = true;
    } else if has_digit {
        return None;
    }
    (matched_unit && total > 0).then_some(total)
}

fn parse_step_created_at(step: &Value, content: &str) -> Option<DateTime<Utc>> {
    if let Some(raw) = step.get("created_at").and_then(|v| v.as_str())
        && let Ok(dt) = DateTime::parse_from_rfc3339(raw.trim())
    {
        return Some(dt.with_timezone(&Utc));
    }
    for line in content.lines().take(4) {
        if let Some(rest) = line.trim_end_matches('\r').strip_prefix("Created At:")
            && let Ok(dt) = DateTime::parse_from_rfc3339(rest.trim())
        {
            return Some(dt.with_timezone(&Utc));
        }
    }
    None
}

fn compute_timer_expires_at(
    is_recurring: bool,
    duration_secs: Option<u64>,
    desc: Option<&str>,
    created_at: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    if is_recurring {
        return None;
    }
    let secs = duration_secs.or_else(|| desc.and_then(parse_timer_duration_secs))?;
    let started = created_at?;
    let secs_i64 = i64::try_from(secs).ok()?;
    started.checked_add_signed(chrono::Duration::seconds(secs_i64))
}

/// Skips leading blank / `Created At:` / `Completed At:` headers on Antigravity `GENERIC` tool outputs.
fn body_lines_after_agy_headers(text: &str) -> Vec<&str> {
    let mut iter = text.lines().peekable();
    let mut skipped = 0usize;
    while let Some(&raw) = iter.peek() {
        let line = raw.trim_end_matches('\r');
        if skipped < 8
            && (line.trim().is_empty()
                || line.starts_with("Created At:")
                || line.starts_with("Completed At:"))
        {
            iter.next();
            skipped += 1;
        } else {
            break;
        }
    }
    iter.map(|l| l.trim_end_matches('\r')).collect()
}

/// Parses `manage_task(Action: "kill")` output:
/// `Task "<session_id>/task-XX" cancelled.` (or `canceled.`).
fn parse_manage_task_cancelled(text: &str, expected_session_id: &str) -> Vec<String> {
    let expected_session = expected_session_id.trim();
    if expected_session.is_empty() {
        return Vec::new();
    }
    let prefix = format!("{expected_session}/");
    let mut out = Vec::new();
    for line in body_lines_after_agy_headers(text).into_iter().take(4) {
        let trimmed = line.trim();
        if let Some(after) = trimmed.strip_prefix("Task \"")
            && let Some(end_q) = after.find('"')
        {
            let tid = after[..end_q].trim();
            let tail = after[end_q + 1..].trim().to_ascii_lowercase();
            if tid.starts_with(&prefix)
                && tid.len() > prefix.len()
                && (tail.starts_with("cancelled")
                    || tail.starts_with("canceled")
                    || tail.starts_with("killed")
                    || tail.starts_with("stopped"))
            {
                out.push(tid.to_string());
            }
        }
    }
    out
}

/// Detects authoritative `manage_task(Action: "list")` empty output.
fn is_no_background_tasks_running_output(text: &str) -> bool {
    let body = body_lines_after_agy_headers(text);
    body.first()
        .is_some_and(|l| l.trim() == "No background tasks are currently running.")
}

/// Parses terminal status from `manage_task(Action: "status")`:
/// `Task: <session_id>/task-XX` followed by `Status: COMPLETED|FAILED|CANCELLED|...`.
fn parse_manage_task_terminal_status(
    text: &str,
    expected_session_id: &str,
) -> Option<(String, bool)> {
    let expected_session = expected_session_id.trim();
    if expected_session.is_empty() {
        return None;
    }
    let prefix = format!("{expected_session}/");
    let body = body_lines_after_agy_headers(text);
    let task_line = body.first()?.trim();
    let tid = task_line.strip_prefix("Task:")?.trim();
    if !tid.starts_with(&prefix) || tid.len() <= prefix.len() {
        return None;
    }
    let status_line = body
        .iter()
        .skip(1)
        .take(3)
        .find_map(|l| l.trim().strip_prefix("Status:"))?
        .trim()
        .to_ascii_uppercase();
    if matches!(status_line.as_str(), "RUNNING" | "PENDING" | "STARTING") {
        return None;
    }
    let canceled_or_failed = matches!(
        status_line.as_str(),
        "FAILED" | "ERROR" | "CANCELLED" | "CANCELED" | "KILLED" | "STOPPED" | "TIMED_OUT"
    );
    Some((tid.to_string(), canceled_or_failed))
}

/// Strictly classifies subagent launch tools (`invoke_subagent` / `spawn_subagent`),
/// explicitly excluding management tools like `manage_subagents` or `define_subagent`.
fn is_subagent_launch_tool(name_or_title: &str, args_or_raw_in: Option<&Value>) -> bool {
    let lower = name_or_title.trim().to_ascii_lowercase();
    let clean = lower.strip_prefix("run ").unwrap_or(&lower).trim();
    if clean.contains("manage_subagent")
        || clean.contains("define_subagent")
        || clean.contains("list_subagent")
        || clean.contains("kill_subagent")
        || clean.contains("send_message")
    {
        return false;
    }
    if clean == "invoke_subagent"
        || clean == "spawn_subagent"
        || clean.starts_with("invoke_subagent ")
        || clean.starts_with("spawn_subagent ")
    {
        return true;
    }
    if let Some(obj) = args_or_raw_in {
        let has_prompt = obj.get("Prompt").or_else(|| obj.get("prompt")).is_some();
        let has_role = obj
            .get("Role")
            .or_else(|| obj.get("TypeName"))
            .or_else(|| obj.get("role"))
            .is_some();
        if has_prompt && has_role {
            return true;
        }
    }
    false
}

/// Extracts `conversationId` from either structured JSON (`rawOutput.conversationId`)
/// or plain text output containing `"conversationId": "<uuid>"` (Antigravity `invoke_subagent` output).
fn extract_subagent_conversation_id(
    raw_out: Option<&Value>,
    text_fallback: Option<&str>,
) -> Option<String> {
    if let Some(o) = raw_out
        && let Some(id) = o
            .get("conversationId")
            .or_else(|| o.get("conversation_id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        return Some(id.to_string());
    }
    let extracted_text = extract_acp_raw_output_text(raw_out);
    let candidate_text = text_fallback.or(extracted_text.as_deref())?;
    for key in ["\"conversationId\"", "\"conversation_id\""] {
        if let Some(pos) = candidate_text.find(key) {
            let after_key = candidate_text[pos + key.len()..].trim_start();
            let Some(after_colon) = after_key.strip_prefix(':') else {
                continue;
            };
            let after_ws = after_colon.trim_start();
            let Some(after_quote) = after_ws.strip_prefix('"') else {
                continue;
            };
            let Some(end_quote) = after_quote.find('"') else {
                continue;
            };
            let id = after_quote[..end_quote].trim();
            if !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
            {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn acp_output_indicates_bg_completion(
    raw_out: Option<&Value>,
    raw_output_text: Option<&str>,
) -> bool {
    if raw_out.and_then(|o| o.get("exitCode")).is_some() {
        return true;
    }
    if let Some(text) = raw_output_text
        && (!parse_bg_task_completed_or_canceled(text).is_empty()
            || text.contains("Command exited with code ")
            || is_signal_termination_text(text))
    {
        return true;
    }
    false
}

fn is_signal_termination_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("command was terminated by signal or canceled")
        || lower.contains("terminated by signal")
        || lower.contains("tool execution was canceled")
        || lower.contains("killed by signal")
        || lower.contains("sigkill")
        || lower.contains("sigterm")
}

fn detect_acp_signal_cancel(
    call_id: &str,
    status: ToolCallStatus,
    raw_output: Option<&str>,
    bg_tracker: &mut BackgroundTaskTracker,
) -> Option<AcpSignalCancelEvent> {
    if status != ToolCallStatus::Failed {
        return None;
    }
    let out_str = raw_output?;
    if !is_signal_termination_text(out_str) {
        return None;
    }
    let tracked = bg_tracker.remove_by_call_id(call_id);
    let task_id = tracked
        .as_ref()
        .map(|t| t.task_id.clone())
        .or_else(|| {
            parse_bg_task_completed_or_canceled(out_str)
                .into_iter()
                .next()
                .map(|(id, _, _)| id)
        })
        .unwrap_or_else(|| call_id.to_string());
    let log_path = extract_log_path_from_text(out_str)
        .or_else(|| tracked.as_ref().and_then(|t| t.log_path.clone()));
    let output_tail = log_path
        .as_deref()
        .and_then(|p| read_log_tail(Path::new(p), 1500))
        .or_else(|| Some(truncate_preview(out_str.trim(), 1200)));
    Some(AcpSignalCancelEvent {
        task_id,
        reason: truncate_preview(out_str.trim(), 300),
        log_path,
        output_tail,
    })
}

fn read_log_tail(path: &Path, max_chars: usize) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        Some(trimmed.to_string())
    } else {
        let tail: String = chars[chars.len() - max_chars..].iter().collect();
        Some(format!("...[truncated]...\n{tail}"))
    }
}

fn build_self_heal_prompt(heal: &PendingSelfHeal, attempt: u32) -> String {
    let mut parts = vec![format!(
        "[Orbit Self-Healing Wakeup #{attempt}/{MAX_CONSECUTIVE_SELF_HEALS}]\n\
         Background task `{}` terminated unexpectedly due to an OS signal (NOT canceled by the user).\n\
         Reason: {}",
        heal.task_id, heal.reason
    )];
    if let Some(ref lp) = heal.log_path {
        parts.push(format!("Task log file: `{lp}`"));
    }
    if let Some(ref tail) = heal.output_tail
        && !tail.trim().is_empty()
    {
        parts.push(format!("Last task output:\n```\n{}\n```", tail.trim()));
    }
    parts.push(
        "Please inspect the output/log above, identify what caused the signal interruption (if applicable), and continue your remaining unfinished work."
            .to_string(),
    );
    parts.join("\n\n")
}

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

    if is_subagent_launch_tool(tool_name, args) {
        let role = transcript_arg_str(args, &["Role", "TypeName", "role"])
            .unwrap_or_else(|| "Subagent".into());
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
        ToolCallDetail::Subagent { child_agent_id, .. } => {
            if child_agent_id.is_none() {
                *child_agent_id = extract_subagent_conversation_id(None, Some(content));
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

#[derive(Debug, Clone)]
enum PendingTranscriptToolMeta {
    Other,
    Subagent {
        role: String,
        prompt_preview: String,
    },
    Schedule {
        is_cron: bool,
        duration_secs: Option<u64>,
        timer_condition: Option<String>,
    },
    ManageTask {
        action: String,
        task_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct ActiveTranscriptBgTask {
    is_recurring: bool,
    timer_condition: Option<String>,
}

struct AgyTranscriptMonitor {
    session_id: String,
    brain_dir: PathBuf,
    full_path: PathBuf,
    compact_path: PathBuf,
    byte_offset: u64,
    seen_steps: HashSet<u64>,
    max_seen_step: u64,
    always_active: bool,
    in_wakeup: bool,
    pending_tools: Vec<(String, ToolCallDetail)>,
    pending_silent_tools: VecDeque<PendingTranscriptToolMeta>,
    active_bg_tasks: HashMap<String, ActiveTranscriptBgTask>,
    consecutive_errors: u32,
    last_error_at: Option<Instant>,
    last_error_message: Option<String>,
    in_progress_thinking_lens: HashMap<u64, usize>,
    last_in_progress_heartbeat: Option<Instant>,
    seen_undelivered_files: HashSet<PathBuf>,
    last_checked_path: Option<PathBuf>,
    last_checked_mtime: Option<std::time::SystemTime>,
}

impl AgyTranscriptMonitor {
    fn new(session_id: &str) -> Self {
        let data_dir = resolve_agy_data_dir();
        let brain_dir = data_dir.join("brain").join(session_id);
        Self::from_brain_dir_with_session(brain_dir, session_id)
    }

    fn new_subagent(session_id: &str) -> Self {
        let data_dir = resolve_agy_data_dir();
        let brain_dir = data_dir.join("brain").join(session_id);
        Self::from_brain_dir_subagent(brain_dir, session_id)
    }

    fn from_brain_dir_with_session(brain_dir: PathBuf, session_id: impl Into<String>) -> Self {
        let logs_dir = brain_dir.join(".system_generated/logs");
        Self {
            session_id: session_id.into(),
            brain_dir,
            full_path: logs_dir.join("transcript_full.jsonl"),
            compact_path: logs_dir.join("transcript.jsonl"),
            byte_offset: 0,
            seen_steps: HashSet::new(),
            max_seen_step: 0,
            always_active: false,
            in_wakeup: false,
            pending_tools: Vec::new(),
            pending_silent_tools: VecDeque::new(),
            active_bg_tasks: HashMap::new(),
            consecutive_errors: 0,
            last_error_at: None,
            last_error_message: None,
            in_progress_thinking_lens: HashMap::new(),
            last_in_progress_heartbeat: None,
            seen_undelivered_files: HashSet::new(),
            last_checked_path: None,
            last_checked_mtime: None,
        }
    }

    fn from_brain_dir_subagent(brain_dir: PathBuf, session_id: impl Into<String>) -> Self {
        let mut mon = Self::from_brain_dir_with_session(brain_dir, session_id);
        mon.always_active = true;
        mon.in_wakeup = true;
        mon
    }

    fn max_seen_step(&self) -> u64 {
        self.max_seen_step
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
        let _ = self.sync_silent_with_bg_tasks();
    }

    /// Reads newly flushed foreground transcript lines for background-task / subagent metadata
    /// mid-turn WITHOUT clearing `pending_silent_tools`.
    fn poll_foreground_bg_deltas(&mut self) -> Vec<BgTaskDelta> {
        let mut bg_deltas = Vec::new();
        let _ = self.read_new_lines(true, &mut bg_deltas);
        self.in_wakeup = self.always_active;
        bg_deltas
    }

    fn sync_silent_with_bg_tasks(&mut self) -> Vec<BgTaskDelta> {
        let mut bg_deltas = Vec::new();
        let _ = self.read_new_lines(true, &mut bg_deltas);
        self.in_wakeup = self.always_active;
        self.pending_tools.clear();
        self.pending_silent_tools.clear();
        self.consecutive_errors = 0;
        self.last_error_at = None;
        self.last_error_message = None;
        self.in_progress_thinking_lens.clear();
        self.last_in_progress_heartbeat = None;
        bg_deltas
    }

    fn poll_post_turn_wakeup(&mut self) -> Vec<WakeupStep> {
        let mut bg_deltas = Vec::new();
        let mut out = self.read_new_lines(false, &mut bg_deltas);
        self.check_error_stall(&mut out, Instant::now());
        out
    }

    /// Scan `<brain_dir>/.system_generated/messages/undelivered/` for `MESSAGE_PRIORITY_LOW`
    /// cancellation notices targeting tracked background tasks (or signal-killed tasks).
    fn poll_undelivered_cancellations(
        &mut self,
        bg_tracker: &BackgroundTaskTracker,
    ) -> Vec<UndeliveredCancelEvent> {
        let dir = self
            .brain_dir
            .join(".system_generated/messages/undelivered");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || self.seen_undelivered_files.contains(&path) {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&path) else {
                continue;
            };
            self.seen_undelivered_files.insert(path);

            let lower = raw.to_ascii_lowercase();
            let is_low_or_hidden =
                raw.contains("MESSAGE_PRIORITY_LOW") || lower.contains("hidefromuser");
            let is_cancel = is_signal_termination_text(&raw)
                || lower.contains("was canceled")
                || lower.contains("was cancelled");
            if !(is_low_or_hidden && is_cancel) {
                continue;
            }

            let parsed_tasks = parse_bg_task_completed_or_canceled(&raw);
            let matched_task_id = parsed_tasks
                .into_iter()
                .find(|(_, canceled, _)| *canceled)
                .map(|(tid, _, lp)| (tid, lp))
                .or_else(|| {
                    bg_tracker
                        .tasks
                        .keys()
                        .find(|tid| raw.contains(tid.as_str()))
                        .map(|tid| (tid.clone(), extract_log_path_from_text(&raw)))
                });
            if let Some((task_id, log_path)) = matched_task_id {
                out.push(UndeliveredCancelEvent {
                    task_id,
                    reason: "Background task terminated by signal or canceled (undelivered low-priority notice)".to_string(),
                    log_path,
                });
            }
        }
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

    fn read_new_lines(
        &mut self,
        silent: bool,
        bg_deltas: &mut Vec<BgTaskDelta>,
    ) -> Vec<WakeupStep> {
        let Some(path) = self.active_path().map(Path::to_path_buf) else {
            return Vec::new();
        };
        let Ok(stat_meta) = std::fs::metadata(&path) else {
            return Vec::new();
        };
        let stat_mtime = stat_meta.modified().ok();
        // Fast path: when completely caught up at EOF and neither file length nor mtime changed,
        // avoid `open`/`seek`/`read` syscalls altogether. Any deferred unfinished `PLANNER_RESPONSE`
        // keeps `self.byte_offset < stat_meta.len()`, so in-place rewrites are never skipped.
        if stat_meta.len() == self.byte_offset
            && self.last_checked_path.as_deref() == Some(path.as_path())
            && self.last_checked_mtime == stat_mtime
        {
            return Vec::new();
        }
        let Ok(mut file) = std::fs::File::open(&path) else {
            return Vec::new();
        };
        let Ok(meta) = file.metadata() else {
            return Vec::new();
        };
        let file_len = meta.len();
        let file_mtime = meta.modified().ok();
        if file_len < self.byte_offset {
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
            if self.byte_offset == file_len {
                self.last_checked_path = Some(path);
                self.last_checked_mtime = file_mtime;
            }
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

            // Defer unfinished `PLANNER_RESPONSE` steps (agy rewrites them in place),
            // BUT stream incremental `thinking` & periodic progress heartbeats so long
            // reasoning/compaction phases never trigger false 45s `waiting for response` stalls.
            if step_type == "PLANNER_RESPONSE"
                && matches!(
                    status.as_str(),
                    "PENDING" | "RUNNING" | "GENERATING" | "WAITING" | "WAITING_FOR_USER"
                )
            {
                if !silent && self.in_wakeup {
                    let th = step
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim();
                    let prev_len = self
                        .in_progress_thinking_lens
                        .get(&step_idx)
                        .copied()
                        .unwrap_or(0);
                    let heartbeat_due = self
                        .last_in_progress_heartbeat
                        .is_none_or(|t| t.elapsed() >= IN_PROGRESS_HEARTBEAT_INTERVAL);
                    if !th.is_empty() && (th.len() > prev_len || heartbeat_due) {
                        self.in_progress_thinking_lens.insert(step_idx, th.len());
                        self.last_in_progress_heartbeat = Some(Instant::now());
                        out.push(WakeupStep::Thinking {
                            id: format!("wake_th_{step_idx}"),
                            text: th.to_string(),
                        });
                    } else if th.is_empty() && heartbeat_due {
                        self.last_in_progress_heartbeat = Some(Instant::now());
                        out.push(WakeupStep::Thinking {
                            id: format!("wake_th_{step_idx}"),
                            text: "Thinking…".to_string(),
                        });
                    }
                }
                break;
            }

            if step_type == "GENERIC"
                && (status == "PENDING"
                    || (silent
                        && status == "RUNNING"
                        && parse_bg_task_started(
                            step.get("content").and_then(|v| v.as_str()).unwrap_or(""),
                            &self.session_id,
                        )
                        .is_none()))
            {
                break;
            }

            consumed_bytes += chunk.len() as u64;
            if self.seen_steps.contains(&step_idx) {
                continue;
            }
            self.seen_steps.insert(step_idx);
            if step_idx != u64::MAX && step_idx > self.max_seen_step {
                self.max_seen_step = step_idx;
            }
            self.in_progress_thinking_lens.remove(&step_idx);

            let content = step.get("content").and_then(|v| v.as_str()).unwrap_or("");

            // Track tool calls emitted by `PLANNER_RESPONSE` even in silent foreground sync mode
            // so subsequent `GENERIC` steps can pair `invoke_subagent`, `schedule` (`TimerCondition`
            // / `DurationSeconds`), and `manage_task` metadata 1-to-1.
            if step_type == "PLANNER_RESPONSE"
                && let Some(calls) = step.get("tool_calls").and_then(|v| v.as_array())
            {
                for c in calls {
                    let tname = c.get("name").and_then(|v| v.as_str()).unwrap_or("tool");
                    let args = c.get("args");
                    if is_subagent_launch_tool(tname, args) {
                        let role = transcript_arg_str(args, &["Role", "TypeName", "role"])
                            .unwrap_or_else(|| "Subagent".into());
                        let prompt = transcript_arg_str(args, &["Prompt", "prompt"])
                            .map(|s| truncate_preview(&s, 240))
                            .unwrap_or_default();
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::Subagent {
                                role,
                                prompt_preview: prompt,
                            });
                    } else if tname.eq_ignore_ascii_case("schedule") {
                        let is_cron =
                            transcript_arg_str(args, &["CronExpression", "cron_expression"])
                                .is_some();
                        let duration_secs =
                            transcript_arg_u32(args, &["DurationSeconds", "duration_seconds"])
                                .map(u64::from);
                        let timer_condition =
                            transcript_arg_str(args, &["TimerCondition", "timer_condition"])
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("never"));
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::Schedule {
                                is_cron,
                                duration_secs,
                                timer_condition,
                            });
                    } else if tname.eq_ignore_ascii_case("manage_task") {
                        let action = transcript_arg_str(args, &["Action", "action"])
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        let task_id = transcript_arg_str(args, &["TaskId", "task_id"]);
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::ManageTask { action, task_id });
                    } else {
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::Other);
                    }
                }
            }

            // Collect completed background tasks from explicit `Task id "..." finished/canceled`
            // messages, one-shot timer `[Message] ... sender=<session_id>/task-XX` notifications,
            // `TimerCondition` early cancellations, and `manage_task` (`kill`/`list`/`status`) results.
            let mut completed_bg_tasks: Vec<(String, bool)> = Vec::new();
            let mut started_bg_expires_at: Option<DateTime<Utc>> = None;

            // Capture background-task & subagent lifecycle even in silent foreground sync mode.
            // Only `GENERIC` steps in `RUNNING` state with strict session-scoped header match
            // count as a real background task start; `DONE` tool outputs (read_file, grep, sqlite)
            // can never enqueue phantom background tasks.
            if step_type == "GENERIC" {
                let pending_meta = self
                    .pending_silent_tools
                    .pop_front()
                    .unwrap_or(PendingTranscriptToolMeta::Other);
                let popped_subagent = match &pending_meta {
                    PendingTranscriptToolMeta::Subagent {
                        role,
                        prompt_preview,
                    } => Some((role.clone(), prompt_preview.clone())),
                    _ => None,
                };
                if (popped_subagent.is_some()
                    || content.contains("Created the following subagent:"))
                    && let Some(sub_id) = extract_subagent_conversation_id(None, Some(content))
                {
                    let (role, prompt_preview) =
                        popped_subagent.unwrap_or_else(|| ("Subagent".to_string(), String::new()));
                    bg_deltas.push(BgTaskDelta::SubagentSpawned {
                        subagent_id: sub_id.clone(),
                        role: role.clone(),
                        prompt_preview: prompt_preview.clone(),
                    });
                    if !silent {
                        out.push(WakeupStep::SubagentSpawned {
                            subagent_id: sub_id,
                            role,
                            prompt_preview,
                        });
                    }
                }
                if status == "RUNNING"
                    && let Some((task_id, description, log_path)) =
                        parse_bg_task_started(content, &self.session_id)
                {
                    let (sched_is_cron, sched_duration_secs, sched_condition) = match &pending_meta
                    {
                        PendingTranscriptToolMeta::Schedule {
                            is_cron,
                            duration_secs,
                            timer_condition,
                        } => (*is_cron, *duration_secs, timer_condition.clone()),
                        _ => (false, None, None),
                    };
                    let is_recurring =
                        sched_is_cron || is_recurring_bg_task_description(description.as_deref());
                    let expires_at = compute_timer_expires_at(
                        is_recurring,
                        sched_duration_secs,
                        description.as_deref(),
                        parse_step_created_at(&step, content),
                    );
                    started_bg_expires_at = expires_at;
                    self.active_bg_tasks.insert(
                        task_id.clone(),
                        ActiveTranscriptBgTask {
                            is_recurring,
                            timer_condition: if is_recurring { None } else { sched_condition },
                        },
                    );
                    bg_deltas.push(BgTaskDelta::Started {
                        task_id,
                        call_id: None,
                        description,
                        log_path,
                        expires_at,
                    });
                } else if status != "RUNNING" {
                    // Authoritative completions/cancellations returned by `manage_task` tools:
                    // 1) `Task "<session_id>/task-XX" cancelled.`
                    for tid in parse_manage_task_cancelled(content, &self.session_id) {
                        self.active_bg_tasks.remove(&tid);
                        if !completed_bg_tasks.iter().any(|(id, _)| id == &tid) {
                            completed_bg_tasks.push((tid, true));
                        }
                    }
                    // 2) `manage_task(Action: "list")` -> `No background tasks are currently running.`
                    if is_no_background_tasks_running_output(content) {
                        for (tid, _) in self.active_bg_tasks.drain() {
                            if !completed_bg_tasks.iter().any(|(id, _)| id == &tid) {
                                completed_bg_tasks.push((tid, false));
                            }
                        }
                    }
                    // 3) `manage_task(Action: "status")` returning a non-RUNNING terminal state
                    if let Some((tid, canceled)) =
                        parse_manage_task_terminal_status(content, &self.session_id)
                    {
                        self.active_bg_tasks.remove(&tid);
                        if !completed_bg_tasks.iter().any(|(id, _)| id == &tid) {
                            completed_bg_tasks.push((tid, canceled));
                        }
                    }
                    // 4) Direct paired `manage_task(Action: "kill", TaskId: ...)` completion fallback
                    if status == "DONE"
                        && let PendingTranscriptToolMeta::ManageTask {
                            action,
                            task_id: Some(tid),
                        } = &pending_meta
                        && action == "kill"
                    {
                        let session_prefix = format!("{}/", self.session_id);
                        if tid.starts_with(&session_prefix) && tid.len() > session_prefix.len() {
                            self.active_bg_tasks.remove(tid);
                            if !completed_bg_tasks.iter().any(|(id, _)| id == tid) {
                                completed_bg_tasks.push((tid.clone(), true));
                            }
                        }
                    }
                    for (task_id, canceled) in &completed_bg_tasks {
                        bg_deltas.push(BgTaskDelta::Completed {
                            task_id: task_id.clone(),
                            canceled: *canceled,
                        });
                        if !silent {
                            out.push(WakeupStep::BackgroundTaskCompleted {
                                task_id: task_id.clone(),
                                canceled: *canceled,
                            });
                        }
                    }
                }
            } else if is_system_message_step(step_type, content) {
                for (task_id, canceled, _) in parse_bg_task_completed_or_canceled(content) {
                    self.active_bg_tasks.remove(&task_id);
                    if !completed_bg_tasks.iter().any(|(id, _)| id == &task_id) {
                        completed_bg_tasks.push((task_id, canceled));
                    }
                }
                let session_prefix = format!("{}/", self.session_id);
                let senders = parse_system_message_senders(content);
                for sender in &senders {
                    if sender.starts_with(&session_prefix) && sender.len() > session_prefix.len() {
                        let is_recurring = self
                            .active_bg_tasks
                            .get(sender)
                            .map(|t| t.is_recurring)
                            .unwrap_or(false);
                        if !is_recurring && !completed_bg_tasks.iter().any(|(id, _)| id == sender) {
                            self.active_bg_tasks.remove(sender);
                            completed_bg_tasks.push((sender.clone(), false));
                        }
                    }
                }
                // Early cancellation of `TimerCondition` timers: when a real `[Message] sender=...`
                // arrives, Antigravity silently cancels matching `TimerCondition: "any"` or
                // `TimerCondition: "<sender>"` timers without emitting a separate cancel notice.
                let mut cond_canceled: Vec<String> = Vec::new();
                for sender in &senders {
                    if sender.eq_ignore_ascii_case("system") {
                        continue;
                    }
                    for (tid, meta) in &self.active_bg_tasks {
                        if tid == sender {
                            continue;
                        }
                        if let Some(ref cond) = meta.timer_condition
                            && (cond.eq_ignore_ascii_case("any") || cond == sender)
                            && !cond_canceled.contains(tid)
                        {
                            cond_canceled.push(tid.clone());
                        }
                    }
                }
                for tid in cond_canceled {
                    self.active_bg_tasks.remove(&tid);
                    if !completed_bg_tasks.iter().any(|(id, _)| id == &tid) {
                        completed_bg_tasks.push((tid, false));
                    }
                }
                if content.contains("All your subagents and background tasks have been stopped") {
                    for (tid, _) in self.active_bg_tasks.drain() {
                        if !completed_bg_tasks.iter().any(|(id, _)| id == &tid) {
                            completed_bg_tasks.push((tid, true));
                        }
                    }
                }
                for (task_id, canceled) in &completed_bg_tasks {
                    bg_deltas.push(BgTaskDelta::Completed {
                        task_id: task_id.clone(),
                        canceled: *canceled,
                    });
                }
            }

            if silent {
                continue;
            }

            if step_type == "CHECKPOINT" {
                self.consecutive_errors = 0;
                self.last_error_at = None;
                self.last_error_message = None;
                self.last_in_progress_heartbeat = Some(Instant::now());
                out.push(WakeupStep::Notice {
                    level: "info".into(),
                    message: "Context compacted automatically".into(),
                });
            } else if is_system_message_step(step_type, content) {
                for (task_id, canceled) in completed_bg_tasks {
                    out.push(WakeupStep::BackgroundTaskCompleted { task_id, canceled });
                }
                if !self.in_wakeup {
                    self.in_wakeup = true;
                    self.consecutive_errors = 0;
                    self.last_error_at = None;
                    self.last_error_message = None;
                    self.last_in_progress_heartbeat = Some(Instant::now());
                    if !self.always_active {
                        out.push(WakeupStep::Started {
                            source: "background_task".into(),
                        });
                    }
                }
            } else if self.always_active && step_type == "USER_INPUT" {
                self.in_wakeup = true;
                self.consecutive_errors = 0;
                self.last_error_at = None;
                self.last_error_message = None;
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
                self.last_in_progress_heartbeat = Some(Instant::now());
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
                self.last_in_progress_heartbeat = Some(Instant::now());
                let (cid, orig_detail) = self.pending_tools.remove(0);
                let detail = attach_transcript_tool_result(orig_detail, content);
                if status == "RUNNING"
                    && let Some((task_id, description, log_path)) =
                        parse_bg_task_started(content, &self.session_id)
                {
                    out.push(WakeupStep::BackgroundTaskStarted {
                        task_id,
                        call_id: Some(cid.clone()),
                        description,
                        log_path,
                        expires_at: started_bg_expires_at,
                    });
                    // Keep the tool call visibly running while the background task executes
                    out.push(WakeupStep::Tool {
                        call_id: cid,
                        status: ToolCallStatus::Running,
                        detail,
                    });
                } else {
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
            } else if step_type == "GENERIC"
                && status == "RUNNING"
                && let Some((task_id, description, log_path)) =
                    parse_bg_task_started(content, &self.session_id)
            {
                out.push(WakeupStep::BackgroundTaskStarted {
                    task_id,
                    call_id: None,
                    description,
                    log_path,
                    expires_at: started_bg_expires_at,
                });
            }
        }

        self.byte_offset += consumed_bytes;
        if self.byte_offset == file_len {
            self.last_checked_path = Some(path);
            self.last_checked_mtime = file_mtime;
        } else {
            self.last_checked_mtime = None;
        }
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
    let mut usage_cmd = Command::new(executable);
    usage_cmd
        .args(["-p", "/usage", "--output-format", "json"])
        .env("NO_COLOR", "1");
    crate::shell_snapshot::apply_cached_shell_env(&mut usage_cmd);
    let usage_fut = usage_cmd.output();

    let mut credits_cmd = Command::new(executable);
    credits_cmd
        .args(["-p", "/credits", "--output-format", "json"])
        .env("NO_COLOR", "1");
    crate::shell_snapshot::apply_cached_shell_env(&mut credits_cmd);
    let credits_fut = credits_cmd.output();

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
        let mut cmd = Command::new(exe);
        cmd.args(["acp", "--stdio"])
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        crate::shell_snapshot::apply_cached_shell_env(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;

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
        let mut wakeup_tools = WakeupToolRegistry::default();
        let _ = wakeup_tools.resolve_wakeup_tool(
            "call_wakeup_dup",
            ToolCallStatus::Completed,
            ToolCallDetail::Generic {
                tool_name: "Duplicate tool".into(),
                arguments_json: String::new(),
            },
        );
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_test".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        handle_acp_session_update(
            "ag_test",
            "conv_test",
            &json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": { "text": "Inspecting code" }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            "ag_test",
            "conv_test",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_wakeup_dup",
                "title": "Duplicate tool",
                "status": "completed"
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            "ag_test",
            "conv_test",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_live",
                "kind": "read",
                "title": "Read file",
                "status": "completed",
                "rawInput": { "Path": "src/main.rs" }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        handle_acp_session_update(
            "ag_test",
            "conv_test",
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "text": "Done!" }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
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
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_net".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        handle_acp_session_update(
            "ag_test",
            "conv_net",
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": {
                    "text": "\n⚠ API error (attempt 1): request failed: read: connection reset by peer\n\n⚠ The stream was interrupted. Please continue the task you were working on.\n"
                }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
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

        let mut monitor =
            AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), "conv_wake");
        monitor.full_path = dir.join("missing.jsonl");
        monitor.compact_path = transcript_path;

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

    #[tokio::test]
    async fn wakeup_tool_registry_reconciles_synthetic_wake_tool_with_native_acp_call_id() {
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_bg".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        // 1. Transcript emits synthetic `wake_tool_684_0` running a background command
        let _ = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_684_0",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "cargo test -p orbit-cli --test e2e_orchestration".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
        );
        let bg_meta = BackgroundTaskInfo {
            task_id: Some("conv_1/task-685".into()),
            label: Some("Run e2e tests".into()),
            log_path: Some("/tmp/task-685.log".into()),
        };
        wakeup_tools.mark_background_task("wake_tool_684_0", bg_meta.clone());
        bg_tracker.record_started(
            "conv_1/task-685".into(),
            Some("wake_tool_684_0".into()),
            Some("/tmp/task-685.log".into()),
            Some(bg_meta),
        );

        // 2. Wakeup round settles while background task is STILL running -> tool stays open!
        let closed_while_bg = wakeup_tools.settle_open_tools(&bg_tracker.active_call_ids());
        assert!(
            closed_while_bg.is_empty(),
            "Active background task tool must stay running while task executes"
        );

        // 3. Background task gets killed by signal; ACP delivers native `call_2135459364435572431` failed!
        let sig_cancel = handle_acp_session_update(
            "ag_test",
            "conv_1",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_2135459364435572431",
                "kind": "execute",
                "title": "Run tests",
                "status": "failed",
                "rawInput": { "CommandLine": "cargo test -p orbit-cli --test e2e_orchestration" },
                "rawOutput": "Command was terminated by signal or canceled."
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        // Must converge onto the EXACT canonical `wake_tool_684_0` card (no zombie + no duplicate!)
        let ev = evt_rx.try_recv().expect("converged tool update emitted");
        assert!(matches!(
            ev,
            DriverEvent::ToolCall {
                ref call_id,
                status: ToolCallStatus::Failed,
                background_task: Some(ref bt),
                ..
            } if call_id == "wake_tool_684_0" && bt.task_id.as_deref() == Some("conv_1/task-685")
        ));
        assert!(evt_rx.try_recv().is_err());

        // Must trigger self-healing signal cancel event with tracked task_id & log_path
        let sc = sig_cancel.expect("signal cancel detected");
        assert_eq!(sc.task_id, "conv_1/task-685");
        assert_eq!(sc.log_path.as_deref(), Some("/tmp/task-685.log"));
        assert!(bg_tracker.is_empty());

        // 4. Orphan non-bg running tool is cleanly closed on Settled
        let _ = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_700_0",
            ToolCallStatus::Running,
            ToolCallDetail::FileRead {
                path: "README.md".into(),
                start_line: None,
                end_line: None,
            },
        );
        let settled = wakeup_tools.settle_open_tools(&bg_tracker.active_call_ids());
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].0, "wake_tool_700_0");
    }

    #[tokio::test]
    async fn wakeup_tool_registry_reconciles_acp_first_without_zombie_running_or_bg_duplicates() {
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_acp_first".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(32);

        // Scenario A: Foreground shell during wakeup arrives via ACP FIRST (`call_1834`),
        // then transcript produces `wake_tool_1228_0`, then command finishes!
        handle_acp_session_update(
            "ag_test",
            "conv_1",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_1834",
                "kind": "execute",
                "title": "Run command",
                "status": "in_progress",
                "rawInput": { "CommandLine": "echo hello && git status" }
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;

        let ev_start = evt_rx.try_recv().expect("acp running emitted");
        assert!(matches!(
            ev_start,
            DriverEvent::ToolCall {
                ref call_id,
                status: ToolCallStatus::Running,
                background_task: None,
                ..
            } if call_id == "call_1834"
        ));

        // Transcript poll sees `wake_tool_1228_0` Running -> must map onto `call_1834` (no duplicate!)
        let res_wake_run = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_1228_0",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "echo hello && git status".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
        );
        assert!(matches!(res_wake_run, WakeupToolResolution::SkipDuplicate));

        // Transcript poll sees `wake_tool_1228_0` Completed -> must emit Completed under `call_1834`!
        let res_wake_done = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_1228_0",
            ToolCallStatus::Completed,
            ToolCallDetail::Shell {
                command: "echo hello && git status".into(),
                cwd: None,
                exit_code: Some(0),
                output_preview: Some("hello".into()),
            },
        );
        assert!(matches!(
            res_wake_done,
            WakeupToolResolution::Emit {
                ref call_id,
                status: ToolCallStatus::Completed,
                background_task: None,
                ..
            } if call_id == "call_1834"
        ));

        // Subsequent ACP Completed for `call_1834` must be suppressed as duplicate (already completed)
        handle_acp_session_update(
            "ag_test",
            "conv_1",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_1834",
                "kind": "execute",
                "status": "completed",
                "rawInput": { "CommandLine": "echo hello && git status" }
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        assert!(
            evt_rx.try_recv().is_err(),
            "duplicate terminal ACP update suppressed"
        );

        // Scenario B: Fast <10ms ACP tool completes BEFORE transcript poll arrives
        handle_acp_session_update(
            "ag_test",
            "conv_1",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_fast_read",
                "kind": "read",
                "status": "completed",
                "rawInput": { "Path": "Cargo.toml" }
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let _ = evt_rx.try_recv().expect("fast read completed");

        let res_fast_wake = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_1246_0",
            ToolCallStatus::Running,
            ToolCallDetail::FileRead {
                path: "Cargo.toml".into(),
                start_line: None,
                end_line: None,
            },
        );
        assert!(
            matches!(res_fast_wake, WakeupToolResolution::SkipDuplicate),
            "late transcript Running must NOT regress fast Completed ACP tool or create duplicate"
        );

        // Scenario C: Real background task arrives ACP-first (`call_1041`), then transcript marks it
        // as background task `conv_1/task-1290` -> must stay ONE card with `background_task: Some(...)`
        handle_acp_session_update(
            "ag_test",
            "conv_1",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_1041",
                "kind": "execute",
                "status": "in_progress",
                "rawInput": { "CommandLine": "python render_video.py --hd" }
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let _ = evt_rx
            .try_recv()
            .expect("bg shell initially running foreground");

        // Transcript step starts the tool (`wake_tool_1289_0` maps to `call_1041`)
        let _ = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_1289_0",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "python render_video.py --hd".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
        );
        let canon = wakeup_tools
            .canonical_id_for_wakeup("wake_tool_1289_0")
            .to_string();
        assert_eq!(canon, "call_1041");

        let bg_info = BackgroundTaskInfo {
            task_id: Some("conv_1/task-1290".into()),
            label: Some("python render_video.py --hd".into()),
            log_path: Some("/tmp/task-1290.log".into()),
        };
        wakeup_tools.mark_background_task(&canon, bg_info.clone());
        bg_tracker.record_started(
            "conv_1/task-1290".into(),
            Some(canon.clone()),
            Some("/tmp/task-1290.log".into()),
            Some(bg_info),
        );

        let res_bg_emit = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_1289_0",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "python render_video.py --hd".into(),
                cwd: None,
                exit_code: None,
                output_preview: Some(
                    "Tool is running as a background task with task id: conv_1/task-1290".into(),
                ),
            },
        );
        assert!(matches!(
            res_bg_emit,
            WakeupToolResolution::Emit {
                ref call_id,
                status: ToolCallStatus::Running,
                background_task: Some(ref bt),
                ..
            } if call_id == "call_1041" && bt.task_id.as_deref() == Some("conv_1/task-1290")
        ));

        // On Settled, ONLY `call_1041` remains open (ZERO zombie foreground calls!)
        let settled = wakeup_tools.settle_open_tools(&bg_tracker.active_call_ids());
        assert!(settled.is_empty());

        // When BackgroundTaskCompleted arrives, `finish_tool` closes `call_1041` immediately
        let info = bg_tracker.remove("conv_1/task-1290").expect("tracked");
        let cid = info.call_id.expect("bound call_id");
        assert_eq!(cid, "call_1041");
        let (_, closed_bg) = wakeup_tools
            .finish_tool(&cid, ToolCallStatus::Completed)
            .expect("closed");
        assert_eq!(
            closed_bg.and_then(|b| b.task_id).as_deref(),
            Some("conv_1/task-1290")
        );
    }

    #[test]
    fn transcript_monitor_handles_checkpoint_compaction_in_progress_thinking_and_undelivered_cancel()
     {
        let dir = std::env::temp_dir().join(format!("orbit_chk_test_{}", uuid::Uuid::new_v4()));
        let logs_dir = dir.join(".system_generated/logs");
        let undeliv_dir = dir.join(".system_generated/messages/undelivered");
        std::fs::create_dir_all(&logs_dir).expect("mkdir logs");
        std::fs::create_dir_all(&undeliv_dir).expect("mkdir undelivered");

        let log_file = dir.join("task-716.log");
        std::fs::write(&log_file, "9 passed; 1 failed\nscenario_02 failed\n").expect("write log");

        let transcript_path = logs_dir.join("transcript_full.jsonl");
        let initial = [
            json!({
                "step_index": 20,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "<SYSTEM_MESSAGE>Task id \"conv_1/task-700\" finished with result: ok</SYSTEM_MESSAGE>"
            }),
            json!({
                "step_index": 21,
                "type": "CHECKPOINT",
                "status": "DONE",
                "content": "compacted"
            }),
            json!({
                "step_index": 22,
                "type": "PLANNER_RESPONSE",
                "status": "RUNNING",
                "thinking": "Analyzing compaction and running tests...",
                "content": ""
            }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        std::fs::write(&transcript_path, &initial).expect("write initial");

        let mut monitor = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), "conv_1");
        let steps1 = monitor.poll_post_turn_wakeup();
        // Must surface compaction divider notice AND incremental thinking while PLANNER_RESPONSE is still RUNNING!
        assert!(steps1.iter().any(|s| matches!(
            s,
            WakeupStep::Notice { level, message } if level == "info" && message.contains("compacted")
        )));
        assert!(steps1.iter().any(|s| matches!(
            s,
            WakeupStep::Thinking { id, text } if id == "wake_th_22" && text.contains("Analyzing compaction")
        )));

        // Now agy rewrites step 22 to DONE with a background task tool call + GENERIC background start
        let updated = [
            json!({
                "step_index": 20,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "<SYSTEM_MESSAGE>Task id \"conv_1/task-700\" finished with result: ok</SYSTEM_MESSAGE>"
            }),
            json!({
                "step_index": 21,
                "type": "CHECKPOINT",
                "status": "DONE",
                "content": "compacted"
            }),
            json!({
                "step_index": 22,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "thinking": "Analyzing compaction and running tests... Done thinking.",
                "content": "Running suite",
                "tool_calls": [
                    {
                        "name": "run_command",
                        "args": { "CommandLine": "\"cargo test\"" }
                    }
                ]
            }),
            json!({
                "step_index": 23,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Tool is running as a background task with task id: conv_1/task-716\nTask logs are available at: file://{}", log_file.display())
            }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        std::fs::write(&transcript_path, &updated).expect("rewrite done");

        let steps2 = monitor.poll_post_turn_wakeup();
        assert!(steps2.iter().any(|s| matches!(
            s,
            WakeupStep::BackgroundTaskStarted { task_id, call_id, .. }
                if task_id == "conv_1/task-716" && call_id.as_deref() == Some("wake_tool_22_0")
        )));

        // Simulate undelivered MESSAGE_PRIORITY_LOW cancellation file
        let mut tracker = BackgroundTaskTracker::default();
        tracker.record_started(
            "conv_1/task-716".into(),
            Some("wake_tool_22_0".into()),
            Some(log_file.to_string_lossy().to_string()),
            None,
        );
        assert_eq!(tracker.waiting_source_label(), "waiting:1 background task");

        std::fs::write(
            undeliv_dir.join("msg_1.txt"),
            format!(
                "priority=MESSAGE_PRIORITY_LOW hideFromUser=true Task id \"conv_1/task-716\" was canceled with result: Tool execution was canceled\nLog: file://{}",
                log_file.display()
            ),
        )
        .expect("write undelivered");

        let cancels = monitor.poll_undelivered_cancellations(&tracker);
        assert_eq!(cancels.len(), 1);
        assert_eq!(cancels[0].task_id, "conv_1/task-716");
        // Second poll must NOT double-trigger the same undelivered file
        assert!(monitor.poll_undelivered_cancellations(&tracker).is_empty());

        let heal = PendingSelfHeal {
            task_id: cancels[0].task_id.clone(),
            reason: cancels[0].reason.clone(),
            log_path: cancels[0].log_path.clone(),
            output_tail: cancels[0]
                .log_path
                .as_deref()
                .and_then(|p| read_log_tail(Path::new(p), 500)),
            detected_at: Instant::now(),
            transcript_step_at_detect: monitor.max_seen_step(),
        };
        let prompt = build_self_heal_prompt(&heal, 1);
        assert!(prompt.contains("conv_1/task-716"));
        assert!(prompt.contains("scenario_02 failed"));
        assert!(prompt.contains("NOT canceled by the user"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bg_task_parser_rejects_source_code_completed_outputs_and_foreign_sessions() {
        // Positive: real Antigravity background-task start header belonging to current session
        let real_start = "Created At: 2026-11-20T14:56:09+08:00\n\
Tool is running as a background task with task id: sess_abc/task-1290\n\
Task Description: Render HD video\n\
Task logs are available at: file:///tmp/task-1290.log\n\
Log output:\n\
Task Description: spoofed inside log stdout\n\
Log: file:///tmp/spoofed.log\n";
        let (tid, desc, log) =
            parse_bg_task_started(real_start, "sess_abc").expect("must match own bg task");
        assert_eq!(tid, "sess_abc/task-1290");
        assert_eq!(desc.as_deref(), Some("Render HD video"));
        assert_eq!(log.as_deref(), Some("/tmp/task-1290.log"));

        // Positive: header without leading Created At
        let minimal = "Tool is running as a background task with task id: sess_abc/task-42\n\
Task logs are available at: file:///tmp/task-42.log\n";
        let (tid2, desc2, log2) =
            parse_bg_task_started(minimal, "sess_abc").expect("must match minimal header");
        assert_eq!(tid2, "sess_abc/task-42");
        assert_eq!(desc2, None);
        assert_eq!(log2.as_deref(), Some("/tmp/task-42.log"));

        // Negative 1: `read_file` reading `antigravity.rs` source code containing test literals
        let read_file_out = "Created At: 2026-11-20T15:00:00+08:00\n\
Completed At: 2026-11-20T15:00:01+08:00\n\
Showing lines 4650-4670 of 4726\n\
4667: \"content\": \"Tool is running as a background task with task id: conv_1/task-716\"\n";
        assert!(parse_bg_task_started(read_file_out, "sess_abc").is_none());
        assert!(parse_bg_task_started(read_file_out, "conv_1").is_none());

        // Negative 2: completed shell/sqlite output printing another session's (or even same session's) task ID
        let sqlite_out = "Created At: 2026-11-20T15:17:37+08:00\n\
Completed At: 2026-11-20T15:17:40+08:00\n\
Command exited with code 0.\n\n\
Command output:\n\
6205|wake_tool_1293_1|running|shell|Created At: 2026-11-20T15:13:56+08:00\n\
Tool is running as a background task with task id: sess_abc/task-1295\n";
        assert!(parse_bg_task_started(sqlite_out, "sess_abc").is_none());

        // Negative 3: foreign session ID even with valid header structure
        let foreign_start = "Created At: 2026-11-20T15:13:56+08:00\n\
Tool is running as a background task with task id: 403bd6c2-other/task-1295\n\
Task logs are available at: file:///tmp/task-1295.log\n";
        assert!(parse_bg_task_started(foreign_start, "sess_abc").is_none());

        // Negative 4: indented / quoted marker line
        let quoted = "  \"Tool is running as a background task with task id: sess_abc/task-1\"\n";
        assert!(parse_bg_task_started(quoted, "sess_abc").is_none());
    }

    #[test]
    fn transcript_monitor_ignores_generic_done_and_embedded_system_message_in_tool_outputs() {
        let dir = std::env::temp_dir().join(format!("orbit_strict_bg_{}", uuid::Uuid::new_v4()));
        let logs_dir = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs_dir).expect("mkdir logs");
        let transcript_path = logs_dir.join("transcript_full.jsonl");

        // Steps 1-4 are ordinary DONE tool outputs / planner text quoting bg markers & `<SYSTEM_MESSAGE>`
        let noisy_steps = [
            json!({
                "step_index": 1,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Showing lines 4390-4680\n\"content\": \"<SYSTEM_MESSAGE>background task done</SYSTEM_MESSAGE>\"\nTool is running as a background task with task id: conv_self/task-999\n"
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Created At: 2026-11-20T15:00:00+08:00\nTool is running as a background task with task id: conv_self/task-888\n"
            }),
            json!({
                "step_index": 3,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": "Created At: 2026-11-20T15:00:01+08:00\nTool is running as a background task with task id: other_session/task-777\n"
            }),
            json!({
                "step_index": 4,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "content": "Note: `<SYSTEM_MESSAGE>` is only for real system notifications."
            }),
        ];
        let payload1 = noisy_steps
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&transcript_path, &payload1).expect("write noisy");

        let mut monitor =
            AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), "conv_self");
        let steps1 = monitor.poll_post_turn_wakeup();
        assert!(
            steps1.is_empty(),
            "read_file/DONE/foreign/planner steps must NEVER trigger wakeup or bg tasks, got: {}",
            steps1.len()
        );
        assert!(!monitor.in_wakeup);

        // Also check silent foreground sync on noisy steps yields ZERO BgTaskDelta
        let mut silent_mon =
            AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), "conv_self");
        let silent_deltas = silent_mon.sync_silent_with_bg_tasks();
        assert!(silent_deltas.is_empty());

        // Now append a REAL self-session RUNNING background task + its real SYSTEM_MESSAGE completion
        let real_steps = [
            json!({
                "step_index": 5,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": "Created At: 2026-11-20T15:10:00+08:00\nTool is running as a background task with task id: conv_self/task-500\nTask Description: Generate video\nTask logs are available at: file:///tmp/task-500.log\n"
            }),
            json!({
                "step_index": 6,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "<SYSTEM_MESSAGE>Task id \"conv_self/task-500\" finished with result: ok</SYSTEM_MESSAGE>"
            }),
        ];
        let payload2 = format!(
            "{payload1}{}\n",
            real_steps
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        );
        std::fs::write(&transcript_path, &payload2).expect("append real");

        let steps2 = monitor.poll_post_turn_wakeup();
        assert!(steps2.iter().any(|s| matches!(
            s,
            WakeupStep::BackgroundTaskStarted { task_id, description, log_path, .. }
                if task_id == "conv_self/task-500"
                    && description.as_deref() == Some("Generate video")
                    && log_path.as_deref() == Some("/tmp/task-500.log")
        )));
        assert!(steps2.iter().any(|s| matches!(
            s,
            WakeupStep::BackgroundTaskCompleted { task_id, canceled: false }
                if task_id == "conv_self/task-500"
        )));
        assert!(
            steps2
                .iter()
                .any(|s| matches!(s, WakeupStep::Started { .. }))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn acp_session_update_rejects_foreign_or_completed_replayed_bg_tasks() {
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        // Simulate shim reattach having already observed `conv_self/task-10` as completed on disk
        bg_tracker.mark_finished("conv_self/task-10".into());

        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "turn_reattach".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(16);

        // 1. Replayed historical ACP frame for already-finished `conv_self/task-10` must NOT
        //    force status back to Running or re-add to `bg_tracker`!
        handle_acp_session_update(
            "ag_test",
            "conv_self",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_replayed_done",
                "kind": "execute",
                "status": "completed",
                "rawInput": { "CommandLine": "cargo test" },
                "rawOutput": "Created At: 2026-11-20T15:00:00+08:00\nTool is running as a background task with task id: conv_self/task-10\n"
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let ev1 = evt_rx.try_recv().expect("replayed frame emitted");
        assert!(matches!(
            ev1,
            DriverEvent::ToolCall {
                ref call_id,
                status: ToolCallStatus::Completed,
                background_task: None,
                ..
            } if call_id == "call_replayed_done"
        ));
        assert!(bg_tracker.is_empty());

        // 2. Completed `read_file` containing test marker for `conv_1/task-716` must stay Completed
        handle_acp_session_update(
            "ag_test",
            "conv_self",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_read_src",
                "kind": "read",
                "status": "completed",
                "rawInput": { "Path": "crates/orbit-providers/src/antigravity.rs" },
                "rawOutput": "Showing lines 4660-4670\n\"Tool is running as a background task with task id: conv_1/task-716\""
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let ev2 = evt_rx.try_recv().expect("read_file emitted");
        assert!(matches!(
            ev2,
            DriverEvent::ToolCall {
                ref call_id,
                status: ToolCallStatus::Completed,
                background_task: None,
                ..
            } if call_id == "call_read_src"
        ));
        assert!(bg_tracker.is_empty());

        // 3. Genuine live background task for `conv_self/task-20` MUST be tracked & marked Running
        handle_acp_session_update(
            "ag_test",
            "conv_self",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_live_bg",
                "kind": "execute",
                "status": "completed",
                "rawInput": { "CommandLine": "python generate_video.py" },
                "rawOutput": "Created At: 2026-11-20T15:20:00+08:00\nTool is running as a background task with task id: conv_self/task-20\nTask Description: Video generation\nTask logs are available at: file:///tmp/task-20.log\n"
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let ev3 = evt_rx.try_recv().expect("live bg emitted");
        assert!(matches!(
            ev3,
            DriverEvent::ToolCall {
                ref call_id,
                status: ToolCallStatus::Running,
                background_task: Some(ref bt),
                ..
            } if call_id == "call_live_bg"
                && bt.task_id.as_deref() == Some("conv_self/task-20")
                && bt.label.as_deref() == Some("Video generation")
        ));
        assert!(!bg_tracker.is_empty());
        assert_eq!(
            bg_tracker.waiting_source_label(),
            "waiting:1 background task"
        );
    }

    #[test]
    fn schedule_timer_completes_on_system_message_sender_while_cron_stays_active() {
        let dir = std::env::temp_dir().join(format!("orbit_timer_test_{}", uuid::Uuid::new_v4()));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("create logs dir");
        let full_path = logs.join("transcript_full.jsonl");
        let sess = "e17d47cc-55c1-4d29-9f49-e893aa911896";

        let lines = [
            json!({
                "step_index": 1,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Created At: 2026-11-20T10:10:45+08:00\nTool is running as a background task with task id: {sess}/task-46\nTask Description: Timer: 8m, Prompt: Check progress\n")
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Created At: 2026-11-20T10:10:46+08:00\nTool is running as a background task with task id: {sess}/task-99\nTask Description: Cron: */5 * * * *, Prompt: Periodic health check\n")
            }),
            json!({
                "step_index": 3,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": format!("[Message] timestamp=2026-11-20T10:18:46Z sender={sess}/task-46 priority=MESSAGE_PRIORITY_HIGH content=Check progress")
            }),
            json!({
                "step_index": 4,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": format!("[Message] timestamp=2026-11-20T10:20:00Z sender={sess}/task-99 priority=MESSAGE_PRIORITY_HIGH content=Periodic health check")
            }),
        ];
        let body = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(&full_path, body).expect("write transcript");

        let mut mon = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), sess);
        let deltas = mon.sync_silent_with_bg_tasks();

        // One-shot Timer (`task-46`) must emit Completed; recurring Cron (`task-99`) must NOT.
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, canceled: false } if task_id == &format!("{sess}/task-46")
        )));
        assert!(!deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, .. } if task_id == &format!("{sess}/task-99")
        )));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn acp_schedule_dispatch_ack_and_run_prefix_reconcile_without_duplicates() {
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "t1".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(32);

        // 1. ACP starts `Run schedule` and immediately emits dispatch-ack `completed` without `rawOutput`
        handle_acp_session_update(
            "ag_p",
            "sess_sched",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_sched_1",
                "title": "Run schedule",
                "status": "in_progress",
                "rawInput": { "DurationSeconds": 480, "Prompt": "watchdog" }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let _ = evt_rx.try_recv();

        handle_acp_session_update(
            "ag_p",
            "sess_sched",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_sched_1",
                "title": "Run schedule",
                "status": "completed"
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let _ = evt_rx.try_recv();

        // 2. Transcript discovers `Timer: 8m` background task -> must bind onto `call_sched_1`
        let matched =
            wakeup_tools.find_matching_call_id_for_bg_task(Some("Timer: 8m, Prompt: watchdog"));
        assert_eq!(matched.as_deref(), Some("call_sched_1"));

        let bg_info = BackgroundTaskInfo {
            task_id: Some("sess_sched/task-46".into()),
            label: Some("Timer: 8m, Prompt: watchdog".into()),
            log_path: None,
        };
        wakeup_tools.mark_background_task("call_sched_1", bg_info.clone());
        bg_tracker.record_started(
            "sess_sched/task-46".into(),
            Some("call_sched_1".into()),
            None,
            Some(bg_info),
        );

        // 3. A subsequent ACP `completed` without exitCode/completion must keep it Running!
        handle_acp_session_update(
            "ag_p",
            "sess_sched",
            &json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_sched_1",
                "title": "Run schedule",
                "status": "completed"
            }),
            true,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        assert!(!bg_tracker.is_empty());

        // 4. Normalized signatures match `"Run mcp_orbit_get_agent_status"` with `"mcp_orbit_get_agent_status"`
        let acp_sig = normalize_tool_signature(&ToolCallDetail::Generic {
            tool_name: "Run mcp_orbit_get_agent_status".into(),
            arguments_json: "{}".into(),
        });
        let tx_sig = normalize_tool_signature(&ToolCallDetail::Generic {
            tool_name: "mcp_orbit_get_agent_status".into(),
            arguments_json: "{}".into(),
        });
        assert!(signatures_match(&acp_sig, &tx_sig));
    }

    #[test]
    fn subagent_classification_and_text_conversation_id_and_subagent_monitor() {
        // `manage_subagents` must NOT be classified as ToolCallDetail::Subagent
        let manage_acp = map_agy_tool_detail(
            &json!({
                "title": "Run manage_subagents",
                "rawInput": { "Action": "list" }
            }),
            None,
        );
        assert!(matches!(manage_acp, ToolCallDetail::Generic { .. }));
        let manage_tx =
            map_transcript_tool_detail("manage_subagents", Some(&json!({ "Action": "list" })));
        assert!(matches!(manage_tx, ToolCallDetail::Generic { .. }));

        // `invoke_subagent` with text-wrapped JSON extracts `conversationId`
        let sub_text = "Created At: 2026-11-20T10:10:02+08:00\nCompleted At: 2026-11-20T10:10:02+08:00\nCreated the following subagent:\n{\n  \"conversationId\":  \"dbb609d7-d67b-42cb-a105-6c660888af12\",\n  \"logAbsoluteUri\":  \"file:///tmp/transcript.jsonl\"\n}";
        let start_detail = map_agy_tool_detail(
            &json!({
                "title": "Run invoke_subagent",
                "rawInput": {
                    "Role": "Action Cinema Directing Researcher",
                    "Prompt": "Investigate camera framing"
                }
            }),
            None,
        );
        let done_detail = map_agy_tool_detail(
            &json!({
                "title": "Run invoke_subagent",
                "rawOutput": sub_text
            }),
            Some(&start_detail),
        );
        assert!(matches!(
            done_detail,
            ToolCallDetail::Subagent {
                child_agent_id: Some(ref cid),
                ref role,
                ref prompt_preview,
            } if cid == "dbb609d7-d67b-42cb-a105-6c660888af12"
                && role == "Action Cinema Directing Researcher"
                && prompt_preview == "Investigate camera framing"
        ));

        // Subagent transcript monitor (`from_brain_dir_subagent`) streams steps & settles
        let dir = std::env::temp_dir().join(format!("orbit_submon_test_{}", uuid::Uuid::new_v4()));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("create logs dir");
        let full_path = logs.join("transcript_full.jsonl");
        let lines = [
            json!({
                "step_index": 0,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "You are a subagent tasked with researching cinema framing."
            }),
            json!({
                "step_index": 1,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "thinking": "Searching literature",
                "content": "",
                "tool_calls": [{ "id": "tc1", "name": "search_web", "args": { "query": "action cinematography" } }]
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Found 10 references"
            }),
            json!({
                "step_index": 3,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "content": "Completed research and reported to parent.",
                "tool_calls": []
            }),
        ];
        std::fs::write(
            &full_path,
            lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .expect("write subagent transcript");

        let mut sub_mon =
            AgyTranscriptMonitor::from_brain_dir_subagent(dir.clone(), "dbb609d7-sub");
        let events = sub_mon.poll_post_turn_wakeup();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, WakeupStep::Thinking { .. }))
        );
        assert!(events.iter().any(|e| matches!(e, WakeupStep::Tool { .. })));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, WakeupStep::Message { .. }))
        );
        assert!(events.iter().any(|e| matches!(e, WakeupStep::Settled)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn parallel_same_prefix_shells_map_distinctly_and_new_turn_never_hijacks_canceled_wake_tools()
     {
        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut seg_state = AgyStreamSegmentState {
            turn_tag: "t1".into(),
            ..AgyStreamSegmentState::default()
        };
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let mut last_ask_raw = None;
        let (evt_tx, mut evt_rx) = mpsc::channel(64);

        // Turn 1 (wakeup): creates `wake_tool_276_1` (shell) and `wake_tool_276_2` (schedule),
        // marks them as background tasks, then user interrupts & cancels both.
        wakeup_tools.begin_turn();
        let _ = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_276_1",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "set -a && source runtime.env && set +a && python exp5_gpu_ab.py".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
        );
        let _ = wakeup_tools.resolve_wakeup_tool(
            "wake_tool_276_2",
            ToolCallStatus::Running,
            ToolCallDetail::Generic {
                tool_name: "schedule".into(),
                arguments_json: "{\"DurationSeconds\":480}".into(),
            },
        );
        let _ = wakeup_tools.mark_background_task(
            "wake_tool_276_1",
            BackgroundTaskInfo {
                task_id: Some("sess_1/task-278".into()),
                label: Some("set -a && source runtime.env && set +a &&".into()),
                log_path: None,
            },
        );
        let _ = wakeup_tools.mark_background_task(
            "wake_tool_276_2",
            BackgroundTaskInfo {
                task_id: Some("sess_1/task-279".into()),
                label: Some("Timer: 8m, Prompt: old watchdog".into()),
                log_path: None,
            },
        );
        let closed = wakeup_tools.settle_open_tools(&HashSet::new());
        assert_eq!(closed.len(), 2);

        // Turn 2 (new foreground user turn): starts TWO parallel commands with the exact same
        // prefix + ONE new `Run schedule`. None of them may hijack `wake_tool_276_1` / `276_2`!
        wakeup_tools.begin_turn();
        for (cid, cmd) in [
            (
                "call_plan",
                "set -a && source runtime.env && set +a && python exp5_plan_r4.py",
            ),
            (
                "call_gpu",
                "set -a && source runtime.env && set +a && python exp5_gpu_ab.py",
            ),
        ] {
            handle_acp_session_update(
                "ag_t",
                "sess_1",
                &json!({
                    "sessionUpdate": "tool_call",
                    "toolCallId": cid,
                    "kind": "execute",
                    "status": "in_progress",
                    "rawInput": { "CommandLine": cmd }
                }),
                false,
                &mut wakeup_tools,
                &mut bg_tracker,
                &mut seg_state,
                &mut tool_cache,
                &mut subagent_monitors,
                &mut last_ask_raw,
                &evt_tx,
            )
            .await;
            let ev = evt_rx.try_recv().expect("shell event");
            assert!(
                matches!(ev, DriverEvent::ToolCall { ref call_id, .. } if call_id == cid),
                "new turn ACP call `{cid}` must NOT hijack prior turn wake_tool_276_1"
            );
        }

        // New `Run schedule` starts & immediately acks completed via ACP
        handle_acp_session_update(
            "ag_t",
            "sess_1",
            &json!({
                "sessionUpdate": "tool_call",
                "toolCallId": "call_sched_new",
                "title": "Run schedule",
                "status": "completed",
                "rawInput": { "DurationSeconds": 480, "Prompt": "new watchdog" }
            }),
            false,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut seg_state,
            &mut tool_cache,
            &mut subagent_monitors,
            &mut last_ask_raw,
            &evt_tx,
        )
        .await;
        let ev_sched = evt_rx.try_recv().expect("schedule event");
        assert!(
            matches!(ev_sched, DriverEvent::ToolCall { ref call_id, .. } if call_id == "call_sched_new"),
            "new turn `Run schedule` must NOT hijack prior turn wake_tool_276_2"
        );

        // Now `apply_foreground_bg_deltas` delivers `task-290` & `task-291` with IDENTICAL truncated
        // first-line descriptions + `task-292` Timer -> must map 1-to-1 to `call_plan`, `call_gpu`,
        // and `call_sched_new` without overwriting or spawning synthetic `bg_task_*`!
        let deltas = vec![
            BgTaskDelta::Started {
                task_id: "sess_1/task-290".into(),
                call_id: None,
                description: Some("set -a && source runtime.env && set +a &&".into()),
                log_path: Some("/tmp/task-290.log".into()),
                expires_at: None,
            },
            BgTaskDelta::Started {
                task_id: "sess_1/task-291".into(),
                call_id: None,
                description: Some("set -a && source runtime.env && set +a &&".into()),
                log_path: Some("/tmp/task-291.log".into()),
                expires_at: None,
            },
            BgTaskDelta::Started {
                task_id: "sess_1/task-292".into(),
                call_id: None,
                description: Some("Timer: 8m, Prompt: new watchdog".into()),
                log_path: Some("/tmp/task-292.log".into()),
                expires_at: None,
            },
        ];
        apply_foreground_bg_deltas(
            "ag_t",
            deltas.clone(),
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut tool_cache,
            &mut subagent_monitors,
            &evt_tx,
        )
        .await;

        assert_eq!(bg_tracker.call_id_for("sess_1/task-290"), Some("call_plan"));
        assert_eq!(bg_tracker.call_id_for("sess_1/task-291"), Some("call_gpu"));
        assert_eq!(
            bg_tracker.call_id_for("sess_1/task-292"),
            Some("call_sched_new")
        );
        // Drain the 3 emitted updates
        for _ in 0..3 {
            let _ = evt_rx.try_recv().expect("bg task update emitted");
        }

        // Re-applying identical `Started` deltas (e.g. at turn end) must emit ZERO redundant ToolCall events!
        apply_foreground_bg_deltas(
            "ag_t",
            deltas,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut tool_cache,
            &mut subagent_monitors,
            &evt_tx,
        )
        .await;
        assert!(
            evt_rx.try_recv().is_err(),
            "unchanged background tasks must not be re-emitted after AssistantMessage"
        );
    }

    #[test]
    fn force_cancellation_system_notice_cancels_active_transcript_timers() {
        let dir = std::env::temp_dir().join(format!("orbit_fcancel_test_{}", uuid::Uuid::new_v4()));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let full_path = logs.join("transcript_full.jsonl");
        let sess = "sess_fc";
        let lines = [
            json!({
                "step_index": 1,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Created At: 2026-11-20T20:25:13+08:00\nTool is running as a background task with task id: {sess}/task-279\nTask Description: Timer: 8m, Prompt: watchdog\n")
            }),
            json!({
                "step_index": 2,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "[Message] timestamp=2026-11-20T12:27:04Z sender=system priority=MESSAGE_PRIORITY_LOW content=[Notice] All your subagents and background tasks have been stopped due to user force cancellation."
            }),
        ];
        std::fs::write(
            &full_path,
            lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .expect("write");

        let mut mon = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), sess);
        let deltas = mon.sync_silent_with_bg_tasks();
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, canceled: true } if task_id == &format!("{sess}/task-279")
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timer_condition_any_canceled_by_system_message_not_user_input() {
        let dir = std::env::temp_dir().join(format!("orbit_tcond_test_{}", uuid::Uuid::new_v4()));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let full_path = logs.join("transcript_full.jsonl");
        let sess = "sess_tc";
        let lines = [
            json!({
                "step_index": 1,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "tool_calls": [{
                    "name": "schedule",
                    "args": {
                        "DurationSeconds": "420",
                        "Prompt": "\"watchdog\"",
                        "TimerCondition": "\"any\""
                    }
                }]
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "RUNNING",
                "created_at": "2026-11-20T14:00:00Z",
                "content": format!("Created At: 2026-11-20T22:00:00+08:00\nTool is running as a background task with task id: {sess}/task-500\nTask Description: Timer: 7m, Prompt: watchdog\n")
            }),
            // USER_INPUT must NOT cancel TimerCondition: "any"
            json!({
                "step_index": 3,
                "type": "USER_INPUT",
                "status": "DONE",
                "content": "<USER_REQUEST>follow-up</USER_REQUEST>"
            }),
            // A real SYSTEM_MESSAGE from another task silently cancels task-500 in Antigravity
            json!({
                "step_index": 4,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": format!("<SYSTEM_MESSAGE>\n[Message] timestamp=2026-11-20T14:02:00Z sender={sess}/task-469 priority=MESSAGE_PRIORITY_HIGH content=Task id \"{sess}/task-469\" finished with result:\nok\n</SYSTEM_MESSAGE>")
            }),
        ];
        std::fs::write(
            &full_path,
            lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .expect("write");

        let mut mon = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), sess);
        let deltas = mon.sync_silent_with_bg_tasks();
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Started { task_id, expires_at: Some(_), .. } if task_id == &format!("{sess}/task-500")
        )));
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, canceled: false } if task_id == &format!("{sess}/task-500")
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manage_task_kill_and_empty_list_settle_active_tasks() {
        let dir = std::env::temp_dir().join(format!("orbit_mtask_test_{}", uuid::Uuid::new_v4()));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let full_path = logs.join("transcript_full.jsonl");
        let sess = "sess_mt";
        let lines = [
            json!({
                "step_index": 1,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Created At: 2026-11-20T21:38:36+08:00\nTool is running as a background task with task id: {sess}/task-470\nTask Description: Timer: 8m, Prompt: wd1\n")
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Created At: 2026-11-20T21:38:37+08:00\nTool is running as a background task with task id: {sess}/task-471\nTask Description: Timer: 9m, Prompt: wd2\n")
            }),
            json!({
                "step_index": 3,
                "type": "GENERIC",
                "status": "DONE",
                "content": format!("Created At: 2026-11-20T21:46:24+08:00\nCompleted At: 2026-11-20T21:46:27+08:00\nTask \"{sess}/task-470\" cancelled.\n")
            }),
            json!({
                "step_index": 4,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Created At: 2026-11-20T22:43:33+08:00\nCompleted At: 2026-11-20T22:43:33+08:00\nNo background tasks are currently running.\n"
            }),
        ];
        std::fs::write(
            &full_path,
            lines
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )
        .expect("write");

        let mut mon = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), sess);
        let deltas = mon.sync_silent_with_bg_tasks();
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, canceled: true } if task_id == &format!("{sess}/task-470")
        )));
        assert!(deltas.iter().any(|d| matches!(
            d,
            BgTaskDelta::Completed { task_id, canceled: false } if task_id == &format!("{sess}/task-471")
        )));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn timer_bg_task_never_hijacks_parallel_running_shell_or_prior_turn_schedule() {
        let mut reg = WakeupToolRegistry::default();
        // Turn 1: a `wake_tool_535_2` schedule left open in Running state
        reg.begin_turn();
        let _ = reg.resolve_wakeup_tool(
            "wake_tool_535_2",
            ToolCallStatus::Running,
            ToolCallDetail::Generic {
                tool_name: "schedule".into(),
                arguments_json: "{\"DurationSeconds\":420}".into(),
            },
        );
        reg.mark_background_task(
            "wake_tool_535_2",
            BackgroundTaskInfo {
                task_id: Some("sess_1/task-538".into()),
                label: Some("Timer: 7m".into()),
                log_path: None,
            },
        );

        // Turn 2: parallel `run_command` + `Run schedule` arrive via ACP
        reg.begin_turn();
        let res_sched = reg.resolve_acp_tool(
            "call_sched_625",
            ToolCallStatus::Completed,
            ToolCallDetail::Generic {
                tool_name: "Run schedule".into(),
                arguments_json: "{\"DurationSeconds\":360}".into(),
            },
            None,
            false,
        );
        assert!(
            matches!(res_sched, AcpToolResolution::Emit { ref call_id, .. } if call_id == "call_sched_625"),
            "new turn Run schedule must NOT hijack prior turn wake_tool_535_2"
        );
        let _ = reg.resolve_acp_tool(
            "call_shell_624",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "python exp6_pure_ceiling.py".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
            None,
            false,
        );
        // Bind timer when no unbound schedule exists -> must return None, NEVER bind to `call_shell_624`!
        let _ = reg.mark_background_task(
            "call_sched_625",
            BackgroundTaskInfo {
                task_id: Some("sess_1/task-625".into()),
                label: Some("Timer: 6m".into()),
                log_path: None,
            },
        );
        assert_eq!(
            reg.find_matching_call_id_for_bg_task(Some("Timer: 5m, Prompt: extra")),
            None,
            "unbound Timer must NEVER hijack a parallel running Shell command"
        );
    }

    #[test]
    fn one_shot_timer_duration_parsing_and_expiry_reaping() {
        assert_eq!(
            parse_timer_duration_secs("Timer: 6m, Prompt: check"),
            Some(360)
        );
        assert_eq!(
            parse_timer_duration_secs("Timer: 45s, Prompt: quick"),
            Some(45)
        );
        assert_eq!(
            parse_timer_duration_secs("Timer: 1h30m, Prompt: long"),
            Some(5400)
        );
        assert_eq!(
            parse_timer_duration_secs("Cron: */5 * * * *, Prompt: recurring"),
            None
        );

        let now = Utc::now();
        let mut tracker = BackgroundTaskTracker::default();
        tracker.record_started_with_expiry(
            "sess/task-expired".into(),
            Some("call_exp".into()),
            None,
            None,
            Some(now - chrono::Duration::seconds(300)),
        );
        tracker.record_started_with_expiry(
            "sess/task-live".into(),
            Some("call_live".into()),
            None,
            None,
            Some(now + chrono::Duration::seconds(300)),
        );
        tracker.record_started_with_expiry(
            "sess/task-shell".into(),
            Some("call_sh".into()),
            None,
            None,
            None,
        );
        let reaped = tracker.reap_expired_timers(now);
        assert_eq!(reaped.len(), 1);
        assert_eq!(reaped[0].task_id, "sess/task-expired");
        assert!(tracker.is_finished("sess/task-expired"));
        assert!(tracker.tasks.contains_key("sess/task-live"));
        assert!(tracker.tasks.contains_key("sess/task-shell"));
    }

    #[tokio::test]
    async fn agy_prompt_missing_trajectory_transparently_recreates_session_with_handoff() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixListener;

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_evicted_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).expect("bind mock agy shim");

        let received_prompts: Arc<tokio::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let received_bg = Arc::clone(&received_prompts);

        // Mock ACP server over shim UDS:
        // - `initialize` -> OK
        // - `session/load` for stale UUID -> silently returns OK (matching real `agy` bug!)
        // - `session/prompt` on stale UUID -> returns `Failed to send user message: not_found: trajectory not found: ...`
        // - `session/new` -> returns fresh UUID `fresh-sess-9999`
        // - `session/prompt` on fresh UUID -> streams assistant chunk and returns `stopReason: end_turn`
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq = 0u64;
            async fn send_stdout(
                wh: &mut tokio::net::unix::OwnedWriteHalf,
                seq: &mut u64,
                payload: Value,
            ) {
                *seq += 1;
                let frame = ShimFrame::StdoutLine {
                    line: payload.to_string(),
                    seq: Some(*seq),
                };
                let mut enc = serde_json::to_string(&frame).expect("encode frame");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
                let _ = wh.flush().await;
            }

            while let Ok(Some(raw)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let id = req.get("id").cloned().unwrap_or(Value::Null);
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"jsonrpc":"2.0","id":id,"result":{}}),
                        )
                        .await;
                    }
                    "session/load" => {
                        // Mimics real `agy`: returns OK even though trajectory file was evicted!
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "result":{
                                    "sessionId":"f8c809f0-5617-41ca-b27f-63bd7019833f",
                                    "models":{"currentModelId":"gemini-4-neon-xhigh"}
                                }
                            }),
                        )
                        .await;
                    }
                    "session/new" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "result":{
                                    "sessionId":"fresh-sess-9999",
                                    "models":{"currentModelId":"gemini-4-neon-xhigh"}
                                }
                            }),
                        )
                        .await;
                    }
                    "session/set_model" | "session/set_mode" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"jsonrpc":"2.0","id":id,"result":{}}),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        let sid = req
                            .pointer("/params/sessionId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let text = req
                            .pointer("/params/prompt/0/text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        received_bg.lock().await.push((sid.clone(), text));
                        if sid == "f8c809f0-5617-41ca-b27f-63bd7019833f" {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc":"2.0",
                                    "id":id,
                                    "error":{
                                        "code":-32000,
                                        "message":"Failed to send user message: not_found: trajectory not found: f8c809f0-5617-41ca-b27f-63bd7019833f: not found"
                                    }
                                }),
                            )
                            .await;
                        } else {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc":"2.0",
                                    "method":"session/update",
                                    "params":{
                                        "sessionId":sid,
                                        "update":{
                                            "sessionUpdate":"agent_message_chunk",
                                            "content":{"type":"text","text":"Recovered seamlessly!"}
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({"jsonrpc":"2.0","id":id,"result":{"stopReason":"end_turn"}}),
                            )
                            .await;
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
        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = SessionLaunchConfig {
            agent_id: "ag_evicted".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-neon".into()),
            effort: Some(ReasoningEffort::Xhigh),
            mode: SessionMode::FullAccess,
            resume_session_id: Some("f8c809f0-5617-41ca-b27f-63bd7019833f".into()),
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::clone(&seq_tracker),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: Some(
                "[Prior Conversation Context in this Workspace]\nUser: 设计免服登录\n\nAssistant: 已发布 v0.8.0"
                    .into(),
            ),
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
        };

        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));

        // Wait for initial Idle
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_recover_1".into(),
                prompt: "做一个友好的登录界面".into(),
                behavior: ActiveTurnBehavior::Steer,
            })
            .await
            .expect("send prompt");

        let mut saw_new_session = false;
        let mut saw_notice = false;
        let mut saw_reply = false;
        let mut saw_error = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn timeout")
                .expect("ev");
            match ev {
                DriverEvent::SessionInitialized {
                    persistence_ref, ..
                } if persistence_ref == "fresh-sess-9999" => {
                    saw_new_session = true;
                }
                DriverEvent::Notice { level, message }
                    if level == "info" && message.contains("f8c809f0") =>
                {
                    saw_notice = true;
                }
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "Recovered seamlessly!" => {
                    saw_reply = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Error { .. }) => {
                    saw_error = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }

        assert!(saw_new_session, "must persist fresh sessionId");
        assert!(
            saw_notice,
            "must inform user of transparent session recreation"
        );
        assert!(saw_reply, "must deliver recovered assistant reply");
        assert!(
            !saw_error,
            "must NOT surface trajectory-not-found error to user"
        );

        let prompts = received_prompts.lock().await;
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].0, "f8c809f0-5617-41ca-b27f-63bd7019833f");
        assert_eq!(prompts[1].0, "fresh-sess-9999");
        assert!(
            prompts[1]
                .1
                .contains("[Prior Conversation Context in this Workspace]")
        );
        assert!(prompts[1].1.contains("做一个友好的登录界面"));
        assert!(seq_tracker.load(Ordering::Relaxed) >= 4);

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn agy_reattach_restores_active_turn_segment_ids_and_settles_preflight_prompt() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::AsyncWriteExt;
        use tokio::net::UnixListener;

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_reattach_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).expect("bind mock reattached shim");

        // Simulate frames arriving after daemon hot-restart for an in-flight turn whose
        // original `session/prompt` RPC had `id: 7` (from before restart).
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let frames = [
                ShimFrame::AttachAck { latest_seq: 500 },
                ShimFrame::StdoutLine {
                    seq: Some(501),
                    line: json!({
                        "jsonrpc":"2.0",
                        "method":"session/update",
                        "params":{
                            "sessionId":"sess_live_reattach",
                            "update":{
                                "sessionUpdate":"agent_message_chunk",
                                "content":{"type":"text","text":"Continued after hot restart."}
                            }
                        }
                    })
                    .to_string(),
                },
                ShimFrame::StdoutLine {
                    seq: Some(502),
                    line: json!({
                        "jsonrpc":"2.0",
                        "id":7,
                        "result":{"stopReason":"end_turn"}
                    })
                    .to_string(),
                },
            ];
            for f in frames {
                let mut enc = serde_json::to_string(&f).expect("encode frame");
                enc.push('\n');
                let _ = stream.write_all(enc.as_bytes()).await;
            }
            let _ = stream.flush().await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        });

        let shim = orbit_shim::connect_existing_shim_after(&sock_path, Some(500))
            .await
            .expect("reattach");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let seq_tracker = Arc::new(AtomicU64::new(500));
        let cfg = SessionLaunchConfig {
            agent_id: "ag_reattach".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-carbon".into()),
            effort: Some(ReasoningEffort::High),
            mode: SessionMode::FullAccess,
            resume_session_id: Some("sess_live_reattach".into()),
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: true,
            shim_after_seq: Some(500),
            shim_seq_tracker: Arc::clone(&seq_tracker),
            persisted_active_turn_id: Some("turn_cbc687".into()),
            persisted_turn_step_seq: 8,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
        };

        let actor = tokio::spawn(run_agy_actor(cfg, shim, true, cmd_rx, evt_tx));

        let mut finalized_id = None;
        let mut settled_idle = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await {
            match ev {
                DriverEvent::AssistantText {
                    item_id,
                    text,
                    streaming: false,
                } => {
                    assert_eq!(text, "Continued after hot restart.");
                    finalized_id = Some(item_id);
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    settled_idle = true;
                    break;
                }
                _ => {}
            }
        }

        assert_eq!(
            finalized_id.as_deref(),
            Some("agy_msg_turn_cbc687_9"),
            "must continue persisted turn_id + step_seq instead of falling back to `agy_msg_init_1`"
        );
        assert!(
            settled_idle,
            "must recognize pre-restart session/prompt completion (`id: 7`) and transition to Idle"
        );
        assert_eq!(seq_tracker.load(Ordering::Relaxed), 502);

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }
}
