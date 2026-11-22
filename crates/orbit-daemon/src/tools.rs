//! Transport-neutral Orbit Orchestration Tool Catalog (~39 tools).
//!
//! Exposed to agents via the built-in MCP Server (`POST /mcp/agents`) and `orbit mcp` stdio bridge.
//! Handles caller identity resolution, per-profile tool policies, idempotency receipts,
//! durable finish subscriptions, and single-hop cross-host delegation.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, ForkTarget, McpToolPolicy, ProviderKind, ReasoningEffort,
    ScheduleNewAgentConfig, ScheduleStatus, ScheduleTarget, SessionMode, TimelinePayload,
    ToolCallDetail, WireMessage, WorkspaceIsolation, WorktreeCreateMode,
};
use orbit_store::ReceiptOutcome;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::DaemonRuntime;
use crate::manager::{CreateAgentSpec, CreateWorkspaceSpec, ForkAgentSpec};
use crate::messaging::{AgentPromptSource, QueuedAgentMessage};

#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
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

pub fn list_tool_definitions(policy: &McpToolPolicy) -> Vec<ToolDefinition> {
    if !policy.enabled {
        return Vec::new();
    }
    all_tool_definitions()
        .into_iter()
        .filter(|t| !policy.disabled_tools.iter().any(|d| d == t.name))
        .collect()
}

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

    match execute_tool_inner(runtime, caller_agent_id, name, &args).await {
        Ok(val) => ToolExecResult::ok(val),
        Err(e) => ToolExecResult::err(e.to_string()),
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
            let s = rt.scheduler.update_schedule(
                &sid,
                opt_str(args, "name"),
                opt_str(args, "cron").as_deref(),
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
                        "modes": ["default", "plan", "accept_edits", "full_access"],
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
            let peer_cards: Vec<Value> = peers
                .iter()
                .map(|p| {
                    let last_reply = rt
                        .store
                        .last_assistant_message_text(&p.agent_id)
                        .ok()
                        .flatten()
                        .map(|s| truncate_chars(&s, 280));
                    json!({
                        "agent_id": p.agent_id,
                        "handle": p.handle.as_deref().unwrap_or("peer"),
                        "is_self": caller_agent_id == Some(p.agent_id.as_str()),
                        "title": p.title,
                        "provider": p.provider,
                        "model": p.model,
                        "mode": p.mode,
                        "status": p.status,
                        "cwd": p.cwd,
                        "last_summary": last_reply,
                        "updated_at": p.updated_at,
                    })
                })
                .collect();
            Ok(json!({
                "workspace": ws,
                "peers": peer_cards,
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
                delivered.push(json!({
                    "agent_id": peer.agent_id,
                    "handle": peer_h,
                    "message_id": mid,
                }));
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
            let updated_by = caller
                .as_ref()
                .and_then(|c| c.handle.clone())
                .or_else(|| opt_str(args, "updated_by"))
                .unwrap_or_else(|| "peer".into());
            let board = rt.orchestrator.update_workspace_board(
                &ws_id,
                &content,
                section.as_deref(),
                Some(&updated_by),
                caller_agent_id,
            )?;
            Ok(json!({ "board": board }))
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

            Ok(json!({
                "ok": true,
                "agent_id": agent.agent_id,
                "workspace_id": agent.workspace_id,
                "cwd": agent.cwd,
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

        other => anyhow::bail!("unknown Orbit tool `{other}`"),
    }
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
        "plan" => SessionMode::Plan,
        "accept_edits" | "accept-edits" => SessionMode::AcceptEdits,
        "full_access" | "full-access" | "bypass" => SessionMode::FullAccess,
        other => SessionMode::Custom(other.to_string()),
    }
}

fn parse_turn_behavior(raw: Option<&str>) -> ActiveTurnBehavior {
    match raw.map(str::to_ascii_lowercase).as_deref() {
        Some("queue") => ActiveTurnBehavior::Queue,
        Some("interrupt") => ActiveTurnBehavior::Interrupt,
        _ => ActiveTurnBehavior::Steer,
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

fn all_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        // --- Agents (10) ---
        ToolDefinition {
            name: "create_agent",
            description: "Create and launch a new Orbit coding agent asynchronously (optionally inside a specific workspace, isolated git worktree, profile, or remote connected host). Automatically registers a durable finish notification back to the calling agent by default.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string", "description": "Provider ID (`claude_code`, `codex`, `antigravity`, `opencode`, `gemini_cli`, `cursor_agent`, `copilot`, `goose`, `pi`, `kimi`, `cline`, `mistral_vibe`, `augment`) or `provider/model` pair." },
                    "model": { "type": "string" },
                    "effort": { "type": "string", "enum": ["off", "low", "medium", "high", "xhigh", "max", "ultra"] },
                    "fast_mode": { "type": "boolean", "description": "Enable Fast output mode (lower latency, higher quota usage) when supported by the model." },
                    "mode": { "type": "string", "enum": ["default", "plan", "accept_edits", "full_access"] },
                    "profile_id": { "type": "string", "description": "Optional saved profile ID or name from `list_profiles`." },
                    "workspace_id": { "type": "string", "description": "Existing workspace ID from `create_workspace` or `list_workspaces`." },
                    "cwd": { "type": "string", "description": "Working directory (defaults to caller's directory on local host)." },
                    "worktree": { "type": "boolean", "description": "Create a fresh isolated git worktree for this agent." },
                    "base_ref": { "type": "string" },
                    "branch": { "type": "string" },
                    "title": { "type": "string" },
                    "initial_prompt": { "type": "string" },
                    "background": { "type": "boolean" },
                    "auto_archive": { "type": "boolean" },
                    "notify_on_finish": { "type": "boolean", "default": true },
                    "host_id": { "type": "string", "description": "Optional remote host ID from `list_hosts`." },
                    "idempotency_key": { "type": "string" }
                },
                "required": ["initial_prompt"]
            }),
        },
        ToolDefinition {
            name: "fork_agent",
            description: "Fork/branch an existing agent conversation into a new session (either in the same directory or inside a freshly created isolated git worktree), optionally truncating history at `up_to_seq`.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source_agent_id": { "type": "string", "description": "Source agent ID to fork from (defaults to the calling agent)." },
                    "up_to_seq": { "type": "integer", "description": "Optional timeline sequence upper bound (inclusive). Omit to fork the full conversation." },
                    "worktree": { "type": "boolean", "description": "When true, creates a fresh isolated git worktree workspace for the branch." },
                    "branch": { "type": "string", "description": "Optional git branch name when forking into a worktree." },
                    "base_ref": { "type": "string", "description": "Optional git base ref when forking into a worktree." },
                    "title": { "type": "string", "description": "Optional custom title for the forked conversation." },
                    "initial_prompt": { "type": "string", "description": "Optional immediate follow-up prompt to dispatch on the forked branch." },
                    "host_id": { "type": "string", "description": "Optional remote host ID from `list_hosts`." }
                }
            }),
        },
        ToolDefinition {
            name: "send_agent_prompt",
            description: "Send a follow-up prompt or message to another agent by ID (supports steering a running turn, queueing, or interrupting, and optional finish notification).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "prompt": { "type": "string" },
                    "active_turn_behavior": { "type": "string", "enum": ["steer", "queue", "interrupt"], "default": "steer" },
                    "session_mode": { "type": "string", "enum": ["default", "plan", "accept_edits", "full_access"] },
                    "notify_on_finish": { "type": "boolean", "default": true },
                    "host_id": { "type": "string" },
                    "message_id": { "type": "string" }
                },
                "required": ["agent_id", "prompt"]
            }),
        },
        ToolDefinition {
            name: "get_agent_status",
            description: "Inspect the latest status snapshot, pending permission requests, and last assistant summary of an agent.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "get_agent_activity",
            description: "Fetch a curated summary of recent timeline entries (messages, tool calls, status transitions) for an agent.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "since_seq": { "type": "integer" },
                    "limit": { "type": "integer", "default": 40 },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "list_agents",
            description: "List managed Orbit agents on this host or a connected remote host.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" },
                    "cwd": { "type": "string" },
                    "parent_agent_id": { "type": "string" },
                    "include_archived": { "type": "boolean", "default": false },
                    "include_background": { "type": "boolean", "default": false },
                    "host_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "cancel_agent",
            description: "Interrupt an agent's active turn while keeping its session alive for follow-up prompts.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "archive_agent",
            description: "Soft-archive an agent, closing its process and cascading to non-detached same-workspace children.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "kill_agent",
            description: "Terminate an agent's underlying provider runtime process immediately.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "update_agent",
            description: "Update an agent's title, model, reasoning effort, fast mode, session mode, or labels.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "title": { "type": "string" },
                    "model": { "type": "string" },
                    "effort": { "type": "string" },
                    "fast_mode": { "type": "boolean", "description": "Toggle Fast output mode (lower latency, higher quota usage)." },
                    "mode": { "type": "string" },
                    "labels": { "type": "object" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id"]
            }),
        },
        ToolDefinition {
            name: "set_agent_mode",
            description: "Switch an agent's permission/execution mode (`default`, `plan`, `accept_edits`, `full_access`).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "mode": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id", "mode"]
            }),
        },
        // --- Workspaces & Scripts (7) ---
        ToolDefinition {
            name: "create_workspace",
            description: "Create a local or git-worktree-isolated workspace (branch off, checkout existing branch, or checkout a PR) with `.worktreeinclude` copy and `orbit.json`/`paseo.json` setup hooks.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "source_cwd": { "type": "string" },
                    "isolation": { "type": "string", "enum": ["local", "worktree"], "default": "local" },
                    "worktree_mode": { "type": "string", "enum": ["branch_off", "checkout_branch", "checkout_pr"], "default": "branch_off" },
                    "new_branch": { "type": "string" },
                    "branch": { "type": "string" },
                    "base_ref": { "type": "string" },
                    "pr_number": { "type": "integer" },
                    "title": { "type": "string" },
                    "background": { "type": "boolean" },
                    "host_id": { "type": "string" }
                },
                "required": ["source_cwd"]
            }),
        },
        ToolDefinition {
            name: "list_workspaces",
            description: "List active workspaces, their directories, branches, and isolation modes.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "include_archived": { "type": "boolean", "default": false },
                    "include_background": { "type": "boolean", "default": false },
                    "host_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "rename_workspace",
            description: "Rename a workspace.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" },
                    "title": { "type": "string" }
                },
                "required": ["title"]
            }),
        },
        ToolDefinition {
            name: "archive_workspace",
            description: "Archive a workspace and its sessions; runs teardown and deletes the managed git worktree once its final workspace is archived.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" },
                    "host_id": { "type": "string" }
                },
                "required": ["workspace_id"]
            }),
        },
        ToolDefinition {
            name: "list_workspace_scripts",
            description: "List scripts and long-running services configured in `orbit.json` / `paseo.json` along with live ports and `*.localhost` proxy URLs.",
            input_schema: json!({
                "type": "object",
                "properties": { "workspace_id": { "type": "string" } }
            }),
        },
        ToolDefinition {
            name: "start_workspace_script",
            description: "Start a configured workspace script or service through Orbit's managed launcher and reverse proxy.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" },
                    "script_name": { "type": "string" }
                },
                "required": ["workspace_id", "script_name"]
            }),
        },
        ToolDefinition {
            name: "stop_workspace_script",
            description: "Stop a running workspace script or service.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" },
                    "script_name": { "type": "string" }
                },
                "required": ["workspace_id", "script_name"]
            }),
        },
        // --- Terminals (5) ---
        ToolDefinition {
            name: "list_terminals",
            description: "List active detached PTY terminal sessions.",
            input_schema: json!({
                "type": "object",
                "properties": { "cwd": { "type": "string" } }
            }),
        },
        ToolDefinition {
            name: "create_terminal",
            description: "Create a detached PTY terminal session (survives daemon restarts).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "cwd": { "type": "string" },
                    "title": { "type": "string" },
                    "cols": { "type": "integer", "default": 120 },
                    "rows": { "type": "integer", "default": 36 },
                    "initial_command": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "capture_terminal",
            description: "Capture clean plain-text screen output from a terminal session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "terminal_id": { "type": "string" },
                    "lines": { "type": "integer", "default": 200 }
                },
                "required": ["terminal_id"]
            }),
        },
        ToolDefinition {
            name: "send_terminal_keys",
            description: "Send text and/or special key tokens (`Enter`, `Ctrl+C`, `Tab`, `Esc`, `Up`, `Down`) to a terminal session.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "terminal_id": { "type": "string" },
                    "text": { "type": "string" },
                    "keys": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["terminal_id"]
            }),
        },
        ToolDefinition {
            name: "kill_terminal",
            description: "Terminate a PTY terminal session.",
            input_schema: json!({
                "type": "object",
                "properties": { "terminal_id": { "type": "string" } },
                "required": ["terminal_id"]
            }),
        },
        // --- Schedules & Heartbeats (11) ---
        ToolDefinition {
            name: "create_schedule",
            description: "Create a cron schedule (supports 5-field cron or presets like `5m`, `1h`, `daily 09:00`) that launches a fresh agent on each trigger.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "cron": { "type": "string" },
                    "prompt": { "type": "string" },
                    "cwd": { "type": "string" },
                    "workspace_id": { "type": "string" },
                    "provider": { "type": "string" },
                    "model": { "type": "string" },
                    "effort": { "type": "string" },
                    "fast_mode": { "type": "boolean", "description": "Enable Fast output mode (lower latency, higher quota usage) when supported by the model." },
                    "mode": { "type": "string" },
                    "profile_id": { "type": "string" },
                    "worktree": { "type": "boolean" },
                    "base_ref": { "type": "string" },
                    "max_runs": { "type": "integer" },
                    "expires_at": { "type": "string" }
                },
                "required": ["name", "cron", "prompt"]
            }),
        },
        ToolDefinition {
            name: "list_schedules",
            description: "List daemon-managed cron schedules and heartbeats.",
            input_schema: json!({
                "type": "object",
                "properties": { "include_completed": { "type": "boolean", "default": false } }
            }),
        },
        ToolDefinition {
            name: "inspect_schedule",
            description: "Inspect a schedule definition and its recent execution history.",
            input_schema: json!({
                "type": "object",
                "properties": { "schedule_id": { "type": "string" } },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "pause_schedule",
            description: "Pause an active cron schedule.",
            input_schema: json!({
                "type": "object",
                "properties": { "schedule_id": { "type": "string" } },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "resume_schedule",
            description: "Resume a paused cron schedule.",
            input_schema: json!({
                "type": "object",
                "properties": { "schedule_id": { "type": "string" } },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "update_schedule",
            description: "Update a schedule's name, cron cadence, prompt, max_runs, or expiration.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "schedule_id": { "type": "string" },
                    "name": { "type": "string" },
                    "cron": { "type": "string" },
                    "prompt": { "type": "string" },
                    "max_runs": { "type": "integer" },
                    "expires_at": { "type": "string" }
                },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "schedule_logs",
            description: "Return recent execution records and summaries for a schedule.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "schedule_id": { "type": "string" },
                    "limit": { "type": "integer", "default": 20 }
                },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "run_schedule_once",
            description: "Trigger one immediate run of a schedule without modifying its cron.",
            input_schema: json!({
                "type": "object",
                "properties": { "schedule_id": { "type": "string" } },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "delete_schedule",
            description: "Delete a schedule permanently.",
            input_schema: json!({
                "type": "object",
                "properties": { "schedule_id": { "type": "string" } },
                "required": ["schedule_id"]
            }),
        },
        ToolDefinition {
            name: "create_heartbeat",
            description: "Attach a recurring cron-backed wake-up prompt to the calling agent (or specified agent_id) to keep long tasks moving.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "cron": { "type": "string", "description": "Cron expression or preset like `5m`, `10m`, `1h`." },
                    "prompt": { "type": "string" },
                    "name": { "type": "string" },
                    "agent_id": { "type": "string" },
                    "max_runs": { "type": "integer" },
                    "expires_at": { "type": "string" }
                },
                "required": ["cron", "prompt"]
            }),
        },
        ToolDefinition {
            name: "delete_heartbeat",
            description: "Delete an active heartbeat once a long-running task is complete.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "heartbeat_id": { "type": "string" },
                    "schedule_id": { "type": "string" }
                }
            }),
        },
        // --- Profiles / Discovery / Permissions / Hosts (6) ---
        ToolDefinition {
            name: "list_profiles",
            description: "List saved Agent Profiles including `when_to_use` delegation guidance, provider, model, effort, and mode.",
            input_schema: json!({
                "type": "object",
                "properties": { "host_id": { "type": "string" } }
            }),
        },
        ToolDefinition {
            name: "list_providers",
            description: "List installed coding agent CLIs and supported modes on this host.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        ToolDefinition {
            name: "list_models",
            description: "List dynamically discovered models and reasoning-effort options for installed providers.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "provider": { "type": "string" },
                    "host_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "list_pending_permissions",
            description: "List currently pending tool permission approvals across managed agents.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "host_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "respond_to_permission",
            description: "Approve or deny a pending tool permission request on a delegated child agent.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "request_id": { "type": "string" },
                    "decision": { "type": "string", "enum": ["allow", "allow_always", "deny"] },
                    "host_id": { "type": "string" }
                },
                "required": ["agent_id", "request_id", "decision"]
            }),
        },
        ToolDefinition {
            name: "list_hosts",
            description: "List this daemon (`self`) and all connected remote Orbit hosts available for cross-machine delegation.",
            input_schema: json!({ "type": "object", "properties": {} }),
        },
        // --- Mission Workspace Collaboration (7) ---
        ToolDefinition {
            name: "workspace_peers",
            description: "Inspect your current Mission Workspace: list all sibling peers (@handles, providers, statuses, cwds, latest summaries), Shared Blackboard revision/path, and recent workspace events/conflicts.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string", "description": "Optional workspace ID (defaults to caller's Mission Workspace)." }
                }
            }),
        },
        ToolDefinition {
            name: "message_peer",
            description: "Send a direct collaborative message to a sibling peer in the same Mission Workspace by `@handle` (e.g. `reviewer`, `backend`) or `all` to broadcast to every sibling peer.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string", "description": "Peer @handle (e.g. `reviewer`, `impl`) or `all`." },
                    "message": { "type": "string", "description": "Message or question for the peer." },
                    "active_turn_behavior": { "type": "string", "enum": ["steer", "queue", "interrupt"], "default": "steer" },
                    "notify_on_finish": { "type": "boolean", "default": true },
                    "workspace_id": { "type": "string" }
                },
                "required": ["handle", "message"]
            }),
        },
        ToolDefinition {
            name: "read_peer_context",
            description: "Read-only peek into a sibling peer's status, edited files, latest summary, and recent activity by `@handle` without interrupting or waking that peer.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string", "description": "Peer @handle or agent_id inside the Mission Workspace." },
                    "limit": { "type": "integer", "default": 20 },
                    "workspace_id": { "type": "string" }
                },
                "required": ["handle"]
            }),
        },
        ToolDefinition {
            name: "workspace_board_read",
            description: "Read the Mission Workspace's Shared Blackboard (`BOARD.md`) containing shared goals, API contracts, task ownership, and review findings.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "workspace_id": { "type": "string" }
                }
            }),
        },
        ToolDefinition {
            name: "workspace_board_update",
            description: "Update the Mission Workspace's Shared Blackboard (`BOARD.md`) — either a specific Markdown `## Section` (e.g. `Decisions & Contracts`, `Tasks`, `Findings`) or the full document.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "Markdown body for the section or full document." },
                    "section": { "type": "string", "description": "Optional Markdown `## Section` heading to upsert without disturbing other sections." },
                    "workspace_id": { "type": "string" }
                },
                "required": ["content"]
            }),
        },
        ToolDefinition {
            name: "spawn_workspace_peer",
            description: "Spawn a visible sibling peer inside your current Mission Workspace with a readable `@handle` (optionally in a different provider/model family and/or an isolated git worktree while sharing the same Mission Workspace & Shared Blackboard).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "handle": { "type": "string", "description": "Short peer handle (e.g. `reviewer`, `backend`, `tester`)." },
                    "provider": { "type": "string", "description": "Provider ID (`claude_code`, `codex`, `antigravity`, `opencode`, `gemini_cli`, `cursor_agent`, `copilot`, `goose`, `pi`, `kimi`, `cline`, `mistral_vibe`, `augment`) or `provider/model`." },
                    "model": { "type": "string" },
                    "effort": { "type": "string", "enum": ["off", "low", "medium", "high", "xhigh", "max", "ultra"] },
                    "fast_mode": { "type": "boolean", "description": "Enable Fast output mode (lower latency, higher quota usage) when supported by the model." },
                    "mode": { "type": "string", "enum": ["default", "plan", "accept_edits", "full_access"] },
                    "profile_id": { "type": "string" },
                    "cwd": { "type": "string" },
                    "worktree": { "type": "boolean", "description": "Isolate this peer in its own git worktree while staying in the same Mission Workspace." },
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
        ToolDefinition {
            name: "move_agent_to_workspace",
            description: "Move an agent into an existing Mission Workspace or split it out into a standalone Mission Workspace (omit `target_workspace_id`).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "target_workspace_id": { "type": "string" },
                    "new_workspace_title": { "type": "string" }
                }
            }),
        },
    ]
}
