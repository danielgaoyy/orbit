//! Transport-neutral Orbit Orchestration Tool Catalog.
//!
//! Exposed to agents via the built-in MCP Server (`POST /mcp/agents`) and `orbit mcp` stdio bridge.
//! Exposes a compact Core Pack (11 tools) plus startup-locked optional Capability Packs (up to 18
//! total composite tools in `tools/list`) to minimize LLM context overhead and preserve prompt cache
//! prefixes, while keeping 100% backward compatibility in `execute_tool` for all legacy tool names.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, ArtifactKind, BestOfNCandidateSpec, BrowserActionRequest,
    DagStepSpec, ForkTarget, McpToolPack, McpToolPolicy, MonitorKind, ProviderKind,
    ReasoningEffort, ScheduleNewAgentConfig, ScheduleStatus, ScheduleTarget, SessionMode,
    TimelinePayload, ToolCallDetail, WireMessage, WorkspaceIsolation, WorktreeCreateMode,
    WorktreeHandoffDirection, WorktreeHandoffMode,
};
use orbit_store::{ReceiptOutcome, UpsertArtifactInput};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::DaemonRuntime;
use crate::browser::CreateBrowserSessionSpec;
use crate::goals::{GoalCriterionPatch, StartGoalSpec, StartMonitorSpec, authorize_agent_control};
use crate::manager::{
    CreateAgentSpec, CreateWorkspaceSpec, ExecuteWorktreeHandoffSpec, ForkAgentSpec,
};
use crate::messaging::{AgentPromptSource, QueuedAgentMessage};
use crate::workflows::{
    StartBatchFanoutSpec, StartBestOfNSpec, StartDagWorkflowSpec, authorize_workflow_cancel,
    authorize_workflow_launch_from_caller, clamp_workflow_session_mode,
    validate_mcp_verify_command,
};

pub const LABEL_MCP_PACKS: &str = "orbit.mcp_packs";

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

#[derive(Debug, Clone)]
struct PackToolDefinition {
    pack: McpToolPack,
    def: ToolDefinition,
}

#[derive(Debug, Clone)]
pub struct ToolExecResult {
    pub is_error: bool,
    pub structured: Value,
    pub text: String,
}

impl ToolExecResult {
    pub fn ok(structured: Value) -> Self {
        let text = serde_json::to_string_pretty(&structured).unwrap_or_else(|_| "{}".into());
        Self {
            is_error: false,
            structured,
            text,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        let msg = message.into();
        Self {
            is_error: true,
            structured: json!({ "error": msg }),
            text: msg,
        }
    }
}

/// Fast deterministic check whether an initial prompt requests browser/web interaction.
pub fn prompt_mentions_browser(prompt: &str) -> bool {
    let lower = prompt.to_ascii_lowercase();
    [
        "browser",
        "screenshot",
        "webpage",
        "e2e",
        "playwright",
        "cdp",
        "dom ",
        "浏览器",
        "截图",
        "网页",
        "打开页面",
    ]
    .iter()
    .any(|kw| lower.contains(kw) || prompt.contains(kw))
}

/// Determine which MCP capability packs should be locked for a newly launched agent session.
///
/// The returned pack list is persisted on the agent at launch and kept immutable for the lifetime
/// of the session so `tools/list` never mutates mid-conversation (protecting LLM prompt caches).
pub fn detect_startup_mcp_packs(
    initial_prompt: &str,
    is_subagent: bool,
    has_browser_binary: bool,
) -> Vec<McpToolPack> {
    let mut packs = vec![McpToolPack::Core];
    let lower = initial_prompt.to_ascii_lowercase();
    let has_any = |kws: &[&str]| {
        kws.iter()
            .any(|kw| lower.contains(kw) || initial_prompt.contains(kw))
    };

    if has_browser_binary && prompt_mentions_browser(initial_prompt) {
        packs.push(McpToolPack::Browser);
    }
    if has_any(&[
        "cron",
        "schedule",
        "heartbeat",
        "recurring",
        "定时",
        "每天",
        "每小时",
        "周期",
    ]) {
        packs.push(McpToolPack::Schedules);
    }
    if !is_subagent
        && has_any(&[
            "batch",
            "csv",
            "fanout",
            "dag",
            "workflow",
            "best-of-n",
            "best_of_n",
            "arena",
            "工作流",
            "流水线",
            "批量",
        ])
    {
        packs.push(McpToolPack::Workflows);
    }
    if has_any(&[
        "/goal",
        "autonomous goal",
        "start_goal",
        "monitor",
        "log_file_tail",
        "http_health",
        "健康检查",
        "监控",
    ]) {
        packs.push(McpToolPack::Goals);
    }
    if has_any(&[
        "pty",
        "detached terminal",
        "interactive terminal",
        "离屏终端",
        "虚拟终端",
    ]) {
        packs.push(McpToolPack::Terminals);
    }
    if has_any(&[
        "artifact",
        "mermaid",
        "svg",
        "mcp_app",
        "mcp app",
        "可视化产物",
        "交互图表",
    ]) {
        packs.push(McpToolPack::Artifacts);
    }
    if has_any(&[
        "lsp",
        "goto definition",
        "go to definition",
        "workspace symbol",
        "diagnostics",
        "语言服务",
        "符号跳转",
    ]) {
        packs.push(McpToolPack::Lsp);
    }
    packs
}

pub fn format_mcp_packs_label(packs: &[McpToolPack]) -> String {
    packs.iter().map(|p| p.slug()).collect::<Vec<_>>().join(",")
}

pub fn parse_mcp_packs_label(raw: &str) -> Vec<McpToolPack> {
    let mut packs = Vec::new();
    for part in raw.split(',') {
        if let Some(p) = McpToolPack::parse_slug(part)
            && !packs.contains(&p)
        {
            packs.push(p);
        }
    }
    if !packs.contains(&McpToolPack::Core) {
        packs.insert(0, McpToolPack::Core);
    }
    packs
}

/// Resolve the effective `McpToolPolicy` for an optional calling agent.
pub fn resolve_caller_policy(
    runtime: &DaemonRuntime,
    caller_agent_id: Option<&str>,
) -> McpToolPolicy {
    let Some(cid) = caller_agent_id else {
        return McpToolPolicy::default();
    };
    let Some(agent) = runtime.store.get_agent(cid).ok().flatten() else {
        return McpToolPolicy::default();
    };
    let Some(pid) = agent.profile_id.as_deref() else {
        return McpToolPolicy::default();
    };
    runtime
        .store
        .get_profile(pid)
        .ok()
        .flatten()
        .map(|p| p.mcp_policy)
        .unwrap_or_default()
}

pub fn resolve_effective_packs(
    runtime: &DaemonRuntime,
    caller_agent_id: Option<&str>,
    policy: &McpToolPolicy,
) -> Vec<McpToolPack> {
    if !policy.enabled {
        return Vec::new();
    }
    let base: Vec<McpToolPack> = if let Some(ref explicit) = policy.enabled_packs {
        explicit.clone()
    } else if let Some(cid) = caller_agent_id {
        if let Some(agent) = runtime.store.get_agent(cid).ok().flatten() {
            if let Some(raw_packs) = agent.labels.get(LABEL_MCP_PACKS) {
                parse_mcp_packs_label(raw_packs)
            } else {
                vec![McpToolPack::Core]
            }
        } else {
            vec![McpToolPack::Core]
        }
    } else {
        // External MCP clients (no caller_agent_id) see the full 18-tool catalog unless disabled.
        McpToolPack::ALL.to_vec()
    };

    let mut out = Vec::with_capacity(base.len());
    for pack in base {
        if !policy.disabled_packs.contains(&pack) && !out.contains(&pack) {
            out.push(pack);
        }
    }
    out
}

pub fn list_tool_definitions(policy: &McpToolPolicy) -> Vec<ToolDefinition> {
    if !policy.enabled {
        return Vec::new();
    }
    let allowed_packs: Vec<McpToolPack> = policy
        .enabled_packs
        .as_deref()
        .unwrap_or(&McpToolPack::ALL)
        .iter()
        .copied()
        .filter(|p| !policy.disabled_packs.contains(p))
        .collect();
    pack_tool_definitions()
        .into_iter()
        .filter(|entry| {
            allowed_packs.contains(&entry.pack)
                && !policy.disabled_tools.iter().any(|d| d == entry.def.name)
        })
        .map(|entry| entry.def)
        .collect()
}

pub fn list_tool_definitions_for_caller(
    runtime: &DaemonRuntime,
    caller_agent_id: Option<&str>,
    policy: &McpToolPolicy,
) -> Vec<ToolDefinition> {
    if !policy.enabled {
        return Vec::new();
    }
    let packs = resolve_effective_packs(runtime, caller_agent_id, policy);
    pack_tool_definitions()
        .into_iter()
        .filter(|entry| {
            packs.contains(&entry.pack)
                && !policy.disabled_tools.iter().any(|d| d == entry.def.name)
        })
        .map(|entry| entry.def)
        .collect()
}

fn tool_pack_for_canonical_name(canonical_name: &str) -> McpToolPack {
    match canonical_name {
        "browser_open"
        | "browser_snapshot"
        | "browser_screenshot"
        | "browser_click"
        | "browser_type"
        | "browser_scroll"
        | "browser_press_key"
        | "browser_select_option"
        | "browser_upload_file"
        | "browser_pick_element"
        | "browser_close"
        | "orbit_browser" => McpToolPack::Browser,
        "create_schedule" | "list_schedules" | "inspect_schedule" | "pause_schedule"
        | "resume_schedule" | "update_schedule" | "schedule_logs" | "run_schedule_once"
        | "delete_schedule" | "create_heartbeat" | "delete_heartbeat" | "orbit_schedule" => {
            McpToolPack::Schedules
        }
        "run_batch_fanout"
        | "run_dag_workflow"
        | "run_best_of_n"
        | "get_workflow_status"
        | "cancel_workflow"
        | "orbit_workflow" => McpToolPack::Workflows,
        "start_goal"
        | "update_goal_progress"
        | "complete_goal"
        | "start_monitor"
        | "stop_monitor"
        | "orbit_goal" => McpToolPack::Goals,
        "list_terminals" | "create_terminal" | "capture_terminal" | "send_terminal_keys"
        | "kill_terminal" | "orbit_terminal" => McpToolPack::Terminals,
        "publish_artifact" | "list_artifacts" | "read_artifact" | "delete_artifact"
        | "orbit_artifact" => McpToolPack::Artifacts,
        "lsp_diagnostics" | "lsp_symbols" | "lsp_goto_definition" | "orbit_lsp" => McpToolPack::Lsp,
        _ => McpToolPack::Core,
    }
}

fn resolve_composite_dispatch(name: &str, args: &Value) -> anyhow::Result<&'static str> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_ascii_lowercase());
    let req_action = || {
        action
            .as_deref()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("missing required parameter `action` for `{name}`"))
    };

    match name {
        "workspace_peers" => {
            if opt_str(args, "handle").is_some() {
                Ok("read_peer_context")
            } else {
                Ok("workspace_peers")
            }
        }
        "workspace_board" => {
            let act = action.as_deref().unwrap_or_else(|| {
                if args.get("content").and_then(|v| v.as_str()).is_some() {
                    "update"
                } else {
                    "read"
                }
            });
            match act {
                "read" => Ok("workspace_board_read"),
                "update" | "write" => Ok("workspace_board_update"),
                other => anyhow::bail!("unsupported `workspace_board` action `{other}`"),
            }
        }
        "orbit_agent" => match req_action()? {
            "list" => Ok("list_agents"),
            "status" => Ok("get_agent_status"),
            "activity" => Ok("get_agent_activity"),
            "fork" => Ok("fork_agent"),
            "update" => Ok("update_agent"),
            "set_mode" | "mode" => Ok("set_agent_mode"),
            "cancel" | "interrupt" => Ok("cancel_agent"),
            "archive" => Ok("archive_agent"),
            "kill" | "delete" => Ok("kill_agent"),
            "move_workspace" | "move" => Ok("move_agent_to_workspace"),
            other => anyhow::bail!("unsupported `orbit_agent` action `{other}`"),
        },
        "orbit_workspace" => match req_action()? {
            "create" => Ok("create_workspace"),
            "list" => Ok("list_workspaces"),
            "rename" => Ok("rename_workspace"),
            "archive" => Ok("archive_workspace"),
            "list_scripts" => Ok("list_workspace_scripts"),
            "start_script" => Ok("start_workspace_script"),
            "stop_script" => Ok("stop_workspace_script"),
            other => anyhow::bail!("unsupported `orbit_workspace` action `{other}`"),
        },
        "orbit_timer" => match req_action()? {
            "create" => Ok("create_timer"),
            "list" => Ok("list_timers"),
            "cancel" => Ok("cancel_timer"),
            other => anyhow::bail!("unsupported `orbit_timer` action `{other}`"),
        },
        "orbit_control" => match req_action()? {
            "list_profiles" | "profiles" => Ok("list_profiles"),
            "list_providers" | "providers" => Ok("list_providers"),
            "list_models" | "models" => Ok("list_models"),
            "list_hosts" | "hosts" => Ok("list_hosts"),
            "list_pending_permissions" | "list_permissions" | "permissions" => {
                Ok("list_pending_permissions")
            }
            "respond_permission" | "respond_to_permission" => Ok("respond_to_permission"),
            "list_projects" | "projects" => Ok("list_projects"),
            "inspect_project_context" | "project_context" => Ok("inspect_project_context"),
            other => anyhow::bail!("unsupported `orbit_control` action `{other}`"),
        },
        "orbit_browser" => match req_action()? {
            "open" => Ok("browser_open"),
            "snapshot" => Ok("browser_snapshot"),
            "screenshot" => Ok("browser_screenshot"),
            "click" => Ok("browser_click"),
            "type" => Ok("browser_type"),
            "scroll" => Ok("browser_scroll"),
            "press_key" => Ok("browser_press_key"),
            "select_option" => Ok("browser_select_option"),
            "upload_file" => Ok("browser_upload_file"),
            "pick_element" => Ok("browser_pick_element"),
            "close" => Ok("browser_close"),
            other => anyhow::bail!("unsupported `orbit_browser` action `{other}`"),
        },
        "orbit_schedule" => match req_action()? {
            "create" => Ok("create_schedule"),
            "list" => Ok("list_schedules"),
            "inspect" => Ok("inspect_schedule"),
            "update" => Ok("update_schedule"),
            "pause" => Ok("pause_schedule"),
            "resume" => Ok("resume_schedule"),
            "run_once" => Ok("run_schedule_once"),
            "logs" => Ok("schedule_logs"),
            "delete" => Ok("delete_schedule"),
            "create_heartbeat" | "heartbeat_add" => Ok("create_heartbeat"),
            "delete_heartbeat" | "heartbeat_delete" => Ok("delete_heartbeat"),
            other => anyhow::bail!("unsupported `orbit_schedule` action `{other}`"),
        },
        "orbit_workflow" => match req_action()? {
            "batch_fanout" | "batch" => Ok("run_batch_fanout"),
            "dag" => Ok("run_dag_workflow"),
            "best_of_n" => Ok("run_best_of_n"),
            "status" => Ok("get_workflow_status"),
            "cancel" => Ok("cancel_workflow"),
            other => anyhow::bail!("unsupported `orbit_workflow` action `{other}`"),
        },
        "orbit_goal" => match req_action()? {
            "start_goal" | "start" => Ok("start_goal"),
            "update_goal" | "update_goal_progress" | "update" => Ok("update_goal_progress"),
            "complete_goal" | "complete" => Ok("complete_goal"),
            "start_monitor" => Ok("start_monitor"),
            "stop_monitor" => Ok("stop_monitor"),
            other => anyhow::bail!("unsupported `orbit_goal` action `{other}`"),
        },
        "orbit_terminal" => match req_action()? {
            "list" => Ok("list_terminals"),
            "create" => Ok("create_terminal"),
            "capture" => Ok("capture_terminal"),
            "send_keys" | "keys" => Ok("send_terminal_keys"),
            "kill" => Ok("kill_terminal"),
            other => anyhow::bail!("unsupported `orbit_terminal` action `{other}`"),
        },
        "orbit_artifact" => match req_action()? {
            "publish" => Ok("publish_artifact"),
            "list" => Ok("list_artifacts"),
            "read" => Ok("read_artifact"),
            "delete" => Ok("delete_artifact"),
            other => anyhow::bail!("unsupported `orbit_artifact` action `{other}`"),
        },
        "orbit_lsp" => match req_action()? {
            "diagnostics" => Ok("lsp_diagnostics"),
            "symbols" => Ok("lsp_symbols"),
            "goto_definition" => Ok("lsp_goto_definition"),
            other => anyhow::bail!("unsupported `orbit_lsp` action `{other}`"),
        },
        _ => Ok(""),
    }
}

#[derive(Debug)]
struct BoardCasConflictError {
    expected: u64,
    current_board: orbit_protocol::WorkspaceBoardSnapshot,
}

impl std::fmt::Display for BoardCasConflictError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "board_revision_conflict: expected revision {} but current revision is {}",
            self.expected, self.current_board.revision
        )
    }
}

impl std::error::Error for BoardCasConflictError {}

/// Execute any Orbit orchestration tool by name with caller scoping & policy checks.
pub async fn execute_tool(
    runtime: &Arc<DaemonRuntime>,
    caller_agent_id: Option<&str>,
    name: &str,
    args: Value,
) -> ToolExecResult {
    let policy = resolve_caller_policy(runtime, caller_agent_id);
    if !policy.enabled || policy.disabled_tools.iter().any(|d| d == name) {
        return ToolExecResult::err(format!(
            "tool `{name}` is disabled by this agent's profile policy"
        ));
    }

    let resolved = match resolve_composite_dispatch(name, &args) {
        Ok("") => name.to_string(),
        Ok(mapped) => mapped.to_string(),
        Err(e) => return ToolExecResult::err(e.to_string()),
    };

    if policy.disabled_tools.iter().any(|d| d == &resolved) {
        return ToolExecResult::err(format!(
            "tool `{resolved}` is disabled by this agent's profile policy"
        ));
    }
    let pack = tool_pack_for_canonical_name(&resolved);
    if policy.disabled_packs.contains(&pack)
        || policy
            .enabled_packs
            .as_ref()
            .is_some_and(|allowed| !allowed.contains(&pack))
    {
        return ToolExecResult::err(format!(
            "tool pack `{}` for `{resolved}` is disabled by this agent's profile policy",
            pack.slug()
        ));
    }

    match execute_tool_inner(runtime, caller_agent_id, &resolved, &args).await {
        Ok(val) => ToolExecResult::ok(val),
        Err(e) => {
            if let Some(cas_err) = e.downcast_ref::<BoardCasConflictError>() {
                let structured = json!({
                    "error": cas_err.to_string(),
                    "conflict": true,
                    "expected_revision": cas_err.expected,
                    "current_revision": cas_err.current_board.revision,
                    "current_board": cas_err.current_board,
                });
                let text =
                    serde_json::to_string_pretty(&structured).unwrap_or_else(|_| e.to_string());
                return ToolExecResult {
                    is_error: true,
                    structured,
                    text,
                };
            }
            ToolExecResult::err(e.to_string())
        }
    }
}

