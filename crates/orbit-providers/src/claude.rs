//! Native Rust driver for Anthropic Claude Code (`claude` bidirectional `stream-json` Control Protocol
//! + `~/.claude/.credentials.json` / macOS Keychain OAuth Usage).

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

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
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::attachments::build_claude_content_blocks_async;
use crate::driver::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, ProviderSessionHandle, QueuedDriverPrompt,
    SessionLaunchConfig, emit_dropped_queued_prompts, emit_prompt_delivery,
};

const CLAUDE_MCP_FILE_PREFIX: &str = "claude-mcp-";
const CLAUDE_MCP_FILE_SUFFIX: &str = ".json";

struct ClaudeMcpConfigGuard {
    path: Option<PathBuf>,
}

impl ClaudeMcpConfigGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn into_path(mut self) -> Option<PathBuf> {
        self.path.take()
    }
}

impl Drop for ClaudeMcpConfigGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn resolve_absolute_runtime_dir(runtime_dir: &Path) -> Result<PathBuf, String> {
    let abs = if runtime_dir.is_absolute() {
        runtime_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| format!("failed to resolve current working directory: {e}"))?
            .join(runtime_dir)
    };
    Ok(std::fs::canonicalize(&abs).unwrap_or(abs))
}

fn sanitize_mcp_agent_id(agent_id: &str) -> String {
    let cleaned: String = agent_id
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "agent".to_string()
    } else {
        cleaned
    }
}

fn extract_claude_mcp_config_agent_id(file_name: &str) -> Option<&str> {
    let stem = file_name
        .strip_prefix(CLAUDE_MCP_FILE_PREFIX)?
        .strip_suffix(CLAUDE_MCP_FILE_SUFFIX)?;
    // Canonical hyphenated UUID is 36 ASCII bytes preceded by '-'.
    if stem.len() <= 37 {
        return None;
    }
    let sep_idx = stem.len() - 37;
    if stem.as_bytes().get(sep_idx) != Some(&b'-') {
        return None;
    }
    let uuid_part = &stem[sep_idx + 1..];
    Uuid::parse_str(uuid_part).ok()?;
    Some(&stem[..sep_idx])
}

/// Write a private MCP config JSON file under `<runtime_dir>/claude-mcp-<agent_id>-<uuid>.json`
/// so the Orbit MCP Bearer token is never exposed on `claude`'s process `argv`.
///
/// - On Unix, `<runtime_dir>` is created/verified as a real, current-UID-owned `0700` directory
///   and the file is opened directly via `create_new(true)` with mode `0600` before writing.
/// - On Windows (with workspace `unsafe_code = "deny"`), `<runtime_dir>` resides under the current
///   user's private Orbit profile directory (`%USERPROFILE%\.orbit\runtime`) with exclusive
///   `create_new(true)` creation.
/// - Always returns an absolute path so relative `--home` flags work regardless of `claude`'s `cwd`.
fn write_claude_mcp_config_file(
    runtime_dir: &Path,
    agent_id: &str,
    mcp: &OrbitMcpLaunchSpec,
) -> Result<ClaudeMcpConfigGuard, String> {
    orbit_shim::ensure_private_dir(runtime_dir)
        .map_err(|e| format!("failed to prepare private Orbit runtime dir: {e}"))?;
    let abs_runtime_dir = resolve_absolute_runtime_dir(runtime_dir)?;

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

    let safe_id = sanitize_mcp_agent_id(agent_id);
    let uuid = Uuid::new_v4();
    let final_path = abs_runtime_dir.join(format!(
        "{CLAUDE_MCP_FILE_PREFIX}{safe_id}-{uuid}{CLAUDE_MCP_FILE_SUFFIX}"
    ));

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let mut file = opts
        .open(&final_path)
        .map_err(|e| format!("failed to create private claude MCP config file: {e}"))?;
    // Attach cleanup guard immediately so any subsequent error unlinks the newly created file.
    let guard = ClaudeMcpConfigGuard::new(final_path);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to enforce 0600 on claude MCP config file: {e}"))?;
    }
    file.write_all(mcp_json.as_bytes())
        .map_err(|e| format!("failed to write private claude MCP config file: {e}"))?;
    drop(file);

    Ok(guard)
}

/// Snapshot existing `claude-mcp-<agent_id>-<uuid>.json` files inside `runtime_dir` BEFORE startup
/// recovery spawns any new agent tasks.
pub fn snapshot_claude_mcp_config_files(runtime_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(name_str) = name.to_str()
            && extract_claude_mcp_config_agent_id(name_str).is_some()
        {
            out.push(entry.path());
        }
    }
    out
}

