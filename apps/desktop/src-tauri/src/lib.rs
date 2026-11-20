mod updater;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_LOCAL_ID: AtomicU64 = AtomicU64::new(1);

fn next_unique_local_tag(prefix: &str) -> String {
    let seq = NEXT_LOCAL_ID.fetch_add(1, Ordering::Relaxed);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("{prefix}-{ts}-{seq}")
}

use crate::updater::{
    AppUpdateProgress, CURRENT_APP_VERSION, ReleaseUpdateInfo, check_latest_release,
    perform_one_click_update,
};

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::{
    AgentTimelineDeltaView, AgentTimelineView, AskSideQuestionRequest, ClientProjectView,
    ClientThreadPref, CoreEventSubscriber, CoreNotification, CreateAgentRequest, FleetOverviewView,
    GitReviewView, HostCardView, HostDetailView, OrbitAppCore, RewindAgentResultView,
    SideQuestionStreamEvent, SubscribeOptions, WorkspaceFileView,
};
use orbit_client_core::catalog::ResolvedModelSelectionView;
use orbit_client_core::composer::{
    ClientPlatform, ComposePromptRequest, ComposedPromptView, MentionCandidateView,
    SlashCommandView,
};
use orbit_client_core::contract::{
    BridgeInfoView, FleetSidebarRequest, MentionRankRequest, ModelSelectionRequest,
    RenderDiagramRequest, RenderDiagramView,
};
use orbit_client_core::desktop_ops::{
    ExternalEditorInfo, ExternalEditorKind, SshHostItem, StagedAttachmentInfo,
    detect_installed_editors, discover_user_ssh_hosts, list_local_workspace_files,
    open_external_url, open_in_external_editor, pick_local_directory, provision_relay_over_ssh,
    resolve_user_home, upgrade_remote_daemon_over_ssh,
};
use orbit_client_core::render::{RichBlockView, parse_markdown_blocks};
use orbit_client_core::sidebar::FleetSidebarView;
use orbit_client_core::stage::{AgentStageDeltaView, AgentStageView};
use orbit_protocol::{
    ActiveTurnBehavior, AgentProfile, OrchestrationConfig, PromptBadge, ProviderKind,
    ProviderModelCatalog, ProviderSubagentEvent, ReasoningEffort, ScheduleRunRecord,
    ScheduleTarget, SessionMode, WorkspaceIsolation, WorktreeCreateMode,
};
use orbit_providers::{SideQuestionSpec, run_side_question};
use tauri::{AppHandle, State, Url, WebviewUrl, WebviewWindowBuilder, ipc::Channel};

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
    state.core.subscribe_with_options(
        SubscribeOptions {
            min_interval_ms: 28,
            include_terminal_raw_bytes: true,
        },
        Arc::new(TauriChannelSubscriber { channel: on_event }),
    );
}

#[tauri::command]
fn get_bridge_info(state: State<'_, DesktopAppState>) -> BridgeInfoView {
    state.core.bridge_info()
}

#[tauri::command]
fn get_fleet_overview(state: State<'_, DesktopAppState>) -> FleetOverviewView {
    state.core.fleet_overview()
}

#[tauri::command]
fn get_fleet_sidebar(
    state: State<'_, DesktopAppState>,
    req: FleetSidebarRequest,
) -> FleetSidebarView {
    state.core.fleet_sidebar_by_request(&req)
}

#[tauri::command]
fn get_host_detail(state: State<'_, DesktopAppState>, host_id: String) -> Option<HostDetailView> {
    state.core.host_detail(&host_id)
}

#[tauri::command]
fn get_agent_stage(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> AgentStageView {
    state.core.agent_stage(&host_id, &agent_id)
}

#[tauri::command]
fn get_agent_stage_delta(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    since_revision: u64,
    since_aux_revision: Option<u64>,
) -> AgentStageDeltaView {
    state
        .core
        .agent_stage_delta_with_aux(&host_id, &agent_id, since_revision, since_aux_revision)
}

#[tauri::command]
fn get_fallback_provider_catalogs(state: State<'_, DesktopAppState>) -> Vec<ProviderModelCatalog> {
    state.core.fallback_provider_catalogs()
}

#[tauri::command]
fn resolve_model_selection(
    state: State<'_, DesktopAppState>,
    req: ModelSelectionRequest,
) -> ResolvedModelSelectionView {
    state.core.resolve_model_selection(&req)
}

#[tauri::command]
fn get_slash_commands(
    state: State<'_, DesktopAppState>,
    platform: Option<ClientPlatform>,
    query: Option<String>,
) -> Vec<SlashCommandView> {
    state.core.slash_commands(
        platform.unwrap_or(ClientPlatform::Desktop),
        query.as_deref(),
    )
}

#[tauri::command]
fn compose_prompt(
    state: State<'_, DesktopAppState>,
    req: ComposePromptRequest,
) -> ComposedPromptView {
    state.core.compose_prompt_for_host(&req)
}

#[tauri::command]
fn rank_mention_candidates(
    state: State<'_, DesktopAppState>,
    req: MentionRankRequest,
) -> Vec<MentionCandidateView> {
    state.core.rank_mention_candidates(&req)
}

#[tauri::command]
fn build_side_question_context(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    max_chars: Option<usize>,
) -> String {
    state
        .core
        .build_side_question_context(&host_id, &agent_id, max_chars)
}

#[tauri::command]
async fn render_diagram(req: RenderDiagramRequest) -> RenderDiagramView {
    let kind = req.kind;
    let theme = req.theme;
    match tokio::task::spawn_blocking(move || orbit_client_core::diagram::render_diagram(&req))
        .await
    {
        Ok(view) => view,
        Err(err) => RenderDiagramView {
            kind,
            theme,
            svg: None,
            width: 0.0,
            height: 0.0,
            error: Some(err.to_string()),
            diagram_type_label: None,
        },
    }
}

#[tauri::command]
fn mark_agent_seen(state: State<'_, DesktopAppState>, host_id: String, agent_id: String, seq: u64) {
    state.core.mark_agent_seen(&host_id, &agent_id, seq);
}

#[tauri::command]
fn mark_agent_unread(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.mark_agent_unread(&host_id, &agent_id);
}

#[tauri::command]
fn set_agent_pinned(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    pinned: bool,
) {
    state.core.set_agent_pinned(&host_id, &agent_id, pinned);
}

#[tauri::command]
fn toggle_pin_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) -> bool {
    state.core.toggle_pin_agent(&host_id, &agent_id)
}

