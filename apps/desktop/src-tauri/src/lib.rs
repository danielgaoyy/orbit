mod updater;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::updater::{
    AppUpdateProgress, CURRENT_APP_VERSION, ReleaseUpdateInfo, check_latest_release,
    perform_one_click_update,
};

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::{
    AgentTimelineDeltaView, AgentTimelineView, ClientProjectView, CoreEventSubscriber,
    CoreNotification, CreateAgentRequest, FleetOverviewView, GitReviewView, HostCardView,
    HostDetailView, OrbitAppCore,
};
use orbit_client_core::desktop_ops::{
    ExternalEditorInfo, ExternalEditorKind, SshHostItem, StagedAttachmentInfo,
    detect_installed_editors, discover_user_ssh_hosts, list_local_workspace_files,
    open_in_external_editor, pick_local_directory, provision_relay_over_ssh, resolve_user_home,
    stage_local_attachment, upgrade_remote_daemon_over_ssh,
};
use orbit_protocol::{
    ActiveTurnBehavior, AgentProfile, OrchestrationConfig, PromptBadge, ProviderKind,
    ProviderSubagentEvent, ReasoningEffort, ScheduleRunRecord, ScheduleTarget, SessionMode,
    WorkspaceIsolation, WorktreeCreateMode,
};
use tauri::{State, ipc::Channel};

struct DesktopAppState {
    core: Arc<OrbitAppCore>,
}

struct TauriChannelSubscriber {
    channel: Channel<CoreNotification>,
}

impl CoreEventSubscriber for TauriChannelSubscriber {
    fn on_notification(&self, event: CoreNotification) {
        let _ = self.channel.send(event);
    }
}

fn default_desktop_db_path() -> PathBuf {
    if let Some(home) = resolve_user_home() {
        let dir = home.join(".orbit");
        let _ = fs::create_dir_all(&dir);
        return dir.join("desktop-client.sqlite");
    }
    PathBuf::from("orbit-desktop-client.sqlite")
}

#[tauri::command]
fn subscribe_core_events(state: State<'_, DesktopAppState>, on_event: Channel<CoreNotification>) {
    state
        .core
        .subscribe(Arc::new(TauriChannelSubscriber { channel: on_event }));
}

#[tauri::command]
fn get_fleet_overview(state: State<'_, DesktopAppState>) -> FleetOverviewView {
    state.core.fleet_overview()
}

#[tauri::command]
fn get_host_detail(state: State<'_, DesktopAppState>, host_id: String) -> Option<HostDetailView> {
    state.core.host_detail(&host_id)
}

#[tauri::command]
fn get_agent_timeline(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> AgentTimelineView {
    state.core.agent_timeline(&host_id, &agent_id)
}

#[tauri::command]
fn get_agent_timeline_delta(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    since_seq: u64,
) -> AgentTimelineDeltaView {
    state
        .core
        .agent_timeline_delta(&host_id, &agent_id, since_seq)
}

#[tauri::command]
fn get_git_review(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: Option<String>,
) -> GitReviewView {
    state.core.git_review(&host_id, &cwd, path.as_deref())
}

#[tauri::command]
fn pair_host(
    state: State<'_, DesktopAppState>,
    pairing_uri: String,
) -> Result<HostCardView, String> {
    state
        .core
        .pair_host(&pairing_uri)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pair_ssh_host(
    state: State<'_, DesktopAppState>,
    ssh_alias: String,
    remote_port: Option<u16>,
) -> Result<HostCardView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.pair_ssh_host_blocking(&ssh_alias, remote_port)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("task join error: {e}"))?
}

