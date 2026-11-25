//! Pure-Rust generic Agent Client Protocol (ACP) JSON-RPC 2.0 client driver.
//!
//! Runs ANY stdio ACP-compatible coding agent CLI inside a detached `orbit shim` UDS
//! (`shim_id: format!("acp_{}", cfg.agent_id)`), supporting hot-upgrade shim reattach with
//! monotonic cursor tracking, `initialize`, `session/new` / `session/load` (with HTTP→stdio MCP
//! fallback and conversation handoff recovery), `session/set_mode`, `session/set_model`,
//! `session/set_config_option`, `session/prompt`, `session/cancel`, streaming `session/update`,
//! and `session/request_permission`.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use orbit_protocol::{
    AcpAgentSource, AcpAgentSpec, ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentStatus,
    QuestionItem, ReasoningEffort, SessionMode, ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after, kill_existing_shim,
    spawn_detached_shim,
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use crate::acp_registry::{get_acp_agent_spec, is_blocked_acp_env_key, resolve_acp_agent_command};
use crate::attachments::{AcpPromptMediaCaps, build_acp_prompt_blocks_async, is_path_within_dir};
use crate::driver::{
    DriverCommand, DriverEvent, PromptOrigin, ProviderSessionHandle, QueuedDriverPrompt, RpcId,
    SessionLaunchConfig, emit_dropped_queued_prompts, emit_prompt_delivery,
};

const ACP_BOOT_TIMEOUT: Duration = Duration::from_secs(30);

fn resolve_home_dir(cfg: &SessionLaunchConfig) -> PathBuf {
    if let Some(dir) = cfg
        .extra_env
        .get("ORBIT_HOME")
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("ORBIT_HOME")
        && !dir.trim().is_empty()
    {
        return PathBuf::from(dir.trim());
    }
    if let Some(home) = std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()))
    {
        return PathBuf::from(home).join(".orbit");
    }
    PathBuf::from(".orbit")
}

fn resolve_launch_spec(cfg: &SessionLaunchConfig) -> Result<AcpAgentSpec, String> {
    if let Some(ref spec) = cfg.acp_spec {
        return Ok(spec.clone());
    }
    if let Some(ref id) = cfg.acp_agent_id {
        let home = resolve_home_dir(cfg);
        return get_acp_agent_spec(&home, id)
            .ok_or_else(|| format!("ACP agent `{id}` not found in Orbit registry on this host"));
    }
    // Direct executable fallback when invoked programmatically without an explicit registry entry
    let fallback_cmd = cfg.executable.to_string_lossy().to_string();
    if fallback_cmd.trim().is_empty() {
        return Err("ACP session launch requires `acp_spec` or `acp_agent_id`".to_string());
    }
    Ok(AcpAgentSpec {
        id: "acp".to_string(),
        name: "ACP Agent".to_string(),
        description: String::new(),
        command: fallback_cmd,
        args: Vec::new(),
        env: HashMap::new(),
        install_hint: None,
        homepage: None,
        version: None,
        source: AcpAgentSource::Custom,
        enabled: true,
    })
}

fn build_acp_spawn_env(cfg: &SessionLaunchConfig, spec: &AcpAgentSpec) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("NO_COLOR".into(), "1".into());
    crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);
    // Apply custom ACP spec env BEFORE Orbit's own `extra_env`, never honoring untrusted
    // `RemoteRegistry` env entries or blocked dynamic-linker / ORBIT_* overrides.
    if spec.source != AcpAgentSource::RemoteRegistry {
        for (k, v) in &spec.env {
            if !is_blocked_acp_env_key(k) {
                env.insert(k.clone(), v.clone());
            }
        }
    }
    // Orbit's own extra_env (e.g. ORBIT_HOME, ORBIT_SERVER, ORBIT_AGENT_ID) always wins.
    for (k, v) in &cfg.extra_env {
        env.insert(k.clone(), v.clone());
    }
    env
}

async fn spawn_fresh_acp_shim(
    cfg: &SessionLaunchConfig,
    spec: &AcpAgentSpec,
) -> Result<orbit_shim::ShimClient, String> {
    let resolved_bin = {
        let cmd = spec.command.clone();
        let fallback_exe = cfg.executable.clone();
        tokio::task::spawn_blocking(move || {
            resolve_acp_agent_command(&cmd)
                .or_else(|| crate::is_executable_file(&fallback_exe).then_some(fallback_exe))
        })
        .await
        .ok()
        .flatten()
    }
    .ok_or_else(|| {
        format!(
            "ACP agent `{}` executable (`{}`) not found on host PATH",
            spec.id, spec.command
        )
    })?;

    // Terminate any stale shim still bound to this socket and reset the monotonic cursor
    // before spawning a fresh detached shim.
    kill_existing_shim(&cfg.shim_socket).await;
    cfg.shim_seq_tracker.store(0, Ordering::Relaxed);

    let env = build_acp_spawn_env(cfg, spec);
    let shim_spec = ShimSpawnSpec {
        sandbox: cfg.sandbox.clone(),
        egress_proxy_url: None,
        shim_id: format!("acp_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: resolved_bin.to_string_lossy().to_string(),
        args: spec.args.clone(),
        cwd: cfg.cwd.clone(),
        env,
        cleanup_paths: Vec::new(),
    };

    spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &shim_spec)
        .await
        .map_err(|e| format!("failed to spawn ACP shim for `{}`: {e}", spec.id))
}