async fn execute_tool_inner(
    rt: &Arc<DaemonRuntime>,
    caller_agent_id: Option<&str>,
    name: &str,
    args: &Value,
) -> anyhow::Result<Value> {
    let caller = caller_agent_id.and_then(|id| rt.store.get_agent(id).ok().flatten());
    let target_host =
        resolve_target_host_id(rt, args.get("host_id").and_then(|v| v.as_str())).await?;

    match name {
        // ==================== AGENTS ====================
        "create_agent" => {
            let profile_id = opt_str(args, "profile_id").or_else(|| opt_str(args, "profile"));
            let local_profile = if target_host.is_none() {
                profile_id.as_deref().and_then(|pid| {
                    rt.store
                        .get_profile(pid)
                        .ok()
                        .flatten()
                        .or_else(|| rt.store.get_profile_by_name(pid).ok().flatten())
                })
            } else {
                None
            };
            let (provider, inline_model) = if let Some(provider_raw) = opt_str(args, "provider") {
                parse_provider_and_model(&provider_raw)?
            } else if let Some(ref prof) = local_profile {
                (prof.provider, None)
            } else if let Some(ref c) = caller {
                (c.provider, None)
            } else {
                anyhow::bail!(
                    "missing required string argument `provider` (or supply `profile_id`)"
                );
            };
            let model = opt_str(args, "model")
                .or(inline_model)
                .or_else(|| local_profile.as_ref().and_then(|p| p.model.clone()));
            let effort = opt_str(args, "effort")
                .and_then(|s| ReasoningEffort::from_slug(&s))
                .or_else(|| local_profile.as_ref().and_then(|p| p.effort));
            let fast_mode = args.get("fast_mode").and_then(|v| v.as_bool());
            let mode = opt_str(args, "mode")
                .map(|s| parse_session_mode(&s))
                .or_else(|| local_profile.as_ref().map(|p| p.mode.clone()))
                .unwrap_or(SessionMode::AcceptEdits);
            let initial_prompt = opt_str(args, "initial_prompt")
                .or_else(|| opt_str(args, "prompt"))
                .unwrap_or_default();
            let title = opt_str(args, "title");
            let workspace_id = opt_str(args, "workspace_id");
            let background = args
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| caller.as_ref().is_some_and(|c| c.background));
            let auto_archive = args
                .get("auto_archive")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let notify_on_finish = args
                .get("notify_on_finish")
                .and_then(|v| v.as_bool())
                .unwrap_or(caller_agent_id.is_some());
            let idempotency_key = opt_str(args, "idempotency_key");

            // Check local idempotency receipt when supplied
            if let Some(ref idem) = idempotency_key {
                let hash = sha256_json(args);
                match rt
                    .store
                    .check_or_reserve_receipt("create_agent", idem, &hash)?
                {
                    ReceiptOutcome::Committed(cached_json) => {
                        return Ok(serde_json::from_str(&cached_json)?);
                    }
                    ReceiptOutcome::Conflict => {
                        anyhow::bail!("idempotency_key `{idem}` reused with different arguments");
                    }
                    ReceiptOutcome::PendingInFlight => {
                        anyhow::bail!(
                            "create_agent with idempotency_key `{idem}` is already in flight"
                        );
                    }
                    ReceiptOutcome::Reserved => {}
                }
            }

            let created_agent = if let Some(ref remote_host_id) = target_host {
                let cwd = opt_str(args, "cwd").unwrap_or_default();
                if workspace_id.is_none() && cwd.is_empty() {
                    anyhow::bail!(
                        "remote `create_agent` requires `workspace_id` or `cwd` on host `{remote_host_id}`"
                    );
                }
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CreateAgent {
                            request_id: req_id,
                            cwd,
                            extra_dirs: Vec::new(),
                            title,
                            provider,
                            model,
                            effort,
                            fast_mode,
                            mode,
                            initial_prompt,
                            initial_display_prompt: None,
                            initial_badges: Vec::new(),
                            worktree: args
                                .get("worktree")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false),
                            workspace_id,
                            parent_agent_id: caller_agent_id.map(ToOwned::to_owned),
                            parent_host_id: caller_agent_id.map(|_| rt.host_info.host_id.clone()),
                            profile_id,
                            background,
                            auto_archive,
                            notify_on_finish: Some(false),
                            idempotency_key: idempotency_key.clone(),
                            base_ref: opt_str(args, "base_ref"),
                            worktree_mode: parse_worktree_mode(
                                opt_str(args, "worktree_mode").as_deref(),
                            ),
                            branch: opt_str(args, "branch"),
                            handle: opt_str(args, "handle"),
                            auto_cluster: args.get("auto_cluster").and_then(|v| v.as_bool()),
                            acp_agent_id: opt_str(args, "acp_agent_id"),
                        },
                        Duration::from_secs(30),
                    )
                    .await;
                match resp {
                    Ok(WireMessage::AgentCreated { agent, .. }) => agent,
                    Ok(WireMessage::RpcError { message, .. }) => {
                        if let Some(ref idem) = idempotency_key {
                            let _ = rt.store.delete_pending_receipt("create_agent", idem);
                        }
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    Err(e) => {
                        if let Some(ref idem) = idempotency_key {
                            let _ = rt.store.delete_pending_receipt("create_agent", idem);
                        }
                        return Err(e);
                    }
                    _ => anyhow::bail!("unexpected remote reply to create_agent"),
                }
            } else {
                let cwd = opt_str(args, "cwd")
                    .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                    .unwrap_or_else(|| "~".to_string());
                let res = rt
                    .orchestrator
                    .create_and_start_agent(CreateAgentSpec {
                        cwd,
                        extra_dirs: Vec::new(),
                        title,
                        provider,
                        model,
                        effort,
                        fast_mode,
                        mode,
                        initial_prompt,
                        initial_display_prompt: None,
                        initial_badges: Vec::new(),
                        worktree: args
                            .get("worktree")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false),
                        workspace_id,
                        parent_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        parent_host_id: None,
                        profile_id,
                        background,
                        auto_archive,
                        base_ref: opt_str(args, "base_ref"),
                        worktree_mode: parse_worktree_mode(
                            opt_str(args, "worktree_mode").as_deref(),
                        ),
                        branch: opt_str(args, "branch"),
                        pr_number: args
                            .get("pr_number")
                            .and_then(|v| v.as_u64())
                            .map(|n| n as u32),
                        handle: opt_str(args, "handle"),
                        auto_cluster: args.get("auto_cluster").and_then(|v| v.as_bool()),
                        acp_agent_id: opt_str(args, "acp_agent_id"),
                    })
                    .await;
                match res {
                    Ok(a) => a,
                    Err(e) => {
                        if let Some(ref idem) = idempotency_key {
                            let _ = rt.store.delete_pending_receipt("create_agent", idem);
                        }
                        return Err(e);
                    }
                }
            };

            // Commit durable Finish Subscription BEFORE returning tool acknowledgment
            if notify_on_finish && let Some(cid) = caller_agent_id {
                rt.finish_subs
                    .register(
                        cid.to_string(),
                        created_agent.agent_id.clone(),
                        target_host.clone(),
                        true,
                    )
                    .await?;
            }

            let result = json!({
                "agent_id": created_agent.agent_id,
                "workspace_id": created_agent.workspace_id,
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "cwd": created_agent.cwd,
                "title": created_agent.title,
                "provider": created_agent.provider,
                "model": created_agent.model,
                "status": created_agent.status,
                "notify_on_finish": notify_on_finish,
            });
            if let Some(ref idem) = idempotency_key {
                let _ = rt
                    .store
                    .commit_receipt("create_agent", idem, &result.to_string());
            }
            Ok(result)
        }

        "send_agent_prompt" => {
            let agent_id = str_arg(args, "agent_id")?;
            let prompt = str_arg(args, "prompt")?;
            let behavior = parse_turn_behavior(opt_str(args, "active_turn_behavior").as_deref());
            let session_mode = opt_str(args, "session_mode").map(|s| parse_session_mode(&s));
            let notify_on_finish = args
                .get("notify_on_finish")
                .and_then(|v| v.as_bool())
                .unwrap_or(caller_agent_id.is_some());
            let message_id = opt_str(args, "message_id")
                .unwrap_or_else(|| format!("am_{}", Uuid::new_v4().simple()));

            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::SendPrompt {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            prompt,
                            display_prompt: None,
                            badges: Vec::new(),
                            active_turn_behavior: behavior,
                            message_id: Some(message_id.clone()),
                            caller_agent_id: caller_agent_id.map(ToOwned::to_owned),
                            notify_on_finish: Some(false),
                        },
                        Duration::from_secs(30),
                    )
                    .await?;
                if let WireMessage::RpcError { message, .. } = resp {
                    anyhow::bail!("remote send_agent_prompt failed: {message}");
                }
                if notify_on_finish && let Some(cid) = caller_agent_id {
                    rt.finish_subs
                        .register(
                            cid.to_string(),
                            agent_id.clone(),
                            Some(remote_host_id.clone()),
                            false,
                        )
                        .await?;
                }
                return Ok(json!({
                    "status": "queued",
                    "agent_id": agent_id,
                    "host_id": remote_host_id,
                    "message_id": message_id,
                    "notify_on_finish": notify_on_finish,
                }));
            }

            if rt.store.get_agent(&agent_id)?.is_none() {
                anyhow::bail!("agent `{agent_id}` not found");
            }
            if notify_on_finish && let Some(cid) = caller_agent_id {
                rt.finish_subs
                    .register(cid.to_string(), agent_id.clone(), None, false)
                    .await?;
            }
            let (queued_id, _ack) = rt
                .delivery
                .enqueue(QueuedAgentMessage {
                    recipient_id: agent_id.clone(),
                    message_id,
                    source: AgentPromptSource {
                        kind: AgentMessageKind::DirectMessage,
                        source_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        source_host_id: Some(rt.host_info.host_id.clone()),
                        source_title: caller.as_ref().map(|c| c.title.clone()),
                        event: None,
                        permission_context: None,
                    },
                    body: prompt,
                    active_turn_behavior: behavior,
                    session_mode,
                    unarchive: true,
                })
                .await?;
            Ok(json!({
                "status": "queued",
                "agent_id": agent_id,
                "host_id": rt.host_info.host_id,
                "message_id": queued_id,
                "notify_on_finish": notify_on_finish,
            }))
        }

        "get_agent_status" => {
            let agent_id = str_arg(args, "agent_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListAgents {
                            request_id: req_id,
                            workspace_id: None,
                            include_archived: true,
                            include_background: true,
                            parent_agent_id: None,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                if let WireMessage::AgentsSnapshot { agents, .. } = resp {
                    let ag = agents
                        .into_iter()
                        .find(|a| a.agent_id == agent_id)
                        .ok_or_else(|| anyhow::anyhow!("remote agent `{agent_id}` not found"))?;
                    return Ok(json!({ "host_id": remote_host_id, "agent": ag }));
                }
                anyhow::bail!("unexpected remote response to get_agent_status");
            }
            let ag = rt
                .store
                .get_agent(&agent_id)?
                .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?;
            let pending_perms = rt
                .orchestrator
                .list_pending_permissions(Some(&agent_id))
                .await;
            let last_msg = rt.store.last_assistant_message_text(&agent_id)?;
            Ok(json!({
                "host_id": rt.host_info.host_id,
                "agent": ag,
                "pending_permissions": pending_perms,
                "last_assistant_message": last_msg.map(|s| truncate_chars(&s, 2000)),
            }))
        }

        "get_agent_activity" => {
            let agent_id = str_arg(args, "agent_id")?;
            let since_seq = args.get("since_seq").and_then(|v| v.as_u64()).unwrap_or(0);
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(40)
                .clamp(1, 200) as usize;
            let events = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::SubscribeTimeline {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            since_seq,
                            tail_limit: None,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                match resp {
                    WireMessage::TimelineCatchup { events, .. } => events,
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    _ => anyhow::bail!("unexpected remote reply"),
                }
            } else {
                let (evs, _) = rt.store.fetch_timeline_since(&agent_id, since_seq, 500)?;
                evs
            };

            let start = events.len().saturating_sub(limit);
            let summary: Vec<Value> = events[start..]
                .iter()
                .map(summarize_timeline_event)
                .collect();
            Ok(json!({
                "agent_id": agent_id,
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "count": summary.len(),
                "entries": summary,
            }))
        }

        "list_agents" => {
            let workspace_id = opt_str(args, "workspace_id");
            let cwd_filter = opt_str(args, "cwd");
            let parent_filter = opt_str(args, "parent_agent_id");
            let include_archived = args
                .get("include_archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let include_background = args
                .get("include_background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let mut agents = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListAgents {
                            request_id: req_id,
                            workspace_id: workspace_id.clone(),
                            include_archived,
                            include_background,
                            parent_agent_id: parent_filter.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                match resp {
                    WireMessage::AgentsSnapshot { agents, .. } => agents,
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    _ => anyhow::bail!("unexpected remote reply"),
                }
            } else {
                rt.store.list_agents_filtered(
                    workspace_id.as_deref(),
                    include_archived,
                    include_background,
                    parent_filter.as_deref(),
                )?
            };
            if let Some(ref c) = cwd_filter {
                agents.retain(|a| a.cwd == *c);
            }
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "agents": agents,
            }))
        }

        "cancel_agent" => {
            let agent_id = str_arg(args, "agent_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::InterruptTurn {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            preserve_queue: false,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
            } else {
                rt.orchestrator.interrupt_agent(&agent_id).await?;
            }
            Ok(json!({ "ok": true, "agent_id": agent_id }))
        }

        "archive_agent" => {
            let agent_id = str_arg(args, "agent_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ArchiveAgent {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
            } else {
                rt.finish_subs.cancel_for_caller(&agent_id).await;
                rt.orchestrator.archive_agent(&agent_id).await?;
            }
            Ok(json!({ "ok": true, "agent_id": agent_id, "archived": true }))
        }

        "kill_agent" => {
            let agent_id = str_arg(args, "agent_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::DeleteAgent {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
            } else {
                rt.orchestrator.kill_agent(&agent_id).await?;
            }
            Ok(json!({ "ok": true, "agent_id": agent_id, "killed": true }))
        }

        "update_agent" => {
            let agent_id = str_arg(args, "agent_id")?;
            let title = opt_str(args, "title");
            let model = opt_str(args, "model");
            let effort = opt_str(args, "effort").and_then(|s| ReasoningEffort::from_slug(&s));
            let fast_mode = args.get("fast_mode").and_then(|v| v.as_bool());
            let mode = opt_str(args, "mode").map(|s| parse_session_mode(&s));
            let labels: Option<HashMap<String, String>> = args
                .get("labels")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::UpdateAgent {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            title,
                            model,
                            effort,
                            fast_mode,
                            mode,
                            labels,
                            handle: opt_str(args, "handle"),
                            task_summary: opt_str(args, "task_summary"),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                return Ok(json!({ "ok": true, "agent_id": agent_id }));
            }
            let updated = rt
                .orchestrator
                .update_agent(
                    &agent_id,
                    title,
                    model,
                    effort,
                    fast_mode,
                    mode,
                    labels,
                    opt_str(args, "handle"),
                    opt_str(args, "task_summary"),
                )
                .await?;
            Ok(json!({ "agent": updated }))
        }

        "set_agent_mode" => {
            let agent_id = str_arg(args, "agent_id")?;
            let mode = parse_session_mode(&str_arg(args, "mode")?);
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::SetSessionMode {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            mode,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
            } else {
                rt.orchestrator.set_session_mode(&agent_id, mode).await?;
            }
            Ok(json!({ "ok": true, "agent_id": agent_id }))
        }

        // ==================== WORKSPACES & SCRIPTS ====================
        "create_workspace" => {
            let source_cwd = opt_str(args, "source_cwd")
                .or_else(|| opt_str(args, "path"))
                .or_else(|| opt_str(args, "cwd"))
                .or_else(|| {
                    if target_host.is_none() {
                        caller.as_ref().map(|c| c.cwd.clone())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("`source_cwd` is required"))?;
            let isolation = match opt_str(args, "isolation").as_deref() {
                Some("worktree") => WorkspaceIsolation::Worktree,
                _ => {
                    if opt_str(args, "new_branch").is_some()
                        || opt_str(args, "branch").is_some()
                        || args.get("pr_number").is_some()
                    {
                        WorkspaceIsolation::Worktree
                    } else {
                        WorkspaceIsolation::Local
                    }
                }
            };
            let worktree_mode = parse_worktree_mode(opt_str(args, "worktree_mode").as_deref());
            let branch = opt_str(args, "new_branch").or_else(|| opt_str(args, "branch"));
            let base_ref = opt_str(args, "base_ref");
            let pr_number = args
                .get("pr_number")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let title = opt_str(args, "title");
            let background = args
                .get("background")
                .and_then(|v| v.as_bool())
                .unwrap_or_else(|| caller.as_ref().is_some_and(|c| c.background));
            let parent_workspace_id = caller.as_ref().map(|c| c.workspace_id.clone());

            let ws = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CreateWorkspace {
                            request_id: req_id,
                            source_cwd,
                            title,
                            worktree_branch: branch,
                            base_ref,
                            isolation,
                            worktree_mode,
                            pr_number,
                            parent_workspace_id: None,
                            background,
                            idempotency_key: opt_str(args, "idempotency_key"),
                            is_curated: Some(true),
                            mission_goal: opt_str(args, "mission_goal"),
                            project_root: opt_str(args, "project_root"),
                        },
                        Duration::from_secs(30),
                    )
                    .await?;
                match resp {
                    WireMessage::WorkspaceCreated { workspace, .. } => workspace,
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    _ => anyhow::bail!("unexpected remote reply to create_workspace"),
                }
            } else {
                rt.orchestrator
                    .create_workspace_v2(CreateWorkspaceSpec {
                        source_cwd,
                        title,
                        worktree_branch: branch,
                        base_ref,
                        isolation,
                        worktree_mode,
                        pr_number,
                        parent_workspace_id,
                        background,
                        project_root: opt_str(args, "project_root"),
                        mission_goal: opt_str(args, "mission_goal"),
                        allow_reuse_existing: false,
                        is_curated: true,
                    })
                    .await?
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "workspace": ws,
            }))
        }

        "list_workspaces" => {
            let include_archived = args
                .get("include_archived")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let include_background = args
                .get("include_background")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let workspaces = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListWorkspaces {
                            request_id: req_id,
                            include_archived,
                            include_background,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                match resp {
                    WireMessage::WorkspacesSnapshot { workspaces, .. } => workspaces,
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    _ => anyhow::bail!("unexpected remote reply"),
                }
            } else {
                rt.store
                    .list_workspaces_filtered(include_archived, include_background)?
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "workspaces": workspaces,
            }))
        }

        "rename_workspace" => {
            let workspace_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let title = str_arg(args, "title")?;
            let ws = rt
                .orchestrator
                .rename_workspace(&workspace_id, &title)
                .await?;
            Ok(json!({ "workspace": ws }))
        }

        "archive_workspace" => {
            let workspace_id = str_arg(args, "workspace_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let _ = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ArchiveWorkspace {
                            request_id: req_id,
                            workspace_id: workspace_id.clone(),
                            keep_agents_unclassified: false,
                        },
                        Duration::from_secs(20),
                    )
                    .await?;
            } else {
                rt.orchestrator.archive_workspace(&workspace_id).await?;
            }
            Ok(json!({ "ok": true, "workspace_id": workspace_id, "archived": true }))
        }

        "handoff_worktree" => {
            let requested_ws_id = opt_str(args, "workspace_id");
            let requested_agent = opt_str(args, "agent_id");
            let fallback_ws_id = requested_ws_id
                .clone()
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()));

            let resolved_agent_id: Option<String> = if let Some(ref raw_ag) = requested_agent {
                if target_host.is_some() {
                    Some(raw_ag.clone())
                } else if let Ok(Some(exact)) = rt.store.get_agent(raw_ag) {
                    Some(exact.agent_id)
                } else if let Some(ref ws_ctx) = fallback_ws_id {
                    let resolved = rt
                        .orchestrator
                        .resolve_peer_in_workspace(ws_ctx, raw_ag)?
                        .ok_or_else(|| {
                            anyhow::anyhow!("peer `{raw_ag}` not found in workspace `{ws_ctx}`")
                        })?;
                    Some(resolved.agent_id)
                } else {
                    Some(raw_ag.clone())
                }
            } else if target_host.is_none()
                && caller.as_ref().is_some_and(|c| {
                    c.labels.contains_key("orbit.worktree_source_cwd")
                        && requested_ws_id
                            .as_deref()
                            .is_none_or(|ws| ws == c.workspace_id)
                })
            {
                caller_agent_id.map(ToOwned::to_owned)
            } else {
                None
            };

            let workspace_id = requested_ws_id
                .or_else(|| {
                    if target_host.is_none() {
                        resolved_agent_id
                            .as_deref()
                            .and_then(|aid| rt.store.get_agent(aid).ok().flatten())
                            .map(|a| a.workspace_id)
                    } else {
                        None
                    }
                })
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;

            let direction = match opt_str(args, "direction")
                .as_deref()
                .unwrap_or("worktree_to_local")
                .to_ascii_lowercase()
                .as_str()
            {
                "worktree_to_local" | "to_local" | "to-local" => {
                    WorktreeHandoffDirection::WorktreeToLocal
                }
                "local_to_worktree" | "to_worktree" | "to-worktree" => {
                    WorktreeHandoffDirection::LocalToWorktree
                }
                other => anyhow::bail!(
                    "invalid `direction` `{other}` (expected `worktree_to_local` or `local_to_worktree`)"
                ),
            };
            let mode = match opt_str(args, "mode")
                .as_deref()
                .unwrap_or("apply")
                .to_ascii_lowercase()
                .as_str()
            {
                "apply" => WorktreeHandoffMode::Apply,
                "move" => WorktreeHandoffMode::Move,
                other => anyhow::bail!("invalid `mode` `{other}` (expected `apply` or `move`)"),
            };
            let target_workspace_id = opt_str(args, "target_workspace_id");
            let include_committed = args
                .get("include_committed")
                .and_then(|v| v.as_bool())
                .unwrap_or(direction == WorktreeHandoffDirection::WorktreeToLocal);
            let dry_run = args
                .get("dry_run")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let branch = opt_str(args, "branch");
            let allow_three_way_conflicts = args
                .get("allow_three_way_conflicts")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let resolved_host_id = target_host
                .clone()
                .unwrap_or_else(|| rt.host_info.host_id.clone());

            if dry_run {
                let preview = if let Some(ref remote_host_id) = target_host {
                    let req_id = Uuid::new_v4();
                    match rt
                        .connected_hosts
                        .send_rpc(
                            remote_host_id,
                            req_id,
                            WireMessage::PreviewWorktreeHandoff {
                                request_id: req_id,
                                workspace_id,
                                direction,
                                target_workspace_id,
                                include_committed,
                            },
                            Duration::from_secs(30),
                        )
                        .await?
                    {
                        WireMessage::WorktreeHandoffPreviewResult { preview, .. } => preview,
                        WireMessage::RpcError { message, .. } => {
                            anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                        }
                        _ => anyhow::bail!("unexpected remote reply to handoff_worktree preview"),
                    }
                } else {
                    rt.orchestrator
                        .preview_worktree_handoff_with_agent(
                            &workspace_id,
                            resolved_agent_id.as_deref(),
                            direction,
                            target_workspace_id.as_deref(),
                            include_committed,
                        )
                        .await?
                };
                return Ok(json!({
                    "host_id": resolved_host_id,
                    "dry_run": true,
                    "preview": preview,
                }));
            }

            let outcome = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ExecuteWorktreeHandoff {
                            request_id: req_id,
                            workspace_id,
                            direction,
                            mode,
                            target_workspace_id,
                            include_committed,
                            branch,
                            allow_three_way_conflicts,
                        },
                        Duration::from_secs(60),
                    )
                    .await?
                {
                    WireMessage::WorktreeHandoffExecuted { outcome, .. } => outcome,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to handoff_worktree execute"),
                }
            } else {
                rt.orchestrator
                    .execute_worktree_handoff(ExecuteWorktreeHandoffSpec {
                        workspace_id,
                        source_agent_id: resolved_agent_id,
                        direction,
                        mode,
                        target_workspace_id,
                        include_committed,
                        branch,
                        allow_three_way_conflicts,
                    })
                    .await?
            };
            Ok(json!({
                "host_id": resolved_host_id,
                "dry_run": false,
                "outcome": outcome,
            }))
        }

        "list_workspace_scripts" => {
            let workspace_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let scripts = rt
                .orchestrator
                .list_workspace_scripts(&workspace_id)
                .await?;
            Ok(json!({ "workspace_id": workspace_id, "scripts": scripts }))
        }

        "start_workspace_script" => {
            let workspace_id = str_arg(args, "workspace_id")?;
            let script_name = str_arg(args, "script_name")?;
            let info = rt
                .orchestrator
                .start_workspace_script(&workspace_id, &script_name)
                .await?;
            Ok(json!({ "script": info }))
        }

        "stop_workspace_script" => {
            let workspace_id = str_arg(args, "workspace_id")?;
            let script_name = str_arg(args, "script_name")?;
            let info = rt
                .orchestrator
                .stop_workspace_script(&workspace_id, &script_name)
                .await?;
            Ok(json!({ "script": info }))
        }

        // ==================== TERMINALS ====================
        "list_terminals" => {
            let cwd_filter = opt_str(args, "cwd");
            let mut terms = rt.orchestrator.list_live_terminals().await;
            if let Some(ref c) = cwd_filter {
                terms.retain(|t| t.cwd == *c);
            }
            Ok(json!({ "terminals": terms }))
        }

        "create_terminal" => {
            let cwd = opt_str(args, "cwd")
                .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                .unwrap_or_else(|| "~".to_string());
            let title = opt_str(args, "title");
            let cols = args.get("cols").and_then(|v| v.as_u64()).unwrap_or(120) as u16;
            let rows = args.get("rows").and_then(|v| v.as_u64()).unwrap_or(36) as u16;
            let initial_command = opt_str(args, "initial_command");
            let term = rt
                .orchestrator
                .create_terminal_with_command(
                    cwd,
                    title,
                    cols,
                    rows,
                    initial_command,
                    HashMap::new(),
                )
                .await?;
            Ok(json!({ "terminal": term }))
        }

        "capture_terminal" => {
            let terminal_id = str_arg(args, "terminal_id")?;
            let lines = args.get("lines").and_then(|v| v.as_u64()).map(|n| n as u32);
            let text = rt
                .orchestrator
                .capture_terminal_text(&terminal_id, lines)
                .await?;
            Ok(json!({ "terminal_id": terminal_id, "text": text }))
        }

        "send_terminal_keys" => {
            let terminal_id = str_arg(args, "terminal_id")?;
            let text = opt_str(args, "text");
            let keys: Vec<String> = args
                .get("keys")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(ToOwned::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            rt.orchestrator
                .send_terminal_keys(&terminal_id, text.as_deref(), &keys)
                .await?;
            Ok(json!({ "ok": true, "terminal_id": terminal_id }))
        }

        "kill_terminal" => {
            let terminal_id = str_arg(args, "terminal_id")?;
            rt.orchestrator.kill_terminal(&terminal_id).await?;
            Ok(json!({ "ok": true, "terminal_id": terminal_id }))
        }

        // ==================== SCHEDULES & HEARTBEATS ====================
        "create_schedule" => {
            let name = str_arg(args, "name")?;
            let cron = str_arg(args, "cron")?;
            let prompt = str_arg(args, "prompt")?;
            let provider_raw = opt_str(args, "provider").unwrap_or_else(|| "claude_code".into());
            let (provider, inline_model) = parse_provider_and_model(&provider_raw)?;
            let model = opt_str(args, "model").or(inline_model);
            let effort = opt_str(args, "effort").and_then(|s| ReasoningEffort::from_slug(&s));
            let mode = opt_str(args, "mode")
                .map(|s| parse_session_mode(&s))
                .unwrap_or(SessionMode::AcceptEdits);
            let cwd = opt_str(args, "cwd")
                .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                .ok_or_else(|| anyhow::anyhow!("`cwd` is required"))?;
            let max_runs = args
                .get("max_runs")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let expires_at = opt_str(args, "expires_at")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            let target = ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
                cwd,
                workspace_id: opt_str(args, "workspace_id"),
                provider,
                acp_agent_id: opt_str(args, "acp_agent_id"),
                model,
                effort,
                fast_mode: args
                    .get("fast_mode")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                mode,
                profile_id: opt_str(args, "profile_id"),
                worktree: args
                    .get("worktree")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                base_ref: opt_str(args, "base_ref"),
                archive_on_finish: true,
                background: args
                    .get("background")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            });
            let sched = rt.scheduler.create_schedule(
                name,
                &cron,
                opt_str(args, "timezone"),
                prompt,
                target,
                max_runs,
                expires_at,
                caller_agent_id.map(ToOwned::to_owned),
            )?;
            Ok(json!({ "schedule": sched }))
        }

        "list_schedules" => {
            let include_completed = args
                .get("include_completed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let all = rt.store.list_schedules(include_completed)?;
            Ok(json!({ "schedules": all }))
        }

        "inspect_schedule" => {
            let sid = str_arg(args, "schedule_id")?;
            let sched = rt
                .store
                .get_schedule(&sid)?
                .ok_or_else(|| anyhow::anyhow!("schedule `{sid}` not found"))?;
            let runs = rt.store.list_schedule_runs(&sid, 20)?;
            Ok(json!({ "schedule": sched, "recent_runs": runs }))
        }

        "pause_schedule" => {
            let sid = str_arg(args, "schedule_id")?;
            let s = rt
                .scheduler
                .set_schedule_status(&sid, ScheduleStatus::Paused)?;
            Ok(json!({ "schedule": s }))
        }

        "resume_schedule" => {
            let sid = str_arg(args, "schedule_id")?;
            let s = rt
                .scheduler
                .set_schedule_status(&sid, ScheduleStatus::Active)?;
            Ok(json!({ "schedule": s }))
        }

        "update_schedule" => {
            let sid = str_arg(args, "schedule_id")?;
            let expires_at = opt_str(args, "expires_at")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            let timezone_update = match args.get("timezone") {
                None => None,
                Some(Value::String(s)) => Some(s.as_str()),
                Some(_) => anyhow::bail!("`timezone` must be a string"),
            };
            let s = rt.scheduler.update_schedule(
                &sid,
                opt_str(args, "name"),
                opt_str(args, "cron").as_deref(),
                timezone_update,
                opt_str(args, "prompt"),
                args.get("max_runs")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32),
                expires_at,
                None,
            )?;
            Ok(json!({ "schedule": s }))
        }

        "schedule_logs" => {
            let sid = str_arg(args, "schedule_id")?;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
            let runs = rt.store.list_schedule_runs(&sid, limit)?;
            Ok(json!({ "schedule_id": sid, "runs": runs }))
        }

        "run_schedule_once" => {
            let sid = str_arg(args, "schedule_id")?;
            let run = rt.scheduler.run_once(&sid).await?;
            Ok(json!({ "run": run }))
        }

        "delete_schedule" => {
            let sid = str_arg(args, "schedule_id")?;
            rt.scheduler.delete_schedule(&sid)?;
            Ok(json!({ "ok": true, "schedule_id": sid }))
        }

        "create_heartbeat" => {
            let target_agent = opt_str(args, "agent_id")
                .or_else(|| caller_agent_id.map(ToOwned::to_owned))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "`create_heartbeat` requires an active caller agent or `agent_id`"
                    )
                })?;
            let cron = str_arg(args, "cron")?;
            let prompt = str_arg(args, "prompt")?;
            let name = opt_str(args, "name").unwrap_or_else(|| "Agent Heartbeat".to_string());
            let max_runs = args
                .get("max_runs")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32);
            let expires_at = opt_str(args, "expires_at")
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));
            let hb = rt.scheduler.create_schedule(
                name,
                &cron,
                opt_str(args, "timezone"),
                prompt,
                ScheduleTarget::ExistingAgent {
                    agent_id: target_agent,
                },
                max_runs,
                expires_at,
                caller_agent_id.map(ToOwned::to_owned),
            )?;
            Ok(json!({ "heartbeat": hb }))
        }

        "delete_heartbeat" => {
            let hid = opt_str(args, "heartbeat_id")
                .or_else(|| opt_str(args, "schedule_id"))
                .ok_or_else(|| anyhow::anyhow!("`heartbeat_id` is required"))?;
            rt.scheduler.delete_schedule(&hid)?;
            Ok(json!({ "ok": true, "heartbeat_id": hid }))
        }

        // ==================== ONE-SHOT CONDITIONAL TIMERS ====================
        "create_timer" => {
            let duration_seconds = args
                .get("duration_seconds")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("`duration_seconds` integer is required"))?;
            let prompt = str_arg(args, "prompt")?;
            let condition = match opt_str(args, "condition") {
                Some(raw) => raw
                    .parse::<orbit_protocol::TimerCondition>()
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
                None => orbit_protocol::TimerCondition::Never,
            };
            if let Some(ref remote_host_id) = target_host {
                let target_agent = opt_str(args, "agent_id").ok_or_else(|| {
                    anyhow::anyhow!(
                        "`create_timer` requires an explicit `agent_id` when targeting a remote `host_id`"
                    )
                })?;
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CreateConditionalTimer {
                            request_id: req_id,
                            agent_id: target_agent,
                            duration_seconds,
                            prompt,
                            condition,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                return match resp {
                    WireMessage::ConditionalTimerCreated { timer, .. } => {
                        Ok(json!({ "host_id": remote_host_id, "timer": timer }))
                    }
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    other => anyhow::bail!("unexpected response from remote host: {other:?}"),
                };
            }
            let target_agent = opt_str(args, "agent_id")
                .or_else(|| caller_agent_id.map(ToOwned::to_owned))
                .ok_or_else(|| {
                    anyhow::anyhow!("`create_timer` requires an active caller agent or `agent_id`")
                })?;
            let timer = rt
                .conditional_timers
                .create_timer(&target_agent, duration_seconds, prompt, condition)
                .await?;
            Ok(json!({ "host_id": rt.host_info.host_id, "timer": timer }))
        }

        "list_timers" => {
            let include_resolved = args
                .get("include_resolved")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListConditionalTimers {
                            request_id: req_id,
                            agent_id: opt_str(args, "agent_id"),
                            include_resolved,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                return match resp {
                    WireMessage::ConditionalTimersSnapshot { timers, .. } => {
                        Ok(json!({ "host_id": remote_host_id, "timers": timers }))
                    }
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    other => anyhow::bail!("unexpected response from remote host: {other:?}"),
                };
            }
            let target_agent =
                opt_str(args, "agent_id").or_else(|| caller_agent_id.map(ToOwned::to_owned));
            let timers = rt
                .conditional_timers
                .list_timers(target_agent.as_deref(), include_resolved)?;
            Ok(json!({ "host_id": rt.host_info.host_id, "timers": timers }))
        }

        "cancel_timer" => {
            let timer_id = str_arg(args, "timer_id")?;
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CancelConditionalTimer {
                            request_id: req_id,
                            timer_id,
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                return match resp {
                    WireMessage::ConditionalTimerUpdated { timer, .. } => {
                        Ok(json!({ "host_id": remote_host_id, "timer": timer }))
                    }
                    WireMessage::RpcError { message, .. } => anyhow::bail!("{message}"),
                    other => anyhow::bail!("unexpected response from remote host: {other:?}"),
                };
            }
            let timer = rt.conditional_timers.cancel_timer(&timer_id).await?;
            Ok(json!({ "host_id": rt.host_info.host_id, "timer": timer }))
        }

        // ==================== PROFILES / DISCOVERY / PERMISSIONS / HOSTS ====================
        "list_profiles" => {
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListProfiles { request_id: req_id },
                        Duration::from_secs(15),
                    )
                    .await?;
                if let WireMessage::ProfilesSnapshot { profiles, .. } = resp {
                    return Ok(json!({ "host_id": remote_host_id, "profiles": profiles }));
                }
            }
            let profiles = rt.store.list_profiles()?;
            Ok(json!({ "host_id": rt.host_info.host_id, "profiles": profiles }))
        }

        "list_providers" => {
            let installed =
                tokio::task::spawn_blocking(orbit_providers::discover_installed_providers)
                    .await
                    .unwrap_or_default();
            let list: Vec<Value> = installed
                .into_iter()
                .map(|p| {
                    json!({
                        "provider": p.kind,
                        "display_name": p.kind.display_name(),
                        "binary": p.executable_path,
                        "transport": p.protocol_transport,
                        "modes": ["default", "auto", "plan", "accept_edits", "full_access"],
                    })
                })
                .collect();
            Ok(json!({ "host_id": rt.host_info.host_id, "providers": list }))
        }

        "list_models" => {
            let filter_provider = opt_str(args, "provider")
                .and_then(|s| parse_provider_and_model(&s).ok().map(|(p, _)| p));
            let catalogs = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListProviderModels {
                            request_id: req_id,
                            force_refresh: false,
                        },
                        Duration::from_secs(20),
                    )
                    .await?;
                match resp {
                    WireMessage::ProviderModelsSnapshot { catalogs, .. } => catalogs,
                    _ => Vec::new(),
                }
            } else {
                rt.get_or_probe_provider_catalogs(false).await
            };
            let filtered: Vec<_> = catalogs
                .into_iter()
                .filter(|c| filter_provider.is_none_or(|fp| fp == c.provider))
                .collect();
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "catalogs": filtered,
            }))
        }

        "list_pending_permissions" => {
            let agent_filter = opt_str(args, "agent_id");
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListPendingPermissions {
                            request_id: req_id,
                            agent_id: agent_filter.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                if let WireMessage::PendingPermissionsSnapshot { permissions, .. } = resp {
                    return Ok(json!({ "host_id": remote_host_id, "permissions": permissions }));
                }
            }
            let perms = rt
                .orchestrator
                .list_pending_permissions(agent_filter.as_deref())
                .await;
            Ok(json!({ "host_id": rt.host_info.host_id, "permissions": perms }))
        }

        "respond_to_permission" => {
            let agent_id = str_arg(args, "agent_id")?;
            let request_id = opt_str(args, "request_id")
                .or_else(|| opt_str(args, "permission_request_id"))
                .ok_or_else(|| anyhow::anyhow!("missing required string argument `request_id`"))?;
            let decision = str_arg(args, "decision")?;
            if !matches!(decision.as_str(), "allow" | "allow_always" | "deny") {
                anyhow::bail!("decision must be `allow`, `allow_always`, or `deny`");
            }
            // Enforce 3-Tier Guardian + Personal Router Pre-Review ceiling when called by PersonalRouter
            if let Some(cid) = caller_agent_id
                && rt
                    .personal_router
                    .is_authorized_global_router(cid, target_host.is_some())
                    .await
            {
                let pending_assessment = if target_host.is_none() {
                    rt.orchestrator
                        .list_pending_permissions(Some(&agent_id))
                        .await
                        .into_iter()
                        .find(|p| p.request_id == request_id)
                        .and_then(|p| p.guardian_assessment)
                } else {
                    None
                };
                if let Err(reason) = rt
                    .personal_router
                    .validate_router_permission_decision(&decision, pending_assessment.as_ref())
                    .await
                {
                    anyhow::bail!("{reason}");
                }
            }
            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let resp = rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ResolvePermission {
                            request_id: req_id,
                            agent_id: agent_id.clone(),
                            permission_request_id: request_id.clone(),
                            decision: decision.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?;
                if let WireMessage::RpcError { message, .. } = resp {
                    anyhow::bail!("{message}");
                }
            } else {
                rt.orchestrator
                    .resolve_permission(&agent_id, request_id.clone(), decision.clone())
                    .await?;
            }
            Ok(json!({
                "ok": true,
                "agent_id": agent_id,
                "request_id": request_id,
                "decision": decision,
            }))
        }

        "list_hosts" => {
            let peers = rt.connected_hosts.list_summaries().await;
            Ok(json!({
                "self": rt.host_info,
                "connected_hosts": peers,
            }))
        }

        "list_projects" => {
            let requested_host = opt_str(args, "host_id");
            let resolved_host = match requested_host.as_deref() {
                Some("all") | Some("*") => Some("all".to_string()),
                _ => target_host.clone(),
            };
            let projects = rt
                .personal_router
                .list_fleet_projects(resolved_host.as_deref())
                .await;
            Ok(json!({
                "projects": projects,
            }))
        }

        "inspect_project_context" => {
            let cwd = str_arg(args, "cwd")?;
            let packet = rt
                .personal_router
                .inspect_project_context(target_host.as_deref(), &cwd)
                .await?;
            Ok(serde_json::to_value(packet)?)
        }

        // ==================== MISSION WORKSPACE COLLABORATION ====================
        "workspace_peers" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "`workspace_id` is required when called outside an agent session"
                    )
                })?;
            let ws = rt
                .store
                .get_workspace(&ws_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))?;
            let peers = rt
                .store
                .list_agents_filtered(Some(&ws_id), false, false, None)?;
            let board = rt.orchestrator.get_workspace_board(&ws_id).ok();
            let events = rt.orchestrator.list_workspace_events(&ws_id, 20)?;
            let caller_cwd = caller.as_ref().map(|c| c.cwd.as_str());
            let mut shared_checkout_peers: Vec<String> = Vec::new();
            let peer_cards: Vec<Value> = peers
                .iter()
                .map(|p| {
                    let is_self = caller_agent_id == Some(p.agent_id.as_str());
                    let handle = p.handle.as_deref().unwrap_or("peer");
                    if !is_self && caller_cwd.is_some_and(|cwd| !cwd.is_empty() && cwd == p.cwd) {
                        shared_checkout_peers.push(format!("@{handle}"));
                    }
                    let last_reply = rt
                        .store
                        .last_assistant_message_text(&p.agent_id)
                        .ok()
                        .flatten()
                        .map(|s| truncate_chars(&s, 280));
                    json!({
                        "agent_id": p.agent_id,
                        "handle": handle,
                        "is_self": is_self,
                        "title": p.title,
                        "provider": p.provider,
                        "model": p.model,
                        "mode": p.mode,
                        "status": p.status,
                        "cwd": p.cwd,
                        "worktree_branch": p.labels.get("orbit.worktree_branch"),
                        "is_worktree_isolated": p.labels.contains_key("orbit.worktree_source_cwd"),
                        "last_summary": last_reply,
                        "updated_at": p.updated_at,
                    })
                })
                .collect();
            Ok(json!({
                "workspace": ws,
                "peers": peer_cards,
                "shared_checkout_peers": shared_checkout_peers,
                "board_revision": board.as_ref().map(|b| b.revision).unwrap_or(0),
                "board_path": board.as_ref().and_then(|b| b.board_path.clone()),
                "recent_events": events,
            }))
        }

        "message_peer" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let handle_raw = str_arg(args, "handle")?;
            let prompt = opt_str(args, "message")
                .or_else(|| opt_str(args, "prompt"))
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `message`"))?;
            let behavior = parse_turn_behavior(opt_str(args, "active_turn_behavior").as_deref());
            let notify_on_finish = args
                .get("notify_on_finish")
                .and_then(|v| v.as_bool())
                .unwrap_or(caller_agent_id.is_some());
            let clean_target = handle_raw.trim().trim_start_matches('@');

            let targets: Vec<orbit_protocol::AgentSummary> = if clean_target
                .eq_ignore_ascii_case("all")
            {
                rt.store
                    .list_agents_filtered(Some(&ws_id), false, false, None)?
                    .into_iter()
                    .filter(|a| caller_agent_id != Some(a.agent_id.as_str()))
                    .collect()
            } else {
                match rt
                    .orchestrator
                    .resolve_peer_in_workspace(&ws_id, clean_target)?
                {
                    Some(p) => vec![p],
                    None => anyhow::bail!(
                        "peer `@{clean_target}` not found in workspace `{ws_id}` (call `workspace_peers` to see available handles)"
                    ),
                }
            };
            if targets.is_empty() {
                anyhow::bail!("no sibling peers available in workspace `{ws_id}`");
            }

            let sender_handle = caller
                .as_ref()
                .and_then(|c| c.handle.clone())
                .unwrap_or_else(|| "peer".into());
            let mut delivered = Vec::new();
            for peer in targets {
                if notify_on_finish && let Some(cid) = caller_agent_id {
                    let _ = rt
                        .finish_subs
                        .register(cid.to_string(), peer.agent_id.clone(), None, false)
                        .await;
                }
                let msg_id = format!("pm_{}", Uuid::new_v4().simple());
                let qmsg = QueuedAgentMessage {
                    recipient_id: peer.agent_id.clone(),
                    message_id: msg_id.clone(),
                    source: AgentPromptSource {
                        kind: AgentMessageKind::DirectMessage,
                        source_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        source_host_id: Some(rt.host_info.host_id.clone()),
                        source_title: Some(format!("@{sender_handle}")),
                        event: Some("peer_message".into()),
                        permission_context: None,
                    },
                    body: prompt.clone(),
                    active_turn_behavior: behavior,
                    session_mode: None,
                    unarchive: true,
                };
                let (mid, _) = rt.delivery.enqueue(qmsg).await?;
                let peer_h = peer.handle.clone().unwrap_or_else(|| "peer".into());
                if let Ok(ws_ev) = rt.store.append_workspace_event(
                    &ws_id,
                    caller_agent_id,
                    Some(&sender_handle),
                    orbit_protocol::WorkspaceEventKind::PeerMessage,
                    &format!(
                        "@{sender_handle} -> @{peer_h}: {}",
                        truncate_chars(&prompt, 120)
                    ),
                    None,
                ) {
                    let _ = rt.orchestrator.broadcast_tx.send(
                        crate::manager::DaemonBroadcast::WorkspaceEventRecorded(ws_ev),
                    );
                }
                let mut entry = json!({
                    "agent_id": peer.agent_id,
                    "handle": peer_h,
                    "message_id": mid,
                });
                if !clean_target.eq_ignore_ascii_case("all")
                    && !clean_target.eq_ignore_ascii_case(&peer_h)
                    && clean_target != peer.agent_id
                {
                    entry["resolved_via_alias"] = json!(true);
                }
                delivered.push(entry);
            }
            Ok(json!({
                "ok": true,
                "workspace_id": ws_id,
                "delivered": delivered,
            }))
        }

        "read_peer_context" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let handle_raw = str_arg(args, "handle")?;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let peer = rt
                .orchestrator
                .resolve_peer_in_workspace(&ws_id, &handle_raw)?
                .ok_or_else(|| {
                    anyhow::anyhow!("peer `{handle_raw}` not found in workspace `{ws_id}`")
                })?;
            let (events, _) = rt.store.fetch_timeline_since(&peer.agent_id, 0, 500)?;
            let mut edited_files: Vec<String> = Vec::new();
            let mut entries: Vec<Value> = Vec::new();
            for ev in &events {
                if let TimelinePayload::ToolCall {
                    detail: ToolCallDetail::FileEdit { path, .. },
                    ..
                } = &ev.payload
                    && !edited_files.contains(path)
                {
                    edited_files.push(path.clone());
                }
            }
            let start = events.len().saturating_sub(limit);
            for ev in &events[start..] {
                entries.push(summarize_timeline_event(ev));
            }
            let last_summary = rt
                .store
                .last_assistant_message_text(&peer.agent_id)?
                .map(|s| truncate_chars(&s, 2000));
            Ok(json!({
                "workspace_id": ws_id,
                "agent_id": peer.agent_id,
                "handle": peer.handle.as_deref().unwrap_or("peer"),
                "title": peer.title,
                "provider": peer.provider,
                "cwd": peer.cwd,
                "status": peer.status,
                "edited_files": edited_files,
                "last_summary": last_summary,
                "recent_entries": entries,
            }))
        }

        "workspace_board_read" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let board = rt.orchestrator.get_workspace_board(&ws_id)?;
            Ok(json!({ "board": board }))
        }

        "workspace_board_update" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let content = str_arg(args, "content")?;
            let section = opt_str(args, "section");
            let expected_revision = match args.get("expected_revision") {
                None | Some(Value::Null) => None,
                Some(v) => Some(
                    v.as_u64()
                        .ok_or_else(|| anyhow::anyhow!("`expected_revision` must be an integer"))?,
                ),
            };
            let updated_by = caller
                .as_ref()
                .and_then(|c| c.handle.clone())
                .or_else(|| opt_str(args, "updated_by"))
                .unwrap_or_else(|| "peer".into());
            match rt.orchestrator.update_workspace_board_cas(
                &ws_id,
                &content,
                section.as_deref(),
                expected_revision,
                Some(&updated_by),
                caller_agent_id,
            )? {
                Ok(board) => Ok(json!({ "board": board })),
                Err(current_board) => Err(anyhow::Error::new(BoardCasConflictError {
                    expected: expected_revision.unwrap_or(0),
                    current_board,
                })),
            }
        }

        "spawn_workspace_peer" => {
            let ws_id = opt_str(args, "workspace_id")
                .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                .ok_or_else(|| anyhow::anyhow!("`workspace_id` is required"))?;
            let ws = rt
                .store
                .get_workspace(&ws_id)?
                .ok_or_else(|| anyhow::anyhow!("workspace `{ws_id}` not found"))?;
            let handle = opt_str(args, "handle");
            let profile_id = opt_str(args, "profile_id").or_else(|| opt_str(args, "profile"));
            let local_profile = profile_id.as_deref().and_then(|pid| {
                rt.store
                    .get_profile(pid)
                    .ok()
                    .flatten()
                    .or_else(|| rt.store.get_profile_by_name(pid).ok().flatten())
            });
            let (provider, inline_model) = if let Some(provider_raw) = opt_str(args, "provider") {
                parse_provider_and_model(&provider_raw)?
            } else if let Some(ref prof) = local_profile {
                (prof.provider, None)
            } else if let Some(ref c) = caller {
                (c.provider, None)
            } else {
                (ProviderKind::ClaudeCode, None)
            };
            let model = opt_str(args, "model")
                .or(inline_model)
                .or_else(|| local_profile.as_ref().and_then(|p| p.model.clone()));
            let effort = opt_str(args, "effort")
                .and_then(|s| ReasoningEffort::from_slug(&s))
                .or_else(|| local_profile.as_ref().and_then(|p| p.effort));
            let fast_mode = args.get("fast_mode").and_then(|v| v.as_bool());
            let mode = opt_str(args, "mode")
                .map(|s| parse_session_mode(&s))
                .or_else(|| local_profile.as_ref().map(|p| p.mode.clone()))
                .unwrap_or(SessionMode::AcceptEdits);
            let initial_prompt = opt_str(args, "initial_prompt")
                .or_else(|| opt_str(args, "prompt"))
                .unwrap_or_default();
            let title = opt_str(args, "title");
            let worktree = args
                .get("worktree")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let notify_on_finish = args
                .get("notify_on_finish")
                .and_then(|v| v.as_bool())
                .unwrap_or(caller_agent_id.is_some());
            let cwd = opt_str(args, "cwd")
                .or_else(|| ws.source_cwd.clone())
                .unwrap_or_else(|| ws.cwd.clone());

            let peer = rt
                .orchestrator
                .create_and_start_agent(CreateAgentSpec {
                    cwd,
                    extra_dirs: Vec::new(),
                    title,
                    provider,
                    model,
                    effort,
                    fast_mode,
                    mode,
                    initial_prompt,
                    initial_display_prompt: None,
                    initial_badges: Vec::new(),
                    worktree,
                    workspace_id: Some(ws_id.clone()),
                    // Sibling peers stay top-level inside the Mission Workspace (not collapsed as hidden subagents)
                    parent_agent_id: None,
                    parent_host_id: None,
                    profile_id,
                    background: false,
                    auto_archive: false,
                    base_ref: opt_str(args, "base_ref"),
                    worktree_mode: parse_worktree_mode(opt_str(args, "worktree_mode").as_deref()),
                    branch: opt_str(args, "branch"),
                    pr_number: None,
                    handle,
                    auto_cluster: Some(false),
                    acp_agent_id: opt_str(args, "acp_agent_id"),
                })
                .await?;
            if notify_on_finish && let Some(cid) = caller_agent_id {
                let _ = rt
                    .finish_subs
                    .register(cid.to_string(), peer.agent_id.clone(), None, false)
                    .await;
            }
            Ok(json!({
                "ok": true,
                "workspace_id": ws_id,
                "agent_id": peer.agent_id,
                "handle": peer.handle,
                "cwd": peer.cwd,
                "agent": peer,
            }))
        }

        "fork_agent" => {
            let source_agent_id = opt_str(args, "source_agent_id")
                .or_else(|| opt_str(args, "agent_id"))
                .or_else(|| caller_agent_id.map(ToOwned::to_owned))
                .ok_or_else(|| {
                    anyhow::anyhow!("missing required string argument `source_agent_id`")
                })?;
            let up_to_seq = args.get("up_to_seq").and_then(|v| v.as_u64());
            let worktree = args
                .get("worktree")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let branch = opt_str(args, "branch");
            let base_ref = opt_str(args, "base_ref");
            let target = if worktree || branch.is_some() || base_ref.is_some() {
                ForkTarget::NewWorktree { branch, base_ref }
            } else {
                ForkTarget::SameDirectory
            };
            let title = opt_str(args, "title");
            let initial_prompt =
                opt_str(args, "initial_prompt").or_else(|| opt_str(args, "prompt"));
            let notify_on_finish = args
                .get("notify_on_finish")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let (agent, workspace) = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ForkAgent {
                            request_id: req_id,
                            source_agent_id,
                            up_to_seq,
                            target,
                            title,
                            initial_prompt,
                        },
                        Duration::from_secs(30),
                    )
                    .await?
                {
                    WireMessage::AgentForked {
                        agent, workspace, ..
                    } => (agent, workspace),
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to fork_agent"),
                }
            } else {
                let outcome = rt
                    .orchestrator
                    .fork_agent(ForkAgentSpec {
                        source_agent_id,
                        up_to_seq,
                        target,
                        title,
                        initial_prompt,
                    })
                    .await?;
                (outcome.agent, outcome.workspace)
            };

            if notify_on_finish
                && target_host.is_none()
                && let Some(cid) = caller_agent_id
            {
                let _ = rt
                    .finish_subs
                    .register(cid.to_string(), agent.agent_id.clone(), None, false)
                    .await;
            }

            Ok(json!({
                "ok": true,
                "agent_id": agent.agent_id,
                "handle": agent.handle,
                "workspace_id": agent.workspace_id,
                "cwd": agent.cwd,
                "worktree_branch": agent.labels.get("orbit.worktree_branch"),
                "forked_from_agent_id": agent.forked_from_agent_id,
                "forked_at_seq": agent.forked_at_seq,
                "agent": agent,
                "workspace": workspace,
            }))
        }

        "move_agent_to_workspace" => {
            let agent_id = opt_str(args, "agent_id")
                .or_else(|| caller_agent_id.map(ToOwned::to_owned))
                .ok_or_else(|| anyhow::anyhow!("`agent_id` is required"))?;
            let target_workspace_id = opt_str(args, "target_workspace_id");
            let new_title = opt_str(args, "new_workspace_title");
            let updated = rt
                .orchestrator
                .move_agent_to_workspace(&agent_id, target_workspace_id, new_title)
                .await?;
            Ok(json!({
                "ok": true,
                "agent": updated,
            }))
        }

        // ==================== ARTIFACTS & SEP-1865 MCP APPS ====================
        "publish_artifact" => {
            let title = str_arg(args, "title")?;
            let kind_raw = str_arg(args, "kind")?;
            let kind = ArtifactKind::from_slug(&kind_raw).ok_or_else(|| {
                anyhow::anyhow!(
                    "unsupported artifact kind `{kind_raw}` (expected: markdown, html, code, svg, mermaid, mcp_app)"
                )
            })?;
            let content = args
                .get("content")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `content`"))?
                .to_string();
            let artifact_id = opt_str(args, "artifact_id");
            let language = opt_str(args, "language");
            let mime_type = opt_str(args, "mime_type");
            let resource_uri = opt_str(args, "resource_uri");
            let summary_note = opt_str(args, "summary_note");
            let agent_id = opt_str(args, "agent_id").or_else(|| {
                if target_host.is_none() {
                    caller_agent_id.map(ToOwned::to_owned)
                } else {
                    None
                }
            });

            let artifact = if let Some(ref remote_host_id) = target_host {
                let workspace_id = opt_str(args, "workspace_id").ok_or_else(|| {
                    anyhow::anyhow!(
                        "remote `publish_artifact` requires `workspace_id` on host `{remote_host_id}`"
                    )
                })?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::UpsertArtifact {
                            request_id: req_id,
                            artifact_id,
                            workspace_id,
                            agent_id,
                            title,
                            kind,
                            language,
                            mime_type,
                            resource_uri,
                            content,
                            summary_note,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::ArtifactUpdated { artifact, .. } => artifact,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to publish_artifact"),
                }
            } else {
                let workspace_id = opt_str(args, "workspace_id")
                    .or_else(|| caller.as_ref().map(|c| c.workspace_id.clone()))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "`workspace_id` is required when called outside an agent session"
                        )
                    })?;
                let orch = Arc::clone(&rt.orchestrator);
                tokio::task::spawn_blocking(move || {
                    orch.upsert_artifact(UpsertArtifactInput {
                        artifact_id,
                        workspace_id,
                        agent_id,
                        title,
                        kind,
                        language,
                        mime_type,
                        resource_uri,
                        content,
                        summary_note,
                    })
                })
                .await
                .map_err(|e| anyhow::anyhow!("publish_artifact worker failed: {e}"))??
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "artifact": artifact,
            }))
        }

        "list_artifacts" => {
            let workspace_id = opt_str(args, "workspace_id");
            let agent_id = opt_str(args, "agent_id");
            let artifacts = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListArtifacts {
                            request_id: req_id,
                            workspace_id,
                            agent_id,
                        },
                        Duration::from_secs(15),
                    )
                    .await?
                {
                    WireMessage::ArtifactsSnapshot { artifacts, .. } => artifacts,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to list_artifacts"),
                }
            } else {
                let orch = Arc::clone(&rt.orchestrator);
                tokio::task::spawn_blocking(move || {
                    orch.list_artifacts(workspace_id.as_deref(), agent_id.as_deref())
                })
                .await
                .map_err(|e| anyhow::anyhow!("list_artifacts worker failed: {e}"))??
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "artifacts": artifacts,
            }))
        }

        "read_artifact" => {
            let artifact_id = str_arg(args, "artifact_id")?;
            let version = match args.get("version") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let raw = v
                        .as_u64()
                        .ok_or_else(|| anyhow::anyhow!("`version` must be a positive integer"))?;
                    if raw == 0 {
                        anyhow::bail!("`version` must be >= 1");
                    }
                    let parsed = u32::try_from(raw)
                        .map_err(|_| anyhow::anyhow!("`version` exceeds u32 range"))?;
                    Some(parsed)
                }
            };
            let artifact = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::GetArtifact {
                            request_id: req_id,
                            artifact_id: artifact_id.clone(),
                            version,
                        },
                        Duration::from_secs(15),
                    )
                    .await?
                {
                    WireMessage::ArtifactContent { artifact, .. } => artifact,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to read_artifact"),
                }
            } else {
                let orch = Arc::clone(&rt.orchestrator);
                let aid_owned = artifact_id.clone();
                tokio::task::spawn_blocking(move || orch.get_artifact(&aid_owned, version))
                    .await
                    .map_err(|e| anyhow::anyhow!("read_artifact worker failed: {e}"))??
                    .ok_or_else(|| anyhow::anyhow!("artifact `{artifact_id}` not found"))?
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "artifact": artifact,
            }))
        }

        "delete_artifact" => {
            let artifact_id = str_arg(args, "artifact_id")?;
            let deleted = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::DeleteArtifact {
                            request_id: req_id,
                            artifact_id: artifact_id.clone(),
                        },
                        Duration::from_secs(15),
                    )
                    .await?
                {
                    WireMessage::ArtifactDeleted { .. } => true,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to delete_artifact"),
                }
            } else {
                let orch = Arc::clone(&rt.orchestrator);
                let aid_owned = artifact_id.clone();
                tokio::task::spawn_blocking(move || orch.delete_artifact(&aid_owned))
                    .await
                    .map_err(|e| anyhow::anyhow!("delete_artifact worker failed: {e}"))??
            };
            Ok(json!({
                "ok": true,
                "deleted": deleted,
                "artifact_id": artifact_id,
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
            }))
        }

        "lsp_diagnostics" => {
            let path = opt_str(args, "path");
            let cwd = opt_str(args, "cwd")
                .or_else(|| {
                    if target_host.is_none() {
                        caller.as_ref().map(|c| c.cwd.clone())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("missing required parameter `cwd` (workspace root directory)")
                })?;

            let diagnostics = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::LspGetDiagnostics {
                            request_id: req_id,
                            cwd: cwd.clone(),
                            path: path.clone(),
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::LspDiagnosticsResponse { diagnostics, .. } => diagnostics,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to lsp_diagnostics"),
                }
            } else {
                let resolved_cwd = crate::expand_host_path(&cwd);
                let authorized_cwd =
                    crate::authorize_workspace_mutation_cwd(&rt.store, &resolved_cwd)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.lsp
                    .get_diagnostics(&authorized_cwd, path.as_deref())
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "cwd": cwd,
                "path": path,
                "count": diagnostics.len(),
                "diagnostics": diagnostics,
            }))
        }

        "lsp_symbols" => {
            let query = opt_str(args, "query").unwrap_or_default();
            let path = opt_str(args, "path");
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(50)
                .clamp(1, 200);
            let cwd = opt_str(args, "cwd")
                .or_else(|| {
                    if target_host.is_none() {
                        caller.as_ref().map(|c| c.cwd.clone())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("missing required parameter `cwd` (workspace root directory)")
                })?;

            let symbols = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::LspQuerySymbols {
                            request_id: req_id,
                            cwd: cwd.clone(),
                            query: query.clone(),
                            path: path.clone(),
                            limit,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::LspSymbolsResponse { symbols, .. } => symbols,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to lsp_symbols"),
                }
            } else {
                let resolved_cwd = crate::expand_host_path(&cwd);
                let authorized_cwd =
                    crate::authorize_workspace_mutation_cwd(&rt.store, &resolved_cwd)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.lsp
                    .query_symbols(&authorized_cwd, &query, path.as_deref(), limit)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "cwd": cwd,
                "query": query,
                "path": path,
                "count": symbols.len(),
                "symbols": symbols,
            }))
        }

        "lsp_goto_definition" => {
            let path = str_arg(args, "path")?;
            let line = args
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&v| v >= 1)
                .ok_or_else(|| anyhow::anyhow!("missing or invalid 1-based integer `line`"))?;
            let col = args
                .get("col")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&v| v >= 1)
                .ok_or_else(|| anyhow::anyhow!("missing or invalid 1-based integer `col`"))?;
            let cwd = opt_str(args, "cwd")
                .or_else(|| {
                    if target_host.is_none() {
                        caller.as_ref().map(|c| c.cwd.clone())
                    } else {
                        None
                    }
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("missing required parameter `cwd` (workspace root directory)")
                })?;

            let locations = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::LspGotoDefinition {
                            request_id: req_id,
                            cwd: cwd.clone(),
                            path: path.clone(),
                            line,
                            col,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::LspLocationsResponse { locations, .. } => locations,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}");
                    }
                    _ => anyhow::bail!("unexpected remote reply to lsp_goto_definition"),
                }
            } else {
                let resolved_cwd = crate::expand_host_path(&cwd);
                let authorized_cwd =
                    crate::authorize_workspace_mutation_cwd(&rt.store, &resolved_cwd)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.lsp
                    .goto_definition(&authorized_cwd, &path, line, col)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "cwd": cwd,
                "path": path,
                "line": line,
                "col": col,
                "locations": locations,
            }))
        }

        // ==================== AUTONOMOUS GOAL MODE & MONITORS ====================
        "start_goal" => {
            let objective = str_arg(args, "objective")?;
            let agent_id = opt_str(args, "agent_id")
                .or_else(|| {
                    if target_host.is_none() {
                        caller_agent_id.map(ToOwned::to_owned)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `agent_id`"))?;
            let criteria: Vec<String> = args
                .get("criteria")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::trim))
                        .filter(|s| !s.is_empty())
                        .map(ToOwned::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            let verify_command = opt_str(args, "verify_command");
            let max_iterations = args
                .get("max_iterations")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok());
            let max_duration_secs = args.get("max_duration_secs").and_then(Value::as_u64);

            let goal = if let Some(ref remote_host_id) = target_host {
                authorize_remote_agent_control(rt, remote_host_id, caller_agent_id, &agent_id)
                    .await?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StartGoalRun {
                            request_id: req_id,
                            agent_id,
                            objective,
                            criteria,
                            verify_command,
                            max_iterations,
                            max_duration_secs,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::GoalRunUpdated { goal, .. } => goal,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to start_goal"),
                }
            } else {
                authorize_agent_control(&rt.store, caller_agent_id, &agent_id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                let is_self_call = caller_agent_id == Some(agent_id.as_str());
                rt.goals
                    .start_goal(StartGoalSpec {
                        agent_id,
                        objective,
                        criteria,
                        verify_command,
                        max_iterations,
                        max_duration_secs,
                        // Don't interrupt the calling agent's own active turn
                        dispatch_initial_prompt: !is_self_call,
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "goal": goal,
            }))
        }

        "update_goal_progress" => {
            if target_host.is_some() {
                anyhow::bail!("`update_goal_progress` operates on the local host goal run");
            }
            let goal_id = opt_str(args, "goal_id");
            let agent_id =
                opt_str(args, "agent_id").or_else(|| caller_agent_id.map(ToOwned::to_owned));
            let note = opt_str(args, "note");
            let mut patches = Vec::new();
            if let Some(arr) = args.get("criteria").and_then(Value::as_array) {
                for item in arr {
                    let key = item
                        .get("id")
                        .and_then(Value::as_str)
                        .or_else(|| item.get("label").and_then(Value::as_str))
                        .unwrap_or_default()
                        .trim();
                    let passed = item.get("passed").and_then(Value::as_bool).unwrap_or(true);
                    if !key.is_empty() {
                        patches.push(GoalCriterionPatch {
                            id_or_label: key.to_string(),
                            passed,
                        });
                    }
                }
            }
            if let Some(single_id) =
                opt_str(args, "criterion_id").or_else(|| opt_str(args, "criterion"))
            {
                let passed = args.get("passed").and_then(Value::as_bool).unwrap_or(true);
                patches.push(GoalCriterionPatch {
                    id_or_label: single_id,
                    passed,
                });
            }
            let goal = rt
                .goals
                .update_goal_progress(
                    caller_agent_id,
                    goal_id.as_deref(),
                    agent_id.as_deref(),
                    patches,
                    note,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(json!({ "goal": goal }))
        }

        "complete_goal" => {
            if target_host.is_some() {
                anyhow::bail!("`complete_goal` operates on the local host goal run");
            }
            let goal_id = opt_str(args, "goal_id");
            let agent_id =
                opt_str(args, "agent_id").or_else(|| caller_agent_id.map(ToOwned::to_owned));
            let summary = opt_str(args, "summary");
            let failed = args.get("failed").and_then(Value::as_bool).unwrap_or(false);
            let goal = rt
                .goals
                .complete_goal(
                    caller_agent_id,
                    goal_id.as_deref(),
                    agent_id.as_deref(),
                    summary,
                    failed,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            Ok(json!({ "goal": goal }))
        }

        "start_monitor" => {
            let agent_id = opt_str(args, "agent_id")
                .or_else(|| {
                    if target_host.is_none() {
                        caller_agent_id.map(ToOwned::to_owned)
                    } else {
                        None
                    }
                })
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `agent_id`"))?;
            let name = opt_str(args, "name").unwrap_or_default();
            let expires_in_secs = args.get("expires_in_secs").and_then(Value::as_u64);
            let kind_str = str_arg(args, "kind")?.to_ascii_lowercase();
            let kind = match kind_str.as_str() {
                "command_stream" | "command" => {
                    let command = str_arg(args, "command")?;
                    let cmd_args: Vec<String> = args
                        .get("args")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    let pattern = opt_str(args, "pattern");
                    MonitorKind::CommandStream {
                        command,
                        args: cmd_args,
                        pattern,
                    }
                }
                "log_file_tail" | "log_tail" | "file" => {
                    let path = str_arg(args, "path")?;
                    let pattern = str_arg(args, "pattern")?;
                    MonitorKind::LogFileTail { path, pattern }
                }
                "http_health" | "http" => {
                    let url = str_arg(args, "url")?;
                    let expect_status = args
                        .get("expect_status")
                        .and_then(Value::as_u64)
                        .and_then(|v| u16::try_from(v).ok())
                        .unwrap_or(200);
                    let interval_secs = args
                        .get("interval_secs")
                        .and_then(Value::as_u64)
                        .unwrap_or(30);
                    MonitorKind::HttpHealth {
                        url,
                        expect_status,
                        interval_secs,
                    }
                }
                other => {
                    anyhow::bail!(
                        "unsupported monitor kind `{other}`; expected `command_stream`, `log_file_tail`, or `http_health`"
                    )
                }
            };

            let monitor = if let Some(ref remote_host_id) = target_host {
                authorize_remote_agent_control(rt, remote_host_id, caller_agent_id, &agent_id)
                    .await?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StartMonitorWatch {
                            request_id: req_id,
                            agent_id,
                            name,
                            kind,
                            expires_in_secs,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::MonitorWatchUpdated { monitor, .. } => monitor,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to start_monitor"),
                }
            } else {
                authorize_agent_control(&rt.store, caller_agent_id, &agent_id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.goals
                    .start_monitor(StartMonitorSpec {
                        agent_id,
                        name,
                        kind,
                        expires_in_secs,
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "monitor": monitor,
            }))
        }

        "stop_monitor" => {
            let monitor_id = str_arg(args, "monitor_id")?;
            let monitor = if let Some(ref remote_host_id) = target_host {
                if caller_agent_id.is_some() {
                    let list_req = Uuid::new_v4();
                    let owner_agent_id = match rt
                        .connected_hosts
                        .send_rpc(
                            remote_host_id,
                            list_req,
                            WireMessage::ListMonitorWatches {
                                request_id: list_req,
                                agent_id: None,
                            },
                            Duration::from_secs(15),
                        )
                        .await?
                    {
                        WireMessage::MonitorWatchesSnapshot { monitors, .. } => monitors
                            .into_iter()
                            .find(|m| m.monitor_id == monitor_id)
                            .map(|m| m.agent_id)
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "monitor `{monitor_id}` not found on host `{remote_host_id}`"
                                )
                            })?,
                        WireMessage::RpcError { message, .. } => {
                            anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                        }
                        _ => anyhow::bail!("unexpected remote reply to list_monitor_watches"),
                    };
                    authorize_remote_agent_control(
                        rt,
                        remote_host_id,
                        caller_agent_id,
                        &owner_agent_id,
                    )
                    .await?;
                }
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StopMonitorWatch {
                            request_id: req_id,
                            monitor_id,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::MonitorWatchUpdated { monitor, .. } => monitor,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to stop_monitor"),
                }
            } else {
                let existing = rt
                    .store
                    .get_monitor_watch(&monitor_id)?
                    .ok_or_else(|| anyhow::anyhow!("monitor `{monitor_id}` not found"))?;
                authorize_agent_control(&rt.store, caller_agent_id, &existing.agent_id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.goals
                    .stop_monitor(&monitor_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "monitor": monitor,
            }))
        }

        // ==================== PURE-RUST CDP BROWSER AUTOMATION ====================
        "browser_open" => {
            let url = opt_str(args, "url");
            let existing_session_id = opt_str(args, "session_id");
            if let Some(session_id) = existing_session_id {
                let target_url = url.ok_or_else(|| {
                    anyhow::anyhow!("`url` is required when navigating an existing `session_id`")
                })?;
                authorize_browser_session_access(
                    rt,
                    target_host.as_deref(),
                    caller_agent_id,
                    &session_id,
                )
                .await?;
                let mut out = execute_browser_action_tool(
                    rt,
                    target_host.as_deref(),
                    caller_agent_id,
                    &session_id,
                    BrowserActionRequest::Navigate { url: target_url },
                )
                .await?;
                out.session.last_screenshot_base64 = None;
                return Ok(json!({
                    "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                    "session": out.session,
                    "elements": out.elements,
                    "text_excerpt": out.text_excerpt,
                    "captcha_detected": out.captcha_detected,
                }));
            }

            let headless = args.get("headless").and_then(Value::as_bool);
            let viewport_width = args
                .get("viewport_width")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok());
            let viewport_height = args
                .get("viewport_height")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok());
            let owner_agent_id = caller_agent_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string);
            let workspace_root = owner_agent_id
                .as_deref()
                .and_then(|aid| rt.store.get_agent(aid).ok().flatten())
                .map(|a| a.cwd);

            let mut session = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CreateBrowserSession {
                            request_id: req_id,
                            agent_id: owner_agent_id,
                            initial_url: url,
                            headless,
                            viewport_width,
                            viewport_height,
                            workspace_root,
                        },
                        Duration::from_secs(30),
                    )
                    .await?
                {
                    WireMessage::BrowserSessionUpdated { session, .. } => session,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to browser_open"),
                }
            } else {
                rt.browser
                    .create_session(CreateBrowserSessionSpec {
                        agent_id: owner_agent_id,
                        initial_url: url,
                        headless,
                        viewport_width,
                        viewport_height,
                        workspace_root: workspace_root.map(PathBuf::from),
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };
            session.last_screenshot_base64 = None;

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": session,
            }))
        }

        "browser_snapshot" => {
            let session_id = str_arg(args, "session_id")?;
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::Snapshot,
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_screenshot" => {
            let session_id = str_arg(args, "session_id")?;
            let full_page = args
                .get("full_page")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::Screenshot { full_page },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "screenshot_base64": out.screenshot_base64,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_click" => {
            let session_id = str_arg(args, "session_id")?;
            let ref_id = opt_str(args, "ref_id");
            let x = args.get("x").and_then(Value::as_f64);
            let y = args.get("y").and_then(Value::as_f64);
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::Click { ref_id, x, y },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_type" => {
            let session_id = str_arg(args, "session_id")?;
            let ref_id = opt_str(args, "ref_id");
            let text = str_arg(args, "text")?;
            let submit = args.get("submit").and_then(Value::as_bool).unwrap_or(false);
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::Type {
                    ref_id,
                    text,
                    submit,
                },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_scroll" => {
            let session_id = str_arg(args, "session_id")?;
            let delta_x = args.get("delta_x").and_then(Value::as_f64).unwrap_or(0.0);
            let delta_y = args.get("delta_y").and_then(Value::as_f64).unwrap_or(400.0);
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::Scroll { delta_x, delta_y },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_press_key" => {
            let session_id = str_arg(args, "session_id")?;
            let key = str_arg(args, "key")?;
            let modifiers = args
                .get("modifiers")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let ref_id = opt_str(args, "ref_id");
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::PressKey {
                    key,
                    modifiers,
                    ref_id,
                },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_select_option" => {
            let session_id = str_arg(args, "session_id")?;
            let ref_id = opt_str(args, "ref_id");
            let selector = opt_str(args, "selector");
            let value = opt_str(args, "value");
            let label = opt_str(args, "label");
            let index = args
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok());
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::SelectOption {
                    ref_id,
                    selector,
                    value,
                    label,
                    index,
                },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_upload_file" => {
            let session_id = str_arg(args, "session_id")?;
            let ref_id = opt_str(args, "ref_id");
            let selector = opt_str(args, "selector");
            let mut files: Vec<String> = args
                .get("files")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if files.is_empty()
                && let Some(single) = opt_str(args, "file")
            {
                files.push(single);
            }
            if files.is_empty() {
                anyhow::bail!(
                    "`files` (array of workspace file paths) is required for upload_file"
                );
            }
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::UploadFile {
                    ref_id,
                    selector,
                    files,
                },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
            }))
        }

        "browser_pick_element" => {
            let session_id = str_arg(args, "session_id")?;
            let x = args.get("x").and_then(Value::as_f64);
            let y = args.get("y").and_then(Value::as_f64);
            let selector = opt_str(args, "selector");
            let ref_id = opt_str(args, "ref_id");
            let interactive = args
                .get("interactive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let mut out = execute_browser_action_tool(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
                BrowserActionRequest::PickElement {
                    x,
                    y,
                    selector,
                    ref_id,
                    interactive,
                },
            )
            .await?;
            out.session.last_screenshot_base64 = None;
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "session": out.session,
                "elements": out.elements,
                "text_excerpt": out.text_excerpt,
                "captcha_detected": out.captcha_detected,
                "picked_element": out.picked_element,
            }))
        }

        "browser_close" => {
            let session_id = str_arg(args, "session_id")?;
            authorize_browser_session_access(
                rt,
                target_host.as_deref(),
                caller_agent_id,
                &session_id,
            )
            .await?;
            let remaining = if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CloseBrowserSession {
                            request_id: req_id,
                            session_id: session_id.clone(),
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::BrowserSessionsSnapshot { sessions, .. } => sessions.len(),
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to browser_close"),
                }
            } else {
                rt.browser
                    .close_session(&session_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .sessions
                    .len()
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "closed_session_id": session_id,
                "active_sessions": remaining,
            }))
        }

        // ==================== BATCH CSV, DAG WORKFLOWS & BEST-OF-N ARENA ====================
        "run_batch_fanout" => {
            authorize_workflow_launch_from_caller(&rt.store, caller_agent_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let caller = caller_agent_id.and_then(|cid| rt.store.get_agent(cid).ok().flatten());
            let name = opt_str(args, "name").unwrap_or_default();
            let csv_content = str_arg(args, "csv_content")?;
            let prompt_template = str_arg(args, "prompt_template")?;
            let (provider, inline_model) = if let Some(p_raw) = opt_str(args, "provider") {
                parse_provider_and_model(&p_raw)?
            } else if let Some(ref c) = caller {
                (c.provider, None)
            } else {
                (ProviderKind::ClaudeCode, None)
            };
            let model = opt_str(args, "model").or(inline_model);
            let requested_mode = opt_str(args, "mode")
                .map(|s| parse_session_mode(&s))
                .unwrap_or_else(|| {
                    caller
                        .as_ref()
                        .map(|c| c.mode.clone())
                        .unwrap_or(SessionMode::AcceptEdits)
                });
            let mode = match caller.as_ref() {
                Some(c) => clamp_workflow_session_mode(&c.mode, requested_mode),
                None => requested_mode,
            };
            let max_concurrency = args
                .get("max_concurrency")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(3)
                .clamp(1, 8);
            let isolated_worktree = args
                .get("isolated_worktree")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            let run = if let Some(ref remote_host_id) = target_host {
                let cwd = str_arg(args, "cwd")?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StartBatchFanout {
                            request_id: req_id,
                            name,
                            cwd,
                            csv_content,
                            prompt_template,
                            provider,
                            model,
                            mode,
                            max_concurrency,
                            isolated_worktree,
                        },
                        Duration::from_secs(30),
                    )
                    .await?
                {
                    WireMessage::WorkflowRunUpdated { run, .. } => run,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to run_batch_fanout"),
                }
            } else {
                let cwd = opt_str(args, "cwd")
                    .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                    .ok_or_else(|| anyhow::anyhow!("missing required parameter `cwd`"))?;
                rt.workflows
                    .start_batch_fanout(StartBatchFanoutSpec {
                        name,
                        cwd,
                        owner_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        csv_content,
                        prompt_template,
                        provider,
                        model,
                        mode,
                        max_concurrency,
                        isolated_worktree,
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "run": run,
            }))
        }

        "run_dag_workflow" => {
            authorize_workflow_launch_from_caller(&rt.store, caller_agent_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let caller = caller_agent_id.and_then(|cid| rt.store.get_agent(cid).ok().flatten());
            let name = opt_str(args, "name").unwrap_or_default();
            let raw_steps = args
                .get("steps")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `steps`"))?;
            let steps: Vec<DagStepSpec> = serde_json::from_value(raw_steps)
                .map_err(|e| anyhow::anyhow!("invalid `steps` array: {e}"))?;
            let default_provider = if let Some(p_raw) = opt_str(args, "default_provider") {
                parse_provider_and_model(&p_raw)?.0
            } else if let Some(ref c) = caller {
                c.provider
            } else {
                ProviderKind::ClaudeCode
            };
            let requested_mode = opt_str(args, "default_mode")
                .map(|s| parse_session_mode(&s))
                .unwrap_or_else(|| {
                    caller
                        .as_ref()
                        .map(|c| c.mode.clone())
                        .unwrap_or(SessionMode::AcceptEdits)
                });
            let default_mode = match caller.as_ref() {
                Some(c) => clamp_workflow_session_mode(&c.mode, requested_mode),
                None => requested_mode,
            };
            let max_concurrency = args
                .get("max_concurrency")
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(3)
                .clamp(1, 8);

            let run = if let Some(ref remote_host_id) = target_host {
                let cwd = str_arg(args, "cwd")?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StartDagWorkflow {
                            request_id: req_id,
                            name,
                            cwd,
                            steps,
                            default_provider,
                            default_mode,
                            max_concurrency,
                        },
                        Duration::from_secs(30),
                    )
                    .await?
                {
                    WireMessage::WorkflowRunUpdated { run, .. } => run,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to run_dag_workflow"),
                }
            } else {
                let cwd = opt_str(args, "cwd")
                    .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                    .ok_or_else(|| anyhow::anyhow!("missing required parameter `cwd`"))?;
                rt.workflows
                    .start_dag_workflow(StartDagWorkflowSpec {
                        name,
                        cwd,
                        owner_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        steps,
                        default_provider,
                        default_mode,
                        max_concurrency,
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "run": run,
            }))
        }

        "run_best_of_n" => {
            authorize_workflow_launch_from_caller(&rt.store, caller_agent_id)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let caller = caller_agent_id.and_then(|cid| rt.store.get_agent(cid).ok().flatten());
            let name = opt_str(args, "name").unwrap_or_default();
            let prompt = str_arg(args, "prompt")?;
            let raw_candidates = args
                .get("candidates")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("missing required parameter `candidates`"))?;
            let candidates: Vec<BestOfNCandidateSpec> = serde_json::from_value(raw_candidates)
                .map_err(|e| anyhow::anyhow!("invalid `candidates` array: {e}"))?;
            let requested_mode = opt_str(args, "mode")
                .map(|s| parse_session_mode(&s))
                .unwrap_or_else(|| {
                    caller
                        .as_ref()
                        .map(|c| c.mode.clone())
                        .unwrap_or(SessionMode::AcceptEdits)
                });
            let mode = match caller.as_ref() {
                Some(c) => clamp_workflow_session_mode(&c.mode, requested_mode),
                None => requested_mode,
            };
            let verify_command = opt_str(args, "verify_command");
            validate_mcp_verify_command(
                caller.as_ref().map(|c| &c.mode),
                verify_command.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!("{e}"))?;

            let run = if let Some(ref remote_host_id) = target_host {
                let cwd = str_arg(args, "cwd")?;
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::StartBestOfN {
                            request_id: req_id,
                            name,
                            cwd,
                            prompt,
                            candidates,
                            mode,
                            verify_command,
                        },
                        Duration::from_secs(30),
                    )
                    .await?
                {
                    WireMessage::WorkflowRunUpdated { run, .. } => run,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to run_best_of_n"),
                }
            } else {
                let cwd = opt_str(args, "cwd")
                    .or_else(|| caller.as_ref().map(|c| c.cwd.clone()))
                    .ok_or_else(|| anyhow::anyhow!("missing required parameter `cwd`"))?;
                rt.workflows
                    .start_best_of_n(StartBestOfNSpec {
                        name,
                        cwd,
                        owner_agent_id: caller_agent_id.map(ToOwned::to_owned),
                        prompt,
                        candidates,
                        mode,
                        verify_command,
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };

            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "run": run,
            }))
        }

        "get_workflow_status" => {
            let run_id = opt_str(args, "run_id");
            let include_completed = args
                .get("include_completed")
                .and_then(Value::as_bool)
                .unwrap_or(true);

            if let Some(ref remote_host_id) = target_host {
                let req_id = Uuid::new_v4();
                let runs = match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::ListWorkflowRuns {
                            request_id: req_id,
                            include_completed: true,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::WorkflowRunsSnapshot { runs, .. } => runs,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to get_workflow_status"),
                };
                if let Some(rid) = run_id {
                    let run = runs.into_iter().find(|r| r.run_id == rid).ok_or_else(|| {
                        anyhow::anyhow!("workflow run `{rid}` not found on host `{remote_host_id}`")
                    })?;
                    return Ok(json!({
                        "host_id": remote_host_id,
                        "run": run,
                    }));
                }
                let filtered: Vec<_> = if include_completed {
                    runs
                } else {
                    runs.into_iter().filter(|r| r.status.is_active()).collect()
                };
                return Ok(json!({
                    "host_id": remote_host_id,
                    "runs": filtered,
                }));
            }

            if let Some(rid) = run_id {
                let run = rt
                    .workflows
                    .get_run(&rid)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .ok_or_else(|| anyhow::anyhow!("workflow run `{rid}` not found"))?;
                Ok(json!({
                    "host_id": rt.host_info.host_id.clone(),
                    "run": run,
                }))
            } else {
                let runs = rt
                    .workflows
                    .list_runs(include_completed)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                Ok(json!({
                    "host_id": rt.host_info.host_id.clone(),
                    "runs": runs,
                }))
            }
        }

        "cancel_workflow" => {
            let run_id = str_arg(args, "run_id")?;
            let run = if let Some(ref remote_host_id) = target_host {
                if caller_agent_id.is_some() {
                    // Enforce caller ownership before forwarding cancel to remote host.
                    let list_req = Uuid::new_v4();
                    let remote_runs = match rt
                        .connected_hosts
                        .send_rpc(
                            remote_host_id,
                            list_req,
                            WireMessage::ListWorkflowRuns {
                                request_id: list_req,
                                include_completed: true,
                            },
                            Duration::from_secs(20),
                        )
                        .await?
                    {
                        WireMessage::WorkflowRunsSnapshot { runs, .. } => runs,
                        WireMessage::RpcError { message, .. } => {
                            anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                        }
                        _ => anyhow::bail!("unexpected remote reply to list_workflow_runs"),
                    };
                    let target_run = remote_runs
                        .into_iter()
                        .find(|r| r.run_id == run_id)
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "workflow run `{run_id}` not found on host `{remote_host_id}`"
                            )
                        })?;
                    authorize_workflow_cancel(caller_agent_id, &target_run)
                        .map_err(|e| anyhow::anyhow!("{e}"))?;
                }
                let req_id = Uuid::new_v4();
                match rt
                    .connected_hosts
                    .send_rpc(
                        remote_host_id,
                        req_id,
                        WireMessage::CancelWorkflowRun {
                            request_id: req_id,
                            run_id,
                        },
                        Duration::from_secs(20),
                    )
                    .await?
                {
                    WireMessage::WorkflowRunUpdated { run, .. } => run,
                    WireMessage::RpcError { message, .. } => {
                        anyhow::bail!("remote host `{remote_host_id}` error: {message}")
                    }
                    _ => anyhow::bail!("unexpected remote reply to cancel_workflow"),
                }
            } else {
                let existing = rt
                    .workflows
                    .get_run(&run_id)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
                    .ok_or_else(|| anyhow::anyhow!("workflow run `{run_id}` not found"))?;
                authorize_workflow_cancel(caller_agent_id, &existing)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                rt.workflows
                    .cancel_run(&run_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };
            Ok(json!({
                "host_id": target_host.unwrap_or_else(|| rt.host_info.host_id.clone()),
                "run": run,
            }))
        }

        other => anyhow::bail!("unknown Orbit tool `{other}`"),
    }
}