#[tauri::command]
fn remove_host(state: State<'_, DesktopAppState>, host_id: String) -> Result<(), String> {
    state.core.remove_host(&host_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_host_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    custom_name: Option<String>,
    route_mode: Option<HostRouteMode>,
    ssh_alias: Option<String>,
    remote_daemon_port: Option<u16>,
    custom_direct_endpoint: Option<String>,
    relay_url: Option<String>,
    reconnect: Option<bool>,
) -> Result<HostCardView, String> {
    state
        .core
        .update_host_config(
            &host_id,
            custom_name,
            route_mode,
            ssh_alias,
            remote_daemon_port,
            custom_direct_endpoint,
            relay_url,
            reconnect.unwrap_or(true),
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_host_relay(
    state: State<'_, DesktopAppState>,
    host_id: String,
    relay_url: Option<String>,
) -> Result<(), String> {
    state
        .core
        .update_host_relay(&host_id, relay_url.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn upgrade_remote_daemon(
    state: State<'_, DesktopAppState>,
    host_id: String,
    ssh_alias: String,
    remote_port: Option<u16>,
) -> Result<String, String> {
    let alias_copy = ssh_alias.clone();
    let (ver, maybe_uri) = tauri::async_runtime::spawn_blocking(move || {
        upgrade_remote_daemon_over_ssh(&ssh_alias, remote_port)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))??;
    if let Some(uri) = maybe_uri {
        let _ = state.core.pair_host(&uri);
        let _ = state.core.update_host_config(
            &host_id,
            None,
            None,
            Some(alias_copy),
            None,
            None,
            None,
            true,
        );
    } else {
        state.core.connect_host_async(host_id);
    }
    Ok(ver)
}

#[tauri::command]
fn upsert_project(
    state: State<'_, DesktopAppState>,
    project_id: Option<String>,
    host_id: String,
    name: String,
    directories: Vec<String>,
) -> Result<ClientProjectView, String> {
    state
        .core
        .upsert_project(project_id.as_deref(), &host_id, &name, directories)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_project(state: State<'_, DesktopAppState>, project_id: String) -> Result<(), String> {
    state
        .core
        .remove_project(&project_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn list_directory(state: State<'_, DesktopAppState>, host_id: String, path: Option<String>) {
    state.core.list_directory(&host_id, path);
}

#[tauri::command]
fn connect_host(state: State<'_, DesktopAppState>, host_id: String) {
    state.core.connect_host_async(host_id);
}

#[tauri::command]
fn connect_all_hosts(state: State<'_, DesktopAppState>) {
    state.core.connect_all_hosts();
}

#[tauri::command]
fn refresh_host(state: State<'_, DesktopAppState>, host_id: String) {
    state.core.refresh_host(&host_id);
}

#[tauri::command]
fn refresh_quotas(state: State<'_, DesktopAppState>, host_id: String) {
    state.core.refresh_quotas(&host_id);
}

#[tauri::command]
fn refresh_provider_models(
    state: State<'_, DesktopAppState>,
    host_id: String,
    force_refresh: bool,
) {
    state.core.refresh_provider_models(&host_id, force_refresh);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn create_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    extra_dirs: Option<Vec<String>>,
    provider: ProviderKind,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
    mode: SessionMode,
    initial_prompt: String,
    initial_display_prompt: Option<String>,
    initial_badges: Option<Vec<PromptBadge>>,
    worktree_branch: Option<String>,
    profile_id: Option<String>,
) -> String {
    state.core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &cwd,
            extra_dirs: extra_dirs.unwrap_or_default(),
            provider,
            model,
            effort,
            mode,
            initial_prompt: &initial_prompt,
            initial_display_prompt,
            initial_badges: initial_badges.unwrap_or_default(),
            worktree_branch: worktree_branch.as_deref(),
        },
        profile_id,
    )
}

#[tauri::command]
fn open_agent_timeline(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.open_agent_timeline(&host_id, &agent_id);
}

#[tauri::command]
fn send_prompt(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    prompt: String,
    display_prompt: Option<String>,
    badges: Option<Vec<PromptBadge>>,
    active_turn_behavior: Option<ActiveTurnBehavior>,
) {
    state.core.send_prompt_with_behavior(
        &host_id,
        &agent_id,
        &prompt,
        display_prompt,
        badges.unwrap_or_default(),
        active_turn_behavior.unwrap_or_default(),
    );
}

#[tauri::command]
fn interrupt_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.interrupt_agent(&host_id, &agent_id);
}

#[tauri::command]
fn set_session_mode(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    mode: SessionMode,
) {
    state.core.set_session_mode(&host_id, &agent_id, mode);
}

#[tauri::command]
fn set_agent_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
) {
    state
        .core
        .set_agent_config(&host_id, &agent_id, model, effort);
}

#[tauri::command]
fn set_agent_model(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    model: Option<String>,
) {
    state.core.set_agent_model(&host_id, &agent_id, model);
}

#[tauri::command]
fn switch_agent_provider(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    provider: ProviderKind,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
) {
    state
        .core
        .switch_agent_provider_with_config(&host_id, &agent_id, provider, model, effort);
}

#[tauri::command]
fn archive_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.archive_agent(&host_id, &agent_id);
}

#[tauri::command]
fn unarchive_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.unarchive_agent(&host_id, &agent_id);
}

#[tauri::command]
fn delete_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.delete_agent(&host_id, &agent_id);
}

#[tauri::command]
fn detach_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.detach_agent(&host_id, &agent_id);
}

