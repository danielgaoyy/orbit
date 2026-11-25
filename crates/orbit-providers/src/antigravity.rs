//! Native Rust driver for Google Antigravity (`agy acp --stdio` + `transcript_full.jsonl` monitor + `/usage`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentStatus, BackgroundTaskInfo, ProviderKind,
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
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::attachments::{AcpPromptMediaCaps, build_acp_prompt_blocks_async, is_path_within_dir};
use crate::driver::{
    DriverCommand, DriverEvent, PromptOrigin, ProviderSessionHandle, QueuedDriverPrompt, RpcId,
    SessionLaunchConfig, emit_dropped_queued_prompts, emit_prompt_delivery,
};

fn build_agy_launch_env(cfg: &SessionLaunchConfig) -> HashMap<String, String> {
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
    if cfg.fast_mode {
        env.insert("AGY_FAST_MODE".into(), "1".into());
    }
    crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);
    env
}

pub async fn spawn_antigravity_session(
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

        let env = build_agy_launch_env(&cfg);

        let spec = ShimSpawnSpec {
            sandbox: cfg.sandbox.clone(),
            egress_proxy_url: None,
            shim_id: format!("agy_{}", cfg.agent_id),
            mode: ShimMode::Stdio,
            program: cfg.executable.to_string_lossy().to_string(),
            args,
            cwd: cfg.cwd.clone(),
            env,
            cleanup_paths: Vec::new(),
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
    perm_options: crate::AcpPermissionOptions,
    is_question: bool,
    raw_questions: Vec<QuestionItem>,
}

struct PendingAgyPlanModeSwitch {
    target_mode: SessionMode,
    previous_mode: SessionMode,
    deferred_perm: Option<(PendingPerm, Value)>,
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

/// An in-flight `session/prompt` RPC superseded by a native non-destructive mid-turn steer
/// (`session/prompt` without `session/cancel`). `agy acp` completes the prior RPC immediately
/// with `stopReason: "end_turn"` while keeping running tools, subagents, and background tasks alive.
struct SupersededAgyPrompt {
    rpc_key: String,
    turn_id: Option<String>,
    message_ids: Vec<String>,
    dispatched_prompt: Option<QueuedDriverPrompt>,
}

#[derive(Default)]
struct AgyStreamSegmentState {
    turn_tag: String,
    step_seq: u64,
    active_msg_id: Option<String>,
    active_thought_id: Option<String>,
    msg_accum: HashMap<String, String>,
    thought_accum: HashMap<String, String>,
    latest_msg_text: Option<String>,
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
    let mut agy_meta = serde_json::Map::new();
    if let Some(append) = cfg
        .system_prompt_append
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        agy_meta.insert(
            "systemPrompt".to_string(),
            json!({
                "append": append
            }),
        );
    }
    if cfg.fast_mode {
        agy_meta.insert("fastMode".to_string(), Value::Bool(true));
    }
    if agy_meta.is_empty() {
        None
    } else {
        Some(json!({
            "antigravity": Value::Object(agy_meta)
        }))
    }
}

fn build_agy_set_fast_mode_request(rpc_id: i64, session_id: &str, enabled: bool) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": rpc_id,
        "method": "session/set_config_option",
        "params": {
            "sessionId": session_id,
            "configId": "fast_mode",
            "value": if enabled { "on" } else { "off" }
        }
    })
}

fn parse_agy_fast_mode_config_value(sess_val: &Value) -> Option<bool> {
    let opt = sess_val
        .get("configOptions")
        .and_then(|v| v.as_array())?
        .iter()
        .find(|opt| opt.get("id").and_then(|x| x.as_str()) == Some("fast_mode"))?;
    let current = opt.get("currentValue").or_else(|| opt.get("value"))?;
    if let Some(b) = current.as_bool() {
        return Some(b);
    }
    match current.as_str()?.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" | "enabled" => Some(true),
        "off" | "false" | "0" | "disabled" => Some(false),
        _ => None,
    }
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

fn prepend_interrupted_reply_handoff(handoff: Option<String>, prompt: &str) -> String {
    let Some(ctx) = handoff.filter(|c| !c.trim().is_empty()) else {
        return prompt.to_string();
    };
    if prompt.contains("[Interrupted Prior Turn Context]") {
        return prompt.to_string();
    }
    format!("{ctx}\n\n{prompt}")
}