/// Spawn (or reattach to) a generic ACP coding agent session hosted inside a detached `orbit shim`.
pub async fn spawn_acp_session(cfg: SessionLaunchConfig) -> Result<ProviderSessionHandle, String> {
    if cfg.native_fork_only {
        return Err("generic ACP eager native fork is deferred to handoff on first prompt".into());
    }
    let spec = resolve_launch_spec(&cfg)?;
    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let (shim, is_reattached) = if let Some(existing) = reattached {
        (existing, true)
    } else {
        let fresh = spawn_fresh_acp_shim(&cfg, &spec).await?;
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_acp_actor(
        cfg,
        spec,
        shim,
        is_reattached,
        cmd_rx,
        evt_tx,
    ));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

struct PendingAcpPerm {
    rpc_id: Value,
    perm_options: crate::AcpPermissionOptions,
    is_question: bool,
    raw_questions: Vec<QuestionItem>,
}

struct PendingAcpPlanModeSwitch {
    target_mode: SessionMode,
    previous_mode: SessionMode,
    deferred_perm: Option<(PendingAcpPerm, Value)>,
}

#[derive(Default)]
struct AcpStreamSegmentState {
    turn_tag: String,
    step_seq: u64,
    active_msg_id: Option<String>,
    active_thought_id: Option<String>,
    msg_accum: HashMap<String, String>,
    thought_accum: HashMap<String, String>,
}

#[derive(Debug, Clone)]
struct AcpConfigOptionMeta {
    id: String,
    category: Option<String>,
    option_values: Vec<String>,
}

struct OpenedAcpSession {
    session_id: String,
    current_model: Option<String>,
    available_modes: Vec<String>,
    config_options: Vec<AcpConfigOptionMeta>,
}

fn build_acp_mcp_servers_http(cfg: &SessionLaunchConfig) -> Value {
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

fn build_acp_mcp_servers_stdio(cfg: &SessionLaunchConfig) -> Value {
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

fn prepend_handoff_context(ctx: Option<String>, prompt: &str) -> String {
    let Some(ctx) = ctx.filter(|c| !c.trim().is_empty()) else {
        return prompt.to_string();
    };
    if prompt.contains("[Prior Conversation Context in this Workspace]") {
        return prompt.to_string();
    }
    format!("{ctx}\n\n{prompt}")
}

fn resolve_wire_mode_id(mode: &SessionMode, available_modes: &[String]) -> Option<String> {
    if let SessionMode::Custom(raw) = mode {
        let trimmed = raw.trim();
        return (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    let candidates: &[&str] = match mode {
        // Auto (Guardian) maps to approval-gating mode so Orbit's daemon receives permission requests.
        SessionMode::Default | SessionMode::Auto => &["default", "ask", "code", "normal"],
        SessionMode::Plan => &["plan", "architect", "read-only", "readonly"],
        SessionMode::AcceptEdits => &[
            "accept-edits",
            "accept_edits",
            "acceptEdits",
            "auto-edit",
            "auto_edit",
            "edit",
        ],
        SessionMode::FullAccess => &[
            "bypass-permissions",
            "bypassPermissions",
            "full-access",
            "full_access",
            "yolo",
            "auto",
        ],
        SessionMode::Custom(_) => &[],
    };

    if !available_modes.is_empty() {
        for cand in candidates {
            if let Some(matched) = available_modes
                .iter()
                .find(|m| m.eq_ignore_ascii_case(cand))
            {
                return Some(matched.clone());
            }
        }
    }
    candidates.first().map(|s| (*s).to_string())
}

fn parse_config_options(val: &Value) -> Vec<AcpConfigOptionMeta> {
    val.get("configOptions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let id = item
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|s| !s.is_empty())?
                        .to_string();
                    let category = item
                        .get("category")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned);
                    let option_values = item
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|opts| {
                            opts.iter()
                                .filter_map(|o| {
                                    o.get("value")
                                        .or_else(|| o.get("id"))
                                        .and_then(Value::as_str)
                                        .map(ToOwned::to_owned)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(AcpConfigOptionMeta {
                        id,
                        category,
                        option_values,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn effort_wire_slug(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max | ReasoningEffort::Ultra => "max",
    }
}

fn find_effort_config_option(
    config_options: &[AcpConfigOptionMeta],
    effort: ReasoningEffort,
) -> Option<(String, String)> {
    let target = config_options.iter().find(|opt| {
        opt.category
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case("thought_level"))
            || matches!(
                opt.id.to_ascii_lowercase().as_str(),
                "reasoning_effort" | "thought_level" | "thinking" | "effort"
            )
    })?;
    let desired = effort_wire_slug(effort);
    if target.option_values.is_empty() {
        return Some((target.id.clone(), desired.to_string()));
    }
    let matched = target
        .option_values
        .iter()
        .find(|v| v.eq_ignore_ascii_case(desired))
        .or_else(|| {
            if matches!(effort, ReasoningEffort::Xhigh | ReasoningEffort::Ultra) {
                target
                    .option_values
                    .iter()
                    .find(|v| v.eq_ignore_ascii_case("high") || v.eq_ignore_ascii_case("max"))
            } else {
                None
            }
        })
        .cloned()?;
    Some((target.id.clone(), matched))
}

#[derive(Debug, Clone)]
enum BootRpcError {
    Rpc(String),
    ShimExited(String),
    Timeout(String),
}

impl BootRpcError {
    fn is_shim_dead(&self) -> bool {
        matches!(self, Self::ShimExited(_))
    }

    fn message(&self) -> &str {
        match self {
            Self::Rpc(m) | Self::ShimExited(m) | Self::Timeout(m) => m.as_str(),
        }
    }
}

impl std::fmt::Display for BootRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

fn is_auth_or_http_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("authentication")
        || lower.contains("unauthenticated")
        || lower.contains("invalid api key")
        || lower.contains("invalid token")
        || lower.contains("expired token")
        || lower.contains("cookie")
        || lower.contains("http 401")
        || lower.contains("http 403")
        || lower.contains("status 401")
        || lower.contains("status 403")
        || lower.contains("rate limit")
        || lower.contains("quota")
}

fn is_missing_session_error(msg: &str) -> bool {
    if is_auth_or_http_error(msg) {
        return false;
    }
    let lower = msg.to_ascii_lowercase();
    lower.contains("session not found")
        || lower.contains("unknown session")
        || lower.contains("session does not exist")
        || lower.contains("no such session")
        || lower.contains("invalid session id")
        || lower.contains("session_not_found")
        || lower.contains("trajectory not found")
}

fn should_retry_mcp_fallback(err: &BootRpcError) -> bool {
    match err {
        BootRpcError::Rpc(msg) => !is_missing_session_error(msg) && !is_auth_or_http_error(msg),
        BootRpcError::ShimExited(_) | BootRpcError::Timeout(_) => false,
    }
}

async fn reply_method_not_found(shim: &orbit_shim::ShimClient, id_val: &Value, method: &str) {
    let resp = json!({
        "jsonrpc": "2.0",
        "id": id_val,
        "error": {
            "code": -32601,
            "message": format!("Method not found: {method}")
        }
    })
    .to_string();
    let _ = shim.write_stdin_line(resp).await;
}

async fn finalize_stream_segments(
    seg_state: &mut AcpStreamSegmentState,
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

async fn initialize_acp_connection(
    shim: &mut orbit_shim::ShimClient,
    next_rpc_id: &mut i64,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    shim_seq_tracker: &std::sync::atomic::AtomicU64,
) -> Result<Value, BootRpcError> {
    let init_rx = send_acp_req(
        shim,
        next_rpc_id,
        pending_rpcs,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": crate::acp_callbacks::acp_client_capabilities(),
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
    .await
    .map_err(|e| BootRpcError::ShimExited(format!("failed to send initialize: {e}")))?;

    wait_rpc_during_boot(shim, pending_rpcs, init_rx, shim_seq_tracker).await
}

#[allow(clippy::too_many_arguments)]
async fn open_acp_session(
    cfg: &SessionLaunchConfig,
    shim: &mut orbit_shim::ShimClient,
    next_rpc_id: &mut i64,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    resume_id: Option<&str>,
    requested_model: Option<&str>,
    requested_effort: Option<ReasoningEffort>,
    mode: &SessionMode,
) -> Result<OpenedAcpSession, BootRpcError> {
    let is_resume = resume_id.is_some();
    let method = if is_resume {
        "session/load"
    } else {
        "session/new"
    };
    let make_params = |mcp_val: Value| {
        if let Some(sid) = resume_id {
            json!({
                "sessionId": sid,
                "cwd": cfg.cwd.to_string_lossy(),
                "mcpServers": mcp_val
            })
        } else {
            json!({
                "cwd": cfg.cwd.to_string_lossy(),
                "mcpServers": mcp_val
            })
        }
    };

    let sess_rx = send_acp_req(
        shim,
        next_rpc_id,
        pending_rpcs,
        method,
        make_params(build_acp_mcp_servers_http(cfg)),
    )
    .await
    .map_err(|e| BootRpcError::ShimExited(format!("failed to send {method}: {e}")))?;

    let sess_val = match wait_rpc_during_boot(shim, pending_rpcs, sess_rx, &cfg.shim_seq_tracker)
        .await
    {
        Ok(v) => v,
        Err(first_err) if cfg.mcp_spec.is_some() && should_retry_mcp_fallback(&first_err) => {
            // Fallback 1: stdio `orbit mcp` descriptor
            let fb_rx = send_acp_req(
                shim,
                next_rpc_id,
                pending_rpcs,
                method,
                make_params(build_acp_mcp_servers_stdio(cfg)),
            )
            .await
            .map_err(|e| {
                BootRpcError::ShimExited(format!("failed to send stdio fallback {method}: {e}"))
            })?;
            match wait_rpc_during_boot(shim, pending_rpcs, fb_rx, &cfg.shim_seq_tracker).await {
                Ok(v) => v,
                Err(stdio_err) if should_retry_mcp_fallback(&stdio_err) => {
                    // Fallback 2: empty mcpServers for minimal ACP servers without MCP support
                    let empty_rx = send_acp_req(
                        shim,
                        next_rpc_id,
                        pending_rpcs,
                        method,
                        make_params(json!([])),
                    )
                    .await
                    .map_err(|e| {
                        BootRpcError::ShimExited(format!(
                            "failed to send empty-mcp fallback {method}: {e}"
                        ))
                    })?;
                    wait_rpc_during_boot(shim, pending_rpcs, empty_rx, &cfg.shim_seq_tracker)
                        .await
                        .map_err(|empty_err| {
                            let combined = format!(
                                "{first_err} (stdio fallback: {stdio_err}; empty-mcp fallback: {empty_err})"
                            );
                            if empty_err.is_shim_dead() {
                                BootRpcError::ShimExited(combined)
                            } else {
                                BootRpcError::Rpc(combined)
                            }
                        })?
                }
                Err(stdio_err) => return Err(stdio_err),
            }
        }
        Err(e) => return Err(e),
    };

    // Drain replay notifications on `session/load` until 50ms of quiet (max 3s).
    // Reply `-32601` to any unexpected server->client JSON-RPC request so the agent never deadlocks.
    if is_resume {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(50), shim.rx.recv()).await {
                Ok(Some(frame)) => {
                    if let Some(seq) = frame.seq() {
                        cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                    }
                    if let ShimFrame::StdoutLine { line, .. } = frame
                        && let Ok(val) = serde_json::from_str::<Value>(&line)
                        && let Some(method_name) = val.get("method").and_then(Value::as_str)
                        && let Some(id_val) = val.get("id").filter(|v| !v.is_null())
                    {
                        reply_method_not_found(shim, id_val, method_name).await;
                    }
                }
                _ => break,
            }
        }
    }

    let session_id = sess_val
        .get("sessionId")
        .and_then(Value::as_str)
        .or(resume_id)
        .unwrap_or("acp_session")
        .to_string();

    let available_modes: Vec<String> = sess_val
        .pointer("/modes/availableModes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(Value::as_str).map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let config_options = parse_config_options(&sess_val);

    let mut current_model = sess_val
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| requested_model.map(ToOwned::to_owned));

    // Apply requested model if supplied
    if let Some(req_model) = requested_model.map(str::trim).filter(|s| !s.is_empty())
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_model",
            json!({
                "sessionId": session_id,
                "modelId": req_model
            }),
        )
        .await
        && let Ok(v) = wait_rpc_during_boot(shim, pending_rpcs, rx, &cfg.shim_seq_tracker).await
    {
        if let Some(m) = v
            .pointer("/models/currentModelId")
            .or_else(|| v.get("modelId"))
            .and_then(Value::as_str)
        {
            current_model = Some(m.to_string());
        } else {
            current_model = Some(req_model.to_string());
        }
    }

    // Apply requested effort via `session/set_config_option` if advertised
    if let Some(eff) = requested_effort
        && let Some((opt_id, opt_val)) = find_effort_config_option(&config_options, eff)
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_config_option",
            json!({
                "sessionId": session_id,
                "configId": opt_id,
                "value": opt_val
            }),
        )
        .await
    {
        let _ = wait_rpc_during_boot(shim, pending_rpcs, rx, &cfg.shim_seq_tracker).await;
    }

    // Apply initial mode when non-default
    if !matches!(mode, SessionMode::Default)
        && let Some(wire_mode) = resolve_wire_mode_id(mode, &available_modes)
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": wire_mode
            }),
        )
        .await
    {
        let _ = wait_rpc_during_boot(shim, pending_rpcs, rx, &cfg.shim_seq_tracker).await;
    }

    Ok(OpenedAcpSession {
        session_id,
        current_model,
        available_modes,
        config_options,
    })
}

async fn run_acp_actor(
    cfg: SessionLaunchConfig,
    spec: AcpAgentSpec,
    mut shim: orbit_shim::ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_rpc_id: i64 = if is_reattached { 100_000 } else { 1 };
    let mut pending_rpcs: HashMap<String, oneshot::Sender<Result<Value, Value>>> = HashMap::new();
    let mut inject_handoff_on_next_prompt = false;
    let mut prompt_media_caps = AcpPromptMediaCaps::default();
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();
    let mut pending_system_prompt = cfg
        .system_prompt_append
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let (mut session_id, mut current_model, mut available_modes, mut config_options) =
        if is_reattached && let Some(ref sid) = cfg.resume_session_id {
            (sid.clone(), cfg.model.clone(), Vec::new(), Vec::new())
        } else {
            // 1. ACP `initialize` handshake
            let init_res = match initialize_acp_connection(
                &mut shim,
                &mut next_rpc_id,
                &mut pending_rpcs,
                &cfg.shim_seq_tracker,
            )
            .await
            {
                Ok(v) => v,
                Err(err) => {
                    warn!(agent_id = %cfg.agent_id, acp_id = %spec.id, error = %err, "ACP initialize failed");
                    if !is_reattached {
                        let _ = shim.kill().await;
                    }
                    let _ = evt_tx
                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                            message: format!("{} ACP initialize failed: {err}", spec.name),
                            recoverable: false,
                        }))
                        .await;
                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                    return;
                }
            };

            // Per ACP specification, `agentCapabilities.loadSession` defaults to `false` when omitted.
            let supports_load = init_res
                .pointer("/agentCapabilities/loadSession")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            prompt_media_caps = AcpPromptMediaCaps::from_acp_initialize(&init_res);
            let resume_target = if supports_load {
                cfg.resume_session_id.as_deref()
            } else {
                None
            };
            if cfg.resume_session_id.is_some() && !supports_load {
                inject_handoff_on_next_prompt = true;
            }

            // 2. Open session (`session/load` with transparent `session/new` + handoff fallback)
            let opened = match open_acp_session(
                &cfg,
                &mut shim,
                &mut next_rpc_id,
                &mut pending_rpcs,
                resume_target,
                cfg.model.as_deref(),
                cfg.effort,
                &cfg.mode,
            )
            .await
            {
                Ok(s) => s,
                Err(first_err) if resume_target.is_some() => {
                    warn!(
                        agent_id = %cfg.agent_id,
                        acp_id = %spec.id,
                        error = %first_err,
                        "ACP session/load failed; transparently starting fresh session with handoff context"
                    );
                    // If the shim died during `session/load`, respawn cleanly and re-initialize
                    // before opening a fresh `session/new` session.
                    if first_err.is_shim_dead() {
                        if !is_reattached {
                            let _ = shim.kill().await;
                        }
                        match spawn_fresh_acp_shim(&cfg, &spec).await {
                            Ok(fresh_shim) => {
                                shim = fresh_shim;
                                next_rpc_id = 1;
                                pending_rpcs.clear();
                                if let Err(reinit_err) = initialize_acp_connection(
                                    &mut shim,
                                    &mut next_rpc_id,
                                    &mut pending_rpcs,
                                    &cfg.shim_seq_tracker,
                                )
                                .await
                                {
                                    let _ = shim.kill().await;
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                            message: format!(
                                                "{} session open failed: {first_err} (respawn initialize failed: {reinit_err})",
                                                spec.name
                                            ),
                                            recoverable: false,
                                        }))
                                        .await;
                                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                    return;
                                }
                            }
                            Err(spawn_err) => {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                        message: format!(
                                            "{} session open failed: {first_err} (respawn failed: {spawn_err})",
                                            spec.name
                                        ),
                                        recoverable: false,
                                    }))
                                    .await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                return;
                            }
                        }
                    }
                    match open_acp_session(
                        &cfg,
                        &mut shim,
                        &mut next_rpc_id,
                        &mut pending_rpcs,
                        None,
                        cfg.model.as_deref(),
                        cfg.effort,
                        &cfg.mode,
                    )
                    .await
                    {
                        Ok(fresh) => {
                            inject_handoff_on_next_prompt = true;
                            let _ = evt_tx
                                .send(DriverEvent::Notice {
                                    level: "info".into(),
                                    message: format!(
                                        "{} could not resume its prior session; started a fresh session with conversation context restored.",
                                        spec.name
                                    ),
                                })
                                .await;
                            fresh
                        }
                        Err(e) => {
                            if !is_reattached {
                                let _ = shim.kill().await;
                            }
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                    message: format!(
                                        "{} session open failed: {first_err} (fresh fallback: {e})",
                                        spec.name
                                    ),
                                    recoverable: false,
                                }))
                                .await;
                            let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                            return;
                        }
                    }
                }
                Err(e) => {
                    if !is_reattached {
                        let _ = shim.kill().await;
                    }
                    let _ = evt_tx
                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                            message: format!("{} session open failed: {e}", spec.name),
                            recoverable: false,
                        }))
                        .await;
                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                    return;
                }
            };

            (
                opened.session_id,
                opened.current_model,
                opened.available_modes,
                opened.config_options,
            )
        };

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: session_id.clone(),
            model: current_model.clone(),
        })
        .await;
    if !is_reattached || cfg.persisted_initializing {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }

    let mut mode = cfg.mode.clone();
    let mut active_effort: Option<ReasoningEffort> = cfg.effort;
    let mut active_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut reattached_turn_in_flight = is_reattached && active_turn_id.is_some();
    let mut active_prompt_rpc_key: Option<String> = None;
    let mut non_prompt_rpcs: HashSet<String> = HashSet::new();
    let mut seg_state = if let Some(ref tid) = active_turn_id {
        AcpStreamSegmentState {
            turn_tag: tid.clone(),
            step_seq: cfg.persisted_turn_step_seq,
            ..AcpStreamSegmentState::default()
        }
    } else {
        AcpStreamSegmentState::default()
    };
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_perms: HashMap<String, PendingAcpPerm> = HashMap::new();
    let mut pending_perm_order: VecDeque<String> = VecDeque::new();
    let mut pending_plan_mode_switches: HashMap<String, PendingAcpPlanModeSwitch> = HashMap::new();
    let mut pending_human_steers: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_prompts: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<QueuedDriverPrompt> = None;
    let mut active_turn_message_ids: Vec<String> = Vec::new();
    let mut cancelling_active_turn = false;
    let mut cancel_deadline: Option<tokio::time::Instant> = None;
    let mut last_dispatched_prompt: Option<QueuedDriverPrompt> = None;
    let mut recovered_missing_session = false;
    let (mut acp_cb_host, mut acp_cb_reply_rx) =
        crate::acp_callbacks::AcpCallbackHost::new(std::path::PathBuf::from(&cfg.cwd));
    acp_cb_host.policy = cfg.sandbox.clone();
    acp_cb_host.proxy = cfg.egress_proxy_url.clone();

    loop {
        tokio::select! {
            Some(reply_line) = acp_cb_reply_rx.recv() => {
                if shim.write_stdin_line(reply_line).await.is_err() {
                    finalize_stream_segments(&mut seg_state, &evt_tx).await;
                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                    break;
                }
            }
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        prompt,
                        behavior,
                        origin,
                        message_ids,
                    } => {
                        let has_active_turn =
                            active_prompt_rpc_key.is_some() || active_turn_id.is_some();
                        if has_active_turn || !pending_perms.is_empty() {
                            let blocking_tid = active_turn_id.clone();
                            if matches!(behavior, ActiveTurnBehavior::Interrupt) {
                                pending_perm_order.clear();
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                for (_, p) in pending_perms.drain() {
                                    let resp = json!({
                                        "jsonrpc": "2.0",
                                        "id": p.rpc_id,
                                        "result": {
                                            "outcome": { "outcome": "cancelled" }
                                        }
                                    })
                                    .to_string();
                                    let _ = shim.write_stdin_line(resp).await;
                                }
                                if !has_active_turn {
                                    // Orphaned pending permission with no active turn: fall
                                    // through to dispatch the replacement turn directly.
                                } else {
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
                                        if let Some(ref tid) = active_turn_id {
                                            let active_ids =
                                                std::mem::take(&mut active_turn_message_ids);
                                            emit_prompt_delivery(
                                                &evt_tx,
                                                tid,
                                                &active_ids,
                                                AgentMessageDeliveryStatus::Dropped,
                                                None,
                                            )
                                            .await;
                                        }
                                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
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
                                    continue;
                                }
                            } else {
                                // Generic ACP servers lack native non-destructive mid-turn steer,
                                // and `session/cancel` is a destructive force-cancellation that can
                                // kill running tools/subagents/background tasks. Degrade `Steer`
                                // safely to non-destructive queueing (human steers drain ahead of
                                // queued agent notifications once the active turn settles).
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
                                if origin == PromptOrigin::Human
                                    && matches!(behavior, ActiveTurnBehavior::Steer)
                                {
                                    pending_human_steers.push_back(queued);
                                } else {
                                    pending_prompts.push_back(queued);
                                }
                                continue;
                            }
                        }

                        cancelling_active_turn = false;
                        cancel_deadline = None;
                        let mut effective_prompt = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };
                        if let Some(sys) = pending_system_prompt.take() {
                            effective_prompt = format!("[System Instructions]\n{sys}\n\n{effective_prompt}");
                        }
                        active_turn_message_ids = message_ids.clone();
                        last_dispatched_prompt = Some(QueuedDriverPrompt {
                            turn_id: turn_id.clone(),
                            prompt: effective_prompt.clone(),
                            message_ids,
                        });
                        if dispatch_acp_prompt(
                            &shim,
                            &session_id,
                            cfg.attachments_dir.as_deref(),
                            prompt_media_caps,
                            &mut next_rpc_id,
                            &mut active_turn_id,
                            &mut active_prompt_rpc_key,
                            &mut seg_state,
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
                            emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                                .await;
                            emit_dropped_queued_prompts(
                                &evt_tx,
                                pending_interrupt_replacement.take(),
                                &mut pending_prompts,
                            )
                            .await;
                            finalize_stream_segments(&mut seg_state, &evt_tx).await;
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
                    DriverCommand::Interrupt => {
                        emit_dropped_queued_prompts(&evt_tx, None, &mut pending_human_steers)
                            .await;
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut pending_prompts,
                        )
                        .await;
                        pending_perm_order.clear();
                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
                        let had_active =
                            active_prompt_rpc_key.is_some() || active_turn_id.is_some();
                        cancelling_active_turn = had_active;
                        if had_active {
                            cancel_deadline =
                                Some(tokio::time::Instant::now() + Duration::from_secs(10));
                        }
                        for (_, p) in pending_perms.drain() {
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": { "outcome": "cancelled" }
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                        let cancel = json!({
                            "jsonrpc": "2.0",
                            "method": "session/cancel",
                            "params": { "sessionId": session_id }
                        })
                        .to_string();
                        if shim.write_stdin_line(cancel).await.is_err() {
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
                            finalize_stream_segments(&mut seg_state, &evt_tx).await;
                            let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                            break;
                        }
                    }
                    DriverCommand::InterruptAndDrainQueue => {
                        pending_perm_order.clear();
                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
                        for (_, p) in pending_perms.drain() {
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": { "outcome": "cancelled" }
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                        let had_active =
                            active_prompt_rpc_key.is_some() || active_turn_id.is_some();
                        if had_active {
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
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                break;
                            }
                        } else if let Some(next_q) = pending_interrupt_replacement
                            .take()
                            .or_else(|| pending_human_steers.pop_front())
                            .or_else(|| pending_prompts.pop_front())
                        {
                            let mut effective_prompt = if inject_handoff_on_next_prompt {
                                inject_handoff_on_next_prompt = false;
                                prepend_handoff_context(
                                    pending_handoff_context.take(),
                                    &next_q.prompt,
                                )
                            } else {
                                next_q.prompt
                            };
                            if let Some(sys) = pending_system_prompt.take() {
                                effective_prompt = format!(
                                    "[System Instructions]\n{sys}\n\n{effective_prompt}"
                                );
                            }
                            active_turn_message_ids = next_q.message_ids.clone();
                            last_dispatched_prompt = Some(QueuedDriverPrompt {
                                turn_id: next_q.turn_id.clone(),
                                prompt: effective_prompt.clone(),
                                message_ids: next_q.message_ids,
                            });
                            if dispatch_acp_prompt(
                                &shim,
                                &session_id,
                                cfg.attachments_dir.as_deref(),
                                prompt_media_caps,
                                &mut next_rpc_id,
                                &mut active_turn_id,
                                &mut active_prompt_rpc_key,
                                &mut seg_state,
                                &evt_tx,
                                next_q.turn_id.clone(),
                                effective_prompt,
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
                                    &mut pending_prompts,
                                )
                                .await;
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
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
                    DriverCommand::SetMode { mode: new_mode } => {
                        mode = new_mode.clone();
                        if let Some(wire_mode) = resolve_wire_mode_id(&mode, &available_modes) {
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            non_prompt_rpcs.insert(RpcId::Num(id).as_key());
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_mode",
                                "params": {
                                    "sessionId": session_id,
                                    "modeId": wire_mode
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(req).await;
                        }
                    }
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode: _,
                    } => {
                        if let Some(m) = model.filter(|s| !s.trim().is_empty()) {
                            current_model = Some(m.clone());
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            non_prompt_rpcs.insert(RpcId::Num(id).as_key());
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
                        if let Some(eff) = effort {
                            active_effort = Some(eff);
                            let (opt_id, opt_val) = find_effort_config_option(&config_options, eff)
                                .unwrap_or_else(|| {
                                    ("reasoning_effort".to_string(), effort_wire_slug(eff).to_string())
                                });
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            non_prompt_rpcs.insert(RpcId::Num(id).as_key());
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_config_option",
                                "params": {
                                    "sessionId": session_id,
                                    "configId": opt_id,
                                    "value": opt_val
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(req).await;
                        }
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(p) = pending_perms.remove(&request_id) {
                            pending_perm_order.retain(|id| id != &request_id);
                            let outcome = p.perm_options.outcome_for_decision(decision.as_str());
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": outcome
                                }
                            })
                            .to_string();
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                break;
                            }
                            emit_post_permission_status(
                                &pending_perms,
                                &mut pending_perm_order,
                                &active_turn_id,
                                &evt_tx,
                            )
                            .await;
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
                        if perm.is_some() {
                            pending_perm_order.retain(|id| id != &request_id);
                        }
                        // ACP permission responses do not carry native inline edited plan bodies.
                        // When the user edited the plan on `ApproveAndBuild`, cancel/decline the
                        // stale unedited plan gate so the daemon starts one clean build turn with
                        // the approved edited plan text after mode switch completes.
                        let has_edits = edited_plan_markdown
                            .as_deref()
                            .is_some_and(|s| !s.trim().is_empty());
                        let compute_outcome = |p: &PendingAcpPerm| -> Value {
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
                            if let Some(wire_mode) =
                                resolve_wire_mode_id(&target_mode, &available_modes)
                            {
                                let id = next_rpc_id;
                                next_rpc_id += 1;
                                let rpc_key = RpcId::Num(id).as_key();
                                let req = json!({
                                    "jsonrpc": "2.0",
                                    "id": id,
                                    "method": "session/set_mode",
                                    "params": {
                                        "sessionId": session_id,
                                        "modeId": wire_mode
                                    }
                                })
                                .to_string();
                                if shim.write_stdin_line(req).await.is_err() {
                                    finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                    break;
                                }
                                let deferred_perm = perm.map(|p| {
                                    let outcome = compute_outcome(&p);
                                    (p, outcome)
                                });
                                pending_plan_mode_switches.insert(
                                    rpc_key,
                                    PendingAcpPlanModeSwitch {
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
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                break;
                            }
                            emit_post_permission_status(
                                &pending_perms,
                                &mut pending_perm_order,
                                &active_turn_id,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    DriverCommand::AnswerQuestion { request_id, answers } => {
                        if let Some(p) = pending_perms.remove(&request_id)
                            && p.is_question
                        {
                            pending_perm_order.retain(|id| id != &request_id);
                            let mut answers_map = serde_json::Map::new();
                            for (idx, q) in p.raw_questions.iter().enumerate() {
                                let joined = answers
                                    .get(idx)
                                    .map(|v| v.join(", "))
                                    .unwrap_or_default();
                                answers_map.insert(q.question.clone(), Value::String(joined));
                            }
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": p.rpc_id,
                                "result": {
                                    "outcome": {
                                        "outcome": "selected",
                                        "optionId": p.perm_options.question_option_id(),
                                        "updatedInput": { "answers": answers_map }
                                    }
                                }
                            })
                            .to_string();
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                break;
                            }
                            emit_post_permission_status(
                                &pending_perms,
                                &mut pending_perm_order,
                                &active_turn_id,
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
                    finalize_stream_segments(&mut seg_state, &evt_tx).await;
                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                    break;
                };
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                        let method = val.get("method").and_then(Value::as_str);

                        if has_id && method.is_none() {
                            if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone()) {
                                let key = rid.as_key();
                                if let Some(pending_switch) = pending_plan_mode_switches.remove(&key)
                                {
                                    if let Some(err_obj) =
                                        val.get("error").filter(|v| !v.is_null())
                                    {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(Value::as_str)
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
                                            let reject_outcome = p.perm_options.deny_outcome();
                                            let resp = json!({
                                                "jsonrpc": "2.0",
                                                "id": p.rpc_id,
                                                "result": { "outcome": reject_outcome }
                                            })
                                            .to_string();
                                            if shim.write_stdin_line(resp).await.is_err() {
                                                finalize_stream_segments(&mut seg_state, &evt_tx)
                                                    .await;
                                                let _ = evt_tx
                                                    .send(DriverEvent::Exited { code: None })
                                                    .await;
                                                break;
                                            }
                                            emit_post_permission_status(
                                                &pending_perms,
                                                &mut pending_perm_order,
                                                &active_turn_id,
                                                &evt_tx,
                                            )
                                            .await;
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
                                            if shim.write_stdin_line(resp).await.is_err() {
                                                finalize_stream_segments(&mut seg_state, &evt_tx)
                                                    .await;
                                                let _ = evt_tx
                                                    .send(DriverEvent::Exited { code: None })
                                                    .await;
                                                break;
                                            }
                                            emit_post_permission_status(
                                                &pending_perms,
                                                &mut pending_perm_order,
                                                &active_turn_id,
                                                &evt_tx,
                                            )
                                            .await;
                                        }
                                    }
                                    continue;
                                }
                                if non_prompt_rpcs.remove(&key) {
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
                                    cancelling_active_turn = false;
                                    cancel_deadline = None;
                                    active_prompt_rpc_key = None;
                                    pending_perms.clear();
                                    pending_perm_order.clear();
                                    pending_plan_mode_switches.clear();
                                    finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                    let finished_turn_id = active_turn_id.take();

                                    // Transparent missing-session recovery if `session/prompt` reports evicted session
                                    if let Some(err_obj) = val.get("error").filter(|v| !v.is_null()) {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(Value::as_str)
                                            .unwrap_or("");
                                        if !recovered_missing_session
                                            && is_missing_session_error(err_msg)
                                            && let Some(failed_q) = last_dispatched_prompt.take()
                                        {
                                            recovered_missing_session = true;
                                            if let Ok(fresh) = open_acp_session(
                                                &cfg,
                                                &mut shim,
                                                &mut next_rpc_id,
                                                &mut pending_rpcs,
                                                None,
                                                current_model.as_deref(),
                                                active_effort,
                                                &mode,
                                            )
                                            .await
                                            {
                                                session_id = fresh.session_id;
                                                current_model = fresh.current_model;
                                                available_modes = fresh.available_modes;
                                                config_options = fresh.config_options;
                                                let _ = evt_tx
                                                    .send(DriverEvent::SessionInitialized {
                                                        persistence_ref: session_id.clone(),
                                                        model: current_model.clone(),
                                                    })
                                                    .await;
                                                let recovered_prompt = prepend_handoff_context(
                                                    pending_handoff_context.take(),
                                                    &failed_q.prompt,
                                                );
                                                last_dispatched_prompt = Some(QueuedDriverPrompt {
                                                    turn_id: failed_q.turn_id.clone(),
                                                    prompt: recovered_prompt.clone(),
                                                    message_ids: failed_q.message_ids,
                                                });
                                                if dispatch_acp_prompt(
                                                    &shim,
                                                    &session_id,
                                                    cfg.attachments_dir.as_deref(),
                                                    prompt_media_caps,
                                                    &mut next_rpc_id,
                                                    &mut active_turn_id,
                                                    &mut active_prompt_rpc_key,
                                                    &mut seg_state,
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
                                                    finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                                    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                                    break;
                                                }
                                                continue;
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

                                    if let Some(next_q) = pending_interrupt_replacement
                                        .take()
                                        .or_else(|| pending_human_steers.pop_front())
                                        .or_else(|| pending_prompts.pop_front())
                                    {
                                        active_turn_message_ids = next_q.message_ids.clone();
                                        last_dispatched_prompt = Some(next_q.clone());
                                        if dispatch_acp_prompt(
                                            &shim,
                                            &session_id,
                                            cfg.attachments_dir.as_deref(),
                                            prompt_media_caps,
                                            &mut next_rpc_id,
                                            &mut active_turn_id,
                                            &mut active_prompt_rpc_key,
                                            &mut seg_state,
                                            &evt_tx,
                                            next_q.turn_id.clone(),
                                            next_q.prompt,
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
                                            finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                            let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
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
                                    } else if let Some(err_obj) =
                                        val.get("error").filter(|v| !v.is_null())
                                    {
                                        let msg = err_obj
                                            .get("message")
                                            .and_then(Value::as_str)
                                            .unwrap_or("ACP turn failed")
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
                            if !cancelling_active_turn
                                && let Some(update) = val.pointer("/params/update")
                            {
                                handle_generic_session_update(
                                    update,
                                    &mut mode,
                                    &mut config_options,
                                    &mut seg_state,
                                    &mut tool_cache,
                                    &evt_tx,
                                )
                                .await;
                            }
                        } else if has_id && method == Some("session/request_permission") {
                            handle_generic_permission_request(
                                &val,
                                &mode,
                                &cfg.cwd,
                                cfg.attachments_dir.as_deref(),
                                &active_turn_id,
                                &shim,
                                &mut pending_perms,
                                &mut pending_perm_order,
                                &evt_tx,
                            )
                            .await;
                        } else if has_id
                            && let Some(unknown_method) = method
                            && let Some(id_val) = val.get("id").filter(|v| !v.is_null())
                        {
                            let params = val.get("params").unwrap_or(&Value::Null);
                            match acp_cb_host
                                .handle_request(id_val, unknown_method, params)
                                .await
                            {
                                crate::acp_callbacks::AcpCallbackOutcome::Reply(resp) => {
                                    if shim.write_stdin_line(resp).await.is_err() {
                                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
                                        let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
                                        break;
                                    }
                                }
                                crate::acp_callbacks::AcpCallbackOutcome::Deferred => {}
                                crate::acp_callbacks::AcpCallbackOutcome::Unsupported => {
                                    // Reply JSON-RPC -32601 to any unhandled server->client request so
                                    // ACP agents never deadlock.
                                    reply_method_not_found(&shim, id_val, unknown_method).await;
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
                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
                        let _ = evt_tx.send(DriverEvent::Exited { code }).await;
                        break;
                    }
                    _ => {}
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
                pending_perm_order.clear();
                finalize_stream_segments(&mut seg_state, &evt_tx).await;
                let _ = evt_tx
                    .send(DriverEvent::Notice {
                        level: "warning".into(),
                        message: "ACP cancel timed out waiting for turn settlement; resetting active turn state.".into(),
                    })
                    .await;
                if let Some(next_q) = pending_interrupt_replacement
                    .take()
                    .or_else(|| pending_human_steers.pop_front())
                    .or_else(|| pending_prompts.pop_front())
                {
                    active_turn_message_ids = next_q.message_ids.clone();
                    last_dispatched_prompt = Some(next_q.clone());
                    if dispatch_acp_prompt(
                        &shim,
                        &session_id,
                        cfg.attachments_dir.as_deref(),
                        prompt_media_caps,
                        &mut next_rpc_id,
                        &mut active_turn_id,
                        &mut active_prompt_rpc_key,
                        &mut seg_state,
                        &evt_tx,
                        next_q.turn_id.clone(),
                        next_q.prompt,
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
                        finalize_stream_segments(&mut seg_state, &evt_tx).await;
                        let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
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

async fn emit_post_permission_status(
    pending_perms: &HashMap<String, PendingAcpPerm>,
    pending_perm_order: &mut VecDeque<String>,
    active_turn_id: &Option<String>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    while let Some(front) = pending_perm_order.front() {
        if pending_perms.contains_key(front) {
            break;
        }
        pending_perm_order.pop_front();
    }
    if let Some(next_req_id) = pending_perm_order.front().cloned() {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(
                AgentStatus::AwaitingPermission {
                    turn_id: active_turn_id.clone().unwrap_or_default(),
                    request_id: next_req_id,
                },
            ))
            .await;
    } else if let Some(tid) = active_turn_id {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                turn_id: tid.clone(),
            }))
            .await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_acp_prompt(
    shim: &orbit_shim::ShimClient,
    session_id: &str,
    attachments_dir: Option<&Path>,
    prompt_media_caps: AcpPromptMediaCaps,
    next_rpc_id: &mut i64,
    active_turn_id: &mut Option<String>,
    active_prompt_rpc_key: &mut Option<String>,
    seg_state: &mut AcpStreamSegmentState,
    evt_tx: &mpsc::Sender<DriverEvent>,
    turn_id: String,
    prompt: String,
) -> Result<(), ()> {
    *active_turn_id = Some(turn_id.clone());
    *seg_state = AcpStreamSegmentState {
        turn_tag: turn_id.clone(),
        ..AcpStreamSegmentState::default()
    };

    let _ = evt_tx
        .send(DriverEvent::StatusChanged(AgentStatus::Running { turn_id }))
        .await;

    let id = *next_rpc_id;
    *next_rpc_id += 1;
    *active_prompt_rpc_key = Some(RpcId::Num(id).as_key());

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
) -> Result<Value, BootRpcError> {
    let deadline = tokio::time::Instant::now() + ACP_BOOT_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(BootRpcError::Timeout("ACP initialization timed out".into()));
        }
        tokio::select! {
            res = &mut target_rx => {
                return match res {
                    Ok(Ok(v)) => Ok(v),
                    Ok(Err(e)) => Err(BootRpcError::Rpc(e.to_string())),
                    Err(_) => Err(BootRpcError::ShimExited("rpc channel dropped".into())),
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
                                if let Ok(val) = serde_json::from_str::<Value>(&line) {
                                    if let Some(method_name) = val.get("method").and_then(Value::as_str) {
                                        if let Some(id_val) = val.get("id").filter(|v| !v.is_null()) {
                                            reply_method_not_found(shim, id_val, method_name).await;
                                        }
                                    } else if let Some(id_val) = val.get("id")
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
                            }
                            ShimFrame::Exited { code } => {
                                return Err(BootRpcError::ShimExited(format!(
                                    "ACP agent exited early with code {code:?}"
                                )));
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => return Err(BootRpcError::ShimExited("ACP shim closed".into())),
                    Err(_) => return Err(BootRpcError::Timeout("ACP initialization timed out".into())),
                }
            }
        }
    }
}

async fn handle_generic_session_update(
    update: &Value,
    mode: &mut SessionMode,
    config_options: &mut Vec<AcpConfigOptionMeta>,
    seg_state: &mut AcpStreamSegmentState,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
        return;
    };

    let turn_tag = if seg_state.turn_tag.is_empty() {
        "init".to_string()
    } else {
        seg_state.turn_tag.clone()
    };

    match kind {
        "user_message_chunk" => {}
        "agent_message_chunk" => {
            let chunk = update
                .pointer("/content/text")
                .and_then(Value::as_str)
                .unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            seg_state.active_thought_id = None;
            let mid = if let Some(explicit) = update
                .get("messageId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                format!("{turn_tag}_{explicit}")
            } else if let Some(ref cur) = seg_state.active_msg_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("acp_msg_{turn_tag}_{}", seg_state.step_seq);
                seg_state.active_msg_id = Some(id.clone());
                id
            };
            let entry = seg_state.msg_accum.entry(mid.clone()).or_default();
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
                .and_then(Value::as_str)
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
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                format!("{turn_tag}_th_{explicit}")
            } else if let Some(ref cur) = seg_state.active_thought_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("acp_th_{turn_tag}_{}", seg_state.step_seq);
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
            let Some(call_id) = update
                .get("toolCallId")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
            else {
                return;
            };
            let status = match update.get("status").and_then(Value::as_str) {
                Some("completed") => ToolCallStatus::Completed,
                Some("failed") | Some("error") => ToolCallStatus::Failed,
                Some("denied") | Some("cancelled") => ToolCallStatus::Denied,
                _ => ToolCallStatus::Running,
            };
            let detail = map_generic_tool_detail(update, tool_cache.get(&call_id));
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
            tool_cache.insert(call_id.clone(), detail.clone());
            let _ = evt_tx
                .send(DriverEvent::ToolCall {
                    call_id,
                    status,
                    detail,
                    background_task: None,
                })
                .await;
        }
        "plan" => {
            if let Some(entries) = update.get("entries").and_then(Value::as_array)
                && !entries.is_empty()
            {
                let lines: Vec<String> = entries
                    .iter()
                    .filter_map(|e| {
                        let content = e.get("content").and_then(Value::as_str)?.trim();
                        if content.is_empty() {
                            return None;
                        }
                        let st = e.get("status").and_then(Value::as_str).unwrap_or("pending");
                        let marker = match st {
                            "completed" => "[x]",
                            "in_progress" => "[~]",
                            _ => "[ ]",
                        };
                        Some(format!("{marker} {content}"))
                    })
                    .collect();
                if !lines.is_empty() {
                    let _ = evt_tx
                        .send(DriverEvent::Notice {
                            level: "info".into(),
                            message: format!("Plan:\n{}", lines.join("\n")),
                        })
                        .await;
                }
            }
        }
        "current_mode_update" => {
            if let Some(mode_id) = update
                .get("currentModeId")
                .or_else(|| update.get("modeId"))
                .and_then(Value::as_str)
            {
                let parsed = match mode_id.to_ascii_lowercase().as_str() {
                    "default" | "ask" => SessionMode::Default,
                    "plan" | "architect" => SessionMode::Plan,
                    "accept-edits" | "accept_edits" | "acceptedits" | "auto-edit" => {
                        SessionMode::AcceptEdits
                    }
                    "full-access" | "full_access" | "bypass-permissions" | "bypasspermissions" => {
                        SessionMode::FullAccess
                    }
                    _ => SessionMode::Custom(mode_id.to_string()),
                };
                if !matches!(*mode, SessionMode::Auto) {
                    *mode = parsed;
                }
            }
        }
        "config_option_update" => {
            let updated = parse_config_options(update);
            if !updated.is_empty() {
                *config_options = updated;
            }
        }
        _ => {}
    }
}

fn truncate_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push('…');
        out
    }
}

fn map_generic_tool_detail(update: &Value, prev: Option<&ToolCallDetail>) -> ToolCallDetail {
    let explicit_kind = update.get("kind").and_then(Value::as_str).unwrap_or("");
    let effective_kind = if !explicit_kind.is_empty() {
        explicit_kind
    } else {
        match prev {
            Some(ToolCallDetail::Shell { .. }) => "execute",
            Some(ToolCallDetail::FileEdit { .. }) => "edit",
            Some(ToolCallDetail::FileRead { .. }) => "read",
            Some(ToolCallDetail::Search { .. }) => "search",
            _ => "",
        }
    };
    let title = update
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let raw_in = update.get("rawInput");
    let raw_out = update.get("rawOutput");

    if crate::managed_dispatch::is_launch(title)
        || matches!(prev, Some(ToolCallDetail::Subagent { .. }))
    {
        return crate::managed_dispatch::acp_detail(update, prev);
    }

    let out_preview = raw_out.and_then(|o| {
        if let Some(s) = o.as_str() {
            Some(truncate_preview(s, 800))
        } else {
            o.get("output")
                .or_else(|| o.get("stdout"))
                .or_else(|| o.get("content"))
                .or_else(|| o.get("result"))
                .and_then(Value::as_str)
                .map(|s| truncate_preview(s, 800))
        }
    });

    if effective_kind == "execute"
        || raw_in
            .and_then(|i| i.get("command").or_else(|| i.get("CommandLine")))
            .is_some()
    {
        let (prev_cmd, prev_cwd, prev_exit, prev_out) = match prev {
            Some(ToolCallDetail::Shell {
                command,
                cwd,
                exit_code,
                output_preview,
            }) => (
                Some(command.clone()),
                cwd.clone(),
                *exit_code,
                output_preview.clone(),
            ),
            _ => (None, None, None, None),
        };
        let command = raw_in
            .and_then(|i| i.get("command").or_else(|| i.get("CommandLine")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(prev_cmd)
            .unwrap_or_else(|| title.to_string());
        let cwd = crate::extract_acp_shell_cwd(raw_in).or(prev_cwd);
        let exit_code = raw_out
            .and_then(|o| o.get("exitCode").or_else(|| o.get("exit_code")))
            .and_then(Value::as_i64)
            .map(|c| c as i32)
            .or(prev_exit);
        return ToolCallDetail::Shell {
            command,
            cwd,
            exit_code,
            output_preview: out_preview.or(prev_out),
        };
    }

    if effective_kind == "edit" || effective_kind == "write" {
        let (prev_path, prev_diff) = match prev {
            Some(ToolCallDetail::FileEdit { path, unified_diff }) => {
                (Some(path.clone()), unified_diff.clone())
            }
            _ => (None, None),
        };
        let path = raw_in
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("Path"))
                    .or_else(|| i.get("file_path"))
                    .or_else(|| i.get("TargetFile"))
            })
            .and_then(Value::as_str)
            .or_else(|| update.pointer("/locations/0/path").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .or(prev_path)
            .unwrap_or_else(|| title.to_string());

        let diff = update
            .get("content")
            .and_then(Value::as_array)
            .and_then(|arr| {
                arr.iter().find_map(|item| {
                    if item.get("type").and_then(Value::as_str) == Some("diff") {
                        let old_t = item.get("oldText").and_then(Value::as_str).unwrap_or("");
                        let new_t = item.get("newText").and_then(Value::as_str).unwrap_or("");
                        crate::build_compact_unified_diff(&path, old_t, new_t)
                    } else {
                        None
                    }
                })
            })
            .or(prev_diff);

        return ToolCallDetail::FileEdit {
            path,
            unified_diff: diff,
        };
    }

    if effective_kind == "read" {
        let (prev_path, prev_start, prev_end) = match prev {
            Some(ToolCallDetail::FileRead {
                path,
                start_line,
                end_line,
            }) => (Some(path.clone()), *start_line, *end_line),
            _ => (None, None, None),
        };
        let path = raw_in
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("Path"))
                    .or_else(|| i.get("file_path"))
            })
            .and_then(Value::as_str)
            .or_else(|| update.pointer("/locations/0/path").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .or(prev_path)
            .unwrap_or_else(|| title.to_string());
        let start_line = raw_in
            .and_then(|i| i.get("startLine").or_else(|| i.get("start_line")))
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .or(prev_start);
        let end_line = raw_in
            .and_then(|i| i.get("endLine").or_else(|| i.get("end_line")))
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .or(prev_end);
        return ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        };
    }

    if effective_kind == "search" {
        let (prev_query, prev_scope) = match prev {
            Some(ToolCallDetail::Search { query, scope }) => (Some(query.clone()), scope.clone()),
            _ => (None, None),
        };
        let query = raw_in
            .and_then(|i| i.get("query").or_else(|| i.get("pattern")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(prev_query)
            .unwrap_or_else(|| title.to_string());
        let scope = raw_in
            .and_then(|i| i.get("path").or_else(|| i.get("scope")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(prev_scope);
        return ToolCallDetail::Search { query, scope };
    }

    if let Some(existing) = prev {
        return existing.clone();
    }

    ToolCallDetail::Generic {
        tool_name: title.to_string(),
        arguments_json: raw_in.map(|v| v.to_string()).unwrap_or_default(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_generic_permission_request(
    val: &Value,
    mode: &SessionMode,
    workspace_cwd: &Path,
    attachments_dir: Option<&Path>,
    active_turn_id: &Option<String>,
    shim: &orbit_shim::ShimClient,
    pending_perms: &mut HashMap<String, PendingAcpPerm>,
    pending_perm_order: &mut VecDeque<String>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val["id"].clone();
    let params = &val["params"];
    let tool_call = &params["toolCall"];
    let options = params["options"].as_array().cloned().unwrap_or_default();

    let perm_options = crate::AcpPermissionOptions::parse(&options);

    let kind_str = tool_call.get("kind").and_then(Value::as_str).unwrap_or("");
    let is_attachment_read = kind_str == "read"
        && attachments_dir.is_some_and(|dir| {
            tool_call
                .get("rawInput")
                .and_then(|i| i.get("path").or_else(|| i.get("Path")))
                .and_then(Value::as_str)
                .or_else(|| {
                    tool_call
                        .pointer("/locations/0/path")
                        .and_then(Value::as_str)
                })
                .is_some_and(|p| is_path_within_dir(Path::new(p), dir))
        });

    let plan_approval = crate::plan_approval::extract_plan_approval_from_acp_tool_call(
        tool_call,
        mode,
        workspace_cwd,
        "acp_plan",
    );

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
                .and_then(Value::as_str)
                .unwrap_or("Approve tool execution")
                .to_string()
        });
    let authoritative = crate::acp_tool_call_has_authoritative_identity(tool_call);
    let detail = if plan_approval.is_none() && !authoritative {
        crate::acp_title_only_detail(&title, tool_call)
    } else {
        map_generic_tool_detail(tool_call, None)
    };
    let allowed_decisions = perm_options.allowed_decisions(plan_approval.is_some(), authoritative);

    pending_perms.insert(
        req_id.clone(),
        PendingAcpPerm {
            rpc_id,
            perm_options,
            is_question: false,
            raw_questions: Vec::new(),
        },
    );
    pending_perm_order.push_back(req_id.clone());

    let _ = evt_tx
        .send(DriverEvent::PermissionRequest {
            request_id: req_id.clone(),
            title,
            detail,
            allowed_decisions,
            plan_approval,
        })
        .await;
    if pending_perm_order.len() == 1 {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(
                AgentStatus::AwaitingPermission {
                    turn_id: active_turn_id.clone().unwrap_or_default(),
                    request_id: req_id,
                },
            ))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    #[cfg(unix)]
    #[tokio::test]
    async fn generic_acp_full_lifecycle_permission_cancel_and_shim_reattach() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("orbit_acp_drv_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mock_bin = dir.join("mock-acp-cli");
        let cancel_log = dir.join("cancel.log");
        let sock_path = dir.join("acp.sock");

        std::fs::write(
            &mock_bin,
            format!(
                r#"#!/bin/sh
CANCEL_LOG="{}"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"MockGeneric","version":"1.0.0"}},"agentCapabilities":{{"loadSession":true}}}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sess_generic_1","models":{{"currentModelId":"mock-model-1"}},"modes":{{"availableModes":[{{"id":"default"}},{{"id":"accept-edits"}}]}}}}}}'
      ;;
    *'"method":"session/prompt"'*'"run_perm"'*)
      # Extract numeric RPC id using sed
      RID=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"Thinking about tool..."}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"plan","entries":[{{"content":"Run check","status":"in_progress"}}]}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"tool_call","toolCallId":"tc_1","kind":"execute","title":"Run tests","status":"in_progress","rawInput":{{"command":"cargo test"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":"perm_rpc_1","method":"session/request_permission","params":{{"sessionId":"sess_generic_1","toolCall":{{"toolCallId":"tc_1","kind":"execute","title":"Run cargo test","rawInput":{{"command":"cargo test"}}}},"options":[{{"optionId":"opt_allow","kind":"allow_once"}},{{"optionId":"opt_always","kind":"allow_always"}},{{"optionId":"opt_deny","kind":"reject_once"}}]}}}}'
      # Wait for permission response
      IFS= read -r perm_reply
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"tc_1","kind":"execute","title":"Run tests","status":"completed","rawOutput":{{"exitCode":0,"output":"ok"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"All tests passed!"}}}}}}}}'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$RID"
      ;;
    *'"method":"session/prompt"'*'"long_turn"'*)
      RID=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Working..."}}}}}}}}'
      IFS= read -r next_msg
      printf '%s\n' "$next_msg" >> "$CANCEL_LOG"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"cancelled"}}}}\n' "$RID"
      ;;
    *'"method":"session/prompt"'*'"after_reattach"'*)
      RID=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_generic_1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Survived reattach!"}}}}}}}}'
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$RID"
      ;;
    *'"method":"session/cancel"'*)
      printf '%s\n' "$line" >> "$CANCEL_LOG"
      ;;
  esac