#[tauri::command]
fn mark_agent_read(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.mark_agent_read(&host_id, &agent_id);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    title: Option<String>,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
    mode: Option<SessionMode>,
    labels: Option<HashMap<String, String>>,
) {
    state
        .core
        .update_agent(&host_id, &agent_id, title, model, effort, mode, labels);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn create_workspace_v2(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source_cwd: String,
    title: Option<String>,
    isolation: WorkspaceIsolation,
    worktree_mode: WorktreeCreateMode,
    worktree_branch: Option<String>,
    base_ref: Option<String>,
    pr_number: Option<u32>,
) {
    state.core.create_workspace_v2(
        &host_id,
        &source_cwd,
        title,
        isolation,
        worktree_mode,
        worktree_branch,
        base_ref,
        pr_number,
    );
}

#[tauri::command]
fn rename_workspace(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    title: String,
) {
    state.core.rename_workspace(&host_id, &workspace_id, &title);
}

#[tauri::command]
fn archive_workspace(state: State<'_, DesktopAppState>, host_id: String, workspace_id: String) {
    state.core.archive_workspace(&host_id, &workspace_id);
}

#[tauri::command]
fn unarchive_workspace(state: State<'_, DesktopAppState>, host_id: String, workspace_id: String) {
    state.core.unarchive_workspace(&host_id, &workspace_id);
}

#[tauri::command]
fn list_workspace_scripts(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
) {
    state.core.list_workspace_scripts(&host_id, &workspace_id);
}

#[tauri::command]
fn start_workspace_script(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    script_name: String,
) {
    state
        .core
        .start_workspace_script(&host_id, &workspace_id, &script_name);
}

#[tauri::command]
fn stop_workspace_script(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    script_name: String,
) {
    state
        .core
        .stop_workspace_script(&host_id, &workspace_id, &script_name);
}

#[tauri::command]
fn upsert_profile(state: State<'_, DesktopAppState>, host_id: String, profile: AgentProfile) {
    state.core.upsert_profile(&host_id, profile);
}

#[tauri::command]
fn delete_profile(state: State<'_, DesktopAppState>, host_id: String, profile_id: String) {
    state.core.delete_profile(&host_id, &profile_id);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn create_schedule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    name: String,
    cron_expr: String,
    timezone: Option<String>,
    prompt: String,
    target: ScheduleTarget,
    max_runs: Option<u32>,
) {
    state
        .core
        .create_schedule(&host_id, name, cron_expr, timezone, prompt, target, max_runs);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_schedule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    schedule_id: String,
    name: Option<String>,
    cron_expr: Option<String>,
    prompt: Option<String>,
    max_runs: Option<u32>,
    archive_on_finish: Option<bool>,
) {
    state.core.update_schedule(
        &host_id,
        &schedule_id,
        name,
        cron_expr,
        prompt,
        max_runs,
        archive_on_finish,
    );
}

#[tauri::command]
fn pause_schedule(state: State<'_, DesktopAppState>, host_id: String, schedule_id: String) {
    state.core.pause_schedule(&host_id, &schedule_id);
}

#[tauri::command]
fn resume_schedule(state: State<'_, DesktopAppState>, host_id: String, schedule_id: String) {
    state.core.resume_schedule(&host_id, &schedule_id);
}

#[tauri::command]
fn run_schedule_once(state: State<'_, DesktopAppState>, host_id: String, schedule_id: String) {
    state.core.run_schedule_once(&host_id, &schedule_id);
}

#[tauri::command]
fn delete_schedule(state: State<'_, DesktopAppState>, host_id: String, schedule_id: String) {
    state.core.delete_schedule(&host_id, &schedule_id);
}

#[tauri::command]
fn fetch_schedule_runs(
    state: State<'_, DesktopAppState>,
    host_id: String,
    schedule_id: String,
    limit: Option<u32>,
) {
    state
        .core
        .fetch_schedule_runs(&host_id, &schedule_id, limit);
}

#[tauri::command]
fn get_schedule_runs(
    state: State<'_, DesktopAppState>,
    host_id: String,
    schedule_id: String,
) -> Vec<ScheduleRunRecord> {
    state.core.schedule_runs(&host_id, &schedule_id)
}

#[tauri::command]
fn add_connected_host(
    state: State<'_, DesktopAppState>,
    host_id: String,
    label: Option<String>,
    pairing_uri: String,
) {
    state
        .core
        .add_connected_host(&host_id, label, &pairing_uri);
}

#[tauri::command]
fn update_connected_host(
    state: State<'_, DesktopAppState>,
    host_id: String,
    registration_id: String,
    label: Option<String>,
    enabled: Option<bool>,
) {
    state
        .core
        .update_connected_host(&host_id, &registration_id, label, enabled);
}

#[tauri::command]
fn remove_connected_host(
    state: State<'_, DesktopAppState>,
    host_id: String,
    registration_id: String,
) {
    state
        .core
        .remove_connected_host(&host_id, &registration_id);
}

#[tauri::command]
fn sync_fleet_connected_hosts(state: State<'_, DesktopAppState>, target_host_id: String) -> u32 {
    state.core.sync_fleet_connected_hosts(&target_host_id)
}

#[tauri::command]
fn update_orchestration_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    config: OrchestrationConfig,
) {
    state.core.update_orchestration_config(&host_id, config);
}

