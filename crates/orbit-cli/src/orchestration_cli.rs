//! Docker-style Orbit Orchestration CLI subcommands (`workspace`, `script`, `terminal`,
//! `schedule`, `heartbeat`, `profile`, `host`, `permit`, `config`, `skill`, plus `wait`,
//! `stop`, `inspect`, `archive`, `unarchive`, `detach`, `delete`, `mode`).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Subcommand, ValueEnum};
use orbit_client_core::export::{
    ExportAgentMeta, ExportFormat, ExportOptions, export_timeline_transcript, write_file_atomically,
};
use orbit_client_core::{ClientInboundEvent, HostSessionHandle};
use orbit_protocol::{
    ActiveTurnBehavior, AgentProfile, AgentStatus, ForkTarget, McpToolPolicy, ProviderKind,
    ReasoningEffort, ScheduleKind, ScheduleNewAgentConfig, ScheduleTarget, SessionMode,
    TimelinePayload, WireMessage, WorkspaceIsolation, WorktreeCreateMode,
};
use serde_json::json;
use uuid::Uuid;

use crate::{CliMode, CliProvider, connect_cli_server, format_status, print_timeline_payload};

#[derive(Clone, Copy, ValueEnum)]
pub enum CliBehavior {
    Steer,
    Queue,
    Interrupt,
}

