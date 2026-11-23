//! Dedicated native Rust ACP (Agent Client Protocol) JSON-RPC 2.0 stdio driver
//! powering all 9 ACP-speaking first-class Orbit providers inside `orbit shim` UDS:
//! - OpenCode (`opencode acp`)
//! - Gemini CLI (`gemini --experimental-acp` / `--acp`)
//! - Cursor Agent (`cursor-agent acp`)
//! - GitHub Copilot (`copilot --acp`)
//! - Goose (`goose acp`)
//! - Kimi CLI (`kimi acp`)
//! - Cline (`cline --acp`)
//! - Mistral Vibe (`vibe-acp` / `vibe --acp`)
//! - Augment (`auggie --acp`)

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Duration;

use orbit_protocol::{
    ActiveTurnBehavior, AgentStatus, ProviderKind, ProviderModelCatalog, ProviderModelSpec,
    QuestionItem, ReasoningEffort, SessionMode, ToolCallDetail, ToolCallStatus,
};
use orbit_shim::{
    ShimClient, ShimFrame, ShimMode, ShimSpawnSpec, connect_existing_shim_after,
    spawn_detached_shim,
};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::attachments::{AcpPromptMediaCaps, build_acp_prompt_blocks_async, is_path_within_dir};
use crate::driver::{
    DriverCommand, DriverEvent, ProviderSessionHandle, RpcId, SessionLaunchConfig,
};

const ACP_BOOT_TIMEOUT: Duration = Duration::from_secs(30);
const SHIM_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

/// Return the CLI subcommand / flag arguments required to launch an ACP stdio server
/// for each of the 9 first-class ACP providers.
pub fn acp_spawn_args(provider: ProviderKind, executable: &Path) -> Vec<String> {
    match provider {
        ProviderKind::OpenCode => vec!["acp".to_string()],
        ProviderKind::GeminiCli => {
            if let Ok(custom) = std::env::var("ORBIT_GEMINI_ACP_FLAG") {
                let trimmed = custom.trim();
                if !trimmed.is_empty() {
                    return vec![trimmed.to_string()];
                }
            }
            vec!["--experimental-acp".to_string()]
        }
        ProviderKind::CursorAgent => vec!["acp".to_string()],
        ProviderKind::Copilot => vec!["--acp".to_string()],
        ProviderKind::Goose => vec!["acp".to_string()],
        ProviderKind::Kimi => vec!["acp".to_string()],
        ProviderKind::Cline => vec!["--acp".to_string()],
        ProviderKind::MistralVibe => {
            let stem = executable
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if stem.eq_ignore_ascii_case("vibe-acp") {
                Vec::new()
            } else {
                vec!["--acp".to_string()]
            }
        }
        ProviderKind::Augment => vec!["--acp".to_string()],
        _ => vec!["acp".to_string()],
    }
}

/// Instant fallback model catalog for each of the 9 ACP providers (zero startup subprocess cost).
pub fn fallback_acp_provider_models(provider: ProviderKind) -> ProviderModelCatalog {
    let spec = |id: &str, label: &str, description: &str, is_default: bool| ProviderModelSpec {
        id: id.to_string(),
        label: label.to_string(),
        description: description.to_string(),
        is_default,
        default_effort: None,
        thinking_options: Vec::new(),
        supports_fast_mode: false,
    };

    match provider {
        ProviderKind::OpenCode => ProviderModelCatalog {
            provider,
            configured_default_model: Some("default".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "default",
                    "OpenCode Default",
                    "Active model configured in OpenCode",
                    true,
                ),
                spec(
                    "anthropic/claude-sonnet-4-5",
                    "Claude Sonnet 4.5",
                    "Anthropic Sonnet via OpenCode",
                    false,
                ),
                spec(
                    "openai/gpt-5",
                    "GPT-5",
                    "OpenAI flagship via OpenCode",
                    false,
                ),
            ],
        },
        ProviderKind::GeminiCli => ProviderModelCatalog {
            provider,
            configured_default_model: Some("auto".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "auto",
                    "Gemini Auto",
                    "Default Gemini CLI model routing",
                    true,
                ),
                spec(
                    "gemini-2.5-pro",
                    "Gemini 2.5 Pro",
                    "Deep reasoning and multi-file edits",
                    false,
                ),
                spec(
                    "gemini-2.5-flash",
                    "Gemini 2.5 Flash",
                    "Fast interactive iteration",
                    false,
                ),
            ],
        },
        ProviderKind::CursorAgent => ProviderModelCatalog {
            provider,
            configured_default_model: Some("auto".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "auto",
                    "Cursor Auto",
                    "Automatic model selection in Cursor Agent",
                    true,
                ),
                spec(
                    "claude-4-sonnet",
                    "Claude 4 Sonnet",
                    "Anthropic Sonnet via Cursor",
                    false,
                ),
                spec("gpt-5", "GPT-5", "OpenAI flagship via Cursor", false),
            ],
        },
        ProviderKind::Copilot => ProviderModelCatalog {
            provider,
            configured_default_model: Some("claude-sonnet-4.5".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "claude-sonnet-4.5",
                    "Claude Sonnet 4.5",
                    "Default Copilot coding model",
                    true,
                ),
                spec(
                    "gpt-5",
                    "GPT-5",
                    "OpenAI flagship via GitHub Copilot",
                    false,
                ),
                spec(
                    "gemini-2.5-pro",
                    "Gemini 2.5 Pro",
                    "Google Gemini via GitHub Copilot",
                    false,
                ),
            ],
        },
        ProviderKind::Goose => ProviderModelCatalog {
            provider,
            configured_default_model: Some("default".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "default",
                    "Goose Default",
                    "Active profile model configured in Goose",
                    true,
                ),
                spec(
                    "claude-sonnet-4-5",
                    "Claude Sonnet 4.5",
                    "Anthropic Sonnet via Goose",
                    false,
                ),
                spec("gpt-5", "GPT-5", "OpenAI flagship via Goose", false),
            ],
        },
        ProviderKind::Kimi => ProviderModelCatalog {
            provider,
            configured_default_model: Some("kimi-k2-thinking".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "kimi-k2-thinking",
                    "Kimi K2 Thinking",
                    "Long-horizon reasoning & agentic coding",
                    true,
                ),
                spec(
                    "kimi-k2",
                    "Kimi K2",
                    "Fast everyday software engineering",
                    false,
                ),
            ],
        },
        ProviderKind::Cline => ProviderModelCatalog {
            provider,
            configured_default_model: Some("default".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "default",
                    "Cline Default",
                    "Active model configured in Cline CLI",
                    true,
                ),
                spec(
                    "claude-sonnet-4-5",
                    "Claude Sonnet 4.5",
                    "Anthropic Sonnet via Cline",
                    false,
                ),
                spec("gpt-5", "GPT-5", "OpenAI flagship via Cline", false),
            ],
        },
        ProviderKind::MistralVibe => ProviderModelCatalog {
            provider,
            configured_default_model: Some("devstral-latest".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "devstral-latest",
                    "Devstral",
                    "Mistral agentic software engineering model",
                    true,
                ),
                spec(
                    "mistral-large-latest",
                    "Mistral Large",
                    "General-purpose reasoning & coding",
                    false,
                ),
            ],
        },
        ProviderKind::Augment => ProviderModelCatalog {
            provider,
            configured_default_model: Some("default".into()),
            configured_default_effort: None,
            models: vec![
                spec(
                    "default",
                    "Augment Default",
                    "Active model configured in Auggie",
                    true,
                ),
                spec(
                    "claude-sonnet-4-5",
                    "Claude Sonnet 4.5",
                    "Anthropic Sonnet via Augment",
                    false,
                ),
            ],
        },
        other => ProviderModelCatalog {
            provider: other,
            configured_default_model: Some("default".into()),
            configured_default_effort: None,
            models: vec![spec(
                "default",
                other.display_name(),
                "Provider default model",
                true,
            )],
        },
    }
}

fn build_acp_spawn_spec(
    cfg: &SessionLaunchConfig,
    override_args: Option<Vec<String>>,
) -> ShimSpawnSpec {
    let args = override_args.unwrap_or_else(|| acp_spawn_args(cfg.provider, &cfg.executable));
    let mut env = cfg.extra_env.clone();
    env.insert("NO_COLOR".into(), "1".into());
    crate::shell_snapshot::merge_cached_shell_env_into_map(&mut env);

    ShimSpawnSpec {
        shim_id: format!("{}_{}", cfg.provider.slug(), cfg.agent_id),
        mode: ShimMode::Stdio,
        program: cfg.executable.to_string_lossy().to_string(),
        args,
        cwd: cfg.cwd.clone(),
        env,
        cleanup_paths: Vec::new(),
    }
}

pub async fn spawn_acp_provider_session(
    cfg: SessionLaunchConfig,
) -> Result<ProviderSessionHandle, String> {
    if cfg.native_fork_only {
        return Err(format!(
            "{} eager native fork is deferred to handoff on first prompt",
            cfg.provider.display_name()
        ));
    }
    let reattached = if cfg.reattach_existing_shim {
        connect_existing_shim_after(&cfg.shim_socket, cfg.shim_after_seq).await
    } else {
        None
    };

    let (shim, is_reattached) = if let Some(existing) = reattached {
        (existing, true)
    } else {
        let spec = build_acp_spawn_spec(&cfg, None);
        let fresh = spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &spec)
            .await
            .map_err(|e| format!("failed to spawn {} shim: {e}", cfg.provider.display_name()))?;
        (fresh, false)
    };

    let (cmd_tx, cmd_rx) = mpsc::channel::<DriverCommand>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<DriverEvent>(256);

    tokio::spawn(run_acp_actor(cfg, shim, is_reattached, cmd_rx, evt_tx));

    Ok(ProviderSessionHandle { cmd_tx, evt_rx })
}

