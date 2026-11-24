//! Native Rust driver for Pi (`pi --mode rpc`) over `orbit shim` UDS.
//!
//! Runs Pi's headless JSONL RPC protocol inside a detached `orbit shim` process so
//! `orbit daemon` restarts never interrupt active Pi coding sessions.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageDeliveryStatus, AgentStatus, ProviderKind,
    ProviderModelCatalog, ProviderModelSpec, QuestionItem, ReasoningEffort, SessionMode,
    ThinkingOptionSpec, ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after,
    spawn_detached_shim,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::attachments::build_pi_prompt_payload_async;
use crate::driver::{
    DriverCommand, DriverEvent, OrbitMcpLaunchSpec, ProviderSessionHandle, QueuedDriverPrompt,
    SessionLaunchConfig, emit_dropped_queued_prompts, emit_prompt_delivery,
};

const BOOT_GET_STATE_TIMEOUT: Duration = Duration::from_secs(15);
const SHIM_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(2);
const PI_MCP_FILE_PREFIX: &str = "mcp-pi-";
const PI_MCP_FILE_SUFFIX: &str = ".json";

/// RAII guard for a per-spawn private Pi MCP config file (`0600` on Unix).
/// Automatically unlinks the file if shim launch fails before ownership transfers
/// to `orbit-shim` (`ShimSpawnSpec::cleanup_paths`).
struct PiMcpConfigGuard {
    path: Option<PathBuf>,
}

impl PiMcpConfigGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Disarm automatic cleanup once `orbit-shim` owns `cleanup_paths` for the child's lifetime.
    fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for PiMcpConfigGuard {
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

fn sanitize_pi_mcp_agent_id(agent_id: &str) -> String {
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

fn extract_pi_mcp_config_agent_id(file_name: &str) -> Option<&str> {
    let stem = file_name
        .strip_prefix(PI_MCP_FILE_PREFIX)?
        .strip_suffix(PI_MCP_FILE_SUFFIX)?;
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

/// Write a private MCP config JSON file under `<runtime_dir>/mcp-pi-<agent_id>-<uuid>.json`
/// so the Orbit MCP Bearer token is never exposed on `pi`'s process `argv`.
///
/// - On Unix, `<runtime_dir>` is created/verified as a real, current-UID-owned `0700` directory
///   and the file is opened directly via `create_new(true)` with mode `0600` before writing.
/// - Always returns an absolute path so relative `--home` flags work regardless of `pi`'s `cwd`.
fn write_pi_mcp_config_file(
    runtime_dir: &Path,
    agent_id: &str,
    mcp: &OrbitMcpLaunchSpec,
) -> Result<PiMcpConfigGuard, String> {
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

    let safe_id = sanitize_pi_mcp_agent_id(agent_id);
    let uuid = Uuid::new_v4();
    let final_path = abs_runtime_dir.join(format!(
        "{PI_MCP_FILE_PREFIX}{safe_id}-{uuid}{PI_MCP_FILE_SUFFIX}"
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
        .map_err(|e| format!("failed to create private pi MCP config file: {e}"))?;
    let guard = PiMcpConfigGuard::new(final_path);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to enforce 0600 on pi MCP config file: {e}"))?;
    }
    file.write_all(mcp_json.as_bytes())
        .map_err(|e| format!("failed to write private pi MCP config file: {e}"))?;
    drop(file);

    Ok(guard)
}

/// Snapshot existing `mcp-pi-<agent_id>-<uuid>.json` files inside `runtime_dir` BEFORE startup
/// recovery spawns any new agent tasks.
pub fn snapshot_pi_mcp_config_files(runtime_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(runtime_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if let Some(name_str) = name.to_str()
            && extract_pi_mcp_config_agent_id(name_str).is_some()
        {
            out.push(entry.path());
        }
    }
    out
}

/// Remove ONLY pre-snapshotted `mcp-pi-<agent_id>-<uuid>.json` candidate files whose agent did
/// not survive daemon restart (never touching fresh files written by newly launching agents).
pub fn reap_snapshotted_pi_mcp_configs(
    candidates: &[PathBuf],
    surviving_agent_ids: &HashSet<String>,
) -> usize {
    let surviving_sanitized: HashSet<String> = surviving_agent_ids
        .iter()
        .map(|id| sanitize_pi_mcp_agent_id(id))
        .collect();
    let mut removed = 0usize;
    for path in candidates {
        let Some(name_str) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(file_agent_id) = extract_pi_mcp_config_agent_id(name_str)
            && !surviving_sanitized.contains(file_agent_id)
            && std::fs::remove_file(path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

/// Instant fallback model catalog for Pi (no subprocess startup delay).
pub fn fallback_pi_models() -> ProviderModelCatalog {
    let pi_efforts = |def: ReasoningEffort| -> Vec<ThinkingOptionSpec> {
        [
            (ReasoningEffort::Off, "Off", "Thinking disabled"),
            (ReasoningEffort::Low, "Low", "Fast & concise"),
            (ReasoningEffort::Medium, "Medium", "Balanced speed & depth"),
            (ReasoningEffort::High, "High", "Greater reasoning depth"),
            (
                ReasoningEffort::Xhigh,
                "Extra High",
                "Maximum Pi thinking budget",
            ),
        ]
        .into_iter()
        .map(|(effort, label, desc)| ThinkingOptionSpec {
            effort,
            label: label.to_string(),
            description: Some(desc.to_string()),
            is_default: effort == def,
        })
        .collect()
    };

    ProviderModelCatalog {
        provider: ProviderKind::Pi,
        configured_default_model: Some("default".to_string()),
        configured_default_effort: Some(ReasoningEffort::Medium),
        models: vec![
            ProviderModelSpec {
                id: "default".to_string(),
                label: "Pi Default".to_string(),
                description: "Active model configured in Pi CLI".to_string(),
                is_default: true,
                default_effort: Some(ReasoningEffort::Medium),
                thinking_options: pi_efforts(ReasoningEffort::Medium),
                supports_fast_mode: false,
            },
            ProviderModelSpec {
                id: "anthropic/claude-sonnet-4-5".to_string(),
                label: "Claude Sonnet 4.5".to_string(),
                description: "Anthropic Sonnet via Pi".to_string(),
                is_default: false,
                default_effort: Some(ReasoningEffort::Medium),
                thinking_options: pi_efforts(ReasoningEffort::Medium),
                supports_fast_mode: false,
            },
            ProviderModelSpec {
                id: "openai/gpt-5".to_string(),
                label: "GPT-5".to_string(),
                description: "OpenAI flagship via Pi".to_string(),
                is_default: false,
                default_effort: Some(ReasoningEffort::Medium),
                thinking_options: pi_efforts(ReasoningEffort::Medium),
                supports_fast_mode: false,
            },
        ],
    }
}

pub fn pi_thinking_level(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh | ReasoningEffort::Max | ReasoningEffort::Ultra => "xhigh",
    }
}

fn build_pi_spawn_spec(
    cfg: &SessionLaunchConfig,
    include_resume: bool,
    mcp_config_path: Option<&Path>,
) -> ShimSpawnSpec {
    let mut args = vec!["--mode".to_string(), "rpc".to_string()];

    if include_resume
        && let Some(sid) = cfg
            .resume_session_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        args.push("--session".to_string());
        args.push(sid.to_string());
    }

    if let Some(model) = cfg
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("default"))
    {
        args.push("--model".to_string());
        args.push(model.to_string());
    }

    if let Some(eff) = cfg.effort {
        args.push("--thinking".to_string());
        args.push(pi_thinking_level(eff).to_string());
    }

    if let Some(append) = cfg
        .system_prompt_append
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("--append-system-prompt".to_string());
        args.push(append.to_string());
    }

    let mut env = cfg.extra_env.clone();
    env.insert("NO_COLOR".into(), "1".into());
    crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);

    let mut cleanup_paths = Vec::new();
    if let Some(path) = mcp_config_path {
        let path_str = path.to_string_lossy().to_string();
        // Expose only the private 0600 file path via environment (never pass secrets in argv
        // and never inject unverified CLI flags into `pi --mode rpc`).
        env.insert("ORBIT_MCP_CONFIG".into(), path_str.clone());
        env.insert("PI_MCP_CONFIG".into(), path_str);
        cleanup_paths.push(path.to_path_buf());
    }

    ShimSpawnSpec {
        shim_id: format!("pi_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args,
        cwd: cfg.cwd.clone(),
        env,
        cleanup_paths,
    }
}

pub async fn spawn_pi_session(cfg: SessionLaunchConfig) -> Result<ProviderSessionHandle, String> {
    if cfg.native_fork_only {
        return Err(
            "pi does not support native session fork; deferring to handoff on first prompt".into(),
        );
    }
    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let (shim, is_reattached) = if let Some(existing) = reattached {
        (existing, true)
    } else {
        let mut mcp_guard = match cfg.mcp_spec {
            Some(ref mcp) => Some(write_pi_mcp_config_file(
                &cfg.runtime_dir,
                &cfg.agent_id,
                mcp,
            )?),
            None => None,
        };
        let spec = build_pi_spawn_spec(&cfg, true, mcp_guard.as_ref().and_then(|g| g.path()));
        let fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
            .await
            .map_err(|e| format!("failed to spawn pi shim: {e}"))?;
        if let Some(ref mut guard) = mcp_guard {
            guard.disarm();
        }
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_pi_actor(cfg, shim, is_reattached, cmd_rx, evt_tx));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

/// Kill the active shim process and wait (up to 2s) for it to exit and release its socket
/// so respawning on the same `shim_socket` never races with the old shim's socket unlink guard.
async fn kill_and_wait_shim(shim: &mut ShimClient) {
    let _ = shim.kill().await;
    let deadline = tokio::time::Instant::now() + SHIM_KILL_WAIT_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, shim.rx.recv()).await {
            Ok(Some(ShimFrame::Exited { .. })) | Ok(None) | Err(_) => break,
            Ok(Some(_)) => {}
        }
    }
}

#[derive(Default)]
struct PiStreamSegmentState {
    turn_tag: String,
    step_seq: u64,
    active_msg_id: Option<String>,
    active_thought_id: Option<String>,
    msg_accum: HashMap<String, String>,
    thought_accum: HashMap<String, String>,
    emitted_text_in_message: bool,
}

impl PiStreamSegmentState {
    fn reset_for_turn(&mut self, turn_id: &str) {
        self.turn_tag = turn_id.to_string();
        self.step_seq = 0;
        self.active_msg_id = None;
        self.active_thought_id = None;
        self.msg_accum.clear();
        self.thought_accum.clear();
        self.emitted_text_in_message = false;
    }

    fn clear_active_thought(&mut self) {
        if let Some(tid) = self.active_thought_id.take() {
            self.thought_accum.remove(&tid);
        }
    }

    async fn finalize_active_message(&mut self, evt_tx: &mpsc::Sender<DriverEvent>) {
        if let Some(mid) = self.active_msg_id.take()
            && let Some(text) = self.msg_accum.remove(&mid)
        {
            let _ = evt_tx
                .send(DriverEvent::AssistantText {
                    item_id: mid,
                    text,
                    streaming: false,
                })
                .await;
        }
    }

    async fn finalize_all_messages(&mut self, evt_tx: &mpsc::Sender<DriverEvent>) {
        self.active_msg_id = None;
        self.active_thought_id = None;
        self.thought_accum.clear();
        for (mid, text) in self.msg_accum.drain() {
            let _ = evt_tx
                .send(DriverEvent::AssistantText {
                    item_id: mid,
                    text,
                    streaming: false,
                })
                .await;
        }
    }
}

struct PendingExtensionRequest {
    rpc_id: String,
    method: String,
}

struct PiBootState {
    persistence_ref: String,
    model: Option<String>,
    active_provider: Option<String>,
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

fn extract_pi_state_from_data(data: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let session_ref = data
        .get("sessionFile")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            data.get("sessionId")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
        })
        .map(ToOwned::to_owned);

    let mut active_provider: Option<String> = None;
    let model = data.get("model").and_then(|m| {
        if let Some(s) = m.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            if let Some((prov, _)) = s.split_once('/') {
                active_provider = Some(prov.trim().to_string());
            }
            return Some(s.to_string());
        }
        let id = m.get("id").and_then(Value::as_str)?.trim();
        if id.is_empty() {
            return None;
        }
        if let Some(prov) = m
            .get("provider")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            active_provider = Some(prov.to_string());
            if id.contains('/') {
                Some(id.to_string())
            } else {
                Some(format!("{prov}/{id}"))
            }
        } else {
            if let Some((prov, _)) = id.split_once('/') {
                active_provider = Some(prov.trim().to_string());
            }
            Some(id.to_string())
        }
    });

    (session_ref, model, active_provider)
}