async fn authorize_browser_session_access(
    rt: &DaemonRuntime,
    remote_host_id: Option<&str>,
    caller_agent_id: Option<&str>,
    session_id: &str,
) -> anyhow::Result<()> {
    let Some(caller_id) = caller_agent_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    if let Some(remote_id) = remote_host_id {
        let req_id = Uuid::new_v4();
        let owner = match rt
            .connected_hosts
            .send_rpc(
                remote_id,
                req_id,
                WireMessage::ListBrowserSessions {
                    request_id: req_id,
                    include_screenshots: false,
                },
                Duration::from_secs(15),
            )
            .await?
        {
            WireMessage::BrowserSessionsSnapshot { sessions, .. } => {
                sessions
                    .into_iter()
                    .find(|s| s.session_id == session_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "browser session `{session_id}` not found on host `{remote_id}`"
                        )
                    })?
                    .agent_id
            }
            WireMessage::RpcError { message, .. } => {
                anyhow::bail!("remote host `{remote_id}` error: {message}")
            }
            _ => anyhow::bail!("unexpected remote reply while verifying browser session ownership"),
        };
        let Some(owner_id) = owner else {
            anyhow::bail!(
                "caller agent `{caller_id}` is not authorized to access unowned browser session `{session_id}`"
            );
        };
        if owner_id != caller_id {
            authorize_remote_agent_control(rt, remote_id, Some(caller_id), &owner_id).await?;
        }
        return Ok(());
    }

    let owner = rt
        .browser
        .get_session_owner(session_id)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let Some(owner_id) = owner else {
        anyhow::bail!(
            "caller agent `{caller_id}` is not authorized to access unowned browser session `{session_id}`"
        );
    };
    authorize_agent_control(&rt.store, Some(caller_id), &owner_id)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