/// Kill the active shim process and wait (up to 2s) for it to exit and release its socket
/// before respawning or returning on boot error.
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

#[derive(Debug)]
enum BootRpcError {
    RpcError(Value),
    Timeout(String),
    Exited(String),
    Transport(String),
}

impl BootRpcError {
    fn message(&self) -> String {
        match self {
            Self::RpcError(v) => v
                .get("message")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| v.to_string()),
            Self::Timeout(s) | Self::Exited(s) | Self::Transport(s) => s.clone(),
        }
    }

    fn rpc_code(&self) -> Option<i64> {
        match self {
            Self::RpcError(v) => v.get("code").and_then(Value::as_i64),
            _ => None,
        }
    }
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

impl AcpStreamSegmentState {
    fn reset_for_turn(&mut self, turn_id: &str) {
        self.turn_tag = turn_id.to_string();
        self.step_seq = 0;
        self.active_msg_id = None;
        self.active_thought_id = None;
        self.msg_accum.clear();
        self.thought_accum.clear();
    }

    fn clear_active_thought(&mut self) {
        if let Some(tid) = self.active_thought_id.take() {
            self.thought_accum.remove(&tid);
        }
    }

    async fn finalize_active_message(&mut self, evt_tx: &mpsc::Sender<DriverEvent>) {
        if let Some(prev_mid) = self.active_msg_id.take()
            && let Some(prev_text) = self.msg_accum.remove(&prev_mid)
        {
            let _ = evt_tx
                .send(DriverEvent::AssistantText {
                    item_id: prev_mid,
                    text: prev_text,
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

struct PendingAcpPerm {
    rpc_id: Value,
    allow_option_id: String,
    allow_always_option_id: String,
    deny_option_id: String,
    is_question: bool,
    raw_questions: Vec<QuestionItem>,
}

struct OpenedAcpSession {
    session_id: String,
    current_model: Option<String>,
    available_modes: Vec<String>,
    effort_config_option_id: Option<String>,
}

fn build_mcp_servers_http(cfg: &SessionLaunchConfig) -> Value {
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

fn build_mcp_servers_stdio(cfg: &SessionLaunchConfig) -> Value {
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

/// Strictly match missing/evicted session errors by requiring a session noun alongside a missing
/// or unknown/expired indicator, so runtime tool errors like `"file not found"` or
/// `"model not found"` never reset a live session.
fn is_missing_session_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    let has_session_noun = lower.contains("session")
        || lower.contains("trajectory")
        || lower.contains("conversation")
        || lower.contains("thread");
    let has_missing_indicator = lower.contains("not found")
        || lower.contains("not_found")
        || lower.contains("does not exist")
        || lower.contains("unknown")
        || lower.contains("invalid")
        || lower.contains("expired")
        || lower.contains("no such");
    has_session_noun && has_missing_indicator
}

fn should_retry_gemini_acp_flag(provider: ProviderKind, err: &str) -> bool {
    if provider != ProviderKind::GeminiCli {
        return false;
    }
    let lower = err.to_ascii_lowercase();
    lower.contains("experimental-acp")
        || lower.contains("unknown option")
        || lower.contains("unknown argument")
        || lower.contains("unrecognized option")
}

fn resolve_acp_mode_id(mode: &SessionMode, available_modes: &[String]) -> Option<String> {
    let candidates: &[&str] = match mode {
        // Auto (Guardian) maps to approval-gating mode so Orbit's daemon receives permission requests.
        SessionMode::Default | SessionMode::Auto => &["default", "ask", "standard"],
        SessionMode::Plan => &["plan", "architect", "readonly", "read-only"],
        SessionMode::AcceptEdits => &[
            "accept-edits",
            "accept_edits",
            "acceptEdits",
            "code",
            "auto",
        ],
        SessionMode::FullAccess => &[
            "bypassPermissions",
            "full-access",
            "full_access",
            "yolo",
            "dontAsk",
            "auto",
        ],
        SessionMode::Custom(c) => return Some(c.clone()),
    };

    if !available_modes.is_empty() {
        for cand in candidates {
            if let Some(found) = available_modes
                .iter()
                .find(|m| m.eq_ignore_ascii_case(cand))
            {
                return Some(found.clone());
            }
        }
        return None;
    }

    candidates.first().map(|s| (*s).to_string())
}

fn parse_acp_mode_id(mode_id: &str) -> SessionMode {
    match mode_id.trim().to_ascii_lowercase().as_str() {
        "default" | "ask" | "standard" => SessionMode::Default,
        "plan" | "architect" | "readonly" | "read-only" => SessionMode::Plan,
        "accept-edits" | "accept_edits" | "acceptedits" | "code" => SessionMode::AcceptEdits,
        "bypasspermissions" | "full-access" | "full_access" | "yolo" | "dontask" => {
            SessionMode::FullAccess
        }
        other => SessionMode::Custom(other.to_string()),
    }
}

fn acp_effort_wire_value(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max | ReasoningEffort::Ultra => "max",
    }
}

fn find_acp_effort_config_option_id(sess_val: &Value) -> Option<String> {
    let arr = sess_val.get("configOptions")?.as_array()?;
    for opt in arr {
        let Some(id) = opt.get("id").and_then(Value::as_str) else {
            continue;
        };
        let lower = id.trim().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "reasoning_effort" | "reasoning-effort" | "effort" | "thinking" | "thought_level"
        ) {
            return Some(id.to_string());
        }
    }
    None
}

async fn send_acp_req(
    shim: &ShimClient,
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

async fn wait_acp_rpc_during_boot(
    provider_name: &str,
    shim: &mut ShimClient,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    mut target_rx: oneshot::Receiver<Result<Value, Value>>,
    shim_seq_tracker: &std::sync::atomic::AtomicU64,
) -> Result<Value, BootRpcError> {
    let deadline = tokio::time::Instant::now() + ACP_BOOT_TIMEOUT;
    let mut stderr_tail = String::new();

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(BootRpcError::Timeout(format!(
                "{provider_name} ACP initialization timed out"
            )));
        }
        tokio::select! {
            res = &mut target_rx => {
                return match res {
                    Ok(Ok(v)) => Ok(v),
                    Ok(Err(e)) => Err(BootRpcError::RpcError(e)),
                    Err(_) => Err(BootRpcError::Transport("ACP rpc channel dropped".into())),
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
                                let Ok(val) = serde_json::from_str::<Value>(&line) else {
                                    continue;
                                };
                                let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                                let method = val.get("method").and_then(Value::as_str);

                                if has_id && method.is_none() {
                                    if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone())
                                        && let Some(tx) = pending_rpcs.remove(&rid.as_key())
                                    {
                                        if let Some(err) = val.get("error").filter(|v| !v.is_null()) {
                                            let _ = tx.send(Err(err.clone()));
                                        } else {
                                            let _ = tx.send(Ok(val.get("result").cloned().unwrap_or(Value::Null)));
                                        }
                                    }
                                } else if has_id && let Some(req_method) = method {
                                    // Reply immediately to any server->client RPC during boot so the
                                    // ACP server never deadlocks waiting for a response.
                                    let resp = json!({
                                        "jsonrpc": "2.0",
                                        "id": val["id"],
                                        "error": {
                                            "code": -32601,
                                            "message": format!("Method not supported during boot: {req_method}")
                                        }
                                    })
                                    .to_string();
                                    let _ = shim.write_stdin_line(resp).await;
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
                                return Err(BootRpcError::Exited(if stderr_tail.is_empty() {
                                    format!("{provider_name} exited early with code {code:?}")
                                } else {
                                    format!("{provider_name} exited early (code {code:?}): {stderr_tail}")
                                }));
                            }
                            _ => {}
                        }
                    }
                    Ok(None) => return Err(BootRpcError::Exited(format!("{provider_name} shim closed"))),
                    Err(_) => return Err(BootRpcError::Timeout(format!("{provider_name} ACP initialization timed out"))),
                }
            }
        }
    }
}