async fn boot_pi_get_state(
    cfg: &SessionLaunchConfig,
    shim: &mut ShimClient,
    next_cmd_id: &mut u64,
) -> Result<PiBootState, String> {
    let req_id = format!("pi_cmd_{}", *next_cmd_id);
    *next_cmd_id += 1;

    let cmd = json!({
        "id": req_id,
        "type": "get_state"
    })
    .to_string();
    shim.write_stdin_line(cmd)
        .await
        .map_err(|e| format!("failed to send Pi get_state: {e}"))?;

    let deadline = tokio::time::Instant::now() + BOOT_GET_STATE_TIMEOUT;
    let mut stderr_tail = String::new();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err("Pi RPC get_state timed out".to_string());
        }
        match tokio::time::timeout(remaining, shim.rx.recv()).await {
            Ok(Some(frame)) => {
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if val.get("type").and_then(Value::as_str) == Some("response")
                            && (val.get("id").and_then(Value::as_str) == Some(req_id.as_str())
                                || val.get("command").and_then(Value::as_str) == Some("get_state"))
                        {
                            let ok = val.get("success").and_then(Value::as_bool).unwrap_or(true);
                            if !ok {
                                let err_msg = val
                                    .get("error")
                                    .and_then(Value::as_str)
                                    .unwrap_or("Pi get_state failed");
                                return Err(err_msg.to_string());
                            }
                            let data = val.get("data").unwrap_or(&Value::Null);
                            let (session_ref, model, active_provider) =
                                extract_pi_state_from_data(data);
                            let persistence_ref = session_ref
                                .or_else(|| cfg.resume_session_id.clone())
                                .unwrap_or_else(|| format!("pi_{}", cfg.agent_id));
                            let inferred_provider = active_provider.or_else(|| {
                                cfg.model
                                    .as_deref()
                                    .and_then(|m| m.split_once('/'))
                                    .map(|(p, _)| p.trim().to_string())
                                    .filter(|p| !p.is_empty())
                            });
                            return Ok(PiBootState {
                                persistence_ref,
                                model: model.or_else(|| cfg.model.clone()),
                                active_provider: inferred_provider,
                            });
                        }
                    }
                    ShimFrame::StderrLine { line, .. } => {
                        if !line.trim().is_empty() && stderr_tail.len() < 1024 {
                            if !stderr_tail.is_empty() {
                                stderr_tail.push('\n');
                            }
                            stderr_tail.push_str(line.trim());
                        }
                    }
                    ShimFrame::Exited { code } => {
                        return Err(if stderr_tail.is_empty() {
                            format!("Pi exited early during boot (code: {code:?})")
                        } else {
                            format!("Pi exited early during boot (code: {code:?}): {stderr_tail}")
                        });
                    }
                    _ => {}
                }
            }
            Ok(None) => return Err("Pi shim channel closed during boot".to_string()),
            Err(_) => return Err("Pi RPC get_state timed out".to_string()),
        }
    }
}