done
"#,
                cancel_log.display()
            ),
        )
        .expect("write mock script");
        let mut perms = std::fs::metadata(&mock_bin).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&mock_bin, perms).expect("chmod");

        // Launch an in-process `orbit_shim` hosting our mock ACP CLI
        let shim_sock_bg = sock_path.clone();
        let shim_spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "acp_test_agent".into(),
            mode: ShimMode::Stdio,
            program: mock_bin.to_string_lossy().to_string(),
            args: vec![],
            cwd: dir.clone(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
        };
        let shim_task = tokio::spawn(async move {
            let _ = orbit_shim::run_shim(&shim_sock_bg, shim_spec).await;
        });

        // Wait until shim UDS is ready
        let mut shim_client = None;
        for _ in 0..40 {
            if let Some(c) = connect_existing_shim_after(&sock_path, None).await {
                shim_client = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let shim_client = shim_client.expect("shim connected");

        let spec = AcpAgentSpec {
            id: "mock-generic".into(),
            name: "Mock Generic".into(),
            description: "Test ACP agent".into(),
            command: mock_bin.to_string_lossy().to_string(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };

        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_acp_test".into(),
            provider: orbit_protocol::ProviderKind::Acp,
            executable: mock_bin.clone(),
            orbit_exe: PathBuf::from("orbit"),
            shim_socket: sock_path.clone(),
            runtime_dir: dir.clone(),
            cwd: dir.clone(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
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
            shim_seq_tracker: Arc::clone(&seq_tracker),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: Some("mock-generic".into()),
            acp_spec: Some(spec.clone()),
        };

        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let actor1 = tokio::spawn(run_acp_actor(
            cfg.clone(),
            spec.clone(),
            shim_client,
            false,
            cmd_rx,
            evt_tx,
        ));

        // 1. Expect SessionInitialized + Idle
        let mut init_sid = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                match ev {
                    DriverEvent::SessionInitialized {
                        persistence_ref,
                        model,
                    } => {
                        init_sid = Some(persistence_ref);
                        assert_eq!(model.as_deref(), Some("mock-model-1"));
                    }
                    DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                    _ => {}
                }
            }
        }
        assert_eq!(init_sid.as_deref(), Some("sess_generic_1"));

        // 2. Send prompt triggering thinking, plan, tool call, permission request, and streaming text
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_1".into(),
                prompt: "run_perm".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send prompt");

        let mut saw_thinking = false;
        let mut saw_plan = false;
        let mut saw_tool_completed = false;
        let mut saw_perm_req = false;
        let mut final_text = String::new();
        let deadline2 = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline2 {
            if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                match ev {
                    DriverEvent::ThinkingText { text, .. } if text.contains("Thinking") => {
                        saw_thinking = true;
                    }
                    DriverEvent::Notice { message, .. } if message.contains("Plan:") => {
                        saw_plan = true;
                    }
                    DriverEvent::PermissionRequest { request_id, .. } => {
                        saw_perm_req = true;
                        cmd_tx
                            .send(DriverCommand::ResolvePermission {
                                request_id,
                                decision: "allow".into(),
                            })
                            .await
                            .expect("resolve perm");
                    }
                    DriverEvent::ToolCall {
                        status: ToolCallStatus::Completed,
                        detail: ToolCallDetail::Shell { command, .. },
                        ..
                    } if command == "cargo test" => {
                        saw_tool_completed = true;
                    }
                    DriverEvent::AssistantText {
                        text,
                        streaming: false,
                        ..
                    } => {
                        final_text = text;
                    }
                    DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                    _ => {}
                }
            }
        }
        assert!(saw_thinking, "expected thinking update");
        assert!(saw_plan, "expected plan update");
        assert!(saw_perm_req, "expected permission request");
        assert!(saw_tool_completed, "expected completed shell tool call");
        assert_eq!(final_text, "All tests passed!");

        // 3. Test session/cancel via DriverCommand::Interrupt
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_2".into(),
                prompt: "long_turn".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send long_turn");
        tokio::time::sleep(Duration::from_millis(120)).await;
        cmd_tx.send(DriverCommand::Interrupt).await.expect("cancel");

        let deadline3 = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline3 {
            if let Ok(Some(DriverEvent::StatusChanged(AgentStatus::Idle))) =
                tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                break;
            }
        }
        let logged_cancel = std::fs::read_to_string(&cancel_log).unwrap_or_default();
        assert!(
            logged_cancel.contains("session/cancel"),
            "expected session/cancel sent to agent, got: {logged_cancel}"
        );

        // 4. Simulate daemon hot-upgrade: drop actor1 WITHOUT killing shim, then reattach via `spawn_acp_session`!
        let cursor_before_reattach = seq_tracker.load(Ordering::Relaxed);
        assert!(cursor_before_reattach > 0);
        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), actor1).await;

        let reattach_tracker = Arc::new(AtomicU64::new(cursor_before_reattach));
        let mut reattach_cfg = cfg.clone();
        reattach_cfg.reattach_existing_shim = true;
        reattach_cfg.resume_session_id = Some("sess_generic_1".into());
        reattach_cfg.shim_after_seq = Some(cursor_before_reattach);
        reattach_cfg.shim_seq_tracker = Arc::clone(&reattach_tracker);

        let mut handle2 = spawn_acp_session(reattach_cfg)
            .await
            .expect("reattach existing ACP shim");

        // Send a prompt on the reattached session without re-initializing
        handle2
            .cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_3".into(),
                prompt: "after_reattach".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send after_reattach");

        let mut reattached_reply = String::new();
        let deadline4 = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline4 {
            if let Ok(Some(ev)) =
                tokio::time::timeout(Duration::from_secs(2), handle2.evt_rx.recv()).await
            {
                match ev {
                    DriverEvent::AssistantText {
                        text,
                        streaming: false,
                        ..
                    } => {
                        reattached_reply = text;
                    }
                    DriverEvent::StatusChanged(AgentStatus::Idle)
                        if !reattached_reply.is_empty() =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(reattached_reply, "Survived reattach!");
        assert!(reattach_tracker.load(Ordering::Relaxed) > cursor_before_reattach);

        // Shutdown cleanly
        let _ = handle2.cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tool_call_update_partial_patch_preserves_kind_and_fields() {
        // Shell initial call with command + cwd, followed by partial patch omitting `kind` and `cwd`
        let init_shell = json!({
            "kind": "execute",
            "title": "Run cargo test",
            "rawInput": { "command": "cargo test", "cwd": "/repo" }
        });
        let prev_shell = map_generic_tool_detail(&init_shell, None);
        let patch_shell = json!({
            "title": "Run cargo test",
            "status": "completed",
            "rawOutput": { "exitCode": 0, "stdout": "all green" }
        });
        let updated_shell = map_generic_tool_detail(&patch_shell, Some(&prev_shell));
        assert!(matches!(
            updated_shell,
            ToolCallDetail::Shell {
                ref command,
                ref cwd,
                exit_code: Some(0),
                output_preview: Some(ref preview),
            } if command == "cargo test" && cwd.as_deref() == Some("/repo") && preview == "all green"
        ));

        // Second partial patch with neither kind nor rawOutput preserves previous exit_code & preview
        let patch_shell_2 = json!({ "status": "completed" });
        let updated_shell_2 = map_generic_tool_detail(&patch_shell_2, Some(&updated_shell));
        assert_eq!(
            serde_json::to_value(&updated_shell_2).expect("json 2"),
            serde_json::to_value(&updated_shell).expect("json 1"),
        );

        // FileEdit initial call with diff, followed by status-only patch omitting `kind` and `content`
        let init_edit = json!({
            "kind": "edit",
            "title": "Edit lib.rs",
            "rawInput": { "path": "src/lib.rs" },
            "content": [{ "type": "diff", "oldText": "old\n", "newText": "new\n" }]
        });
        let prev_edit = map_generic_tool_detail(&init_edit, None);
        let patch_edit = json!({ "status": "completed" });
        let updated_edit = map_generic_tool_detail(&patch_edit, Some(&prev_edit));
        match updated_edit {
            ToolCallDetail::FileEdit { path, unified_diff } => {
                assert_eq!(path, "src/lib.rs");
                assert!(
                    unified_diff.as_deref().is_some_and(|d| d.contains("+new")),
                    "expected prior unified_diff to be preserved"
                );
            }
            other => panic!("expected FileEdit, got {other:?}"),
        }
    }

    #[test]
    fn spawn_env_precedence_and_error_heuristics() {
        let mut cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_env".into(),
            provider: orbit_protocol::ProviderKind::Acp,
            executable: PathBuf::from("/bin/echo"),
            orbit_exe: PathBuf::from("orbit"),
            shim_socket: PathBuf::from("/tmp/x.sock"),
            runtime_dir: PathBuf::from("/tmp"),
            cwd: PathBuf::from("/tmp"),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: None,
            system_prompt_append: None,
            extra_env: HashMap::from([
                ("ORBIT_HOME".into(), "/real/orbit".into()),
                ("SHARED_VAR".into(), "from_orbit".into()),
            ]),
            reattach_existing_shim: false,
            shim_after_seq: None,
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: Some("custom".into()),
            acp_spec: None,
        };

        let custom_spec = AcpAgentSpec {
            id: "custom".into(),
            name: "Custom".into(),
            description: String::new(),
            command: "/bin/echo".into(),
            args: vec![],
            env: HashMap::from([
                ("ORBIT_HOME".into(), "/evil/orbit".into()),
                ("LD_PRELOAD".into(), "/tmp/evil.so".into()),
                ("SHARED_VAR".into(), "from_spec".into()),
                ("CUSTOM_ONLY".into(), "ok".into()),
            ]),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        let env = build_acp_spawn_env(&cfg, &custom_spec);
        assert_eq!(
            env.get("ORBIT_HOME").map(String::as_str),
            Some("/real/orbit")
        );
        assert_eq!(
            env.get("SHARED_VAR").map(String::as_str),
            Some("from_orbit")
        );
        assert_eq!(env.get("CUSTOM_ONLY").map(String::as_str), Some("ok"));
        assert!(!env.contains_key("LD_PRELOAD"));

        // RemoteRegistry env must never be injected
        let mut remote_spec = custom_spec.clone();
        remote_spec.source = AcpAgentSource::RemoteRegistry;
        cfg.extra_env.clear();
        let remote_env = build_acp_spawn_env(&cfg, &remote_spec);
        assert!(!remote_env.contains_key("CUSTOM_ONLY"));

        // Missing-session heuristic excludes auth/HTTP errors
        assert!(is_missing_session_error("Session not found: sess_123"));
        assert!(is_missing_session_error("Unknown session id"));
        assert!(!is_missing_session_error(
            "HTTP 401 Unauthorized: session token expired"
        ));
        assert!(!is_missing_session_error(
            "Authentication failed: invalid cookie"
        ));
        assert!(!should_retry_mcp_fallback(&BootRpcError::Timeout(
            "t".into()
        )));
        assert!(!should_retry_mcp_fallback(&BootRpcError::ShimExited(
            "e".into()
        )));
        assert!(!should_retry_mcp_fallback(&BootRpcError::Rpc(
            "Session not found".into()
        )));
        assert!(should_retry_mcp_fallback(&BootRpcError::Rpc(
            "Invalid params: unsupported mcpServers type".into()
        )));

        // Reject option fallback & None -> cancelled, plus strict allow_once separation
        let opts_reject_always = vec![
            json!({ "optionId": "a1", "kind": "allow_once" }),
            json!({ "optionId": "r_all", "kind": "reject_always" }),
        ];
        let parsed = crate::AcpPermissionOptions::parse(&opts_reject_always);
        assert_eq!(parsed.deny.as_deref(), Some("r_all"));
        assert_eq!(
            parsed.allowed_decisions(false, true),
            vec!["allow".to_string(), "deny".to_string()]
        );

        let opts_only_allow = vec![json!({ "optionId": "a1", "kind": "allow_once" })];
        let parsed_only_allow = crate::AcpPermissionOptions::parse(&opts_only_allow);
        assert_eq!(parsed_only_allow.deny, None);
        assert_eq!(
            parsed_only_allow.deny_outcome(),
            json!({ "outcome": "cancelled" })
        );

        let opts_only_always = vec![
            json!({ "optionId": "aa1", "kind": "allow_always" }),
            json!({ "optionId": "r1", "kind": "reject_once" }),
        ];
        let parsed_only_always = crate::AcpPermissionOptions::parse(&opts_only_always);
        assert_eq!(parsed_only_always.allow_once, None);
        assert_eq!(
            parsed_only_always.allowed_decisions(false, true),
            vec!["allow_always".to_string(), "deny".to_string()]
        );
        assert_eq!(
            parsed_only_always.outcome_for_decision("allow"),
            json!({ "outcome": "cancelled" })
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generic_acp_unknown_rpc_concurrent_perms_and_interrupt_clears_queue() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("orbit_acp_edge_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let mock_bin = dir.join("mock-acp-edge");
        let log_file = dir.join("edge.log");
        let sock_path = dir.join("acp_edge.sock");

        std::fs::write(
            &mock_bin,
            format!(
                r#"#!/bin/sh
LOG="{}"
while IFS= read -r line; do
  case "$line" in
    *'"method":"initialize"'*)
      # Send an unsupported server->client callback during boot to verify -32601 reply (no deadlock)
      printf '%s\n' '{{"jsonrpc":"2.0","id":"boot_cb","method":"fs/read_text_file","params":{{"path":"/etc/hosts"}}}}'
      IFS= read -r cb_reply
      printf 'BOOT_CB=%s\n' "$cb_reply" >> "$LOG"
      printf '%s\n' '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":1,"agentInfo":{{"name":"EdgeACP","version":"1.0"}}}}}}'
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":2,"result":{{"sessionId":"sess_edge_1"}}}}'
      ;;
    *'"method":"session/prompt"'*'"edge_turn"'*)
      RID=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
      # Tool call followed by partial tool_call_update omitting kind
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_edge_1","update":{{"sessionUpdate":"tool_call","toolCallId":"tc_e1","kind":"execute","title":"Build","status":"in_progress","rawInput":{{"command":"cargo check","cwd":"/work"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"sess_edge_1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"tc_e1","status":"completed","rawOutput":{{"exitCode":0,"stdout":"ok"}}}}}}}}'
      # Emit two concurrent permissions before waiting for replies
      printf '%s\n' '{{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{{"sessionId":"sess_edge_1","toolCall":{{"toolCallId":"tc_p1","kind":"execute","title":"Cmd 1","rawInput":{{"command":"echo 1"}}}},"options":[{{"optionId":"a1","kind":"allow_once"}}]}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":"p2","method":"session/request_permission","params":{{"sessionId":"sess_edge_1","toolCall":{{"toolCallId":"tc_p2","kind":"execute","title":"Cmd 2","rawInput":{{"command":"echo 2"}}}},"options":[{{"optionId":"a2","kind":"allow_once"}}]}}}}'
      IFS= read -r r1
      printf 'PERM1=%s\n' "$r1" >> "$LOG"
      IFS= read -r r2
      printf 'PERM2=%s\n' "$r2" >> "$LOG"
      # Wait for cancel
      IFS= read -r cancel_msg
      printf 'AFTER=%s\n' "$cancel_msg" >> "$LOG"
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"cancelled"}}}}\n' "$RID"
      ;;
    *'"method":"session/prompt"'*'"should_be_cleared"'*)
      printf 'UNEXPECTED_QUEUED_PROMPT=1\n' >> "$LOG"
      ;;
  esac
