mod qr;
mod tray;
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
    AppUpdateProgress, CURRENT_APP_VERSION, LocalDaemonSyncResult, ReleaseUpdateInfo,
    check_latest_release, perform_one_click_update, sync_local_daemon as run_sync_local_daemon,
};

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::LifecycleHooksSnapshotView;
use orbit_client_core::app_core::{
    AgentTimelineDeltaView, AgentTimelineView, AskSideQuestionRequest, ClientProjectView,
    ClientThreadPref, CoreEventSubscriber, CoreNotification, CreateAgentRequest, FleetOverviewView,
    GitReviewView, GuardianConfigSnapshotView, HostCardView, HostDetailView,
    ImChannelTestResultView, ImChannelsSnapshotView, ImportManagedSkillRequest,
    ManagedSkillContentView, OrbitAppCore, PushDevicesSnapshotView, PushTestResultView,
    RegisterPushDeviceRequest, RewindAgentResultView, SideQuestionStreamEvent, SubscribeOptions,
    UpdatePushPreferencesRequest, UpsertImChannelRequest, UpsertManagedSkillRequest,
    WorkspaceFileView,
};
use orbit_client_core::catalog::ResolvedModelSelectionView;
use orbit_client_core::cloud_sync::{
    CloudSyncDevicePollView, CloudSyncResultView, CloudSyncStatusView, GitHubDeviceFlowStartView,
};
use orbit_client_core::composer::{
    ClientPlatform, ComposePromptRequest, ComposedPromptView, MentionCandidateView,
    SlashCommandView,
};
use orbit_client_core::contract::{
    AcpRegistryRefreshView, BridgeInfoView, ConversationSearchHit, CreateWorkspaceEntryRequest,
    DeletePathRuleRequest, DeleteWorkspaceEntryRequest, EventTriggersSnapshotView,
    ExecuteWorktreeHandoffRequest, FleetSidebarRequest, ForkAgentRequest, ForkAgentResultView,
    GitReposInspectionView, InstructionFileContentView, InstructionFileSavedView,
    InstructionFilesSnapshotView, ListInstructionFilesRequest, MatchPathRulesRequest,
    MatchedPathRulesView, MentionRankRequest, ModelSelectionRequest, PathRulesSnapshotView,
    PreviewWorktreeHandoffRequest, ReadInstructionFileRequest, RenameWorkspaceEntryRequest,
    RenderDiagramRequest, RenderDiagramView, SaveInstructionFileRequest,
    SearchWorkspaceSymbolsRequest, SpawnWorkspacePeerRequest, UpsertArtifactRequest,
    UpsertEventTriggerRequest, UpsertExternalMcpServerRequest, UpsertPathRuleRequest,
    WorkspaceEntryMutationResultView, WorkspaceFileWriteResultView, WorkspaceSymbolsView,
    WriteWorkspaceFileRequest,
};
use orbit_client_core::desktop_ops::{
    ExternalEditorInfo, ExternalEditorKind, SshHostItem, StagedAttachmentInfo,
    detect_installed_editors, discover_user_ssh_hosts, list_local_workspace_files,
    open_external_url, open_in_external_editor, open_local_file_externally as open_local_file_os,
    pick_local_directory, pick_local_save_file, provision_relay_over_ssh, resolve_user_home,
    upgrade_remote_daemon_over_ssh,
};
use orbit_client_core::export::{
    ExportOptions, ExportedTranscript, ExtractedCodeBlock, write_file_atomically,
};
use orbit_client_core::render::{
    HighlightedLineView, RichBlockView, highlight_code_lines_capped, parse_markdown_blocks,
};
use orbit_client_core::sidebar::FleetSidebarView;
use orbit_client_core::stage::{AgentStageDeltaView, AgentStageView, StageTurnView};
use orbit_protocol::{
    AcpAgentAvailability, AcpAgentSpec, AcpProbeResult, ActiveTurnBehavior, AgentProfile,
    ArtifactDetail, ArtifactSummary, ConditionalTimerRecord, ContextOccupancyBreakdown,
    EventTriggerDelivery, ExternalMcpServerRecord, GuardianPolicyConfig, HookExecutionRecord,
    HookScope, LifecycleHookRule, LspDiagnostic, LspLocation, LspServerStatus, LspSymbolInfo,
    ManagedSkillRecord, McpProbeResult, OrchestrationConfig, PromptBadge, ProviderKind,
    ProviderModelCatalog, ProviderSubagentEvent, ReasoningEffort, ScheduleRunRecord,
    ScheduleTarget, SessionMode, SkillWriteScope, TimerCondition, UsageInsightsSnapshot,
    WorkspaceBoardSnapshot, WorkspaceEventRecord, WorkspaceIsolation, WorktreeCreateMode,
    WorktreeHandoffOutcome, WorktreeHandoffPreview,
};
use orbit_providers::{
    ProviderReadinessInfo, SideQuestionSpec,
    probe_local_provider_readiness as run_local_provider_readiness_probe, run_side_question,
};
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
    state.core.replace_subscribers(
        SubscribeOptions {
            min_interval_ms: 33,
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
async fn search_messages(
    state: State<'_, DesktopAppState>,
    query: String,
    limit: Option<u32>,
) -> Result<Vec<ConversationSearchHit>, String> {
    let core = Arc::clone(&state.core);
    let max_hits = limit.unwrap_or(20);
    tauri::async_runtime::spawn_blocking(move || core.search_messages(&query, max_hits))
        .await
        .map_err(|e| format!("search messages task error: {e}"))
}

#[tauri::command]
async fn export_agent_transcript(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    options: Option<ExportOptions>,
) -> Result<ExportedTranscript, String> {
    let core = Arc::clone(&state.core);
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        core.export_agent_transcript(&host_id, &agent_id, &opts)
    })
    .await
    .map_err(|e| format!("export transcript task error: {e}"))?
}

#[tauri::command]
async fn save_exported_transcript(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    options: Option<ExportOptions>,
    initial_dir: Option<String>,
) -> Result<Option<String>, String> {
    let core = Arc::clone(&state.core);
    let opts = options.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        let exported = core.export_agent_transcript(&host_id, &agent_id, &opts)?;
        let Some(chosen) = pick_local_save_file(
            &exported.suggested_filename,
            opts.format.extension(),
            initial_dir.as_deref(),
        )?
        else {
            return Ok(None);
        };
        let target_path = PathBuf::from(&chosen);
        write_file_atomically(&target_path, exported.content.as_bytes())?;
        Ok(Some(chosen))
    })
    .await
    .map_err(|e| format!("save exported transcript task error: {e}"))?
}

#[tauri::command]
async fn list_agent_code_blocks(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<Vec<ExtractedCodeBlock>, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.list_agent_code_blocks(&host_id, &agent_id))
        .await
        .map_err(|e| format!("list code blocks task error: {e}"))?
}

#[tauri::command]
async fn get_fleet_sidebar(
    state: State<'_, DesktopAppState>,
    req: FleetSidebarRequest,
) -> Result<FleetSidebarView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.fleet_sidebar_by_request(&req))
        .await
        .map_err(|e| format!("fleet sidebar task error: {e}"))
}

#[tauri::command]
fn get_host_detail(state: State<'_, DesktopAppState>, host_id: String) -> Option<HostDetailView> {
    state.core.host_detail(&host_id)
}

#[tauri::command]
async fn get_agent_stage(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<AgentStageView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.agent_stage(&host_id, &agent_id))
        .await
        .map_err(|e| format!("agent stage task error: {e}"))
}