struct BrowserToolActionOutput {
    session: orbit_protocol::BrowserSessionSummary,
    elements: Vec<orbit_protocol::BrowserElementRef>,
    screenshot_base64: Option<String>,
    text_excerpt: Option<String>,
    captcha_detected: bool,
    picked_element: Option<orbit_protocol::PickedElementInfo>,
}

async fn execute_browser_action_tool(
    rt: &DaemonRuntime,
    remote_host_id: Option<&str>,
    caller_agent_id: Option<&str>,
    session_id: &str,
    action: BrowserActionRequest,
) -> anyhow::Result<BrowserToolActionOutput> {
    if let Some(remote_id) = remote_host_id {
        let req_id = Uuid::new_v4();
        match rt
            .connected_hosts
            .send_rpc(
                remote_id,
                req_id,
                WireMessage::ExecuteBrowserAction {
                    request_id: req_id,
                    session_id: session_id.to_string(),
                    action,
                },
                Duration::from_secs(30),
            )
            .await?
        {
            WireMessage::BrowserActionResponse {
                session,
                elements,
                screenshot_base64,
                text_excerpt,
                captcha_detected,
                picked_element,
                ..
            } => Ok(BrowserToolActionOutput {
                session,
                elements,
                screenshot_base64,
                text_excerpt,
                captcha_detected,
                picked_element,
            }),
            WireMessage::RpcError { message, .. } => {
                anyhow::bail!("remote host `{remote_id}` error: {message}")
            }
            _ => anyhow::bail!("unexpected remote reply to browser action"),
        }
    } else {
        let mut extra_roots: Vec<PathBuf> = Vec::new();
        let owner_agent_id = rt
            .browser
            .get_session_owner(session_id)
            .await
            .ok()
            .flatten()
            .or_else(|| {
                caller_agent_id
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            });
        if let Some(aid) = owner_agent_id
            && let Ok(Some(agent)) = rt.store.get_agent(&aid)
        {
            extra_roots.push(PathBuf::from(&agent.cwd));
            if !agent.workspace_id.is_empty()
                && let Ok(Some(ws)) = rt.store.get_workspace(&agent.workspace_id)
            {
                extra_roots.push(PathBuf::from(&ws.cwd));
                if let Some(ref src) = ws.source_cwd {
                    extra_roots.push(PathBuf::from(src));
                }
            }
        }
        let outcome = rt
            .browser
            .execute_action_with_allowed_roots(session_id, action, &extra_roots)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(BrowserToolActionOutput {
            session: outcome.session,
            elements: outcome.elements,
            screenshot_base64: outcome.screenshot_base64,
            text_excerpt: outcome.text_excerpt,
            captcha_detected: outcome.captcha_detected,
            picked_element: outcome.picked_element,
        })
    }
}