done
"#,
                log_file.display()
            ),
        )
        .expect("write mock");
        let mut perms = std::fs::metadata(&mock_bin).expect("meta").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&mock_bin, perms).expect("chmod");

        let shim_sock_bg = sock_path.clone();
        let shim_spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "acp_edge".into(),
            mode: ShimMode::Stdio,
            program: mock_bin.to_string_lossy().to_string(),
            args: vec![],
            cwd: dir.clone(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
        };
        let shim_task = tokio::spawn(async move {
            let _ = orbit_shim::run_shim(&shim_sock_bg, shim_spec).await;
        });

        let mut shim_client = None;
        for _ in 0..40 {
            if let Some(c) = connect_existing_shim_after(&sock_path, None).await {
                shim_client = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let shim_client = shim_client.expect("shim connected");

        let spec = AcpAgentSpec {
            id: "edge-acp".into(),
            name: "Edge ACP".into(),
            description: String::new(),
            command: mock_bin.to_string_lossy().to_string(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_edge".into(),
            provider: orbit_protocol::ProviderKind::Acp,
            executable: mock_bin.clone(),
            orbit_exe: PathBuf::from("orbit"),
            shim_socket: sock_path.clone(),
            runtime_dir: dir.clone(),
            cwd: dir.clone(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: None,
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
            shim_seq_tracker: Arc::new(AtomicU64::new(0)),
            persisted_active_turn_id: None,
            persisted_turn_step_seq: 0,
            fallback_handoff_context: None,
            persisted_running_bg_tasks: vec![],
            persisted_bg_wait: false,
            persisted_initializing: false,
            acp_agent_id: Some("edge-acp".into()),
            acp_spec: Some(spec.clone()),
        };

        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let actor = tokio::spawn(run_acp_actor(cfg, spec, shim_client, false, cmd_rx, evt_tx));

        // Wait for boot Idle (proves -32601 reply prevented boot deadlock!)
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        let mut booted = false;
        while tokio::time::Instant::now() < deadline {
            if let Ok(Some(DriverEvent::StatusChanged(AgentStatus::Idle))) =
                tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                booted = true;
                break;
            }
        }
        assert!(booted, "boot should succeed after -32601 reply");

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_edge".into(),
                prompt: "edge_turn".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send edge_turn");

        let mut perm_ids = Vec::new();
        let mut saw_partial_shell_completed = false;
        let deadline2 = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline2 && perm_ids.len() < 2 {
            if let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                match ev {
                    DriverEvent::ToolCall {
                        status: ToolCallStatus::Completed,
                        detail:
                            ToolCallDetail::Shell {
                                command,
                                cwd,
                                exit_code: Some(0),
                                ..
                            },
                        ..
                    } if command == "cargo check" && cwd.as_deref() == Some("/work") => {
                        saw_partial_shell_completed = true;
                    }
                    DriverEvent::PermissionRequest { request_id, .. } => {
                        perm_ids.push(request_id);
                    }
                    _ => {}
                }
            }
        }
        assert!(
            saw_partial_shell_completed,
            "partial tool_call_update without kind must keep Shell + cwd"
        );
        assert_eq!(perm_ids.len(), 2, "expected 2 concurrent permissions");

        // Resolve first permission with deny (which has no deny option -> must send cancelled outcome!)
        cmd_tx
            .send(DriverCommand::ResolvePermission {
                request_id: perm_ids[0].clone(),
                decision: "deny".into(),
            })
            .await
            .expect("resolve perm 1");

        // Must stay AwaitingPermission for perm_ids[1], NOT switch to Running yet!
        let status_after_first = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
            .await
            .expect("status event")
            .expect("some");
        assert!(
            matches!(
                status_after_first,
                DriverEvent::StatusChanged(AgentStatus::AwaitingPermission { ref request_id, .. })
                    if request_id == &perm_ids[1]
            ),
            "expected AwaitingPermission for second request, got {status_after_first:?}"
        );

        // Resolve second permission -> now transitions to Running
        cmd_tx
            .send(DriverCommand::ResolvePermission {
                request_id: perm_ids[1].clone(),
                decision: "allow".into(),
            })
            .await
            .expect("resolve perm 2");
        let status_after_second = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv())
            .await
            .expect("status event")
            .expect("some");
        assert!(
            matches!(
                status_after_second,
                DriverEvent::StatusChanged(AgentStatus::Running { .. })
            ),
            "expected Running after all permissions resolved, got {status_after_second:?}"
        );

        // Queue a follow-up prompt while turn is still running, then send Interrupt -> queued prompt MUST be cleared!
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_queued".into(),
                prompt: "should_be_cleared".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("queue prompt");
        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("interrupt");

        let deadline3 = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline3 {
            if let Ok(Some(DriverEvent::StatusChanged(AgentStatus::Idle))) =
                tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await
            {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(80)).await;

        let log_contents = std::fs::read_to_string(&log_file).unwrap_or_default();
        assert!(
            log_contents.contains("-32601"),
            "expected -32601 reply for unknown server RPC, got: {log_contents}"
        );
        assert!(
            log_contents.contains("\"cancelled\""),
            "expected cancelled outcome when no deny option exists, got: {log_contents}"
        );
        assert!(
            !log_contents.contains("UNEXPECTED_QUEUED_PROMPT"),
            "Interrupt must clear pending_prompts, got: {log_contents}"
        );

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generic_acp_agent_steer_degrades_to_queue_and_interrupt_and_drain_queue_preserves_fifo()
     {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let short_id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let sock_path = std::env::temp_dir().join(format!("o_acpg_deliv_{short_id}.sock"));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let received_prompts: Arc<tokio::sync::Mutex<Vec<String>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&received_prompts);
        let cancel_count = Arc::new(AtomicU64::new(0));
        let cancel_bg = Arc::clone(&cancel_count);
        let (release_slow_tx, mut release_slow_rx) = mpsc::channel::<()>(2);

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
                    _ = release_slow_rx.recv() => {
                        if let Some(pid) = inflight_prompt_id.take() {
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
                                        "result": { "sessionId": "sess-acpg-deliv" }
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
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "jsonrpc": "2.0",
                                            "method": "session/update",
                                            "params": {
                                                "sessionId": "sess-acpg-deliv",
                                                "update": {
                                                    "sessionUpdate": "agent_thought_chunk",
                                                    "content": { "type": "text", "text": "pondering" }
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
                                                "sessionId": "sess-acpg-deliv",
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
            .expect("connect");
        let spec = AcpAgentSpec {
            id: "deliv-acp".into(),
            name: "Deliv ACP".into(),
            description: String::new(),
            command: "/bin/true".into(),
            args: vec![],
            env: HashMap::new(),
            install_hint: None,
            homepage: None,
            version: None,
            source: AcpAgentSource::Custom,
            enabled: true,
        };
        let cfg = SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_acpg_deliv".into(),
            provider: orbit_protocol::ProviderKind::Acp,
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
            mode: SessionMode::Default,
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
            acp_agent_id: Some("deliv-acp".into()),
            acp_spec: Some(spec.clone()),
        };
        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let actor = tokio::spawn(run_acp_actor(cfg, spec, shim, false, cmd_rx, evt_tx));

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

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t1".into(),
                prompt: "SLOW_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("t1");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("think")
                    .expect("ev"),
                DriverEvent::ThinkingText { .. }
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_ag".into(),
                prompt: "AGENT_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_ag".into()],
            })
            .await
            .expect("agent steer");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);

        // Human Steer on generic ACP MUST degrade safely to non-destructive priority queueing
        // (zero session/cancel!), draining ahead of queued agent notifications once SLOW_1 settles.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_hu".into(),
                prompt: "HUMAN_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("human steer");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            cancel_count.load(Ordering::SeqCst),
            0,
            "Human Steer on generic ACP MUST NOT send destructive session/cancel"
        );
        release_slow_tx.send(()).await.expect("release SLOW_1");
        let mut ag_events = Vec::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("idle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } if message_ids == vec!["msg_ag".to_string()] => ag_events.push(status),
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if ag_events.contains(&AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 0);
        assert_eq!(
            ag_events,
            vec![
                AgentMessageDeliveryStatus::Queued,
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t2".into(),
                prompt: "SLOW_2".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("t2");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("think 2")
                    .expect("ev"),
                DriverEvent::ThinkingText { .. }
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q1".into(),
                prompt: "Q1".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["m_q1".into()],
            })
            .await
            .expect("q1");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q2".into(),
                prompt: "Q2".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["m_q2".into()],
            })
            .await
            .expect("q2");
        cmd_tx
            .send(DriverCommand::InterruptAndDrainQueue)
            .await
            .expect("interrupt drain");

        let mut q2_settled = false;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("drain idle")
                .expect("ev");
            match ev {
                DriverEvent::PromptDeliveryUpdated {
                    message_ids,
                    status,
                    ..
                } => {
                    assert_ne!(status, AgentMessageDeliveryStatus::Dropped);
                    if message_ids == vec!["m_q2".to_string()]
                        && status == AgentMessageDeliveryStatus::Settled
                    {
                        q2_settled = true;
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) if q2_settled => break,
                _ => {}
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 1);

        // Explicit ActiveTurnBehavior::Interrupt still emits session/cancel and preempts immediately.
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t3".into(),
                prompt: "SLOW_3".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("t3");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("think 3")
                    .expect("ev"),
                DriverEvent::ThinkingText { .. }
            ) {
                break;
            }
        }
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_int".into(),
                prompt: "HUMAN_INTERRUPT".into(),
                behavior: ActiveTurnBehavior::Interrupt,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("human interrupt");
        loop {
            if matches!(
                tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                    .await
                    .expect("interrupt idle")
                    .expect("ev"),
                DriverEvent::StatusChanged(AgentStatus::Idle)
            ) {
                break;
            }
        }
        assert_eq!(cancel_count.load(Ordering::SeqCst), 2);
        assert_eq!(
            *received_prompts.lock().await,
            vec![
                "SLOW_1",
                "HUMAN_1",
                "AGENT_1",
                "SLOW_2",
                "Q1",
                "Q2",
                "SLOW_3",
                "HUMAN_INTERRUPT",
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }
}