#[tauri::command]
fn seed_thread_prefs(state: State<'_, DesktopAppState>, seeds: Vec<ClientThreadPref>) {
    state.core.seed_thread_prefs(seeds);
}

#[tauri::command]
fn on_app_foreground(state: State<'_, DesktopAppState>) {
    state.core.on_app_foreground();
}

#[tauri::command]
fn on_app_background(state: State<'_, DesktopAppState>) {
    state.core.on_app_background();
}

#[tauri::command]
fn on_network_changed(state: State<'_, DesktopAppState>) {
    state.core.on_network_changed();
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
async fn rewind_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    to_before_seq: u64,
    restore_files: bool,
) -> Result<RewindAgentResultView, String> {
    state
        .core
        .rewind_agent_on_host(&host_id, &agent_id, to_before_seq, restore_files)
        .await
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
    if state.core.is_local_host(&host_id) {
        list_local_workspace_files(&cwd, 2500).unwrap_or_default()
    } else {
        Vec::new()
    }
}

#[tauri::command]
async fn read_workspace_file(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
) -> Result<WorkspaceFileView, String> {
    state
        .core
        .read_workspace_file_on_host(&host_id, &cwd, &path)
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn ask_side_question(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
    cwd: String,
    provider: ProviderKind,
    model: Option<String>,
    question: String,
    context_summary: String,
    on_event: Channel<SideQuestionStreamEvent>,
) -> Result<String, String> {
    let is_local = state.core.is_local_host(&host_id);
    let fallback_payload = is_local.then(|| {
        (
            cwd.clone(),
            model.clone(),
            question.clone(),
            context_summary.clone(),
        )
    });

    let (ev_tx, mut ev_rx) = tokio::sync::mpsc::unbounded_channel::<SideQuestionStreamEvent>();
    match state
        .core
        .ask_side_question_on_host(
            AskSideQuestionRequest {
                host_id,
                agent_id,
                cwd,
                provider,
                model,
                question,
                context_summary,
            },
            ev_tx,
        )
        .await
    {
        Ok(req_id) => {
            tauri::async_runtime::spawn(async move {
                while let Some(ev) = ev_rx.recv().await {
                    let is_done = matches!(ev, SideQuestionStreamEvent::Done { .. });
                    let _ = on_event.send(ev);
                    if is_done {
                        break;
                    }
                }
            });
            Ok(req_id)
        }
        Err(daemon_err) => {
            let Some((fb_cwd, fb_model, fb_question, fb_context)) = fallback_payload else {
                return Err(daemon_err);
            };
            let req_id = next_unique_local_tag("sq");
            let resolved_cwd = if fb_cwd.trim().is_empty() || fb_cwd.trim() == "~" {
                resolve_user_home().unwrap_or_else(|| PathBuf::from("."))
            } else if let Some(rest) = fb_cwd.trim().strip_prefix("~/") {
                resolve_user_home()
                    .map(|h| h.join(rest))
                    .unwrap_or_else(|| PathBuf::from(fb_cwd.trim()))
            } else {
                PathBuf::from(fb_cwd.trim())
            };
            let req_id_bg = req_id.clone();
            tauri::async_runtime::spawn(async move {
                let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                let ch_delta = on_event.clone();
                let req_for_delta = req_id_bg.clone();
                let forward = tokio::spawn(async move {
                    while let Some(text_delta) = delta_rx.recv().await {
                        if ch_delta
                            .send(SideQuestionStreamEvent::Delta {
                                request_id: req_for_delta.clone(),
                                text_delta,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                });

                let result = run_side_question(
                    SideQuestionSpec {
                        provider,
                        cwd: resolved_cwd,
                        model: fb_model,
                        question: fb_question,
                        context_summary: fb_context,
                    },
                    delta_tx,
                )
                .await;

                let _ = forward.await;
                let done_ev = match result {
                    Ok(full_text) => SideQuestionStreamEvent::Done {
                        request_id: req_id_bg,
                        full_text,
                        error: None,
                    },
                    Err(err) => SideQuestionStreamEvent::Done {
                        request_id: req_id_bg,
                        full_text: String::new(),
                        error: Some(err),
                    },
                };
                let _ = on_event.send(done_ev);
            });
            Ok(req_id)
        }
    }
}

#[tauri::command]
fn parse_markdown_preview(markdown: String, streaming: bool) -> Vec<RichBlockView> {
    parse_markdown_blocks(&markdown, streaming)
}

#[tauri::command]
fn open_browser_popout_window(
    app: AppHandle,
    url: String,
    title: Option<String>,
) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL `{trimmed}`: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http:// and https:// URLs are supported".into());
    }

    let label = next_unique_local_tag("orbit-browser");
    let win_title = title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed);

    WebviewWindowBuilder::new(&app, label, WebviewUrl::External(parsed))
        .title(win_title)
        .inner_size(1120.0, 760.0)
        .min_inner_size(420.0, 360.0)
        .on_navigation(|target| matches!(target.scheme(), "http" | "https" | "about"))
        .build()
        .map_err(|e| format!("Failed to open browser window: {e}"))?;

    Ok(())
}

#[tauri::command]
fn open_external_browser_url(url: String) -> Result<(), String> {
    let trimmed = url.trim();
    let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL `{trimmed}`: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only http:// and https:// URLs can be opened externally".into());
    }
    open_external_url(parsed.as_str())
}