async fn send_acp_initialize(
    cfg: &SessionLaunchConfig,
    shim: &mut ShimClient,
    next_rpc_id: &mut i64,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
) -> Result<Value, BootRpcError> {
    let init_rx = send_acp_req(
        shim,
        next_rpc_id,
        pending_rpcs,
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {
                    "readTextFile": false,
                    "writeTextFile": false
                },
                "terminal": false
            },
            "clientInfo": {
                "name": "orbit",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
    .await
    .map_err(|e| BootRpcError::Transport(format!("failed to send initialize: {e}")))?;

    wait_acp_rpc_during_boot(
        cfg.provider.display_name(),
        shim,
        pending_rpcs,
        init_rx,
        &cfg.shim_seq_tracker,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn open_acp_session(
    cfg: &SessionLaunchConfig,
    shim: &mut ShimClient,
    next_rpc_id: &mut i64,
    pending_rpcs: &mut HashMap<String, oneshot::Sender<Result<Value, Value>>>,
    resume_id: Option<&str>,
    requested_model: Option<&str>,
    requested_effort: Option<ReasoningEffort>,
    mode: &SessionMode,
) -> Result<OpenedAcpSession, BootRpcError> {
    let is_resume = resume_id.is_some();
    let provider_name = cfg.provider.display_name();

    let build_open_req = |mcp_val: Value| {
        let mut params = if let Some(sid) = resume_id {
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
        };
        if let Some(append) = cfg
            .system_prompt_append
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            params["_meta"] = json!({
                "systemPrompt": { "append": append }
            });
        }
        let method = if is_resume {
            "session/load"
        } else {
            "session/new"
        };
        (method, params)
    };

    let (method, params) = build_open_req(build_mcp_servers_http(cfg));
    let sess_rx = send_acp_req(shim, next_rpc_id, pending_rpcs, method, params)
        .await
        .map_err(|e| BootRpcError::Transport(format!("failed to send {method}: {e}")))?;

    let sess_val = match wait_acp_rpc_during_boot(
        provider_name,
        shim,
        pending_rpcs,
        sess_rx,
        &cfg.shim_seq_tracker,
    )
    .await
    {
        Ok(v) => v,
        Err(first_err)
            if cfg.mcp_spec.is_some()
                && matches!(first_err, BootRpcError::RpcError(_))
                && !(is_resume
                    && (is_missing_session_error(&first_err.message())
                        || first_err.rpc_code() == Some(-32601))) =>
        {
            // Fallback 1: stdio MCP descriptor for ACP agents that don't accept Streamable HTTP
            let (fb_method, fb_params) = build_open_req(build_mcp_servers_stdio(cfg));
            let fb_rx = send_acp_req(shim, next_rpc_id, pending_rpcs, fb_method, fb_params)
                .await
                .map_err(|e| {
                    BootRpcError::Transport(format!(
                        "failed to send stdio fallback {fb_method}: {e}"
                    ))
                })?;
            match wait_acp_rpc_during_boot(
                provider_name,
                shim,
                pending_rpcs,
                fb_rx,
                &cfg.shim_seq_tracker,
            )
            .await
            {
                Ok(v) => v,
                Err(stdio_err)
                    if matches!(stdio_err, BootRpcError::RpcError(_))
                        && !(is_resume
                            && (is_missing_session_error(&stdio_err.message())
                                || stdio_err.rpc_code() == Some(-32601))) =>
                {
                    // Fallback 2: empty MCP servers if the provider rejects injected MCP servers
                    let (empty_method, empty_params) = build_open_req(json!([]));
                    let empty_rx =
                        send_acp_req(shim, next_rpc_id, pending_rpcs, empty_method, empty_params)
                            .await
                            .map_err(|e| {
                                BootRpcError::Transport(format!(
                                    "failed to send empty-mcp fallback: {e}"
                                ))
                            })?;
                    wait_acp_rpc_during_boot(
                        provider_name,
                        shim,
                        pending_rpcs,
                        empty_rx,
                        &cfg.shim_seq_tracker,
                    )
                    .await?
                }
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };

    // Drain replay notifications on `session/load` until 50ms of quiet, aborting immediately if the shim exits
    if is_resume {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(50), shim.rx.recv()).await {
                Ok(Some(ShimFrame::Exited { code })) => {
                    return Err(BootRpcError::Exited(format!(
                        "{provider_name} exited during session/load replay (code {code:?})"
                    )));
                }
                Ok(Some(frame)) => {
                    if let Some(seq) = frame.seq() {
                        cfg.shim_seq_tracker.fetch_max(seq, Ordering::Relaxed);
                    }
                }
                Ok(None) => {
                    return Err(BootRpcError::Exited(format!(
                        "{provider_name} shim closed during session/load replay"
                    )));
                }
                Err(_) => break,
            }
        }
    }

    let session_id = sess_val
        .get("sessionId")
        .and_then(Value::as_str)
        .or(resume_id)
        .unwrap_or("unknown")
        .to_string();

    let available_modes: Vec<String> = sess_val
        .pointer("/modes/availableModes")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    item.get("id")
                        .or_else(|| item.get("modeId"))
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default();

    let effort_config_option_id = find_acp_effort_config_option_id(&sess_val);

    let mut current_model = sess_val
        .pointer("/models/currentModelId")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| requested_model.map(ToOwned::to_owned));

    if let Some(req_model) = requested_model.map(str::trim).filter(|s| {
        !s.is_empty() && !s.eq_ignore_ascii_case("default") && !s.eq_ignore_ascii_case("auto")
    }) && current_model.as_deref() != Some(req_model)
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
        && let Ok(v) =
            wait_acp_rpc_during_boot(provider_name, shim, pending_rpcs, rx, &cfg.shim_seq_tracker)
                .await
    {
        current_model = v
            .pointer("/models/currentModelId")
            .or_else(|| v.get("modelId"))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| Some(req_model.to_string()));
    }

    if let (Some(opt_id), Some(eff)) = (effort_config_option_id.as_deref(), requested_effort)
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_config_option",
            json!({
                "sessionId": session_id,
                "configId": opt_id,
                "value": acp_effort_wire_value(eff),
            }),
        )
        .await
    {
        let _ =
            wait_acp_rpc_during_boot(provider_name, shim, pending_rpcs, rx, &cfg.shim_seq_tracker)
                .await;
    }

    if !matches!(mode, SessionMode::Default)
        && let Some(mode_id) = resolve_acp_mode_id(mode, &available_modes)
        && let Ok(rx) = send_acp_req(
            shim,
            next_rpc_id,
            pending_rpcs,
            "session/set_mode",
            json!({
                "sessionId": session_id,
                "modeId": mode_id
            }),
        )
        .await
    {
        let _ =
            wait_acp_rpc_during_boot(provider_name, shim, pending_rpcs, rx, &cfg.shim_seq_tracker)
                .await;
    }

    Ok(OpenedAcpSession {
        session_id,
        current_model,
        available_modes,
        effort_config_option_id,
    })
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_acp_prompt(
    shim: &ShimClient,
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
    seg_state.reset_for_turn(&turn_id);

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

async fn emit_status_after_perm_resolve(
    active_turn_id: &Option<String>,
    pending_perms: &HashMap<String, PendingAcpPerm>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    if let Some(next_req_id) = pending_perms.keys().next().cloned() {
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
    seg_state: &mut AcpStreamSegmentState,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    seg_state.finalize_all_messages(evt_tx).await;
    let _ = evt_tx.send(DriverEvent::Exited { code: None }).await;
}

async fn run_acp_actor(
    cfg: SessionLaunchConfig,
    mut shim: ShimClient,
    is_reattached: bool,
    mut cmd_rx: mpsc::Receiver<DriverCommand>,
    evt_tx: mpsc::Sender<DriverEvent>,
) {
    let provider_name = cfg.provider.display_name();
    let provider_slug = cfg.provider.slug();
    let mut next_rpc_id: i64 = if is_reattached { 100_000 } else { 1 };
    let mut pending_rpcs: HashMap<String, oneshot::Sender<Result<Value, Value>>> = HashMap::new();
    let mut inject_handoff_on_next_prompt = false;
    let mut prompt_media_caps = AcpPromptMediaCaps::default();
    let mut recovered_missing_session = false;
    let mut pending_handoff_context = cfg.fallback_handoff_context.clone();

    let (mut session_id, mut current_model, mut available_modes, mut effort_config_option_id) =
        if is_reattached && let Some(ref sid) = cfg.resume_session_id {
            (sid.clone(), cfg.model.clone(), Vec::new(), None)
        } else {
            // 1. `initialize` (with Gemini CLI `--experimental-acp` -> `--acp` transparent fallback)
            let init_result =
                match send_acp_initialize(&cfg, &mut shim, &mut next_rpc_id, &mut pending_rpcs)
                    .await
                {
                    Ok(v) => v,
                    Err(first_err)
                        if should_retry_gemini_acp_flag(cfg.provider, &first_err.message()) =>
                    {
                        let first_msg = first_err.message();
                        warn!(
                            agent_id = %cfg.agent_id,
                            error = %first_msg,
                            "Gemini CLI rejected --experimental-acp; retrying with --acp"
                        );
                        kill_and_wait_shim(&mut shim).await;
                        cfg.shim_seq_tracker.store(0, Ordering::Relaxed);
                        pending_rpcs.clear();
                        let retry_spec =
                            build_acp_spawn_spec(&cfg, Some(vec!["--acp".to_string()]));
                        match spawn_detached_shim(&cfg.orbit_exe, &cfg.shim_socket, &retry_spec)
                            .await
                        {
                            Ok(mut retry_shim) => {
                                match send_acp_initialize(
                                    &cfg,
                                    &mut retry_shim,
                                    &mut next_rpc_id,
                                    &mut pending_rpcs,
                                )
                                .await
                                {
                                    Ok(v) => {
                                        shim = retry_shim;
                                        v
                                    }
                                    Err(e) => {
                                        kill_and_wait_shim(&mut retry_shim).await;
                                        let _ = evt_tx
                                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                                message: format!(
                                                    "{provider_name} ACP initialize failed: {}",
                                                    e.message()
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
                                        message: format!("{provider_name} retry shim failed: {e}"),
                                        recoverable: false,
                                    }))
                                    .await;
                                return;
                            }
                        }
                    }
                    Err(err) => {
                        let msg = err.message();
                        warn!(
                            agent_id = %cfg.agent_id,
                            provider = %provider_slug,
                            error = %msg,
                            "ACP initialize failed"
                        );
                        kill_and_wait_shim(&mut shim).await;
                        let _ = evt_tx
                            .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                message: format!("{provider_name} ACP initialize failed: {msg}"),
                                recoverable: false,
                            }))
                            .await;
                        return;
                    }
                };

            // Per ACP specification, `loadSession` defaults to `false` when omitted.
            let supports_load_session = init_result
                .pointer("/agentCapabilities/loadSession")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            prompt_media_caps = AcpPromptMediaCaps::from_acp_initialize(&init_result);

            // 2. `session/load` or `session/new` (with fallback to fresh session + handoff context)
            let desired_resume = if supports_load_session {
                cfg.resume_session_id.as_deref()
            } else {
                if cfg.resume_session_id.is_some() {
                    inject_handoff_on_next_prompt = true;
                }
                None
            };

            let opened = match open_acp_session(
                &cfg,
                &mut shim,
                &mut next_rpc_id,
                &mut pending_rpcs,
                desired_resume,
                cfg.model.as_deref(),
                cfg.effort,
                &cfg.mode,
            )
            .await
            {
                Ok(s) => s,
                Err(first_err)
                    if desired_resume.is_some()
                        && matches!(first_err, BootRpcError::RpcError(_)) =>
                {
                    let first_msg = first_err.message();
                    warn!(
                        agent_id = %cfg.agent_id,
                        provider = %provider_slug,
                        old_session_id = ?desired_resume,
                        error = %first_msg,
                        "ACP session/load failed; transparently falling back to session/new with handoff context"
                    );
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
                            recovered_missing_session = true;
                            let _ = evt_tx
                                .send(DriverEvent::Notice {
                                    level: "info".into(),
                                    message: format!(
                                        "Previous {provider_name} session could not be loaded; started a fresh session with recent conversation context."
                                    ),
                                })
                                .await;
                            fresh
                        }
                        Err(e) => {
                            kill_and_wait_shim(&mut shim).await;
                            let _ = evt_tx
                                .send(DriverEvent::StatusChanged(AgentStatus::Error {
                                    message: format!(
                                        "{provider_name} session open failed: {first_msg} (fresh fallback: {})",
                                        e.message()
                                    ),
                                    recoverable: false,
                                }))
                                .await;
                            return;
                        }
                    }
                }
                Err(e) => {
                    kill_and_wait_shim(&mut shim).await;
                    let _ = evt_tx
                        .send(DriverEvent::StatusChanged(AgentStatus::Error {
                            message: format!(
                                "{provider_name} session open failed: {}",
                                e.message()
                            ),
                            recoverable: false,
                        }))
                        .await;
                    return;
                }
            };

            (
                opened.session_id,
                opened.current_model,
                opened.available_modes,
                opened.effort_config_option_id,
            )
        };

    let _ = evt_tx
        .send(DriverEvent::SessionInitialized {
            persistence_ref: session_id.clone(),
            model: current_model.clone(),
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
    let mut reattached_turn_in_flight = is_reattached && active_turn_id.is_some();
    let mut active_prompt_rpc_key: Option<String> = None;
    let mut non_prompt_rpcs: HashSet<String> = HashSet::new();
    let mut last_dispatched_prompt: Option<(String, String)> = None;
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
    let mut pending_prompts: VecDeque<(String, String)> = VecDeque::new();
    let mut pending_interrupt_replacement: Option<(String, String)> = None;
    let mut user_interrupted = false;
    let mut cancelling_active_turn = false;
    let mut cancel_deadline: Option<tokio::time::Instant> = None;

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
                        user_interrupted = false;
                        if active_prompt_rpc_key.is_some() || active_turn_id.is_some() {
                            if matches!(behavior, ActiveTurnBehavior::Queue) {
                                pending_prompts.push_back((turn_id, prompt));
                            } else if matches!(behavior, ActiveTurnBehavior::Interrupt) {
                                user_interrupted = true;
                                seg_state.finalize_all_messages(&evt_tx).await;
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
                                    finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                        .await;
                                    break;
                                }
                                pending_interrupt_replacement = Some((turn_id, prompt));
                            } else if !pending_perms.is_empty() {
                                let _ = evt_tx
                                    .send(DriverEvent::Notice {
                                        level: "warning".into(),
                                        message: "Waiting for your permission response above. Please approve, deny, or stop the current turn first.".into(),
                                    })
                                    .await;
                            } else {
                                // `ActiveTurnBehavior::Steer` on ACP providers without native mid-RPC steer:
                                // cancel active turn cleanly while preserving accepted FIFO `pending_prompts`.
                                user_interrupted = true;
                                seg_state.finalize_all_messages(&evt_tx).await;
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
                                    finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx)
                                        .await;
                                    break;
                                }
                                pending_interrupt_replacement = Some((turn_id, prompt));
                            }
                            continue;
                        }

                        cancelling_active_turn = false;
                        cancel_deadline = None;
                        let effective_prompt = if inject_handoff_on_next_prompt {
                            inject_handoff_on_next_prompt = false;
                            prepend_handoff_context(pending_handoff_context.take(), &prompt)
                        } else {
                            prompt
                        };
                        last_dispatched_prompt = Some((turn_id.clone(), effective_prompt.clone()));
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
                            turn_id,
                            effective_prompt,
                        )
                        .await
                        .is_err()
                        {
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                            break;
                        }
                    }
                    DriverCommand::Interrupt => {
                        user_interrupted = true;
                        pending_prompts.clear();
                        pending_interrupt_replacement = None;
                        seg_state.finalize_all_messages(&evt_tx).await;
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
                            finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                            break;
                        }
                    }
                    DriverCommand::SetMode { mode: new_mode } => {
                        mode = new_mode;
                        if let Some(mode_id) = resolve_acp_mode_id(&mode, &available_modes) {
                            let id = next_rpc_id;
                            next_rpc_id += 1;
                            non_prompt_rpcs.insert(RpcId::Num(id).as_key());
                            let req = json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "method": "session/set_mode",
                                "params": {
                                    "sessionId": session_id,
                                    "modeId": mode_id
                                }
                            })
                            .to_string();
                            if shim.write_stdin_line(req).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                        }
                    }
                    DriverCommand::SetConfig {
                        model,
                        effort,
                        fast_mode: _,
                    } => {
                        if let Some(m) = model
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| {
                                !s.is_empty()
                                    && !s.eq_ignore_ascii_case("default")
                                    && !s.eq_ignore_ascii_case("auto")
                            })
                        {
                            current_model = Some(m.to_string());
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
                            if shim.write_stdin_line(req).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                        }
                        if let (Some(opt_id), Some(eff)) =
                            (effort_config_option_id.as_deref(), effort)
                        {
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
                                    "value": acp_effort_wire_value(eff),
                                }
                            })
                            .to_string();
                            if shim.write_stdin_line(req).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
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
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                            emit_status_after_perm_resolve(
                                &active_turn_id,
                                &pending_perms,
                                &evt_tx,
                            )
                            .await;
                        }
                    }
                    DriverCommand::AnswerQuestion { request_id, answers } => {
                        if let Some(p) = pending_perms.remove(&request_id)
                            && p.is_question
                        {
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
                                        "optionId": p.allow_option_id,
                                        "updatedInput": { "answers": answers_map }
                                    }
                                }
                            })
                            .to_string();
                            if shim.write_stdin_line(resp).await.is_err() {
                                finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                                break;
                            }
                            emit_status_after_perm_resolve(
                                &active_turn_id,
                                &pending_perms,
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
                        let has_id = val.get("id").filter(|v| !v.is_null()).is_some();
                        let method = val.get("method").and_then(Value::as_str);

                        if has_id && method.is_none() {
                            if let Ok(rid) = serde_json::from_value::<RpcId>(val["id"].clone()) {
                                let key = rid.as_key();
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
                                    seg_state.finalize_all_messages(&evt_tx).await;
                                    active_turn_id = None;

                                    if let Some(err_obj) = val.get("error").filter(|v| !v.is_null()) {
                                        let err_msg = err_obj
                                            .get("message")
                                            .and_then(Value::as_str)
                                            .unwrap_or("");
                                        if !recovered_missing_session
                                            && is_missing_session_error(err_msg)
                                            && let Some((failed_turn_id, failed_prompt)) =
                                                last_dispatched_prompt.take()
                                        {
                                            recovered_missing_session = true;
                                            if let Ok(fresh) = open_acp_session(
                                                &cfg,
                                                &mut shim,
                                                &mut next_rpc_id,
                                                &mut pending_rpcs,
                                                None,
                                                current_model.as_deref(),
                                                cfg.effort,
                                                &mode,
                                            )
                                            .await
                                            {
                                                session_id = fresh.session_id;
                                                current_model = fresh.current_model;
                                                available_modes = fresh.available_modes;
                                                effort_config_option_id =
                                                    fresh.effort_config_option_id;
                                                let _ = evt_tx
                                                    .send(DriverEvent::SessionInitialized {
                                                        persistence_ref: session_id.clone(),
                                                        model: current_model.clone(),
                                                    })
                                                    .await;
                                                let _ = evt_tx
                                                    .send(DriverEvent::Notice {
                                                        level: "info".into(),
                                                        message: format!(
                                                            "Previous {provider_name} session expired; started a fresh session with conversation context."
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
                                                    failed_turn_id,
                                                    recovered_prompt,
                                                )
                                                .await
                                                .is_err()
                                                {
                                                    finalize_and_emit_unexpected_exit(
                                                        &mut seg_state,
                                                        &evt_tx,
                                                    )
                                                    .await;
                                                    break;
                                                }
                                                continue;
                                            }
                                        }
                                    }
                                    last_dispatched_prompt = None;

                                    if let Some((next_turn_id, next_prompt)) =
                                        pending_interrupt_replacement
                                            .take()
                                            .or_else(|| pending_prompts.pop_front())
                                    {
                                        user_interrupted = false;
                                        last_dispatched_prompt =
                                            Some((next_turn_id.clone(), next_prompt.clone()));
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
                                            next_turn_id,
                                            next_prompt,
                                        )
                                        .await
                                        .is_err()
                                        {
                                            finalize_and_emit_unexpected_exit(
                                                &mut seg_state,
                                                &evt_tx,
                                            )
                                            .await;
                                            break;
                                        }
                                    } else if let Some(err_obj) =
                                        val.get("error").filter(|v| !v.is_null())
                                        && !user_interrupted
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
                                        user_interrupted = false;
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
                                handle_acp_session_update(
                                    provider_slug,
                                    update,
                                    &mut mode,
                                    &mut seg_state,
                                    &mut tool_cache,
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
                        } else if has_id && let Some(unknown_method) = method {
                            debug!(
                                agent_id = %cfg.agent_id,
                                provider = %provider_slug,
                                method = %unknown_method,
                                "Replying -32601 to unsupported ACP server->client RPC"
                            );
                            let resp = json!({
                                "jsonrpc": "2.0",
                                "id": val["id"],
                                "error": {
                                    "code": -32601,
                                    "message": format!("Method not supported by client: {unknown_method}")
                                }
                            })
                            .to_string();
                            let _ = shim.write_stdin_line(resp).await;
                        }
                    }
                    ShimFrame::Exited { code } => {
                        seg_state.finalize_all_messages(&evt_tx).await;
                        if code != Some(0) {
                            warn!(
                                agent_id = %cfg.agent_id,
                                provider = %provider_slug,
                                exit_code = ?code,
                                "ACP provider shim process exited"
                            );
                        }
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
                active_turn_id = None;
                pending_perms.clear();
                seg_state.finalize_all_messages(&evt_tx).await;
                let _ = evt_tx
                    .send(DriverEvent::Notice {
                        level: "warning".into(),
                        message: format!(
                            "{provider_name} cancel timed out waiting for turn settlement; resetting active turn state."
                        ),
                    })
                    .await;
                if let Some((next_turn_id, next_prompt)) = pending_interrupt_replacement
                    .take()
                    .or_else(|| pending_prompts.pop_front())
                {
                    user_interrupted = false;
                    last_dispatched_prompt =
                        Some((next_turn_id.clone(), next_prompt.clone()));
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
                        next_turn_id,
                        next_prompt,
                    )
                    .await
                    .is_err()
                    {
                        finalize_and_emit_unexpected_exit(&mut seg_state, &evt_tx).await;
                        break;
                    }
                } else {
                    user_interrupted = false;
                    let _ = evt_tx
                        .send(DriverEvent::StatusChanged(AgentStatus::Idle))
                        .await;
                }
            }
        }
    }
}