impl From<CliBehavior> for ActiveTurnBehavior {
    fn from(v: CliBehavior) -> Self {
        match v {
            CliBehavior::Steer => ActiveTurnBehavior::Steer,
            CliBehavior::Queue => ActiveTurnBehavior::Queue,
            CliBehavior::Interrupt => ActiveTurnBehavior::Interrupt,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliEffort {
    Low,
    Medium,
    High,
    Max,
}

impl From<CliEffort> for ReasoningEffort {
    fn from(v: CliEffort) -> Self {
        match v {
            CliEffort::Low => ReasoningEffort::Low,
            CliEffort::Medium => ReasoningEffort::Medium,
            CliEffort::High => ReasoningEffort::High,
            CliEffort::Max => ReasoningEffort::Max,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliWorktreeMode {
    BranchOff,
    CheckoutBranch,
    CheckoutPr,
}

impl From<CliWorktreeMode> for WorktreeCreateMode {
    fn from(v: CliWorktreeMode) -> Self {
        match v {
            CliWorktreeMode::BranchOff => WorktreeCreateMode::BranchOff,
            CliWorktreeMode::CheckoutBranch => WorktreeCreateMode::CheckoutBranch,
            CliWorktreeMode::CheckoutPr => WorktreeCreateMode::CheckoutPr,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliExportFormat {
    #[value(alias = "markdown")]
    Md,
    Json,
    Html,
}

impl From<CliExportFormat> for ExportFormat {
    fn from(v: CliExportFormat) -> Self {
        match v {
            CliExportFormat::Md => ExportFormat::Markdown,
            CliExportFormat::Json => ExportFormat::Json,
            CliExportFormat::Html => ExportFormat::Html,
        }
    }
}

#[derive(Subcommand)]
pub enum AgentAction {
    /// Export an agent conversation transcript to Markdown, JSON, or self-contained HTML.
    Export {
        agent_id: String,
        /// Output format: `md` (default), `json`, or `html`.
        #[arg(short = 'f', long, value_enum, default_value = "md")]
        format: CliExportFormat,
        /// Exclude assistant thinking / reasoning blocks.
        #[arg(long)]
        no_thinking: bool,
        /// Exclude tool call details.
        #[arg(long)]
        no_tools: bool,
        /// Exclude timestamps from the exported transcript.
        #[arg(long)]
        no_timestamps: bool,
        /// Output file path (writes atomically; omit to write to stdout).
        #[arg(short = 'o', long = "output")]
        output: Option<PathBuf>,
    },
    /// Fork/branch an existing agent conversation in the same directory or into a new git worktree.
    Fork {
        agent_id: String,
        /// Optional timeline sequence upper bound (inclusive). Omit to fork full history.
        #[arg(long = "at-seq")]
        at_seq: Option<u64>,
        /// Create a fresh isolated git worktree workspace for the forked branch.
        #[arg(long)]
        worktree: bool,
        /// Optional git branch name when forking into a worktree.
        #[arg(long)]
        branch: Option<String>,
        /// Optional git base ref when forking into a worktree.
        #[arg(long = "base-ref")]
        base_ref: Option<String>,
        /// Optional custom title for the forked conversation.
        #[arg(long)]
        title: Option<String>,
        /// Optional initial follow-up prompt to dispatch immediately on the forked branch.
        #[arg(long)]
        prompt: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum WorkspaceAction {
    /// List workspaces.
    Ls {
        #[arg(short = 'a', long)]
        all: bool,
        #[arg(long)]
        include_background: bool,
    },
    /// Create a local workspace or isolated git worktree.
    Create {
        source_cwd: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        worktree: bool,
        #[arg(long, value_enum, default_value = "branch-off")]
        mode: CliWorktreeMode,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        base_ref: Option<String>,
        #[arg(long)]
        pr: Option<u32>,
    },
    /// Rename a workspace.
    Rename { workspace_id: String, title: String },
    /// Archive a workspace (tears down worktree when last active agent/workspace archives).
    Archive { workspace_id: String },
    /// Unarchive a workspace.
    Unarchive { workspace_id: String },
}

#[derive(Subcommand)]
pub enum ScriptAction {
    /// List configured scripts and services (`orbit.json` / `paseo.json`) for a workspace.
    Ls { workspace_id: String },
    /// Start a configured workspace script or dev service.
    Start { workspace_id: String, name: String },
    /// Stop a running workspace script or dev service.
    Stop { workspace_id: String, name: String },
}

#[derive(Subcommand)]
pub enum TerminalAction {
    /// List active detached PTY terminals.
    Ls,
    /// Create a detached PTY terminal.
    Create {
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(long, default_value_t = 120)]
        cols: u16,
        #[arg(long, default_value_t = 36)]
        rows: u16,
    },
    /// Capture rendered plain text from a detached PTY terminal.
    Capture {
        terminal_id: String,
        #[arg(long)]
        lines: Option<u32>,
    },
    /// Send text or named keys (Enter, Ctrl+C, etc.) to a PTY terminal.
    SendKeys {
        terminal_id: String,
        #[arg(long)]
        text: Option<String>,
        #[arg(long = "key")]
        keys: Vec<String>,
    },
    /// Kill a detached PTY terminal.
    Kill { terminal_id: String },
}

#[derive(Subcommand)]
pub enum ScheduleAction {
    /// List Cron schedules.
    Ls {
        #[arg(short = 'a', long)]
        all: bool,
    },
    /// Create a recurring Cron schedule that spawns a fresh agent run.
    Create {
        name: String,
        #[arg(long)]
        cron: String,
        #[arg(long)]
        cwd: String,
        #[arg(long)]
        prompt: String,
        #[arg(short, long, value_enum, default_value = "claude-code")]
        provider: CliProvider,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(short, long, value_enum, default_value = "accept-edits")]
        mode: CliMode,
        #[arg(long)]
        worktree: bool,
        #[arg(long)]
        no_archive_on_finish: bool,
        #[arg(long)]
        max_runs: Option<u32>,
    },
    /// Inspect a schedule and its recent runs.
    Inspect { schedule_id: String },
    /// Update a schedule.
    Update {
        schedule_id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cron: Option<String>,
        #[arg(long)]
        prompt: Option<String>,
        #[arg(long)]
        max_runs: Option<u32>,
    },
    /// Pause an active schedule.
    Pause { schedule_id: String },
    /// Resume a paused schedule.
    Resume { schedule_id: String },
    /// Trigger an immediate one-off run of a schedule.
    RunOnce { schedule_id: String },
    /// Show recent execution logs for a schedule.
    Logs {
        schedule_id: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
    /// Delete a schedule.
    Delete { schedule_id: String },
}

#[derive(Subcommand)]
pub enum HeartbeatAction {
    /// List active heartbeats attached to long-lived agents.
    Ls {
        #[arg(short = 'a', long)]
        all: bool,
    },
    /// Attach a periodic heartbeat wakeup to an existing agent.
    Create {
        agent_id: String,
        #[arg(long)]
        cron: String,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        max_runs: Option<u32>,
    },
    /// Delete a heartbeat.
    Delete { schedule_id: String },
}

#[derive(Subcommand)]
pub enum ProfileAction {
    /// List Agent Profiles on the daemon.
    Ls,
    /// Create or update an Agent Profile.
    Create {
        name: String,
        #[arg(short, long, value_enum)]
        provider: CliProvider,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum)]
        effort: Option<CliEffort>,
        #[arg(short, long, value_enum, default_value = "accept-edits")]
        mode: CliMode,
        #[arg(long, default_value = "")]
        when_to_use: String,
        #[arg(long)]
        system_prompt: Option<String>,
        #[arg(long = "disable-tool")]
        disabled_tools: Vec<String>,
        #[arg(long)]
        disable_mcp: bool,
    },
    /// Delete an Agent Profile.
    Delete { profile_id: String },
}

#[derive(Subcommand)]
pub enum HostAction {
    /// List Daemon-to-Daemon Connected Hosts registered on this daemon.
    Ls,
    /// Register a remote Orbit Daemon via its `orbit://pair/...` URI for cross-host orchestration.
    Add {
        pairing_uri: String,
        #[arg(long)]
        label: Option<String>,
    },
    /// Enable a connected host registration.
    Enable { registration_id: String },
    /// Disable a connected host registration.
    Disable { registration_id: String },
    /// Remove a connected host registration.
    Remove { registration_id: String },
}

#[derive(Subcommand)]
pub enum PermitAction {
    /// List pending permission requests across agents.
    Ls {
        #[arg(long)]
        agent: Option<String>,
    },
    /// Approve a pending permission request.
    Allow {
        agent_id: String,
        request_id: String,
        #[arg(long, default_value = "allow")]
        decision: String,
    },
    /// Deny a pending permission request.
    Deny {
        agent_id: String,
        request_id: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Show the daemon's current Orchestration configuration.
    Get,
    /// Update the daemon's Orchestration configuration.
    Set {
        #[arg(long)]
        mcp_inject: Option<bool>,
        #[arg(long)]
        idle_cleanup: Option<bool>,
        #[arg(long)]
        idle_timeout: Option<u64>,
        #[arg(long)]
        resume_on_startup: Option<bool>,
        #[arg(long)]
        keep_awake: Option<bool>,
    },
}

#[derive(Subcommand)]
pub enum SkillAction {
    /// List bundled Orbit orchestration skills and their installation state.
    Ls,
    /// Install/sync selected orchestration skills (`orbit`, `orbit-handoff`, `orbit-committee`, `orbit-advisor`).
    Sync {
        #[arg(value_name = "SKILL_ID")]
        skill_ids: Vec<String>,
    },
}

pub fn resolve_server_url(home_dir: &Path, explicit_server: Option<String>) -> String {
    if let Some(s) = explicit_server.filter(|v| !v.trim().is_empty()) {
        return s;
    }
    let port = orbit_daemon::read_live_daemon_port(home_dir);
    format!("ws://127.0.0.1:{port}/ws")
}

fn matches_rpc_response(w: &WireMessage, req_id: Uuid) -> bool {
    match w {
        WireMessage::Ack { request_id }
        | WireMessage::WorkspacesSnapshot { request_id, .. }
        | WireMessage::WorkspaceCreated { request_id, .. }
        | WireMessage::WorkspaceScriptsSnapshot { request_id, .. }
        | WireMessage::AgentsSnapshot { request_id, .. }
        | WireMessage::AgentCreated { request_id, .. }
        | WireMessage::AgentForked { request_id, .. }
        | WireMessage::PendingPermissionsSnapshot { request_id, .. }
        | WireMessage::TimelineCatchup { request_id, .. }
        | WireMessage::TerminalsSnapshot { request_id, .. }
        | WireMessage::TerminalAttached { request_id, .. }
        | WireMessage::TerminalCaptured { request_id, .. }
        | WireMessage::ProfilesSnapshot { request_id, .. }
        | WireMessage::ProfileUpserted { request_id, .. }
        | WireMessage::SchedulesSnapshot { request_id, .. }
        | WireMessage::ScheduleCreated { request_id, .. }
        | WireMessage::ScheduleRunsSnapshot { request_id, .. }
        | WireMessage::ConnectedHostsSnapshot { request_id, .. }
        | WireMessage::ConnectedHostAdded { request_id, .. }
        | WireMessage::OrchestrationConfigSnapshot { request_id, .. }
        | WireMessage::OrchestrationSkillsSnapshot { request_id, .. }
        | WireMessage::KeepAwakeStatusSnapshot { request_id, .. }
        | WireMessage::ProviderSubagentsSnapshot { request_id, .. } => *request_id == req_id,
        WireMessage::RpcError {
            request_id: Some(r),
            ..
        } => *r == req_id,
        _ => false,
    }
}

async fn send_rpc(
    client: &HostSessionHandle,
    req_id: Uuid,
    msg: WireMessage,
    timeout_secs: u64,
) -> anyhow::Result<WireMessage> {
    let resp = client
        .request(msg, Duration::from_secs(timeout_secs), |w| {
            matches_rpc_response(w, req_id)
        })
        .await?;
    if let WireMessage::RpcError { code, message, .. } = resp {
        anyhow::bail!("[{code}] {message}");
    }
    Ok(resp)
}

fn print_out(json_mode: bool, value: &serde_json::Value, human: &str) {
    if json_mode {
        if let Ok(s) = serde_json::to_string_pretty(value) {
            println!("{s}");
        }
    } else {
        println!("{human}");
    }
}

pub async fn cmd_wait(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    agent_id: &str,
    timeout_secs: u64,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    let mut rx = client.subscribe();

    // Check current status first
    let req_id = Uuid::new_v4();
    let resp = send_rpc(
        &client,
        req_id,
        WireMessage::ListAgents {
            request_id: req_id,
            workspace_id: None,
            include_archived: true,
            include_background: true,
            parent_agent_id: None,
        },
        15,
    )
    .await?;
    let mut current = if let WireMessage::AgentsSnapshot { agents, .. } = resp {
        agents
            .into_iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?
    } else {
        anyhow::bail!("unexpected response")
    };

    let is_terminal = |s: &AgentStatus| {
        matches!(
            s,
            AgentStatus::Error { .. }
                | AgentStatus::Closed
                | AgentStatus::AwaitingPermission { .. }
        )
    };

    let fetch_timeline = |c: orbit_client_core::HostSessionHandle, aid: String| async move {
        let tl_req = Uuid::new_v4();
        match send_rpc(
            &c,
            tl_req,
            WireMessage::SubscribeTimeline {
                request_id: tl_req,
                agent_id: aid,
                since_seq: 0,
            },
            15,
        )
        .await
        {
            Ok(WireMessage::TimelineCatchup { events, .. }) => events,
            _ => Vec::new(),
        }
    };

    let has_pending_prompt = |events: &[orbit_protocol::TimelineEvent]| -> bool {
        let last_prompt_seq = events.iter().rev().find_map(|e| match &e.payload {
            TimelinePayload::UserPrompt { .. } | TimelinePayload::AgentMessage { .. } => {
                Some(e.seq)
            }
            _ => None,
        });
        let last_reply_seq = events.iter().rev().find_map(|e| match &e.payload {
            TimelinePayload::AssistantMessage {
                streaming: false, ..
            } => Some(e.seq),
            _ => None,
        });
        match (last_prompt_seq, last_reply_seq) {
            (Some(p), Some(r)) => p > r,
            (Some(_), None) => true,
            _ => false,
        }
    };

    let mut settled = if is_terminal(&current.status) {
        true
    } else if matches!(current.status, AgentStatus::Idle) {
        let events = fetch_timeline(client.clone(), agent_id.to_string()).await;
        !has_pending_prompt(&events)
    } else {
        false
    };

    if !settled {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
        let mut saw_running = matches!(
            current.status,
            AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. }
        );
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                anyhow::bail!("timed out waiting for agent `{agent_id}` after {timeout_secs}s");
            }
            let wait_res = tokio::time::timeout(deadline - now, rx.recv()).await;
            match wait_res {
                Ok(Ok(ClientInboundEvent::Wire(WireMessage::AgentUpdated { agent })))
                    if agent.agent_id == agent_id =>
                {
                    current = agent;
                    if matches!(
                        current.status,
                        AgentStatus::Running { .. } | AgentStatus::AutonomousWakeup { .. }
                    ) {
                        saw_running = true;
                    } else if is_terminal(&current.status) {
                        settled = true;
                        break;
                    } else if matches!(current.status, AgentStatus::Idle) {
                        if saw_running {
                            settled = true;
                            break;
                        }
                        let events = fetch_timeline(client.clone(), agent_id.to_string()).await;
                        if !has_pending_prompt(&events) {
                            settled = true;
                            break;
                        }
                    }
                }
                Ok(Ok(ClientInboundEvent::Disconnected)) => {
                    anyhow::bail!("disconnected from Orbit Daemon while waiting");
                }
                Err(_) => {
                    anyhow::bail!("timed out waiting for agent `{agent_id}` after {timeout_secs}s");
                }
                _ => {}
            }
        }
    }
    let _ = settled;

    // Grab last assistant message from timeline
    let events = fetch_timeline(client.clone(), agent_id.to_string()).await;
    let last_message = events.into_iter().rev().find_map(|e| {
        if let TimelinePayload::AssistantMessage { text, .. } = e.payload {
            Some(text)
        } else {
            None
        }
    });

    let val = json!({
        "agent": current,
        "last_message": last_message,
    });
    let human = format!(
        "{} | status: {} | title: {}\n{}",
        current.agent_id,
        format_status(&current.status),
        current.title,
        last_message.as_deref().unwrap_or("")
    );
    print_out(json_mode, &val, &human);
    Ok(())
}

pub async fn cmd_inspect(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    agent_id: &str,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    let req_id = Uuid::new_v4();
    let resp = send_rpc(
        &client,
        req_id,
        WireMessage::ListAgents {
            request_id: req_id,
            workspace_id: None,
            include_archived: true,
            include_background: true,
            parent_agent_id: None,
        },
        15,
    )
    .await?;
    let agent = if let WireMessage::AgentsSnapshot { agents, .. } = resp {
        agents
            .into_iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?
    } else {
        anyhow::bail!("unexpected response")
    };

    let p_req = Uuid::new_v4();
    let perms = match send_rpc(
        &client,
        p_req,
        WireMessage::ListPendingPermissions {
            request_id: p_req,
            agent_id: Some(agent_id.to_string()),
        },
        10,
    )
    .await
    {
        Ok(WireMessage::PendingPermissionsSnapshot { permissions, .. }) => permissions,
        _ => Vec::new(),
    };

    let s_req = Uuid::new_v4();
    let subagents = match send_rpc(
        &client,
        s_req,
        WireMessage::ListProviderSubagents {
            request_id: s_req,
            parent_agent_id: agent_id.to_string(),
        },
        10,
    )
    .await
    {
        Ok(WireMessage::ProviderSubagentsSnapshot { subagents, .. }) => subagents,
        _ => Vec::new(),
    };

    let tl_req = Uuid::new_v4();
    let events = match send_rpc(
        &client,
        tl_req,
        WireMessage::SubscribeTimeline {
            request_id: tl_req,
            agent_id: agent_id.to_string(),
            since_seq: 0,
        },
        15,
    )
    .await
    {
        Ok(WireMessage::TimelineCatchup { events, .. }) => events,
        _ => Vec::new(),
    };

    if json_mode {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "agent": agent,
                "pending_permissions": perms,
                "provider_subagents": subagents,
                "recent_events": events.iter().rev().take(20).rev().collect::<Vec<_>>(),
            }))?
        );
    } else {
        println!("Agent ID   : {}", agent.agent_id);
        println!("Title      : {}", agent.title);
        println!(
            "Provider   : {} (model: {:?})",
            agent.provider.display_name(),
            agent.model
        );
        println!("Status     : {}", format_status(&agent.status));
        println!("Workspace  : {} ({})", agent.workspace_id, agent.cwd);
        println!(
            "Parent     : {:?} (host: {:?})",
            agent.parent_agent_id, agent.parent_host_id
        );
        println!("Profile    : {:?}", agent.profile_id);
        println!("Archived   : {:?}", agent.archived_at);
        println!("Attention  : {:?}", agent.attention);
        if !perms.is_empty() {
            println!("Pending Permissions: {}", perms.len());
            for p in &perms {
                println!("  - [{}] {}", p.request_id, p.title);
            }
        }
        if !subagents.is_empty() {
            println!("Provider Subagents: {}", subagents.len());
            for s in &subagents {
                println!("  - [{}] {} ({:?})", s.subagent_id, s.title, s.status);
            }
        }
        if !events.is_empty() {
            println!("\nRecent Timeline:");
            for ev in events.iter().rev().take(10).rev() {
                print_timeline_payload(ev.seq, &ev.payload);
            }
        }
    }
    Ok(())
}

pub async fn cmd_simple_agent_op(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    msg: WireMessage,
    req_id: Uuid,
    label: &str,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    let _ = send_rpc(&client, req_id, msg, 20).await?;
    print_out(
        json_mode,
        &json!({ "ok": true, "action": label }),
        &format!("[✓] {label}"),
    );
    Ok(())
}

pub async fn cmd_workspace(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: WorkspaceAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        WorkspaceAction::Ls {
            all,
            include_background,
        } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListWorkspaces {
                    request_id: req_id,
                    include_archived: all,
                    include_background,
                },
                15,
            )
            .await?;
            if let WireMessage::WorkspacesSnapshot { workspaces, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&workspaces)?);
                } else if workspaces.is_empty() {
                    println!("No workspaces found.");
                } else {
                    for w in workspaces {
                        println!(
                            "{:<36} | {:<8} | {:<18} | {}",
                            w.workspace_id,
                            if w.is_worktree { "worktree" } else { "local" },
                            w.git_branch.as_deref().unwrap_or("-"),
                            w.cwd
                        );
                    }
                }
            }
        }
        WorkspaceAction::Create {
            source_cwd,
            title,
            worktree,
            mode,
            branch,
            base_ref,
            pr,
        } => {
            let req_id = Uuid::new_v4();
            let isolation = if worktree || branch.is_some() || pr.is_some() {
                WorkspaceIsolation::Worktree
            } else {
                WorkspaceIsolation::Local
            };
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::CreateWorkspace {
                    request_id: req_id,
                    source_cwd,
                    title,
                    worktree_branch: branch,
                    base_ref,
                    isolation,
                    worktree_mode: mode.into(),
                    pr_number: pr,
                    parent_workspace_id: None,
                    background: false,
                    idempotency_key: None,
                    is_curated: Some(true),
                    mission_goal: None,
                    project_root: None,
                },
                60,
            )
            .await?;
            if let WireMessage::WorkspaceCreated { workspace, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&workspace)?,
                    &format!(
                        "[✓] Workspace `{}` -> {} (branch: {})",
                        workspace.workspace_id,
                        workspace.cwd,
                        workspace.git_branch.as_deref().unwrap_or("-")
                    ),
                );
            }
        }
        WorkspaceAction::Rename {
            workspace_id,
            title,
        } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::RenameWorkspace {
                    request_id: req_id,
                    workspace_id: workspace_id.clone(),
                    title: title.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "workspace_id": workspace_id, "title": title }),
                &format!("[✓] Renamed `{workspace_id}` to `{title}`"),
            );
        }
        WorkspaceAction::Archive { workspace_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::ArchiveWorkspace {
                    request_id: req_id,
                    workspace_id: workspace_id.clone(),
                    keep_agents_unclassified: false,
                },
                60,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "workspace_id": workspace_id }),
                &format!("[✓] Archived workspace `{workspace_id}`"),
            );
        }
        WorkspaceAction::Unarchive { workspace_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::UnarchiveWorkspace {
                    request_id: req_id,
                    workspace_id: workspace_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "workspace_id": workspace_id }),
                &format!("[✓] Unarchived workspace `{workspace_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_script(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: ScriptAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        ScriptAction::Ls { workspace_id } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListWorkspaceScripts {
                    request_id: req_id,
                    workspace_id,
                },
                15,
            )
            .await?;
            if let WireMessage::WorkspaceScriptsSnapshot { scripts, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&scripts)?);
                } else if scripts.is_empty() {
                    println!("No scripts configured in orbit.json / paseo.json.");
                } else {
                    for s in scripts {
                        println!(
                            "{:<20} | {:<8} | {:<8} | {} {}",
                            s.name,
                            format!("{:?}", s.kind),
                            format!("{:?}", s.status),
                            s.command,
                            s.proxy_url.as_deref().unwrap_or("")
                        );
                    }
                }
            }
        }
        ScriptAction::Start { workspace_id, name } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::StartWorkspaceScript {
                    request_id: req_id,
                    workspace_id: workspace_id.clone(),
                    script_name: name.clone(),
                },
                20,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "workspace_id": workspace_id, "script": name }),
                &format!("[✓] Started script `{name}`"),
            );
        }
        ScriptAction::Stop { workspace_id, name } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::StopWorkspaceScript {
                    request_id: req_id,
                    workspace_id: workspace_id.clone(),
                    script_name: name.clone(),
                },
                20,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "workspace_id": workspace_id, "script": name }),
                &format!("[✓] Stopped script `{name}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_terminal(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: TerminalAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        TerminalAction::Ls => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListTerminals { request_id: req_id },
                15,
            )
            .await?;
            if let WireMessage::TerminalsSnapshot { terminals, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&terminals)?);
                } else if terminals.is_empty() {
                    println!("No active terminals.");
                } else {
                    for t in terminals {
                        println!("{:<36} | {:<18} | {}", t.terminal_id, t.title, t.cwd);
                    }
                }
            }
        }
        TerminalAction::Create {
            cwd,
            title,
            cols,
            rows,
        } => {
            let target_cwd = cwd.unwrap_or_else(|| {
                std::env::current_dir()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| ".".into())
            });
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::CreateTerminal {
                    request_id: req_id,
                    cwd: target_cwd,
                    title,
                    cols,
                    rows,
                },
                15,
            )
            .await?;
            if let WireMessage::TerminalAttached { terminal, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&terminal)?,
                    &format!(
                        "[✓] Created terminal `{}` in {}",
                        terminal.terminal_id, terminal.cwd
                    ),
                );
            }
        }
        TerminalAction::Capture { terminal_id, lines } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::CaptureTerminal {
                    request_id: req_id,
                    terminal_id,
                    lines,
                },
                15,
            )
            .await?;
            if let WireMessage::TerminalCaptured { text, .. } = resp {
                print_out(json_mode, &json!({ "text": text }), &text);
            }
        }
        TerminalAction::SendKeys {
            terminal_id,
            text,
            keys,
        } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::SendTerminalKeys {
                    request_id: req_id,
                    terminal_id: terminal_id.clone(),
                    text,
                    keys,
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "terminal_id": terminal_id }),
                &format!("[✓] Sent keys to `{terminal_id}`"),
            );
        }
        TerminalAction::Kill { terminal_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::KillTerminal {
                    request_id: req_id,
                    terminal_id: terminal_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "terminal_id": terminal_id }),
                &format!("[✓] Killed terminal `{terminal_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_schedule(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: ScheduleAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        ScheduleAction::Ls { all } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListSchedules {
                    request_id: req_id,
                    include_completed: all,
                },
                15,
            )
            .await?;
            if let WireMessage::SchedulesSnapshot { schedules, .. } = resp {
                let items: Vec<_> = schedules
                    .into_iter()
                    .filter(|s| s.kind == ScheduleKind::Schedule)
                    .collect();
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    println!("No schedules configured.");
                } else {
                    for s in items {
                        println!(
                            "{:<14} | {:<10} | {:<16} | runs:{} | {}",
                            s.schedule_id,
                            format!("{:?}", s.status),
                            s.cron_expr,
                            s.run_count,
                            s.name
                        );
                    }
                }
            }
        }
        ScheduleAction::Create {
            name,
            cron,
            cwd,
            prompt,
            provider,
            model,
            profile,
            mode,
            worktree,
            no_archive_on_finish,
            max_runs,
        } => {
            let req_id = Uuid::new_v4();
            let target = ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
                cwd,
                workspace_id: None,
                provider: provider.into(),
                model,
                effort: None,
                profile_id: profile,
                mode: mode.into(),
                worktree,
                base_ref: None,
                background: false,
                archive_on_finish: !no_archive_on_finish,
            });
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::CreateSchedule {
                    request_id: req_id,
                    name,
                    cron_expr: cron,
                    timezone: None,
                    prompt,
                    target,
                    max_runs,
                    expires_at: None,
                    created_by_agent: std::env::var("ORBIT_AGENT_ID").ok(),
                },
                15,
            )
            .await?;
            if let WireMessage::ScheduleCreated { schedule, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&schedule)?,
                    &format!(
                        "[✓] Created schedule `{}` ({}) -> next run {:?}",
                        schedule.schedule_id, schedule.cron_expr, schedule.next_run_at
                    ),
                );
            }
        }
        ScheduleAction::Inspect { schedule_id } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListSchedules {
                    request_id: req_id,
                    include_completed: true,
                },
                15,
            )
            .await?;
            let sched = if let WireMessage::SchedulesSnapshot { schedules, .. } = resp {
                schedules
                    .into_iter()
                    .find(|s| s.schedule_id == schedule_id)
                    .ok_or_else(|| anyhow::anyhow!("schedule `{schedule_id}` not found"))?
            } else {
                anyhow::bail!("unexpected response")
            };
            let r_req = Uuid::new_v4();
            let runs = match send_rpc(
                &client,
                r_req,
                WireMessage::ListScheduleRuns {
                    request_id: r_req,
                    schedule_id: schedule_id.clone(),
                    limit: Some(10),
                },
                15,
            )
            .await
            {
                Ok(WireMessage::ScheduleRunsSnapshot { runs, .. }) => runs,
                _ => Vec::new(),
            };
            print_out(
                json_mode,
                &json!({ "schedule": sched, "runs": runs }),
                &format!(
                    "{} ({}) | {:?} | cron: {} | runs: {}",
                    sched.schedule_id, sched.name, sched.status, sched.cron_expr, sched.run_count
                ),
            );
        }
        ScheduleAction::Update {
            schedule_id,
            name,
            cron,
            prompt,
            max_runs,
        } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::UpdateSchedule {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                    name,
                    cron_expr: cron,
                    prompt,
                    max_runs,
                    expires_at: None,
                    archive_on_finish: None,
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Updated schedule `{schedule_id}`"),
            );
        }
        ScheduleAction::Pause { schedule_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::PauseSchedule {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Paused schedule `{schedule_id}`"),
            );
        }
        ScheduleAction::Resume { schedule_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::ResumeSchedule {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Resumed schedule `{schedule_id}`"),
            );
        }
        ScheduleAction::RunOnce { schedule_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::RunScheduleOnce {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                },
                60,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Triggered one-off run for `{schedule_id}`"),
            );
        }
        ScheduleAction::Logs { schedule_id, limit } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListScheduleRuns {
                    request_id: req_id,
                    schedule_id,
                    limit: Some(limit),
                },
                15,
            )
            .await?;
            if let WireMessage::ScheduleRunsSnapshot { runs, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&runs)?);
                } else if runs.is_empty() {
                    println!("No runs recorded yet.");
                } else {
                    for r in runs {
                        println!(
                            "{} | {:?} | agent:{:?} | {}",
                            r.started_at.to_rfc3339(),
                            r.status,
                            r.agent_id,
                            r.summary.or(r.error).unwrap_or_default()
                        );
                    }
                }
            }
        }
        ScheduleAction::Delete { schedule_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::DeleteSchedule {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Deleted schedule `{schedule_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_heartbeat(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: HeartbeatAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        HeartbeatAction::Ls { all } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListSchedules {
                    request_id: req_id,
                    include_completed: all,
                },
                15,
            )
            .await?;
            if let WireMessage::SchedulesSnapshot { schedules, .. } = resp {
                let items: Vec<_> = schedules
                    .into_iter()
                    .filter(|s| s.kind == ScheduleKind::Heartbeat)
                    .collect();
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    println!("No active heartbeats.");
                } else {
                    for h in items {
                        println!(
                            "{:<14} | {:<10} | {:<16} | {}",
                            h.schedule_id,
                            format!("{:?}", h.status),
                            h.cron_expr,
                            h.name
                        );
                    }
                }
            }
        }
        HeartbeatAction::Create {
            agent_id,
            cron,
            prompt,
            name,
            max_runs,
        } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::CreateSchedule {
                    request_id: req_id,
                    name: name.unwrap_or_else(|| format!("Heartbeat ({agent_id})")),
                    cron_expr: cron,
                    timezone: None,
                    prompt,
                    target: ScheduleTarget::ExistingAgent { agent_id },
                    max_runs,
                    expires_at: None,
                    created_by_agent: std::env::var("ORBIT_AGENT_ID").ok(),
                },
                15,
            )
            .await?;
            if let WireMessage::ScheduleCreated { schedule, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&schedule)?,
                    &format!(
                        "[✓] Created heartbeat `{}` ({})",
                        schedule.schedule_id, schedule.cron_expr
                    ),
                );
            }
        }
        HeartbeatAction::Delete { schedule_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::DeleteSchedule {
                    request_id: req_id,
                    schedule_id: schedule_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "schedule_id": schedule_id }),
                &format!("[✓] Deleted heartbeat `{schedule_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_profile(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: ProfileAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        ProfileAction::Ls => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListProfiles { request_id: req_id },
                15,
            )
            .await?;
            if let WireMessage::ProfilesSnapshot { profiles, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&profiles)?);
                } else if profiles.is_empty() {
                    println!("No agent profiles configured.");
                } else {
                    for p in profiles {
                        println!(
                            "{:<16} | {:<18} | {:<12} | model:{:?} | {}",
                            p.profile_id,
                            p.name,
                            p.provider.display_name(),
                            p.model,
                            p.when_to_use
                        );
                    }
                }
            }
        }
        ProfileAction::Create {
            name,
            provider,
            id,
            model,
            effort,
            mode,
            when_to_use,
            system_prompt,
            disabled_tools,
            disable_mcp,
        } => {
            let now = chrono::Utc::now();
            let profile = AgentProfile {
                profile_id: id.unwrap_or_else(|| {
                    format!("prof_{}", &Uuid::new_v4().simple().to_string()[..8])
                }),
                name,
                provider: ProviderKind::from(provider),
                model,
                effort: effort.map(Into::into),
                mode: SessionMode::from(mode),
                when_to_use,
                system_prompt_append: system_prompt,
                mcp_policy: McpToolPolicy {
                    enabled: !disable_mcp,
                    disabled_tools,
                },
                created_at: now,
                updated_at: now,
            };
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::UpsertProfile {
                    request_id: req_id,
                    profile,
                },
                15,
            )
            .await?;
            if let WireMessage::ProfileUpserted { profile, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&profile)?,
                    &format!(
                        "[✓] Saved profile `{}` ({})",
                        profile.profile_id, profile.name
                    ),
                );
            }
        }
        ProfileAction::Delete { profile_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::DeleteProfile {
                    request_id: req_id,
                    profile_id: profile_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "profile_id": profile_id }),
                &format!("[✓] Deleted profile `{profile_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_host(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: HostAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        HostAction::Ls => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListConnectedHosts { request_id: req_id },
                15,
            )
            .await?;
            if let WireMessage::ConnectedHostsSnapshot { hosts, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&hosts)?);
                } else if hosts.is_empty() {
                    println!("No connected hosts registered.");
                } else {
                    for h in hosts {
                        println!(
                            "{:<20} | {:<16} | {:<12} | enabled:{} | host_id:{:?}",
                            h.registration_id,
                            h.label,
                            format!("{:?}", h.status),
                            h.enabled,
                            h.host_id
                        );
                    }
                }
            }
        }
        HostAction::Add { pairing_uri, label } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::AddConnectedHost {
                    request_id: req_id,
                    label,
                    pairing_uri,
                },
                15,
            )
            .await?;
            if let WireMessage::ConnectedHostAdded { host, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&host)?,
                    &format!(
                        "[✓] Registered connected host `{}` ({})",
                        host.registration_id, host.label
                    ),
                );
            }
        }
        HostAction::Enable { registration_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::UpdateConnectedHost {
                    request_id: req_id,
                    registration_id: registration_id.clone(),
                    label: None,
                    enabled: Some(true),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "registration_id": registration_id, "enabled": true }),
                &format!("[✓] Enabled connected host `{registration_id}`"),
            );
        }
        HostAction::Disable { registration_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::UpdateConnectedHost {
                    request_id: req_id,
                    registration_id: registration_id.clone(),
                    label: None,
                    enabled: Some(false),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "registration_id": registration_id, "enabled": false }),
                &format!("[✓] Disabled connected host `{registration_id}`"),
            );
        }
        HostAction::Remove { registration_id } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::RemoveConnectedHost {
                    request_id: req_id,
                    registration_id: registration_id.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "registration_id": registration_id }),
                &format!("[✓] Removed connected host `{registration_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_permit(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: PermitAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        PermitAction::Ls { agent } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListPendingPermissions {
                    request_id: req_id,
                    agent_id: agent,
                },
                15,
            )
            .await?;
            if let WireMessage::PendingPermissionsSnapshot { permissions, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&permissions)?);
                } else if permissions.is_empty() {
                    println!("No pending permissions.");
                } else {
                    for p in permissions {
                        println!("{} | {} | {}", p.agent_id, p.request_id, p.title);
                    }
                }
            }
        }
        PermitAction::Allow {
            agent_id,
            request_id: perm_id,
            decision,
        } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::ResolvePermission {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                    permission_request_id: perm_id.clone(),
                    decision: decision.clone(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "agent_id": agent_id, "permission_request_id": perm_id, "decision": decision }),
                &format!("[✓] Resolved permission `{perm_id}` -> {decision}"),
            );
        }
        PermitAction::Deny {
            agent_id,
            request_id: perm_id,
        } => {
            let req_id = Uuid::new_v4();
            send_rpc(
                &client,
                req_id,
                WireMessage::ResolvePermission {
                    request_id: req_id,
                    agent_id: agent_id.clone(),
                    permission_request_id: perm_id.clone(),
                    decision: "deny".into(),
                },
                15,
            )
            .await?;
            print_out(
                json_mode,
                &json!({ "ok": true, "agent_id": agent_id, "permission_request_id": perm_id, "decision": "deny" }),
                &format!("[✓] Denied permission `{perm_id}`"),
            );
        }
    }
    Ok(())
}