#[tauri::command]
async fn get_agent_stage_delta(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    since_revision: u64,
    since_aux_revision: Option<u64>,
) -> Result<AgentStageDeltaView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.agent_stage_delta_with_aux(&host_id, &agent_id, since_revision, since_aux_revision)
    })
    .await
    .map_err(|e| format!("agent stage delta task error: {e}"))
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
async fn get_git_review(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: Option<String>,
) -> Result<GitReviewView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.git_review(&host_id, &cwd, path.as_deref()))
        .await
        .map_err(|e| format!("git review task error: {e}"))
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
fn export_host_pairing_uri(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<String, String> {
    state
        .core
        .export_host_pairing_uri(&host_id)
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
fn reorder_projects(
    state: State<'_, DesktopAppState>,
    ordered_keys: Vec<String>,
) -> Result<Vec<String>, String> {
    state
        .core
        .reorder_projects(ordered_keys)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn get_project_order(state: State<'_, DesktopAppState>) -> Vec<String> {
    state.core.project_order()
}

#[tauri::command]
async fn inspect_git_repos(
    state: State<'_, DesktopAppState>,
    host_id: String,
    path: String,
    extra_dirs: Option<Vec<String>>,
) -> Result<GitReposInspectionView, String> {
    state
        .core
        .inspect_git_repos_on_host(&host_id, &path, extra_dirs.unwrap_or_default())
        .await
}

#[tauri::command]
fn get_git_repos(
    state: State<'_, DesktopAppState>,
    host_id: String,
    path: String,
) -> Option<GitReposInspectionView> {
    state.core.git_repos_inspection(&host_id, &path)
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
    acp_agent_id: Option<String>,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
    fast_mode: Option<bool>,
    mode: SessionMode,
    initial_prompt: String,
    initial_display_prompt: Option<String>,
    initial_badges: Option<Vec<PromptBadge>>,
    worktree_branch: Option<String>,
    profile_id: Option<String>,
    workspace_id: Option<String>,
    handle: Option<String>,
    auto_cluster: Option<bool>,
) -> String {
    state.core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &cwd,
            extra_dirs: extra_dirs.unwrap_or_default(),
            provider,
            acp_agent_id,
            model,
            effort,
            fast_mode,
            mode,
            initial_prompt: &initial_prompt,
            initial_display_prompt,
            initial_badges: initial_badges.unwrap_or_default(),
            worktree_branch: worktree_branch.as_deref(),
            workspace_id: workspace_id.as_deref(),
            handle,
            auto_cluster,
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
fn enqueue_prompt(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    prompt: String,
    display_prompt: Option<String>,
    badges: Option<Vec<PromptBadge>>,
    queue_id: Option<String>,
) -> String {
    state.core.enqueue_prompt(
        &host_id,
        &agent_id,
        &prompt,
        display_prompt,
        badges.unwrap_or_default(),
        queue_id,
    )
}

#[tauri::command]
fn remove_queued_prompt(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    queue_id: String,
) {
    state
        .core
        .remove_queued_prompt(&host_id, &agent_id, &queue_id);
}

#[tauri::command]
fn flush_queued_prompt_now(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    queue_id: String,
) {
    state
        .core
        .flush_queued_prompt_now(&host_id, &agent_id, &queue_id);
}

#[tauri::command]
fn reorder_queued_prompts(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    queue_ids: Vec<String>,
) {
    state
        .core
        .reorder_queued_prompts(&host_id, &agent_id, queue_ids);
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
    acp_agent_id: Option<String>,
) {
    state.core.switch_agent_provider_with_config(
        &host_id,
        &agent_id,
        provider,
        model,
        effort,
        acp_agent_id,
    );
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
    fast_mode: Option<bool>,
    mode: Option<SessionMode>,
    labels: Option<HashMap<String, String>>,
    handle: Option<String>,
) {
    state.core.update_agent_full(
        &host_id, &agent_id, title, model, effort, fast_mode, mode, labels, handle,
    );
}

#[tauri::command]
fn fetch_workspace_board(state: State<'_, DesktopAppState>, host_id: String, workspace_id: String) {
    state.core.fetch_workspace_board(&host_id, &workspace_id);
}

#[tauri::command]
fn get_workspace_board(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
) -> Option<WorkspaceBoardSnapshot> {
    state.core.workspace_board(&host_id, &workspace_id)
}

#[tauri::command]
fn update_workspace_board(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    content: String,
    section: Option<String>,
    updated_by: Option<String>,
) {
    state
        .core
        .update_workspace_board(&host_id, &workspace_id, content, section, updated_by);
}

#[tauri::command]
fn fetch_workspace_events(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    limit: Option<u32>,
) {
    state
        .core
        .fetch_workspace_events(&host_id, &workspace_id, limit);
}

#[tauri::command]
fn get_workspace_events(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
) -> Vec<WorkspaceEventRecord> {
    state.core.workspace_events(&host_id, &workspace_id)
}

#[tauri::command]
fn update_workspace_goal(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    goal: String,
) {
    state
        .core
        .update_workspace_goal(&host_id, &workspace_id, goal);
}

#[tauri::command]
fn move_agent_to_workspace(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    target_workspace_id: Option<String>,
) {
    state
        .core
        .move_agent_to_workspace(&host_id, &agent_id, target_workspace_id);
}

#[tauri::command]
fn move_agent_to_workspace_v2(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    target_workspace_id: Option<String>,
    new_workspace_title: Option<String>,
    make_unclassified: bool,
) {
    state.core.move_agent_to_workspace_v2(
        &host_id,
        &agent_id,
        target_workspace_id,
        new_workspace_title,
        make_unclassified,
    );
}

#[tauri::command]
fn organize_project_workspaces(
    state: State<'_, DesktopAppState>,
    host_id: String,
    project_root: String,
    agent_ids: Vec<String>,
    auto_apply: bool,
) {
    state
        .core
        .organize_project_workspaces(&host_id, &project_root, agent_ids, auto_apply);
}

#[tauri::command]
fn summarize_agent(state: State<'_, DesktopAppState>, host_id: String, agent_id: String) {
    state.core.summarize_agent(&host_id, &agent_id);
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_agent_identity(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    handle: Option<String>,
    title: Option<String>,
    task_summary: Option<String>,
    lock_handle: bool,
    lock_title: bool,
) {
    state.core.update_agent_identity(
        &host_id,
        &agent_id,
        handle,
        title,
        task_summary,
        lock_handle,
        lock_title,
    );
}

#[tauri::command]
fn merge_workspaces(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source_workspace_id: String,
    target_workspace_id: String,
) {
    state
        .core
        .merge_workspaces(&host_id, &source_workspace_id, &target_workspace_id);
}

#[tauri::command]
fn dismiss_cluster_suggestion(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) {
    state.core.dismiss_cluster_suggestion(&host_id, &agent_id);
}

#[tauri::command]
fn spawn_workspace_peer(
    state: State<'_, DesktopAppState>,
    req: SpawnWorkspacePeerRequest,
) -> String {
    state.core.spawn_workspace_peer(req)
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
#[allow(clippy::too_many_arguments)]
fn create_mission_workspace(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source_cwd: String,
    title: String,
    mission_goal: Option<String>,
    isolation: WorkspaceIsolation,
    worktree_branch: Option<String>,
    base_ref: Option<String>,
) {
    state.core.create_mission_workspace(
        &host_id,
        &source_cwd,
        title,
        mission_goal,
        isolation,
        worktree_branch,
        base_ref,
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
fn archive_workspace_v2(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    keep_agents_unclassified: bool,
) {
    state
        .core
        .archive_workspace_v2(&host_id, &workspace_id, keep_agents_unclassified);
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
async fn list_artifacts(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: Option<String>,
    agent_id: Option<String>,
) -> Result<Vec<ArtifactSummary>, String> {
    state
        .core
        .list_artifacts_async(&host_id, workspace_id, agent_id)
        .await
}

#[tauri::command]
async fn get_artifact(
    state: State<'_, DesktopAppState>,
    host_id: String,
    artifact_id: String,
    version: Option<u32>,
) -> Result<ArtifactDetail, String> {
    state
        .core
        .get_artifact_async(&host_id, &artifact_id, version)
        .await
}

#[tauri::command]
async fn upsert_artifact(
    state: State<'_, DesktopAppState>,
    request: UpsertArtifactRequest,
) -> Result<ArtifactSummary, String> {
    state.core.upsert_artifact_async(request).await
}

#[tauri::command]
async fn delete_artifact(
    state: State<'_, DesktopAppState>,
    host_id: String,
    artifact_id: String,
) -> Result<String, String> {
    state
        .core
        .delete_artifact_async(&host_id, &artifact_id)
        .await
}

#[tauri::command]
fn list_acp_agents(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Vec<AcpAgentAvailability> {
    state.core.list_acp_agents(&host_id)
}

#[tauri::command]
fn refresh_acp_agents(state: State<'_, DesktopAppState>, host_id: String) {
    state.core.refresh_acp_agents(&host_id);
}

#[tauri::command]
async fn upsert_custom_acp_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    spec: AcpAgentSpec,
) -> Result<Vec<AcpAgentAvailability>, String> {
    state
        .core
        .upsert_custom_acp_agent_async(&host_id, spec)
        .await
}

#[tauri::command]
async fn delete_custom_acp_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<Vec<AcpAgentAvailability>, String> {
    state
        .core
        .delete_custom_acp_agent_async(&host_id, &agent_id)
        .await
}

#[tauri::command]
async fn test_acp_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    cwd: Option<String>,
) -> Result<AcpProbeResult, String> {
    state
        .core
        .test_acp_agent_async(&host_id, &agent_id, cwd)
        .await
}

#[tauri::command]
async fn refresh_acp_registry(
    state: State<'_, DesktopAppState>,
    host_id: String,
    registry_url: Option<String>,
) -> Result<AcpRegistryRefreshView, String> {
    state
        .core
        .refresh_acp_registry_async(&host_id, registry_url)
        .await
}

#[tauri::command]
async fn list_external_mcp_servers(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    state
        .core
        .list_external_mcp_servers_async(&host_id, cwd)
        .await
}

#[tauri::command]
async fn upsert_external_mcp_server(
    state: State<'_, DesktopAppState>,
    request: UpsertExternalMcpServerRequest,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    state.core.upsert_external_mcp_server_async(request).await
}

#[tauri::command]
async fn toggle_external_mcp_server(
    state: State<'_, DesktopAppState>,
    host_id: String,
    server_id: String,
    enabled: bool,
    cwd: Option<String>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    state
        .core
        .toggle_external_mcp_server_async(&host_id, &server_id, enabled, cwd)
        .await
}

#[tauri::command]
async fn delete_external_mcp_server(
    state: State<'_, DesktopAppState>,
    host_id: String,
    server_id: String,
    cwd: Option<String>,
) -> Result<Vec<ExternalMcpServerRecord>, String> {
    state
        .core
        .delete_external_mcp_server_async(&host_id, &server_id, cwd)
        .await
}

#[tauri::command]
async fn probe_external_mcp_server(
    state: State<'_, DesktopAppState>,
    host_id: String,
    server_id: String,
    cwd: Option<String>,
) -> Result<McpProbeResult, String> {
    state
        .core
        .probe_external_mcp_server_async(&host_id, &server_id, cwd)
        .await
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
    state.core.create_schedule(
        &host_id, name, cron_expr, timezone, prompt, target, max_runs,
    );
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn update_schedule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    schedule_id: String,
    name: Option<String>,
    cron_expr: Option<String>,
    timezone: Option<String>,
    prompt: Option<String>,
    max_runs: Option<u32>,
    archive_on_finish: Option<bool>,
) {
    state.core.update_schedule(
        &host_id,
        &schedule_id,
        name,
        cron_expr,
        timezone,
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
    state.core.add_connected_host(&host_id, label, &pairing_uri);
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
    state.core.remove_connected_host(&host_id, &registration_id);
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
async fn list_managed_skills(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state.core.list_managed_skills_async(&host_id, cwd).await
}

#[tauri::command]
async fn read_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    skill_id: String,
    cwd: Option<String>,
) -> Result<ManagedSkillContentView, String> {
    state
        .core
        .read_managed_skill_async(&host_id, &skill_id, cwd)
        .await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn upsert_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    scope: SkillWriteScope,
    cwd: Option<String>,
    slug: String,
    name: String,
    description: String,
    content: String,
    enabled: bool,
    expected_etag: Option<String>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state
        .core
        .upsert_managed_skill_async(UpsertManagedSkillRequest {
            host_id,
            scope,
            cwd,
            slug,
            name,
            description,
            content,
            enabled,
            expected_etag,
        })
        .await
}

#[tauri::command]
async fn toggle_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    skill_id: String,
    enabled: bool,
    cwd: Option<String>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state
        .core
        .toggle_managed_skill_async(&host_id, &skill_id, enabled, cwd)
        .await
}

#[tauri::command]
async fn delete_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    skill_id: String,
    cwd: Option<String>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state
        .core
        .delete_managed_skill_async(&host_id, &skill_id, cwd)
        .await
}

#[tauri::command]
async fn import_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    scope: SkillWriteScope,
    cwd: Option<String>,
    source_markdown_or_path: String,
    slug_override: Option<String>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state
        .core
        .import_managed_skill_async(ImportManagedSkillRequest {
            host_id,
            scope,
            cwd,
            source_markdown_or_path,
            slug_override,
        })
        .await
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
fn get_provider_subagent_turns(
    state: State<'_, DesktopAppState>,
    host_id: String,
    parent_agent_id: String,
    subagent_id: String,
    fallback_prompt: Option<String>,
) -> Vec<StageTurnView> {
    state.core.provider_subagent_turns(
        &host_id,
        &parent_agent_id,
        &subagent_id,
        fallback_prompt.as_deref(),
    )
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
async fn fork_agent(
    state: State<'_, DesktopAppState>,
    req: ForkAgentRequest,
) -> Result<ForkAgentResultView, String> {
    state.core.fork_agent_on_host(req).await
}

#[tauri::command]
async fn preview_worktree_handoff(
    state: State<'_, DesktopAppState>,
    req: PreviewWorktreeHandoffRequest,
) -> Result<WorktreeHandoffPreview, String> {
    state.core.preview_worktree_handoff_on_host(req).await
}

#[tauri::command]
async fn execute_worktree_handoff(
    state: State<'_, DesktopAppState>,
    req: ExecuteWorktreeHandoffRequest,
) -> Result<WorktreeHandoffOutcome, String> {
    state.core.execute_worktree_handoff_on_host(req).await
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
async fn list_ssh_hosts() -> Result<Vec<SshHostItem>, String> {
    tauri::async_runtime::spawn_blocking(discover_user_ssh_hosts)
        .await
        .map_err(|e| format!("list ssh hosts task error: {e}"))
}

#[tauri::command]
async fn list_external_editors() -> Result<Vec<ExternalEditorInfo>, String> {
    tauri::async_runtime::spawn_blocking(detect_installed_editors)
        .await
        .map_err(|e| format!("list external editors task error: {e}"))
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
async fn get_workspace_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    force: Option<bool>,
) -> Result<Vec<String>, String> {
    let core = Arc::clone(&state.core);
    let force_refresh = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        if !force_refresh {
            let cached = core.workspace_files(&host_id, &cwd);
            if !cached.is_empty() {
                return cached;
            }
        }
        core.fetch_workspace_files(&host_id, &cwd);
        if core.is_local_host(&host_id) {
            list_local_workspace_files(&cwd, 10_000).unwrap_or_default()
        } else {
            core.workspace_files(&host_id, &cwd)
        }
    })
    .await
    .map_err(|e| format!("workspace files task error: {e}"))
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
async fn search_workspace_symbols(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    query: String,
    limit: Option<u32>,
) -> Result<WorkspaceSymbolsView, String> {
    state
        .core
        .search_workspace_symbols_on_host(SearchWorkspaceSymbolsRequest {
            host_id,
            cwd,
            query,
            limit: limit.unwrap_or(30),
        })
        .await
}

#[tauri::command]
async fn write_workspace_file(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    content: String,
    expected_etag: Option<String>,
    force: Option<bool>,
) -> Result<WorkspaceFileWriteResultView, String> {
    state
        .core
        .write_workspace_file_on_host(WriteWorkspaceFileRequest {
            host_id,
            cwd,
            path,
            content,
            expected_etag,
            force: force.unwrap_or(false),
        })
        .await
}

#[tauri::command]
async fn create_workspace_entry(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    is_dir: bool,
    initial_content: Option<String>,
) -> Result<WorkspaceEntryMutationResultView, String> {
    state
        .core
        .create_workspace_entry_on_host(CreateWorkspaceEntryRequest {
            host_id,
            cwd,
            path,
            is_dir,
            initial_content,
        })
        .await
}

#[tauri::command]
async fn rename_workspace_entry(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    old_path: String,
    new_path: String,
) -> Result<WorkspaceEntryMutationResultView, String> {
    state
        .core
        .rename_workspace_entry_on_host(RenameWorkspaceEntryRequest {
            host_id,
            cwd,
            old_path,
            new_path,
        })
        .await
}

#[tauri::command]
async fn delete_workspace_entry(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
) -> Result<WorkspaceEntryMutationResultView, String> {
    state
        .core
        .delete_workspace_entry_on_host(DeleteWorkspaceEntryRequest { host_id, cwd, path })
        .await
}

#[tauri::command]
async fn list_instruction_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<InstructionFilesSnapshotView, String> {
    state
        .core
        .list_instruction_files_on_host(ListInstructionFilesRequest { host_id, cwd })
        .await
}

#[tauri::command]
async fn read_instruction_file(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
) -> Result<InstructionFileContentView, String> {
    state
        .core
        .read_instruction_file_on_host(ReadInstructionFileRequest { host_id, cwd, path })
        .await
}

#[tauri::command]
async fn save_instruction_file(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    content: String,
    expected_etag: Option<String>,
) -> Result<InstructionFileSavedView, String> {
    state
        .core
        .save_instruction_file_on_host(SaveInstructionFileRequest {
            host_id,
            cwd,
            path,
            content,
            expected_etag,
        })
        .await
}

#[tauri::command]
async fn list_path_rules(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<PathRulesSnapshotView, String> {
    state.core.list_path_rules_on_host(host_id, cwd).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn upsert_path_rule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    scope: orbit_protocol::RuleScope,
    name: String,
    description: String,
    trigger_mode: orbit_protocol::RuleTriggerMode,
    globs: Vec<String>,
    enabled: bool,
    body: String,
    expected_etag: Option<String>,
) -> Result<PathRulesSnapshotView, String> {
    state
        .core
        .upsert_path_rule_on_host(UpsertPathRuleRequest {
            host_id,
            cwd,
            scope,
            name,
            description,
            trigger_mode,
            globs,
            enabled,
            body,
            expected_etag,
        })
        .await
}

#[tauri::command]
async fn delete_path_rule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
) -> Result<PathRulesSnapshotView, String> {
    state
        .core
        .delete_path_rule_on_host(DeletePathRuleRequest { host_id, cwd, path })
        .await
}

#[tauri::command]
async fn match_path_rules(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    active_paths: Vec<String>,
    manual_rule_names: Vec<String>,
) -> Result<MatchedPathRulesView, String> {
    state
        .core
        .match_path_rules_on_host(MatchPathRulesRequest {
            host_id,
            cwd,
            active_paths,
            manual_rule_names,
        })
        .await
}

#[tauri::command]
async fn create_conditional_timer(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    duration_seconds: u64,
    prompt: String,
    condition: TimerCondition,
) -> Result<ConditionalTimerRecord, String> {
    state
        .core
        .create_conditional_timer_on_host(&host_id, &agent_id, duration_seconds, prompt, condition)
        .await
}

#[tauri::command]
async fn list_conditional_timers(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
    include_resolved: Option<bool>,
) -> Result<Vec<ConditionalTimerRecord>, String> {
    state
        .core
        .list_conditional_timers_on_host(&host_id, agent_id, include_resolved.unwrap_or(true))
        .await
}

#[tauri::command]
async fn cancel_conditional_timer(
    state: State<'_, DesktopAppState>,
    host_id: String,
    timer_id: String,
) -> Result<ConditionalTimerRecord, String> {
    state
        .core
        .cancel_conditional_timer_on_host(&host_id, &timer_id)
        .await
}

#[tauri::command]
async fn fire_conditional_timer_now(
    state: State<'_, DesktopAppState>,
    host_id: String,
    timer_id: String,
) -> Result<ConditionalTimerRecord, String> {
    state
        .core
        .fire_conditional_timer_now_on_host(&host_id, &timer_id)
        .await
}

#[tauri::command]
async fn list_lsp_servers(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<Vec<LspServerStatus>, String> {
    state.core.list_lsp_servers_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn lsp_query_symbols(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    query: String,
    path: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<LspSymbolInfo>, String> {
    state
        .core
        .lsp_query_symbols_on_host(&host_id, &cwd, &query, path, limit.unwrap_or(50))
        .await
}

#[tauri::command]
async fn lsp_get_diagnostics(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: Option<String>,
) -> Result<Vec<LspDiagnostic>, String> {
    state
        .core
        .lsp_get_diagnostics_on_host(&host_id, &cwd, path)
        .await
}

#[tauri::command]
async fn lsp_goto_definition(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
) -> Result<Vec<LspLocation>, String> {
    state
        .core
        .lsp_goto_definition_on_host(&host_id, &cwd, &path, line, col)
        .await
}

#[tauri::command]
async fn restart_lsp_server(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    server_id: String,
) -> Result<Vec<LspServerStatus>, String> {
    state
        .core
        .restart_lsp_server_on_host(&host_id, &cwd, &server_id)
        .await
}

#[tauri::command]
async fn get_usage_insights(
    state: State<'_, DesktopAppState>,
    host_id: String,
    days: Option<u32>,
) -> Result<UsageInsightsSnapshot, String> {
    state
        .core
        .get_usage_insights_on_host(&host_id, days.unwrap_or(7))
        .await
}

#[tauri::command]
async fn get_context_occupancy(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<ContextOccupancyBreakdown, String> {
    state
        .core
        .get_context_occupancy_on_host(&host_id, &agent_id)
        .await
}

#[tauri::command]
async fn list_lifecycle_hooks(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
) -> Result<LifecycleHooksSnapshotView, String> {
    state.core.list_lifecycle_hooks_on_host(&host_id, cwd).await
}

#[tauri::command]
async fn upsert_lifecycle_hook(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
    hook: LifecycleHookRule,
) -> Result<LifecycleHooksSnapshotView, String> {
    state
        .core
        .upsert_lifecycle_hook_on_host(&host_id, cwd, hook)
        .await
}

#[tauri::command]
async fn delete_lifecycle_hook(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
    hook_id: String,
    scope: HookScope,
) -> Result<LifecycleHooksSnapshotView, String> {
    state
        .core
        .delete_lifecycle_hook_on_host(&host_id, cwd, hook_id, scope)
        .await
}

#[tauri::command]
async fn set_project_hooks_trust(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    trusted: bool,
) -> Result<LifecycleHooksSnapshotView, String> {
    state
        .core
        .set_project_hooks_trust_on_host(&host_id, cwd, trusted)
        .await
}

#[tauri::command]
async fn test_lifecycle_hook(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: Option<String>,
    hook_id: String,
    sample_payload: serde_json::Value,
) -> Result<HookExecutionRecord, String> {
    state
        .core
        .test_lifecycle_hook_on_host(&host_id, cwd, hook_id, sample_payload)
        .await
}

#[tauri::command]
async fn list_event_triggers(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<EventTriggersSnapshotView, String> {
    state.core.list_event_triggers_on_host(&host_id).await
}

#[tauri::command]
async fn upsert_event_trigger(
    state: State<'_, DesktopAppState>,
    req: UpsertEventTriggerRequest,
) -> Result<EventTriggersSnapshotView, String> {
    state.core.upsert_event_trigger_on_host(req).await
}

#[tauri::command]
async fn delete_event_trigger(
    state: State<'_, DesktopAppState>,
    host_id: String,
    trigger_id: String,
) -> Result<EventTriggersSnapshotView, String> {
    state
        .core
        .delete_event_trigger_on_host(&host_id, trigger_id)
        .await
}

#[tauri::command]
async fn test_event_trigger(
    state: State<'_, DesktopAppState>,
    host_id: String,
    trigger_id: String,
    sample_event_type: String,
    sample_payload: serde_json::Value,
) -> Result<EventTriggerDelivery, String> {
    state
        .core
        .test_event_trigger_on_host(&host_id, trigger_id, sample_event_type, sample_payload)
        .await
}

#[tauri::command]
async fn get_guardian_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<GuardianConfigSnapshotView, String> {
    state.core.get_guardian_config_on_host(&host_id).await
}

#[tauri::command]
async fn update_guardian_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    config: GuardianPolicyConfig,
) -> Result<GuardianConfigSnapshotView, String> {
    state
        .core
        .update_guardian_config_on_host(&host_id, config)
        .await
}

#[tauri::command]
async fn list_im_channels(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<ImChannelsSnapshotView, String> {
    state.core.list_im_channels_on_host(&host_id).await
}

#[tauri::command]
async fn upsert_im_channel(
    state: State<'_, DesktopAppState>,
    host_id: String,
    request: UpsertImChannelRequest,
) -> Result<ImChannelsSnapshotView, String> {
    state
        .core
        .upsert_im_channel_on_host(&host_id, request)
        .await
}

#[tauri::command]
async fn delete_im_channel(
    state: State<'_, DesktopAppState>,
    host_id: String,
    channel_id: String,
) -> Result<ImChannelsSnapshotView, String> {
    state
        .core
        .delete_im_channel_on_host(&host_id, channel_id)
        .await
}

#[tauri::command]
async fn test_im_channel(
    state: State<'_, DesktopAppState>,
    host_id: String,
    channel_id: String,
    target_chat_id: String,
    message: String,
) -> Result<ImChannelTestResultView, String> {
    state
        .core
        .test_im_channel_on_host(&host_id, channel_id, target_chat_id, message)
        .await
}

#[tauri::command]
fn focus_main_window(app: AppHandle) -> Result<(), String> {
    tray::focus_main_window_impl(&app)
}

#[tauri::command]
fn update_tray_status(
    app: AppHandle,
    tray_state: State<'_, tray::TrayMenuState>,
    running_count: u32,
    pending_approvals: u32,
) -> Result<(), String> {
    tray::update_tray_status_impl(&app, &tray_state, running_count, pending_approvals)
}

#[tauri::command]
async fn probe_local_provider_readiness() -> Result<Vec<ProviderReadinessInfo>, String> {
    tauri::async_runtime::spawn_blocking(run_local_provider_readiness_probe)
        .await
        .map_err(|e| format!("provider readiness probe task error: {e}"))
}

#[tauri::command]
fn render_qr_svg(content: String) -> Result<String, String> {
    qr::render_qr_svg_string(&content)
}

#[tauri::command]
async fn list_push_devices(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<PushDevicesSnapshotView, String> {
    state.core.list_push_devices_on_host(&host_id).await
}

#[tauri::command]
async fn register_push_device(
    state: State<'_, DesktopAppState>,
    host_id: String,
    request: RegisterPushDeviceRequest,
) -> Result<PushDevicesSnapshotView, String> {
    state
        .core
        .register_push_device_on_host(&host_id, request)
        .await
}

#[tauri::command]
async fn unregister_push_device(
    state: State<'_, DesktopAppState>,
    host_id: String,
    device_id: String,
) -> Result<PushDevicesSnapshotView, String> {
    state
        .core
        .unregister_push_device_on_host(&host_id, device_id)
        .await
}

#[tauri::command]
async fn update_push_preferences(
    state: State<'_, DesktopAppState>,
    host_id: String,
    request: UpdatePushPreferencesRequest,
) -> Result<PushDevicesSnapshotView, String> {
    state
        .core
        .update_push_preferences_on_host(&host_id, request)
        .await
}

#[tauri::command]
async fn test_push_notification(
    state: State<'_, DesktopAppState>,
    host_id: String,
    device_id: Option<String>,
) -> Result<PushTestResultView, String> {
    state
        .core
        .test_push_notification_on_host(&host_id, device_id)
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_goal_run(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    objective: String,
    criteria: Option<Vec<String>>,
    verify_command: Option<String>,
    max_iterations: Option<u32>,
    max_duration_secs: Option<u64>,
) -> Result<orbit_protocol::GoalRunRecord, String> {
    state
        .core
        .start_goal_run_on_host(orbit_client_core::contract::StartGoalRunRequest {
            host_id,
            agent_id,
            objective,
            criteria: criteria.unwrap_or_default(),
            verify_command,
            max_iterations,
            max_duration_secs,
        })
        .await
}

#[tauri::command]
async fn list_goal_runs(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
) -> Result<Vec<orbit_protocol::GoalRunRecord>, String> {
    state.core.list_goal_runs_on_host(&host_id, agent_id).await
}

#[tauri::command]
async fn control_goal_run(
    state: State<'_, DesktopAppState>,
    host_id: String,
    goal_id: String,
    action: orbit_protocol::GoalControlAction,
) -> Result<orbit_protocol::GoalRunRecord, String> {
    state
        .core
        .control_goal_run_on_host(orbit_client_core::contract::ControlGoalRunRequest {
            host_id,
            goal_id,
            action,
        })
        .await
}

#[tauri::command]
async fn start_monitor_watch(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    name: String,
    kind: orbit_protocol::MonitorKind,
    expires_in_secs: Option<u64>,
) -> Result<orbit_protocol::MonitorWatchRecord, String> {
    state
        .core
        .start_monitor_watch_on_host(orbit_client_core::contract::StartMonitorWatchRequest {
            host_id,
            agent_id,
            name,
            kind,
            expires_in_secs,
        })
        .await
}

#[tauri::command]
async fn list_monitor_watches(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
) -> Result<Vec<orbit_protocol::MonitorWatchRecord>, String> {
    state
        .core
        .list_monitor_watches_on_host(&host_id, agent_id)
        .await
}

#[tauri::command]
async fn stop_monitor_watch(
    state: State<'_, DesktopAppState>,
    host_id: String,
    monitor_id: String,
) -> Result<orbit_protocol::MonitorWatchRecord, String> {
    state
        .core
        .stop_monitor_watch_on_host(orbit_client_core::contract::StopMonitorWatchRequest {
            host_id,
            monitor_id,
        })
        .await
}

#[tauri::command]
async fn list_browser_sessions(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<orbit_client_core::contract::BrowserSessionsSnapshotView, String> {
    state.core.list_browser_sessions_on_host(&host_id).await
}

#[tauri::command]
async fn create_browser_session(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
    initial_url: Option<String>,
    headless: Option<bool>,
    viewport_width: Option<u32>,
    viewport_height: Option<u32>,
) -> Result<orbit_protocol::BrowserSessionSummary, String> {
    state
        .core
        .create_browser_session_on_host(orbit_client_core::contract::CreateBrowserSessionRequest {
            host_id,
            agent_id,
            initial_url,
            headless,
            viewport_width,
            viewport_height,
        })
        .await
}

#[tauri::command]
async fn execute_browser_action(
    state: State<'_, DesktopAppState>,
    host_id: String,
    session_id: String,
    action: orbit_protocol::BrowserActionRequest,
) -> Result<orbit_client_core::contract::BrowserActionResponseView, String> {
    state
        .core
        .execute_browser_action_on_host(orbit_client_core::contract::ExecuteBrowserActionRequest {
            host_id,
            session_id,
            action,
        })
        .await
}

#[tauri::command]
async fn close_browser_session(
    state: State<'_, DesktopAppState>,
    host_id: String,
    session_id: String,
) -> Result<orbit_client_core::contract::BrowserSessionsSnapshotView, String> {
    state
        .core
        .close_browser_session_on_host(&host_id, &session_id)
        .await
}

#[tauri::command]
async fn update_browser_policy(
    state: State<'_, DesktopAppState>,
    host_id: String,
    policy: orbit_protocol::BrowserPolicyConfig,
) -> Result<orbit_client_core::contract::BrowserSessionsSnapshotView, String> {
    state
        .core
        .update_browser_policy_on_host(&host_id, policy)
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_batch_fanout(
    state: State<'_, DesktopAppState>,
    host_id: String,
    name: String,
    cwd: String,
    csv_content: String,
    prompt_template: String,
    provider: orbit_protocol::ProviderKind,
    model: Option<String>,
    mode: orbit_protocol::SessionMode,
    max_concurrency: Option<u32>,
    isolated_worktree: Option<bool>,
) -> Result<orbit_protocol::WorkflowRunRecord, String> {
    state
        .core
        .start_batch_fanout_on_host(orbit_client_core::contract::StartBatchFanoutRequest {
            host_id,
            name,
            cwd,
            csv_content,
            prompt_template,
            provider,
            model,
            mode,
            max_concurrency,
            isolated_worktree: isolated_worktree.unwrap_or(false),
        })
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_dag_workflow(
    state: State<'_, DesktopAppState>,
    host_id: String,
    name: String,
    cwd: String,
    steps: Vec<orbit_protocol::DagStepSpec>,
    default_provider: orbit_protocol::ProviderKind,
    default_mode: orbit_protocol::SessionMode,
    max_concurrency: Option<u32>,
) -> Result<orbit_protocol::WorkflowRunRecord, String> {
    state
        .core
        .start_dag_workflow_on_host(orbit_client_core::contract::StartDagWorkflowRequest {
            host_id,
            name,
            cwd,
            steps,
            default_provider,
            default_mode,
            max_concurrency,
        })
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn start_best_of_n(
    state: State<'_, DesktopAppState>,
    host_id: String,
    name: String,
    cwd: String,
    prompt: String,
    candidates: Vec<orbit_protocol::BestOfNCandidateSpec>,
    mode: orbit_protocol::SessionMode,
    verify_command: Option<String>,
) -> Result<orbit_protocol::WorkflowRunRecord, String> {
    state
        .core
        .start_best_of_n_on_host(orbit_client_core::contract::StartBestOfNRequest {
            host_id,
            name,
            cwd,
            prompt,
            candidates,
            mode,
            verify_command,
        })
        .await
}

#[tauri::command]
async fn list_workflow_runs(
    state: State<'_, DesktopAppState>,
    host_id: String,
    include_completed: Option<bool>,
) -> Result<Vec<orbit_protocol::WorkflowRunRecord>, String> {
    state
        .core
        .list_workflow_runs_on_host(&host_id, include_completed.unwrap_or(true))
        .await
}

#[tauri::command]
async fn cancel_workflow_run(
    state: State<'_, DesktopAppState>,
    host_id: String,
    run_id: String,
) -> Result<orbit_protocol::WorkflowRunRecord, String> {
    state
        .core
        .cancel_workflow_run_on_host(orbit_client_core::contract::CancelWorkflowRunRequest {
            host_id,
            run_id,
        })
        .await
}

#[tauri::command]
async fn select_best_of_n_winner(
    state: State<'_, DesktopAppState>,
    host_id: String,
    run_id: String,
    winner_step_id: String,
    apply_to_workspace: Option<bool>,
) -> Result<orbit_protocol::WorkflowRunRecord, String> {
    state
        .core
        .select_best_of_n_winner_on_host(orbit_client_core::contract::SelectBestOfNWinnerRequest {
            host_id,
            run_id,
            winner_step_id,
            apply_to_workspace: apply_to_workspace.unwrap_or(true),
        })
        .await
}

#[tauri::command]
async fn highlight_workspace_code(
    language: String,
    content: String,
    max_lines: Option<usize>,
) -> Result<Vec<HighlightedLineView>, String> {
    let cap = max_lines.unwrap_or(2500).clamp(1, 5000);
    tauri::async_runtime::spawn_blocking(move || {
        highlight_code_lines_capped(&language, &content, cap)
    })
    .await
    .map_err(|e| format!("code highlight task error: {e}"))
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
async fn parse_markdown_preview(
    markdown: String,
    streaming: bool,
) -> Result<Vec<RichBlockView>, String> {
    tauri::async_runtime::spawn_blocking(move || parse_markdown_blocks(&markdown, streaming))
        .await
        .map_err(|e| format!("markdown preview task error: {e}"))
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

#[tauri::command]
fn open_local_file_externally(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
) -> Result<(), String> {
    if !state.core.is_local_host(&host_id) {
        return Err("Only files on the local host can be opened externally".into());
    }
    let authorized_cwd = state
        .core
        .is_registered_local_cwd(&host_id, &cwd)
        .then_some(cwd.as_str());
    open_local_file_os(authorized_cwd, &path)
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

#[derive(Debug, Clone, serde::Serialize)]
struct ClipboardImagePayload {
    filename: String,
    mime_type: String,
    data_b64: String,
}

#[cfg(target_os = "linux")]
const MAX_CLIPBOARD_IMAGE_BYTES: usize = 25 * 1024 * 1024;
#[cfg(target_os = "linux")]
const MAX_CLIPBOARD_URI_IMAGES: usize = 10;

#[cfg(target_os = "linux")]
fn image_mime_for_extension(ext_lower: &str) -> Option<&'static str> {
    match ext_lower {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn read_linux_gtk_clipboard_images() -> Vec<ClipboardImagePayload> {
    use base64::Engine as _;

    let clipboard = gtk::Clipboard::get(&gdk::SELECTION_CLIPBOARD);
    let Some(targets) = clipboard.wait_for_targets() else {
        return Vec::new();
    };
    let target_names: Vec<String> = targets.iter().map(|a| a.name().to_string()).collect();

    // 1. Prefer raw encoded image MIME targets directly so PNG/JPEG/WebP/GIF bytes are preserved verbatim.
    const PREFERRED_MIMES: [(&str, &str); 5] = [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
        ("image/bmp", "bmp"),
    ];
    for (mime, ext) in PREFERRED_MIMES {
        if target_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(mime))
        {
            let atom = gdk::Atom::intern(mime);
            if let Some(sel) = clipboard.wait_for_contents(&atom) {
                let bytes = sel.data();
                if !bytes.is_empty() && bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES {
                    return vec![ClipboardImagePayload {
                        filename: format!("clipboard-image.{ext}"),
                        mime_type: mime.to_string(),
                        data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
                    }];
                }
            }
        }
    }

    // 2. Fallback to GdkPixbuf conversion (e.g., TIFF / X11 pixmap / BMP variants) -> PNG.
    let has_any_image_target = target_names
        .iter()
        .any(|name| name.to_ascii_lowercase().starts_with("image/"));
    if has_any_image_target
        && let Some(pixbuf) = clipboard.wait_for_image()
        && let Ok(bytes) = pixbuf.save_to_bufferv("png", &[])
        && !bytes.is_empty()
        && bytes.len() <= MAX_CLIPBOARD_IMAGE_BYTES
    {
        return vec![ClipboardImagePayload {
            filename: "clipboard-image.png".to_string(),
            mime_type: "image/png".to_string(),
            data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
        }];
    }

    // 3. Support image files copied from Linux file managers (`text/uri-list` / `x-special/gnome-copied-files`).
    let has_uri_target = target_names.iter().any(|name| {
        name.eq_ignore_ascii_case("text/uri-list")
            || name.eq_ignore_ascii_case("x-special/gnome-copied-files")
    });
    if has_uri_target {
        let mut out = Vec::new();
        for uri in clipboard.wait_for_uris() {
            if out.len() >= MAX_CLIPBOARD_URI_IMAGES {
                break;
            }
            let raw_uri = uri.trim();
            let Some(encoded_path) = raw_uri.strip_prefix("file://") else {
                continue;
            };
            // Strip optional `localhost` authority (`file://localhost/path`)
            let encoded_path = encoded_path
                .strip_prefix("localhost/")
                .map(|rest| format!("/{rest}"))
                .unwrap_or_else(|| encoded_path.to_string());
            let decoded_path = decode_percent_utf8(&encoded_path);
            let path = std::path::Path::new(&decoded_path);
            let Ok(meta) = fs::metadata(path) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_CLIPBOARD_IMAGE_BYTES as u64 {
                continue;
            }
            let Some(ext) = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
            else {
                continue;
            };
            let Some(mime) = image_mime_for_extension(&ext) else {
                continue;
            };
            let Ok(bytes) = fs::read(path) else {
                continue;
            };
            if bytes.is_empty() || bytes.len() > MAX_CLIPBOARD_IMAGE_BYTES {
                continue;
            }
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| !n.trim().is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("clipboard-image.{ext}"));
            out.push(ClipboardImagePayload {
                filename,
                mime_type: mime.to_string(),
                data_b64: base64::engine::general_purpose::STANDARD.encode(bytes),
            });
        }
        return out;
    }

    Vec::new()
}

/// Reads images from the native Linux GTK clipboard, bypassing WebKitGTK's `DataTransfer::allowsFileAccess()`
/// restriction (CVE-2025-13947 regression) that leaves `e.clipboardData.items` empty for pasted images.
#[tauri::command]
async fn read_clipboard_images(app: AppHandle) -> Result<Vec<ClipboardImagePayload>, String> {
    #[cfg(target_os = "linux")]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        app.run_on_main_thread(move || {
            let images = read_linux_gtk_clipboard_images();
            let _ = tx.send(images);
        })
        .map_err(|e| format!("Failed to dispatch GTK clipboard read: {e}"))?;

        match tokio::time::timeout(std::time::Duration::from_secs(3), rx).await {
            Ok(Ok(images)) => Ok(images),
            Ok(Err(_)) => Err("Clipboard reader channel closed".into()),
            Err(_) => Err("Timed out reading Linux clipboard".into()),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = app;
        Ok(Vec::new())
    }
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
    target_tag: Option<String>,
    on_progress: Channel<AppUpdateProgress>,
) -> Result<(), String> {
    perform_one_click_update(target_tag, on_progress).await
}

#[tauri::command]
async fn sync_local_daemon(
    state: State<'_, DesktopAppState>,
    host_id: Option<String>,
    force: Option<bool>,
) -> Result<LocalDaemonSyncResult, String> {
    let result = run_sync_local_daemon(force.unwrap_or(false)).await?;
    if result.status != "skipped_dev"
        && let Some(hid) = host_id
    {
        state.core.connect_host_async(hid.clone());
        state.core.refresh_host(&hid);
    }
    Ok(result)
}

#[tauri::command]
async fn upgrade_host_daemon_over_wire(
    state: State<'_, DesktopAppState>,
    host_id: String,
    target_version: Option<String>,
) -> Result<String, String> {
    state
        .core
        .upgrade_host_daemon_over_wire(&host_id, target_version)
        .await
}

// --- Zero-Server E2EE Cloud Sync Commands ---

#[tauri::command]
fn get_cloud_sync_status(state: State<'_, DesktopAppState>) -> CloudSyncStatusView {
    state.core.cloud_sync_status()
}

#[tauri::command]
fn cloud_sync_generate_passphrase(state: State<'_, DesktopAppState>) -> String {
    state.core.cloud_sync_generate_passphrase()
}

#[tauri::command]
fn cloud_sync_set_github_client_id(
    state: State<'_, DesktopAppState>,
    client_id: Option<String>,
) -> Result<CloudSyncStatusView, String> {
    state
        .core
        .cloud_sync_set_github_client_id(client_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cloud_sync_set_auto_sync(
    state: State<'_, DesktopAppState>,
    enabled: bool,
) -> Result<CloudSyncStatusView, String> {
    state
        .core
        .cloud_sync_set_auto_sync(enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn cloud_sync_start_github_device_flow(
    state: State<'_, DesktopAppState>,
    client_id_override: Option<String>,
) -> Result<GitHubDeviceFlowStartView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_start_github_device_flow(client_id_override)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync start task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_poll_github_device_flow(
    state: State<'_, DesktopAppState>,
    device_code: String,
    client_id_override: Option<String>,
) -> Result<CloudSyncDevicePollView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_poll_github_device_flow(&device_code, client_id_override)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync poll task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_login_with_token(
    state: State<'_, DesktopAppState>,
    token: String,
) -> Result<CloudSyncStatusView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_login_with_token(&token)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync token login task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_unlock_with_passphrase(
    state: State<'_, DesktopAppState>,
    passphrase: String,
) -> Result<CloudSyncResultView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_unlock_with_passphrase(&passphrase)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync unlock task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_now(state: State<'_, DesktopAppState>) -> Result<CloudSyncResultView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.cloud_sync_now().map_err(|e| e.to_string()))
        .await
        .map_err(|e| format!("cloud sync task error: {e}"))?
}

#[tauri::command]
fn cloud_sync_trigger_background(state: State<'_, DesktopAppState>) {
    state.core.schedule_cloud_auto_sync(false);
}

#[tauri::command]
fn cloud_sync_logout(
    state: State<'_, DesktopAppState>,
    clear_local_hosts: bool,
) -> Result<CloudSyncStatusView, String> {
    state
        .core
        .cloud_sync_logout(clear_local_hosts)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn cloud_sync_get_preferences(
    state: State<'_, DesktopAppState>,
) -> HashMap<String, serde_json::Value> {
    state.core.cloud_sync_get_preferences()
}

#[tauri::command]
fn cloud_sync_update_preferences(
    state: State<'_, DesktopAppState>,
    patch: HashMap<String, serde_json::Value>,
) -> Result<(), String> {
    state
        .core
        .cloud_sync_update_preferences(patch)
        .map_err(|e| e.to_string())
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
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().clamp(2, 4))
        .unwrap_or(4);
    let _tauri_rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .max_blocking_threads(16)
        .thread_name("orbit-tauri-rt")
        .enable_all()
        .build()
        .expect("failed to initialize Orbit Desktop Tokio runtime");
    tauri::async_runtime::set(_tauri_rt.handle().clone());

    let db_path = default_desktop_db_path();
    tracing::info!(
        version = CURRENT_APP_VERSION,
        db_path = %db_path.display(),
        "Starting Orbit Desktop"
    );
    let core = OrbitAppCore::open(&db_path, true)
        .or_else(|err| {
            tracing::error!(error = %err, "Failed to open desktop replica SQLite; falling back to in-memory store");
            OrbitAppCore::open_in_memory_with_discovery(true)
        })
        .expect("failed to initialize OrbitAppCore");
    core.connect_all_hosts();

    tauri::Builder::default()
        .manage(DesktopAppState { core })
        .manage(tray::TrayMenuState::default())
        .setup(|app| {
            tray::setup_system_tray(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            subscribe_core_events,
            get_bridge_info,
            get_fleet_overview,
            search_messages,
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
            reorder_projects,
            get_project_order,
            inspect_git_repos,
            get_git_repos,
            list_directory,
            connect_host,
            connect_all_hosts,
            refresh_host,
            refresh_quotas,
            refresh_provider_models,
            create_agent,
            open_agent_timeline,
            send_prompt,
            enqueue_prompt,
            remove_queued_prompt,
            flush_queued_prompt_now,
            reorder_queued_prompts,
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
            fetch_workspace_board,
            get_workspace_board,
            update_workspace_board,
            fetch_workspace_events,
            get_workspace_events,
            update_workspace_goal,
            move_agent_to_workspace,
            move_agent_to_workspace_v2,
            organize_project_workspaces,
            summarize_agent,
            update_agent_identity,
            merge_workspaces,
            dismiss_cluster_suggestion,
            spawn_workspace_peer,
            create_workspace_v2,
            create_mission_workspace,
            rename_workspace,
            archive_workspace,
            archive_workspace_v2,
            unarchive_workspace,
            list_workspace_scripts,
            start_workspace_script,
            stop_workspace_script,
            upsert_profile,
            delete_profile,
            list_artifacts,
            get_artifact,
            upsert_artifact,
            delete_artifact,
            list_acp_agents,
            refresh_acp_agents,
            upsert_custom_acp_agent,
            delete_custom_acp_agent,
            test_acp_agent,
            refresh_acp_registry,
            list_external_mcp_servers,
            upsert_external_mcp_server,
            toggle_external_mcp_server,
            delete_external_mcp_server,
            probe_external_mcp_server,
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
            export_host_pairing_uri,
            update_orchestration_config,
            sync_orchestration_skills,
            list_managed_skills,
            read_managed_skill,
            upsert_managed_skill,
            toggle_managed_skill,
            delete_managed_skill,
            import_managed_skill,
            list_provider_subagents,
            open_provider_subagent_timeline,
            get_provider_subagent_timeline,
            get_provider_subagent_turns,
            refresh_pending_permissions,
            rewind_agent,
            fork_agent,
            preview_worktree_handoff,
            execute_worktree_handoff,
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
            export_agent_transcript,
            save_exported_transcript,
            list_agent_code_blocks,
            get_workspace_files,
            search_workspace_symbols,
            read_workspace_file,
            write_workspace_file,
            create_workspace_entry,
            rename_workspace_entry,
            delete_workspace_entry,
            list_instruction_files,
            read_instruction_file,
            save_instruction_file,
            list_path_rules,
            upsert_path_rule,
            delete_path_rule,
            match_path_rules,
            list_lsp_servers,
            lsp_query_symbols,
            lsp_get_diagnostics,
            lsp_goto_definition,
            restart_lsp_server,
            get_usage_insights,
            get_context_occupancy,
            list_lifecycle_hooks,
            upsert_lifecycle_hook,
            delete_lifecycle_hook,
            set_project_hooks_trust,
            test_lifecycle_hook,
            create_conditional_timer,
            list_conditional_timers,
            cancel_conditional_timer,
            fire_conditional_timer_now,
            get_guardian_config,
            update_guardian_config,
            list_im_channels,
            upsert_im_channel,
            delete_im_channel,
            test_im_channel,
            focus_main_window,
            update_tray_status,
            probe_local_provider_readiness,
            render_qr_svg,
            list_push_devices,
            register_push_device,
            unregister_push_device,
            update_push_preferences,
            test_push_notification,
            start_goal_run,
            list_goal_runs,
            control_goal_run,
            start_monitor_watch,
            list_monitor_watches,
            stop_monitor_watch,
            list_event_triggers,
            upsert_event_trigger,
            delete_event_trigger,
            test_event_trigger,
            list_browser_sessions,
            create_browser_session,
            execute_browser_action,
            close_browser_session,
            update_browser_policy,
            start_batch_fanout,
            start_dag_workflow,
            start_best_of_n,
            list_workflow_runs,
            cancel_workflow_run,
            select_best_of_n_winner,
            highlight_workspace_code,
            ask_side_question,
            render_diagram,
            parse_markdown_preview,
            open_browser_popout_window,
            open_external_browser_url,
            open_local_file_externally,
            stage_host_attachment,
            stage_host_attachment_bytes,
            read_clipboard_images,
            provision_ssh_relay,
            get_app_version,
            check_app_update,
            perform_app_self_update,
            sync_local_daemon,
            upgrade_host_daemon_over_wire,
            get_cloud_sync_status,
            cloud_sync_generate_passphrase,
            cloud_sync_set_github_client_id,
            cloud_sync_set_auto_sync,
            cloud_sync_start_github_device_flow,
            cloud_sync_poll_github_device_flow,
            cloud_sync_login_with_token,
            cloud_sync_unlock_with_passphrase,
            cloud_sync_now,
            cloud_sync_trigger_background,
            cloud_sync_logout,
            cloud_sync_get_preferences,
            cloud_sync_update_preferences,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit Tauri desktop application");
}