#[tauri::command]
fn sync_orchestration_skills(
    state: State<'_, DesktopAppState>,
    host_id: String,
    skill_ids: Vec<String>,
) {
    state.core.sync_orchestration_skills(&host_id, skill_ids);
}

#[tauri::command]
fn list_provider_subagents(
    state: State<'_, DesktopAppState>,
    host_id: String,
    parent_agent_id: String,
) {
    state
        .core
        .list_provider_subagents(&host_id, &parent_agent_id);
}

#[tauri::command]
fn open_provider_subagent_timeline(
    state: State<'_, DesktopAppState>,
    host_id: String,
    parent_agent_id: String,
    subagent_id: String,
) {
    state
        .core
        .open_provider_subagent_timeline(&host_id, &parent_agent_id, &subagent_id);
}

#[tauri::command]
fn get_provider_subagent_timeline(
    state: State<'_, DesktopAppState>,
    host_id: String,
    parent_agent_id: String,
    subagent_id: String,
) -> Vec<ProviderSubagentEvent> {
    state
        .core
        .provider_subagent_timeline(&host_id, &parent_agent_id, &subagent_id)
}

#[tauri::command]
fn refresh_pending_permissions(state: State<'_, DesktopAppState>, host_id: String) {
    state.core.refresh_pending_permissions(&host_id);
}