pub async fn cmd_config(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: ConfigAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        ConfigAction::Get => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::GetOrchestrationConfig { request_id: req_id },
                15,
            )
            .await?;
            if let WireMessage::OrchestrationConfigSnapshot { config, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&config)?,
                    &serde_json::to_string_pretty(&config)?,
                );
            }
        }
        ConfigAction::Set {
            mcp_inject,
            idle_cleanup,
            idle_timeout,
            resume_on_startup,
            keep_awake,
        } => {
            let get_id = Uuid::new_v4();
            let cur = match send_rpc(
                &client,
                get_id,
                WireMessage::GetOrchestrationConfig { request_id: get_id },
                15,
            )
            .await?
            {
                WireMessage::OrchestrationConfigSnapshot { config, .. } => config,
                _ => anyhow::bail!("unexpected response"),
            };
            let mut next = cur;
            if let Some(v) = mcp_inject {
                next.mcp_inject_enabled = v;
            }
            if let Some(v) = idle_cleanup {
                next.idle_cleanup_enabled = v;
            }
            if let Some(v) = idle_timeout {
                next.idle_timeout_secs = v;
            }
            if let Some(v) = resume_on_startup {
                next.resume_on_startup = v;
            }
            if let Some(v) = keep_awake {
                next.keep_awake_while_running = v;
            }
            let set_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                set_id,
                WireMessage::UpdateOrchestrationConfig {
                    request_id: set_id,
                    config: next,
                },
                15,
            )
            .await?;
            if let WireMessage::OrchestrationConfigSnapshot { config, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&config)?,
                    &format!(
                        "[✓] Updated orchestration config:\n{}",
                        serde_json::to_string_pretty(&config)?
                    ),
                );
            }
        }
    }
    Ok(())
}