fn decode_percent_utf8(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (
                char::from(bytes[i + 1]).to_digit(16),
                char::from(bytes[i + 2]).to_digit(16),
            )
        {
            out.push(((hi << 4) | lo) as u8);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
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
async fn stage_host_attachment_bytes(
    request: tauri::ipc::Request<'_>,
    state: State<'_, DesktopAppState>,
) -> Result<StagedAttachmentInfo, String> {
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(raw) => raw.clone(),
        tauri::ipc::InvokeBody::Json(_) => {
            return Err("Expected raw binary body for attachment upload".into());
        }
    };
    let host_id = request
        .headers()
        .get("x-orbit-host-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Missing x-orbit-host-id header".to_string())?
        .to_string();
    let filename = request
        .headers()
        .get("x-orbit-filename")
        .and_then(|v| v.to_str().ok())
        .map(decode_percent_utf8)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "attachment.bin".to_string());
    state
        .core
        .stage_attachment_on_host(&host_id, &filename, bytes)
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

fn init_desktop_file_logging() {
    let log_path = resolve_user_home()
        .map(|home| home.join(".orbit").join("logs").join("desktop.log"))
        .unwrap_or_else(|| PathBuf::from("orbit-desktop.log"));

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if let Ok(writer) = orbit_client_core::logging::RotatingLogWriter::open(
        log_path,
        orbit_client_core::logging::DEFAULT_LOG_MAX_BYTES,
        orbit_client_core::logging::DEFAULT_LOG_MAX_BACKUPS,
        cfg!(debug_assertions),
    ) {
        let _ = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter(env_filter)
            .with_writer(move || writer.clone())
            .try_init();
    } else {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .try_init();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_desktop_file_logging();
    let db_path = default_desktop_db_path();
    tracing::info!(
        version = CURRENT_APP_VERSION,
        db_path = %db_path.display(),
        "Starting Orbit Desktop"
    );
    let core = OrbitAppCore::open(&db_path, true)
        .or_else(|err| {
            tracing::error!(error = %err, "Failed to open desktop replica SQLite; falling back to in-memory store");
            OrbitAppCore::open_in_memory()
        })
        .expect("failed to initialize OrbitAppCore");
    core.connect_all_hosts();

    tauri::Builder::default()
        .manage(DesktopAppState { core })
        .invoke_handler(tauri::generate_handler![
            subscribe_core_events,
            get_bridge_info,
            get_fleet_overview,
            get_fleet_sidebar,
            get_host_detail,
            get_agent_stage,
            get_agent_stage_delta,
            get_fallback_provider_catalogs,
            resolve_model_selection,
            get_slash_commands,
            compose_prompt,
            rank_mention_candidates,
            build_side_question_context,
            mark_agent_seen,
            mark_agent_unread,
            set_agent_pinned,
            toggle_pin_agent,
            seed_thread_prefs,
            on_app_foreground,
            on_app_background,
            on_network_changed,
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
            rewind_agent,
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
            read_workspace_file,
            ask_side_question,
            render_diagram,
            parse_markdown_preview,
            open_browser_popout_window,
            open_external_browser_url,
            stage_host_attachment,
            stage_host_attachment_bytes,
            provision_ssh_relay,
            get_app_version,
            check_app_update,
            perform_app_self_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit Tauri desktop application");
}