#[tauri::command]
fn resolve_permission(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    permission_request_id: String,
    decision: String,
) {
    state
        .core
        .resolve_permission(&host_id, &agent_id, &permission_request_id, &decision);
}

#[tauri::command]
fn answer_question(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    question_request_id: String,
    answers: Vec<Vec<String>>,
) {
    state
        .core
        .answer_question(&host_id, &agent_id, &question_request_id, answers);
}

#[tauri::command]
fn fetch_git_status(state: State<'_, DesktopAppState>, host_id: String, cwd: String) {
    state.core.fetch_git_status(&host_id, &cwd);
}

#[tauri::command]
fn fetch_git_diff(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: Option<String>,
    staged: bool,
) {
    state
        .core
        .fetch_git_diff(&host_id, &cwd, path.as_deref(), staged);
}

#[tauri::command]
fn start_terminal(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    cols: u16,
    rows: u16,
) {
    state.core.start_terminal(&host_id, &cwd, cols, rows);
}

#[tauri::command]
fn attach_terminal(
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
    slot: u8,
) {
    state.core.attach_terminal(&host_id, &terminal_id, slot);
}

#[tauri::command]
fn kill_terminal(state: State<'_, DesktopAppState>, host_id: String, terminal_id: String) {
    state.core.kill_terminal(&host_id, &terminal_id);
}

#[tauri::command]
fn send_terminal_input(state: State<'_, DesktopAppState>, host_id: String, data: String) {
    state.core.send_terminal_input(&host_id, data.into_bytes());
}

#[tauri::command]
fn send_terminal_input_slot(
    state: State<'_, DesktopAppState>,
    host_id: String,
    slot: u8,
    data: String,
) {
    state
        .core
        .send_terminal_input_slot(&host_id, slot, data.into_bytes());
}

#[tauri::command]
fn resize_terminal(state: State<'_, DesktopAppState>, host_id: String, cols: u16, rows: u16) {
    state.core.resize_terminal(&host_id, cols, rows);
}

#[tauri::command]
fn resize_terminal_slot(
    state: State<'_, DesktopAppState>,
    host_id: String,
    slot: u8,
    cols: u16,
    rows: u16,
) {
    state.core.resize_terminal_slot(&host_id, slot, cols, rows);
}

#[tauri::command]
fn list_ssh_hosts() -> Vec<SshHostItem> {
    discover_user_ssh_hosts()
}

#[tauri::command]
fn list_external_editors() -> Vec<ExternalEditorInfo> {
    detect_installed_editors()
}

#[tauri::command]
fn open_in_editor(
    editor: ExternalEditorKind,
    cwd: String,
    ssh_target: Option<String>,
) -> Result<(), String> {
    open_in_external_editor(editor, &cwd, ssh_target.as_deref())
}

#[tauri::command]
async fn pick_project_folder(initial_dir: Option<String>) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || pick_local_directory(initial_dir.as_deref()))
        .await
        .map_err(|e| format!("folder picker task error: {e}"))?
}

#[tauri::command]
fn get_workspace_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Vec<String> {
    let cached = state.core.workspace_files(&host_id, &cwd);
    if !cached.is_empty() {
        return cached;
    }
    state.core.fetch_workspace_files(&host_id, &cwd);
    list_local_workspace_files(&cwd, 2500).unwrap_or_default()
}

#[tauri::command]
fn stage_attachment(filename: String, bytes: Vec<u8>) -> Result<StagedAttachmentInfo, String> {
    stage_local_attachment(&filename, &bytes)
}

#[tauri::command]
async fn stage_host_attachment(
    state: State<'_, DesktopAppState>,
    host_id: String,
    filename: String,
    data_b64: String,
) -> Result<StagedAttachmentInfo, String> {
    state
        .core
        .stage_attachment_b64_on_host(&host_id, &filename, &data_b64)
        .await
}