pub async fn cmd_agent(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: AgentAction,
) -> anyhow::Result<()> {
    match action {
        AgentAction::Export {
            agent_id,
            format,
            no_thinking,
            no_tools,
            no_timestamps,
            output,
        } => {
            cmd_export_agent(
                server,
                via_relay,
                agent_id,
                format,
                no_thinking,
                no_tools,
                no_timestamps,
                output,
            )
            .await
        }
        AgentAction::Fork {
            agent_id,
            at_seq,
            worktree,
            branch,
            base_ref,
            title,
            prompt,
        } => {
            cmd_fork_agent(
                server, via_relay, json_mode, agent_id, at_seq, worktree, branch, base_ref, title,
                prompt,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_fork_agent(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    source_agent_id: String,
    at_seq: Option<u64>,
    worktree: bool,
    branch: Option<String>,
    base_ref: Option<String>,
    title: Option<String>,
    initial_prompt: Option<String>,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    let req_id = Uuid::new_v4();
    let target = if worktree || branch.is_some() || base_ref.is_some() {
        ForkTarget::NewWorktree { branch, base_ref }
    } else {
        ForkTarget::SameDirectory
    };
    let resp = send_rpc(
        &client,
        req_id,
        WireMessage::ForkAgent {
            request_id: req_id,
            source_agent_id,
            up_to_seq: at_seq,
            target,
            title,
            initial_prompt,
        },
        60,
    )
    .await?;
    if let WireMessage::AgentForked {
        agent, workspace, ..
    } = resp
    {
        let payload = json!({
            "agent": agent,
            "workspace": workspace,
        });
        let summary = if let Some(ref ws) = workspace
            && ws.is_worktree
        {
            format!(
                "[✓] Forked `{}` -> `{}` in worktree `{}` (branch: {})",
                agent.forked_from_agent_id.as_deref().unwrap_or("source"),
                agent.agent_id,
                ws.cwd,
                ws.git_branch.as_deref().unwrap_or("detached")
            )
        } else {
            format!(
                "[✓] Forked `{}` -> `{}` ({})",
                agent.forked_from_agent_id.as_deref().unwrap_or("source"),
                agent.agent_id,
                agent.title
            )
        };
        print_out(json_mode, &payload, &summary);
    } else {
        anyhow::bail!("unexpected response to fork_agent");
    }
    Ok(())
}

pub async fn cmd_skill(
    server: &str,
    via_relay: bool,
    json_mode: bool,
    action: SkillAction,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    match action {
        SkillAction::Ls => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::ListOrchestrationSkills { request_id: req_id },
                15,
            )
            .await?;
            if let WireMessage::OrchestrationSkillsSnapshot { skills, .. } = resp {
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&skills)?);
                } else {
                    for s in skills {
                        println!(
                            "{:<16} | {:<16} | installed:{:<5} | outdated:{:<5} | {}",
                            s.id, s.slash_command, s.installed, s.outdated, s.description
                        );
                    }
                }
            }
        }
        SkillAction::Sync { skill_ids } => {
            let req_id = Uuid::new_v4();
            let resp = send_rpc(
                &client,
                req_id,
                WireMessage::SyncOrchestrationSkills {
                    request_id: req_id,
                    skill_ids,
                },
                15,
            )
            .await?;
            if let WireMessage::OrchestrationSkillsSnapshot { skills, .. } = resp {
                print_out(
                    json_mode,
                    &serde_json::to_value(&skills)?,
                    &format!(
                        "[✓] Synced {} installed orchestration skill(s)",
                        skills.iter().filter(|s| s.installed).count()
                    ),
                );
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn cmd_export_agent(
    server: &str,
    via_relay: bool,
    agent_id: String,
    format: CliExportFormat,
    no_thinking: bool,
    no_tools: bool,
    no_timestamps: bool,
    output: Option<PathBuf>,
) -> anyhow::Result<()> {
    let client = connect_cli_server(server, via_relay).await?;
    let agents_req = Uuid::new_v4();
    let agents_resp = send_rpc(
        &client,
        agents_req,
        WireMessage::ListAgents {
            request_id: agents_req,
            workspace_id: None,
            include_archived: true,
            include_background: true,
            parent_agent_id: None,
        },
        15,
    )
    .await?;
    let agent = if let WireMessage::AgentsSnapshot { agents, .. } = agents_resp {
        agents
            .into_iter()
            .find(|a| a.agent_id == agent_id)
            .ok_or_else(|| anyhow::anyhow!("agent `{agent_id}` not found"))?
    } else {
        anyhow::bail!("unexpected response when fetching agent metadata");
    };

    let ws_title = {
        let ws_req = Uuid::new_v4();
        match send_rpc(
            &client,
            ws_req,
            WireMessage::ListWorkspaces {
                request_id: ws_req,
                include_archived: true,
                include_background: true,
            },
            10,
        )
        .await
        {
            Ok(WireMessage::WorkspacesSnapshot { workspaces, .. }) => workspaces
                .into_iter()
                .find(|w| w.workspace_id == agent.workspace_id)
                .map(|w| w.title),
            _ => None,
        }
    };

    let mut all_events = Vec::new();
    let mut since_seq = 0u64;
    loop {
        let tl_req = Uuid::new_v4();
        let resp = send_rpc(
            &client,
            tl_req,
            WireMessage::SubscribeTimeline {
                request_id: tl_req,
                agent_id: agent_id.clone(),
                since_seq,
            },
            20,
        )
        .await?;
        match resp {
            WireMessage::TimelineCatchup {
                events, has_more, ..
            } => {
                let last_seq = events.last().map(|e| e.seq).unwrap_or(since_seq);
                all_events.extend(events);
                if !has_more || last_seq <= since_seq {
                    break;
                }
                since_seq = last_seq;
            }
            _ => anyhow::bail!("unexpected response when fetching timeline"),
        }
    }

    let meta = ExportAgentMeta {
        host_id: client
            .welcome_host
            .as_ref()
            .map(|h| h.host_id.clone())
            .unwrap_or_default(),
        host_name: client.welcome_host.as_ref().map(|h| h.hostname.clone()),
        agent_id: agent.agent_id.clone(),
        title: agent.title.clone(),
        provider: Some(agent.provider),
        model: agent.model.clone(),
        cwd: Some(agent.cwd.clone()),
        workspace_title: ws_title,
        task_summary: agent.task_summary.clone(),
        created_at: Some(agent.created_at),
        exported_at: None,
    };
    let opts = ExportOptions {
        format: format.into(),
        include_thinking: !no_thinking,
        include_tool_calls: !no_tools,
        include_timestamps: !no_timestamps,
    };
    let exported = export_timeline_transcript(&meta, &all_events, &opts);

    if let Some(path) = output {
        write_file_atomically(&path, exported.content.as_bytes()).map_err(anyhow::Error::msg)?;
        eprintln!(
            "[✓] Exported {} ({} turns, {} bytes) -> {}",
            exported.suggested_filename,
            exported.turn_count,
            exported.byte_len,
            path.display()
        );
    } else {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(exported.content.as_bytes())?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod fork_rpc_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn matches_rpc_response_accepts_agent_forked() {
        let req_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let msg = WireMessage::AgentForked {
            request_id: req_id,
            agent: orbit_protocol::AgentSummary {
                agent_id: "ag_fork".into(),
                workspace_id: "ws_1".into(),
                cwd: "/tmp/repo".into(),
                title: "Forked".into(),
                provider: orbit_protocol::ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                mode: orbit_protocol::SessionMode::Default,
                status: orbit_protocol::AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 1,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: orbit_protocol::AttentionState::None,
                last_activity_at: Some(now),
                queued_prompts: Vec::new(),
                read_seq: 1,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: Some("ag_src".into()),
                forked_at_seq: Some(1),
            },
            workspace: None,
        };
        assert!(matches_rpc_response(&msg, req_id));
        assert!(!matches_rpc_response(&msg, other_id));
    }
}