fn extract_acp_content_text(update: &Value) -> String {
    if let Some(t) = update.pointer("/content/text").and_then(Value::as_str) {
        return t.to_string();
    }
    if let Some(t) = update.get("content").and_then(Value::as_str) {
        return t.to_string();
    }
    if let Some(arr) = update.get("content").and_then(Value::as_array) {
        let mut out = String::new();
        for item in arr {
            if item.get("type").and_then(Value::as_str) == Some("diff") {
                continue;
            }
            if let Some(t) = item
                .get("text")
                .or_else(|| item.pointer("/content/text"))
                .and_then(Value::as_str)
            {
                out.push_str(t);
            }
        }
        return out;
    }
    String::new()
}

async fn handle_acp_session_update(
    provider_slug: &str,
    update: &Value,
    mode: &mut SessionMode,
    seg_state: &mut AcpStreamSegmentState,
    tool_cache: &mut HashMap<String, ToolCallDetail>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let Some(kind) = update.get("sessionUpdate").and_then(Value::as_str) else {
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
            let chunk = extract_acp_content_text(update);
            if chunk.is_empty() {
                return;
            }
            seg_state.clear_active_thought();
            let mid = if let Some(explicit) = update
                .get("messageId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                let explicit_id = format!("{turn_tag}_{explicit}");
                if seg_state.active_msg_id.as_deref() != Some(explicit_id.as_str()) {
                    seg_state.finalize_active_message(evt_tx).await;
                    seg_state.active_msg_id = Some(explicit_id.clone());
                }
                explicit_id
            } else if let Some(ref cur) = seg_state.active_msg_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("{provider_slug}_msg_{turn_tag}_{}", seg_state.step_seq);
                seg_state.active_msg_id = Some(id.clone());
                id
            };
            let entry = seg_state.msg_accum.entry(mid.clone()).or_default();
            entry.push_str(&chunk);
            let _ = evt_tx
                .send(DriverEvent::AssistantText {
                    item_id: mid,
                    text: entry.clone(),
                    streaming: true,
                })
                .await;
        }
        "agent_thought_chunk" => {
            let chunk = extract_acp_content_text(update);
            if chunk.is_empty() {
                return;
            }
            seg_state.finalize_active_message(evt_tx).await;
            let tid = if let Some(explicit) = update
                .get("messageId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                let explicit_id = format!("{turn_tag}_th_{explicit}");
                if seg_state.active_thought_id.as_deref() != Some(explicit_id.as_str()) {
                    seg_state.clear_active_thought();
                    seg_state.active_thought_id = Some(explicit_id.clone());
                }
                explicit_id
            } else if let Some(ref cur) = seg_state.active_thought_id {
                cur.clone()
            } else {
                seg_state.step_seq += 1;
                let id = format!("{provider_slug}_th_{turn_tag}_{}", seg_state.step_seq);
                seg_state.active_thought_id = Some(id.clone());
                id
            };
            let entry = seg_state.thought_accum.entry(tid.clone()).or_default();
            entry.push_str(&chunk);
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
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
            else {
                return;
            };
            let status = match update.get("status").and_then(Value::as_str) {
                Some("completed") => ToolCallStatus::Completed,
                Some("failed" | "error" | "cancelled") => ToolCallStatus::Failed,
                Some("denied") => ToolCallStatus::Denied,
                _ => ToolCallStatus::Running,
            };
            seg_state.clear_active_thought();
            seg_state.finalize_active_message(evt_tx).await;
            let is_terminal = matches!(
                status,
                ToolCallStatus::Completed | ToolCallStatus::Failed | ToolCallStatus::Denied
            );
            let detail = if is_terminal {
                let prev = tool_cache.remove(&call_id);
                map_acp_tool_detail(update, prev.as_ref())
            } else {
                let mapped = map_acp_tool_detail(update, tool_cache.get(&call_id));
                tool_cache.insert(call_id.clone(), mapped.clone());
                mapped
            };
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
            if let Some(entries) = update.get("entries").and_then(Value::as_array) {
                let summary_lines: Vec<String> = entries
                    .iter()
                    .filter_map(|e| {
                        let content = e.get("content").and_then(Value::as_str)?.trim();
                        if content.is_empty() {
                            return None;
                        }
                        let st = e.get("status").and_then(Value::as_str).unwrap_or("pending");
                        let mark = match st {
                            "completed" => "[x]",
                            "in_progress" => "[~]",
                            _ => "[ ]",
                        };
                        Some(format!("{mark} {content}"))
                    })
                    .collect();
                if !summary_lines.is_empty() {
                    let _ = evt_tx
                        .send(DriverEvent::Notice {
                            level: "info".into(),
                            message: format!("Plan:\n{}", summary_lines.join("\n")),
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
                .filter(|s| !s.trim().is_empty())
                && !matches!(*mode, SessionMode::Auto)
            {
                *mode = parse_acp_mode_id(mode_id);
            }
        }
        _ => {}
    }
}

fn truncate_preview(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars).collect();
    out.push('…');
    out
}

fn extract_acp_tool_content_text(update: &Value) -> Option<String> {
    let arr = update.get("content").and_then(Value::as_array)?;
    let mut combined = String::new();
    for item in arr {
        let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
        if item_type == "diff" {
            continue;
        }
        let text = if item_type == "text" {
            item.get("text").and_then(Value::as_str)
        } else if item_type == "content" {
            item.pointer("/content/text").and_then(Value::as_str)
        } else {
            item.get("text")
                .or_else(|| item.pointer("/content/text"))
                .and_then(Value::as_str)
        };
        if let Some(t) = text
            && !t.is_empty()
        {
            if !combined.is_empty() {
                combined.push('\n');
            }
            combined.push_str(t);
        }
    }
    (!combined.trim().is_empty()).then(|| truncate_preview(&combined, 800))
}

fn infer_acp_kind_from_prev(prev: Option<&ToolCallDetail>) -> &'static str {
    match prev {
        Some(ToolCallDetail::Shell { .. }) => "execute",
        Some(ToolCallDetail::FileEdit { .. }) => "edit",
        Some(ToolCallDetail::FileRead { .. }) => "read",
        Some(ToolCallDetail::Search { .. }) => "search",
        _ => "",
    }
}

