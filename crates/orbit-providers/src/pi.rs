//! Native Rust driver for Pi (`pi --mode rpc`) over `orbit shim` UDS.
//!
//! Runs Pi's headless JSONL RPC protocol inside a detached `orbit shim` process so
//! `orbit daemon` restarts never interrupt active Pi coding sessions.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, ProviderKind, ProviderModelCatalog, ProviderModelSpec,
    QuestionItem, ReasoningEffort, SessionMode, ThinkingOptionSpec, ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after,
    spawn_detached_shim,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tracing::{debug, warn};

use crate::attachments::{CLAUDE_INLINE_IMAGE_MAX_BYTES, resolve_authorized_staged_images};
use crate::driver::{DriverCommand, DriverEvent, ProviderSessionHandle, SessionLaunchConfig};

const BOOT_GET_STATE_TIMEOUT: Duration = Duration::from_secs(15);
const SHIM_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

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

fn build_pi_spawn_spec(cfg: &SessionLaunchConfig, include_resume: bool) -> ShimSpawnSpec {
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

    ShimSpawnSpec {
        shim_id: format!("pi_{}", cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args,
        cwd: cfg.cwd.clone(),
        env,
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
        let spec = build_pi_spawn_spec(&cfg, true);
        let fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
            .await
            .map_err(|e| format!("failed to spawn pi shim: {e}"))?;
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

fn build_pi_images_payload(prompt: &str, attachments_dir: Option<&Path>) -> Vec<Value> {
    resolve_authorized_staged_images(prompt, attachments_dir, CLAUDE_INLINE_IMAGE_MAX_BYTES)
        .into_iter()
        .map(|img| {
            json!({
                "type": "image",
                "data": B64.encode(&img.bytes),
                "mimeType": img.mime_type,
            })
        })
        .collect()
}

fn build_pi_prompt_command(
    cmd_id: &str,
    cmd_type: &str,
    prompt: &str,
    attachments_dir: Option<&Path>,
) -> String {
    let images = build_pi_images_payload(prompt, attachments_dir);
    let mut obj = json!({
        "id": cmd_id,
        "type": cmd_type,
        "message": prompt,
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
            .map(ToOwned::to_owned)
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
                let fresh_spec = build_pi_spawn_spec(&cfg, false);
                match spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &fresh_spec).await {
                    Ok(mut fresh_shim) => {
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
    let mut pending_prompts: VecDeque<(String, String)> = VecDeque::new();
    let mut inflight_followups: HashMap<String, (String, String)> = HashMap::new();
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
                    } => {
                        let effective_prompt = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };

                        if active_turn_id.is_some() {
                            match behavior {
                                ActiveTurnBehavior::Steer => {
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let line = build_pi_prompt_command(
                                        &cmd_id,
                                        "steer",
                                        &effective_prompt,
                                        cfg.attachments_dir.as_deref(),
                                    );
                                    if shim.write_stdin_line(line).await.is_err() {
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                            .await;
                                        break;
                                    }
                                }
                                ActiveTurnBehavior::Queue => {
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    inflight_followups.insert(
                                        cmd_id.clone(),
                                        (turn_id, effective_prompt.clone()),
                                    );
                                    let line = build_pi_prompt_command(
                                        &cmd_id,
                                        "follow_up",
                                        &effective_prompt,
                                        cfg.attachments_dir.as_deref(),
                                    );
                                    if shim.write_stdin_line(line).await.is_err() {
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                            .await;
                                        break;
                                    }
                                }
                                ActiveTurnBehavior::Interrupt => {
                                    cancel_pending_ext_reqs(&shim, &mut pending_ext_reqs).await;
                                    inflight_followups.clear();
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let abort_line = json!({
                                        "id": cmd_id,
                                        "type": "abort"
                                    })
                                    .to_string();
                                    if shim.write_stdin_line(abort_line).await.is_err() {
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                            .await;
                                        break;
                                    }
                                    pending_prompts.push_front((turn_id, effective_prompt));
                                }
                            }
                            continue;
                        }

                        active_turn_id = Some(turn_id.clone());
                        seg_state.reset_for_turn(&turn_id);
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Running { turn_id }))
                            .await;
                        let cmd_id = format!("pi_cmd_{next_cmd_id}");
                        next_cmd_id += 1;
                        let line = build_pi_prompt_command(
                            &cmd_id,
                            "prompt",
                            &effective_prompt,
                            cfg.attachments_dir.as_deref(),
                        );
                        if shim.write_stdin_line(line).await.is_err() {
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        pending_prompts.clear();
                        inflight_followups.clear();
                        cancel_pending_ext_reqs(&shim, &mut pending_ext_reqs).await;
                        let cmd_id = format!("pi_cmd_{next_cmd_id}");
                        next_cmd_id += 1;
                        let abort_line = json!({
                            "id": cmd_id,
                            "type": "abort"
                        })
                        .to_string();
                        if shim.write_stdin_line(abort_line).await.is_err() {
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                            break;
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
                                let cmd_id = val.get("id").and_then(Value::as_str).unwrap_or("");
                                let ok =
                                    val.get("success").and_then(Value::as_bool).unwrap_or(true);
                                let failed_followup = inflight_followups.remove(cmd_id);
                                if !ok {
                                    let err_msg = val
                                        .get("error")
                                        .and_then(Value::as_str)
                                        .unwrap_or("Pi RPC command failed")
                                        .to_string();
                                    if cmd_name == "follow_up"
                                        && let Some(queued) = failed_followup
                                    {
                                        pending_prompts.push_back(queued);
                                    } else if cmd_name == "prompt" {
                                        seg_state.finalize_all_messages(&evt_tx).await;
                                        active_turn_id = None;
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                                message: err_msg,
                                                recoverable: true,
                                            }))
                                            .await;
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
                                active_turn_id = None;
                                pending_ext_reqs.clear();
                                inflight_followups.clear();
                                if let Some((next_turn_id, next_prompt)) =
                                    pending_prompts.pop_front()
                                {
                                    active_turn_id = Some(next_turn_id.clone());
                                    seg_state.reset_for_turn(&next_turn_id);
                                    let _ = evt_tx
                                        .send(DriverEvent::StatusChanged(AgentStatus::Running {
                                            turn_id: next_turn_id,
                                        }))
                                        .await;
                                    let cmd_id = format!("pi_cmd_{next_cmd_id}");
                                    next_cmd_id += 1;
                                    let line = build_pi_prompt_command(
                                        &cmd_id,
                                        "prompt",
                                        &next_prompt,
                                        cfg.attachments_dir.as_deref(),
                                    );
                                    if shim.write_stdin_line(line).await.is_err() {
                                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                            .await;
                                        break;
                                    }
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
                allowed_decisions: vec!["allow".into(), "deny".into()],
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
        let handle = tokio::spawn(async move {
            let _ = orbit_shim::run_shim(&sock_bg, spec).await;
        });
        for _ in 0..150 {
            if sock_path.exists()
                && let Ok(client) = ShimClient::connect(&sock_path).await
            {
                return (handle, client);
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
        panic!("timed out waiting for in-process orbit shim socket");
    }

    #[cfg(unix)]
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

        let with_resume = build_pi_spawn_spec(&cfg, true);
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

        let without_resume = build_pi_spawn_spec(&cfg, false);
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
        let spec = build_pi_spawn_spec(&cfg, true);
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
        let spec = build_pi_spawn_spec(&cfg1, false);
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
}