async fn authorize_remote_agent_control(
    rt: &DaemonRuntime,
    remote_host_id: &str,
    caller_agent_id: Option<&str>,
    target_agent_id: &str,
) -> anyhow::Result<()> {
    let Some(caller_id) = caller_agent_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let req_id = Uuid::new_v4();
    let resp = rt
        .connected_hosts
        .send_rpc(
            remote_host_id,
            req_id,
            WireMessage::ListAgents {
                request_id: req_id,
                workspace_id: None,
                include_archived: true,
                include_background: true,
                parent_agent_id: None,
            },
            Duration::from_secs(15),
        )
        .await?;
    let agents = match resp {
        WireMessage::AgentsSnapshot { agents, .. } => agents,
        WireMessage::RpcError { message, .. } => {
            anyhow::bail!("remote host `{remote_host_id}` error: {message}")
        }
        _ => anyhow::bail!("unexpected remote reply while verifying agent ownership"),
    };
    let target = agents
        .into_iter()
        .find(|a| a.agent_id == target_agent_id)
        .ok_or_else(|| {
            anyhow::anyhow!("target agent `{target_agent_id}` not found on host `{remote_host_id}`")
        })?;
    if target.parent_agent_id.as_deref() == Some(caller_id)
        && target.parent_host_id.as_deref() == Some(rt.host_info.host_id.as_str())
    {
        return Ok(());
    }
    if rt
        .personal_router
        .is_authorized_global_router(caller_id, true)
        .await
    {
        return Ok(());
    }
    anyhow::bail!(
        "caller agent `{caller_id}` is not authorized to control remote agent `{target_agent_id}` on host `{remote_host_id}`"
    )
}