fn map_acp_tool_detail(update: &Value, prev: Option<&ToolCallDetail>) -> ToolCallDetail {
    let raw_kind = update
        .get("kind")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let kind = if raw_kind.is_empty() {
        infer_acp_kind_from_prev(prev)
    } else {
        raw_kind
    };
    let title = update
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let raw_in = update.get("rawInput");
    let raw_out = update.get("rawOutput");

    let out_preview = raw_out
        .and_then(|o| {
            if let Some(s) = o.as_str() {
                (!s.trim().is_empty()).then(|| truncate_preview(s, 800))
            } else {
                o.get("output")
                    .or_else(|| o.get("stdout"))
                    .or_else(|| o.get("content"))
                    .or_else(|| o.get("result"))
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| truncate_preview(s, 800))
            }
        })
        .or_else(|| extract_acp_tool_content_text(update));

    if kind == "execute"
        || raw_in
            .and_then(|i| {
                i.get("command")
                    .or_else(|| i.get("CommandLine"))
                    .or_else(|| i.get("cmd"))
            })
            .is_some()
    {
        let cmd = raw_in
            .and_then(|i| {
                i.get("command")
                    .or_else(|| i.get("CommandLine"))
                    .or_else(|| i.get("cmd"))
            })
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { command, .. }) => Some(command.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());
        let cwd = raw_in
            .and_then(|i| i.get("cwd").or_else(|| i.get("workdir")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Shell { cwd, .. }) => cwd.clone(),
                _ => None,
            });
        let exit_code = raw_out
            .and_then(|o| o.get("exitCode").or_else(|| o.get("exit_code")))
            .and_then(Value::as_i64)
            .and_then(|c| i32::try_from(c).ok())
            .or(match prev {
                Some(ToolCallDetail::Shell { exit_code, .. }) => *exit_code,
                _ => None,
            });
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

    if matches!(kind, "edit" | "write" | "delete" | "move") {
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
            .or_else(|| match prev {
                Some(ToolCallDetail::FileEdit { path, .. }) => Some(path.clone()),
                _ => None,
            })
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
            .or_else(|| match prev {
                Some(ToolCallDetail::FileEdit { unified_diff, .. }) => unified_diff.clone(),
                _ => None,
            });

        return ToolCallDetail::FileEdit {
            path,
            unified_diff: diff,
        };
    }

    if kind == "read" {
        let path = raw_in
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("Path"))
                    .or_else(|| i.get("file_path"))
                    .or_else(|| i.get("AbsolutePath"))
            })
            .and_then(Value::as_str)
            .or_else(|| update.pointer("/locations/0/path").and_then(Value::as_str))
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::FileRead { path, .. }) => Some(path.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());
        return ToolCallDetail::FileRead {
            path,
            start_line: None,
            end_line: None,
        };
    }

    if matches!(kind, "search" | "fetch") {
        let query = raw_in
            .and_then(|i| {
                i.get("query")
                    .or_else(|| i.get("pattern"))
                    .or_else(|| i.get("url"))
            })
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Search { query, .. }) => Some(query.clone()),
                _ => None,
            })
            .unwrap_or_else(|| title.to_string());
        let scope = raw_in
            .and_then(|i| i.get("path").or_else(|| i.get("scope")))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or_else(|| match prev {
                Some(ToolCallDetail::Search { scope, .. }) => scope.clone(),
                _ => None,
            });
        return ToolCallDetail::Search { query, scope };
    }

    if let Some(existing) = prev {
        return existing.clone();
    }

    ToolCallDetail::Generic {
        tool_name: title.to_string(),
        arguments_json: raw_in.map(Value::to_string).unwrap_or_default(),
    }
}