async fn build_pi_prompt_command_async(
    cmd_id: &str,
    cmd_type: &str,
    prompt: &str,
    attachments_dir: Option<&Path>,
) -> String {
    let (enriched, images) =
        build_pi_prompt_payload_async(prompt.to_string(), attachments_dir.map(Path::to_path_buf))
            .await;
    let mut obj = json!({
        "id": cmd_id,
        "type": cmd_type,
        "message": enriched,
    });
    if !images.is_empty() {
        obj["images"] = Value::Array(images);
    }
    obj.to_string()
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

fn extract_pi_result_text(result: Option<&Value>) -> Option<String> {
    let val = result?;
    if let Some(s) = val.as_str() {
        return (!s.trim().is_empty()).then(|| truncate_preview(s, 800));
    }
    if let Some(arr) = val.get("content").and_then(Value::as_array) {
        let mut combined = String::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(t);
            }
        }
        if !combined.trim().is_empty() {
            return Some(truncate_preview(&combined, 800));
        }
    }
    val.get("output")
        .or_else(|| val.get("stdout"))
        .or_else(|| val.get("text"))
        .or_else(|| val.get("error"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(|s| truncate_preview(s, 800))
}

fn map_pi_tool_detail(
    tool_name: &str,
    args: Option<&Value>,
    result: Option<&Value>,
    prev: Option<&ToolCallDetail>,
) -> ToolCallDetail {
    if crate::managed_dispatch::is_launch(tool_name)
        || matches!(prev, Some(ToolCallDetail::Subagent { .. }))
    {
        return crate::managed_dispatch::detail(
            args.unwrap_or(&Value::Null),
            result.unwrap_or(&Value::Null),
            prev,
        );
    }
    let lower = tool_name.trim().to_ascii_lowercase();
    let out_preview = extract_pi_result_text(result);

    if matches!(
        lower.as_str(),
        "bash" | "shell" | "exec" | "run_command" | "command"
    ) {
        let cmd = args
            .and_then(|a| a.get("command").or_else(|| a.get("cmd")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { command, .. }) => Some(command.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tool_name.to_string());
        let cwd = args
            .and_then(|a| a.get("cwd").or_else(|| a.get("workdir")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { cwd, .. }) => cwd.clone(),
                _ => None,
            });
        let exit_code = result
            .and_then(|r| {
                r.get("exitCode")
                    .or_else(|| r.get("exit_code"))
                    .or_else(|| r.pointer("/details/exitCode"))
            })
            .and_then(Value::as_i64)
            .and_then(|c| i32::try_from(c).ok());
        let prev_preview = match prev {
            Some(ToolCallDetail::Shell { output_preview, .. }) => output_preview.clone(),
            _ => None,
        };
        return ToolCallDetail::Shell {
            command: cmd,
            cwd,
            exit_code,
            output_preview: out_preview.or(prev_preview),
        };
    }

    if matches!(lower.as_str(), "read" | "read_file" | "view_file" | "cat") {
        let path = args
            .and_then(|a| a.get("path").or_else(|| a.get("file_path")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::FileRead { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tool_name.to_string());
        let start_line = args
            .and_then(|a| a.get("offset").or_else(|| a.get("start_line")))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .or(match prev {
                Some(ToolCallDetail::FileRead { start_line, .. }) => *start_line,
                _ => None,
            });
        let end_line = args
            .and_then(|a| a.get("end_line"))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
            .or_else(|| {
                let limit = args
                    .and_then(|a| a.get("limit"))
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|&l| l > 0)?;
                let base = start_line.unwrap_or(1);
                Some(base.saturating_add(limit).saturating_sub(1))
            })
            .or(match prev {
                Some(ToolCallDetail::FileRead { end_line, .. }) => *end_line,
                _ => None,
            });
        return ToolCallDetail::FileRead {
            path,
            start_line,
            end_line,
        };
    }

    if matches!(
        lower.as_str(),
        "edit" | "write" | "write_file" | "edit_file" | "patch" | "str_replace"
    ) {
        let path = args
            .and_then(|a| a.get("path").or_else(|| a.get("file_path")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::FileEdit { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tool_name.to_string());

        let diff = result
            .and_then(|r| {
                r.pointer("/details/diff")
                    .or_else(|| r.get("diff"))
                    .and_then(Value::as_str)
            })
            .map(|d| crate::compact_unified_diff(d, crate::MAX_UNIFIED_DIFF_BYTES))
            .or_else(|| {
                let old_t = args
                    .and_then(|a| a.get("oldText").or_else(|| a.get("old_string")))
                    .and_then(Value::as_str);
                let new_t = args
                    .and_then(|a| {
                        a.get("newText")
                            .or_else(|| a.get("new_string"))
                            .or_else(|| a.get("content"))
                    })
                    .and_then(Value::as_str);
                match (old_t, new_t) {
                    (Some(o), Some(n)) => crate::build_compact_unified_diff(&path, o, n),
                    (None, Some(n)) if lower.starts_with("write") => {
                        crate::build_compact_unified_diff(&path, "", n)
                    }
                    _ => None,
                }
            })
            .or_else(|| match prev {
                Some(ToolCallDetail::FileEdit { unified_diff, .. }) => unified_diff.clone(),
                _ => None,
            });

        return ToolCallDetail::FileEdit {
            path,
            unified_diff: diff,
        };
    }

    if matches!(lower.as_str(), "grep" | "find" | "search" | "glob" | "ls") {
        let query = args
            .and_then(|a| {
                a.get("pattern")
                    .or_else(|| a.get("query"))
                    .or_else(|| a.get("glob"))
            })
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Search { query, .. }) => Some(query.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tool_name.to_string());
        let scope = args
            .and_then(|a| a.get("path").or_else(|| a.get("dir")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        return ToolCallDetail::Search { query, scope };
    }

    if let Some(existing) = prev {
        return existing.clone();
    }

    ToolCallDetail::Generic {
        tool_name: tool_name.to_string(),
        arguments_json: args.map(Value::to_string).unwrap_or_default(),
    }
}

fn extract_assistant_message_text(msg: &Value) -> Option<String> {
    if let Some(s) = msg.get("content").and_then(Value::as_str)
        && !s.is_empty()
    {
        return Some(s.to_string());
    }
    let blocks = msg.get("content").and_then(Value::as_array)?;
    let mut combined = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("text")
            && let Some(t) = block.get("text").and_then(Value::as_str)
        {
            combined.push_str(t);
        }
    }
    (!combined.is_empty()).then_some(combined)
}

async fn cancel_pending_ext_reqs(
    shim: &ShimClient,
    pending_ext_reqs: &mut HashMap<String, PendingExtensionRequest>,
) {
    for (_, req) in pending_ext_reqs.drain() {
        let cancel = json!({
            "type": "extension_ui_response",
            "id": req.rpc_id,
            "cancelled": true
        })
        .to_string();
        let _ = shim.write_stdin_line(cancel).await;
    }
}

async fn emit_status_after_ext_resolve(
    active_turn_id: &Option<String>,
    pending_ext_reqs: &HashMap<String, PendingExtensionRequest>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    if let Some(next_req_id) = pending_ext_reqs.keys().next().cloned() {
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

async fn finalize_and_emit_unexpected_exit(
    seg_state: &mut PiStreamSegmentState,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    seg_state.finalize_all_messages(evt_tx).await;
    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
}

async fn run_pi_actor(
    cfg: SessionLaunchConfig,
    mut shim: ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let mut next_cmd_id: u64 = if is_reattached { 100_000 } else { 1 };
    let mut inject_handoff_on_next_prompt = false;
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();

    let (persistence_ref, current_model, mut active_pi_provider) = if is_reattached {
        let inferred_provider = cfg
            .model
            .as_deref()
            .and_then(|m| m.split_once('/'))
            .map(|(p, _)| p.trim().to_string())
            .filter(|p| !p.is_empty());
        (
            cfg.resume_session_id
                .clone()
                .unwrap_or_else(|| format!("pi_{}", cfg.agent_id)),
            cfg.model.clone(),
            inferred_provider,
        )
    } else {
        match boot_pi_get_state(&cfg, &mut shim, &mut next_cmd_id).await {
            Ok(boot) => (boot.persistence_ref, boot.model, boot.active_provider),
            Err(first_err) if cfg.resume_session_id.is_some() => {
                warn!(
                    agent_id = %cfg.agent_id,
                    resume_session_id = ?cfg.resume_session_id,
                    error = %first_err,
                    "Pi session resume failed; restarting fresh Pi session with handoff context"
                );
                kill_and_wait_shim(&mut shim).await;
                cfg.shim_seq_tracker.store(0, Ordering::Relaxed);
                let mut fresh_mcp_guard = match cfg.mcp_spec {
                    Some(ref mcp) => {
                        match write_pi_mcp_config_file(&cfg.runtime_dir, &cfg.agent_id, mcp) {
                            Ok(guard) => Some(guard),
                            Err(e) => {
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                        message: format!(
                                            "Pi startup failed: {first_err} (fresh MCP config failed: {e})"
                                        ),
                                        recoverable: false,
                                    }))
                                    .await;
                                return;
                            }
                        }
                    }
                    None => None,
                };
                let fresh_spec = build_pi_spawn_spec(
                    &cfg,
                    false,
                    fresh_mcp_guard.as_ref().and_then(|g| g.path()),
                );
                match spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &fresh_spec).await {
                    Ok(mut fresh_shim) => {
                        if let Some(ref mut guard) = fresh_mcp_guard {
                            guard.disarm();
                        }
                        match boot_pi_get_state(&cfg, &mut fresh_shim, &mut next_cmd_id).await {
                            Ok(boot) => {
                                shim = fresh_shim;
                                inject_handoff_on_next_prompt = true;
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "info".into(),
                                        message: "Previous Pi session could not be resumed; started a fresh session with recent conversation context.".into(),
                                    })
                                    .await;
                                (boot.persistence_ref, boot.model, boot.active_provider)
                            }
                            Err(e) => {
                                kill_and_wait_shim(&mut fresh_shim).await;
                                let _ = evt_tx
                                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                        message: format!(
                                            "Pi startup failed: {first_err} (fresh fallback: {e})"
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
                                message: format!(
                                    "Pi startup failed: {first_err} (fresh shim failed: {e})"
                                ),
                                recoverable: false,
                            }))
                            .await;
                        return;
                    }
                }
            }
            Err(err) => {
                warn!(agent_id = %cfg.agent_id, error = %err, "Pi RPC boot failed");
                kill_and_wait_shim(&mut shim).await;
                let _ = evt_tx
                    .send(DriverEvent::StatusChanged(AgentStatus::Error {
                        message: format!("Pi RPC boot failed: {err}"),
                        recoverable: false,
                    }))
                    .await;
                return;
            }
        }
    };

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref,
            model: current_model,
        })
        .await;
    if !is_reattached {
        let _ = evt_tx
            .send(DriverEvent::StatusChanged(AgentStatus::Idle))
            .await;
    }

    let mut mode = cfg.mode.clone();
    let mut active_turn_id: Option<String> = if is_reattached {
        cfg.persisted_active_turn_id.clone()
    } else {
        None
    };
    let mut seg_state = if let Some(ref tid) = active_turn_id {
        PiStreamSegmentState {
            turn_tag: tid.clone(),
            step_seq: cfg.persisted_turn_step_seq,
            ..PiStreamSegmentState::default()
        }
    } else {
        PiStreamSegmentState::default()
    };
    let mut tool_cache: HashMap<String, ToolCallDetail> = HashMap::new();
    let mut pending_prompts: VecDeque<QueuedDriverPrompt> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<QueuedDriverPrompt> = None;
    let mut active_turn_message_ids: Vec<String> = Vec::new();
    let mut pending_ext_reqs: HashMap<String, PendingExtensionRequest> = HashMap::new();

    loop {
        tokio::select! {
            maybe_cmd = cmd_rx.recv() => {
                let Some(cmd) = maybe_cmd else { break; };
                match cmd {
                    DriverCommand::SendPrompt {
                        turn_id,
                        prompt,
                        behavior,
                        origin: _origin,
                        message_ids,
                    } => {
                        let effective_prompt = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };

                        if let Some(ref blocking_tid) = active_turn_id {
                            let blocking_tid_opt = Some(blocking_tid.clone());
                            match behavior {
                                ActiveTurnBehavior::Steer => {
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let line = build_pi_prompt_command_async(
                                        &cmd_id,
                                        "steer",
                                        &effective_prompt,
                                        cfg.attachments_dir.as_deref(),
                                    )
                                    .await;
                                    if shim.write_stdin_line(line).await.is_err() {
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
                                            &mut pending_prompts,
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
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
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
                                }
                                ActiveTurnBehavior::Queue => {
                                    emit_prompt_delivery(
                                        &evt_tx,
                                        &turn_id,
                                        &message_ids,
                                        AgentMessageDeliveryStatus::Queued,
                                        blocking_tid_opt,
                                    )
                                    .await;
                                    pending_prompts.push_back(QueuedDriverPrompt {
                                        turn_id,
                                        prompt: effective_prompt,
                                        message_ids,
                                    });
                                }
                                ActiveTurnBehavior::Interrupt => {
                                    cancel_pending_ext_reqs(&shim, &mut pending_ext_reqs).await;
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let abort_line = json!({
                                        "id": cmd_id,
                                        "type": "abort"
                                    })
                                    .to_string();
                                    if shim.write_stdin_line(abort_line).await.is_err() {
                                        emit_prompt_delivery(
                                            &evt_tx,
                                            &turn_id,
                                            &message_ids,
                                            AgentMessageDeliveryStatus::Dropped,
                                            None,
                                        )
                                        .await;
                                        emit_dropped_queued_prompts(
                                            &evt_tx,
                                            pending_interrupt_replacement.take(),
                                            &mut pending_prompts,
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
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                            .await;
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
                                        blocking_tid_opt,
                                    )
                                    .await;
                                    pending_interrupt_replacement = Some(QueuedDriverPrompt {
                                        turn_id,
                                        prompt: effective_prompt,
                                        message_ids,
                                    });
                                }
                            }
                            continue;
                        }

                        active_turn_id = Some(turn_id.clone());
                        active_turn_message_ids = message_ids;
                        seg_state.reset_for_turn(&turn_id);
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                turn_id: turn_id.clone(),
                            }))
                            .await;
                        let cmd_id = format!("pi_cmd_{next_cmd_id}");
                        next_cmd_id += 1;
                        let line = build_pi_prompt_command_async(
                            &cmd_id,
                            "prompt",
                            &effective_prompt,
                            cfg.attachments_dir.as_deref(),
                        )
                        .await;
                        if shim.write_stdin_line(line).await.is_err() {
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
                                &mut pending_prompts,
                            )
                            .await;
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
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
                            &mut pending_prompts,
                        )
                        .await;
                        cancel_pending_ext_reqs(&shim, &mut pending_ext_reqs).await;
                        let cmd_id = format!("pi_cmd_{next_cmd_id}");
                        next_cmd_id += 1;
                        let abort_line = json!({
                            "id": cmd_id,
                            "type": "abort"
                        })
                        .to_string();
                        if shim.write_stdin_line(abort_line).await.is_err() {
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
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                            break;
                        }
                    }
                    DriverCommand::InterruptAndDrainQueue => {
                        cancel_pending_ext_reqs(&shim, &mut pending_ext_reqs).await;
                        if active_turn_id.is_some() {
                            let cmd_id = format!("pi_cmd_{next_cmd_id}");
                            next_cmd_id += 1;
                            let abort_line = json!({
                                "id": cmd_id,
                                "type": "abort"
                            })
                            .to_string();
                            if shim.write_stdin_line(abort_line).await.is_err() {
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
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                        } else if let Some(next_q) = pending_interrupt_replacement
                            .take()
                            .or_else(|| pending_prompts.pop_front())
                        {
                            active_turn_id = Some(next_q.turn_id.clone());
                            active_turn_message_ids = next_q.message_ids;
                            seg_state.reset_for_turn(&next_q.turn_id);
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                    turn_id: next_q.turn_id.clone(),
                                }))
                                .await;
                            let cmd_id = format!("pi_cmd_{next_cmd_id}");
                            next_cmd_id += 1;
                            let line = build_pi_prompt_command_async(
                                &cmd_id,
                                "prompt",
                                &next_q.prompt,
                                cfg.attachments_dir.as_deref(),
                            )
                            .await;
                            if shim.write_stdin_line(line).await.is_err() {
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
                                    &mut pending_prompts,
                                )
                                .await;
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
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
                        mode = new_mode;
                    }
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode: _,
                    } => {
                        if let Some(m) = model
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("default"))
                        {
                            let cmd_id = format!("pi_cmd_{next_cmd_id}");
                            next_cmd_id += 1;
                            let req = if let Some((prov, mid)) = m.split_once('/') {
                                let clean_prov = prov.trim();
                                if !clean_prov.is_empty() {
                                    active_pi_provider = Some(clean_prov.to_string());
                                }
                                json!({
                                    "id": cmd_id,
                                    "type": "set_model",
                                    "provider": clean_prov,
                                    "modelId": mid.trim(),
                                })
                            } else if let Some(ref cached_prov) = active_pi_provider {
                                json!({
                                    "id": cmd_id,
                                    "type": "set_model",
                                    "provider": cached_prov,
                                    "modelId": m,
                                })
                            } else {
                                json!({
                                    "id": cmd_id,
                                    "type": "set_model",
                                    "modelId": m,
                                })
                            };
                            if shim.write_stdin_line(req.to_string()).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                        }
                        if let Some(eff) = effort {
                            let cmd_id = format!("pi_cmd_{next_cmd_id}");
                            next_cmd_id += 1;
                            let req = json!({
                                "id": cmd_id,
                                "type": "set_thinking_level",
                                "level": pi_thinking_level(eff),
                            });
                            if shim.write_stdin_line(req.to_string()).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                        }
                    }
                    DriverCommand::ResolvePermission { request_id, decision } => {
                        if let Some(req) = pending_ext_reqs.remove(&request_id) {
                            let confirmed = matches!(decision.as_str(), "allow" | "allow_always");
                            let resp = json!({
                                "type": "extension_ui_response",
                                "id": req.rpc_id,
                                "confirmed": confirmed,
                            })
                            .to_string();
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                            emit_status_after_ext_resolve(
                                &active_turn_id,
                                &pending_ext_reqs,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    DriverCommand::ResolvePlanApproval {
                        request_id,
                        action,
                        ..
                    } => {
                        if let Some(req) = pending_ext_reqs.remove(&request_id) {
                            let confirmed = matches!(
                                action,
                                orbit_protocol::PlanApprovalAction::ApproveAndBuild
                            );
                            let resp = json!({
                                "type": "extension_ui_response",
                                "id": req.rpc_id,
                                "confirmed": confirmed,
                            })
                            .to_string();
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                            emit_status_after_ext_resolve(
                                &active_turn_id,
                                &pending_ext_reqs,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    DriverCommand::AnswerQuestion { request_id, answers } => {
                        if let Some(req) = pending_ext_reqs.remove(&request_id) {
                            let first = answers
                                .first()
                                .map(|v| v.join(", "))
                                .unwrap_or_default();
                            let resp = if req.method == "confirm" {
                                json!({
                                    "type": "extension_ui_response",
                                    "id": req.rpc_id,
                                    "confirmed": matches!(
                                        first.to_ascii_lowercase().as_str(),
                                        "yes" | "true" | "allow" | "ok"
                                    ),
                                })
                            } else {
                                json!({
                                    "type": "extension_ui_response",
                                    "id": req.rpc_id,
                                    "value": first,
                                })
                            };
                            if shim.write_stdin_line(resp.to_string()).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                            emit_status_after_ext_resolve(
                                &active_turn_id,
                                &pending_ext_reqs,
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
                    finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                    break;
                };
                if let Some(seq) = frame.seq() {
                    cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                }
                match frame {
                    ShimFrame::StdoutLine { line, .. } => {
                        let Ok(val) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        let ev_type = val.get("type").and_then(Value::as_str).unwrap_or("");
                        match ev_type {
                            "response" => {
                                let cmd_name =
                                    val.get("command").and_then(Value::as_str).unwrap_or("");
                                let ok =
                                    val.get("success").and_then(Value::as_bool).unwrap_or(true);
                                if !ok {
                                    let err_msg = val
                                        .get("error")
                                        .and_then(Value::as_str)
                                        .unwrap_or("Pi RPC command failed")
                                        .to_string();
                                    if cmd_name == "prompt" {
                                        seg_state.finalize_all_messages(&evt_tx).await;
                                        let failed_tid = active_turn_id.take();
                                        let dropped_ids =
                                            std::mem::take(&mut active_turn_message_ids);
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
                                        if let Some(next_q) = pending_interrupt_replacement
                                            .take()
                                            .or_else(|| pending_prompts.pop_front())
                                        {
                                            active_turn_id = Some(next_q.turn_id.clone());
                                            active_turn_message_ids = next_q.message_ids;
                                            seg_state.reset_for_turn(&next_q.turn_id);
                                            let _ = evt_tx
                                                .send(DriverEvent::StatusChanged(
                                                    AgentStatus::Running {
                                                        turn_id: next_q.turn_id.clone(),
                                                    },
                                                ))
                                                .await;
                                            let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                            next_cmd_id += 1;
                                            let line = build_pi_prompt_command_async(
                                                &cmd_id,
                                                "prompt",
                                                &next_q.prompt,
                                                cfg.attachments_dir.as_deref(),
                                            )
                                            .await;
                                            if shim.write_stdin_line(line).await.is_err() {
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
                                                    &mut pending_prompts,
                                                )
                                                .await;
                                                finalize_and_emit_unexpected_exit(
                                                    &mut seg_state,
                                                    &evt_tx,
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
                                                .send(DriverEvent::StatusChanged(
                                                    AgentStatus::Error {
                                                        message: err_msg,
                                                        recoverable: true,
                                                    },
                                                ))
                                                .await;
                                        }
                                    } else {
                                        debug!(
                                            agent_id = %cfg.agent_id,
                                            command = %cmd_name,
                                            error = %err_msg,
                                            "Pi RPC command returned error"
                                        );
                                        let _ = evt_tx
                                            .send(DriverEvent::Notice {
                                                level: "warning".into(),
                                                message: format!("Pi `{cmd_name}` failed: {err_msg}"),
                                            })
                                            .await;
                                    }
                                }
                            }
                            "agent_start" | "turn_start" => {
                                if active_turn_id.is_none() {
                                    let tid = format!("pi_turn_{}", uuid::Uuid::new_v4().simple());
                                    active_turn_id = Some(tid.clone());
                                    seg_state.reset_for_turn(&tid);
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: tid,
                                        }))
                                        .await;
                                }
                            }
                            "message_start" => {
                                if val.pointer("/message/role").and_then(Value::as_str)
                                    == Some("assistant")
                                {
                                    seg_state.emitted_text_in_message = false;
                                }
                            }
                            "message_update" => {
                                handle_pi_message_update(&val, &mut seg_state, &evt_tx).await;
                            }
                            "message_end" => {
                                if let Some(msg) = val.get("message")
                                    && msg.get("role").and_then(Value::as_str) == Some("assistant")
                                {
                                    if !seg_state.emitted_text_in_message
                                        && let Some(text) = extract_assistant_message_text(msg)
                                    {
                                        let turn_tag = if seg_state.turn_tag.is_empty() {
                                            "init".to_string()
                                        } else {
                                            seg_state.turn_tag.clone()
                                        };
                                        seg_state.step_seq += 1;
                                        let mid = format!("pi_msg_{turn_tag}_{}", seg_state.step_seq);
                                        let _ = evt_tx
                                            .send(DriverEvent::AssistantText {
                                                item_id: mid,
                                                text,
                                                streaming: false,
                                            })
                                            .await;
                                    } else {
                                        seg_state.finalize_active_message(&evt_tx).await;
                                    }
                                    seg_state.clear_active_thought();
                                    seg_state.emitted_text_in_message = false;
                                }
                            }
                            "tool_execution_start" => {
                                seg_state.finalize_active_message(&evt_tx).await;
                                seg_state.clear_active_thought();
                                let call_id = val
                                    .get("toolCallId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("pi_tool")
                                    .to_string();
                                let tool_name = val
                                    .get("toolName")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool");
                                let detail = map_pi_tool_detail(
                                    tool_name,
                                    val.get("args"),
                                    None,
                                    tool_cache.get(&call_id),
                                );
                                tool_cache.insert(call_id.clone(), detail.clone());
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id,
                                        status: ToolCallStatus::Running,
                                        detail,
                                        background_task: None,
                                    })
                                    .await;
                            }
                            "tool_execution_update" => {
                                let call_id = val
                                    .get("toolCallId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("pi_tool")
                                    .to_string();
                                let tool_name = val
                                    .get("toolName")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool");
                                let detail = map_pi_tool_detail(
                                    tool_name,
                                    val.get("args"),
                                    val.get("partialResult"),
                                    tool_cache.get(&call_id),
                                );
                                tool_cache.insert(call_id.clone(), detail.clone());
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id,
                                        status: ToolCallStatus::Running,
                                        detail,
                                        background_task: None,
                                    })
                                    .await;
                            }
                            "tool_execution_end" => {
                                let call_id = val
                                    .get("toolCallId")
                                    .and_then(Value::as_str)
                                    .unwrap_or("pi_tool")
                                    .to_string();
                                let tool_name = val
                                    .get("toolName")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool");
                                let is_error = val
                                    .get("isError")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false);
                                let prev = tool_cache.remove(&call_id);
                                let detail = map_pi_tool_detail(
                                    tool_name,
                                    val.get("args"),
                                    val.get("result"),
                                    prev.as_ref(),
                                );
                                let _ = evt_tx
                                    .send(DriverEvent::ToolCall {
                                        call_id,
                                        status: if is_error {
                                            ToolCallStatus::Failed
                                        } else {
                                            ToolCallStatus::Completed
                                        },
                                        detail,
                                        background_task: None,
                                    })
                                    .await;
                            }
                            "turn_end" => {
                                seg_state.finalize_active_message(&evt_tx).await;
                                seg_state.clear_active_thought();
                            }
                            "agent_end" => {
                                seg_state.finalize_all_messages(&evt_tx).await;
                                let finished_tid = active_turn_id.take();
                                let settled_ids = std::mem::take(&mut active_turn_message_ids);
                                pending_ext_reqs.clear();
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
                                if let Some(next_q) = pending_interrupt_replacement
                                    .take()
                                    .or_else(|| pending_prompts.pop_front())
                                {
                                    active_turn_id = Some(next_q.turn_id.clone());
                                    active_turn_message_ids = next_q.message_ids;
                                    seg_state.reset_for_turn(&next_q.turn_id);
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: next_q.turn_id.clone(),
                                        }))
                                        .await;
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let line = build_pi_prompt_command_async(
                                        &cmd_id,
                                        "prompt",
                                        &next_q.prompt,
                                        cfg.attachments_dir.as_deref(),
                                    )
                                    .await;
                                    if shim.write_stdin_line(line).await.is_err() {
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
                                            &mut pending_prompts,
                                        )
                                        .await;
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
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
                            "extension_ui_request" => {
                                handle_pi_extension_ui_request(
                                    &val,
                                    &mode,
                                    &active_turn_id,
                                    &shim,
                                    &mut pending_ext_reqs,
                                    &evt_tx,
                                )
                                .await;
                            }
                            _ => {}
                        }
                    }
                    ShimFrame::Exited { code } => {
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
                        seg_state.finalize_all_messages(&evt_tx).await;
                        if code != Some(0) {
                            warn!(
                                agent_id = %cfg.agent_id,
                                exit_code = ?code,
                                "Pi shim process exited"
                            );
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

async fn handle_pi_message_update(
    val: &Value,
    seg_state: &mut PiStreamSegmentState,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let ev = val.get("assistantMessageEvent").unwrap_or(val);
    let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
    let turn_tag = if seg_state.turn_tag.is_empty() {
        "init".to_string()
    } else {
        seg_state.turn_tag.clone()
    };

    match kind {
        "text_delta" | "text_start" => {
            let chunk = ev.get("delta").and_then(Value::as_str).unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            seg_state.clear_active_thought();
            seg_state.emitted_text_in_message = true;
            let mid = if let Some(ref cur) = seg_state.active_msg_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("pi_msg_{turn_tag}_{}", seg_state.step_seq);
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
        "text_end" => {
            if seg_state.active_msg_id.is_some() {
                seg_state.finalize_active_message(evt_tx).await;
            } else if let Some(content) = ev
                .get("content")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                seg_state.emitted_text_in_message = true;
                seg_state.step_seq += 1;
                let mid = format!("pi_msg_{turn_tag}_{}", seg_state.step_seq);
                let _ = evt_tx
                    .send(DriverEvent::AssistantText {
                        item_id: mid,
                        text: content.to_string(),
                        streaming: false,
                    })
                    .await;
            }
        }
        "thinking_delta" | "thinking_start" => {
            let chunk = ev.get("delta").and_then(Value::as_str).unwrap_or("");
            if chunk.is_empty() {
                return;
            }
            seg_state.finalize_active_message(evt_tx).await;
            let tid = if let Some(ref cur) = seg_state.active_thought_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("pi_th_{turn_tag}_{}", seg_state.step_seq);
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
        "thinking_end" => {
            seg_state.clear_active_thought();
        }
        "error" => {
            let msg = ev
                .get("error")
                .and_then(|e| {
                    e.as_str()
                        .or_else(|| e.get("errorMessage").and_then(Value::as_str))
                        .or_else(|| e.get("message").and_then(Value::as_str))
                })
                .or_else(|| ev.get("reason").and_then(Value::as_str))
                .unwrap_or("Pi model stream error")
                .to_string();
            let _ = evt_tx
                .send(DriverEvent::Notice {
                    level: "warning".into(),
                    message: msg,
                })
                .await;
        }
        _ => {}
    }
}

async fn handle_pi_extension_ui_request(
    val: &Value,
    mode: &SessionMode,
    active_turn_id: &Option<String>,
    shim: &ShimClient,
    pending_ext_reqs: &mut HashMap<String, PendingExtensionRequest>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let method = val
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    if method == "notify" {
        if let Some(msg) = val.get("message").and_then(Value::as_str) {
            let level = val
                .get("notifyType")
                .and_then(Value::as_str)
                .unwrap_or("info")
                .to_string();
            let _ = evt_tx
                .send(DriverEvent::Notice {
                    level,
                    message: msg.to_string(),
                })
                .await;
        }
        return;
    }

    if rpc_id.is_empty() {
        return;
    }

    if method == "confirm" {
        if matches!(mode, SessionMode::FullAccess) {
            let resp = json!({
                "type": "extension_ui_response",
                "id": rpc_id,
                "confirmed": true
            })
            .to_string();
            let _ = shim.write_stdin_line(resp).await;
            return;
        }
        let req_id = format!("pi_perm_{}", uuid::Uuid::new_v4().simple());
        let title = val
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Confirm Pi action")
            .to_string();
        let message = val
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        pending_ext_reqs.insert(req_id.clone(), PendingExtensionRequest { rpc_id, method });
        let _ = evt_tx
            .send(DriverEvent::PermissionRequest {
                request_id: req_id.clone(),
                title: title.clone(),
                detail: ToolCallDetail::Generic {
                    tool_name: title,
                    arguments_json: message,
                },
                // Pi `confirm` extension dialogs carry only free-text display titles without an
                // authoritative tool/command/path identity; keep them individual (never allow_always).
                allowed_decisions: vec!["allow".into(), "deny".into()],
                plan_approval: None,
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
        return;
    }

    if matches!(method.as_str(), "select" | "input" | "editor") {
        let req_id = format!("pi_q_{}", uuid::Uuid::new_v4().simple());
        let question = val
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Pi input requested")
            .to_string();
        let options = val
            .get("options")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        pending_ext_reqs.insert(req_id.clone(), PendingExtensionRequest { rpc_id, method });
        let _ = evt_tx
            .send(DriverEvent::QuestionRequest {
                request_id: req_id.clone(),
                questions: vec![QuestionItem {
                    id: "q0".into(),
                    header: None,
                    question,
                    options,
                    is_multi_select: false,
                }],
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::PromptOrigin;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    #[cfg(unix)]
    fn write_mock_script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write mock script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod mock script");
        path
    }

    #[cfg(unix)]
    async fn start_inprocess_shim(
        sock_path: PathBuf,
        spec: ShimSpawnSpec,
    ) -> (tokio::task::JoinHandle<()>, ShimClient) {
        let sock_bg = sock_path.clone();
        let (err_tx, mut err_rx) = tokio::sync::oneshot::channel::<String>();
        let handle = tokio::spawn(async move {
            if let Err(err) = orbit_shim::run_shim(&sock_bg, spec).await {
                let _ = err_tx.send(err.to_string());
            }
        });
        for _ in 0..400 {
            if let Ok(err) = err_rx.try_recv() {
                panic!("in-process orbit shim exited before ready: {err}");
            }
            if sock_path.exists()
                && let Ok(client) = ShimClient::connect(&sock_path).await
            {
                return (handle, client);
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        panic!("timed out waiting for in-process orbit shim socket");
    }

    fn make_test_cfg(
        agent_id: &str,
        script: &Path,
        sock_path: &Path,
        cwd: &Path,
        seq_tracker: Arc<AtomicU64>,
    ) -> SessionLaunchConfig {
        SessionLaunchConfig {
            agent_id: agent_id.to_string(),
            provider: ProviderKind::Pi,
            executable: script.to_path_buf(),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.to_path_buf(),
            runtime_dir: cwd.to_path_buf(),
            cwd: cwd.to_path_buf(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("anthropic/claude-sonnet-4-5".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
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
        }
    }

    #[test]
    fn pi_read_limit_calculates_end_line_and_safe_integer_bounds() {
        let with_offset_limit = map_pi_tool_detail(
            "read",
            Some(&json!({"path": "src/lib.rs", "offset": 100, "limit": 50})),
            None,
            None,
        );
        assert!(matches!(
            with_offset_limit,
            ToolCallDetail::FileRead {
                ref path,
                start_line: Some(100),
                end_line: Some(149),
            } if path == "src/lib.rs"
        ));

        let with_explicit_end = map_pi_tool_detail(
            "read_file",
            Some(&json!({"path": "src/main.rs", "start_line": 10, "end_line": 25})),
            None,
            None,
        );
        assert!(matches!(
            with_explicit_end,
            ToolCallDetail::FileRead {
                start_line: Some(10),
                end_line: Some(25),
                ..
            }
        ));

        let overflow_exit = map_pi_tool_detail(
            "bash",
            Some(&json!({"command": "echo hi"})),
            Some(&json!({"exitCode": 999999999999i64})),
            None,
        );
        assert!(matches!(
            overflow_exit,
            ToolCallDetail::Shell {
                exit_code: None,
                ..
            }
        ));
    }

    #[test]
    fn pi_spawn_spec_includes_rpc_session_model_thinking_and_handoff() {
        let seq = Arc::new(AtomicU64::new(0));
        let mut cfg = make_test_cfg(
            "ag_spec",
            Path::new("/usr/local/bin/pi"),
            Path::new("/tmp/pi.sock"),
            Path::new("/tmp"),
            seq,
        );
        cfg.resume_session_id = Some("/tmp/prev.jsonl".into());
        cfg.system_prompt_append = Some("Be concise".into());

        let with_resume = build_pi_spawn_spec(&cfg, true, None);
        assert_eq!(
            with_resume.args,
            vec![
                "--mode",
                "rpc",
                "--session",
                "/tmp/prev.jsonl",
                "--model",
                "anthropic/claude-sonnet-4-5",
                "--thinking",
                "high",
                "--append-system-prompt",
                "Be concise",
            ]
        );
        assert_eq!(
            with_resume.env.get("NO_COLOR").map(String::as_str),
            Some("1")
        );

        let without_resume = build_pi_spawn_spec(&cfg, false, None);
        assert!(!without_resume.args.contains(&"--session".to_string()));

        let combined = prepend_handoff_context(
            Some("[Prior Conversation Context in this Workspace]\nPrev".into()),
            "Continue",
        );
        assert!(combined.contains("[Prior Conversation Context in this Workspace]"));
        assert!(combined.ends_with("Continue"));
        assert_eq!(
            prepend_handoff_context(
                Some("[Prior Conversation Context in this Workspace]\nPrev".into()),
                &combined
            ),
            combined
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pi_rpc_streams_thinking_text_tools_steer_abort_and_config_over_shim() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_pi_test_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let log_file = tmp.join("received_commands.log");
        let sock_path = tmp.join("pi.sock");

        let mock_script = write_mock_script(
            &tmp,
            "mock_pi.sh",
            &format!(
                r#"#!/bin/sh
LOG="{}"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$LOG"
  case "$line" in
    *'"type":"get_state"'*)
      printf '%s\n' '{{"type":"response","id":"pi_cmd_1","command":"get_state","success":true,"data":{{"sessionId":"pi-sess-42","sessionFile":"/tmp/pi-sess-42.jsonl","model":{{"provider":"anthropic","id":"claude-sonnet-4-5"}}}}}}'
      ;;
    *'"message":"run demo"'*'"type":"prompt"'*)
      printf '%s\n' '{{"type":"response","command":"prompt","success":true}}'
      printf '%s\n' '{{"type":"agent_start"}}'
      printf '%s\n' '{{"type":"turn_start"}}'
      printf '%s\n' '{{"type":"message_start","message":{{"role":"assistant"}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"thinking_delta","delta":"Checking "}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"thinking_delta","delta":"repo"}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"thinking_end"}}}}'
      printf '%s\n' '{{"type":"tool_execution_start","toolCallId":"call_1","toolName":"bash","args":{{"command":"echo hi"}}}}'
      printf '%s\n' '{{"type":"tool_execution_update","toolCallId":"call_1","toolName":"bash","partialResult":{{"content":[{{"type":"text","text":"hi"}}]}} }}'
      printf '%s\n' '{{"type":"tool_execution_end","toolCallId":"call_1","toolName":"bash","result":{{"content":[{{"type":"text","text":"hi\n"}}],"details":{{"exitCode":0}}}},"isError":false}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":"Hello "}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":"from Pi!"}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_end"}}}}'
      printf '%s\n' '{{"type":"message_end","message":{{"role":"assistant","content":[{{"type":"text","text":"Hello from Pi!"}}]}}}}'
      printf '%s\n' '{{"type":"turn_end"}}'
      printf '%s\n' '{{"type":"agent_end"}}'
      ;;
    *'"message":"long task"'*'"type":"prompt"'*)
      printf '%s\n' '{{"type":"response","command":"prompt","success":true}}'
      printf '%s\n' '{{"type":"agent_start"}}'
      printf '%s\n' '{{"type":"turn_start"}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":"Working..."}}}}'
      ;;
    *'"type":"steer"'*)
      printf '%s\n' '{{"type":"response","command":"steer","success":true}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":" steered!"}}}}'
      ;;
    *'"type":"abort"'*)
      printf '%s\n' '{{"type":"response","command":"abort","success":true}}'
      printf '%s\n' '{{"type":"turn_end"}}'
      printf '%s\n' '{{"type":"agent_end"}}'
      ;;
    *'"type":"set_model"'*)
      printf '%s\n' '{{"type":"response","command":"set_model","success":true}}'
      ;;
    *'"type":"set_thinking_level"'*)
      printf '%s\n' '{{"type":"response","command":"set_thinking_level","success":true}}'
      ;;
  esac
done
"#,
                log_file.display()
            ),
        );

        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = make_test_cfg(
            "ag_pi_1",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker),
        );
        let spec = build_pi_spawn_spec(&cfg, true, None);
        let (shim_task, shim_client) = start_inprocess_shim(sock_path.clone(), spec).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let actor = tokio::spawn(run_pi_actor(cfg, shim_client, false, cmd_rx, evt_tx));

        // 1. Boot: SessionInitialized + Idle
        let mut init_ref = None;
        let mut init_model = None;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            match ev {
                DriverEvent::SessionInitialized {
                    persistence_ref,
                    model,
                } => {
                    init_ref = Some(persistence_ref);
                    init_model = model;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert_eq!(init_ref.as_deref(), Some("/tmp/pi-sess-42.jsonl"));
        assert_eq!(init_model.as_deref(), Some("anthropic/claude-sonnet-4-5"));

        // 2. First turn: thinking, tool call start/update/end, streaming text -> finalized text -> Idle
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_1".into(),
                prompt: "run demo".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send prompt");

        let mut last_thinking = String::new();
        let mut saw_tool_running = false;
        let mut saw_tool_completed = false;
        let mut final_text = String::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn 1 timeout")
                .expect("ev");
            match ev {
                DriverEvent::ThinkingText { text, .. } => last_thinking = text,
                DriverEvent::ToolCall {
                    call_id,
                    status,
                    detail:
                        ToolCallDetail::Shell {
                            command, exit_code, ..
                        },
                    ..
                } => {
                    assert_eq!(call_id, "call_1");
                    assert_eq!(command, "echo hi");
                    if status == ToolCallStatus::Running {
                        saw_tool_running = true;
                    } else if status == ToolCallStatus::Completed {
                        saw_tool_completed = true;
                        assert_eq!(exit_code, Some(0));
                    }
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
        assert_eq!(last_thinking, "Checking repo");
        assert!(saw_tool_running);
        assert!(saw_tool_completed);
        assert_eq!(final_text, "Hello from Pi!");

        // 3. Config update: bare model inherits cached provider from `get_state`, then explicit `openai/gpt-5`
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: Some("claude-opus-4-5".into()),
                effort: None,
                fast_mode: None,
            })
            .await
            .expect("set bare model");
        cmd_tx
            .send(DriverCommand::SetConfig {
                model: Some("openai/gpt-5".into()),
                effort: Some(ReasoningEffort::Xhigh),
                fast_mode: None,
            })
            .await
            .expect("set config");

        // 4. Mid-turn steer + abort
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_2".into(),
                prompt: "long task".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send long task");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("long task start")
                .expect("ev");
            if matches!(
                ev,
                DriverEvent::AssistantText {
                    streaming: true,
                    ..
                }
            ) {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_2_steer".into(),
                prompt: "focus on tests".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send steer");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("steer delta")
                .expect("ev");
            if let DriverEvent::AssistantText { text, .. } = ev
                && text.contains("steered!")
            {
                break;
            }
        }

        cmd_tx
            .send(DriverCommand::Interrupt)
            .await
            .expect("send abort");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("abort settle")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        assert!(seq_tracker.load(Ordering::Relaxed) >= 10);

        let recorded = std::fs::read_to_string(&log_file).expect("read command log");
        assert!(recorded.contains("\"modelId\":\"claude-opus-4-5\""));
        assert!(recorded.contains("\"provider\":\"anthropic\""));
        assert!(recorded.contains("\"type\":\"set_model\""));
        assert!(recorded.contains("\"provider\":\"openai\""));
        assert!(recorded.contains("\"modelId\":\"gpt-5\""));
        assert!(recorded.contains("\"type\":\"set_thinking_level\""));
        assert!(recorded.contains("\"level\":\"xhigh\""));
        assert!(recorded.contains("\"type\":\"steer\""));
        assert!(recorded.contains("\"type\":\"abort\""));

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pi_reattach_preserves_cursor_and_continues_in_flight_turn() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_pi_reattach_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let sock_path = tmp.join("pi_reattach.sock");
        let trigger_file = tmp.join("continue.flag");

        let mock_script = write_mock_script(
            &tmp,
            "mock_pi_reattach.sh",
            &format!(
                r#"#!/bin/sh
FLAG="{}"
while IFS= read -r line; do
  case "$line" in
    *'"type":"get_state"'*)
      printf '%s\n' '{{"type":"response","id":"pi_cmd_1","command":"get_state","success":true,"data":{{"sessionId":"pi-live-99"}}}}'
      ;;
    *'"type":"prompt"'*)
      printf '%s\n' '{{"type":"agent_start"}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":"Before restart."}}}}'
      while [ ! -f "$FLAG" ]; do
        sleep 0.05
      done
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_delta","delta":"After restart."}}}}'
      printf '%s\n' '{{"type":"message_update","assistantMessageEvent":{{"type":"text_end"}}}}'
      printf '%s\n' '{{"type":"agent_end"}}'
      ;;
  esac
done
"#,
                trigger_file.display()
            ),
        );

        let seq_tracker_1 = Arc::new(AtomicU64::new(0));
        let cfg1 = make_test_cfg(
            "ag_pi_reattach",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker_1),
        );
        let spec = build_pi_spawn_spec(&cfg1, false, None);
        let (shim_task, shim_client_1) = start_inprocess_shim(sock_path.clone(), spec).await;

        let (cmd_tx1, cmd_rx1) = mpsc::channel(16);
        let (evt_tx1, mut evt_rx1) = mpsc::channel(64);
        let actor1 = tokio::spawn(run_pi_actor(cfg1, shim_client_1, false, cmd_rx1, evt_tx1));

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx1.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        cmd_tx1
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_reattach".into(),
                prompt: "go".into(),
                behavior: ActiveTurnBehavior::Steer,
                origin: PromptOrigin::Human,
                message_ids: Vec::new(),
            })
            .await
            .expect("send prompt");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx1.recv())
                .await
                .expect("pre-restart delta")
                .expect("ev");
            if let DriverEvent::AssistantText { text, .. } = ev
                && text == "Before restart."
            {
                break;
            }
        }

        let saved_seq = seq_tracker_1.load(Ordering::Relaxed);
        assert!(saved_seq > 0);
        drop(cmd_tx1);
        let _ = tokio::time::timeout(Duration::from_secs(2), actor1).await;

        let shim_client_2 = connect_existing_shim_after(&sock_path, Some(saved_seq))
            .await
            .expect("reattach existing shim");
        let seq_tracker_2 = Arc::new(AtomicU64::new(saved_seq));
        let mut cfg2 = make_test_cfg(
            "ag_pi_reattach",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker_2),
        );
        cfg2.reattach_existing_shim = true;
        cfg2.shim_after_seq = Some(saved_seq);
        cfg2.resume_session_id = Some("pi-live-99".into());
        cfg2.persisted_active_turn_id = Some("turn_reattach".into());
        cfg2.persisted_turn_step_seq = 1;

        let (cmd_tx2, cmd_rx2) = mpsc::channel(16);
        let (evt_tx2, mut evt_rx2) = mpsc::channel(64);
        let actor2 = tokio::spawn(run_pi_actor(cfg2, shim_client_2, true, cmd_rx2, evt_tx2));

        std::fs::write(&trigger_file, b"go").expect("write trigger");

        let mut saw_before_dup = false;
        let mut post_item_id = None;
        let mut post_text = None;
        let mut settled = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(3), evt_rx2.recv()).await
        {
            match ev {
                DriverEvent::AssistantText {
                    item_id,
                    text,
                    streaming,
                } => {
                    if text.contains("Before restart.") {
                        saw_before_dup = true;
                    }
                    if !streaming {
                        post_item_id = Some(item_id);
                        post_text = Some(text);
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    settled = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(
            !saw_before_dup,
            "cursor-aware shim reattach must not duplicate pre-restart deltas"
        );
        assert_eq!(post_item_id.as_deref(), Some("pi_msg_turn_reattach_2"));
        assert_eq!(post_text.as_deref(), Some("After restart."));
        assert!(settled);
        assert!(seq_tracker_2.load(Ordering::Relaxed) > saved_seq);

        let _ = cmd_tx2.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor2).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pi_queue_steer_and_interrupt_and_drain_queue_delivery_lifecycle() {
        use orbit_shim::{ShimListener, ShimWriteHalf};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let sock_path = std::env::temp_dir().join(format!(
            "o_pi_deliv_{}.sock",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind mock shim");

        let cmds_seen: Arc<tokio::sync::Mutex<Vec<(String, String)>>> =
            Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let cmds_bg = Arc::clone(&cmds_seen);
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
                            send_stdout(&mut wh, &mut seq, json!({ "type": "agent_end" })).await;
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
                            "get_state" => {
                                send_stdout(
                                    &mut wh,
                                    &mut seq,
                                    json!({
                                        "type": "response",
                                        "id": val["id"],
                                        "command": "get_state",
                                        "success": true,
                                        "data": { "sessionId": "pi-deliv-1" }
                                    }),
                                )
                                .await;
                            }
                            "prompt" => {
                                let msg = val
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                cmds_bg.lock().await.push(("prompt".into(), msg.clone()));
                                send_stdout(&mut wh, &mut seq, json!({ "type": "agent_start" }))
                                    .await;
                                if msg.starts_with("SLOW_") {
                                    holding_slow = true;
                                } else {
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({ "type": "agent_end" }),
                                    )
                                    .await;
                                }
                            }
                            "steer" => {
                                let msg = val
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                cmds_bg.lock().await.push(("steer".into(), msg));
                            }
                            "abort" => {
                                cmds_bg.lock().await.push(("abort".into(), String::new()));
                                if holding_slow {
                                    holding_slow = false;
                                    send_stdout(
                                        &mut wh,
                                        &mut seq,
                                        json!({ "type": "agent_end" }),
                                    )
                                    .await;
                                }
                            }
                            other => {
                                cmds_bg.lock().await.push((other.into(), String::new()));
                            }
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
        let cfg = make_test_cfg(
            "ag_pi_deliv",
            Path::new("/bin/true"),
            &sock_path,
            &std::env::temp_dir(),
            Arc::new(AtomicU64::new(0)),
        );
        let actor = tokio::spawn(run_pi_actor(cfg, shim, false, cmd_rx, evt_tx));

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

        // 1. Slow turn -> Queue held in driver (not follow_up!) + native Steer -> natural settle drains Queue
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
        assert!(
            !cmds_seen
                .lock()
                .await
                .iter()
                .any(|(c, _)| c == "abort" || c == "follow_up"),
            "Queue must stay in driver until agent_end; Steer must not abort"
        );

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

        // 2. InterruptAndDrainQueue sends abort and preserves FIFO queue
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
        assert_eq!(
            *cmds_seen.lock().await,
            vec![
                ("prompt".into(), "SLOW_1".into()),
                ("steer".into(), "STEER_1".into()),
                ("prompt".into(), "Q1".into()),
                ("prompt".into(), "SLOW_2".into()),
                ("abort".into(), String::new()),
                ("prompt".into(), "Q2".into()),
                ("prompt".into(), "Q3".into()),
            ]
        );

        drop(cmd_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), server).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn pi_mcp_config_writes_0600_file_sets_env_and_cleanup_paths_without_argv_secrets_and_reaps_stale()
     {
        let root = std::env::temp_dir().join(format!("orbit-pi-mcp-test-{}", Uuid::new_v4()));
        let runtime_dir = root.join("runtime");
        let mcp = OrbitMcpLaunchSpec {
            server_name: "orbit".to_string(),
            http_url: "http://127.0.0.1:6868/mcp/agents?caller_agent_id=ag_pi_sec".to_string(),
            bearer_token: "super-secret-pi-token-98765".to_string(),
            stdio_command: PathBuf::from("/usr/local/bin/orbit"),
            stdio_args: vec!["mcp".to_string()],
        };

        let guard = write_pi_mcp_config_file(&runtime_dir, "ag_pi_sec", &mcp)
            .expect("write private pi MCP config");
        let cfg_path = guard.path().expect("guard path").to_path_buf();
        assert!(cfg_path.is_absolute(), "MCP config path must be absolute");
        assert!(cfg_path.exists());
        let file_name = cfg_path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("file name");
        assert!(
            file_name.starts_with("mcp-pi-ag_pi_sec-") && file_name.ends_with(".json"),
            "unexpected file name: {file_name}"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file_mode = std::fs::metadata(&cfg_path)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600, "pi MCP config must be 0600");
            let dir_mode = std::fs::metadata(&runtime_dir)
                .expect("dir metadata")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700, "runtime dir must be 0700");
        }

        let content: Value = serde_json::from_str(
            &std::fs::read_to_string(&cfg_path).expect("read written pi MCP json"),
        )
        .expect("parse written pi MCP json");
        assert_eq!(
            content
                .pointer("/mcpServers/orbit/url")
                .and_then(Value::as_str),
            Some(mcp.http_url.as_str())
        );
        assert_eq!(
            content
                .pointer("/mcpServers/orbit/headers/Authorization")
                .and_then(Value::as_str),
            Some("Bearer super-secret-pi-token-98765")
        );

        let seq = Arc::new(AtomicU64::new(0));
        let mut cfg = make_test_cfg(
            "ag_pi_sec",
            Path::new("/usr/local/bin/pi"),
            Path::new("/tmp/pi-sec.sock"),
            &root,
            seq,
        );
        cfg.mcp_spec = Some(mcp.clone());
        let spec = build_pi_spawn_spec(&cfg, false, Some(&cfg_path));
        for arg in &spec.args {
            assert!(
                !arg.contains(&mcp.bearer_token) && !arg.contains("Authorization"),
                "bearer token must never appear in pi argv: {arg}"
            );
        }
        assert_eq!(
            spec.env.get("ORBIT_MCP_CONFIG").map(String::as_str),
            Some(cfg_path.to_string_lossy().as_ref())
        );
        assert_eq!(
            spec.env.get("PI_MCP_CONFIG").map(String::as_str),
            Some(cfg_path.to_string_lossy().as_ref())
        );
        assert_eq!(spec.cleanup_paths, vec![cfg_path.clone()]);

        // Dropping an armed guard cleans up the file (simulating spawn failure).
        drop(guard);
        assert!(!cfg_path.exists(), "dropping armed guard must unlink file");

        // Disarmed guard leaves file for shim cleanup & startup reaper tests.
        let mut keep_guard =
            write_pi_mcp_config_file(&runtime_dir, "ag_alive", &mcp).expect("write alive");
        let keep_path = keep_guard.path().expect("keep path").to_path_buf();
        keep_guard.disarm();

        let mut stale_guard =
            write_pi_mcp_config_file(&runtime_dir, "ag_dead", &mcp).expect("write dead");
        let stale_path = stale_guard.path().expect("stale path").to_path_buf();
        stale_guard.disarm();

        let candidates = snapshot_pi_mcp_config_files(&runtime_dir);
        assert_eq!(candidates.len(), 2);

        // A fresh file written after snapshot for the same dead id must NEVER be reaped.
        let mut fresh_guard =
            write_pi_mcp_config_file(&runtime_dir, "ag_dead", &mcp).expect("write fresh");
        let fresh_path = fresh_guard.path().expect("fresh path").to_path_buf();
        fresh_guard.disarm();

        let surviving: HashSet<String> = ["ag_alive".to_string()].into_iter().collect();
        let reaped = reap_snapshotted_pi_mcp_configs(&candidates, &surviving);
        assert_eq!(reaped, 1);
        assert!(keep_path.exists());
        assert!(!stale_path.exists());
        assert!(fresh_path.exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