async fn resolve_target_host_id(
    rt: &DaemonRuntime,
    requested: Option<&str>,
) -> anyhow::Result<Option<String>> {
    let Some(raw) = requested.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if raw == rt.host_info.host_id
        || raw.eq_ignore_ascii_case("self")
        || raw.eq_ignore_ascii_case("local")
    {
        return Ok(None);
    }
    if let Some(resolved) = rt.connected_hosts.resolve_host_id(raw).await {
        return Ok(Some(resolved));
    }
    anyhow::bail!(
        "connected host `{raw}` not found or disabled (call `list_hosts` to inspect available hosts)"
    )
}

fn str_arg(args: &Value, key: &str) -> anyhow::Result<String> {
    opt_str(args, key).ok_or_else(|| anyhow::anyhow!("missing required parameter `{key}`"))
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_provider_and_model(raw: &str) -> anyhow::Result<(ProviderKind, Option<String>)> {
    let (prov_part, model_part) = match raw.split_once('/') {
        Some((p, m)) => (
            p.trim(),
            Some(m.trim().to_string()).filter(|s| !s.is_empty()),
        ),
        None => (raw.trim(), None),
    };
    let provider = ProviderKind::from_slug(prov_part).ok_or_else(|| {
        let supported: Vec<&str> = ProviderKind::all().iter().map(|p| p.slug()).collect();
        anyhow::anyhow!(
            "unsupported provider `{prov_part}` (expected one of: {}, optionally as `provider/model`)",
            supported.join(", ")
        )
    })?;
    Ok((provider, model_part))
}

fn parse_session_mode(raw: &str) -> SessionMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "default" => SessionMode::Default,
        "auto" | "guardian" => SessionMode::Auto,
        "plan" => SessionMode::Plan,
        "accept_edits" | "accept-edits" => SessionMode::AcceptEdits,
        "full_access" | "full-access" | "bypass" => SessionMode::FullAccess,
        other => SessionMode::Custom(other.to_string()),
    }
}