fn snapshot_active_partial_reply(seg_state: &AgyStreamSegmentState) -> Option<String> {
    if let Some(ref mid) = seg_state.active_msg_id
        && let Some(text) = seg_state.msg_accum.get(mid)
    {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    seg_state
        .latest_msg_text
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn format_interrupted_reply_handoff(partial: &str) -> String {
    let clipped = truncate_preview(partial.trim(), 1200);
    format!(
        "[Interrupted Prior Turn Context]\n\
         In the previous turn you streamed the following partial reply to the user before being interrupted, and that uncommitted turn was rolled back from your history:\n\
         \"\"\"\n\
         {clipped}\n\
         \"\"\"\n\
         Do not repeat the above verbatim; continue directly from where you left off or address the user's new request below."
    )
}

fn planner_texts_committed_reply(committed: &[String], partial: &str) -> bool {
    let norm_partial = partial.split_whitespace().collect::<Vec<_>>().join(" ");
    if norm_partial.is_empty() {
        return true;
    }
    let char_count = norm_partial.chars().count();
    let needle: String = if char_count <= 24 {
        norm_partial
    } else {
        norm_partial.chars().take(80).collect()
    };
    committed.iter().any(|text| {
        let norm_committed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        norm_committed.contains(&needle)
    })
}

fn classify_agy_stderr_notice(line: &str) -> Option<(&'static str, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("no space left on device")
        || lower.contains("disk is full")
        || lower.contains("database or disk is full")
        || lower.contains("enospc")
    {
        return Some((
            "disk_full",
            format!(
                "Antigravity disk space error: {}",
                truncate_preview(trimmed, 300)
            ),
        ));
    }
    if lower.contains("read-only file system") {
        return Some((
            "readonly_fs",
            format!(
                "Antigravity filesystem error: {}",
                truncate_preview(trimmed, 300)
            ),
        ));
    }
    if lower.contains("cannot allocate memory") || lower.contains("out of memory") {
        return Some((
            "oom",
            format!(
                "Antigravity memory error: {}",
                truncate_preview(trimmed, 300)
            ),
        ));
    }
    None
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

    // Drain replay notifications on `session/load` and `session/fork` until 250ms of quiet (up to 8s ceiling)
    // so large monorepo trajectories never leak historical `session/update` frames into the next prompt turn.
    if drains_replay {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(250), shim.rx.recv()).await {
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

    // Reconcile `fast_mode` when ACP advertises a differing `configOptions` value (e.g. cold `session/load`)
    if let Some(advertised_fast) = parse_agy_fast_mode_config_value(&sess_val)
        && advertised_fast != cfg.fast_mode
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_config_option",
            json!({
                "sessionId": session_id,
                "configId": "fast_mode",
                "value": if cfg.fast_mode { "on" } else { "off" }
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
    mut cfg: SessionLaunchConfig,
    mut shim: orbit_shim::ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_rpc_id: i64 = if is_reattached { 100_000 } else { 1 };
    let mut pending_rpcs: HashMap<String, oneshot::Sender<Result<Value, Value>>> = HashMap::new();
    let mut inject_handoff_on_next_prompt = false;
    let mut prompt_media_caps = AcpPromptMediaCaps::antigravity();
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
                if !is_reattached {
                    let _ = shim.kill().await;
                }
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
                if !is_reattached {
                    let _ = shim.kill().await;
                }
                return;
            }
        };
        prompt_media_caps = AcpPromptMediaCaps::from_antigravity_initialize(&init_val);

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
                            if !is_reattached {
                                let _ = shim.kill().await;
                            }
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
                            message: format!("Antigravity session open failed: {e}"),
                            recoverable: false,
                        }))
                        .await;
                    if !is_reattached {
                        let _ = shim.kill().await;
                    }
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
    let mut pending_fast_mode_rpcs: HashMap<String, bool> = HashMap::new();
    let mut last_dispatched_prompt: Option<QueuedDriverPrompt> = None;
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
    let mut pending_plan_mode_switches: HashMap<String, PendingAgyPlanModeSwitch> = HashMap::new();
    let mut pending_elicits: HashMap<String, PendingElicitation> = HashMap::new();
    let mut pending_human_steers: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_prompts: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<QueuedDriverPrompt> = None;
    let mut superseded_prompt_rpcs: VecDeque<SupersededAgyPrompt> = VecDeque::new();
    let mut active_turn_message_ids: Vec<String> = Vec::new();
    let mut subagent_monitors: HashMap<String, AgySubagentMonitorEntry> = HashMap::new();

    // Post-turn autonomous wakeup (`transcript_full.jsonl`) + first-class background-task tracking
    let agy_data_dir = resolve_agy_data_dir_with_extra_env(&cfg.extra_env);
    let mut transcript_monitor = AgyTranscriptMonitor::from_brain_dir_with_session(
        agy_data_dir.join("brain").join(&session_id),
        &session_id,
    );
    // Skip any historical steps already recorded on disk when attaching/resuming, while:
    // 1. Remembering already-finished background task IDs so shim ring-buffer replay can never
    //    resurrect them,
    // 2. Reconciling stale persisted `Running` background tasks in `orbit.db` in place when the
    //    transcript confirms they already finished (or the shim was not reattached),
    // 3. Re-seeding genuinely still-running background tasks across daemon hot-upgrades so their
    //    canonical `call_id` closes cleanly when they finish later, and
    // 4. Registering historical native subagents so Overview / SubagentsTrack reflect real state.
    let historical_bg_deltas = transcript_monitor.sync_silent_with_bg_tasks();
    transcript_monitor.clear_committed_planner_texts();
    let mut wakeup_tools = WakeupToolRegistry::with_data_dir(agy_data_dir.clone());
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
                register_agy_subagent_monitor_in_dir(
                    &cfg.agent_id,
                    &subagent_id,
                    &role,
                    &prompt_preview,
                    true,
                    false,
                    Some(agy_data_dir.as_path()),
                    &mut subagent_monitors,
                    &evt_tx,
                )
                .await;
            }
            BgTaskDelta::SubagentMessaged { .. } => {}
            BgTaskDelta::SubagentsCanceled { ids } => {
                cancel_agy_subagent_monitors(ids.as_deref(), &mut subagent_monitors, &evt_tx).await;
            }
            BgTaskDelta::Compacted => {}
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
    let mut turn_was_cancelled = false;
    let mut cancelling_active_turn = false;
    let mut cancel_deadline: Option<tokio::time::Instant> = None;
    let mut pending_cancelled_partial_reply: Option<String> = None;
    let mut pending_interrupted_reply_handoff: Option<String> = None;
    let mut last_stderr_notice: HashMap<&'static str, Instant> = HashMap::new();
    let mut pending_self_heal: Option<PendingSelfHeal> = None;
    let mut consecutive_self_heals: u32 = 0;
    let mut last_ask_question_raw: Option<Vec<QuestionItem>> = None;
    let mut transcript_poll = tokio::time::interval(Duration::from_millis(200));
    let mut idle_poll_tick: u8 = 0;

    loop {
        tokio::select! {
            biased;
            maybe_cmd = cmd_rx.recv() => {
                idle_poll_tick = 0;
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        prompt,
                        behavior,
                        origin,
                        message_ids,
                    } => {
                        user_interrupted = false;
                        pending_self_heal = None;
                        consecutive_self_heals = 0;
                        if active_prompt_rpc_key.is_some() || active_turn_id.is_some() {
                            // Degrade agent-originated `Steer` to `Queue` so subagents/peers never
                            // inject mid-turn or disturb active human turns.
                            let effective_behavior = if origin == PromptOrigin::Agent
                                && matches!(behavior, ActiveTurnBehavior::Steer)
                            {
                                ActiveTurnBehavior::Queue
                            } else {
                                behavior
                            };
                            let blocking_tid = active_turn_id.clone();
                            if matches!(effective_behavior, ActiveTurnBehavior::Queue) {
                                // Always enqueue FIFO notifications/prompts even while awaiting permission
                                emit_prompt_delivery(
                                    &evt_tx,
                                    &turn_id,
                                    &message_ids,
                                    AgentMessageDeliveryStatus::Queued,
                                    blocking_tid,
                                )
                                .await;
                                pending_prompts.push_back(QueuedDriverPrompt {
                                    turn_id,
                                    prompt,
                                    message_ids,
                                });
                            } else if matches!(effective_behavior, ActiveTurnBehavior::Interrupt) {
                                cancel_pending_agy_perms_and_elicits(
                                    &shim,
                                    &mut pending_perms,
                                    &mut pending_elicits,
                                    &mut last_ask_question_raw,
                                )
                                .await;
                                if let Some(partial) = snapshot_active_partial_reply(&seg_state) {
                                    pending_cancelled_partial_reply = Some(partial);
                                }
                                finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                                settle_cancelled_turn_resources(
                                    &mut bg_tracker,
                                    &mut wakeup_tools,
                                    &mut subagent_monitors,
                                    &evt_tx,
                                )
                                .await;
                                while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                                    if let Some(ref tid) = sup.turn_id {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            tid,
                                            &sup.message_ids,
                                            AgentMessageDeliveryStatus::Settled,
                                            None,
                                        )
                                        .await;
                                    }
                                }
                                turn_was_cancelled = true;
                                cancelling_active_turn = true;
                                cancel_deadline = Some(
                                    tokio::time::Instant::now() + Duration::from_secs(10),
                                );
                                let cancel = json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/cancel",
                                    "params": { "sessionId": session_id }
                                })
                                .to_string();
                                if shim.write_stdin_line(cancel).await.is_err() {
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        None,
                                        &mut pending_human_steers,
                                    )
                                    .await;
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        pending_interrupt_replacement.take(),
                                        &mut pending_prompts,
                                    )
                                    .await;
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Dropped,
                                        None,
                                    )
                                    .await;
                                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                    break;
                                }
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
                            } else if !pending_perms.is_empty() || !pending_elicits.is_empty() {
                                emit_prompt_delivery(
                                    &evt_tx,
                                    &turn_id,
                                    &message_ids,
                                    AgentMessageDeliveryStatus::Dropped,
                                    None,
                                )
                                .await;
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "warning".into(),
                                        message: "Waiting for your permission or question response above. Please answer it (or stop the current turn) before sending a new prompt.".into(),
                                    })
                                    .await;
                            } else if cancelling_active_turn {
                                // An explicit Interrupt is already mid-flight; wait for `session/cancel`
                                // to settle before dispatching this prompt.
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
                            } else {
                                // `ActiveTurnBehavior::Steer` for human prompts: `agy acp` natively
                                // supports non-destructive mid-turn user message injection via a
                                // concurrent `session/prompt` WITHOUT `session/cancel`. Never send
                                // `session/cancel` or tear down `bg_tracker`/`subagent_monitors`,
                                // because `session/cancel` in `agy` force-kills ALL running
                                // subagents and background tasks/daemons!
                                let bg_deltas = transcript_monitor.poll_foreground_bg_deltas();
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
                                finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                                let prev_tid = active_turn_id.take();
                                let prev_msg_ids = std::mem::take(&mut active_turn_message_ids);
                                let prev_dispatched = last_dispatched_prompt.take();
                                if let Some(prev_rpc_key) = active_prompt_rpc_key.take() {
                                    superseded_prompt_rpcs.push_back(SupersededAgyPrompt {
                                        rpc_key: prev_rpc_key,
                                        turn_id: prev_tid,
                                        message_ids: prev_msg_ids,
                                        dispatched_prompt: prev_dispatched,
                                    });
                                } else {
                                    reattached_turn_in_flight = false;
                                    if let Some(ref tid) = prev_tid {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            tid,
                                            &prev_msg_ids,
                                            AgentMessageDeliveryStatus::Settled,
                                            None,
                                        )
                                        .await;
                                    }
                                }
                                let with_handoff = if inject_handoff_on_next_prompt {
                                    inject_handoff_on_next_prompt = false;
                                    prepend_handoff_context(pending_handoff_context.take(), &prompt)
                                } else {
                                    prompt
                                };
                                let effective_prompt = prepend_interrupted_reply_handoff(
                                    pending_interrupted_reply_handoff.take(),
                                    &with_handoff,
                                );
                                active_turn_message_ids = message_ids.clone();
                                last_dispatched_prompt = Some(QueuedDriverPrompt {
                                    turn_id: turn_id.clone(),
                                    prompt: effective_prompt.clone(),
                                    message_ids,
                                });
                                // Keep `wakeup_tools.current_turn_gen` intact so in-flight tools
                                // from the active turn continue binding to transcript bg task IDs.
                                if dispatch_agy_prompt(
                                    &shim,
                                    &session_id,
                                    cfg.attachments_dir.as_deref(),
                                    prompt_media_caps,
                                    &mut next_rpc_id,
                                    &mut active_turn_id,
                                    &mut active_prompt_rpc_key,
                                    &mut autonomous_turn_id,
                                    &mut transcript_monitor,
                                    &mut seg_state,
                                    &mut last_ask_question_raw,
                                    &evt_tx,
                                    turn_id.clone(),
                                    effective_prompt,
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
                                    while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                                        if let Some(ref tid) = sup.turn_id {
                                            emit_prompt_delivery(
                                                &evt_tx,
                                                tid,
                                                &sup.message_ids,
                                                AgentMessageDeliveryStatus::Dropped,
                                                None,
                                            )
                                            .await;
                                        }
                                    }
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        None,
                                        &mut pending_human_steers,
                                    )
                                    .await;
                                    emit_dropped_queued_prompts(
                                        &evt_tx,
                                        pending_interrupt_replacement.take(),
                                        &mut pending_prompts,
                                    )
                                    .await;
                                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
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
                            continue;
                        }

                        cancelling_active_turn = false;
                        turn_was_cancelled = false;
                        cancel_deadline = None;
                        if autonomous_turn_id.take().is_some() {
                            let bg_deltas = transcript_monitor.sync_silent_with_bg_tasks();
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
                        let with_handoff = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };
                        let effective_prompt = prepend_interrupted_reply_handoff(
                            pending_interrupted_reply_handoff.take(),
                            &with_handoff,
                        );
                        active_turn_message_ids = message_ids.clone();
                        last_dispatched_prompt = Some(QueuedDriverPrompt {
                            turn_id: turn_id.clone(),
                            prompt: effective_prompt.clone(),
                            message_ids,
                        });
                        wakeup_tools.begin_turn();
                        if dispatch_agy_prompt(
                            &shim,
                            &session_id,
                            cfg.attachments_dir.as_deref(),
                            prompt_media_caps,
                            &mut next_rpc_id,
                            &mut active_turn_id,
                            &mut active_prompt_rpc_key,
                            &mut autonomous_turn_id,
                            &mut transcript_monitor,
                            &mut seg_state,
                            &mut last_ask_question_raw,
                            &evt_tx,
                            turn_id.clone(),
                            effective_prompt,
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
                            emit_dropped_queued_prompts(
                                &evt_tx,
                                None,
                                &mut pending_human_steers,
                            )
                            .await;
                            emit_dropped_queued_prompts(
                                &evt_tx,
                                pending_interrupt_replacement.take(),
                                &mut pending_prompts,
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
                        user_interrupted = true;
                        pending_self_heal = None;
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        let dropped_rep = pending_interrupt_replacement.take();
                        emit_dropped_queued_prompts(&evt_tx, dropped_rep, &mut pending_prompts)
                            .await;
                        while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                            if let Some(ref tid) = sup.turn_id {
                                emit_prompt_delivery(
                                    &evt_tx,
                                    tid,
                                    &sup.message_ids,
                                    AgentMessageDeliveryStatus::Settled,
                                    None,
                                )
                                .await;
                            }
                        }
                        if let Some(partial) = snapshot_active_partial_reply(&seg_state) {
                            pending_cancelled_partial_reply = Some(partial);
                        }
                        finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                        let had_active =
                            active_prompt_rpc_key.is_some() || active_turn_id.is_some();
                        cancelling_active_turn = had_active;
                        if had_active {
                            turn_was_cancelled = true;
                            cancel_deadline =
                                Some(tokio::time::Instant::now() + Duration::from_secs(10));
                        }
                        settle_cancelled_turn_resources(
                            &mut bg_tracker,
                            &mut wakeup_tools,
                            &mut subagent_monitors,
                            &evt_tx,
                        )
                        .await;
                        if autonomous_turn_id.take().is_some() {
                            transcript_monitor.sync_silent();
                            if active_turn_id.is_none() {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                    .await;
                            }
                        }
                        cancel_pending_agy_perms_and_elicits(
                            &shim,
                            &mut pending_perms,
                            &mut pending_elicits,
                            &mut last_ask_question_raw,
                        )
                        .await;
                        let cancel = json!({
                            "jsonrpc": "2.0",
                            "method": "session/cancel",
                            "params": { "sessionId": session_id }
                        })
                        .to_string();
                        if shim.write_stdin_line(cancel).await.is_err() {
                            let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                            break;
                        }
                    }
                    DriverCommand::InterruptAndDrainQueue => {
                        user_interrupted = false;
                        pending_self_heal = None;
                        while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                            if let Some(ref tid) = sup.turn_id {
                                emit_prompt_delivery(
                                    &evt_tx,
                                    tid,
                                    &sup.message_ids,
                                    AgentMessageDeliveryStatus::Settled,
                                    None,
                                )
                                .await;
                            }
                        }
                        if let Some(partial) = snapshot_active_partial_reply(&seg_state) {
                            pending_cancelled_partial_reply = Some(partial);
                        }
                        finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                        cancel_pending_agy_perms_and_elicits(
                            &shim,
                            &mut pending_perms,
                            &mut pending_elicits,
                            &mut last_ask_question_raw,
                        )
                        .await;
                        settle_cancelled_turn_resources(
                            &mut bg_tracker,
                            &mut wakeup_tools,
                            &mut subagent_monitors,
                            &evt_tx,
                        )
                        .await;
                        let had_active =
                            active_prompt_rpc_key.is_some() || active_turn_id.is_some();
                        if had_active {
                            turn_was_cancelled = true;
                            cancelling_active_turn = true;
                            cancel_deadline =
                                Some(tokio::time::Instant::now() + Duration::from_secs(10));
                            let cancel = json!({
                                "jsonrpc": "2.0",
                                "method": "session/cancel",
                                "params": { "sessionId": session_id }
                            })
                            .to_string();
                            if shim.write_stdin_line(cancel).await.is_err() {
                                emit_dropped_queued_prompts(
                                    &evt_tx,
                                    None,
                                    &mut pending_human_steers,
                                )
                                .await;
                                emit_dropped_queued_prompts(
                                    &evt_tx,
                                    pending_interrupt_replacement.take(),
                                    &mut pending_prompts,
                                )
                                .await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                break;
                            }
                        } else if let Some(next) = pending_interrupt_replacement
                            .take()
                            .or_else(|| pending_human_steers.pop_front())
                            .or_else(|| pending_prompts.pop_front())
                        {
                            let with_handoff = if inject_handoff_on_next_prompt {
                                inject_handoff_on_next_prompt = false;
                                prepend_handoff_context(
                                    pending_handoff_context.take(),
                                    &next.prompt,
                                )
                            } else {
                                next.prompt
                            };
                            let effective_prompt = prepend_interrupted_reply_handoff(
                                pending_interrupted_reply_handoff.take(),
                                &with_handoff,
                            );
                            active_turn_message_ids = next.message_ids.clone();
                            last_dispatched_prompt = Some(QueuedDriverPrompt {
                                turn_id: next.turn_id.clone(),
                                prompt: effective_prompt.clone(),
                                message_ids: next.message_ids,
                            });
                            wakeup_tools.begin_turn();
                            if dispatch_agy_prompt(
                                &shim,
                                &session_id,
                                cfg.attachments_dir.as_deref(),
                                prompt_media_caps,
                                &mut next_rpc_id,
                                &mut active_turn_id,
                                &mut active_prompt_rpc_key,
                                &mut autonomous_turn_id,
                                &mut transcript_monitor,
                                &mut seg_state,
                                &mut last_ask_question_raw,
                                &evt_tx,
                                next.turn_id.clone(),
                                effective_prompt,
                            )
                            .await
                            .is_err()
                            {
                                let dropped = std::mem::take(&mut active_turn_message_ids);
                                emit_prompt_delivery(
                                    &evt_tx,
                                    &next.turn_id,
                                    &dropped,
                                    AgentMessageDeliveryStatus::Dropped,
                                    None,
                                )
                                .await;
                                break;
                            }
                            emit_prompt_delivery(
                                &evt_tx,
                                &next.turn_id,
                                &active_turn_message_ids,
                                AgentMessageDeliveryStatus::Dispatched,
                                None,
                            )
                            .await;
                        } else if autonomous_turn_id.take().is_some() {
                            transcript_monitor.sync_silent();
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                                .await;
                        }
                    }
                    DriverCommand::SetMode { mode: new_mode } => {
                        mode = new_mode.clone();
                        if !matches!(mode, SessionMode::FullAccess) {
                            let mode_str = match mode {
                                SessionMode::AcceptEdits => "accept-edits",
                                SessionMode::Plan => "plan",
                                // Auto (Guardian) uses approval-gating `default` mode so Orbit's
                                // daemon evaluates every permission request through Guardian.
                                SessionMode::Auto => "default",
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
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode,
                    } => {
                        let model_or_effort_changed = model.is_some() || effort.is_some();
                        if let Some(m) = model {
                            current_base_model =
                                Some(normalize_agy_base_model_id(&m, &canonical_base_ids));
                        }
                        if effort.is_some() {
                            active_effort = effort;
                        }
                        if model_or_effort_changed
                            && let Some(ref base_m) = current_base_model
                        {
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
                        if let Some(enabled) = fast_mode
                            && enabled != cfg.fast_mode
                        {
                            cfg.fast_mode = enabled;
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            pending_fast_mode_rpcs.insert(RpcId::Num(id).as_key(), enabled);
                            let req =
                                build_agy_set_fast_mode_request(id, &session_id, enabled)
                                    .to_string();
                            let _ = shim.write_stdin_line(req).await;
                        }
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(p) = pending_perms.remove(&request_id) {
                            let outcome = p.perm_options.outcome_for_decision(decision.as_str());
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": outcome
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
                    DriverCommand::ResolvePlanApproval {
                        request_id,
                        action,
                        edited_plan_markdown,
                        feedback: _,
                        target_mode,
                    } => {
                        let perm = pending_perms.remove(&request_id);
                        // Antigravity ACP permission responses do not carry inline edited plan
                        // bodies. When the user edited the plan on `ApproveAndBuild`, decline
                        // the stale unedited gate cleanly so the daemon starts one clean build
                        // turn carrying the approved edited plan text after mode switch succeeds.
                        let has_edits = edited_plan_markdown
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty());
                        let compute_outcome = |p: &PendingPerm| -> Value {
                            match action {
                                orbit_protocol::PlanApprovalAction::ApproveAndBuild
                                    if !has_edits =>
                                {
                                    p.perm_options.outcome_for_decision("allow")
                                }
                                _ => p.perm_options.deny_outcome(),
                            }
                        };

                        if matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild) {
                            if !matches!(target_mode, SessionMode::FullAccess) {
                                let mode_str = match target_mode {
                                    SessionMode::AcceptEdits => "accept-edits",
                                    SessionMode::Plan => "plan",
                                    SessionMode::Auto => "default",
                                    _ => "default",
                                };
                                let id = next_rpc_id;
                                next_rpc_id += 1;
                                let rpc_key = RpcId::Num(id).as_key();
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
                                if shim.write_stdin_line(req).await.is_err() {
                                    break;
                                }
                                let deferred_perm = perm.map(|p| {
                                    let outcome = compute_outcome(&p);
                                    (p, outcome)
                                });
                                pending_plan_mode_switches.insert(
                                    rpc_key,
                                    PendingAgyPlanModeSwitch {
                                        target_mode,
                                        previous_mode: mode.clone(),
                                        deferred_perm,
                                    },
                                );
                                continue;
                            }
                            mode = target_mode;
                        }

                        if let Some(p) = perm {
                            let outcome = compute_outcome(&p);
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": outcome
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
                                        "optionId": p.perm_options.question_option_id(),
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
                let Some(frame) = maybe_frame else {
                    while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                        if let Some(ref tid) = sup.turn_id {
                            emit_prompt_delivery(
                                &evt_tx,
                                tid,
                                &sup.message_ids,
                                AgentMessageDeliveryStatus::Dropped,
                                None,
                            )
                            .await;
                        }
                    }
                    emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers).await;
                    emit_dropped_queued_prompts(
                        &evt_tx,
                        pending_interrupt_replacement.take(),
                        &mut pending_prompts,
                    )
                    .await;
                    if let Some(ref tid) = active_turn_id {
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
                        let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                        let method = val.get("method").and_then(|v| v.as_str());

                        if has_id && method.is_none() {
                            // Response to our request (e.g. `session/prompt` completion)
                            if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone()) {
                                let key = rid.as_key();
                                if let Some(enabled) = pending_fast_mode_rpcs.remove(&key) {
                                    if val.get("error").filter(|v| !v.is_null()).is_some() {
                                        let state_label = if enabled { "on" } else { "off" };
                                        let _ = evt_tx
                                            .send(DriverEvent::Notice {
                                                level: "info".into(),
                                                message: format!(
                                                    "Fast mode ({state_label}) saved; takes effect on next Antigravity session restart."
                                                ),
                                            })
                                            .await;
                                    }
                                    continue;
                                }
                                if let Some(pending_switch) = pending_plan_mode_switches.remove(&key)
                                {
                                    if let Some(err_obj) =
                                        val.get("error").filter(|v| !v.is_null())
                                    {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("session/set_mode failed");
                                        mode = pending_switch.previous_mode;
                                        let _ = evt_tx
                                            .send(DriverEvent::Notice {
                                                level: "error".into(),
                                                message: format!(
                                                    "Failed to switch session mode before approving plan: {err_msg}"
                                                ),
                                            })
                                            .await;
                                        if let Some((p, _)) = pending_switch.deferred_perm {
                                            let resp = json!({
                                                "jsonrpc": "2.0",
                                                "id": p.rpc_id,
                                                "result": {
                                                    "outcome": p.perm_options.deny_outcome()
                                                }
                                            })
                                            .to_string();
                                            let _ = shim.write_stdin_line(resp).await;
                                            if let Some(ref tid) = active_turn_id {
                                                let _ = evt_tx
                                                    .send(DriverEvent::StatusChanged(
                                                        AgentStatus::Running {
                                                            turn_id: tid.clone(),
                                                        },
                                                    ))
                                                    .await;
                                            }
                                        }
                                    } else {
                                        mode = pending_switch.target_mode;
                                        if let Some((p, outcome)) = pending_switch.deferred_perm {
                                            let resp = json!({
                                                "jsonrpc": "2.0",
                                                "id": p.rpc_id,
                                                "result": { "outcome": outcome }
                                            })
                                            .to_string();
                                            let _ = shim.write_stdin_line(resp).await;
                                            if let Some(ref tid) = active_turn_id {
                                                let _ = evt_tx
                                                    .send(DriverEvent::StatusChanged(
                                                        AgentStatus::Running {
                                                            turn_id: tid.clone(),
                                                        },
                                                    ))
                                                    .await;
                                            }
                                        }
                                    }
                                    continue;
                                }
                                if post_reattach_non_prompt_rpcs.remove(&key) {
                                    continue;
                                }
                                if let Some(pos) = superseded_prompt_rpcs
                                    .iter()
                                    .position(|sup| sup.rpc_key == key)
                                {
                                    if let Some(sup) = superseded_prompt_rpcs.remove(pos)
                                        && let Some(ref tid) = sup.turn_id
                                    {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            tid,
                                            &sup.message_ids,
                                            AgentMessageDeliveryStatus::Settled,
                                            None,
                                        )
                                        .await;
                                    }
                                    continue;
                                }
                                let is_reattached_completion = reattached_turn_in_flight
                                    && active_prompt_rpc_key.is_none()
                                    && (val.pointer("/result/stopReason").is_some()
                                        || val.get("error").is_some());
                                if active_prompt_rpc_key.as_deref() == Some(key.as_str())
                                    || is_reattached_completion
                                {
                                    let stop_reason = val
                                        .pointer("/result/stopReason")
                                        .and_then(|v| v.as_str());
                                    let was_cancelled = cancelling_active_turn
                                        || turn_was_cancelled
                                        || stop_reason == Some("cancelled");
                                    // If a stricter/older server rejects our concurrent mid-turn
                                    // steer `session/prompt` with a JSON-RPC error while the
                                    // prior turn RPC is still open, fall back transparently to
                                    // non-destructive priority queueing with zero `session/cancel`.
                                    if !was_cancelled
                                        && let Some(err_obj) =
                                            val.get("error").filter(|v| !v.is_null())
                                        && !superseded_prompt_rpcs.is_empty()
                                    {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        if !is_agy_missing_trajectory_error(err_msg)
                                            && let Some(prev) = superseded_prompt_rpcs.pop_back()
                                            && let Some(rejected) = last_dispatched_prompt.take()
                                        {
                                            let rejected_turn_id = rejected.turn_id.clone();
                                            let rejected_msg_ids =
                                                std::mem::take(&mut active_turn_message_ids);
                                            active_prompt_rpc_key = Some(prev.rpc_key);
                                            active_turn_id = prev.turn_id.clone();
                                            active_turn_message_ids = prev.message_ids;
                                            last_dispatched_prompt = prev.dispatched_prompt;
                                            if let Some(ref restored_tid) = active_turn_id {
                                                seg_state.turn_tag = restored_tid.clone();
                                                let _ = evt_tx
                                                    .send(DriverEvent::StatusChanged(
                                                        AgentStatus::Running {
                                                            turn_id: restored_tid.clone(),
                                                        },
                                                    ))
                                                    .await;
                                            }
                                            emit_prompt_delivery(
                                                &evt_tx,
                                                &rejected_turn_id,
                                                &rejected_msg_ids,
                                                AgentMessageDeliveryStatus::Queued,
                                                active_turn_id.clone(),
                                            )
                                            .await;
                                            pending_human_steers.push_front(rejected);
                                            continue;
                                        }
                                    }
                                    let reconcile_in_place_only =
                                        was_cancelled || pending_interrupt_replacement.is_some();
                                    reattached_turn_in_flight = false;
                                    cancelling_active_turn = false;
                                    turn_was_cancelled = false;
                                    cancel_deadline = None;
                                    active_prompt_rpc_key = None;
                                    pending_perms.clear();
                                    pending_plan_mode_switches.clear();
                                    pending_elicits.clear();
                                    last_ask_question_raw = None;
                                    // Finalize any streaming message buffers
                                    finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                                    let finished_turn_id = active_turn_id.take();

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
                                            && let Some(failed_q) = last_dispatched_prompt.take()
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
                                                        &failed_q.prompt,
                                                    );
                                                    last_dispatched_prompt =
                                                        Some(QueuedDriverPrompt {
                                                            turn_id: failed_q.turn_id.clone(),
                                                            prompt: recovered_prompt.clone(),
                                                            message_ids: failed_q.message_ids,
                                                        });
                                                    wakeup_tools.begin_turn();
                                                    if dispatch_agy_prompt(
                                                        &shim,
                                                        &session_id,
                                                        cfg.attachments_dir.as_deref(),
                                                        prompt_media_caps,
                                                        &mut next_rpc_id,
                                                        &mut active_turn_id,
                                                        &mut active_prompt_rpc_key,
                                                        &mut autonomous_turn_id,
                                                        &mut transcript_monitor,
                                                        &mut seg_state,
                                                        &mut last_ask_question_raw,
                                                        &evt_tx,
                                                        failed_q.turn_id.clone(),
                                                        recovered_prompt,
                                                    )
                                                    .await
                                                    .is_err()
                                                    {
                                                        let dropped = std::mem::take(
                                                            &mut active_turn_message_ids,
                                                        );
                                                        emit_prompt_delivery(
                                                            &evt_tx,
                                                            &failed_q.turn_id,
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
                                                            &mut pending_prompts,
                                                        )
                                                        .await;
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
                                    let settled_msg_ids =
                                        std::mem::take(&mut active_turn_message_ids);
                                    if let Some(ref tid) = finished_turn_id {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            tid,
                                            &settled_msg_ids,
                                            AgentMessageDeliveryStatus::Settled,
                                            None,
                                        )
                                        .await;
                                    }

                                    // Fast-forward transcript reader past foreground steps while still
                                    // capturing any newly spawned/completed background task IDs.
                                    let (bg_deltas, committed_texts) =
                                        transcript_monitor.sync_silent_for_turn_end();
                                    if was_cancelled
                                        && let Some(partial) = pending_cancelled_partial_reply.take()
                                        && transcript_monitor.active_path().is_some()
                                        && !planner_texts_committed_reply(
                                            &committed_texts,
                                            &partial,
                                        )
                                    {
                                        pending_interrupted_reply_handoff =
                                            Some(format_interrupted_reply_handoff(&partial));
                                        if pending_interrupt_replacement.is_none() {
                                            let _ = evt_tx
                                                .send(DriverEvent::Notice {
                                                    level: "info".into(),
                                                    message: "Interrupted before completion; partial reply context will be carried into your next turn.".into(),
                                                })
                                                .await;
                                        }
                                    } else {
                                        pending_cancelled_partial_reply = None;
                                    }
                                    apply_foreground_bg_deltas_with_mode(
                                        &cfg.agent_id,
                                        bg_deltas,
                                        reconcile_in_place_only,
                                        was_cancelled,
                                        &mut wakeup_tools,
                                        &mut bg_tracker,
                                        &mut tool_cache,
                                        &mut subagent_monitors,
                                        &evt_tx,
                                    )
                                    .await;
                                    if was_cancelled {
                                        settle_cancelled_turn_resources(
                                            &mut bg_tracker,
                                            &mut wakeup_tools,
                                            &mut subagent_monitors,
                                            &evt_tx,
                                        )
                                        .await;
                                    } else {
                                        // Settle any non-background foreground tools left open when the turn ended
                                        let active_bg_calls = bg_tracker.active_call_ids();
                                        let settle_status = if val.get("error").is_some() {
                                            ToolCallStatus::Failed
                                        } else {
                                            ToolCallStatus::Completed
                                        };
                                        if reconcile_in_place_only {
                                            let settlements: Vec<(String, ToolCallStatus)> =
                                                wakeup_tools
                                                    .settle_open_tools(&active_bg_calls)
                                                    .into_iter()
                                                    .map(|(cid, _, _)| (cid, settle_status))
                                                    .collect();
                                            if !settlements.is_empty() {
                                                let _ = evt_tx
                                                    .send(DriverEvent::ReconcileBackgroundTasks(
                                                        settlements,
                                                    ))
                                                    .await;
                                            }
                                        } else {
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
                                        }
                                    }
                                    // Drain interrupted-replacement, priority human steer, or accepted FIFO queued prompt
                                    if let Some(next_q) = pending_interrupt_replacement
                                        .take()
                                        .or_else(|| pending_human_steers.pop_front())
                                        .or_else(|| pending_prompts.pop_front())
                                    {
                                        let next_prompt = prepend_interrupted_reply_handoff(
                                            pending_interrupted_reply_handoff.take(),
                                            &next_q.prompt,
                                        );
                                        active_turn_message_ids = next_q.message_ids.clone();
                                        last_dispatched_prompt = Some(QueuedDriverPrompt {
                                            turn_id: next_q.turn_id.clone(),
                                            prompt: next_prompt.clone(),
                                            message_ids: next_q.message_ids,
                                        });
                                        wakeup_tools.begin_turn();
                                        if dispatch_agy_prompt(
                                            &shim,
                                            &session_id,
                                            cfg.attachments_dir.as_deref(),
                                            prompt_media_caps,
                                            &mut next_rpc_id,
                                            &mut active_turn_id,
                                            &mut active_prompt_rpc_key,
                                            &mut autonomous_turn_id,
                                            &mut transcript_monitor,
                                            &mut seg_state,
                                            &mut last_ask_question_raw,
                                            &evt_tx,
                                            next_q.turn_id.clone(),
                                            next_prompt,
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
                                                &mut pending_prompts,
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
                                    } else if !user_interrupted
                                        && !was_cancelled
                                        && !bg_tracker.is_empty()
                                    {
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
                                if !cancelling_active_turn
                                    && !in_post_turn_wakeup
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
                                let allow_update = !cancelling_active_turn
                                    && (!in_post_turn_wakeup
                                        || matches!(
                                            update_kind,
                                            "tool_call" | "tool_call_update"
                                        ));
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
                                &cfg.cwd,
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
                    ShimFrame::StderrLine { line, .. } => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            if let Some((category, notice_msg)) = classify_agy_stderr_notice(trimmed)
                            {
                                warn!(
                                    agent_id = %cfg.agent_id,
                                    session_id = %session_id,
                                    stderr_category = category,
                                    stderr = %trimmed,
                                    "Antigravity runtime reported fatal stderr error"
                                );
                                let now = Instant::now();
                                let should_emit = last_stderr_notice
                                    .get(category)
                                    .is_none_or(|prev| {
                                        now.duration_since(*prev) >= Duration::from_secs(30)
                                    });
                                if should_emit {
                                    last_stderr_notice.insert(category, now);
                                    let _ = evt_tx
                                        .send(DriverEvent::Notice {
                                            level: "error".into(),
                                            message: notice_msg,
                                        })
                                        .await;
                                }
                            } else {
                                let lower = trimmed.to_ascii_lowercase();
                                if lower.contains("error")
                                    || lower.contains("fatal")
                                    || lower.contains("panic")
                                {
                                    warn!(
                                        agent_id = %cfg.agent_id,
                                        session_id = %session_id,
                                        stderr = %trimmed,
                                        "Antigravity stderr"
                                    );
                                } else {
                                    tracing::debug!(
                                        agent_id = %cfg.agent_id,
                                        session_id = %session_id,
                                        stderr = %trimmed,
                                        "Antigravity stderr"
                                    );
                                }
                            }
                        }
                    }
                    ShimFrame::Exited { code } => {
                        while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                            if let Some(ref tid) = sup.turn_id {
                                emit_prompt_delivery(
                                    &evt_tx,
                                    tid,
                                    &sup.message_ids,
                                    AgentMessageDeliveryStatus::Dropped,
                                    None,
                                )
                                .await;
                            }
                        }
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut pending_prompts,
                        )
                        .await;
                        if let Some(ref tid) = active_turn_id {
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
                        if code != Some(0) {
                            warn!(
                                agent_id = %cfg.agent_id,
                                session_id = %session_id,
                                exit_code = ?code,
                                "Antigravity shim process exited"
                            );
                        } else {
                            info!(
                                agent_id = %cfg.agent_id,
                                session_id = %session_id,
                                exit_code = ?code,
                                "Antigravity shim process exited cleanly"
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
                    .any(|entry| entry.desc.status == ProviderSubagentStatus::Running);
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

                // Poll native Antigravity subagent transcripts for read-only Subagents Track timelines.
                // Non-Running subagents are checked cheaply via `has_unconsumed_growth()` so resumed
                // subagents (e.g. via `send_message` or after reconnect/compaction) self-heal back to
                // `Running` and stream follow-up turns automatically.
                for (sub_id, entry) in subagent_monitors.iter_mut() {
                    if entry.desc.status != ProviderSubagentStatus::Running
                        && !entry.monitor.has_unconsumed_growth()
                    {
                        continue;
                    }
                    poll_agy_subagent_monitor(sub_id, entry, &evt_tx).await;
                }

                // Surface newly spawned subagents & background tasks even while foreground turn runs
                // continuously without emitting assistant prose, and monitor post-turn wakeups when idle.
                if !cancelling_active_turn {
                    if active_turn_id.is_some() {
                        let bg_deltas = transcript_monitor.poll_foreground_bg_deltas();
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
                    } else {
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
                                command,
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
                                        wakeup_tools.find_matching_call_id_for_bg_task(
                                            description.as_deref(),
                                            command.as_deref(),
                                        )
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
                                        register_agy_subagent_monitor_in_dir(
                                            &cfg.agent_id,
                                            child_conv_id,
                                            role,
                                            prompt_preview,
                                            false,
                                            false,
                                            Some(agy_data_dir.as_path()),
                                            &mut subagent_monitors,
                                            &evt_tx,
                                        )
                                        .await;
                                    } else if let Some(recipient_id) =
                                        extract_send_message_recipient_from_detail(&emit_detail)
                                    {
                                        resume_agy_subagent_monitor(
                                            &recipient_id,
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
                                register_agy_subagent_monitor_in_dir(
                                    &cfg.agent_id,
                                    &subagent_id,
                                    &role,
                                    &prompt_preview,
                                    false,
                                    false,
                                    Some(agy_data_dir.as_path()),
                                    &mut subagent_monitors,
                                    &evt_tx,
                                )
                                .await;
                            }
                            WakeupStep::SubagentMessaged { subagent_id } => {
                                resume_agy_subagent_monitor(
                                    &subagent_id,
                                    &mut subagent_monitors,
                                    &evt_tx,
                                )
                                .await;
                            }
                            WakeupStep::SubagentsCanceled { ids } => {
                                cancel_agy_subagent_monitors(
                                    ids.as_deref(),
                                    &mut subagent_monitors,
                                    &evt_tx,
                                )
                                .await;
                            }
                            WakeupStep::UserMessage { .. } => {}
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
                                active_turn_message_ids.clear();
                                wakeup_tools.begin_turn();
                        if dispatch_agy_prompt(
                                    &shim,
                                    &session_id,
                                    cfg.attachments_dir.as_deref(),
                                    prompt_media_caps,
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

            _ = async {
                if let Some(deadline) = cancel_deadline {
                    tokio::time::sleep_until(deadline).await;
                } else {
                    std::future::pending::<()>().await;
                }
            }, if cancel_deadline.is_some() => {
                cancel_deadline = None;
                cancelling_active_turn = false;
                turn_was_cancelled = false;
                reattached_turn_in_flight = false;
                active_prompt_rpc_key = None;
                let timed_out_tid = active_turn_id.take();
                let settled_msg_ids = std::mem::take(&mut active_turn_message_ids);
                if let Some(ref tid) = timed_out_tid {
                    emit_prompt_delivery(
                        &evt_tx,
                        tid,
                        &settled_msg_ids,
                        AgentMessageDeliveryStatus::Settled,
                        None,
                    )
                    .await;
                }
                pending_perms.clear();
                pending_elicits.clear();
                last_ask_question_raw = None;
                finalize_agy_stream_segments(&mut seg_state, &evt_tx).await;
                let (bg_deltas, committed_texts) = transcript_monitor.sync_silent_for_turn_end();
                if let Some(partial) = pending_cancelled_partial_reply.take()
                    && transcript_monitor.active_path().is_some()
                    && !planner_texts_committed_reply(&committed_texts, &partial)
                {
                    pending_interrupted_reply_handoff =
                        Some(format_interrupted_reply_handoff(&partial));
                }
                apply_foreground_bg_deltas_with_mode(
                    &cfg.agent_id,
                    bg_deltas,
                    true,
                    true,
                    &mut wakeup_tools,
                    &mut bg_tracker,
                    &mut tool_cache,
                    &mut subagent_monitors,
                    &evt_tx,
                )
                .await;
                settle_cancelled_turn_resources(
                    &mut bg_tracker,
                    &mut wakeup_tools,
                    &mut subagent_monitors,
                    &evt_tx,
                )
                .await;
                let _ = evt_tx
                    .send(DriverEvent::Notice {
                        level: "warning".into(),
                        message: "Antigravity cancel timed out waiting for turn settlement; resetting active turn state.".into(),
                    })
                    .await;
                while let Some(sup) = superseded_prompt_rpcs.pop_front() {
                    if let Some(ref tid) = sup.turn_id {
                        emit_prompt_delivery(
                            &evt_tx,
                            tid,
                            &sup.message_ids,
                            AgentMessageDeliveryStatus::Settled,
                            None,
                        )
                        .await;
                    }
                }
                if let Some(next_q) = pending_interrupt_replacement
                    .take()
                    .or_else(|| pending_human_steers.pop_front())
                    .or_else(|| pending_prompts.pop_front())
                {
                    let next_prompt = prepend_interrupted_reply_handoff(
                        pending_interrupted_reply_handoff.take(),
                        &next_q.prompt,
                    );
                    active_turn_message_ids = next_q.message_ids.clone();
                    last_dispatched_prompt = Some(QueuedDriverPrompt {
                        turn_id: next_q.turn_id.clone(),
                        prompt: next_prompt.clone(),
                        message_ids: next_q.message_ids,
                    });
                    wakeup_tools.begin_turn();
                    if dispatch_agy_prompt(
                        &shim,
                        &session_id,
                        cfg.attachments_dir.as_deref(),
                        prompt_media_caps,
                        &mut next_rpc_id,
                        &mut active_turn_id,
                        &mut active_prompt_rpc_key,
                        &mut autonomous_turn_id,
                        &mut transcript_monitor,
                        &mut seg_state,
                        &mut last_ask_question_raw,
                        &evt_tx,
                        next_q.turn_id.clone(),
                        next_prompt,
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
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut pending_prompts,
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
                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                        .await;
                }
            }
        }
    }
}

async fn settle_cancelled_turn_resources(
    bg_tracker: &mut BackgroundTaskTracker,
    wakeup_tools: &mut WakeupToolRegistry,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let mut settlements: Vec<(String, ToolCallStatus)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for cid in bg_tracker.drain_and_finish_call_ids() {
        let _ = wakeup_tools.finish_tool(&cid, ToolCallStatus::Failed);
        if seen.insert(cid.clone()) {
            settlements.push((cid, ToolCallStatus::Failed));
        }
    }
    for (cid, _, _) in wakeup_tools.settle_open_tools(&HashSet::new()) {
        if seen.insert(cid.clone()) {
            settlements.push((cid, ToolCallStatus::Failed));
        }
    }
    if !settlements.is_empty() {
        let _ = evt_tx
            .send(DriverEvent::ReconcileBackgroundTasks(settlements))
            .await;
    }
    cancel_agy_subagent_monitors(None, subagent_monitors, evt_tx).await;
}

#[allow(clippy::too_many_arguments)]
async fn apply_foreground_bg_deltas(
    parent_agent_id: &str,
    bg_deltas: Vec<BgTaskDelta>,
    wakeup_tools: &mut WakeupToolRegistry,
    bg_tracker: &mut BackgroundTaskTracker,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    apply_foreground_bg_deltas_with_mode(
        parent_agent_id,
        bg_deltas,
        false,
        false,
        wakeup_tools,
        bg_tracker,
        tool_cache,
        subagent_monitors,
        evt_tx,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn apply_foreground_bg_deltas_with_mode(
    parent_agent_id: &str,
    bg_deltas: Vec<BgTaskDelta>,
    reconcile_in_place_only: bool,
    turn_cancelled: bool,
    wakeup_tools: &mut WakeupToolRegistry,
    bg_tracker: &mut BackgroundTaskTracker,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let mut in_place_settlements: Vec<(String, ToolCallStatus)> = Vec::new();
    for delta in bg_deltas {
        match delta {
            BgTaskDelta::Started {
                task_id,
                call_id,
                description,
                log_path,
                expires_at,
                command,
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
                        wakeup_tools.find_matching_call_id_for_bg_task(
                            description.as_deref(),
                            command.as_deref(),
                        )
                    });
                if reconcile_in_place_only {
                    if let Some(cid) = resolved_cid {
                        let _ = wakeup_tools.mark_background_task(&cid, bg_info.clone());
                        bg_tracker.record_started_with_expiry(
                            task_id,
                            Some(cid),
                            log_path,
                            Some(bg_info),
                            expires_at,
                        );
                    } else if !turn_cancelled {
                        bg_tracker.record_started_with_expiry(
                            task_id,
                            None,
                            log_path,
                            Some(bg_info),
                            expires_at,
                        );
                    }
                    continue;
                }
                let (final_cid, emit_detail, emit_bg_info) = if let Some(cid) = resolved_cid {
                    match wakeup_tools.mark_background_task(&cid, bg_info.clone()) {
                        Some((det, merged_bg, true)) => (cid, Some(det), merged_bg),
                        Some((_, merged_bg, false)) => (cid, None, merged_bg),
                        None => (cid, None, bg_info.clone()),
                    }
                } else {
                    let synth_id = format!("bg_task_{}", sanitize_bg_task_id(&task_id));
                    let desc_text = command
                        .or_else(|| description.clone())
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
                        if reconcile_in_place_only {
                            in_place_settlements.push((cid.clone(), final_st));
                        } else {
                            let _ = evt_tx
                                .send(DriverEvent::ToolCall {
                                    call_id: cid.clone(),
                                    status: final_st,
                                    detail,
                                    background_task: bg_info,
                                })
                                .await;
                        }
                    } else if reconcile_in_place_only {
                        in_place_settlements.push((cid.clone(), final_st));
                    }
                }
            }
            BgTaskDelta::SubagentSpawned {
                subagent_id,
                role,
                prompt_preview,
            } => {
                let data_dir_opt = wakeup_tools.agy_data_dir.clone();
                register_agy_subagent_monitor_in_dir(
                    parent_agent_id,
                    &subagent_id,
                    &role,
                    &prompt_preview,
                    false,
                    turn_cancelled,
                    data_dir_opt.as_deref(),
                    subagent_monitors,
                    evt_tx,
                )
                .await;
            }
            BgTaskDelta::SubagentMessaged { subagent_id } => {
                if !turn_cancelled {
                    resume_agy_subagent_monitor(&subagent_id, subagent_monitors, evt_tx).await;
                }
            }
            BgTaskDelta::SubagentsCanceled { ids } => {
                cancel_agy_subagent_monitors(ids.as_deref(), subagent_monitors, evt_tx).await;
            }
            BgTaskDelta::Compacted => {
                let _ = evt_tx
                    .send(DriverEvent::Notice {
                        level: "info".into(),
                        message: "Context compacted automatically".into(),
                    })
                    .await;
            }
        }
    }
    if !in_place_settlements.is_empty() {
        let _ = evt_tx
            .send(DriverEvent::ReconcileBackgroundTasks(in_place_settlements))
            .await;
    }
}

async fn finalize_agy_stream_segments(
    seg_state: &mut AgyStreamSegmentState,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    seg_state.active_msg_id = None;
    seg_state.active_thought_id = None;
    for (mid, text) in seg_state.msg_accum.drain() {
        let _ = evt_tx
            .send(DriverEvent::AssistantText {
                item_id: mid,
                text,
                streaming: false,
            })
            .await;
    }
}

async fn cancel_pending_agy_perms_and_elicits(
    shim: &orbit_shim::ShimClient,
    pending_perms: &mut HashMap<String, PendingPerm>,
    pending_elicits: &mut HashMap<String, PendingElicitation>,
    last_ask_question_raw: &mut Option<Vec<QuestionItem>>,
) {
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
    *last_ask_question_raw = None;
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_agy_prompt(
    shim: &orbit_shim::ShimClient,
    session_id: &str,
    attachments_dir: Option<&Path>,
    prompt_media_caps: AcpPromptMediaCaps,
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
    transcript_monitor.clear_committed_planner_texts();
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

    let prompt_blocks = build_acp_prompt_blocks_async(
        prompt,
        attachments_dir.map(Path::to_path_buf),
        prompt_media_caps,
    )
    .await;
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
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
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
        "session_info_update" => {
            let title = update
                .get("title")
                .or_else(|| update.pointer("/info/title"))
                .or_else(|| update.pointer("/sessionInfo/title"))
                .and_then(|v| v.as_str())
                .map(|raw| raw.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|s| !s.is_empty());
            if let Some(title) = title {
                let _ = evt_tx
                    .send(DriverEvent::SessionTitleUpdated {
                        title: truncate_preview(&title, 120),
                    })
                    .await;
            }
            None
        }
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
            seg_state.latest_msg_text = Some(entry.clone());
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
                let data_dir_opt = wakeup_tools.agy_data_dir.clone();
                register_agy_subagent_monitor_in_dir(
                    parent_agent_id,
                    child_conv_id,
                    role,
                    prompt_preview,
                    false,
                    false,
                    data_dir_opt.as_deref(),
                    subagent_monitors,
                    evt_tx,
                )
                .await;
            } else if let Some(recipient_id) = extract_send_message_recipient_from_detail(&detail)
                .or_else(|| {
                    let title = update.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    if is_send_message_tool_name(title) {
                        transcript_arg_str(
                            update.get("rawInput"),
                            &[
                                "Recipient",
                                "recipient",
                                "conversationId",
                                "conversation_id",
                                "agent_id",
                            ],
                        )
                    } else {
                        None
                    }
                })
            {
                resume_agy_subagent_monitor(&recipient_id, subagent_monitors, evt_tx).await;
            } else if effective_status == ToolCallStatus::Completed {
                let title = update.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let is_manage = is_manage_subagents_tool_name(title)
                    || matches!(
                        &detail,
                        ToolCallDetail::Generic { tool_name, .. }
                            if is_manage_subagents_tool_name(tool_name)
                    );
                if is_manage {
                    let raw_out_text = extract_acp_raw_output_text(update.get("rawOutput"));
                    if let Some(target_ids) = extract_manage_subagents_cancel(
                        update.get("rawInput"),
                        raw_out_text.as_deref(),
                    ) {
                        cancel_agy_subagent_monitors(
                            target_ids.as_deref(),
                            subagent_monitors,
                            evt_tx,
                        )
                        .await;
                    }
                }
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

fn extract_system_message_content(raw: &str) -> Option<String> {
    let idx = raw.find("content=")?;
    let body = raw[idx + "content=".len()..]
        .trim_end()
        .trim_end_matches("</SYSTEM_MESSAGE>")
        .trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn is_send_message_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    let without_run = trimmed
        .strip_prefix("Run ")
        .or_else(|| trimmed.strip_prefix("run "))
        .unwrap_or(trimmed);
    let lower_without_run = without_run.to_ascii_lowercase();
    if lower_without_run == "send message" || lower_without_run.starts_with("send message ") {
        return true;
    }
    let first_tok = without_run.split_whitespace().next().unwrap_or("");
    let leaf = first_tok
        .rsplit([':', '/', '.'])
        .next()
        .unwrap_or(first_tok)
        .rsplit("__")
        .next()
        .unwrap_or(first_tok);
    leaf.eq_ignore_ascii_case("send_message") || leaf.eq_ignore_ascii_case("sendmessage")
}

fn is_manage_subagents_tool_name(name: &str) -> bool {
    let trimmed = name.trim();
    let without_run = trimmed
        .strip_prefix("Run ")
        .or_else(|| trimmed.strip_prefix("run "))
        .unwrap_or(trimmed);
    let first_tok = without_run.split_whitespace().next().unwrap_or("");
    let leaf = first_tok
        .rsplit([':', '/', '.'])
        .next()
        .unwrap_or(first_tok)
        .rsplit("__")
        .next()
        .unwrap_or(first_tok);
    leaf.eq_ignore_ascii_case("manage_subagents") || leaf.eq_ignore_ascii_case("managesubagents")
}

fn extract_uuids_from_text(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    if bytes.len() < 36 {
        return out;
    }
    let is_hex = |b: u8| b.is_ascii_hexdigit();
    let mut i = 0;
    while i + 36 <= bytes.len() {
        let slice = &bytes[i..i + 36];
        let matches_shape = slice[8] == b'-'
            && slice[13] == b'-'
            && slice[18] == b'-'
            && slice[23] == b'-'
            && slice[0..8].iter().all(|&b| is_hex(b))
            && slice[9..13].iter().all(|&b| is_hex(b))
            && slice[14..18].iter().all(|&b| is_hex(b))
            && slice[19..23].iter().all(|&b| is_hex(b))
            && slice[24..36].iter().all(|&b| is_hex(b));
        if matches_shape {
            let left_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let right_ok = i + 36 == bytes.len() || !bytes[i + 36].is_ascii_alphanumeric();
            if left_ok
                && right_ok
                && let Ok(s) = std::str::from_utf8(slice)
            {
                let id = s.to_string();
                if !out.contains(&id) {
                    out.push(id);
                }
                i += 36;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn extract_manage_subagents_cancel(
    args: Option<&Value>,
    text_fallback: Option<&str>,
) -> Option<Option<Vec<String>>> {
    let action = transcript_arg_str(args, &["Action", "action"])
        .unwrap_or_default()
        .to_ascii_lowercase();
    if action == "kill_all" {
        return Some(None);
    }
    if action != "kill" {
        return None;
    }
    let mut ids: Vec<String> = args
        .and_then(|a| {
            a.get("ConversationIds")
                .or_else(|| a.get("conversation_ids"))
                .and_then(|v| v.as_array())
        })
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty()
        && let Some(single) = transcript_arg_str(args, &["ConversationId", "conversation_id"])
    {
        ids.push(single);
    }
    if ids.is_empty()
        && let Some(txt) = text_fallback
    {
        ids = extract_uuids_from_text(txt);
    }
    if ids.is_empty() {
        None
    } else {
        Some(Some(ids))
    }
}

fn extract_send_message_recipient_from_detail(detail: &ToolCallDetail) -> Option<String> {
    let ToolCallDetail::Generic {
        tool_name,
        arguments_json,
    } = detail
    else {
        return None;
    };
    if !is_send_message_tool_name(tool_name) {
        return None;
    }
    let parsed: Value = serde_json::from_str(arguments_json).ok()?;
    transcript_arg_str(
        Some(&parsed),
        &[
            "Recipient",
            "recipient",
            "conversationId",
            "conversation_id",
            "agent_id",
        ],
    )
}

struct AgySubagentMonitorEntry {
    desc: ProviderSubagentDescriptor,
    monitor: AgyTranscriptMonitor,
    bg_tasks: HashMap<String, (String, ToolCallDetail, Option<BackgroundTaskInfo>)>,
    open_tools: HashMap<String, (ToolCallDetail, Option<BackgroundTaskInfo>)>,
    last_running_tool: Option<(String, ToolCallDetail)>,
}

impl AgySubagentMonitorEntry {
    fn new(desc: ProviderSubagentDescriptor, monitor: AgyTranscriptMonitor) -> Self {
        Self {
            desc,
            monitor,
            bg_tasks: HashMap::new(),
            open_tools: HashMap::new(),
            last_running_tool: None,
        }
    }

    fn drain_open_tool_timelines(&mut self, final_status: ToolCallStatus) -> Vec<TimelinePayload> {
        let mut seen_call_ids = HashSet::new();
        let mut out = Vec::new();
        let mut open: Vec<_> = self.open_tools.drain().collect();
        open.sort_by(|a, b| a.0.cmp(&b.0));
        for (call_id, (detail, background_task)) in open {
            seen_call_ids.insert(call_id.clone());
            out.push(TimelinePayload::ToolCall {
                call_id,
                status: final_status,
                detail,
                background_task,
            });
        }
        let mut bg: Vec<_> = self.bg_tasks.drain().collect();
        bg.sort_by(|a, b| a.1.0.cmp(&b.1.0));
        for (_, (call_id, detail, background_task)) in bg {
            if seen_call_ids.insert(call_id.clone()) {
                out.push(TimelinePayload::ToolCall {
                    call_id,
                    status: final_status,
                    detail,
                    background_task,
                });
            }
        }
        self.last_running_tool = None;
        out
    }
}

async fn cancel_agy_subagent_monitors(
    target_ids: Option<&[String]>,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let now = Utc::now();
    for (sub_id, entry) in subagent_monitors.iter_mut() {
        if entry.desc.status != ProviderSubagentStatus::Running {
            continue;
        }
        let matches = match target_ids {
            None => true,
            Some(ids) => ids.iter().any(|id| id.trim() == sub_id.as_str()),
        };
        if !matches {
            continue;
        }
        for payload in entry.drain_open_tool_timelines(ToolCallStatus::Failed) {
            let _ = evt_tx
                .send(DriverEvent::ProviderSubagentTimeline {
                    subagent_id: sub_id.clone(),
                    payload,
                })
                .await;
        }
        entry.desc.status = ProviderSubagentStatus::Canceled;
        entry.desc.updated_at = now;
        entry.monitor.in_wakeup = false;
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
            .await;
    }
}

async fn resume_agy_subagent_monitor(
    recipient_id: &str,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let clean_id = recipient_id.trim();
    if clean_id.is_empty() {
        return;
    }
    let Some(entry) = subagent_monitors.get_mut(clean_id) else {
        return;
    };
    entry.monitor.in_wakeup = true;
    if entry.desc.status != ProviderSubagentStatus::Running {
        entry.desc.status = ProviderSubagentStatus::Running;
        entry.desc.updated_at = Utc::now();
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
            .await;
    }
}

fn apply_subagent_wakeup_steps(
    entry: &mut AgySubagentMonitorEntry,
    steps: Vec<WakeupStep>,
    force_terminal_cancel: bool,
) -> (Vec<TimelinePayload>, Option<ProviderSubagentStatus>) {
    let mut payloads = Vec::new();
    let mut status_transition: Option<ProviderSubagentStatus> = None;
    for wake_ev in steps {
        match wake_ev {
            WakeupStep::Started { .. } => {
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
            }
            WakeupStep::UserMessage { id, text } => {
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
                payloads.push(TimelinePayload::UserPrompt {
                    message_id: id,
                    text,
                    badges: Vec::new(),
                });
            }
            WakeupStep::Thinking { id, text } => {
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
                payloads.push(TimelinePayload::AssistantThinking { item_id: id, text });
            }
            WakeupStep::Message { id, text } => {
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
                payloads.push(TimelinePayload::AssistantMessage {
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
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
                if status == ToolCallStatus::Running {
                    let existing_bg = entry
                        .open_tools
                        .get(&call_id)
                        .and_then(|(_, bg)| bg.clone());
                    entry
                        .open_tools
                        .insert(call_id.clone(), (detail.clone(), existing_bg));
                    entry.last_running_tool = Some((call_id.clone(), detail.clone()));
                } else {
                    entry.open_tools.remove(&call_id);
                    entry.bg_tasks.retain(|_, (cid, _, _)| cid != &call_id);
                    if entry
                        .last_running_tool
                        .as_ref()
                        .is_some_and(|(cid, _)| cid == &call_id)
                    {
                        entry.last_running_tool = None;
                    }
                }
                payloads.push(TimelinePayload::ToolCall {
                    call_id,
                    status,
                    detail,
                    background_task: None,
                });
            }
            WakeupStep::BackgroundTaskStarted {
                task_id,
                call_id,
                description,
                log_path,
                command,
                ..
            } => {
                if !force_terminal_cancel && entry.desc.status != ProviderSubagentStatus::Running {
                    entry.desc.status = ProviderSubagentStatus::Running;
                    entry.desc.updated_at = Utc::now();
                    status_transition = Some(ProviderSubagentStatus::Running);
                }
                let (cid, det) = if let Some(c) = call_id {
                    let matched = entry
                        .open_tools
                        .get(&c)
                        .map(|(d, _)| d.clone())
                        .or_else(|| {
                            entry
                                .last_running_tool
                                .as_ref()
                                .filter(|(lc, _)| lc == &c)
                                .map(|(_, d)| d.clone())
                        })
                        .unwrap_or_else(|| ToolCallDetail::Shell {
                            command: command
                                .clone()
                                .or_else(|| description.clone())
                                .unwrap_or_else(|| "Background task".to_string()),
                            cwd: None,
                            exit_code: None,
                            output_preview: None,
                        });
                    (c, matched)
                } else if let Some((lc, ld)) = entry.last_running_tool.clone() {
                    (lc, ld)
                } else {
                    (
                        format!("sub_bg_{}", sanitize_bg_task_id(&task_id)),
                        ToolCallDetail::Shell {
                            command: command
                                .clone()
                                .or_else(|| description.clone())
                                .unwrap_or_else(|| "Background task".to_string()),
                            cwd: None,
                            exit_code: None,
                            output_preview: None,
                        },
                    )
                };
                let bg_info = BackgroundTaskInfo {
                    task_id: Some(task_id.clone()),
                    label: description,
                    log_path,
                };
                entry
                    .open_tools
                    .insert(cid.clone(), (det.clone(), Some(bg_info.clone())));
                entry
                    .bg_tasks
                    .insert(task_id, (cid.clone(), det.clone(), Some(bg_info.clone())));
                payloads.push(TimelinePayload::ToolCall {
                    call_id: cid,
                    status: ToolCallStatus::Running,
                    detail: det,
                    background_task: Some(bg_info),
                });
            }
            WakeupStep::BackgroundTaskCompleted { task_id, canceled } => {
                if let Some((cid, det, bg_info)) = entry.bg_tasks.remove(&task_id) {
                    entry.open_tools.remove(&cid);
                    if entry
                        .last_running_tool
                        .as_ref()
                        .is_some_and(|(lc, _)| lc == &cid)
                    {
                        entry.last_running_tool = None;
                    }
                    payloads.push(TimelinePayload::ToolCall {
                        call_id: cid,
                        status: if canceled {
                            ToolCallStatus::Failed
                        } else {
                            ToolCallStatus::Completed
                        },
                        detail: det,
                        background_task: bg_info,
                    });
                }
            }
            WakeupStep::Settled => {
                payloads.extend(entry.drain_open_tool_timelines(ToolCallStatus::Completed));
                entry.desc.status = ProviderSubagentStatus::Completed;
                entry.desc.updated_at = Utc::now();
                status_transition = Some(ProviderSubagentStatus::Completed);
            }
            WakeupStep::Failed { message } => {
                payloads.extend(entry.drain_open_tool_timelines(ToolCallStatus::Failed));
                entry.desc.status = ProviderSubagentStatus::Failed;
                entry.desc.subtitle = Some(message);
                entry.desc.updated_at = Utc::now();
                status_transition = Some(ProviderSubagentStatus::Failed);
            }
            WakeupStep::Notice { level, message } => {
                payloads.push(TimelinePayload::Notice { level, message });
            }
            WakeupStep::SubagentSpawned { .. }
            | WakeupStep::SubagentMessaged { .. }
            | WakeupStep::SubagentsCanceled { .. } => {}
        }
    }
    if force_terminal_cancel && entry.desc.status == ProviderSubagentStatus::Running {
        payloads.extend(entry.drain_open_tool_timelines(ToolCallStatus::Failed));
        entry.desc.status = ProviderSubagentStatus::Canceled;
        entry.desc.updated_at = Utc::now();
        entry.monitor.in_wakeup = false;
        status_transition = Some(ProviderSubagentStatus::Canceled);
    }
    (payloads, status_transition)
}

async fn poll_agy_subagent_monitor(
    sub_id: &str,
    entry: &mut AgySubagentMonitorEntry,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let steps = entry.monitor.poll_post_turn_wakeup();
    if steps.is_empty() {
        return;
    }
    let prev_status = entry.desc.status;
    let (payloads, status_transition) = apply_subagent_wakeup_steps(entry, steps, false);
    // If the subagent transitioned back to Running (resume/follow-up turn), upsert Running BEFORE
    // streaming the new turn's timeline items so clients reflect Running immediately.
    if status_transition == Some(ProviderSubagentStatus::Running)
        && prev_status != ProviderSubagentStatus::Running
    {
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
            .await;
    }
    for payload in payloads {
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentTimeline {
                subagent_id: sub_id.to_string(),
                payload,
            })
            .await;
    }
    if matches!(
        status_transition,
        Some(ProviderSubagentStatus::Completed | ProviderSubagentStatus::Failed)
    ) {
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn register_agy_subagent_monitor_in_dir(
    parent_agent_id: &str,
    child_conv_id: &str,
    role: &str,
    prompt_preview: &str,
    is_historical: bool,
    force_canceled: bool,
    data_dir: Option<&Path>,
    subagent_monitors: &mut HashMap<String, AgySubagentMonitorEntry>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let clean_id = child_conv_id.trim();
    if clean_id.is_empty() || clean_id.starts_with("ag_") {
        return;
    }
    if subagent_monitors.contains_key(clean_id) {
        if force_canceled {
            let target = vec![clean_id.to_string()];
            cancel_agy_subagent_monitors(Some(&target), subagent_monitors, evt_tx).await;
        }
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
    let desc = ProviderSubagentDescriptor {
        subagent_id: clean_id.to_string(),
        parent_agent_id: parent_agent_id.to_string(),
        parent_subagent_id: None,
        title,
        subtitle,
        status: if force_canceled {
            ProviderSubagentStatus::Canceled
        } else {
            ProviderSubagentStatus::Running
        },
        created_at: now,
        updated_at: now,
    };
    let sub_mon = match data_dir {
        Some(dir) => AgyTranscriptMonitor::from_brain_dir_subagent(
            dir.join("brain").join(clean_id),
            clean_id,
        ),
        None => AgyTranscriptMonitor::new_subagent(clean_id),
    };
    let mut entry = AgySubagentMonitorEntry::new(desc, sub_mon);

    if is_historical || force_canceled {
        if entry.monitor.active_path().is_none() {
            // Historical/canceled subagent without transcript on disk: settle immediately without Running flash.
            entry.desc.status = if force_canceled {
                ProviderSubagentStatus::Canceled
            } else {
                ProviderSubagentStatus::Completed
            };
            entry.monitor.in_wakeup = false;
            let _ = evt_tx
                .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
                .await;
            subagent_monitors.insert(clean_id.to_string(), entry);
            return;
        }
        let initial_steps = entry.monitor.poll_post_turn_wakeup();
        let (initial_timeline, _) =
            apply_subagent_wakeup_steps(&mut entry, initial_steps, force_canceled);
        let _ = evt_tx
            .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
            .await;
        for payload in initial_timeline {
            let _ = evt_tx
                .send(DriverEvent::ProviderSubagentTimeline {
                    subagent_id: clean_id.to_string(),
                    payload,
                })
                .await;
        }
        subagent_monitors.insert(clean_id.to_string(), entry);
        return;
    }

    let _ = evt_tx
        .send(DriverEvent::ProviderSubagentUpsert(entry.desc.clone()))
        .await;
    poll_agy_subagent_monitor(clean_id, &mut entry, evt_tx).await;
    subagent_monitors.insert(clean_id.to_string(), entry);
}

fn map_agy_tool_detail(update: &Value, prev: Option<&ToolCallDetail>) -> ToolCallDetail {
    let kind = update.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let title = update
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("tool");
    let raw_in = update.get("rawInput");
    let raw_out = update.get("rawOutput");

    if crate::managed_dispatch::is_launch(title) {
        return crate::managed_dispatch::acp_detail(update, prev);
    }

    if matches!(prev, Some(ToolCallDetail::Subagent { .. })) {
        return agy_subagent_detail(update, prev);
    }

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
            .and_then(|i| {
                i.get("CommandLine")
                    .or_else(|| i.get("command"))
                    .or_else(|| i.get("cmd"))
            })
            .is_some()
    {
        let cmd = raw_in
            .and_then(|i| {
                i.get("CommandLine")
                    .or_else(|| i.get("command"))
                    .or_else(|| i.get("cmd"))
            })
            .and_then(|v| v.as_str())
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { command, .. }) => Some(command.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());
        let cwd = crate::extract_acp_shell_cwd(raw_in).or_else(|| match prev {
            Some(ToolCallDetail::Shell { cwd, .. }) => cwd.clone(),
            _ => None,
        });
        let exit_code = raw_out
            .and_then(|o| o.get("exitCode"))
            .and_then(|v| v.as_i64())
            .map(|c| c as i32);
        return ToolCallDetail::Shell {
            command: cmd,
            cwd,
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

    if is_subagent_launch_tool(title, raw_in) {
        return agy_subagent_detail(update, prev);
    }
    if let Some(existing) = prev {
        return existing.clone();
    }

    ToolCallDetail::Generic {
        tool_name: title.to_string(),
        arguments_json: raw_in.map(|v| v.to_string()).unwrap_or_default(),
    }
}

fn extract_subagent_conversation_id_from_value(val: &Value) -> Option<String> {
    match val {
        Value::Object(map) => {
            if let Some(id) = map
                .get("conversationId")
                .or_else(|| map.get("conversation_id"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return Some(id.to_string());
            }
            for key in [
                "content",
                "structuredContent",
                "result",
                "output",
                "stdout",
                "text",
            ] {
                if let Some(sub) = map.get(key)
                    && let Some(found) = extract_subagent_conversation_id_from_value(sub)
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(extract_subagent_conversation_id_from_value),
        Value::String(text) => extract_subagent_conversation_id(None, Some(text)),
        _ => None,
    }
}

fn agy_subagent_detail(update: &Value, prev: Option<&ToolCallDetail>) -> ToolCallDetail {
    let mut dispatch = crate::managed_dispatch::acp_detail(update, prev);
    if let ToolCallDetail::Subagent { child_agent_id, .. } = &mut dispatch
        && child_agent_id.is_none()
    {
        *child_agent_id = extract_subagent_conversation_id(update.get("rawOutput"), None)
            .or_else(|| {
                update
                    .get("content")
                    .and_then(extract_subagent_conversation_id_from_value)
            })
            .or_else(|| {
                update
                    .get("content")
                    .and_then(|c| extract_uuids_from_text(&c.to_string()).into_iter().next())
            });
    }
    dispatch
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

#[allow(clippy::too_many_arguments)]
async fn handle_acp_permission_request(
    val: &Value,
    mode: &SessionMode,
    workspace_cwd: &Path,
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

    let perm_options = crate::AcpPermissionOptions::parse(&options);

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
                perm_options,
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

    let plan_approval = crate::plan_approval::extract_plan_approval_from_acp_tool_call(
        tool_call,
        mode,
        workspace_cwd,
        "antigravity_plan",
    );

    // Auto-approve logic for staged attachment reads, `FullAccess`, or native file edits in `AcceptEdits`.
    // Plan approval gates always require explicit human review.
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
    let auto_approve = plan_approval.is_none()
        && (is_attachment_read
            || matches!(mode, SessionMode::FullAccess)
            || (matches!(mode, SessionMode::AcceptEdits)
                && matches!(
                    kind_str,
                    "edit" | "write" | "delete" | "move" | "read" | "search"
                )));

    if auto_approve && let Some(ref allow_opt) = perm_options.allow_once {
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
    let title = plan_approval
        .as_ref()
        .and_then(|p| p.plan_title.clone())
        .unwrap_or_else(|| {
            tool_call
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Approve tool execution")
                .to_string()
        });
    let authoritative = crate::acp_tool_call_has_authoritative_identity(tool_call);
    let detail = if plan_approval.is_none() && !authoritative {
        crate::acp_title_only_detail(&title, tool_call)
    } else {
        map_agy_tool_detail(tool_call, None)
    };
    let allowed_decisions = perm_options.allowed_decisions(plan_approval.is_some(), authoritative);

    pending_perms.insert(
        req_id.clone(),
        PendingPerm {
            rpc_id,
            perm_options,
            is_question: false,
            raw_questions: Vec::new(),
        },
    );

    let _ = evt_tx
        .send(DriverEvent::PermissionRequest {
            request_id: req_id.clone(),
            title,
            detail,
            allowed_decisions,
            plan_approval,
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

// --- Post-turn `transcript_full.jsonl` incremental reader, unified tool reconciliation & self-healing ---

enum WakeupStep {
    Started {
        source: String,
    },
    UserMessage {
        id: String,
        text: String,
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
        command: Option<String>,
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
    SubagentMessaged {
        subagent_id: String,
    },
    SubagentsCanceled {
        ids: Option<Vec<String>>,
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
const WAKEUP_IDLE_SETTLE_GRACE: Duration = Duration::from_secs(3);
const SELF_HEAL_GRACE_PERIOD: Duration = Duration::from_secs(4);
const MAX_CONSECUTIVE_SELF_HEALS: u32 = 3;
const IN_PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const ONE_SHOT_TIMER_EXPIRY_GRACE_SECS: i64 = 90;

fn is_one_shot_timer_expired(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    expires_at
        .is_some_and(|exp| now > exp + chrono::Duration::seconds(ONE_SHOT_TIMER_EXPIRY_GRACE_SECS))
}

fn is_timer_bg_description(desc: Option<&str>) -> bool {
    let Some(raw) = desc else {
        return false;
    };
    let lower = raw.to_ascii_lowercase();
    lower.contains("timer")
        || lower.contains("schedule")
        || lower.contains("cron")
        || lower.contains("alarm")
        || lower.contains("wakeup")
}

#[derive(Debug, Clone)]
struct BgTaskInfo {
    task_id: String,
    call_id: Option<String>,
    log_path: Option<String>,
    meta: Option<BackgroundTaskInfo>,
    expires_at: Option<DateTime<Utc>>,
    is_timer: bool,
}

#[derive(Debug, Clone)]
enum BgTaskDelta {
    Started {
        task_id: String,
        call_id: Option<String>,
        description: Option<String>,
        log_path: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        command: Option<String>,
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
    SubagentMessaged {
        subagent_id: String,
    },
    SubagentsCanceled {
        ids: Option<Vec<String>>,
    },
    Compacted,
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
        let meta_is_timer = expires_at.is_some()
            || meta
                .as_ref()
                .and_then(|m| m.label.as_deref())
                .is_some_and(|l| is_timer_bg_description(Some(l)));
        let entry = self
            .tasks
            .entry(task_id.clone())
            .or_insert_with(|| BgTaskInfo {
                task_id,
                call_id: None,
                log_path: None,
                meta: None,
                expires_at: None,
                is_timer: false,
            });
        if meta_is_timer {
            entry.is_timer = true;
        }
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

    fn drain_and_finish_call_ids(&mut self) -> Vec<String> {
        let mut call_ids = Vec::new();
        for (task_id, info) in self.tasks.drain() {
            self.finished_tasks.insert(task_id);
            if let Some(cid) = info.call_id {
                call_ids.push(cid);
            }
        }
        call_ids
    }

    fn active_call_ids(&self) -> HashSet<String> {
        self.tasks
            .values()
            .filter_map(|t| t.call_id.clone())
            .collect()
    }

    fn waiting_source_label(&self) -> String {
        let n = self.tasks.len();
        if n > 0 && self.tasks.values().all(|t| t.is_timer) {
            if n == 1 {
                "waiting:1 timer".to_string()
            } else {
                format!("waiting:{n} timers")
            }
        } else if n == 1 {
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
    agy_data_dir: Option<PathBuf>,
}

impl WakeupToolRegistry {
    fn with_data_dir(agy_data_dir: PathBuf) -> Self {
        Self {
            agy_data_dir: Some(agy_data_dir),
            ..Self::default()
        }
    }

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

    fn shell_detail_is_finished(detail: &ToolCallDetail) -> bool {
        match detail {
            ToolCallDetail::Shell {
                exit_code,
                output_preview,
                ..
            } => {
                exit_code.is_some()
                    || output_preview.as_deref().is_some_and(|s| {
                        let t = s.trim();
                        !t.is_empty()
                            && !t
                                .to_ascii_lowercase()
                                .contains("running as a background task")
                    })
            }
            _ => false,
        }
    }

    fn is_ambiguous_shell_prefix(norm: &str) -> bool {
        let lower = norm.trim().to_ascii_lowercase();
        let trimmed = lower.trim_end_matches(['"', '\'', ' ']).trim();
        matches!(
            trimmed,
            "python"
                | "python3"
                | "bash"
                | "sh"
                | "zsh"
                | "node"
                | "perl"
                | "ruby"
                | "python -c"
                | "python3 -c"
                | "bash -c"
                | "sh -c"
                | "zsh -c"
                | "node -e"
                | "perl -e"
                | "ruby -e"
        ) || (trimmed.len() <= 24
            && (trimmed.ends_with(" -c")
                || trimmed.ends_with(" -lc")
                || trimmed.ends_with(" -e")
                || trimmed.contains("<<")))
    }

    /// Marks `call_id` as an active background task. Returns `(detail, merged_bg_info, changed)`
    /// where `changed` is `false` when the entry was already `Running` with identical metadata,
    /// preventing redundant post-reply re-emissions that would move the tool card below the reply.
    /// Never regresses a shell entry that already completed with an exit code or real output.
    fn mark_background_task(
        &mut self,
        call_id: &str,
        info: BackgroundTaskInfo,
    ) -> Option<(ToolCallDetail, BackgroundTaskInfo, bool)> {
        let canon = self.canonical_id_for_wakeup(call_id).to_string();
        let entry = self.entries.get_mut(&canon)?;
        if entry.status != ToolCallStatus::Running && Self::shell_detail_is_finished(&entry.detail)
        {
            return None;
        }
        let merged = merge_bg_task_info(entry.bg_info.as_ref(), info);
        let changed = !entry.has_bg_task
            || entry.status != ToolCallStatus::Running
            || entry.bg_info.as_ref() != Some(&merged);
        entry.has_bg_task = true;
        entry.status = ToolCallStatus::Running;
        entry.bg_info = Some(merged.clone());
        Some((entry.detail.clone(), merged, changed))
    }

    fn find_matching_call_id_for_bg_task(
        &self,
        description: Option<&str>,
        full_command: Option<&str>,
    ) -> Option<String> {
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
        }

        let not_finished_shell = |e: &WakeupToolEntry| {
            e.signature.kind == "shell"
                && (e.status == ToolCallStatus::Running
                    || !Self::shell_detail_is_finished(&e.detail))
        };

        // Prefer matching against the full command recorded in PLANNER_RESPONSE before falling back
        // to the potentially truncated `bg_task` first-line description (e.g. `python3 -c`).
        if let Some(cmd) = full_command.map(str::trim).filter(|s| !s.is_empty()) {
            let norm_cmd = cmd.split_whitespace().collect::<Vec<_>>().join(" ");
            let cmd_sig = ToolSignature {
                kind: "shell",
                target: norm_cmd,
            };
            if let Some(cid) = self.order.iter().find(|cid| {
                self.entries.get(*cid).is_some_and(|e| {
                    is_unbound_in_turn(e)
                        && e.status == ToolCallStatus::Running
                        && e.signature.kind == "shell"
                        && signatures_match(&e.signature, &cmd_sig)
                })
            }) {
                return Some(cid.clone());
            }
            if let Some(cid) = self.order.iter().find(|cid| {
                self.entries.get(*cid).is_some_and(|e| {
                    is_unbound_in_turn(e)
                        && not_finished_shell(e)
                        && signatures_match(&e.signature, &cmd_sig)
                })
            }) {
                return Some(cid.clone());
            }
        }

        if let Some(desc) = description.map(str::trim).filter(|s| !s.is_empty()) {
            let norm_desc = desc.split_whitespace().collect::<Vec<_>>().join(" ");
            let ambiguous = Self::is_ambiguous_shell_prefix(&norm_desc);
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
            // dispatch-ack `completed` status (without exit code / real output). Skip ambiguous
            // short prefixes like `python3 -c` unless full_command already matched above.
            if !ambiguous
                && let Some(cid) = self.order.iter().find(|cid| {
                    self.entries.get(*cid).is_some_and(|e| {
                        is_unbound_in_turn(e) && not_finished_shell(e) && shell_matches(e)
                    })
                })
            {
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
        || clean == "invoke subagent"
        || clean == "spawn subagent"
        || clean.starts_with("invoke_subagent ")
        || clean.starts_with("spawn_subagent ")
        || clean.starts_with("invoke subagent ")
        || clean.starts_with("spawn subagent ")
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
    for key in [
        "conversationId=",
        "conversation_id=",
        "conversationId:",
        "conversation_id:",
    ] {
        if let Some(pos) = candidate_text.find(key) {
            let after = candidate_text[pos + key.len()..]
                .trim_start()
                .trim_start_matches(['"', '\'']);
            let id: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .collect();
            if !id.is_empty() {
                return Some(id);
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

    if crate::managed_dispatch::is_launch(tool_name) {
        return crate::managed_dispatch::detail(args.unwrap_or(&Value::Null), &Value::Null, None);
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
                *child_agent_id = crate::managed_dispatch::child_id(&Value::String(content.into()))
                    .or_else(|| extract_subagent_conversation_id(None, Some(content)));
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
    Shell {
        command: String,
    },
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
    ManageSubagents {
        action: String,
        ids: Vec<String>,
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
    last_wakeup_activity_at: Option<Instant>,
    seen_undelivered_files: HashSet<PathBuf>,
    last_checked_path: Option<PathBuf>,
    last_checked_mtime: Option<std::time::SystemTime>,
    committed_planner_texts: Vec<String>,
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
            last_wakeup_activity_at: None,
            seen_undelivered_files: HashSet::new(),
            last_checked_path: None,
            last_checked_mtime: None,
            committed_planner_texts: Vec::new(),
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

    /// Cheap `stat()` probe used to detect follow-up activity on previously settled subagents.
    fn has_unconsumed_growth(&self) -> bool {
        let Some(path) = self.active_path() else {
            return false;
        };
        let Ok(meta) = std::fs::metadata(path) else {
            return false;
        };
        meta.len() != self.byte_offset
    }

    fn sync_silent(&mut self) {
        let _ = self.sync_silent_with_bg_tasks();
    }

    fn clear_committed_planner_texts(&mut self) {
        self.committed_planner_texts.clear();
    }

    fn sync_silent_for_turn_end(&mut self) -> (Vec<BgTaskDelta>, Vec<String>) {
        let deltas = self.sync_silent_with_bg_tasks();
        let texts = std::mem::take(&mut self.committed_planner_texts);
        (deltas, texts)
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
        self.last_wakeup_activity_at = None;
        bg_deltas
    }

    fn poll_post_turn_wakeup(&mut self) -> Vec<WakeupStep> {
        let mut bg_deltas = Vec::new();
        let mut out = self.read_new_lines(false, &mut bg_deltas);
        let now = Instant::now();
        self.check_error_stall(&mut out, now);
        self.check_wakeup_idle_settle(&mut out, now);
        out
    }

    /// When a post-turn wakeup arms a background timer/task (e.g. `schedule`) and finishes its
    /// tool call without a trailing empty-tool `PLANNER_RESPONSE(DONE)`, settle cleanly once
    /// no pending tool calls remain so the conversation returns to talkable `waiting:*` state.
    fn check_wakeup_idle_settle(&mut self, out: &mut Vec<WakeupStep>, now: Instant) {
        if self.always_active
            || !self.in_wakeup
            || !self.pending_tools.is_empty()
            || self.active_bg_tasks.is_empty()
            || self.consecutive_errors != 0
        {
            return;
        }
        let Some(last_act) = self.last_wakeup_activity_at else {
            return;
        };
        if now.saturating_duration_since(last_act) >= WAKEUP_IDLE_SETTLE_GRACE {
            self.in_wakeup = false;
            self.last_wakeup_activity_at = None;
            out.push(WakeupStep::Settled);
        }
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
            if self.seen_undelivered_files.len() >= 2048 {
                self.seen_undelivered_files.retain(|p| p.exists());
                if self.seen_undelivered_files.len() >= 2048 {
                    self.seen_undelivered_files.clear();
                }
            }
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
            if step_type == "PLANNER_RESPONSE"
                && status == "DONE"
                && !content.trim().is_empty()
                && self.committed_planner_texts.len() < 128
            {
                self.committed_planner_texts.push(content.to_string());
            }

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
                    } else if is_manage_subagents_tool_name(tname) {
                        let action = transcript_arg_str(args, &["Action", "action"])
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                        let mut ids: Vec<String> = args
                            .and_then(|a| {
                                a.get("ConversationIds")
                                    .or_else(|| a.get("conversation_ids"))
                                    .and_then(|v| v.as_array())
                            })
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str())
                                    .map(str::trim)
                                    .filter(|s| !s.is_empty())
                                    .map(ToOwned::to_owned)
                                    .collect()
                            })
                            .unwrap_or_default();
                        if ids.is_empty()
                            && let Some(single) =
                                transcript_arg_str(args, &["ConversationId", "conversation_id"])
                        {
                            ids.push(single);
                        }
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::ManageSubagents { action, ids });
                    } else if is_send_message_tool_name(tname) {
                        if let Some(recipient_id) = transcript_arg_str(
                            args,
                            &[
                                "Recipient",
                                "recipient",
                                "conversationId",
                                "conversation_id",
                                "agent_id",
                            ],
                        ) {
                            bg_deltas.push(BgTaskDelta::SubagentMessaged {
                                subagent_id: recipient_id.clone(),
                            });
                            if !silent {
                                out.push(WakeupStep::SubagentMessaged {
                                    subagent_id: recipient_id,
                                });
                            }
                        }
                        self.pending_silent_tools
                            .push_back(PendingTranscriptToolMeta::Other);
                    } else if matches!(
                        tname
                            .rsplit_once(':')
                            .or_else(|| tname.rsplit_once('/'))
                            .or_else(|| tname.rsplit_once("__"))
                            .map(|(_, leaf)| leaf)
                            .unwrap_or(tname)
                            .to_ascii_lowercase()
                            .as_str(),
                        "run_command" | "execute_command" | "bash" | "shell" | "terminal"
                    ) {
                        if let Some(cmd) =
                            transcript_arg_str(args, &["CommandLine", "command", "cmd"])
                        {
                            self.pending_silent_tools
                                .push_back(PendingTranscriptToolMeta::Shell { command: cmd });
                        } else {
                            self.pending_silent_tools
                                .push_back(PendingTranscriptToolMeta::Other);
                        }
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
            let mut started_shell_command: Option<String> = None;

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
                let shell_command = match &pending_meta {
                    PendingTranscriptToolMeta::Shell { command } => Some(command.clone()),
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
                    started_shell_command = shell_command.clone();
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
                        command: shell_command,
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
                    // 5) Paired `manage_subagents(Action: "kill" | "kill_all")` subagent cancellation
                    if status == "DONE"
                        && let PendingTranscriptToolMeta::ManageSubagents { action, ids } =
                            &pending_meta
                    {
                        if action == "kill_all" {
                            bg_deltas.push(BgTaskDelta::SubagentsCanceled { ids: None });
                            if !silent {
                                out.push(WakeupStep::SubagentsCanceled { ids: None });
                            }
                        } else if action == "kill" {
                            let resolved_ids = if ids.is_empty() {
                                extract_uuids_from_text(content)
                            } else {
                                ids.clone()
                            };
                            if !resolved_ids.is_empty() {
                                bg_deltas.push(BgTaskDelta::SubagentsCanceled {
                                    ids: Some(resolved_ids.clone()),
                                });
                                if !silent {
                                    out.push(WakeupStep::SubagentsCanceled {
                                        ids: Some(resolved_ids),
                                    });
                                }
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
                    bg_deltas.push(BgTaskDelta::SubagentsCanceled { ids: None });
                    if !silent {
                        out.push(WakeupStep::SubagentsCanceled { ids: None });
                    }
                }
                for (task_id, canceled) in &completed_bg_tasks {
                    bg_deltas.push(BgTaskDelta::Completed {
                        task_id: task_id.clone(),
                        canceled: *canceled,
                    });
                }
            }

            if step_type == "CHECKPOINT" {
                bg_deltas.push(BgTaskDelta::Compacted);
                if !silent {
                    self.consecutive_errors = 0;
                    self.last_error_at = None;
                    self.last_error_message = None;
                    self.last_in_progress_heartbeat = Some(Instant::now());
                    out.push(WakeupStep::Notice {
                        level: "info".into(),
                        message: "Context compacted automatically".into(),
                    });
                }
                continue;
            }

            if silent {
                continue;
            }

            if is_system_message_step(step_type, content) {
                for (task_id, canceled) in completed_bg_tasks {
                    out.push(WakeupStep::BackgroundTaskCompleted { task_id, canceled });
                }
                if !self.in_wakeup {
                    self.in_wakeup = true;
                    self.consecutive_errors = 0;
                    self.last_error_at = None;
                    self.last_error_message = None;
                    self.last_in_progress_heartbeat = Some(Instant::now());
                    self.last_wakeup_activity_at = Some(Instant::now());
                    out.push(WakeupStep::Started {
                        source: if self.always_active {
                            "message".into()
                        } else {
                            "background_task".into()
                        },
                    });
                    if self.always_active
                        && step_idx > 0
                        && let Some(msg_text) = extract_system_message_content(content)
                    {
                        out.push(WakeupStep::UserMessage {
                            id: format!("sub_msg_{step_idx}"),
                            text: msg_text,
                        });
                    }
                }
            } else if self.always_active && step_type == "USER_INPUT" {
                let was_idle = !self.in_wakeup;
                self.in_wakeup = true;
                self.consecutive_errors = 0;
                self.last_error_at = None;
                self.last_error_message = None;
                if was_idle {
                    out.push(WakeupStep::Started {
                        source: "user_input".into(),
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
                self.last_in_progress_heartbeat = Some(Instant::now());
                self.last_wakeup_activity_at = Some(Instant::now());
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
                    self.last_wakeup_activity_at = None;
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
                self.last_wakeup_activity_at = Some(Instant::now());
                let (cid, orig_detail) = self.pending_tools.remove(0);
                let detail = attach_transcript_tool_result(orig_detail, content);
                if status == "RUNNING"
                    && let Some((task_id, description, log_path)) =
                        parse_bg_task_started(content, &self.session_id)
                {
                    let command = started_shell_command.or_else(|| match &detail {
                        ToolCallDetail::Shell { command, .. } => Some(command.clone()),
                        _ => None,
                    });
                    out.push(WakeupStep::BackgroundTaskStarted {
                        task_id,
                        call_id: Some(cid.clone()),
                        description,
                        log_path,
                        expires_at: started_bg_expires_at,
                        command,
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
                    command: started_shell_command,
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

static AGY_DATA_DIR_OVERRIDE: std::sync::OnceLock<std::sync::RwLock<Option<PathBuf>>> =
    std::sync::OnceLock::new();

/// Register an in-process override for Antigravity's data directory (for deterministic E2E tests).
pub fn set_agy_data_dir_override(dir: Option<PathBuf>) {
    let lock = AGY_DATA_DIR_OVERRIDE.get_or_init(|| std::sync::RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = dir;
    }
}

fn resolve_agy_data_dir_with_extra_env(extra_env: &HashMap<String, String>) -> PathBuf {
    if let Some(dir) = extra_env
        .get("ANTIGRAVITY_EXECUTABLE_DATA_DIR")
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        return PathBuf::from(dir);
    }
    resolve_agy_data_dir()
}

pub fn resolve_agy_data_dir() -> PathBuf {
    if let Some(lock) = AGY_DATA_DIR_OVERRIDE.get()
        && let Ok(guard) = lock.read()
        && let Some(ref dir) = *guard
    {
        return dir.clone();
    }
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
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
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

pub fn antigravity_model_supports_fast_mode(
    model_id: &str,
    opt: Option<&Value>,
    has_fast_config_option: bool,
) -> bool {
    if let Some(obj) = opt
        && let Some(explicit) = obj
            .get("supportsFastMode")
            .or_else(|| obj.get("supports_fast_mode"))
            .and_then(|v| v.as_bool())
    {
        return explicit;
    }
    if !has_fast_config_option {
        // Never fabricate fast-mode support for Antigravity unless `agy acp` explicitly
        // advertises a `fast_mode` config option or per-model flag.
        return false;
    }
    let lower = model_id.trim().to_ascii_lowercase();
    !(lower.contains("orchestrator")
        || lower.contains("flash")
        || lower.contains("haiku")
        || lower.starts_with("gpt-oss"))
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
            supports_fast_mode: antigravity_model_supports_fast_mode(row.id, None, false),
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
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
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
    let has_fast_config_option = config_options
        .iter()
        .any(|o| o.get("id").and_then(|x| x.as_str()) == Some("fast_mode"));

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
                supports_fast_mode: antigravity_model_supports_fast_mode(
                    base_id,
                    Some(opt),
                    has_fast_config_option,
                ),
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
        let matched = wakeup_tools
            .find_matching_call_id_for_bg_task(Some("Timer: 8m, Prompt: watchdog"), None);
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
    fn managed_dispatch_partial_acp_and_transcript_results_link_orbit_thread() {
        for name in [
            "create_agent",
            "mcp_orbit_create_agent",
            "mcp__orbit__create_agent",
            "spawn_workspace_peer",
            "mcp_orbit_spawn_workspace_peer",
            "mcp__orbit__spawn_workspace_peer",
            "Run mcp orbit create agent",
        ] {
            let args = json!({"title":"Review", "initial_prompt":"Check permissions"});
            let started = map_agy_tool_detail(
                &json!({"title": name, "kind":"execute", "rawInput":args}),
                None,
            );
            let result = json!({"content":[{"type":"text","text":"Created thread: {\"agent_id\":\"ag_one\"}"}]});
            let done = map_agy_tool_detail(&json!({"rawOutput":result}), Some(&started));
            let replay = attach_transcript_tool_result(
                map_transcript_tool_detail(name, Some(&args)),
                &result.to_string(),
            );
            for detail in [done, replay] {
                assert!(
                    matches!(detail, ToolCallDetail::Subagent { child_agent_id: Some(id), role, prompt_preview }
                    if id == "ag_one" && role == "Review" && prompt_preview == "Check permissions")
                );
            }
        }
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
                command: None,
            },
            BgTaskDelta::Started {
                task_id: "sess_1/task-291".into(),
                call_id: None,
                description: Some("set -a && source runtime.env && set +a &&".into()),
                log_path: Some("/tmp/task-291.log".into()),
                expires_at: None,
                command: None,
            },
            BgTaskDelta::Started {
                task_id: "sess_1/task-292".into(),
                call_id: None,
                description: Some("Timer: 8m, Prompt: new watchdog".into()),
                log_path: Some("/tmp/task-292.log".into()),
                expires_at: None,
                command: None,
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
            reg.find_matching_call_id_for_bg_task(Some("Timer: 5m, Prompt: extra"), None),
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
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_evicted_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock agy shim");

        let received_prompts: Arc<tokio::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let received_bg = Arc::clone(&received_prompts);

        // Mock ACP server over shim IPC:
        // - `initialize` -> OK
        // - `session/load` for stale UUID -> silently returns OK (matching real `agy` bug!)
        // - `session/prompt` on stale UUID -> returns `Failed to send user message: not_found: trajectory not found: ...`
        // - `session/new` -> returns fresh UUID `fresh-sess-9999`
        // - `session/prompt` on fresh UUID -> streams assistant chunk and returns `stopReason: end_turn`
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq = 0u64;
            async fn send_stdout(wh: &mut ShimWriteHalf, seq: &mut u64, payload: Value) {
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
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_evicted".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-neon".into()),
            effort: Some(ReasoningEffort::Xhigh),
            fast_mode: false,
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
            acp_agent_id: None,
            acp_spec: None,
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
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
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
        use orbit_shim::ShimListener;
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::AsyncWriteExt;

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_reattach_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock reattached shim");

        // Simulate frames arriving after daemon hot-restart for an in-flight turn whose
        // original `session/prompt` RPC had `id: 7` (from before restart).
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("accept");
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
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_reattach".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-carbon".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
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
            acp_agent_id: None,
            acp_spec: None,
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

    #[test]
    fn agy_fast_mode_launch_env_and_session_meta() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;

        let mut cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_fast".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: PathBuf::from("/tmp/agy_fast.sock"),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-carbon".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::FullAccess,
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
            acp_agent_id: None,
            acp_spec: None,
        };

        let env_off = build_agy_launch_env(&cfg);
        assert!(!env_off.contains_key("AGY_FAST_MODE"));
        assert!(build_agy_session_meta(&cfg).is_none());

        cfg.fast_mode = true;
        let env_on = build_agy_launch_env(&cfg);
        assert_eq!(env_on.get("AGY_FAST_MODE").map(String::as_str), Some("1"));
        assert_eq!(
            build_agy_session_meta(&cfg),
            Some(json!({ "antigravity": { "fastMode": true } }))
        );

        cfg.system_prompt_append = Some("Keep it concise.".into());
        assert_eq!(
            build_agy_session_meta(&cfg),
            Some(json!({
                "antigravity": {
                    "systemPrompt": { "append": "Keep it concise." },
                    "fastMode": true
                }
            }))
        );

        let req_on = build_agy_set_fast_mode_request(42, "sess_1", true);
        assert_eq!(req_on["method"], "session/set_config_option");
        assert_eq!(req_on["params"]["sessionId"], "sess_1");
        assert_eq!(req_on["params"]["configId"], "fast_mode");
        assert_eq!(req_on["params"]["value"], "on");

        let req_off = build_agy_set_fast_mode_request(43, "sess_1", false);
        assert_eq!(req_off["params"]["value"], "off");

        // Fallback catalog must NOT advertise fake fast-mode support when `agy acp` has not
        // advertised `fast_mode` in `configOptions`.
        let catalog = fallback_antigravity_models();
        let carbon = catalog
            .models
            .iter()
            .find(|m| m.id == "gemini-4-carbon")
            .expect("carbon");
        assert!(!carbon.supports_fast_mode);
        let opus = catalog
            .models
            .iter()
            .find(|m| m.id == "claude-opus-5-5")
            .expect("opus");
        assert!(!opus.supports_fast_mode);
        let flash = catalog
            .models
            .iter()
            .find(|m| m.id == "gemini-3.8-flash")
            .expect("flash");
        assert!(!flash.supports_fast_mode);

        // When `agy acp` explicitly advertises `fast_mode`, flagship models enable it while
        // flash/orchestrator models remain excluded.
        assert!(antigravity_model_supports_fast_mode(
            "gemini-4-carbon",
            None,
            true
        ));
        assert!(antigravity_model_supports_fast_mode(
            "claude-opus-5-5",
            None,
            true
        ));
        assert!(!antigravity_model_supports_fast_mode(
            "gemini-3.8-flash",
            None,
            true
        ));
        assert!(antigravity_model_supports_fast_mode(
            "gemini-4-carbon",
            Some(&json!({ "supportsFastMode": true })),
            false
        ));
    }

    #[tokio::test]
    async fn agy_live_fast_mode_toggle_ignores_unsupported_method_error() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_fast_toggle_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_fast_calls: Arc<tokio::sync::Mutex<Vec<Value>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed_bg = Arc::clone(&observed_fast_calls);

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
                            json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1}}),
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
                                    "sessionId":"sess_fast_live",
                                    "models":{
                                        "currentModelId":"gemini-4-carbon",
                                        "availableModels":[{"modelId":"gemini-4-carbon"}]
                                    }
                                }
                            }),
                        )
                        .await;
                    }
                    "session/set_config_option" => {
                        observed_bg.lock().await.push(rpc.clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "error":{"code":-32601,"message":"Method not found"}
                            }),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc":"2.0",
                                "method":"session/update",
                                "params":{
                                    "sessionId":"sess_fast_live",
                                    "update":{
                                        "sessionUpdate":"agent_message_chunk",
                                        "content":{"type":"text","text":"Fast mode ok"}
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
                    _ => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"jsonrpc":"2.0","id":id,"result":{}}),
                        )
                        .await;
                    }
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_fast_live".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-4-carbon".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
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
            acp_agent_id: None,
            acp_spec: None,
        };

        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
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
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(true),
            })
            .await
            .expect("send fast toggle");
        // Redundant toggle to the same state must NOT send a duplicate RPC
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: None,
                effort: None,
                fast_mode: Some(true),
            })
            .await
            .expect("send redundant fast toggle");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_fast_1".into(),
                prompt: "ping".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send prompt");

        let mut saw_error = false;
        let mut saw_reply = false;
        let mut saw_restart_notice = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn timeout")
                .expect("ev");
            match ev {
                DriverEvent::StatusChanged(AgentStatus::Error { .. }) => saw_error = true,
                DriverEvent::Notice { level, message }
                    if level == "info"
                        && message.contains("takes effect on next Antigravity session restart") =>
                {
                    saw_restart_notice = true;
                }
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "Fast mode ok" => saw_reply = true,
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }

        assert!(
            !saw_error,
            "unsupported set_config_option must not fail session"
        );
        assert!(
            saw_restart_notice,
            "unsupported set_config_option should inform user via info notice"
        );
        assert!(saw_reply);
        let calls = observed_fast_calls.lock().await;
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["params"]["configId"], "fast_mode");
        assert_eq!(calls[0]["params"]["value"], "on");

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn agy_open_session_reconciles_fast_mode_config_option() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_agy_fast_reconcile_{}.sock",
            uuid::Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");
        let observed_set_opts: Arc<tokio::sync::Mutex<Vec<Value>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed_bg = Arc::clone(&observed_set_opts);

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
                    "session/load" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc":"2.0",
                                "id":id,
                                "result":{
                                    "sessionId":"sess_resume_1",
                                    "models":{
                                        "currentModelId":"gemini-4-carbon",
                                        "availableModels":[{"modelId":"gemini-4-carbon"}]
                                    },
                                    "configOptions":[
                                        {"id":"fast_mode","currentValue":"on"}
                                    ]
                                }
                            }),
                        )
                        .await;
                    }
                    "session/set_config_option" => {
                        observed_bg.lock().await.push(rpc.clone());
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"jsonrpc":"2.0","id":id,"result":{}}),
                        )
                        .await;
                        break;
                    }
                    _ => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({"jsonrpc":"2.0","id":id,"result":{}}),
                        )
                        .await;
                    }
                }
            }
        });

        let mut shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock");
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_fast_reconcile".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: Some("sess_resume_1".into()),
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
            acp_agent_id: None,
            acp_spec: None,
        };

        let mut next_id = 1i64;
        let mut pending = HashMap::new();
        let opened = open_agy_session(
            &cfg,
            &mut shim,
            &mut next_id,
            &mut pending,
            AgySessionOpenMode::Load("sess_resume_1"),
            None,
            None,
            &SessionMode::FullAccess,
        )
        .await
        .expect("open session");
        assert_eq!(opened.session_id, "sess_resume_1");

        let opts = observed_set_opts.lock().await;
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0]["params"]["sessionId"], "sess_resume_1");
        assert_eq!(opts[0]["params"]["configId"], "fast_mode");
        assert_eq!(opts[0]["params"]["value"], "off");

        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn resumed_agy_subagent_transitions_back_to_running_and_settles_on_follow_up_turn() {
        use std::io::Write;

        let tmp = std::env::temp_dir().join(format!(
            "orbit_agy_subagent_resume_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let logs_dir = tmp.join(".system_generated/logs");
        std::fs::create_dir_all(&logs_dir).expect("create logs dir");
        let transcript = logs_dir.join("transcript.jsonl");

        // Turn 1: initial prompt + settled final assistant reply.
        {
            let mut f = std::fs::File::create(&transcript).expect("create transcript");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 0,
                    "type": "USER_INPUT",
                    "status": "DONE",
                    "content": "Initial audit task"
                })
            )
            .expect("write step 0");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 1,
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "content": "Initial audit complete."
                })
            )
            .expect("write step 1");
        }

        let now = Utc::now();
        let sub_id = "sub_resume_1";
        let desc = ProviderSubagentDescriptor {
            subagent_id: sub_id.into(),
            parent_agent_id: "ag_parent".into(),
            parent_subagent_id: None,
            title: "Code Reviewer".into(),
            subtitle: None,
            status: ProviderSubagentStatus::Running,
            created_at: now,
            updated_at: now,
        };
        let sub_mon = AgyTranscriptMonitor::from_brain_dir_subagent(tmp.clone(), sub_id);
        let mut entry = AgySubagentMonitorEntry::new(desc, sub_mon);
        let (tx, mut rx) = mpsc::channel(64);

        // Poll Turn 1 -> settles to Completed.
        poll_agy_subagent_monitor(sub_id, &mut entry, &tx).await;
        assert_eq!(entry.desc.status, ProviderSubagentStatus::Completed);
        assert!(!entry.monitor.has_unconsumed_growth());
        while rx.try_recv().is_ok() {}

        // Turn 2 arrives via parent `send_message`: new SYSTEM_MESSAGE + active tool call.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .expect("open transcript for append");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 2,
                    "type": "SYSTEM_MESSAGE",
                    "status": "DONE",
                    "content": "[Message] sender=ag_parent content=Please fix the P0 finding."
                })
            )
            .expect("write step 2");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 3,
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "thinking": "Inspecting the P0 finding now.",
                    "tool_calls": [
                        {
                            "name": "read_file",
                            "args": { "Path": "/repo/src/lib.rs" }
                        }
                    ]
                })
            )
            .expect("write step 3");
        }

        // Self-healing fallback detects growth on the previously Completed subagent.
        assert!(entry.monitor.has_unconsumed_growth());
        poll_agy_subagent_monitor(sub_id, &mut entry, &tx).await;
        assert_eq!(entry.desc.status, ProviderSubagentStatus::Running);

        let mut saw_running_upsert = false;
        let mut saw_turn2_thinking = false;
        let mut saw_turn2_tool = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.status == ProviderSubagentStatus::Running =>
                {
                    assert!(
                        !saw_turn2_thinking && !saw_turn2_tool,
                        "Running upsert must precede follow-up turn timeline events"
                    );
                    saw_running_upsert = true;
                }
                DriverEvent::ProviderSubagentTimeline {
                    payload: TimelinePayload::AssistantThinking { text, .. },
                    ..
                } if text.contains("P0 finding") => {
                    saw_turn2_thinking = true;
                }
                DriverEvent::ProviderSubagentTimeline {
                    payload: TimelinePayload::ToolCall { .. },
                    ..
                } => {
                    saw_turn2_tool = true;
                }
                _ => {}
            }
        }
        assert!(saw_running_upsert);
        assert!(saw_turn2_thinking);
        assert!(saw_turn2_tool);

        // Turn 2 finishes: tool completes + final assistant reply settles back to Completed.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .expect("open transcript for settle");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 4,
                    "type": "GENERIC",
                    "status": "DONE",
                    "content": "file contents"
                })
            )
            .expect("write step 4");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 5,
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "content": "P0 fixed and verified."
                })
            )
            .expect("write step 5");
        }

        poll_agy_subagent_monitor(sub_id, &mut entry, &tx).await;
        assert_eq!(entry.desc.status, ProviderSubagentStatus::Completed);

        let mut saw_turn2_reply = false;
        let mut saw_completed_upsert = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                DriverEvent::ProviderSubagentTimeline {
                    payload: TimelinePayload::AssistantMessage { text, .. },
                    ..
                } if text.contains("P0 fixed and verified.") => {
                    saw_turn2_reply = true;
                }
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.status == ProviderSubagentStatus::Completed =>
                {
                    saw_completed_upsert = true;
                }
                _ => {}
            }
        }
        assert!(saw_turn2_reply);
        assert!(saw_completed_upsert);

        // Also verify explicit parent `send_message` resume flips Completed -> Running immediately.
        let mut monitors = HashMap::new();
        monitors.insert(sub_id.to_string(), entry);
        resume_agy_subagent_monitor(sub_id, &mut monitors, &tx).await;
        assert_eq!(
            monitors[sub_id].desc.status,
            ProviderSubagentStatus::Running
        );
        assert!(monitors[sub_id].monitor.in_wakeup);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn manage_subagents_kill_and_kill_all_mark_running_subagents_canceled() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_agy_cancel_sub_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let logs = tmp.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let transcript = logs.join("transcript_full.jsonl");

        // Parent invokes two subagents, then calls manage_subagents(kill) on sub_1, then kill_all on sub_2
        let lines = [
            json!({
                "step_index": 0,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "tool_calls": [
                    { "name": "invoke_subagent", "args": { "Role": "Worker 1", "Prompt": "Do A" } },
                    { "name": "invoke_subagent", "args": { "Role": "Worker 2", "Prompt": "Do B" } }
                ]
            }),
            json!({
                "step_index": 1,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Created the following subagent: {\"conversationId\": \"11111111-1111-1111-1111-111111111111\"}"
            }),
            json!({
                "step_index": 2,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Created the following subagent: {\"conversationId\": \"22222222-2222-2222-2222-222222222222\"}"
            }),
            json!({
                "step_index": 3,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "tool_calls": [
                    {
                        "name": "manage_subagents",
                        "args": {
                            "Action": "kill",
                            "ConversationIds": ["11111111-1111-1111-1111-111111111111"]
                        }
                    }
                ]
            }),
            json!({
                "step_index": 4,
                "type": "GENERIC",
                "status": "DONE",
                "content": "Killed subagent 11111111-1111-1111-1111-111111111111"
            }),
        ]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        std::fs::write(&transcript, lines).expect("write transcript");

        let mut monitor = AgyTranscriptMonitor::from_brain_dir_with_session(tmp.clone(), "sess_c");
        let deltas1 = monitor.poll_foreground_bg_deltas();
        assert!(deltas1.iter().any(|d| matches!(
            d,
            BgTaskDelta::SubagentsCanceled { ids: Some(ids) }
                if ids.as_slice() == ["11111111-1111-1111-1111-111111111111"]
        )));

        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let (evt_tx, mut evt_rx) = mpsc::channel(32);

        apply_foreground_bg_deltas(
            "ag_parent",
            deltas1,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut tool_cache,
            &mut subagent_monitors,
            &evt_tx,
        )
        .await;

        assert_eq!(
            subagent_monitors["11111111-1111-1111-1111-111111111111"]
                .desc
                .status,
            ProviderSubagentStatus::Canceled
        );
        assert_eq!(
            subagent_monitors["22222222-2222-2222-2222-222222222222"]
                .desc
                .status,
            ProviderSubagentStatus::Running
        );
        while evt_rx.try_recv().is_ok() {}

        // Now append `manage_subagents(Action: "kill_all")`
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&transcript)
                .expect("open append");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 5,
                    "type": "PLANNER_RESPONSE",
                    "status": "DONE",
                    "tool_calls": [
                        { "name": "manage_subagents", "args": { "Action": "kill_all" } }
                    ]
                })
            )
            .expect("w5");
            writeln!(
                f,
                "{}",
                json!({
                    "step_index": 6,
                    "type": "GENERIC",
                    "status": "DONE",
                    "content": "Killed all subagents"
                })
            )
            .expect("w6");
        }

        let deltas2 = monitor.poll_foreground_bg_deltas();
        apply_foreground_bg_deltas(
            "ag_parent",
            deltas2,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut tool_cache,
            &mut subagent_monitors,
            &evt_tx,
        )
        .await;

        assert_eq!(
            subagent_monitors["22222222-2222-2222-2222-222222222222"]
                .desc
                .status,
            ProviderSubagentStatus::Canceled
        );
        let mut saw_canceled_2 = false;
        while let Ok(ev) = evt_rx.try_recv() {
            if let DriverEvent::ProviderSubagentUpsert(d) = ev
                && d.subagent_id == "22222222-2222-2222-2222-222222222222"
                && d.status == ProviderSubagentStatus::Canceled
            {
                saw_canceled_2 = true;
            }
        }
        assert!(saw_canceled_2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn foreground_checkpoint_emits_compaction_notice() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_agy_fg_ckpt_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let logs = tmp.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let transcript = logs.join("transcript_full.jsonl");
        std::fs::write(
            &transcript,
            format!(
                "{}\n",
                json!({
                    "step_index": 12,
                    "type": "CHECKPOINT",
                    "status": "DONE"
                })
            ),
        )
        .expect("write checkpoint");

        let mut monitor = AgyTranscriptMonitor::from_brain_dir_with_session(tmp.clone(), "sess_cp");
        let deltas = monitor.poll_foreground_bg_deltas();
        assert!(deltas.iter().any(|d| matches!(d, BgTaskDelta::Compacted)));

        let mut wakeup_tools = WakeupToolRegistry::default();
        let mut bg_tracker = BackgroundTaskTracker::default();
        let mut tool_cache = HashMap::new();
        let mut subagent_monitors = HashMap::new();
        let (evt_tx, mut evt_rx) = mpsc::channel(8);

        apply_foreground_bg_deltas(
            "ag_parent",
            deltas,
            &mut wakeup_tools,
            &mut bg_tracker,
            &mut tool_cache,
            &mut subagent_monitors,
            &evt_tx,
        )
        .await;

        let notice = evt_rx.try_recv().expect("compaction notice emitted");
        assert!(matches!(
            notice,
            DriverEvent::Notice { level, message }
                if level == "info" && message == "Context compacted automatically"
        ));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manage_subagents_name_normalization_and_output_uuid_fallback_work() {
        assert!(is_manage_subagents_tool_name("manage_subagents"));
        assert!(is_manage_subagents_tool_name(
            "default_api:manage_subagents"
        ));
        assert!(is_manage_subagents_tool_name("Run manage_subagents"));
        assert!(is_manage_subagents_tool_name(
            "mcp__orbit__manage_subagents"
        ));
        assert!(!is_manage_subagents_tool_name("invoke_subagent"));

        let fallback = extract_manage_subagents_cancel(
            Some(&json!({ "Action": "kill" })),
            Some("Terminated subagent aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee cleanly."),
        );
        assert_eq!(
            fallback,
            Some(Some(vec!["aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into()]))
        );
        assert_eq!(
            extract_manage_subagents_cancel(Some(&json!({ "Action": "kill_all" })), None),
            Some(None)
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agy_interrupt_and_active_interrupt_prompt_clear_stale_queue_and_finalize_without_shift()
     {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let sock_path = std::env::temp_dir().join(format!("o_agy_iq_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let received_prompts: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&received_prompts);
        let cancel_count = Arc::new(AtomicU64::new(0));
        let cancel_bg = Arc::clone(&cancel_count);

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

            let mut inflight_prompt_id: Option<Value> = None;

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(frame) = serde_json::from_str::<ShimFrame>(&raw_line) else {
                    continue;
                };
                if let ShimFrame::WriteStdin { line } = frame {
                    let Ok(req) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                    match method {
                        "initialize" => {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": req["id"],
                                    "result": { "protocolVersion": 1 }
                                }),
                            )
                            .await;
                        }
                        "session/new" => {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": req["id"],
                                    "result": {
                                        "sessionId": "sess-int-test",
                                        "models": {
                                            "currentModelId": "gemini-3.1-pro",
                                            "availableModels": [{ "modelId": "gemini-3.1-pro" }]
                                        }
                                    }
                                }),
                            )
                            .await;
                        }
                        "session/prompt" => {
                            let text = req
                                .pointer("/params/prompt/0/text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            prompts_bg.lock().await.push(text.clone());
                            if text.starts_with("SLOW_") {
                                // Emit a partial streaming chunk and hold the RPC open until `session/cancel`
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "method": "session/update",
                                        "params": {
                                            "sessionId": "sess-int-test",
                                            "update": {
                                                "sessionUpdate": "agent_message_chunk",
                                                "content": { "type": "text", "text": format!("partial:{text}") }
                                            }
                                        }
                                    }),
                                )
                                .await;
                                inflight_prompt_id = Some(req["id"].clone());
                            } else {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "method": "session/update",
                                        "params": {
                                            "sessionId": "sess-int-test",
                                            "update": {
                                                "sessionUpdate": "agent_message_chunk",
                                                "content": { "type": "text", "text": format!("reply:{text}") }
                                            }
                                        }
                                    }),
                                )
                                .await;
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": req["id"],
                                        "result": { "stopReason": "end_turn" }
                                    }),
                                )
                                .await;
                            }
                        }
                        "session/cancel" => {
                            cancel_bg.fetch_add(1, Ordering::SeqCst);
                            if let Some(pid) = inflight_prompt_id.take() {
                                // Emit a trailing chunk right before cancel completes to verify it is dropped!
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "method": "session/update",
                                        "params": {
                                            "sessionId": "sess-int-test",
                                            "update": {
                                                "sessionUpdate": "agent_message_chunk",
                                                "content": { "type": "text", "text": "TRAILING_LEAK_MUST_DROP" }
                                            }
                                        }
                                    }),
                                )
                                .await;
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": pid,
                                        "result": { "stopReason": "cancelled" }
                                    }),
                                )
                                .await;
                            }
                        }
                        _ => {}
                    }
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock shim");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_int_test".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: None,
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
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };

        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // Case 1: Start SLOW_1, queue STALE_QUEUED, then send DriverCommand::Interrupt ->
        // partial text must finalize immediately, STALE_QUEUED must be cleared, and trailing leak dropped!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_slow_1".into(),
                prompt: "SLOW_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send slow 1");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow 1 stream")
                .expect("ev");
            if matches!(
                ev,
                DriverEvent::AssistantText {
                    ref text,
                    streaming: true,
                    ..
                } if text == "partial:SLOW_1"
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_stale_q".into(),
                prompt: "STALE_QUEUED_MUST_NOT_RUN".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: Vec::new(),
            })
            .await
            .expect("queue stale");
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("interrupt");

        let mut saw_partial_final = false;
        let mut saw_trailing_leak = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("interrupt settle")
                .expect("ev");
            match ev {
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "partial:SLOW_1" => saw_partial_final = true,
                DriverEvent::AssistantText { text, .. }
                    if text.contains("TRAILING_LEAK_MUST_DROP") =>
                {
                    saw_trailing_leak = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert!(
            saw_partial_final,
            "Interrupt must finalize partial msg_accum"
        );
        assert!(
            !saw_trailing_leak,
            "trailing chunks during cancel teardown must be dropped"
        );

        // Case 2: Start SLOW_2, queue FIFO_PRESERVED, then preempt directly with
        // ActiveTurnBehavior::Interrupt -> REPLACEMENT_NOW must execute immediately ahead of
        // FIFO_PRESERVED, and FIFO_PRESERVED must NOT be lost (it runs right after)!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_slow_2".into(),
                prompt: "SLOW_2".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send slow 2");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow 2 stream")
                .expect("ev");
            if matches!(
                ev,
                DriverEvent::AssistantText {
                    ref text,
                    streaming: true,
                    ..
                } if text == "partial:SLOW_2"
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_fifo_2".into(),
                prompt: "FIFO_PRESERVED".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: Vec::new(),
            })
            .await
            .expect("queue fifo 2");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_replace".into(),
                prompt: "REPLACEMENT_NOW".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send interrupt replacement");

        let mut saw_slow2_final = false;
        let mut saw_replacement_reply = false;
        let mut saw_fifo_reply = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("replace settle")
                .expect("ev");
            match ev {
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "partial:SLOW_2" => saw_slow2_final = true,
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "reply:REPLACEMENT_NOW" => saw_replacement_reply = true,
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } if text == "reply:FIFO_PRESERVED" => saw_fifo_reply = true,
                DriverEvent::AssistantText { text, .. }
                    if text.contains("TRAILING_LEAK_MUST_DROP") =>
                {
                    panic!("trailing leak must not appear after replacement prompt");
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert!(saw_slow2_final);
        assert!(saw_replacement_reply);
        assert!(
            saw_fifo_reply,
            "accepted FIFO queue entry must be preserved after Interrupt replacement"
        );

        let prompts = received_prompts.lock().await;
        assert_eq!(
            *prompts,
            vec![
                "SLOW_1".to_string(),
                "SLOW_2".to_string(),
                "REPLACEMENT_NOW".to_string(),
                "FIFO_PRESERVED".to_string(),
            ],
            "Stop clears queue; Interrupt replacement runs immediately ahead of preserved FIFO queue"
        );
        assert_eq!(cancel_count.load(Ordering::SeqCst), 2);
        assert!(is_send_message_tool_name("Send message"));
        assert!(is_send_message_tool_name("Run send message"));
        assert!(is_send_message_tool_name("send message to subagent"));

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn agy_agent_steer_degrades_to_queue_and_interrupt_and_drain_queue_preserves_fifo() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let sock_path = std::env::temp_dir().join(format!("o_agy_deliv_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let received_prompts: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&received_prompts);
        let cancel_count = Arc::new(AtomicU64::new(0));
        let cancel_bg = Arc::clone(&cancel_count);
        let (release_think_tx, mut release_think_rx) = mpsc::channel::<()>(4);

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

            let mut inflight_prompt_id: Option<Value> = None;

            loop {
                tokio::select! {
                    _ = release_think_rx.recv() => {
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": "sess-deliv-1",
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": "done:THINK_SLOW" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": pid,
                                    "result": { "stopReason": "end_turn" }
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
                            "initialize" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": req["id"],
                                        "result": { "protocolVersion": 1 }
                                    }),
                                )
                                .await;
                            }
                            "session/new" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": req["id"],
                                        "result": {
                                            "sessionId": "sess-deliv-1",
                                            "models": {
                                                "currentModelId": "gemini-3.1-pro",
                                                "availableModels": [{ "modelId": "gemini-3.1-pro" }]
                                            }
                                        }
                                    }),
                                )
                                .await;
                            }
                            "session/prompt" => {
                                let text = req
                                    .pointer("/params/prompt/0/text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                // Real `agy acp` completes any prior in-flight `session/prompt`
                                // immediately with `end_turn` when a concurrent steer arrives.
                                if let Some(prev_pid) = inflight_prompt_id.take() {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": prev_pid,
                                            "result": { "stopReason": "end_turn" }
                                        }),
                                    )
                                    .await;
                                }
                                prompts_bg.lock().await.push(text.clone());
                                if text.starts_with("THINK_") {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-deliv-1",
                                                "update": {
                                                    "sessionUpdate": "agent_thought_chunk",
                                                    "content": { "type": "text", "text": "thinking..." }
                                                }
                                            }
                                        }),
                                    )
                                    .await;
                                    inflight_prompt_id = Some(req["id"].clone());
                                } else if text.starts_with("SLOW_") {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-deliv-1",
                                                "update": {
                                                    "sessionUpdate": "agent_thought_chunk",
                                                    "content": { "type": "text", "text": format!("thinking:{text}") }
                                                }
                                            }
                                        }),
                                    )
                                    .await;
                                    inflight_prompt_id = Some(req["id"].clone());
                                } else {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-deliv-1",
                                                "update": {
                                                    "sessionUpdate": "agent_message_chunk",
                                                    "content": { "type": "text", "text": format!("reply:{text}") }
                                                }
                                            }
                                        }),
                                    )
                                    .await;
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": req["id"],
                                            "result": { "stopReason": "end_turn" }
                                        }),
                                    )
                                    .await;
                                }
                            }
                            "session/cancel" => {
                                cancel_bg.fetch_add(1, Ordering::SeqCst);
                                if let Some(pid) = inflight_prompt_id.take() {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": pid,
                                            "result": { "stopReason": "cancelled" }
                                        }),
                                    )
                                    .await;
                                }
                            }
                            _ => {
                                if let Some(id) = req.get("id") {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": id,
                                            "result": {}
                                        }),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect mock shim");
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_deliv_test".into(),
            provider: ProviderKind::Antigravity,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("gemini-3.1-pro".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::new(),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: seq_tracker,
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        // Phase A: Long thinking turn + Agent Queue -> 0 session/cancel, Queued -> Dispatched -> Settled
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_think".into(),
                prompt: "THINK_SLOW".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send think");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("think chunk")
                .expect("ev");
            if matches!(ev, DriverEvent::ThinkingText { .. }) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_ag_q1".into(),
                prompt: "AGENT_Q1".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_ag_q1".into()],
            })
            .await
            .expect("queue agent msg");
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            received_prompts.lock().await.len(),
            1,
            "queued agent msg must not dispatch while THINK_SLOW is active"
        );
        release_think_tx.send(()).await.expect("release think");

        let mut phase_a_statuses = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase a settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    blocking_turn_id,
                    ..
                } if message_ids == vec!["msg_ag_q1".to_string()] => {
                    phase_a_statuses.push((status, blocking_turn_id));
                }
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if phase_a_statuses
                        .iter()
                        .any(|(s, _)| *s == AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            phase_a_statuses,
            vec![
                (
                    AgentMessageDeliveryStatus::Queued,
                    Some("t_think".to_string())
                ),
                (AgentMessageDeliveryStatus::Dispatched, None),
                (AgentMessageDeliveryStatus::Settled, None),
            ]
        );

        // Phase B: Agent Steer degrades to Queue (0 cancel), then Human Steer preempts first!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_slow_b".into(),
                prompt: "SLOW_B".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send slow b");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow b thinking")
                .expect("ev");
            if matches!(ev, DriverEvent::ThinkingText { .. }) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_ag_steer".into(),
                prompt: "AGENT_DEGRADED_STEER".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_ag_steer".into()],
            })
            .await
            .expect("agent steer");
        let queued_ev = loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("degraded queued")
                .expect("ev");
            if let DriverEvent::PromptDeliveryUpdated {
                message_ids,
                status,
                blocking_turn_id,
                ..
            } = ev
                && message_ids == vec!["msg_ag_steer".to_string()]
            {
                break (status, blocking_turn_id);
            }
        };
        assert_eq!(
            queued_ev,
            (
                AgentMessageDeliveryStatus::Queued,
                Some("t_slow_b".to_string())
            )
        );
        assert_eq!(
            cancel_count.load(Ordering::SeqCst),
            0,
            "PromptOrigin::Agent Steer MUST degrade to Queue with zero session/cancel"
        );

        // Now Human Steer -> native concurrent session/prompt (0 cancel!), HUMAN_NOW runs 1st,
        // AGENT_DEGRADED_STEER runs 2nd once HUMAN_NOW settles.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_human_steer".into(),
                prompt: "HUMAN_NOW".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("human steer");
        let mut steer_statuses = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase b settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } if message_ids == vec!["msg_ag_steer".to_string()] => {
                    steer_statuses.push(status);
                }
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if steer_statuses.contains(&AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(
            cancel_count.load(Ordering::SeqCst),
            0,
            "Human Steer on agy acp MUST use native concurrent session/prompt with zero session/cancel"
        );
        assert_eq!(
            steer_statuses,
            vec![
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );

        // Phase C: 2 queued agent messages + InterruptAndDrainQueue -> 1 cancel, FIFO drain
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_slow_c".into(),
                prompt: "SLOW_C".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send slow c");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow c thinking")
                .expect("ev");
            if matches!(ev, DriverEvent::ThinkingText { .. }) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_fifo_c1".into(),
                prompt: "FIFO_C1".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_c1".into()],
            })
            .await
            .expect("q c1");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_fifo_c2".into(),
                prompt: "FIFO_C2".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_c2".into()],
            })
            .await
            .expect("q c2");
        cmd_tx
            .send(DriverCommand::InterruptAndDrainQueue)
            .await
            .expect("interrupt and drain");

        let mut c1_states = Vec::new();
        let mut c2_states = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase c settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } => {
                    if message_ids == vec!["msg_c1".to_string()] {
                        c1_states.push(status);
                    } else if message_ids == vec!["msg_c2".to_string()] {
                        c2_states.push(status);
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if c2_states.contains(&AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            c1_states,
            vec![
                AgentMessageDeliveryStatus::Queued,
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );
        assert_eq!(
            c2_states,
            vec![
                AgentMessageDeliveryStatus::Queued,
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );

        // Phase D: hard Interrupt drops queued messages
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_slow_d".into(),
                prompt: "SLOW_D".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send slow d");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow d thinking")
                .expect("ev");
            if matches!(ev, DriverEvent::ThinkingText { .. }) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_drop".into(),
                prompt: "MUST_DROP".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_drop".into()],
            })
            .await
            .expect("queue drop");
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("hard stop");
        let mut saw_dropped = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("phase d settle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } if message_ids == vec!["msg_drop".to_string()]
                    && status == AgentMessageDeliveryStatus::Dropped =>
                {
                    saw_dropped = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_dropped => break,
                _ => {}
            }
        }
        assert!(saw_dropped);

        let prompts = received_prompts.lock().await;
        assert_eq!(
            *prompts,
            vec![
                "THINK_SLOW".to_string(),
                "AGENT_Q1".to_string(),
                "SLOW_B".to_string(),
                "HUMAN_NOW".to_string(),
                "AGENT_DEGRADED_STEER".to_string(),
                "SLOW_C".to_string(),
                "FIFO_C1".to_string(),
                "FIFO_C2".to_string(),
                "SLOW_D".to_string(),
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn background_task_tracker_labels_timers_separately_from_bash_tasks() {
        let mut tracker = BackgroundTaskTracker::default();
        tracker.record_started_with_expiry(
            "s1/task-1".into(),
            None,
            None,
            Some(BackgroundTaskInfo {
                task_id: Some("s1/task-1".into()),
                label: Some("Timer: 5m, Prompt: Check status".into()),
                log_path: None,
            }),
            Some(Utc::now() + chrono::Duration::minutes(5)),
        );
        assert_eq!(tracker.waiting_source_label(), "waiting:1 timer");

        tracker.record_started_with_expiry(
            "s1/task-2".into(),
            None,
            None,
            Some(BackgroundTaskInfo {
                task_id: Some("s1/task-2".into()),
                label: Some("Cron: */5 * * * *".into()),
                log_path: None,
            }),
            None,
        );
        assert_eq!(tracker.waiting_source_label(), "waiting:2 timers");

        // Adding a non-timer background task switches label to generic background tasks
        tracker.record_started(
            "s1/task-3".into(),
            None,
            None,
            Some(BackgroundTaskInfo {
                task_id: Some("s1/task-3".into()),
                label: Some("ffmpeg render".into()),
                log_path: None,
            }),
        );
        assert_eq!(tracker.waiting_source_label(), "waiting:3 background tasks");
    }

    #[test]
    fn wakeup_monitor_settles_idle_timer_without_trailing_done_planner_step() {
        let dir = std::env::temp_dir().join(format!(
            "orbit_wake_settle_test_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir");
        let full_path = logs.join("transcript_full.jsonl");
        let sess = "sess_wake_settle";

        let content = [
            json!({
                "step_index": 1,
                "type": "USER_INPUT",
                "status": "DONE",
                "content": "Wait 5m then check"
            }),
            json!({
                "step_index": 2,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "content": "",
                "tool_calls": [{ "id": "call_sched_1", "name": "schedule", "args": { "DurationSeconds": 300, "Prompt": "check" } }]
            }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        std::fs::write(&full_path, &content).expect("write");

        let mut monitor = AgyTranscriptMonitor::from_brain_dir_with_session(dir.clone(), sess);
        monitor.sync_silent();
        assert!(!monitor.in_wakeup);

        // Append wakeup + schedule re-arm without trailing PLANNER_RESPONSE(DONE)
        let append = [
            json!({
                "step_index": 3,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "[SYSTEM_MESSAGE]\nTimer fired"
            }),
            json!({
                "step_index": 4,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "content": "",
                "tool_calls": [{ "id": "call_sched_2", "name": "schedule", "args": { "DurationSeconds": 120, "Prompt": "check again" } }]
            }),
            json!({
                "step_index": 5,
                "type": "GENERIC",
                "status": "RUNNING",
                "content": format!("Tool is running as a background task with task id: {sess}/task-88\nTask Description: Timer: 2m")
            }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&full_path)
            .expect("open append");
        std::io::Write::write_all(&mut f, append.as_bytes()).expect("append");

        let steps = monitor.poll_post_turn_wakeup();
        assert!(
            steps
                .iter()
                .any(|s| matches!(s, WakeupStep::Started { .. }))
        );
        assert!(monitor.in_wakeup);
        assert!(monitor.pending_tools.is_empty());
        assert!(!monitor.active_bg_tasks.is_empty());

        // Simulate idle grace elapsed -> monitor settles back to waiting:timer state
        let mut settled_out = Vec::new();
        monitor.check_wakeup_idle_settle(
            &mut settled_out,
            Instant::now() + WAKEUP_IDLE_SETTLE_GRACE + Duration::from_secs(1),
        );
        assert!(!monitor.in_wakeup);
        assert!(settled_out.iter().any(|s| matches!(s, WakeupStep::Settled)));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn map_agy_tool_detail_preserves_explicit_shell_cwd() {
        let detail = map_agy_tool_detail(
            &json!({
                "kind": "execute",
                "title": "Run shell command",
                "rawInput": {
                    "CommandLine": "cargo test",
                    "Cwd": "/outside/other-repo"
                }
            }),
            None,
        );
        match detail {
            ToolCallDetail::Shell { command, cwd, .. } => {
                assert_eq!(command, "cargo test");
                assert_eq!(cwd.as_deref(), Some("/outside/other-repo"));
            }
            other => panic!("expected Shell detail, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn agy_interrupt_and_drain_queue_clears_bg_tracker_and_returns_idle_on_empty_queue() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let tmp_data = std::env::temp_dir().join(format!("o_agy_empty_iq_{short_id}"));
        let sess_id = format!("sess-empty-iq-{short_id}");
        let logs_dir = tmp_data
            .join("brain")
            .join(&sess_id)
            .join(".system_generated/logs");
        std::fs::create_dir_all(&logs_dir).expect("mkdir logs");
        let transcript_path = logs_dir.join("transcript_full.jsonl");
        let sock_path = std::env::temp_dir().join(format!("o_agy_empty_iq_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let prompts_seen: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&prompts_seen);
        let tx_path = transcript_path.clone();
        let sess_srv = sess_id.clone();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut inflight_prompt_id: Option<Value> = None;

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

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) =
                    serde_json::from_str::<ShimFrame>(&raw_line)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }),
                        )
                        .await;
                    }
                    "session/new" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": sess_srv } }),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        let text = req
                            .pointer("/params/prompt/0/text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        prompts_bg.lock().await.push(text.clone());
                        if text == "START_BG_TURN" {
                            inflight_prompt_id = Some(req["id"].clone());
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "tool_call",
                                            "toolCallId": "call_bg_empty_iq",
                                            "kind": "execute",
                                            "status": "in_progress",
                                            "rawInput": { "CommandLine": "sleep 60" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let lines = [
                                json!({
                                    "step_index": 0,
                                    "type": "PLANNER_RESPONSE",
                                    "status": "DONE",
                                    "content": "",
                                    "tool_calls": [{
                                        "id": "t_bg_1",
                                        "name": "run_command",
                                        "args": { "CommandLine": "sleep 60" }
                                    }]
                                }),
                                json!({
                                    "step_index": 1,
                                    "type": "GENERIC",
                                    "status": "RUNNING",
                                    "content": format!("Tool is running as a background task with task id: {sess_srv}/task-1\nTask Description: sleep 60")
                                }),
                            ]
                            .iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                                + "\n";
                            std::fs::write(&tx_path, lines).expect("write transcript");
                        } else {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": format!("reply:{text}") }
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "stopReason": "end_turn" } }),
                            )
                            .await;
                        }
                    }
                    "session/cancel" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": {} }),
                        )
                        .await;
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": pid, "result": { "stopReason": "cancelled" } }),
                            )
                            .await;
                        }
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
        let seq_tracker = Arc::new(AtomicU64::new(0));
        let mut extra_env = HashMap::new();
        extra_env.insert(
            "ANTIGRAVITY_EXECUTABLE_DATA_DIR".into(),
            tmp_data.to_string_lossy().to_string(),
        );
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_empty_iq".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env,
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: seq_tracker,
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };

        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "START_BG_TURN".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t1");

        // Wait until transcript poller binds `sess-empty-iq/task-1` onto `call_bg_empty_iq`.
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("bg task attach timeout")
                .expect("ev");
            if matches!(
                ev,
                DriverEvent::ToolCall {
                    ref call_id,
                    status: ToolCallStatus::Running,
                    background_task: Some(_),
                    ..
                } if call_id == "call_bg_empty_iq"
            ) {
                break;
            }
        }

        // Send InterruptAndDrainQueue with an EMPTY queue: must settle to Idle, NOT bgwait!
        cmd_tx
            .send(DriverCommand::InterruptAndDrainQueue)
            .await
            .expect("send interrupt and drain");

        let mut settled_idle = false;
        let mut entered_bgwait = false;
        let mut reconciled_bg = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(4), evt_rx.recv()).await {
            match ev {
                DriverEvent::ReconcileBackgroundTasks(items)
                    if items.iter().any(|(id, st)| {
                        id == "call_bg_empty_iq" && *st == ToolCallStatus::Failed
                    }) =>
                {
                    reconciled_bg = true;
                }
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Failed,
                    ..
                } if call_id == "call_bg_empty_iq" => {
                    reconciled_bg = true;
                }
                DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup { .. }) => {
                    entered_bgwait = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    settled_idle = true;
                    break;
                }
                _ => {}
            }
        }
        assert!(
            settled_idle,
            "empty-queue InterruptAndDrainQueue must settle to Idle"
        );
        assert!(
            !entered_bgwait,
            "must NEVER enter waiting:* background state after cancel"
        );
        assert!(
            reconciled_bg,
            "open background shell must be settled Failed on cancel"
        );

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t2".into(),
                prompt: "FOLLOWUP_OK".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t2");
        let mut saw_t2_reply = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await {
            match ev {
                DriverEvent::AssistantText { text, .. } if text == "reply:FOLLOWUP_OK" => {
                    saw_t2_reply = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_t2_reply => break,
                _ => {}
            }
        }
        assert!(
            saw_t2_reply,
            "follow-up prompt must succeed after interrupt"
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_dir_all(&tmp_data);
    }

    #[tokio::test]
    async fn agy_human_steer_reconciles_bg_tasks_in_place_without_completed_regression_and_carries_rollback_handoff()
     {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let tmp_data = std::env::temp_dir().join(format!("o_agy_steer_h_{short_id}"));
        let sess_id = format!("sess-steer-{short_id}");
        let logs_dir = tmp_data
            .join("brain")
            .join(&sess_id)
            .join(".system_generated/logs");
        std::fs::create_dir_all(&logs_dir).expect("mkdir logs");
        let transcript_path = logs_dir.join("transcript_full.jsonl");
        let sock_path = std::env::temp_dir().join(format!("o_agy_steer_h_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let prompts_seen: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&prompts_seen);
        let tx_path = transcript_path.clone();
        let sess_srv = sess_id.clone();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut inflight_prompt_id: Option<Value> = None;

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

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) =
                    serde_json::from_str::<ShimFrame>(&raw_line)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }),
                        )
                        .await;
                    }
                    "session/new" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": sess_srv } }),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        let text = req
                            .pointer("/params/prompt/0/text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        prompts_bg.lock().await.push(text.clone());
                        if text == "TURN1_SLOW" {
                            inflight_prompt_id = Some(req["id"].clone());
                            // 1) Finished short python3 shell BEFORE bg task
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "tool_call",
                                            "toolCallId": "call_done_py",
                                            "kind": "execute",
                                            "status": "completed",
                                            "rawInput": { "CommandLine": "python3 -c \"print(1)\"" },
                                            "rawOutput": { "exitCode": 0, "stdout": "1\n" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            // 2) Long background python3 shell
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "tool_call",
                                            "toolCallId": "call_bg_py",
                                            "kind": "execute",
                                            "status": "in_progress",
                                            "rawInput": { "CommandLine": "python3 -c \"import time; time.sleep(60)\"" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            let lines = [
                                json!({
                                    "step_index": 0,
                                    "type": "PLANNER_RESPONSE",
                                    "status": "DONE",
                                    "content": "",
                                    "tool_calls": [
                                        { "id": "tp_1", "name": "run_command", "args": { "CommandLine": "python3 -c \"print(1)\"" } },
                                        { "id": "tp_2", "name": "run_command", "args": { "CommandLine": "python3 -c \"import time; time.sleep(60)\"" } }
                                    ]
                                }),
                                json!({
                                    "step_index": 1,
                                    "type": "GENERIC",
                                    "status": "DONE",
                                    "content": "1\n"
                                }),
                                json!({
                                    "step_index": 2,
                                    "type": "GENERIC",
                                    "status": "RUNNING",
                                    "content": format!("Tool is running as a background task with task id: {sess_srv}/task-9\nTask Description: python3 -c")
                                }),
                            ]
                            .iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                                + "\n";
                            std::fs::write(&tx_path, lines).expect("write transcript");
                            // 3) Partial streamed prose NOT committed to PLANNER_RESPONSE(DONE)
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": "PARTIAL_ROLLED_BACK_PROSE" }
                                        }
                                    }
                                }),
                            )
                            .await;
                        } else if text.contains("HUMAN_STEER_2") {
                            // Commit this turn's text to transcript_full so Turn 3 does not re-inject it.
                            inflight_prompt_id = Some(req["id"].clone());
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": "COMMITTED_TURN2_PROSE" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            let mut f = std::fs::OpenOptions::new()
                                .append(true)
                                .open(&tx_path)
                                .expect("open tx");
                            use std::io::Write;
                            writeln!(
                                f,
                                "{}",
                                json!({
                                    "step_index": 3,
                                    "type": "PLANNER_RESPONSE",
                                    "status": "DONE",
                                    "content": "COMMITTED_TURN2_PROSE"
                                })
                            )
                            .expect("append committed");
                        } else {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": "TURN3_DONE" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "stopReason": "end_turn" } }),
                            )
                            .await;
                        }
                    }
                    "session/cancel" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": {} }),
                        )
                        .await;
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": pid, "result": { "stopReason": "cancelled" } }),
                            )
                            .await;
                        }
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
        let mut extra_env = HashMap::new();
        extra_env.insert(
            "ANTIGRAVITY_EXECUTABLE_DATA_DIR".into(),
            tmp_data.to_string_lossy().to_string(),
        );
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_steer_handoff".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env,
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "TURN1_SLOW".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t1");

        let mut saw_bg_attached = false;
        let mut saw_partial = false;
        while !(saw_bg_attached && saw_partial) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("t1 stream timeout")
                .expect("ev");
            match ev {
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Running,
                    ..
                } if call_id == "call_done_py" => {
                    panic!("already-completed python shell regressed to Running!");
                }
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Running,
                    background_task: Some(_),
                    ..
                } if call_id == "call_bg_py" => {
                    saw_bg_attached = true;
                }
                DriverEvent::AssistantText { text, .. }
                    if text.contains("PARTIAL_ROLLED_BACK_PROSE") =>
                {
                    saw_partial = true;
                }
                _ => {}
            }
        }

        // Explicit Human Interrupt cancels Turn 1 (unlike non-destructive Steer).
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t2".into(),
                prompt: "HUMAN_STEER_2".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t2 interrupt");

        let mut saw_reconcile_bg = false;
        let mut saw_t2_msg = false;
        while !(saw_reconcile_bg && saw_t2_msg) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("t2 stream timeout")
                .expect("ev");
            match ev {
                DriverEvent::ReconcileBackgroundTasks(items)
                    if items
                        .iter()
                        .any(|(id, st)| id == "call_bg_py" && *st == ToolCallStatus::Failed) =>
                {
                    saw_reconcile_bg = true;
                }
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Running,
                    ..
                } if call_id == "call_done_py" => {
                    panic!("completed python shell regressed during interrupt!");
                }
                DriverEvent::ToolCall { call_id, .. } if saw_t2_msg && call_id == "call_bg_py" => {
                    panic!("turn 1 bg tool call re-emitted into turn 2 via ToolCall!");
                }
                DriverEvent::AssistantText { text, .. }
                    if text.contains("COMMITTED_TURN2_PROSE") =>
                {
                    saw_t2_msg = true;
                }
                _ => {}
            }
        }

        // Now interrupt Turn 2 (whose reply WAS committed to transcript_full.jsonl) with Turn 3:
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t3".into(),
                prompt: "HUMAN_STEER_3".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t3 interrupt");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("t3 settle timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        let prompts = prompts_seen.lock().await.clone();
        assert_eq!(prompts.len(), 3);
        assert!(
            prompts[1].contains("[Interrupted Prior Turn Context]")
                && prompts[1].contains("PARTIAL_ROLLED_BACK_PROSE")
                && prompts[1].contains("HUMAN_STEER_2"),
            "Turn 2 must carry rollback handoff for uncommitted Turn 1 partial prose, got: {}",
            prompts[1]
        );
        assert!(
            !prompts[2].contains("[Interrupted Prior Turn Context]"),
            "Turn 3 must NOT duplicate handoff when Turn 2 reply already committed to transcript: {}",
            prompts[2]
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_dir_all(&tmp_data);
    }

    #[test]
    fn find_matching_call_id_for_bg_task_multiline_python_never_hijacks_earlier_completed_shell() {
        let mut reg = WakeupToolRegistry::default();
        reg.begin_turn();
        reg.resolve_acp_tool(
            "call_py_done",
            ToolCallStatus::Completed,
            ToolCallDetail::Shell {
                command: "python3 -c \"print('done')\"".into(),
                cwd: None,
                exit_code: Some(0),
                output_preview: Some("done".into()),
            },
            None,
            false,
        );
        reg.resolve_acp_tool(
            "call_py_bg",
            ToolCallStatus::Running,
            ToolCallDetail::Shell {
                command: "python3 -c \"import time\ntime.sleep(60)\"".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
            None,
            false,
        );
        assert_eq!(
            reg.find_matching_call_id_for_bg_task(
                Some("python3 -c"),
                Some("python3 -c \"import time\ntime.sleep(60)\""),
            ),
            Some("call_py_bg".into())
        );

        // If ONLY an already-finished shell exists, an ambiguous truncated `python3 -c` must return None
        // and `mark_background_task` must refuse to regress the finished shell back to Running.
        let mut reg2 = WakeupToolRegistry::default();
        reg2.begin_turn();
        reg2.resolve_acp_tool(
            "call_py_done_only",
            ToolCallStatus::Completed,
            ToolCallDetail::Shell {
                command: "python3 -c \"print('done')\"".into(),
                cwd: None,
                exit_code: Some(0),
                output_preview: Some("done".into()),
            },
            None,
            false,
        );
        assert_eq!(
            reg2.find_matching_call_id_for_bg_task(Some("python3 -c"), None),
            None
        );
        assert!(
            reg2.mark_background_task(
                "call_py_done_only",
                BackgroundTaskInfo {
                    task_id: Some("sess/task-999".into()),
                    label: Some("python3 -c".into()),
                    log_path: None,
                },
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn agy_subagent_content_uuid_foreground_polling_cancel_and_bg_task_settlement() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let tmp_data = std::env::temp_dir().join(format!("o_agy_sub_fg_{short_id}"));
        let sess_id = format!("sess-sub-fg-{short_id}");
        let sub_id = "aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb";
        let sub_logs = tmp_data
            .join("brain")
            .join(sub_id)
            .join(".system_generated/logs");
        std::fs::create_dir_all(&sub_logs).expect("mkdir sub logs");
        let sub_tx_path = sub_logs.join("transcript_full.jsonl");
        let sock_path = std::env::temp_dir().join(format!("o_agy_sub_fg_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let sub_tx_srv = sub_tx_path.clone();
        let sess_srv = sess_id.clone();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut inflight_prompt_id: Option<Value> = None;

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

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) =
                    serde_json::from_str::<ShimFrame>(&raw_line)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }),
                        )
                        .await;
                    }
                    "session/new" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": sess_srv } }),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        inflight_prompt_id = Some(req["id"].clone());
                        // Subagent spawned with conversationId embedded in `content`, not `rawOutput`.
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({
                                "jsonrpc": "2.0",
                                "method": "session/update",
                                "params": {
                                    "sessionId": sess_srv,
                                    "update": {
                                        "sessionUpdate": "tool_call",
                                        "toolCallId": "call_sub_content",
                                        "title": "Invoke subagent",
                                        "status": "completed",
                                        "rawInput": { "Role": "Inspector", "Prompt": "Inspect codebase" },
                                        "content": [{
                                            "type": "content",
                                            "content": {
                                                "type": "text",
                                                "text": format!("Subagent launched conversationId={sub_id}")
                                            }
                                        }]
                                    }
                                }
                            }),
                        )
                        .await;
                        let sub_lines = [
                            json!({
                                "step_index": 0,
                                "type": "PLANNER_RESPONSE",
                                "status": "DONE",
                                "thinking": "Inspecting module now",
                                "content": "",
                                "tool_calls": [{
                                    "id": "sub_tool_1",
                                    "name": "run_command",
                                    "args": { "CommandLine": "sleep 30" }
                                }]
                            }),
                            json!({
                                "step_index": 1,
                                "type": "GENERIC",
                                "status": "RUNNING",
                                "content": format!("Tool is running as a background task with task id: {sub_id}/task-1\nTask Description: sleep 30")
                            }),
                        ]
                        .iter()
                        .map(Value::to_string)
                        .collect::<Vec<_>>()
                        .join("\n")
                            + "\n";
                        std::fs::write(&sub_tx_srv, sub_lines).expect("write sub tx");
                    }
                    "session/cancel" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": {} }),
                        )
                        .await;
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": pid, "result": { "stopReason": "cancelled" } }),
                            )
                            .await;
                        }
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
        let mut extra_env = HashMap::new();
        extra_env.insert(
            "ANTIGRAVITY_EXECUTABLE_DATA_DIR".into(),
            tmp_data.to_string_lossy().to_string(),
        );
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_sub_fg".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env,
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "LAUNCH_SUB".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send launch");

        // Must observe live subagent Running + thinking + tool WHILE parent turn is STILL running!
        let mut saw_running_upsert = false;
        let mut saw_sub_thinking = false;
        let mut saw_sub_tool_running = false;
        while !(saw_running_upsert && saw_sub_thinking && saw_sub_tool_running) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("foreground subagent live timeout")
                .expect("ev");
            match ev {
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.subagent_id == sub_id && d.status == ProviderSubagentStatus::Running =>
                {
                    saw_running_upsert = true;
                }
                DriverEvent::ProviderSubagentTimeline {
                    subagent_id,
                    payload: TimelinePayload::AssistantThinking { text, .. },
                } if subagent_id == sub_id && text.contains("Inspecting module now") => {
                    saw_sub_thinking = true;
                }
                DriverEvent::ProviderSubagentTimeline {
                    subagent_id,
                    payload:
                        TimelinePayload::ToolCall {
                            call_id,
                            status: ToolCallStatus::Running,
                            background_task: Some(_),
                            ..
                        },
                } if subagent_id == sub_id && call_id == "sub_tool_1" => {
                    saw_sub_tool_running = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    panic!("parent must still be running when subagent events arrive");
                }
                _ => {}
            }
        }

        // Now interrupt parent turn -> subagent must transition Canceled and settle open tool Failed!
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("interrupt parent");
        let mut saw_sub_canceled = false;
        let mut saw_sub_tool_failed = false;
        let mut parent_idle = false;
        while !(saw_sub_canceled && saw_sub_tool_failed && parent_idle) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("cancel subagent timeout")
                .expect("ev");
            match ev {
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.subagent_id == sub_id && d.status == ProviderSubagentStatus::Canceled =>
                {
                    saw_sub_canceled = true;
                }
                DriverEvent::ProviderSubagentTimeline {
                    subagent_id,
                    payload:
                        TimelinePayload::ToolCall {
                            call_id,
                            status: ToolCallStatus::Failed,
                            ..
                        },
                } if subagent_id == sub_id && call_id == "sub_tool_1" => {
                    saw_sub_tool_failed = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    parent_idle = true;
                }
                _ => {}
            }
        }

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_dir_all(&tmp_data);
    }

    #[tokio::test]
    async fn agy_stderr_disk_full_emits_error_notice_and_session_info_update_sets_title() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::AtomicU64;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let sock_path = std::env::temp_dir().join(format!("o_agy_stderr_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;

            async fn send_frame(wh: &mut ShimWriteHalf, frame: ShimFrame) {
                let mut enc = serde_json::to_string(&frame).expect("enc");
                enc.push('\n');
                let _ = wh.write_all(enc.as_bytes()).await;
            }

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) =
                    serde_json::from_str::<ShimFrame>(&raw_line)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_frame(
                            &mut wh,
                            ShimFrame::StdoutLine {
                                seq: Some(seq),
                                line: json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }).to_string(),
                            },
                        )
                        .await;
                        seq += 1;
                    }
                    "session/new" => {
                        send_frame(
                            &mut wh,
                            ShimFrame::StdoutLine {
                                seq: Some(seq),
                                line: json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "sess-err-title" } }).to_string(),
                            },
                        )
                        .await;
                        seq += 1;
                    }
                    "session/prompt" => {
                        send_frame(
                            &mut wh,
                            ShimFrame::StderrLine {
                                seq: Some(seq),
                                line: "harmless debug trace line".into(),
                            },
                        )
                        .await;
                        seq += 1;
                        send_frame(
                            &mut wh,
                            ShimFrame::StderrLine {
                                seq: Some(seq),
                                line: "FATAL: No space left on device (os error 28)".into(),
                            },
                        )
                        .await;
                        seq += 1;
                        send_frame(
                            &mut wh,
                            ShimFrame::StdoutLine {
                                seq: Some(seq),
                                line: json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": "sess-err-title",
                                        "update": {
                                            "sessionUpdate": "session_info_update",
                                            "title": "  Fix   auth   refresh  "
                                        }
                                    }
                                })
                                .to_string(),
                            },
                        )
                        .await;
                        seq += 1;
                        send_frame(
                            &mut wh,
                            ShimFrame::StdoutLine {
                                seq: Some(seq),
                                line: json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "stopReason": "end_turn" } }).to_string(),
                            },
                        )
                        .await;
                        seq += 1;
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
            agent_id: "ag_err_title".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
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
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "TRIGGER".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send");

        let mut saw_disk_notice = false;
        let mut saw_title = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(4), evt_rx.recv()).await {
            match ev {
                DriverEvent::Notice { level, message } => {
                    assert!(
                        !message.contains("harmless debug trace"),
                        "benign stderr line should not become a UI notice"
                    );
                    if level == "error" && message.contains("No space left on device") {
                        saw_disk_notice = true;
                    }
                }
                DriverEvent::SessionTitleUpdated { title } if title == "Fix auth refresh" => {
                    saw_title = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_disk_notice && saw_title => {
                    break;
                }
                _ => {}
            }
        }
        assert!(saw_disk_notice, "disk full stderr must emit error Notice");
        assert!(
            saw_title,
            "session_info_update must emit SessionTitleUpdated"
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn agy_subagent_cancel_settles_multiple_open_non_bg_planner_tools() {
        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let dir = std::env::temp_dir().join(format!("o_agy_sub_multi_open_{short_id}"));
        let logs = dir.join(".system_generated/logs");
        std::fs::create_dir_all(&logs).expect("mkdir logs");
        let sub_id = format!("sub-multi-open-{short_id}");
        let lines = [
            json!({
                "step_index": 0,
                "type": "SYSTEM_MESSAGE",
                "status": "DONE",
                "content": "Start inspection"
            }),
            json!({
                "step_index": 1,
                "type": "PLANNER_RESPONSE",
                "status": "DONE",
                "thinking": "Running shell and reading file",
                "content": "",
                "tool_calls": [
                    { "name": "run_command", "args": { "CommandLine": "sleep 20" } },
                    { "name": "read_file", "args": { "Path": "/tmp/sample.txt" } }
                ]
            }),
        ]
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
            + "\n";
        std::fs::write(logs.join("transcript_full.jsonl"), lines).expect("write tx");

        let now = Utc::now();
        let desc = ProviderSubagentDescriptor {
            subagent_id: sub_id.clone(),
            parent_agent_id: "ag_p".into(),
            parent_subagent_id: None,
            title: "Workspace Reader".into(),
            subtitle: None,
            status: ProviderSubagentStatus::Running,
            created_at: now,
            updated_at: now,
        };
        let mon = AgyTranscriptMonitor::from_brain_dir_subagent(dir.clone(), &sub_id);
        let mut monitors = HashMap::new();
        monitors.insert(sub_id.clone(), AgySubagentMonitorEntry::new(desc, mon));
        let (evt_tx, mut evt_rx) = mpsc::channel(32);

        if let Some(entry) = monitors.get_mut(&sub_id) {
            poll_agy_subagent_monitor(&sub_id, entry, &evt_tx).await;
        }
        assert_eq!(monitors[&sub_id].open_tools.len(), 2);

        cancel_agy_subagent_monitors(None, &mut monitors, &evt_tx).await;
        assert!(monitors[&sub_id].open_tools.is_empty());
        assert_eq!(
            monitors[&sub_id].desc.status,
            ProviderSubagentStatus::Canceled
        );

        let mut failed_calls = Vec::new();
        while let Ok(ev) = evt_rx.try_recv() {
            if let DriverEvent::ProviderSubagentTimeline {
                subagent_id,
                payload:
                    TimelinePayload::ToolCall {
                        call_id,
                        status: ToolCallStatus::Failed,
                        ..
                    },
            } = ev
                && subagent_id == sub_id
            {
                failed_calls.push(call_id);
            }
        }
        failed_calls.sort();
        assert_eq!(
            failed_calls,
            vec!["wake_tool_1_0".to_string(), "wake_tool_1_1".to_string()],
            "both parallel open non-bg planner tools must settle Failed when subagent cancels"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn agy_human_steer_preserves_active_subagents_and_background_tasks_without_cancel() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, AtomicUsize};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let tmp_data = std::env::temp_dir().join(format!("o_agy_steer_keep_{short_id}"));
        let sess_id = format!("sess-steer-keep-{short_id}");
        let sub_id = "bbbbbbbb-1111-2222-3333-cccccccccccc";
        let parent_logs = tmp_data
            .join("brain")
            .join(&sess_id)
            .join(".system_generated/logs");
        let sub_logs = tmp_data
            .join("brain")
            .join(sub_id)
            .join(".system_generated/logs");
        std::fs::create_dir_all(&parent_logs).expect("mkdir parent logs");
        std::fs::create_dir_all(&sub_logs).expect("mkdir sub logs");
        let parent_tx_path = parent_logs.join("transcript_full.jsonl");
        let sub_tx_path = sub_logs.join("transcript_full.jsonl");
        let sock_path = std::env::temp_dir().join(format!("o_agy_steer_keep_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let cancel_count = Arc::new(AtomicUsize::new(0));
        let cancel_srv = Arc::clone(&cancel_count);
        let prompts_seen: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_srv = Arc::clone(&prompts_seen);
        let parent_tx_srv = parent_tx_path.clone();
        let sub_tx_srv = sub_tx_path.clone();
        let sess_srv = sess_id.clone();

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut inflight_prompt_id: Option<Value> = None;

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

            while let Ok(Some(raw_line)) = lines.next_line().await {
                let Ok(ShimFrame::WriteStdin { line }) =
                    serde_json::from_str::<ShimFrame>(&raw_line)
                else {
                    continue;
                };
                let Ok(req) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                match method {
                    "initialize" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }),
                        )
                        .await;
                    }
                    "session/new" => {
                        send_stdout(
                            &mut wh,
                            &mut seq,
                            json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": sess_srv } }),
                        )
                        .await;
                    }
                    "session/prompt" => {
                        let text = req
                            .pointer("/params/prompt/0/text")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        prompts_srv.lock().await.push(text.clone());
                        // Real `agy acp` completes the prior prompt RPC immediately with
                        // `end_turn` when a concurrent steer arrives (zero force cancel).
                        if let Some(prev_pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": prev_pid, "result": { "stopReason": "end_turn" } }),
                            )
                            .await;
                        }
                        if text == "LAUNCH_DAEMON_AND_SUB" {
                            inflight_prompt_id = Some(req["id"].clone());
                            // Emit a background daemon tool call + subagent spawn while turn 1 stays open.
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "tool_call",
                                            "toolCallId": "call_tauri_daemon",
                                            "kind": "execute",
                                            "status": "in_progress",
                                            "rawInput": { "CommandLine": "npm run tauri dev" }
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "tool_call",
                                            "toolCallId": "call_sub_keep",
                                            "title": "Invoke subagent",
                                            "status": "completed",
                                            "rawInput": { "Role": "Parallel Worker", "Prompt": "Work in parallel" },
                                            "content": [{
                                                "type": "content",
                                                "content": {
                                                    "type": "text",
                                                    "text": format!("Subagent launched conversationId={sub_id}")
                                                }
                                            }]
                                        }
                                    }
                                }),
                            )
                            .await;
                            let parent_lines = [
                                json!({
                                    "step_index": 0,
                                    "type": "PLANNER_RESPONSE",
                                    "status": "DONE",
                                    "content": "",
                                    "tool_calls": [{
                                        "id": "tp_daemon",
                                        "name": "run_command",
                                        "args": { "CommandLine": "npm run tauri dev" }
                                    }]
                                }),
                                json!({
                                    "step_index": 1,
                                    "type": "GENERIC",
                                    "status": "RUNNING",
                                    "content": format!("Tool is running as a background task with task id: {sess_srv}/task-152\nTask Description: npm run tauri dev")
                                }),
                            ]
                            .iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                                + "\n";
                            std::fs::write(&parent_tx_srv, parent_lines).expect("write parent tx");
                            let sub_lines = [json!({
                                "step_index": 0,
                                "type": "PLANNER_RESPONSE",
                                "status": "DONE",
                                "thinking": "Worker investigating",
                                "content": ""
                            })]
                            .iter()
                            .map(Value::to_string)
                            .collect::<Vec<_>>()
                            .join("\n")
                                + "\n";
                            std::fs::write(&sub_tx_srv, sub_lines).expect("write sub tx");
                        } else {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "jsonrpc": "2.0",
                                    "method": "session/update",
                                    "params": {
                                        "sessionId": sess_srv,
                                        "update": {
                                            "sessionUpdate": "agent_message_chunk",
                                            "content": { "type": "text", "text": format!("steered:{text}") }
                                        }
                                    }
                                }),
                            )
                            .await;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "stopReason": "end_turn" } }),
                            )
                            .await;
                        }
                    }
                    "session/cancel" => {
                        cancel_srv.fetch_add(1, Ordering::SeqCst);
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": pid, "result": { "stopReason": "cancelled" } }),
                            )
                            .await;
                        }
                    }
                    _ => {}
                }
            }
        });

        let shim = orbit_shim::ShimClient::connect(&sock_path)
            .await
            .expect("connect");
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let mut extra_env = HashMap::new();
        extra_env.insert(
            "ANTIGRAVITY_EXECUTABLE_DATA_DIR".into(),
            tmp_data.to_string_lossy().to_string(),
        );
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_steer_keep".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env,
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "LAUNCH_DAEMON_AND_SUB".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send t1");

        // Wait until BOTH background daemon AND running subagent are registered live.
        let mut saw_daemon_bg = false;
        let mut saw_sub_running = false;
        while !(saw_daemon_bg && saw_sub_running) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("t1 daemon+sub timeout")
                .expect("ev");
            match ev {
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Running,
                    background_task: Some(bg),
                    ..
                } if call_id == "call_tauri_daemon"
                    && bg.task_id.as_deref() == Some(&format!("{sess_id}/task-152")) =>
                {
                    saw_daemon_bg = true;
                }
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.subagent_id == sub_id && d.status == ProviderSubagentStatus::Running =>
                {
                    saw_sub_running = true;
                }
                _ => {}
            }
        }

        // Send a Human Steer mid-turn while both daemon and subagent are running.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_steer".into(),
                prompt: "STEER_WITHOUT_KILLING".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send steer");

        // Steer reply must arrive cleanly AND settle into bgwait (NOT Idle, NOT Failed daemon,
        // NOT Canceled subagent, and ZERO session/cancel!).
        let mut saw_steer_reply = false;
        let mut returned_to_bgwait = false;
        while !(saw_steer_reply && returned_to_bgwait) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("steer settle timeout")
                .expect("ev");
            match ev {
                DriverEvent::ReconcileBackgroundTasks(items)
                    if items.iter().any(|(id, _)| id == "call_tauri_daemon") =>
                {
                    panic!("Human Steer MUST NOT reconcile active daemon background task Failed!");
                }
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Failed,
                    ..
                } if call_id == "call_tauri_daemon" => {
                    panic!("Human Steer MUST NOT mark active daemon background task Failed!");
                }
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.subagent_id == sub_id && d.status == ProviderSubagentStatus::Canceled =>
                {
                    panic!("Human Steer MUST NOT mark active subagent Canceled!");
                }
                DriverEvent::AssistantText { text, .. }
                    if text.contains("steered:STEER_WITHOUT_KILLING") =>
                {
                    saw_steer_reply = true;
                }
                DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup { source, .. })
                    if source.starts_with("waiting:") =>
                {
                    returned_to_bgwait = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    panic!(
                        "must remain in waiting:* background state while daemon is still running"
                    );
                }
                _ => {}
            }
        }
        assert_eq!(
            cancel_count.load(Ordering::SeqCst),
            0,
            "Human Steer MUST emit zero session/cancel"
        );

        // Now send a follow-up prompt while `active_prompt_rpc_key == None` (waiting on bg daemon + subagent):
        // must dispatch directly with zero session/cancel and still keep daemon + subagent alive!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_followup_wait".into(),
                prompt: "FOLLOWUP_DURING_BGWAIT".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("send followup during bgwait");

        let mut saw_followup_reply = false;
        let mut back_to_bgwait = false;
        while !(saw_followup_reply && back_to_bgwait) {
            let ev = tokio::time::timeout(Duration::from_secs(5), evt_rx.recv())
                .await
                .expect("followup during bgwait timeout")
                .expect("ev");
            match ev {
                DriverEvent::ReconcileBackgroundTasks(items)
                    if items.iter().any(|(id, _)| id == "call_tauri_daemon") =>
                {
                    panic!("follow-up during bgwait MUST NOT clear active daemon task!");
                }
                DriverEvent::ProviderSubagentUpsert(d)
                    if d.subagent_id == sub_id && d.status == ProviderSubagentStatus::Canceled =>
                {
                    panic!("follow-up during bgwait MUST NOT cancel active subagent!");
                }
                DriverEvent::AssistantText { text, .. }
                    if text.contains("steered:FOLLOWUP_DURING_BGWAIT") =>
                {
                    saw_followup_reply = true;
                }
                DriverEvent::StatusChanged(AgentStatus::AutonomousWakeup { source, .. })
                    if source.starts_with("waiting:") =>
                {
                    back_to_bgwait = true;
                }
                _ => {}
            }
        }
        assert_eq!(
            cancel_count.load(Ordering::SeqCst),
            0,
            "follow-up during bgwait MUST emit zero session/cancel"
        );
        assert_eq!(
            *prompts_seen.lock().await,
            vec![
                "LAUNCH_DAEMON_AND_SUB".to_string(),
                "STEER_WITHOUT_KILLING".to_string(),
                "FOLLOWUP_DURING_BGWAIT".to_string(),
            ]
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_dir_all(&tmp_data);
    }

    #[tokio::test]
    async fn agy_human_steer_falls_back_to_priority_queue_when_server_rejects_concurrent_prompt() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, AtomicUsize};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id = &uuid::Uuid::new_v4().simple().to_string()[..8];
        let sock_path = std::env::temp_dir().join(format!("o_agy_steer_fb_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path).await.expect("bind shim");

        let cancel_count = Arc::new(AtomicUsize::new(0));
        let cancel_srv = Arc::clone(&cancel_count);
        let completed_prompts: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let completed_srv = Arc::clone(&completed_prompts);
        let (release_tx, mut release_rx) = mpsc::channel::<()>(1);

        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (rh, mut wh) = stream.into_split();
            let mut lines = BufReader::new(rh).lines();
            let mut seq: u64 = 1;
            let mut inflight_prompt_id: Option<Value> = None;

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

            loop {
                tokio::select! {
                    _ = release_rx.recv() => {
                        if let Some(pid) = inflight_prompt_id.take() {
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({ "jsonrpc": "2.0", "id": pid, "result": { "stopReason": "end_turn" } }),
                            )
                            .await;
                        }
                    }
                    maybe_line = lines.next_line() => {
                        let Ok(Some(raw_line)) = maybe_line else { break; };
                        let Ok(ShimFrame::WriteStdin { line }) =
                            serde_json::from_str::<ShimFrame>(&raw_line)
                        else {
                            continue;
                        };
                        let Ok(req) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
                        match method {
                            "initialize" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }),
                                )
                                .await;
                            }
                            "session/new" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "sessionId": "sess-fb" } }),
                                )
                                .await;
                            }
                            "session/prompt" => {
                                let text = req
                                    .pointer("/params/prompt/0/text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                if inflight_prompt_id.is_some() {
                                    // Simulate a strict/older server rejecting concurrent prompts.
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "id": req["id"],
                                            "error": { "code": -32000, "message": "prompt already in progress" }
                                        }),
                                    )
                                    .await;
                                } else if text == "SLOW_STRICT" {
                                    completed_srv.lock().await.push(text);
                                    inflight_prompt_id = Some(req["id"].clone());
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-fb",
                                                "update": {
                                                    "sessionUpdate": "agent_thought_chunk",
                                                    "content": { "type": "text", "text": "strict thinking" }
                                                }
                                            }
                                        }),
                                    )
                                    .await;
                                } else {
                                    completed_srv.lock().await.push(text.clone());
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-fb",
                                                "update": {
                                                    "sessionUpdate": "agent_message_chunk",
                                                    "content": { "type": "text", "text": format!("ok:{text}") }
                                                }
                                            }
                                        }),
                                    )
                                    .await;
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "stopReason": "end_turn" } }),
                                    )
                                    .await;
                                }
                            }
                            "session/cancel" => {
                                cancel_srv.fetch_add(1, Ordering::SeqCst);
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
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_steer_fb".into(),
            provider: ProviderKind::Antigravity,
            orbit_exe: PathBuf::from("/bin/true"),
            executable: PathBuf::from("/bin/true"),
            shim_socket: sock_path.clone(),
            runtime_dir: std::env::temp_dir(),
            cwd: std::env::temp_dir(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::FullAccess,
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
            acp_agent_id: None,
            acp_spec: None,
        };
        let actor = tokio::spawn(run_agy_actor(cfg, shim, false, cmd_rx, evt_tx));
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "SLOW_STRICT".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec![],
            })
            .await
            .expect("t1");
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("thinking")
                .expect("ev");
            if matches!(ev, DriverEvent::ThinkingText { .. }) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_ag".into(),
                prompt: "AGENT_AFTER".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["m_ag".into()],
            })
            .await
            .expect("agent q");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_hu".into(),
                prompt: "HUMAN_STEER_FB".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: vec!["m_hu".into()],
            })
            .await
            .expect("human steer");

        // Wait until rejected concurrent steer transparently falls back to Queued.
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("fallback queued")
                .expect("ev");
            if matches!(
                ev,
                DriverEvent::PromptDeliveryUpdated {
                    ref message_ids,
                    status: AgentMessageDeliveryStatus::Queued,
                    ..
                } if message_ids == &vec!["m_hu".to_string()]
            ) {
                break;
            }
        }
        release_tx.send(()).await.expect("release slow");

        let mut saw_ag_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("drain idle")
                .expect("ev");
            match ev {
                DriverEvent::StatusChanged(AgentStatus::Error { .. }) => {
                    panic!(
                        "concurrent-prompt rejection must fall back silently without Error status"
                    );
                }
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status: AgentMessageDeliveryStatus::Settled,
                    ..
                } if message_ids == vec!["m_ag".to_string()] => {
                    saw_ag_settled = true;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if saw_ag_settled => break,
                _ => {}
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            *completed_prompts.lock().await,
            vec![
                "SLOW_STRICT".to_string(),
                "HUMAN_STEER_FB".to_string(),
                "AGENT_AFTER".to_string(),
            ]
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = actor.await;
        server.abort();
        let _ = std::fs::remove_file(&sock_path);
    }
}