#[tauri::command]
async fn provision_ssh_relay(
    state: State<'_, DesktopAppState>,
    target_host_id: String,
    ssh_alias: String,
    fallback_hostname: String,
    relay_port: u16,
) -> Result<String, String> {
    let overview = state.core.fleet_overview();
    let pubkeys: Vec<String> = overview
        .hosts
        .into_iter()
        .map(|h| h.daemon_pubkey_b64)
        .collect();

    let relay_url = tauri::async_runtime::spawn_blocking(move || {
        provision_relay_over_ssh(&ssh_alias, &fallback_hostname, relay_port, &pubkeys)
    })
    .await
    .map_err(|e| format!("task join error: {e}"))??;

    state
        .core
        .update_host_relay(&target_host_id, Some(&relay_url))
        .map_err(|e| e.to_string())?;

    Ok(relay_url)
}

#[tauri::command]
fn get_app_version() -> &'static str {
    CURRENT_APP_VERSION
}

#[tauri::command]
async fn check_app_update() -> Result<ReleaseUpdateInfo, String> {
    check_latest_release().await
}

#[tauri::command]
async fn perform_app_self_update(
    release: ReleaseUpdateInfo,
    on_progress: Channel<AppUpdateProgress>,
) -> Result<(), String> {
    perform_one_click_update(release, on_progress).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_path = default_desktop_db_path();
    let core = OrbitAppCore::open(&db_path, true)
        .or_else(|_| OrbitAppCore::open_in_memory())
        .expect("failed to initialize OrbitAppCore");
    core.connect_all_hosts();

    tauri::Builder::default()
        .manage(DesktopAppState { core })
        .invoke_handler(tauri::generate_handler![
            subscribe_core_events,
            get_fleet_overview,
            get_host_detail,
            get_agent_timeline,
            get_agent_timeline_delta,
            get_git_review,
            pair_host,
            pair_ssh_host,
            remove_host,
            update_host_config,
            update_host_relay,
            upgrade_remote_daemon,
            upsert_project,
            remove_project,
            list_directory,
            connect_host,
            connect_all_hosts,
            refresh_host,
            refresh_quotas,
            refresh_provider_models,
            create_agent,
            open_agent_timeline,
            send_prompt,
            interrupt_agent,
            set_session_mode,
            set_agent_config,
            set_agent_model,
            switch_agent_provider,
            archive_agent,
            unarchive_agent,
            delete_agent,
            detach_agent,
            mark_agent_read,
            update_agent,
            create_workspace_v2,
            rename_workspace,
            archive_workspace,
            unarchive_workspace,
            list_workspace_scripts,
            start_workspace_script,
            stop_workspace_script,
            upsert_profile,
            delete_profile,
            create_schedule,
            update_schedule,
            pause_schedule,
            resume_schedule,
            run_schedule_once,
            delete_schedule,
            fetch_schedule_runs,
            get_schedule_runs,
            add_connected_host,
            update_connected_host,
            remove_connected_host,
            sync_fleet_connected_hosts,
            update_orchestration_config,
            sync_orchestration_skills,
            list_provider_subagents,
            open_provider_subagent_timeline,
            get_provider_subagent_timeline,
            refresh_pending_permissions,
            resolve_permission,
            answer_question,
            fetch_git_status,
            fetch_git_diff,
            start_terminal,
            attach_terminal,
            kill_terminal,
            send_terminal_input,
            send_terminal_input_slot,
            resize_terminal,
            resize_terminal_slot,
            list_ssh_hosts,
            list_external_editors,
            open_in_editor,
            pick_project_folder,
            get_workspace_files,
            stage_attachment,
            stage_host_attachment,
            provision_ssh_relay,
            get_app_version,
            check_app_update,
            perform_app_self_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit Tauri desktop application");
}