fn select_acp_option_id(options: &[Value], target_kind: &str, fallback_id: &str) -> String {
    for opt in options {
        if opt.get("kind").and_then(Value::as_str) == Some(target_kind)
            && let Some(id) = opt.get("optionId").and_then(Value::as_str)
        {
            return id.to_string();
        }
    }
    if target_kind == "reject_once" {
        for opt in options {
            if opt.get("kind").and_then(Value::as_str) == Some("reject_always")
                && let Some(id) = opt.get("optionId").and_then(Value::as_str)
            {
                return id.to_string();
            }
        }
    }
    for opt in options {
        if let Some(id) = opt.get("optionId").and_then(Value::as_str) {
            let lower = id.to_ascii_lowercase();
            let matched = if target_kind.starts_with("allow") {
                lower.contains("allow") || lower.contains("approve")
            } else {
                lower.contains("reject") || lower.contains("deny") || lower.contains("cancel")
            };
            if matched {
                return id.to_string();
            }
        }
    }
    fallback_id.to_string()
}

fn parse_acp_question_items(q_arr: &[Value]) -> Vec<QuestionItem> {
    q_arr
        .iter()
        .enumerate()
        .map(|(idx, q)| {
            let question = q
                .get("question")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            item.as_str()
                                .or_else(|| item.get("label").and_then(Value::as_str))
                                .or_else(|| item.get("value").and_then(Value::as_str))
                                .map(ToOwned::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            let is_multi_select = q
                .get("is_multi_select")
                .or_else(|| q.get("multiSelect"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let header = q
                .get("header")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|h| !h.is_empty() && *h != question.trim())
                .map(ToOwned::to_owned);
            QuestionItem {
                id: format!("q{idx}"),
                header,
                question,
                options,
                is_multi_select,
            }
        })
        .collect()
}

async fn handle_acp_permission_request(
    val: &Value,
    mode: &SessionMode,
    attachments_dir: Option<&Path>,
    active_turn_id: &Option<String>,
    shim: &ShimClient,
    pending_perms: &mut HashMap<String, PendingAcpPerm>,
    evt_tx: &mpsc::Sender<DriverEvent>,
) {
    let rpc_id = val["id"].clone();
    let params = &val["params"];
    let tool_call = &params["toolCall"];
    let options = params["options"].as_array().cloned().unwrap_or_default();

    let allow_opt = select_acp_option_id(&options, "allow_once", "allow");
    let allow_always_opt = select_acp_option_id(&options, "allow_always", &allow_opt);
    let deny_opt = select_acp_option_id(&options, "reject_once", "deny");

    if let Some(q_arr) = tool_call
        .pointer("/rawInput/questions")
        .and_then(Value::as_array)
        && !q_arr.is_empty()
    {
        let parsed_questions = parse_acp_question_items(q_arr);
        let req_id = format!("acp_q_{}", uuid::Uuid::new_v4().simple());
        pending_perms.insert(
            req_id.clone(),
            PendingAcpPerm {
                rpc_id,
                allow_option_id: allow_opt,
                allow_always_option_id: allow_always_opt,
                deny_option_id: deny_opt,
                is_question: true,
                raw_questions: parsed_questions.clone(),
            },
        );
        // Emit QuestionRequest BEFORE AwaitingPermission so listeners see the prompt payload first
        let _ = evt_tx
            .send(DriverEvent::QuestionRequest {
                request_id: req_id.clone(),
                questions: parsed_questions,
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

    let kind_str = tool_call.get("kind").and_then(Value::as_str).unwrap_or("");
    let is_attachment_read = kind_str == "read"
        && attachments_dir.is_some_and(|dir| {
            tool_call
                .get("rawInput")
                .and_then(|i| {
                    i.get("path")
                        .or_else(|| i.get("Path"))
                        .or_else(|| i.get("AbsolutePath"))
                })
                .and_then(Value::as_str)
                .or_else(|| {
                    tool_call
                        .pointer("/locations/0/path")
                        .and_then(Value::as_str)
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

    let req_id = format!("acp_perm_{}", uuid::Uuid::new_v4().simple());
    let title = tool_call
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Approve tool execution")
        .to_string();
    let detail = map_acp_tool_detail(tool_call, None);

    pending_perms.insert(
        req_id.clone(),
        PendingAcpPerm {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::OrbitMcpLaunchSpec;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicU64;

    #[test]
    fn acp_spawn_args_match_all_nine_providers_including_vibe_fallback() {
        assert_eq!(
            acp_spawn_args(ProviderKind::OpenCode, Path::new("/usr/local/bin/opencode")),
            vec!["acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::GeminiCli, Path::new("/usr/local/bin/gemini")),
            vec!["--experimental-acp"]
        );
        assert_eq!(
            acp_spawn_args(
                ProviderKind::CursorAgent,
                Path::new("/usr/local/bin/cursor-agent")
            ),
            vec!["acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::Copilot, Path::new("/usr/local/bin/copilot")),
            vec!["--acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::Goose, Path::new("/usr/local/bin/goose")),
            vec!["acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::Kimi, Path::new("/usr/local/bin/kimi")),
            vec!["acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::Cline, Path::new("/usr/local/bin/cline")),
            vec!["--acp"]
        );
        assert!(
            acp_spawn_args(
                ProviderKind::MistralVibe,
                Path::new("/usr/local/bin/vibe-acp")
            )
            .is_empty()
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::MistralVibe, Path::new("/usr/local/bin/vibe")),
            vec!["--acp"]
        );
        assert_eq!(
            acp_spawn_args(ProviderKind::Augment, Path::new("/usr/local/bin/auggie")),
            vec!["--acp"]
        );
    }

    #[test]
    fn missing_session_classifier_rejects_generic_not_found_and_partial_tool_updates_preserve_kind()
    {
        // P0 #5: never treat file/model not-found errors as missing session
        assert!(is_missing_session_error("session not found: sess_123"));
        assert!(is_missing_session_error("Trajectory does not exist"));
        assert!(is_missing_session_error("Unknown conversation id"));
        assert!(!is_missing_session_error("file not found: src/main.rs"));
        assert!(!is_missing_session_error("model not found: gpt-99"));
        assert!(!is_missing_session_error("method not found (-32601)"));

        // P0 #4: partial tool_call_update WITHOUT `kind` updates FileEdit diff and Shell output cleanly
        let initial_edit = map_acp_tool_detail(
            &json!({
                "toolCallId": "tc_1",
                "kind": "edit",
                "title": "Edit Cargo.toml",
                "rawInput": { "path": "Cargo.toml" }
            }),
            None,
        );
        let patched_edit = map_acp_tool_detail(
            &json!({
                "toolCallId": "tc_1",
                "status": "completed",
                "content": [{ "type": "diff", "oldText": "v1\n", "newText": "v2\n" }]
            }),
            Some(&initial_edit),
        );
        assert!(matches!(
            patched_edit,
            ToolCallDetail::FileEdit {
                ref path,
                unified_diff: Some(ref diff),
            } if path == "Cargo.toml" && diff.contains("+v2")
        ));

        let initial_shell = map_acp_tool_detail(
            &json!({
                "toolCallId": "tc_2",
                "kind": "execute",
                "title": "Run tests",
                "rawInput": { "command": "cargo test" }
            }),
            None,
        );
        let patched_shell = map_acp_tool_detail(
            &json!({
                "toolCallId": "tc_2",
                "status": "completed",
                "content": [{ "type": "content", "content": { "type": "text", "text": "test result: ok." } }],
                "rawOutput": { "exitCode": 0 }
            }),
            Some(&initial_shell),
        );
        assert!(matches!(
            patched_shell,
            ToolCallDetail::Shell {
                ref command,
                exit_code: Some(0),
                output_preview: Some(ref preview),
                ..
            } if command == "cargo test" && preview.contains("test result: ok.")
        ));

        // P1 #13: reject_always and deny/cancel fallback option selection
        let opts = vec![
            json!({"optionId": "allow_1", "kind": "allow_once"}),
            json!({"optionId": "deny_all", "kind": "reject_always"}),
        ];
        assert_eq!(
            select_acp_option_id(&opts, "reject_once", "deny"),
            "deny_all"
        );
        let cancel_opts = vec![json!({"optionId": "cancel_action"})];
        assert_eq!(
            select_acp_option_id(&cancel_opts, "reject_once", "deny"),
            "cancel_action"
        );
    }

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
        for _ in 0..400 {
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
    fn make_acp_cfg(
        provider: ProviderKind,
        agent_id: &str,
        script: &Path,
        sock_path: &Path,
        cwd: &Path,
        seq_tracker: Arc<AtomicU64>,
    ) -> SessionLaunchConfig {
        SessionLaunchConfig {
            agent_id: agent_id.to_string(),
            provider,
            executable: script.to_path_buf(),
            orbit_exe: PathBuf::from("/bin/true"),
            shim_socket: sock_path.to_path_buf(),
            runtime_dir: cwd.to_path_buf(),
            cwd: cwd.to_path_buf(),
            extra_dirs: vec![],
            attachments_dir: None,
            model: Some("claude-sonnet-4-5".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            resume_session_id: None,
            fork_from_session_id: None,
            native_fork_only: false,
            mcp_spec: Some(OrbitMcpLaunchSpec {
                server_name: "orbit".into(),
                http_url: "http://127.0.0.1:6868/mcp/agents".into(),
                bearer_token: "tok".into(),
                stdio_command: PathBuf::from("/usr/local/bin/orbit"),
                stdio_args: vec!["mcp".into()],
            }),
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

    #[cfg(unix)]
    #[tokio::test]
    async fn acp_driver_handles_init_mcp_fallback_updates_permissions_and_cancel_over_shim() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_acp_test_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let log_file = tmp.join("acp_received.log");
        let sock_path = tmp.join("acp.sock");

        // Mock POSIX ACP JSON-RPC 2.0 server:
        // - Sends a server->client RPC during boot (must get -32601 instead of deadlocking)
        // - Rejects HTTP MCP in session/new, accepts stdio fallback
        // - Sends partial tool_call_update WITHOUT `kind` carrying a diff block
        // - Streams agent_thought_chunk, plan, current_mode_update, session/request_permission,
        //   explicit messageId agent_message_chunk, and handles session/cancel cleanly
        let mock_script = write_mock_script(
            &tmp,
            "mock_acp.sh",
            &format!(
                r#"#!/bin/sh
LOG="{}"
extract_id() {{
  printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'
}}
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$LOG"
  id=$(extract_id "$line")
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","id":777,"method":"fs/read_text_file","params":{{"path":"README.md"}}}}'
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"protocolVersion\":1,\"agentCapabilities\":{{\"loadSession\":true}}}}}}"
      ;;
    *'"method":"session/new"'*'"type":"http"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{{\"code\":-32602,\"message\":\"HTTP MCP not supported\"}}}}"
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"sessionId\":\"acp-sess-1\",\"models\":{{\"currentModelId\":\"claude-sonnet-4-5\"}},\"modes\":{{\"availableModes\":[{{\"id\":\"default\"}},{{\"id\":\"accept-edits\"}}]}}}}}}"
      ;;
    *'"method":"session/set_model"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{}}}}"
      ;;
    *'"method":"session/prompt"'*'"text":"edit and run"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"Analyzing files"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"plan","entries":[{{"content":"Inspect lib.rs","status":"completed"}},{{"content":"Run tests","status":"in_progress"}}]}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"current_mode_update","currentModeId":"accept-edits"}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"tool_call","toolCallId":"tc_edit","kind":"edit","title":"Edit src/lib.rs","status":"in_progress","rawInput":{{"path":"src/lib.rs"}}}}}}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"tool_call_update","toolCallId":"tc_edit","status":"completed","content":[{{"type":"diff","oldText":"old\n","newText":"new\n"}}]}} }}}}'
      printf '%s\n' '{{"jsonrpc":"2.0","id":9001,"method":"session/request_permission","params":{{"sessionId":"acp-sess-1","toolCall":{{"toolCallId":"tc_cmd","kind":"execute","title":"Run cargo test","rawInput":{{"command":"cargo test"}}}},"options":[{{"optionId":"opt_allow_once","kind":"allow_once"}},{{"optionId":"opt_allow_always","kind":"allow_always"}},{{"optionId":"opt_reject","kind":"reject_once"}}]}} }}'
      PROMPT_ID="$id"
      ;;
    *'"id":9001'*'"opt_allow_always"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{{"type":"text","text":"All done via ACP!"}}}}}}}}'
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$PROMPT_ID,\"result\":{{\"stopReason\":\"end_turn\"}}}}"
      ;;
    *'"method":"session/prompt"'*'"text":"slow turn"'*)
      SLOW_ID="$id"
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-sess-1","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Starting..."}}}}}}}}'
      ;;
    *'"method":"session/cancel"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$SLOW_ID,\"result\":{{\"stopReason\":\"cancelled\"}}}}"
      ;;
  esac
done
"#,
                log_file.display()
            ),
        );

        let seq_tracker = Arc::new(AtomicU64::new(0));
        let cfg = make_acp_cfg(
            ProviderKind::OpenCode,
            "ag_acp_1",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker),
        );
        let spec = build_acp_spawn_spec(&cfg, None);
        let (shim_task, shim_client) = start_inprocess_shim(sock_path.clone(), spec).await;

        let (cmd_tx, cmd_rx) = mpsc::channel(32);
        let (evt_tx, mut evt_rx) = mpsc::channel(128);
        let actor = tokio::spawn(run_acp_actor(cfg, shim_client, false, cmd_rx, evt_tx));

        let mut init_sid = None;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            match ev {
                DriverEvent::SessionInitialized {
                    persistence_ref, ..
                } => init_sid = Some(persistence_ref),
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert_eq!(init_sid.as_deref(), Some("acp-sess-1"));

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_acp_1".into(),
                prompt: "edit and run".into(),
                behavior: ActiveTurnBehavior::Steer,
            })
            .await
            .expect("send prompt");

        let mut saw_thought = false;
        let mut saw_plan_notice = false;
        let mut saw_edit_diff = false;
        let mut perm_req_id = None;
        let mut final_reply = String::new();

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn 1 timeout")
                .expect("ev");
            match ev {
                DriverEvent::ThinkingText { text, .. } if text == "Analyzing files" => {
                    saw_thought = true;
                }
                DriverEvent::Notice { message, .. } if message.contains("Inspect lib.rs") => {
                    saw_plan_notice = true;
                }
                DriverEvent::ToolCall {
                    call_id,
                    status: ToolCallStatus::Completed,
                    detail: ToolCallDetail::FileEdit { path, unified_diff },
                    ..
                } if call_id == "tc_edit" => {
                    assert_eq!(path, "src/lib.rs");
                    assert!(unified_diff.as_deref().unwrap_or("").contains("+new"));
                    saw_edit_diff = true;
                }
                DriverEvent::PermissionRequest {
                    request_id,
                    title,
                    detail: ToolCallDetail::Shell { command, .. },
                    ..
                } => {
                    assert_eq!(title, "Run cargo test");
                    assert_eq!(command, "cargo test");
                    perm_req_id = Some(request_id.clone());
                    cmd_tx
                        .send(DriverCommand::ResolvePermission {
                            request_id,
                            decision: "allow_always".into(),
                        })
                        .await
                        .expect("resolve permission");
                }
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } => {
                    final_reply = text;
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }

        assert!(saw_thought);
        assert!(saw_plan_notice);
        assert!(
            saw_edit_diff,
            "partial tool_call_update without `kind` must still attach diff to cached FileEdit"
        );
        assert!(perm_req_id.is_some());
        assert_eq!(final_reply, "All done via ACP!");

        // Cancel in-flight turn via DriverCommand::Interrupt
        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_acp_2".into(),
                prompt: "slow turn".into(),
                behavior: ActiveTurnBehavior::Steer,
            })
            .await
            .expect("send slow turn");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("slow start")
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
            .send(DriverCommand::Interrupt)
            .await
            .expect("send cancel");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("cancel settle")
                .expect("ev");
            if matches!(ev, DriverEvent::StatusChanged(AgentStatus::Idle)) {
                break;
            }
        }

        let recorded = std::fs::read_to_string(&log_file).expect("read log");
        assert!(recorded.contains("-32601"));
        assert!(recorded.contains("\"method\":\"session/cancel\""));
        assert!(recorded.contains("\"optionId\":\"opt_allow_always\""));
        assert!(seq_tracker.load(Ordering::Relaxed) >= 8);

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn acp_session_load_failure_falls_back_to_fresh_session_with_handoff_context() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_acp_handoff_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let prompt_log = tmp.join("prompt.log");
        let sock_path = tmp.join("acp_handoff.sock");

        let mock_script = write_mock_script(
            &tmp,
            "mock_acp_handoff.sh",
            &format!(
                r#"#!/bin/sh
LOG="{}"
extract_id() {{
  printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'
}}
while IFS= read -r line; do
  id=$(extract_id "$line")
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"protocolVersion\":1,\"agentCapabilities\":{{\"loadSession\":true}}}}}}"
      ;;
    *'"method":"session/load"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"error\":{{\"code\":-32000,\"message\":\"session not found\"}}}}"
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"sessionId\":\"acp-fresh-77\"}}}}"
      ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' "$line" >> "$LOG"
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-fresh-77","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Continued with handoff!"}}}}}}}}'
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"stopReason\":\"end_turn\"}}}}"
      ;;
  esac
done
"#,
                prompt_log.display()
            ),
        );

        let seq_tracker = Arc::new(AtomicU64::new(0));
        let mut cfg = make_acp_cfg(
            ProviderKind::GeminiCli,
            "ag_acp_handoff",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker),
        );
        cfg.mcp_spec = None;
        cfg.model = None;
        cfg.resume_session_id = Some("expired-sess-1".into());
        cfg.fallback_handoff_context =
            Some("[Prior Conversation Context in this Workspace]\nUser: Build ACP driver".into());

        let spec = build_acp_spawn_spec(&cfg, None);
        let (shim_task, shim_client) = start_inprocess_shim(sock_path.clone(), spec).await;
        let (cmd_tx, cmd_rx) = mpsc::channel(16);
        let (evt_tx, mut evt_rx) = mpsc::channel(64);
        let actor = tokio::spawn(run_acp_actor(cfg, shim_client, false, cmd_rx, evt_tx));

        let mut saw_notice = false;
        let mut fresh_ref = None;
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("boot timeout")
                .expect("ev");
            match ev {
                DriverEvent::Notice { level, message }
                    if level == "info" && message.contains("fresh session") =>
                {
                    saw_notice = true;
                }
                DriverEvent::SessionInitialized {
                    persistence_ref, ..
                } => fresh_ref = Some(persistence_ref),
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert!(saw_notice);
        assert_eq!(fresh_ref.as_deref(), Some("acp-fresh-77"));

        cmd_tx
            .send(DriverCommand::SendPrompt {
                turn_id: "turn_handoff_1".into(),
                prompt: "Continue tests".into(),
                behavior: ActiveTurnBehavior::Steer,
            })
            .await
            .expect("send prompt");

        let mut reply = String::new();
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv())
                .await
                .expect("turn timeout")
                .expect("ev");
            match ev {
                DriverEvent::AssistantText {
                    text,
                    streaming: false,
                    ..
                } => reply = text,
                DriverEvent::StatusChanged(AgentStatus::Idle) => break,
                _ => {}
            }
        }
        assert_eq!(reply, "Continued with handoff!");

        let prompt_sent = std::fs::read_to_string(&prompt_log).expect("read prompt log");
        assert!(prompt_sent.contains("[Prior Conversation Context in this Workspace]"));
        assert!(prompt_sent.contains("Continue tests"));

        let _ = cmd_tx.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn acp_reattach_resumes_inflight_turn_without_duplicate_frames() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_acp_reattach_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let sock_path = tmp.join("acp_reattach.sock");
        let trigger_file = tmp.join("continue.flag");

        let mock_script = write_mock_script(
            &tmp,
            "mock_acp_reattach.sh",
            &format!(
                r#"#!/bin/sh
FLAG="{}"
extract_id() {{
  printf '%s' "$1" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p'
}}
while IFS= read -r line; do
  id=$(extract_id "$line")
  case "$line" in
    *'"method":"initialize"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"protocolVersion\":1}}}}"
      ;;
    *'"method":"session/new"'*)
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"sessionId\":\"acp-live-88\"}}}}"
      ;;
    *'"method":"session/prompt"'*)
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-live-88","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Pre-restart chunk"}}}}}}}}'
      while [ ! -f "$FLAG" ]; do
        sleep 0.05
      done
      printf '%s\n' '{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"acp-live-88","update":{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"Post-restart chunk"}}}}}}}}'
      printf '%s\n' "{{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{{\"stopReason\":\"end_turn\"}}}}"
      ;;
  esac
done
"#,
                trigger_file.display()
            ),
        );

        let seq_tracker_1 = Arc::new(AtomicU64::new(0));
        let mut cfg1 = make_acp_cfg(
            ProviderKind::Goose,
            "ag_acp_reattach",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker_1),
        );
        cfg1.mcp_spec = None;
        cfg1.model = None;

        let spec = build_acp_spawn_spec(&cfg1, None);
        let (shim_task, shim_client_1) = start_inprocess_shim(sock_path.clone(), spec).await;
        let (cmd_tx1, cmd_rx1) = mpsc::channel(16);
        let (evt_tx1, mut evt_rx1) = mpsc::channel(64);
        let actor1 = tokio::spawn(run_acp_actor(cfg1, shim_client_1, false, cmd_rx1, evt_tx1));

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
                turn_id: "turn_goose_1".into(),
                prompt: "work".into(),
                behavior: ActiveTurnBehavior::Steer,
            })
            .await
            .expect("send prompt");

        loop {
            let ev = tokio::time::timeout(Duration::from_secs(3), evt_rx1.recv())
                .await
                .expect("pre chunk")
                .expect("ev");
            if let DriverEvent::AssistantText { text, .. } = ev
                && text == "Pre-restart chunk"
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
            .expect("reattach shim");
        let seq_tracker_2 = Arc::new(AtomicU64::new(saved_seq));
        let mut cfg2 = make_acp_cfg(
            ProviderKind::Goose,
            "ag_acp_reattach",
            &mock_script,
            &sock_path,
            &tmp,
            Arc::clone(&seq_tracker_2),
        );
        cfg2.mcp_spec = None;
        cfg2.model = None;
        cfg2.reattach_existing_shim = true;
        cfg2.shim_after_seq = Some(saved_seq);
        cfg2.resume_session_id = Some("acp-live-88".into());
        cfg2.persisted_active_turn_id = Some("turn_goose_1".into());
        cfg2.persisted_turn_step_seq = 4;

        let (cmd_tx2, cmd_rx2) = mpsc::channel(16);
        let (evt_tx2, mut evt_rx2) = mpsc::channel(64);
        let actor2 = tokio::spawn(run_acp_actor(cfg2, shim_client_2, true, cmd_rx2, evt_tx2));

        std::fs::write(&trigger_file, b"go").expect("trigger");

        let mut saw_dup = false;
        let mut final_id = None;
        let mut final_text = None;
        let mut settled = false;
        while let Ok(Some(ev)) = tokio::time::timeout(Duration::from_secs(3), evt_rx2.recv()).await
        {
            match ev {
                DriverEvent::AssistantText {
                    item_id,
                    text,
                    streaming,
                } => {
                    if text.contains("Pre-restart chunk") {
                        saw_dup = true;
                    }
                    if !streaming {
                        final_id = Some(item_id);
                        final_text = Some(text);
                    }
                }
                DriverEvent::StatusChanged(AgentStatus::Idle) => {
                    settled = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(!saw_dup, "reattach must skip pre-restart frames");
        assert_eq!(final_id.as_deref(), Some("goose_msg_turn_goose_1_5"));
        assert_eq!(final_text.as_deref(), Some("Post-restart chunk"));
        assert!(settled);
        assert!(seq_tracker_2.load(Ordering::Relaxed) > saved_seq);

        let _ = cmd_tx2.send(DriverCommand::Shutdown).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), actor2).await;
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn current_mode_update_preserves_guardian_auto_mode() {
        let mut mode = SessionMode::Auto;
        let mut seg_state = AcpStreamSegmentState::default();
        let mut tool_cache = HashMap::new();
        let (evt_tx, _evt_rx) = mpsc::channel(8);

        for mode_id in ["default", "code", "accept-edits", "bypassPermissions"] {
            let update = json!({
                "sessionUpdate": "current_mode_update",
                "currentModeId": mode_id,
            });
            handle_acp_session_update(
                "opencode",
                &update,
                &mut mode,
                &mut seg_state,
                &mut tool_cache,
                &evt_tx,
            )
            .await;
            assert_eq!(
                mode,
                SessionMode::Auto,
                "ACP server mode echo `{mode_id}` must never overwrite SessionMode::Auto"
            );
        }
    }
}