fn parse_turn_behavior(raw: Option<&str>) -> ActiveTurnBehavior {
    match raw.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("steer") => ActiveTurnBehavior::Steer,
        Some("interrupt") => ActiveTurnBehavior::Interrupt,
        Some("queue") | None => ActiveTurnBehavior::Queue,
        _ => ActiveTurnBehavior::Queue,
    }
}

fn parse_worktree_mode(raw: Option<&str>) -> WorktreeCreateMode {
    match raw.map(str::to_ascii_lowercase).as_deref() {
        Some("checkout_branch" | "checkout-branch") => WorktreeCreateMode::CheckoutBranch,
        Some("checkout_pr" | "checkout-pr") => WorktreeCreateMode::CheckoutPr,
        _ => WorktreeCreateMode::BranchOff,
    }
}

fn sha256_json(val: &Value) -> String {
    let bytes = serde_json::to_vec(val).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(&bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}… [truncated]")
    }
}

fn summarize_timeline_event(ev: &orbit_protocol::TimelineEvent) -> Value {
    match &ev.payload {
        TimelinePayload::UserPrompt { text, .. } => json!({
            "seq": ev.seq,
            "type": "user_prompt",
            "timestamp": ev.timestamp,
            "text": truncate_chars(text, 800),
        }),
        TimelinePayload::AssistantMessage {
            text, streaming, ..
        } => json!({
            "seq": ev.seq,
            "type": "assistant_message",
            "timestamp": ev.timestamp,
            "streaming": streaming,
            "text": truncate_chars(text, 1600),
        }),
        TimelinePayload::ToolCall { status, detail, .. } => {
            let label = match detail {
                ToolCallDetail::Shell {
                    command, exit_code, ..
                } => {
                    format!(
                        "shell: {} (exit={exit_code:?})",
                        truncate_chars(command, 160)
                    )
                }
                ToolCallDetail::FileEdit { path, .. } => format!("edit: {path}"),
                ToolCallDetail::FileRead { path, .. } => format!("read: {path}"),
                ToolCallDetail::Search { query, .. } => format!("search: {query}"),
                ToolCallDetail::Subagent { role, .. } => format!("subagent: {role}"),
                ToolCallDetail::Generic { tool_name, .. } => format!("tool: {tool_name}"),
            };
            json!({
                "seq": ev.seq,
                "type": "tool_call",
                "timestamp": ev.timestamp,
                "status": status,
                "summary": label,
            })
        }
        TimelinePayload::AgentMessage {
            kind,
            source_agent_id,
            event,
            text,
            ..
        } => json!({
            "seq": ev.seq,
            "type": "agent_message",
            "kind": kind,
            "source_agent_id": source_agent_id,
            "event": event,
            "text": truncate_chars(text, 1000),
        }),
        TimelinePayload::PermissionRequest {
            request_id, title, ..
        } => json!({
            "seq": ev.seq,
            "type": "permission_request",
            "request_id": request_id,
            "title": title,
        }),
        TimelinePayload::StatusChanged { status } => json!({
            "seq": ev.seq,
            "type": "status_changed",
            "timestamp": ev.timestamp,
            "status": status,
        }),
        TimelinePayload::Notice { level, message } => json!({
            "seq": ev.seq,
            "type": "notice",
            "level": level,
            "message": message,
        }),
        _ => json!({
            "seq": ev.seq,
            "type": "other",
            "timestamp": ev.timestamp,
        }),
    }
}