/// Remove ONLY pre-snapshotted `claude-mcp-<agent_id>-<uuid>.json` candidate files whose agent did
/// not survive daemon restart (never touching fresh files written by newly launching agents).
pub fn reap_snapshotted_claude_mcp_configs(
    candidates: &[PathBuf],
    surviving_agent_ids: &HashSet<String>,
) -> usize {
    let surviving_sanitized: HashSet<String> = surviving_agent_ids
        .iter()
        .map(|id| sanitize_mcp_agent_id(id))
        .collect();
    let mut removed = 0usize;
    for path in candidates {
        let Some(name_str) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(file_agent_id) = extract_claude_mcp_config_agent_id(name_str)
            && !surviving_sanitized.contains(file_agent_id)
            && std::fs::remove_file(path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

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
    mcp_config_path: Option<&Path>,
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
    if let Some(path) = mcp_config_path {
        args.push("--mcp-config".to_string());
        args.push(path.to_string_lossy().to_string());
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
    let mcp_guard = match cfg.mcp_spec {
        Some(ref mcp) => Some(write_claude_mcp_config_file(
            &cfg.runtime_dir,
            &cfg.agent_id,
            mcp,
        )?),
        None => None,
    };
    let mcp_path_opt = mcp_guard.as_ref().and_then(ClaudeMcpConfigGuard::path);
    let args = build_claude_launch_args(cfg, start_mode, mcp_path_opt);

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

    let cleanup_paths = mcp_path_opt
        .map(|p| vec![p.to_path_buf()])
        .unwrap_or_default();
    let spec = ShimSpawnSpec {
        sandbox: cfg.sandbox.clone(),
        egress_proxy_url: None,
        shim_id: format!("claude_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args,
        cwd: cfg.cwd.clone(),
        env,
        cleanup_paths,
    };

    let mut fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
        .await
        .map_err(|e| format!("failed to spawn claude shim: {e}"))?;

    // `orbit shim` now owns the live child process and will unlink `spec.cleanup_paths`
    // immediately after the child actually exits (surviving daemon restarts/reattaches).
    if let Some(guard) = mcp_guard {
        let _ = guard.into_path();
    }

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
        if let Some(ack_seq) = existing.attach_ack_seq() {
            cfg.shim_seq_tracker.fetch_max(ack_seq, Ordering::Relaxed);
        }
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
    original_plan_markdown: String,
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
    let mut pending_fork_first_prompt: Option<QueuedDriverPrompt> = None;

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
    let mut managed_dispatches = HashSet::new();
    let mut pending_perms: HashMap<String, PendingClaudePerm> = HashMap::new();
    let mut queued_prompts: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<QueuedDriverPrompt> = None;
    let mut active_turn_message_ids: Vec<String> = Vec::new();
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
    let mut current_mode: SessionMode = cfg.mode.clone();
    let mut last_plan_file_path: Option<String> = None;
    let mut last_assistant_text: Option<String> = None;

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else {
                    // Daemon disconnected without killing the shim (e.g. hot-restart):
                    // leave the MCP config path intact while the detached shim stays alive.
                    break;
                };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        mut prompt,
                        behavior,
                        origin: _origin,
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
                        if let Some(ref blocking_tid) = active_turn_id {
                            let blocking_tid_opt = Some(blocking_tid.clone());
                            match behavior {
                                ActiveTurnBehavior::Queue => {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid_opt,
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
                                    let rid = format!("ctrl_{ctrl_seq}");
                                    ctrl_seq += 1;
                                    let req = json!({
                                        "request_id": rid,
                                        "type": "control_request",
                                        "request": {
                                            "subtype": "interrupt",
                                            "cancel_queued": false
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
                                        blocking_tid_opt,
                                    )
                                    .await;
                                    pending_interrupt_replacement = Some(QueuedDriverPrompt {
                                        turn_id,
                                        prompt,
                                        message_ids,
                                    });
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
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            blocking_tid,
                                            &message_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                        emit_dropped_queued_prompts(
                                            &evt_tx,
                                            pending_interrupt_replacement.take(),
                                            &mut queued_prompts,
                                        )
                                        .await;
                                        let active_ids =
                                            std::mem::take(&mut active_turn_message_ids);
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            blocking_tid,
                                            &active_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                        break;
                                    }
                                    active_turn_message_ids.extend(message_ids.iter().cloned());
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        blocking_tid,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Dispatched,
                                        None,
                                    )
                                    .await;
                                    continue;
                                }
                            }
                        }

                        if forked_natively && !emitted_session_init {
                            pending_fork_first_prompt = Some(QueuedDriverPrompt {
                                turn_id: turn_id.clone(),
                                prompt: prompt.clone(),
                                message_ids: message_ids.clone(),
                            });
                        }

                        active_turn_id = Some(turn_id.clone());
                        active_turn_message_ids = message_ids;
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
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut queued_prompts,
                        )
                        .await;
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
                    DriverCommand::InterruptAndDrainQueue => {
                        if active_turn_id.is_some() {
                            let rid = format!("ctrl_{ctrl_seq}");
                            ctrl_seq += 1;
                            let req = json!({
                                "request_id": rid,
                                "type": "control_request",
                                "request": {
                                    "subtype": "interrupt",
                                    "cancel_queued": false
                                }
                            });
                            let _ = shim.write_stdin_line(req.to_string()).await;
                        } else if let Some(next_q) = pending_interrupt_replacement
                            .take()
                            .or_else(|| queued_prompts.pop_front())
                        {
                            active_turn_id = Some(next_q.turn_id.clone());
                            active_turn_message_ids = next_q.message_ids;
                            streamed_blocks.clear();
                            emitted_text_in_turn = false;
                            last_call_prompt_tokens = None;
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                    turn_id: next_q.turn_id.clone(),
                                }))
                                .await;
                            let content_blocks = build_claude_content_blocks_async(
                                next_q.prompt,
                                cfg.attachments_dir.clone(),
                            )
                            .await;
                            let user_frame =
                                build_claude_user_frame(&session_id, content_blocks, false);
                            if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
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
                    DriverCommand::SetMode { mode } => {
                        current_mode = mode.clone();
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
                    DriverCommand::ResolvePlanApproval {
                        request_id,
                        action,
                        edited_plan_markdown,
                        feedback,
                        target_mode,
                    } => {
                        if matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild) {
                            current_mode = target_mode.clone();
                        }
                        if let Some(p) = pending_perms.remove(&request_id) {
                            if matches!(action, orbit_protocol::PlanApprovalAction::ApproveAndBuild)
                            {
                                let mode_rid = format!("ctrl_{ctrl_seq}");
                                ctrl_seq += 1;
                                let mode_req = json!({
                                    "request_id": mode_rid,
                                    "type": "control_request",
                                    "request": {
                                        "subtype": "set_permission_mode",
                                        "mode": claude_mode_flag(&target_mode)
                                    }
                                });
                                let _ = shim.write_stdin_line(mode_req.to_string()).await;
                            }
                            let resp = crate::plan_approval::build_claude_plan_control_response(
                                &p.control_request_id,
                                &p.tool_use_id,
                                &p.original_input,
                                &p.original_plan_markdown,
                                claude_mode_flag(&target_mode),
                                action,
                                edited_plan_markdown.as_deref(),
                                feedback.as_deref(),
                            );
                            let _ = shim.write_stdin_line(resp.to_string()).await;
                            if let Some(ref tid) = active_turn_id {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                        turn_id: tid.clone(),
                                    }))
                                    .await;
                            }
                        } else if matches!(
                            action,
                            orbit_protocol::PlanApprovalAction::ApproveAndBuild
                        ) {
                            let mode_rid = format!("ctrl_{ctrl_seq}");
                            ctrl_seq += 1;
                            let mode_req = json!({
                                "request_id": mode_rid,
                                "type": "control_request",
                                "request": {
                                    "subtype": "set_permission_mode",
                                    "mode": claude_mode_flag(&target_mode)
                                }
                            });
                            let _ = shim.write_stdin_line(mode_req.to_string()).await;
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
                let Some(frame) = maybe_frame else {
                    emit_dropped_queued_prompts(
                        &evt_tx,
                        pending_interrupt_replacement.take(),
                        &mut queued_prompts,
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
                                                        if let Some(orig_q) =
                                                            pending_fork_first_prompt.take()
                                                        {
                                                            inject_handoff_on_next_prompt = false;
                                                            let wire_prompt = if let Some(ctx) =
                                                                pending_handoff_context.take()
                                                                && !ctx.trim().is_empty()
                                                                && !orig_q.prompt.contains(
                                                                    "[Prior Conversation Context in this Workspace]",
                                                                )
                                                            {
                                                                format!("{ctx}\n\n{}", orig_q.prompt)
                                                            } else {
                                                                orig_q.prompt
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
                                                                let dropped = std::mem::take(
                                                                    &mut active_turn_message_ids,
                                                                );
                                                                emit_prompt_delivery(
                                                                    &evt_tx,
                                                                    &orig_q.turn_id,
                                                                    &dropped,
                                                                    AgentMessageDeliveryStatus::Dropped,
                                                                    None,
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
                                        if let Some(bg_kind) = classify_claude_background_task_type(task_type) {
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
                                                    .unwrap_or_else(|| match bg_kind {
                                                        ClaudeBgTaskKind::Shell => ToolCallDetail::Shell {
                                                            command: desc
                                                                .clone()
                                                                .unwrap_or_else(|| "Background command".into()),
                                                            cwd: None,
                                                            exit_code: None,
                                                            output_preview: None,
                                                        },
                                                        ClaudeBgTaskKind::Timer => ToolCallDetail::Generic {
                                                            tool_name: "schedule".to_string(),
                                                            arguments_json: serde_json::json!({
                                                                "task_type": task_type,
                                                                "description": desc,
                                                            })
                                                            .to_string(),
                                                        },
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
                                            last_assistant_text = Some(full_txt.clone());
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
                                            if crate::managed_dispatch::is_launch(name) {
                                                managed_dispatches.insert(call_id.clone());
                                            }
                                            let detail = map_claude_tool_detail(name, input, None);
                                            if matches!(name, "Write" | "Edit" | "MultiEdit")
                                                && let Some(fp) = input
                                                    .get("file_path")
                                                    .and_then(|v| v.as_str())
                                                && orbit_protocol::looks_like_plan_file_path(fp)
                                            {
                                                last_plan_file_path = Some(fp.to_string());
                                            }
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
                                            last_assistant_text = Some(txt.to_string());
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
                                                Some(detail) if managed_dispatches.remove(call_id) => {
                                                    crate::managed_dispatch::detail(&Value::Null, &b["content"], Some(&detail))
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
                                    &current_mode,
                                    &cfg.cwd,
                                    last_plan_file_path.as_deref(),
                                    last_assistant_text.as_deref(),
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
                                let finished_tid = active_turn_id.take();
                                let settled_ids = std::mem::take(&mut active_turn_message_ids);
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
                                        let reported_window = extract_claude_model_context_window(
                                            &val,
                                            reported_model.as_deref(),
                                        );
                                        let _ = evt_tx
                                            .send(DriverEvent::TurnUsage {
                                                input_tokens,
                                                output_tokens,
                                                cache_read_tokens,
                                                cache_write_tokens,
                                                last_call_prompt_tokens: single_call_prompt,
                                                context_window_tokens: reported_window,
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

                                if let Some(ref tid) = finished_tid {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        tid,
                                        &settled_ids,
                                        AgentMessageDeliveryStatus::Settled,
                                        None,
                                    )
                                    .await;
                                }

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

                                // Drain interrupted-replacement or accepted FIFO queued prompt immediately on turn settle
                                if let Some(next_q) = pending_interrupt_replacement
                                    .take()
                                    .or_else(|| queued_prompts.pop_front())
                                {
                                    active_turn_id = Some(next_q.turn_id.clone());
                                    active_turn_message_ids = next_q.message_ids;
                                    streamed_blocks.clear();
                                    emitted_text_in_turn = false;
                                    last_call_prompt_tokens = None;
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: next_q.turn_id.clone(),
                                        }))
                                        .await;
                                    let content_blocks = build_claude_content_blocks_async(
                                        next_q.prompt,
                                        cfg.attachments_dir.clone(),
                                    )
                                    .await;
                                    let user_frame = build_claude_user_frame(
                                        &session_id,
                                        content_blocks,
                                        false,
                                    );
                                    if shim.write_stdin_line(user_frame.to_string()).await.is_err() {
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
                            _ => {}
                        }
                    }
                    ShimFrame::Exited { code } => {
                        emit_dropped_queued_prompts(
                            &evt_tx,
                            pending_interrupt_replacement.take(),
                            &mut queued_prompts,
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

fn extract_claude_model_context_window(val: &Value, dominant_model: Option<&str>) -> Option<u64> {
    let map = val.get("modelUsage").and_then(|v| v.as_object())?;
    let entry = dominant_model
        .and_then(|m| map.get(m))
        .or_else(|| map.values().next())?;
    entry
        .get("contextWindow")
        .or_else(|| entry.get("context_window"))
        .and_then(|v| v.as_u64())
        .filter(|&w| w > 0)
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

#[allow(clippy::too_many_arguments)]
async fn handle_claude_control_request(
    val: &Value,
    current_mode: &SessionMode,
    workspace_cwd: &Path,
    last_plan_file_path: Option<&str>,
    last_assistant_text: Option<&str>,
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
                original_plan_markdown: String::new(),
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
    let raw_title = req
        .get("title")
        .or_else(|| req.get("description"))
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned);

    let mut plan_approval = orbit_protocol::extract_plan_approval_from_tool_input(
        tool_name,
        &input,
        raw_title.as_deref(),
        matches!(current_mode, SessionMode::Plan),
        Some("exit_plan_mode"),
    );
    if let Some(ref mut plan) = plan_approval {
        crate::plan_approval::enrich_plan_payload(
            plan,
            workspace_cwd,
            last_plan_file_path,
            last_assistant_text,
        );
    }

    let title = if let Some(ref plan) = plan_approval {
        plan.plan_title
            .clone()
            .unwrap_or_else(|| "Review implementation plan".to_string())
    } else {
        raw_title.unwrap_or_else(|| format!("Claude wants to use {tool_name}"))
    };
    let detail = map_claude_tool_detail(tool_name, &input, None);
    let allowed_decisions = if plan_approval.is_some() {
        vec!["allow".into(), "deny".into()]
    } else {
        vec!["allow".into(), "allow_always".into(), "deny".into()]
    };

    pending_perms.insert(
        req_id.clone(),
        PendingClaudePerm {
            control_request_id: ctrl_req_id.to_string(),
            tool_use_id,
            original_input: input,
            permission_suggestions: suggestions,
            is_question: false,
            question_texts: Vec::new(),
            original_plan_markdown: plan_approval
                .as_ref()
                .map(|p| p.plan_markdown.clone())
                .unwrap_or_default(),
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
        name if crate::managed_dispatch::is_launch(name) => {
            crate::managed_dispatch::detail(input, &Value::Null, None)
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaudeBgTaskKind {
    Shell,
    Timer,
}

fn classify_claude_background_task_type(task_type: &str) -> Option<ClaudeBgTaskKind> {
    let lower = task_type.trim().to_ascii_lowercase();
    if lower == "local_bash" {
        Some(ClaudeBgTaskKind::Shell)
    } else if matches!(
        lower.as_str(),
        "local_timer" | "timer" | "cron" | "schedule" | "monitor" | "local_monitor"
    ) || lower.contains("timer")
        || lower.contains("cron")
        || lower.contains("schedule")
        || lower.contains("monitor")
    {
        Some(ClaudeBgTaskKind::Timer)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::PromptOrigin;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    fn test_claude_cfg(fork_sid: Option<&str>, native_fork_only: bool) -> SessionLaunchConfig {
        SessionLaunchConfig {
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_claude_fork_test".into(),
            provider: ProviderKind::ClaudeCode,
            executable: PathBuf::from("/bin/true"),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: std::env::temp_dir().join("orbit_claude_unused.sock"),
            runtime_dir: std::env::temp_dir().join("orbit_claude_test_runtime"),
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
            sandbox: None,
            egress_proxy_url: None,
            agent_id: "ag_claude_test".into(),
            provider: ProviderKind::ClaudeCode,
            executable: PathBuf::from("/usr/bin/claude"),
            orbit_exe: PathBuf::from("/usr/bin/orbit"),
            shim_socket: PathBuf::from("/tmp/claude.sock"),
            runtime_dir: std::env::temp_dir().join("orbit_claude_sample_runtime"),
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
        let args = build_claude_launch_args(
            &cfg,
            ClaudeSessionStartMode::ForkFrom("parent-sess-123"),
            None,
        );
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
        let off_args = build_claude_launch_args(
            &sample_cfg(false),
            ClaudeSessionStartMode::Fresh("sess-1"),
            None,
        );
        assert!(!off_args.iter().any(|a| a.starts_with("--settings")));

        let on_args = build_claude_launch_args(
            &sample_cfg(true),
            ClaudeSessionStartMode::Fresh("sess-1"),
            None,
        );
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

    #[test]
    fn claude_mcp_config_writes_0600_file_passes_absolute_path_not_bearer_token_and_cleans_up() {
        let home_dir = std::env::temp_dir().join(format!(
            "orbit_claude_mcp_cfg_test_{}",
            Uuid::new_v4().simple()
        ));
        let _ = std::fs::remove_dir_all(&home_dir);
        let runtime_dir = home_dir.join("runtime");

        let secret_token = "mcp-secret-bearer-token-9a8b7c6d5e4f";
        let mcp = OrbitMcpLaunchSpec {
            server_name: "orbit".into(),
            http_url: "http://127.0.0.1:6868/mcp/agents?caller_agent_id=ag_test_1".into(),
            bearer_token: secret_token.into(),
            stdio_command: PathBuf::from("/usr/local/bin/orbit"),
            stdio_args: vec!["mcp".into()],
        };

        let guard = write_claude_mcp_config_file(&runtime_dir, "ag_test_1", &mcp)
            .expect("write private mcp config");
        let cfg_path = guard.path().expect("guard path").to_path_buf();
        assert!(cfg_path.is_file(), "mcp config file must exist on disk");
        assert!(
            cfg_path.is_absolute(),
            "mcp config path passed to claude must be absolute: {}",
            cfg_path.display()
        );
        let canonical_runtime = std::fs::canonicalize(&runtime_dir).unwrap_or(runtime_dir.clone());
        assert_eq!(
            cfg_path.parent(),
            Some(canonical_runtime.as_path()),
            "mcp config must be created inside runtime_dir"
        );
        let file_name = cfg_path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf8 filename");
        assert!(file_name.starts_with("claude-mcp-ag_test_1-"));
        assert!(file_name.ends_with(".json"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let runtime_mode = std::fs::metadata(&runtime_dir)
                .expect("runtime meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                runtime_mode, 0o700,
                "runtime dir must be 0700, got {:o}",
                runtime_mode
            );
            let file_mode = std::fs::metadata(&cfg_path)
                .expect("file meta")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(
                file_mode, 0o600,
                "claude MCP config file must be 0600, got {:o}",
                file_mode
            );
        }

        let raw = std::fs::read_to_string(&cfg_path).expect("read mcp config");
        let parsed: Value = serde_json::from_str(&raw).expect("valid mcp json");
        assert_eq!(
            parsed.pointer("/mcpServers/orbit/headers/Authorization"),
            Some(&json!(format!("Bearer {secret_token}")))
        );
        assert_eq!(
            parsed.pointer("/mcpServers/orbit/url"),
            Some(&json!(mcp.http_url))
        );

        let mut cfg = sample_cfg(false);
        cfg.mcp_spec = Some(mcp.clone());
        let args = build_claude_launch_args(
            &cfg,
            ClaudeSessionStartMode::Fresh("sess-mcp-1"),
            guard.path(),
        );
        let flag_idx = args
            .iter()
            .position(|a| a == "--mcp-config")
            .expect("--mcp-config flag");
        assert_eq!(args[flag_idx + 1], cfg_path.to_string_lossy());
        assert!(
            !args.iter().any(|a| a.contains(secret_token)),
            "claude argv must NEVER contain the MCP bearer token: {args:?}"
        );
        assert!(
            !args.iter().any(|a| a.contains("Authorization")),
            "claude argv must NEVER contain inline Authorization header: {args:?}"
        );

        drop(guard);
        assert!(
            !cfg_path.exists(),
            "dropping ClaudeMcpConfigGuard must delete the private MCP config file"
        );

        // Verify relative runtime_dir still resolves to an absolute path for --mcp-config.
        let rel_dir = PathBuf::from(format!("target/.tmp-rel-rt-{}", Uuid::new_v4().simple()));
        let rel_guard = write_claude_mcp_config_file(&rel_dir, "ag_rel", &mcp)
            .expect("write relative runtime_dir mcp config");
        assert!(
            rel_guard.path().expect("rel path").is_absolute(),
            "relative runtime_dir must resolve to an absolute --mcp-config path"
        );
        drop(rel_guard);
        let _ = std::fs::remove_dir_all(&rel_dir);
        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn shim_owns_mcp_config_cleanup_across_reconnect_and_snapshot_reaper_avoids_races() {
        let short_id: String = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(8)
            .collect();
        let home_dir = std::env::temp_dir().join(format!("oc_mc_{short_id}"));
        let _ = std::fs::remove_dir_all(&home_dir);
        let shims_dir = home_dir.join("shims");
        let runtime_dir = home_dir.join("runtime");
        std::fs::create_dir_all(&shims_dir).expect("mkdir shims");
        let sock_path = shims_dir.join("e.sock");

        let mcp = OrbitMcpLaunchSpec {
            server_name: "orbit".into(),
            http_url: "http://127.0.0.1:6868/mcp/agents?caller_agent_id=ag-exit-1".into(),
            bearer_token: "exit-cleanup-secret-token".into(),
            stdio_command: PathBuf::from("/usr/local/bin/orbit"),
            stdio_args: vec!["mcp".into()],
        };
        let live_cfg_path = write_claude_mcp_config_file(&runtime_dir, "ag-exit-1", &mcp)
            .expect("write mcp config")
            .into_path()
            .expect("path");
        // Sibling generation file for the same agent_id MUST NOT be deleted by this shim.
        let sibling_gen_path = write_claude_mcp_config_file(&runtime_dir, "ag-exit-1", &mcp)
            .expect("write sibling gen config")
            .into_path()
            .expect("sibling path");
        assert!(live_cfg_path.exists());
        assert!(sibling_gen_path.exists());

        // Run a real in-process `orbit_shim::run_shim` hosting `/bin/cat` with `cleanup_paths`.
        let shim_spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "claude_ag-exit-1".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: home_dir.clone(),
            env: HashMap::new(),
            cleanup_paths: vec![live_cfg_path.clone()],
        };
        let sock_bg = sock_path.clone();
        let shim_task = tokio::spawn(async move {
            orbit_shim::run_shim(&sock_bg, shim_spec)
                .await
                .expect("run shim");
        });
        for _ in 0..50 {
            if sock_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // 1. Connect, disconnect (simulating daemon hot-restart), verify file stays intact.
        {
            let client1 = orbit_shim::ShimClient::connect(&sock_path)
                .await
                .expect("connect 1");
            drop(client1);
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(
            live_cfg_path.exists(),
            "MCP config file must survive daemon disconnect/reattach while shim child is alive"
        );

        // 2. Reattach via `run_claude_actor` (is_reattached = true), send Shutdown -> shim kills
        //    child and unlinks ONLY `live_cfg_path` after the child exits!
        let reattached = orbit_shim::connect_existing_shim_after(&sock_path, Some(0))
            .await
            .expect("reattach");
        let (cmd_tx, cmd_rx) = mpsc::channel(8);
        let (evt_tx, mut evt_rx) = mpsc::channel(16);
        let mut cfg = sample_cfg(false);
        cfg.agent_id = "ag-exit-1".into();
        cfg.shim_socket = sock_path.clone();
        cfg.runtime_dir = runtime_dir.clone();

        let actor = tokio::spawn(run_claude_actor(
            cfg,
            "sess-exit-1".into(),
            reattached,
            true,
            false,
            false,
            None,
            cmd_rx,
            evt_tx,
        ));
        cmd_tx
            .send(DriverCommand::Shutdown)
            .await
            .expect("shutdown");
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        while evt_rx.try_recv().is_ok() {}

        assert!(
            !live_cfg_path.exists(),
            "shim must unlink its exact MCP config file after child exits (even after reattach)"
        );
        assert!(
            sibling_gen_path.exists(),
            "exit of one shim generation must NEVER delete another generation's MCP config file"
        );
        let _ = std::fs::remove_file(&sibling_gen_path);

        // 3. Verify snapshot-based startup reaper:
        //    - Keeps files for surviving/protected agents (including hyphenated UUID IDs)
        //    - Reaps pre-snapshotted stale files
        //    - NEVER deletes fresh files created AFTER the snapshot by newly launching recovery tasks!
        let keep_path = write_claude_mcp_config_file(
            &runtime_dir,
            "550e8400-e29b-41d4-a716-446655440000",
            &mcp,
        )
        .expect("write surviving config")
        .into_path()
        .expect("keep path");
        let stale_path = write_claude_mcp_config_file(&runtime_dir, "ag_dead_99", &mcp)
            .expect("write stale")
            .into_path()
            .expect("stale path");

        let candidates = snapshot_claude_mcp_config_files(&runtime_dir);

        // Simulate a recovery task launching a brand-new session for `ag_dead_99` AFTER snapshot!
        let fresh_after_snapshot = write_claude_mcp_config_file(&runtime_dir, "ag_dead_99", &mcp)
            .expect("write fresh post-snapshot config")
            .into_path()
            .expect("fresh path");

        let mut surviving = HashSet::new();
        surviving.insert("550e8400-e29b-41d4-a716-446655440000".to_string());
        let reaped = reap_snapshotted_claude_mcp_configs(&candidates, &surviving);
        assert_eq!(reaped, 1);
        assert!(
            keep_path.exists(),
            "surviving agent MCP config must survive"
        );
        assert!(
            !stale_path.exists(),
            "pre-snapshotted stale MCP config must be reaped"
        );
        assert!(
            fresh_after_snapshot.exists(),
            "fresh config created after startup snapshot must NEVER be deleted by reaper"
        );

        let _ = std::fs::remove_dir_all(&home_dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn claude_queue_steer_and_interrupt_and_drain_queue_delivery_lifecycle() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "o_cld_deliv_{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        type SeenPrompts = Arc<tokio::sync::Mutex<Vec<(String, Option<String>)>>>;
        let prompts_seen: SeenPrompts = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let prompts_bg = Arc::clone(&prompts_seen);
        let interrupt_cancel_queued: Arc<tokio::sync::Mutex<Vec<Option<bool>>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let int_bg = Arc::clone(&interrupt_cancel_queued);
        let (release_t1_tx, mut release_t1_rx) = mpsc::channel::<()>(2);

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

            let mut holding_slow = false;
            loop {
                tokio::select! {
                    _ = release_t1_rx.recv() => {
                        if holding_slow {
                            holding_slow = false;
                            send_stdout(
                                &mut wh,
                                &mut seq,
                                json!({
                                    "type": "result",
                                    "subtype": "success",
                                    "is_error": false,
                                    "result": "slow finished"
                                }),
                            )
                            .await;
                        }
                    }
                    maybe_line = lines.next_line() => {
                        let Ok(Some(raw)) = maybe_line else { break; };
                        let Ok(ShimFrame::WriteStdin { line }) = serde_json::from_str::<ShimFrame>(&raw) else {
                            continue;
                        };
                        let Ok(val) = serde_json::from_str::<Value>(&line) else { continue; };
                        let ty = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match ty {
                            "user" => {
                                let text = val
                                    .pointer("/message/content/0/text")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let priority = val
                                    .get("priority")
                                    .and_then(|v| v.as_str())
                                    .map(ToOwned::to_owned);
                                prompts_bg.lock().await.push((text.clone(), priority.clone()));
                                if priority.as_deref() == Some("next") {
                                    // Native mid-turn steer doesn't terminate active turn by itself
                                } else if text.starts_with("SLOW_") {
                                    holding_slow = true;
                                } else {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "type": "result",
                                            "subtype": "success",
                                            "is_error": false,
                                            "result": format!("ok:{text}")
                                        }),
                                    )
                                    .await;
                                }
                            }
                            "control_request"
                                if val.pointer("/request/subtype").and_then(|v| v.as_str())
                                    == Some("interrupt") =>
                            {
                                let cq = val
                                    .pointer("/request/cancel_queued")
                                    .and_then(|v| v.as_bool());
                                int_bg.lock().await.push(cq);
                                let rid = val["request_id"].clone();
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "type": "control_response",
                                        "response": { "subtype": "success", "request_id": rid }
                                    }),
                                )
                                .await;
                                if holding_slow {
                                    holding_slow = false;
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({
                                            "type": "result",
                                            "subtype": "success",
                                            "is_error": false,
                                            "result": "interrupted"
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
        cfg.agent_id = "ag_cld_deliv".into();
        cfg.shim_socket = sock_path.clone();
        let actor = tokio::spawn(run_claude_actor(
            cfg,
            "sess_cld_deliv".into(),
            shim,
            false,
            false,
            false,
            None,
            cmd_rx,
            evt_tx,
        ));

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

        // 1. Slow turn -> Queue (0 interrupts) + native Steer (`priority: "next"`, 0 interrupts)
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
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_q1".into(),
                prompt: "Q1".into(),
                behavior: ActiveTurnBehavior::Queue,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_q1".into()],
            })
            .await
            .expect("q1");
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "t_s1".into(),
                prompt: "STEER_1".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Agent,
                message_ids: vec!["msg_s1".into()],
            })
            .await
            .expect("s1");
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(interrupt_cancel_queued.lock().await.is_empty());

        release_t1_tx.send(()).await.expect("release");

        let mut q1_states = Vec::new();
        let mut s1_states = Vec::new();
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
                } => {
                    if message_ids == vec!["msg_q1".to_string()] {
                        q1_states.push(status);
                    } else if message_ids == vec!["msg_s1".to_string()] {
                        s1_states.push(status);
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle)
                    if q1_states.contains(&AgentMessageDeliveryStatus::Settled) =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(
            s1_states,
            vec![
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );
        assert_eq!(
            q1_states,
            vec![
                AgentMessageDeliveryStatus::Queued,
                AgentMessageDeliveryStatus::Dispatched,
                AgentMessageDeliveryStatus::Settled,
            ]
        );

        // 2. InterruptAndDrainQueue sends cancel_queued=false and preserves FIFO
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
        tokio::time::sleep(Duration::from_millis(40)).await;
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
                .expect("idle 2")
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
        assert_eq!(*interrupt_cancel_queued.lock().await, vec![Some(false)]);
        assert_eq!(
            *prompts_seen.lock().await,
            vec![
                ("SLOW_1".into(), None),
                ("STEER_1".into(), Some("next".into())),
                ("Q1".into(), None),
                ("SLOW_2".into(), None),
                ("Q2".into(), None),
                ("Q3".into(), None),
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn classify_claude_background_task_types_separates_timers_and_monitors_from_subagents() {
        assert_eq!(
            classify_claude_background_task_type("local_bash"),
            Some(ClaudeBgTaskKind::Shell)
        );
        for timer_type in [
            "local_timer",
            "timer",
            "cron",
            "schedule",
            "monitor",
            "local_monitor",
        ] {
            assert_eq!(
                classify_claude_background_task_type(timer_type),
                Some(ClaudeBgTaskKind::Timer),
                "{timer_type} must not be classified as a running subagent"
            );
        }
        assert_eq!(classify_claude_background_task_type("local_agent"), None);
        assert_eq!(classify_claude_background_task_type("local_workflow"), None);
    }
}