fn pack_tool_definitions() -> Vec<PackToolDefinition> {
    vec![
        // ==================== CORE PACK (11 tools, compact schemas) ====================
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "workspace_peers",
                description: "Inspect Mission Workspace sibling peers, Shared Blackboard revision, and recent events/conflicts; pass `handle` to peek a specific peer's edited files and recent entries without waking it.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "workspace_id": { "type": "string" },
                        "handle": { "type": "string", "description": "Optional @handle or agent_id to inspect a single peer's context." },
                        "limit": { "type": "integer", "default": 20 }
                    }
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "message_peer",
                description: "Send a collaborative message to a sibling peer by `@handle` (historical handle aliases resolve automatically) or `all` in the same Mission Workspace (with optional finish notification).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "handle": { "type": "string", "description": "Peer @handle or `all`." },
                        "message": { "type": "string" },
                        "active_turn_behavior": { "type": "string", "enum": ["queue", "steer", "interrupt"], "default": "queue", "description": "How to deliver when the recipient is mid-turn: `queue` waits without interrupting; `steer` injects natively only where supported (degrades to `queue` on ACP/Antigravity); `interrupt` explicitly preempts." },
                        "notify_on_finish": { "type": "boolean", "default": true },
                        "workspace_id": { "type": "string" }
                    },
                    "required": ["handle", "message"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "workspace_board",
                description: "Read or update the Mission Workspace Shared Blackboard (`BOARD.md`). Supports per-section upsert (`section`) and optimistic locking (`expected_revision` CAS).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["read", "update"], "default": "read" },
                        "content": { "type": "string", "description": "Markdown body when action=update." },
                        "section": { "type": "string", "description": "Optional `## Section` heading to upsert." },
                        "expected_revision": { "type": "integer", "description": "Optional optimistic lock revision for CAS." },
                        "workspace_id": { "type": "string" }
                    }
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "spawn_workspace_peer",
                description: "Spawn a sibling peer in the current Mission Workspace with a `@handle` (set `worktree=true` for isolated parallel file edits).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "handle": { "type": "string" },
                        "provider": { "type": "string", "description": "Provider ID or `provider/model`." },
                        "acp_agent_id": { "type": "string" },
                        "model": { "type": "string" },
                        "effort": { "type": "string", "enum": ["off", "low", "medium", "high", "xhigh", "max", "ultra"] },
                        "fast_mode": { "type": "boolean" },
                        "mode": { "type": "string", "enum": ["default", "auto", "plan", "accept_edits", "full_access"] },
                        "profile_id": { "type": "string" },
                        "cwd": { "type": "string" },
                        "worktree": { "type": "boolean" },
                        "branch": { "type": "string" },
                        "base_ref": { "type": "string" },
                        "title": { "type": "string" },
                        "initial_prompt": { "type": "string" },
                        "notify_on_finish": { "type": "boolean", "default": true },
                        "workspace_id": { "type": "string" }
                    },
                    "required": ["initial_prompt"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "handoff_worktree",
                description: "Preview (`dry_run: true`) or execute (`apply`/`move`) bidirectional Git Worktree <-> Local checkout handoff with backup ref safety and optional `--3way` conflict markers. Pass `agent_id`/`@handle` for Mission Workspace peer worktrees.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "workspace_id": { "type": "string" },
                        "agent_id": { "type": "string", "description": "Optional peer agent_id or @handle whose worktree to hand off." },
                        "direction": { "type": "string", "enum": ["worktree_to_local", "local_to_worktree"], "default": "worktree_to_local" },
                        "mode": { "type": "string", "enum": ["apply", "move"], "default": "apply" },
                        "target_workspace_id": { "type": "string" },
                        "include_committed": { "type": "boolean" },
                        "branch": { "type": "string" },
                        "allow_three_way_conflicts": { "type": "boolean", "default": false },
                        "dry_run": { "type": "boolean", "default": false },
                        "host_id": { "type": "string" }
                    }
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "create_agent",
                description: "Launch a new child/remote Orbit coding agent asynchronously with automatic finish notification.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "provider": { "type": "string" },
                        "acp_agent_id": { "type": "string" },
                        "model": { "type": "string" },
                        "effort": { "type": "string", "enum": ["off", "low", "medium", "high", "xhigh", "max", "ultra"] },
                        "fast_mode": { "type": "boolean" },
                        "mode": { "type": "string", "enum": ["default", "auto", "plan", "accept_edits", "full_access"] },
                        "profile_id": { "type": "string" },
                        "workspace_id": { "type": "string" },
                        "cwd": { "type": "string" },
                        "worktree": { "type": "boolean" },
                        "base_ref": { "type": "string" },
                        "branch": { "type": "string" },
                        "title": { "type": "string" },
                        "initial_prompt": { "type": "string" },
                        "background": { "type": "boolean" },
                        "auto_archive": { "type": "boolean" },
                        "notify_on_finish": { "type": "boolean", "default": true },
                        "host_id": { "type": "string" },
                        "idempotency_key": { "type": "string" }
                    },
                    "required": ["initial_prompt"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "send_agent_prompt",
                description: "Send a follow-up prompt to another agent by ID (`steer`, `queue`, or `interrupt`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "agent_id": { "type": "string" },
                        "prompt": { "type": "string" },
                        "active_turn_behavior": { "type": "string", "enum": ["queue", "steer", "interrupt"], "default": "queue", "description": "How to deliver when the recipient is mid-turn: `queue` waits without interrupting; `steer` injects natively only where supported (degrades to `queue` on ACP/Antigravity); `interrupt` explicitly preempts." },
                        "session_mode": { "type": "string", "enum": ["default", "auto", "plan", "accept_edits", "full_access"] },
                        "notify_on_finish": { "type": "boolean", "default": true },
                        "host_id": { "type": "string" },
                        "message_id": { "type": "string" }
                    },
                    "required": ["agent_id", "prompt"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "orbit_agent",
                description: "Manage Orbit agents: list, inspect status/activity, fork (same dir or worktree — inside a Mission Workspace `worktree=true` keeps the forked peer in the same workspace for parallel non-blocking development), update config/mode, move workspace, cancel, archive, or kill.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "status", "activity", "fork", "update", "set_mode", "move_workspace", "cancel", "archive", "kill"] },
                        "agent_id": { "type": "string" },
                        "workspace_id": { "type": "string" },
                        "target_workspace_id": { "type": "string" },
                        "new_workspace_title": { "type": "string" },
                        "since_seq": { "type": "integer" },
                        "limit": { "type": "integer" },
                        "up_to_seq": { "type": "integer" },
                        "worktree": { "type": "boolean" },
                        "branch": { "type": "string" },
                        "base_ref": { "type": "string" },
                        "title": { "type": "string" },
                        "initial_prompt": { "type": "string" },
                        "notify_on_finish": { "type": "boolean" },
                        "model": { "type": "string" },
                        "effort": { "type": "string" },
                        "fast_mode": { "type": "boolean" },
                        "mode": { "type": "string" },
                        "include_archived": { "type": "boolean" },
                        "include_background": { "type": "boolean" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "orbit_workspace",
                description: "Manage workspaces and `orbit.json`/`paseo.json` dev scripts: create, list, rename, archive, list_scripts, start_script, stop_script.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["create", "list", "rename", "archive", "list_scripts", "start_script", "stop_script"] },
                        "workspace_id": { "type": "string" },
                        "source_cwd": { "type": "string" },
                        "isolation": { "type": "string", "enum": ["local", "worktree"] },
                        "worktree_mode": { "type": "string", "enum": ["branch_off", "checkout_branch", "checkout_pr"] },
                        "branch": { "type": "string" },
                        "base_ref": { "type": "string" },
                        "pr_number": { "type": "integer" },
                        "title": { "type": "string" },
                        "script_name": { "type": "string" },
                        "include_archived": { "type": "boolean" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "orbit_timer",
                description: "Arm, list, or cancel durable one-shot conditional wakeup timers (`never`, `any`, `all_subagents`, `all_peers`, `sender:<id|@handle>`, `file:<path>`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["create", "list", "cancel"] },
                        "timer_id": { "type": "string" },
                        "duration_seconds": { "type": "integer", "minimum": 1, "maximum": 86400 },
                        "prompt": { "type": "string" },
                        "condition": { "type": "string" },
                        "agent_id": { "type": "string" },
                        "include_resolved": { "type": "boolean" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Core,
            def: ToolDefinition {
                name: "orbit_control",
                description: "Inspect host profiles, providers, models, connected remote hosts, cross-host projects & project context (`AGENTS.md`/`README.md`/git), or list/respond to pending child permission approvals.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list_profiles", "list_providers", "list_models", "list_hosts", "list_projects", "inspect_project_context", "list_pending_permissions", "respond_permission"] },
                        "provider": { "type": "string" },
                        "cwd": { "type": "string", "description": "Project directory path for `inspect_project_context`." },
                        "agent_id": { "type": "string" },
                        "request_id": { "type": "string" },
                        "decision": { "type": "string", "enum": ["allow", "allow_always", "deny"] },
                        "host_id": { "type": "string", "description": "Target host ID (or `all` for `list_projects`)." }
                    },
                    "required": ["action"]
                }),
            },
        },
        // ==================== OPTIONAL CAPABILITY PACKS (7 tools) ====================
        PackToolDefinition {
            pack: McpToolPack::Browser,
            def: ToolDefinition {
                name: "orbit_browser",
                description: "Pure-Rust CDP browser automation (`open`, `snapshot`, `screenshot`, `click`, `type`, `scroll`, `press_key`, `select_option`, `upload_file`, `pick_element`, `close`) with SSRF, upload-confinement & CAPTCHA guards.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["open", "snapshot", "screenshot", "click", "type", "scroll", "press_key", "select_option", "upload_file", "pick_element", "close"] },
                        "session_id": { "type": "string" },
                        "url": { "type": "string" },
                        "ref_id": { "type": "string" },
                        "selector": { "type": "string" },
                        "text": { "type": "string" },
                        "submit": { "type": "boolean" },
                        "key": { "type": "string" },
                        "modifiers": { "type": "array", "items": { "type": "string" } },
                        "value": { "type": "string" },
                        "label": { "type": "string" },
                        "index": { "type": "integer" },
                        "files": { "type": "array", "items": { "type": "string" } },
                        "file": { "type": "string" },
                        "interactive": { "type": "boolean" },
                        "x": { "type": "number" },
                        "y": { "type": "number" },
                        "delta_x": { "type": "number" },
                        "delta_y": { "type": "number" },
                        "full_page": { "type": "boolean" },
                        "headless": { "type": "boolean" },
                        "viewport_width": { "type": "integer" },
                        "viewport_height": { "type": "integer" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Schedules,
            def: ToolDefinition {
                name: "orbit_schedule",
                description: "Manage recurring cron schedules and agent heartbeats (`create`, `list`, `inspect`, `update`, `pause`, `resume`, `run_once`, `logs`, `delete`, `create_heartbeat`, `delete_heartbeat`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["create", "list", "inspect", "update", "pause", "resume", "run_once", "logs", "delete", "create_heartbeat", "delete_heartbeat"] },
                        "schedule_id": { "type": "string" },
                        "heartbeat_id": { "type": "string" },
                        "name": { "type": "string" },
                        "cron": { "type": "string" },
                        "timezone": { "type": "string", "description": "Optional IANA timezone (e.g. 'Asia/Shanghai', 'America/New_York') or fixed offset ('+08:00'); empty string resets to host local." },
                        "prompt": { "type": "string" },
                        "cwd": { "type": "string" },
                        "workspace_id": { "type": "string" },
                        "agent_id": { "type": "string" },
                        "provider": { "type": "string" },
                        "model": { "type": "string" },
                        "mode": { "type": "string" },
                        "worktree": { "type": "boolean" },
                        "max_runs": { "type": "integer" },
                        "expires_at": { "type": "string" },
                        "include_completed": { "type": "boolean" },
                        "limit": { "type": "integer" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Workflows,
            def: ToolDefinition {
                name: "orbit_workflow",
                description: "Launch and manage multi-agent workflows: `batch_fanout` (CSV), `dag` (up to 64 nodes), `best_of_n` (parallel worktree arena), `status`, `cancel`.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["batch_fanout", "dag", "best_of_n", "status", "cancel"] },
                        "run_id": { "type": "string" },
                        "name": { "type": "string" },
                        "cwd": { "type": "string" },
                        "csv_content": { "type": "string" },
                        "prompt_template": { "type": "string" },
                        "prompt": { "type": "string" },
                        "steps": { "type": "array", "items": { "type": "object" } },
                        "candidates": { "type": "array", "items": { "type": "object" } },
                        "provider": { "type": "string" },
                        "default_provider": { "type": "string" },
                        "model": { "type": "string" },
                        "mode": { "type": "string" },
                        "max_concurrency": { "type": "integer" },
                        "isolated_worktree": { "type": "boolean" },
                        "verify_command": { "type": "string" },
                        "include_completed": { "type": "boolean" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Goals,
            def: ToolDefinition {
                name: "orbit_goal",
                description: "Manage autonomous `/goal` verification loops and streaming background monitors (`start_goal`, `update_goal`, `complete_goal`, `start_monitor`, `stop_monitor`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["start_goal", "update_goal", "complete_goal", "start_monitor", "stop_monitor"] },
                        "goal_id": { "type": "string" },
                        "monitor_id": { "type": "string" },
                        "objective": { "type": "string" },
                        "criteria": { "type": "array" },
                        "criterion_id": { "type": "string" },
                        "passed": { "type": "boolean" },
                        "verify_command": { "type": "string" },
                        "max_iterations": { "type": "integer" },
                        "max_duration_secs": { "type": "integer" },
                        "summary": { "type": "string" },
                        "failed": { "type": "boolean" },
                        "kind": { "type": "string", "enum": ["command_stream", "log_file_tail", "http_health"] },
                        "name": { "type": "string" },
                        "command": { "type": "string" },
                        "args": { "type": "array", "items": { "type": "string" } },
                        "path": { "type": "string" },
                        "pattern": { "type": "string" },
                        "url": { "type": "string" },
                        "expect_status": { "type": "integer" },
                        "interval_secs": { "type": "integer" },
                        "expires_in_secs": { "type": "integer" },
                        "agent_id": { "type": "string" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Terminals,
            def: ToolDefinition {
                name: "orbit_terminal",
                description: "Manage detached PTY terminal sessions (`list`, `create`, `capture`, `send_keys`, `kill`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["list", "create", "capture", "send_keys", "kill"] },
                        "terminal_id": { "type": "string" },
                        "cwd": { "type": "string" },
                        "title": { "type": "string" },
                        "initial_command": { "type": "string" },
                        "text": { "type": "string" },
                        "keys": { "type": "array", "items": { "type": "string" } },
                        "lines": { "type": "integer" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Artifacts,
            def: ToolDefinition {
                name: "orbit_artifact",
                description: "Publish, list, read, or delete versioned workspace Artifacts (`markdown`, `html`, `code`, `svg`, `mermaid`, `mcp_app`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["publish", "list", "read", "delete"] },
                        "artifact_id": { "type": "string" },
                        "workspace_id": { "type": "string" },
                        "agent_id": { "type": "string" },
                        "title": { "type": "string" },
                        "kind": { "type": "string", "enum": ["markdown", "html", "code", "svg", "mermaid", "mcp_app"] },
                        "content": { "type": "string" },
                        "language": { "type": "string" },
                        "resource_uri": { "type": "string" },
                        "summary_note": { "type": "string" },
                        "version": { "type": "integer" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
        PackToolDefinition {
            pack: McpToolPack::Lsp,
            def: ToolDefinition {
                name: "orbit_lsp",
                description: "Language server intelligence via Orbit's host-local LSP manager (`diagnostics`, `symbols`, `goto_definition`).",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string", "enum": ["diagnostics", "symbols", "goto_definition"] },
                        "cwd": { "type": "string" },
                        "path": { "type": "string" },
                        "query": { "type": "string" },
                        "line": { "type": "integer" },
                        "col": { "type": "integer" },
                        "limit": { "type": "integer" },
                        "host_id": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_tool_catalog_stays_within_compact_token_budget() {
        let core_policy = McpToolPolicy {
            enabled: true,
            disabled_tools: Vec::new(),
            enabled_packs: Some(vec![McpToolPack::Core]),
            disabled_packs: Vec::new(),
        };
        let core_defs = list_tool_definitions(&core_policy);
        assert_eq!(
            core_defs.len(),
            11,
            "Core pack must expose 11 compact tools"
        );
        let core_json = serde_json::to_string(
            &core_defs
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema
                    })
                })
                .collect::<Vec<_>>(),
        )
        .expect("serialize core defs");
        assert!(
            core_json.len() <= 9_500,
            "Core MCP catalog JSON ({} bytes) exceeded 9,500-byte budget",
            core_json.len()
        );

        let full_defs = list_tool_definitions(&McpToolPolicy::default());
        assert_eq!(
            full_defs.len(),
            18,
            "Full MCP catalog must expose 18 composite tools (11 Core + 7 optional packs)"
        );
        let full_json = serde_json::to_string(
            &full_defs
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": t.input_schema
                    })
                })
                .collect::<Vec<_>>(),
        )
        .expect("serialize full defs");
        assert!(
            full_json.len() <= 18_500,
            "Full MCP catalog JSON ({} bytes) exceeded 18,500-byte budget",
            full_json.len()
        );

        for name in ["orbit_timer", "orbit_workflow", "orbit_browser"] {
            let def = full_defs
                .iter()
                .find(|d| d.name == name)
                .unwrap_or_else(|| panic!("missing MCP tool definition `{name}`"));
            assert!(
                def.input_schema["properties"].get("host_id").is_some(),
                "MCP tool `{name}` must expose `host_id` for multi-host routing"
            );
        }
    }

    #[test]
    fn startup_pack_detection_keeps_normal_chat_minimal_and_gates_subagents() {
        let plain =
            detect_startup_mcp_packs("Explain why this Rust function returns early", false, true);
        assert_eq!(plain, vec![McpToolPack::Core]);

        let browser_no_bin =
            detect_startup_mcp_packs("Open browser and take a screenshot", false, false);
        assert_eq!(browser_no_bin, vec![McpToolPack::Core]);

        let browser_with_bin =
            detect_startup_mcp_packs("Open browser and take a screenshot", false, true);
        assert_eq!(
            browser_with_bin,
            vec![McpToolPack::Core, McpToolPack::Browser]
        );

        let subagent_dag =
            detect_startup_mcp_packs("Run a DAG workflow and cron schedule every 5m", true, false);
        assert!(subagent_dag.contains(&McpToolPack::Core));
        assert!(subagent_dag.contains(&McpToolPack::Schedules));
        assert!(
            !subagent_dag.contains(&McpToolPack::Workflows),
            "subagents cannot launch recursive workflows and must not receive Workflows pack"
        );
    }

    #[test]
    fn composite_dispatch_maps_actions_and_enforces_pack_mapping() {
        let mapped = resolve_composite_dispatch("orbit_agent", &json!({"action": "kill"}))
            .expect("map kill");
        assert_eq!(mapped, "kill_agent");
        assert_eq!(tool_pack_for_canonical_name(mapped), McpToolPack::Core);

        let board_up =
            resolve_composite_dispatch("workspace_board", &json!({"content": "Updated contract"}))
                .expect("map implicit board update");
        assert_eq!(board_up, "workspace_board_update");

        let wf = resolve_composite_dispatch("orbit_workflow", &json!({"action": "best_of_n"}))
            .expect("map best_of_n");
        assert_eq!(wf, "run_best_of_n");
        assert_eq!(tool_pack_for_canonical_name(wf), McpToolPack::Workflows);
    }

    #[tokio::test]
    async fn agent_cannot_access_unowned_human_browser_session() {
        use futures_util::{SinkExt, StreamExt};
        use tokio::net::TcpListener;
        use tokio_tungstenite::tungstenite::Message as TungMsg;

        let tmp_home =
            std::env::temp_dir().join(format!("orbit-tools-browser-auth-{}", Uuid::new_v4()));
        let rt = DaemonRuntime::bootstrap(&tmp_home)
            .await
            .expect("bootstrap runtime");

        // Minimal loopback mock CDP endpoint
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("addr").port();
        let ws_url = format!("ws://127.0.0.1:{port}/devtools/page/auth_test");
        let server_task = tokio::spawn(async move {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
                return;
            };
            let (mut sink, mut rx) = ws.split();
            while let Some(Ok(TungMsg::Text(txt))) = rx.next().await {
                let Ok(val) = serde_json::from_str::<Value>(&txt) else {
                    continue;
                };
                let id = val.get("id").and_then(Value::as_u64).unwrap_or(0);
                let reply = json!({ "id": id, "result": {} });
                if sink
                    .send(TungMsg::Text(reply.to_string().into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        // Create an unowned (human desktop) browser session (`agent_id: None`).
        let sess = rt
            .browser
            .create_session_from_cdp_ws_url(&ws_url, CreateBrowserSessionSpec::default())
            .await
            .expect("create unowned session");

        // An agent caller MUST be denied access to an unowned session (P0 #4).
        let err =
            authorize_browser_session_access(&rt, None, Some("ag_intruder"), &sess.session_id)
                .await
                .expect_err("agent must not access unowned browser session");
        assert!(
            err.to_string()
                .contains("not authorized to access unowned browser session"),
            "unexpected error: {err}"
        );

        // Human operator (`caller_agent_id: None`) is permitted.
        authorize_browser_session_access(&rt, None, None, &sess.session_id)
            .await
            .expect("operator must be allowed");

        rt.browser.shutdown_all().await;
        server_task.abort();
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn mcp_update_schedule_preserves_omitted_and_resets_empty_timezone() {
        let tmp_home = std::env::temp_dir().join(format!(
            "orbit-tools-sched-tz-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&tmp_home).expect("create temp home");
        let rt = DaemonRuntime::bootstrap(&tmp_home)
            .await
            .expect("bootstrap");

        let created = execute_tool(
            &rt,
            None,
            "create_schedule",
            json!({
                "name": "TZ MCP Test",
                "cron": "0 9 * * *",
                "timezone": "Asia/Shanghai",
                "prompt": "check",
                "cwd": tmp_home.to_string_lossy(),
            }),
        )
        .await;
        assert!(!created.is_error, "create failed: {}", created.text);
        let sid = created.structured["schedule"]["schedule_id"]
            .as_str()
            .expect("schedule_id")
            .to_string();
        assert_eq!(created.structured["schedule"]["timezone"], "Asia/Shanghai");

        // Omitting `timezone` keeps existing `Asia/Shanghai`.
        let renamed = execute_tool(
            &rt,
            None,
            "update_schedule",
            json!({
                "schedule_id": sid,
                "name": "TZ MCP Renamed",
            }),
        )
        .await;
        assert!(!renamed.is_error, "rename failed: {}", renamed.text);
        assert_eq!(renamed.structured["schedule"]["timezone"], "Asia/Shanghai");

        // Updating `timezone` to canonical fixed offset works.
        let updated = execute_tool(
            &rt,
            None,
            "update_schedule",
            json!({
                "schedule_id": sid,
                "timezone": "+09:00",
            }),
        )
        .await;
        assert!(!updated.is_error, "update tz failed: {}", updated.text);
        assert_eq!(updated.structured["schedule"]["timezone"], "+09:00");

        // Explicit empty string resets `timezone` back to host local (`None`).
        let reset = execute_tool(
            &rt,
            None,
            "update_schedule",
            json!({
                "schedule_id": sid,
                "timezone": "",
            }),
        )
        .await;
        assert!(!reset.is_error, "reset tz failed: {}", reset.text);
        assert!(reset.structured["schedule"].get("timezone").is_none());

        // Non-string `timezone` (including null / number) is rejected.
        let bad_type = execute_tool(
            &rt,
            None,
            "update_schedule",
            json!({
                "schedule_id": sid,
                "timezone": 123,
            }),
        )
        .await;
        assert!(bad_type.is_error);
        let null_type = execute_tool(
            &rt,
            None,
            "update_schedule",
            json!({
                "schedule_id": sid,
                "timezone": null,
            }),
        )
        .await;
        assert!(null_type.is_error);

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn workspace_board_cas_conflict_and_caller_core_pack_default() {
        let tmp_home =
            std::env::temp_dir().join(format!("orbit-tools-cas-pack-{}", Uuid::new_v4()));
        let ws_dir = tmp_home.join("repo");
        std::fs::create_dir_all(&ws_dir).expect("mkdir repo");
        let rt = DaemonRuntime::bootstrap(&tmp_home)
            .await
            .expect("bootstrap runtime");

        let now = Utc::now();
        rt.store
            .upsert_workspace(&orbit_protocol::WorkspaceSummary {
                workspace_id: "ws_cas_1".into(),
                title: "Mission".into(),
                cwd: ws_dir.to_string_lossy().to_string(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: true,
            })
            .expect("upsert ws");

        let mut labels = HashMap::new();
        labels.insert(LABEL_MCP_PACKS.into(), "core".into());
        rt.store
            .upsert_agent(
                &orbit_protocol::AgentSummary {
                    agent_id: "ag_cas_1".into(),
                    workspace_id: "ws_cas_1".into(),
                    cwd: ws_dir.to_string_lossy().to_string(),
                    title: "Peer".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: orbit_protocol::AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels,
                    attention: orbit_protocol::AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: Some("dev".into()),
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        let caller_tools =
            list_tool_definitions_for_caller(&rt, Some("ag_cas_1"), &McpToolPolicy::default());
        assert_eq!(caller_tools.len(), 11);

        let ok_res = execute_tool(
            &rt,
            Some("ag_cas_1"),
            "workspace_board",
            json!({
                "action": "update",
                "section": "Decisions & Contracts",
                "content": "Contract v1",
                "expected_revision": 0
            }),
        )
        .await;
        assert!(!ok_res.is_error, "CAS rev 0 -> 1 must succeed");
        assert_eq!(ok_res.structured["board"]["revision"], 1);

        let conflict_res = execute_tool(
            &rt,
            Some("ag_cas_1"),
            "workspace_board",
            json!({
                "action": "update",
                "section": "Decisions & Contracts",
                "content": "Stale overwrite",
                "expected_revision": 0
            }),
        )
        .await;
        assert!(conflict_res.is_error, "stale expected_revision must fail");
        assert_eq!(conflict_res.structured["conflict"], true);
        assert_eq!(conflict_res.structured["expected_revision"], 0);
        assert_eq!(conflict_res.structured["current_revision"], 1);
        assert_eq!(conflict_res.structured["current_board"]["revision"], 1);

        let legacy_read =
            execute_tool(&rt, Some("ag_cas_1"), "workspace_board_read", json!({})).await;
        assert!(!legacy_read.is_error);
        let board_txt = legacy_read.structured["board"]["content"]
            .as_str()
            .unwrap_or("");
        assert!(board_txt.contains("Contract v1"));
        assert!(!board_txt.contains("Stale overwrite"));

        rt.browser.shutdown_all().await;
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[test]
    fn parse_turn_behavior_defaults_to_queue_and_respects_explicit_values() {
        assert_eq!(parse_turn_behavior(None), ActiveTurnBehavior::Queue);
        assert_eq!(parse_turn_behavior(Some("")), ActiveTurnBehavior::Queue);
        assert_eq!(
            parse_turn_behavior(Some("queue")),
            ActiveTurnBehavior::Queue
        );
        assert_eq!(
            parse_turn_behavior(Some("STEER")),
            ActiveTurnBehavior::Steer
        );
        assert_eq!(
            parse_turn_behavior(Some("interrupt")),
            ActiveTurnBehavior::Interrupt
        );
    }
}
