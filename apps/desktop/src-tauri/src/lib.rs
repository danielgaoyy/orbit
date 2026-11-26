mod app_menu;
mod notifications;
mod qr;
mod tray;
mod updater;
mod window_state;
mod zoom;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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
    check_latest_release, ensure_bundled_sidecar_installed, perform_one_click_update,
    sync_local_daemon as run_sync_local_daemon,
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
    AcpRegistryRefreshView, AuditChainExportView, BridgeInfoView, ConversationSearchHit,
    CreateWorkspaceEntryRequest, DeletePathRuleRequest, DeleteWorkspaceEntryRequest,
    EventTriggersSnapshotView, ExecuteWorktreeHandoffRequest, FleetSidebarRequest,
    ForkAgentRequest, ForkAgentResultView, GitReposInspectionView, InstructionFileContentView,
    InstructionFileSavedView, InstructionFilesSnapshotView, ListInstructionFilesRequest,
    MatchPathRulesRequest, MatchedPathRulesView, MentionRankRequest, ModelSelectionRequest,
    PathRulesSnapshotView, PreviewWorktreeHandoffRequest, ReadInstructionFileRequest,
    RenameWorkspaceEntryRequest, RenderDiagramRequest, RenderDiagramView, RewindPreviewView,
    SaveInstructionFileRequest, SearchMessagesRequest, SearchMessagesResponse,
    SearchWorkspaceSymbolsRequest, SpawnWorkspacePeerRequest, UpsertArtifactRequest,
    UpsertEventTriggerRequest, UpsertExternalMcpServerRequest, UpsertPathRuleRequest,
    UpsertProjectMemoryRequest, WorkspaceEntryMutationResultView, WorkspaceFileWriteResultView,
    WorkspaceSymbolsView, WriteWorkspaceFileRequest,
};
use orbit_client_core::desktop_ops::{
    ExternalEditorInfo, ExternalEditorKind, LocalMachineInfo, SshHostItem, StagedAttachmentInfo,
    detect_installed_editors, discover_user_ssh_hosts, inspect_local_machine_info,
    list_local_workspace_files, open_external_url, open_in_external_editor,
    open_local_file_externally as open_local_file_os, pick_local_directory, pick_local_save_file,
    provision_relay_over_ssh, resolve_user_home, upgrade_remote_daemon_over_ssh,
};
use orbit_client_core::export::{
    ExportOptions, ExportedTranscript, ExtractedCodeBlock, write_file_atomically,
};
use orbit_client_core::personal_agent::PersonalAgentBriefingView;
use orbit_client_core::render::{
    HighlightedLineView, RichBlockView, highlight_code_lines_capped, parse_markdown_blocks,
};
use orbit_client_core::sidebar::FleetSidebarView;
use orbit_client_core::stage::{AgentStageDeltaView, AgentStageView, StageTurnView};
use orbit_protocol::{
    AcpAgentAvailability, AcpAgentSpec, AcpProbeResult, ActiveTurnBehavior, AgentProfile,
    ArtifactDetail, ArtifactSummary, AuditChainVerification, ConditionalTimerRecord,
    ContextOccupancyBreakdown, EventTriggerDelivery, ExternalMcpServerRecord, FimConfig,
    GitCommitResult, GitConflictListResult, GitConflictResolution, GitCreatePrResult,
    GitGeneratedCommitMessage, GitMutationOutcome, GitPrStatus, GitPushResult,
    GitRebaseActionResult, GitRebaseStatus, GitResolveConflictResult, GitStashListResult,
    GitStashMutationResult, GuardianPolicyConfig, HookExecutionRecord, HookScope, InlineEditInput,
    InlineEditResult, InstalledPluginView, LifecycleHookRule, LspCodeAction, LspCompletionItem,
    LspDiagnostic, LspHoverInfo, LspLocation, LspReferenceLocation, LspServerStatus, LspSymbolInfo,
    LspWorkspaceEdit, LspWorkspaceEditResult, ManagedSkillRecord, McpProbeResult,
    McpResourceContent, McpToolCallOutcome, MemoryCategory, MemoryStatus, MigrationConflict,
    OrbitPluginUiSlot, OrbitPluginUiSpec, OrchestrationConfig, PermissionRuleRecord,
    PrPatrolReport, ProjectMemoryItem, ProjectMemorySearchHit, PromptBadge, ProviderKind,
    ProviderModelCatalog, ProviderSubagentEvent, ReasoningEffort, ResolvePermissionsBatchResult,
    SandboxPolicy, ScheduleRunRecord, ScheduleTarget, SessionMode, SkillWriteScope, TimerCondition,
    UpsertPermissionRuleInput, UsageInsightsSnapshot, VoiceApprovalSafety, VoiceConfig,
    VoiceConfigState, VoiceSpeechResult, VoiceTranscriptionResult, WorkspaceBoardSnapshot,
    WorkspaceEventRecord, WorkspaceIsolation, WorktreeCreateMode, WorktreeHandoffOutcome,
    WorktreeHandoffPreview,
};
use orbit_providers::{
    ProviderReadinessInfo, SideQuestionSpec,
    probe_local_provider_readiness as run_local_provider_readiness_probe, run_side_question,
};
use tauri::{
    AppHandle, Emitter, Manager, State, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    ipc::{Channel, InvokeResponseBody},
};

struct DesktopAppState {
    core: Arc<OrbitAppCore>,
    window_terminal_subs: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl DesktopAppState {
    fn new(core: Arc<OrbitAppCore>) -> Self {
        Self {
            core,
            window_terminal_subs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn register_window_terminal_subscription(&self, window_label: &str, subscription_id: &str) {
        if let Ok(mut map) = self.window_terminal_subs.lock() {
            let entry = map.entry(window_label.to_string()).or_default();
            if !entry.iter().any(|id| id == subscription_id) {
                entry.push(subscription_id.to_string());
            }
        }
    }

    fn unregister_window_terminal_subscription(&self, subscription_id: &str) {
        remove_window_terminal_subscription(&self.window_terminal_subs, subscription_id);
    }

    fn cleanup_window_terminal_subscriptions(&self, window_label: &str) -> usize {
        let sub_ids = self
            .window_terminal_subs
            .lock()
            .ok()
            .and_then(|mut map| map.remove(window_label))
            .unwrap_or_default();
        let count = sub_ids.len();
        for sub_id in sub_ids {
            self.core.unsubscribe_terminal_binary(&sub_id);
        }
        count
    }
}

fn remove_window_terminal_subscription(
    subs_by_window: &Mutex<HashMap<String, Vec<String>>>,
    subscription_id: &str,
) {
    if let Ok(mut map) = subs_by_window.lock() {
        map.retain(|_, ids| {
            ids.retain(|id| id != subscription_id);
            !ids.is_empty()
        });
    }
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
            // PTY output streams over the dedicated raw binary IPC channel (`subscribe_terminal_binary`)
            // so high-throughput terminal builds never churn JSON arrays or the main store.
            include_terminal_raw_bytes: false,
        },
        Arc::new(TauriChannelSubscriber { channel: on_event }),
    );
}

#[tauri::command]
fn subscribe_terminal_binary(
    window: WebviewWindow,
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
    slot: Option<u8>,
    on_chunk: Channel<InvokeResponseBody>,
) -> Result<String, String> {
    let trimmed_host = host_id.trim();
    if trimmed_host.is_empty() {
        return Err("host_id is required for binary terminal subscription".into());
    }
    let trimmed_tid = terminal_id.trim();
    if trimmed_tid.is_empty() && slot.is_none() {
        return Err("terminal_id or slot is required for binary terminal subscription".into());
    }
    if !trimmed_tid.is_empty() {
        let _ = state
            .core
            .ensure_terminal_attached(trimmed_host, trimmed_tid);
    }

    let mut rx = state
        .core
        .subscribe_terminal_binary_with_slot(trimmed_host, trimmed_tid, slot);
    let subscription_id = rx.subscription_id().to_string();
    state.register_window_terminal_subscription(window.label(), &subscription_id);

    let core = Arc::clone(&state.core);
    let window_subs = Arc::clone(&state.window_terminal_subs);
    let sub_id_for_task = subscription_id.clone();

    tauri::async_runtime::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            if chunk.is_empty() {
                continue;
            }
            if on_chunk.send(InvokeResponseBody::Raw(chunk)).is_err() {
                break;
            }
        }
        remove_window_terminal_subscription(&window_subs, &sub_id_for_task);
        core.unsubscribe_terminal_binary(&sub_id_for_task);
    });

    Ok(subscription_id)
}

#[tauri::command]
fn unsubscribe_terminal_binary(
    state: State<'_, DesktopAppState>,
    subscription_id: String,
) -> Result<(), String> {
    let trimmed = subscription_id.trim();
    if !trimmed.is_empty() {
        state.unregister_window_terminal_subscription(trimmed);
        state.core.unsubscribe_terminal_binary(trimmed);
    }
    Ok(())
}

#[tauri::command]
fn ensure_terminal_slot(
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
) -> Result<u8, String> {
    let trimmed_host = host_id.trim();
    let trimmed_tid = terminal_id.trim();
    if trimmed_host.is_empty() || trimmed_tid.is_empty() {
        return Err("host_id and terminal_id are required".into());
    }
    state
        .core
        .ensure_terminal_attached(trimmed_host, trimmed_tid)
        .ok_or_else(|| format!("host `{trimmed_host}` not found"))
}

#[tauri::command]
fn get_bridge_info(state: State<'_, DesktopAppState>) -> BridgeInfoView {
    state.core.bridge_info()
}

#[tauri::command]
async fn get_fleet_overview(
    state: State<'_, DesktopAppState>,
) -> Result<FleetOverviewView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.fleet_overview())
        .await
        .map_err(|e| format!("fleet overview task error: {e}"))
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
async fn search_messages_remote(
    state: State<'_, DesktopAppState>,
    query: String,
    host_id: Option<String>,
    agent_id: Option<String>,
    limit: Option<u32>,
) -> Result<SearchMessagesResponse, String> {
    let core = Arc::clone(&state.core);
    Ok(core
        .search_messages_remote(SearchMessagesRequest {
            query,
            host_id,
            agent_id,
            limit,
        })
        .await)
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
async fn get_host_detail(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<Option<HostDetailView>, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.host_detail(&host_id))
        .await
        .map_err(|e| format!("host detail task error: {e}"))
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
    tail_turn_limit: Option<usize>,
) -> Result<AgentStageDeltaView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.agent_stage_delta_with_options(
            &host_id,
            &agent_id,
            since_revision,
            since_aux_revision,
            tail_turn_limit,
        )
    })
    .await
    .map_err(|e| format!("agent stage delta task error: {e}"))
}

#[tauri::command]
async fn get_agent_stage_turns_slice(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    start_turn_index: usize,
    count: usize,
) -> Result<Vec<StageTurnView>, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.agent_stage_turns_slice(&host_id, &agent_id, start_turn_index, count)
    })
    .await
    .map_err(|e| format!("agent stage turns slice task error: {e}"))
}

#[tauri::command]
async fn load_older_agent_timeline(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    limit_events: Option<u32>,
) -> Result<bool, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.load_older_agent_timeline(&host_id, &agent_id, limit_events.unwrap_or(300))
    })
    .await
    .map_err(|e| format!("load older timeline task error: {e}"))
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
async fn mark_agent_seen(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    seq: u64,
) -> Result<(), String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.mark_agent_seen(&host_id, &agent_id, seq))
        .await
        .map_err(|e| format!("mark agent seen task error: {e}"))
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
async fn get_local_machine_info() -> Result<LocalMachineInfo, String> {
    tauri::async_runtime::spawn_blocking(inspect_local_machine_info)
        .await
        .map_err(|e| format!("inspect local machine task error: {e}"))
}

#[tauri::command]
async fn pair_local_host(state: State<'_, DesktopAppState>) -> Result<HostCardView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        let _ = ensure_bundled_sidecar_installed();
        core.pair_local_host_blocking().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("pair local host task error: {e}"))?
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
fn apply_relay_to_all_hosts(
    state: State<'_, DesktopAppState>,
    raw_relay_url: String,
) -> Result<u32, String> {
    state
        .core
        .apply_relay_to_fleet(&raw_relay_url)
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
async fn open_agent_timeline(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<(), String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.open_agent_timeline(&host_id, &agent_id))
        .await
        .map_err(|e| format!("open agent timeline task error: {e}"))
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
fn interrupt_agent_and_continue(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) {
    state.core.interrupt_agent_and_continue(&host_id, &agent_id);
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
fn harvest_conversation(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    summary_note: Option<String>,
    append_to_workspace_board: Option<bool>,
) {
    state.core.harvest_conversation(
        &host_id,
        &agent_id,
        summary_note,
        append_to_workspace_board.unwrap_or(false),
    );
}

#[tauri::command]
async fn mark_agent_read(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<(), String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || core.mark_agent_read(&host_id, &agent_id))
        .await
        .map_err(|e| format!("mark agent read task error: {e}"))
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
fn reset_workspace_board(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    new_goal: Option<String>,
    keep_notes: Option<String>,
) {
    state
        .core
        .reset_workspace_board(&host_id, &workspace_id, new_goal, keep_notes);
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
async fn archive_workspace(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
) -> Result<(), String> {
    state
        .core
        .archive_workspace_confirmed(&host_id, &workspace_id, false)
        .await
}

#[tauri::command]
async fn archive_workspace_v2(
    state: State<'_, DesktopAppState>,
    host_id: String,
    workspace_id: String,
    keep_agents_unclassified: bool,
) -> Result<(), String> {
    state
        .core
        .archive_workspace_confirmed(&host_id, &workspace_id, keep_agents_unclassified)
        .await
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
async fn read_mcp_resource(
    state: State<'_, DesktopAppState>,
    host_id: String,
    artifact_id: String,
    artifact_version: Option<u32>,
    uri: String,
) -> Result<McpResourceContent, String> {
    state
        .core
        .read_mcp_resource_async(&host_id, &artifact_id, artifact_version, &uri)
        .await
}

#[tauri::command]
async fn call_artifact_mcp_tool(
    state: State<'_, DesktopAppState>,
    host_id: String,
    artifact_id: String,
    artifact_version: Option<u32>,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<McpToolCallOutcome, String> {
    state
        .core
        .call_artifact_mcp_tool_async(
            &host_id,
            &artifact_id,
            artifact_version,
            &tool_name,
            arguments,
        )
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
    create_only: Option<bool>,
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
            create_only: create_only.unwrap_or(false),
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

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn import_managed_skill(
    state: State<'_, DesktopAppState>,
    host_id: String,
    scope: SkillWriteScope,
    cwd: Option<String>,
    source_markdown_or_path: String,
    slug_override: Option<String>,
    expected_etag: Option<String>,
    create_only: Option<bool>,
) -> Result<Vec<ManagedSkillRecord>, String> {
    state
        .core
        .import_managed_skill_async(ImportManagedSkillRequest {
            host_id,
            scope,
            cwd,
            source_markdown_or_path,
            slug_override,
            expected_etag,
            create_only: create_only.unwrap_or(false),
        })
        .await
}

#[tauri::command]
async fn list_provider_subagents(
    state: State<'_, DesktopAppState>,
    host_id: String,
    parent_agent_id: String,
) -> Result<(), String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.list_provider_subagents(&host_id, &parent_agent_id)
    })
    .await
    .map_err(|e| format!("list provider subagents task error: {e}"))
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
async fn preview_rewind_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    to_seq: u64,
) -> Result<RewindPreviewView, String> {
    state
        .core
        .preview_rewind_agent_on_host(&host_id, &agent_id, to_seq)
        .await
}

#[tauri::command]
async fn rewind_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    to_before_seq: u64,
    restore_files: bool,
    restore_paths: Option<Vec<String>>,
) -> Result<RewindAgentResultView, String> {
    state
        .core
        .rewind_agent_on_host(
            &host_id,
            &agent_id,
            to_before_seq,
            restore_files,
            restore_paths,
        )
        .await
}

#[tauri::command]
async fn send_native_approval_notification(
    app: AppHandle,
    state: State<'_, DesktopAppState>,
    req: notifications::NativeApprovalNotificationRequest,
) -> Result<notifications::NativeNotificationDispatchResult, String> {
    Ok(
        notifications::dispatch_native_approval_notification(app, Arc::clone(&state.core), req)
            .await,
    )
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
#[allow(clippy::too_many_arguments)]
async fn resolve_plan_approval(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    permission_request_id: String,
    action: orbit_protocol::PlanApprovalAction,
    edited_plan_markdown: Option<String>,
    feedback: Option<String>,
    target_mode: Option<SessionMode>,
) -> Result<(), String> {
    state
        .core
        .resolve_plan_approval_on_host(
            &host_id,
            &agent_id,
            &permission_request_id,
            action,
            edited_plan_markdown,
            feedback,
            target_mode,
        )
        .await
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
async fn git_stage_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    paths: Vec<String>,
    all: Option<bool>,
) -> Result<GitMutationOutcome, String> {
    state
        .core
        .git_stage_files_on_host(&host_id, &cwd, paths, all.unwrap_or(false))
        .await
}

#[tauri::command]
async fn git_unstage_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    paths: Vec<String>,
    all: Option<bool>,
) -> Result<GitMutationOutcome, String> {
    state
        .core
        .git_unstage_files_on_host(&host_id, &cwd, paths, all.unwrap_or(false))
        .await
}

#[tauri::command]
async fn git_apply_hunk(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    patch: String,
    reverse: Option<bool>,
    cached: Option<bool>,
) -> Result<GitMutationOutcome, String> {
    state
        .core
        .git_apply_hunk_on_host(
            &host_id,
            &cwd,
            patch,
            reverse.unwrap_or(false),
            cached.unwrap_or(false),
        )
        .await
}

#[tauri::command]
async fn git_discard_files(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    paths: Vec<String>,
    include_untracked: Option<bool>,
) -> Result<GitMutationOutcome, String> {
    state
        .core
        .git_discard_files_on_host(&host_id, &cwd, paths, include_untracked.unwrap_or(false))
        .await
}

#[tauri::command]
async fn git_commit(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    message: String,
    amend: Option<bool>,
) -> Result<GitCommitResult, String> {
    state
        .core
        .git_commit_on_host(&host_id, &cwd, message, amend.unwrap_or(false))
        .await
}

#[tauri::command]
async fn git_push(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    remote: Option<String>,
    branch: Option<String>,
    set_upstream: Option<bool>,
) -> Result<GitPushResult, String> {
    state
        .core
        .git_push_on_host(
            &host_id,
            &cwd,
            remote,
            branch,
            set_upstream.unwrap_or(false),
        )
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn git_create_pr(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    title: String,
    body: Option<String>,
    base: Option<String>,
    draft: Option<bool>,
) -> Result<GitCreatePrResult, String> {
    state
        .core
        .git_create_pr_on_host(
            &host_id,
            &cwd,
            title,
            body.unwrap_or_default(),
            base,
            draft.unwrap_or(false),
        )
        .await
}

#[tauri::command]
async fn git_stash_list(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitStashListResult, String> {
    state.core.git_stash_list_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn git_stash_push(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    message: Option<String>,
    include_untracked: Option<bool>,
) -> Result<GitStashMutationResult, String> {
    state
        .core
        .git_stash_push_on_host(&host_id, &cwd, message, include_untracked.unwrap_or(false))
        .await
}

#[tauri::command]
async fn git_stash_apply(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    stash_ref: String,
    expected_oid: Option<String>,
) -> Result<GitStashMutationResult, String> {
    state
        .core
        .git_stash_apply_on_host(&host_id, &cwd, stash_ref, expected_oid)
        .await
}

#[tauri::command]
async fn git_stash_pop(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    stash_ref: String,
    expected_oid: Option<String>,
) -> Result<GitStashMutationResult, String> {
    state
        .core
        .git_stash_pop_on_host(&host_id, &cwd, stash_ref, expected_oid)
        .await
}

#[tauri::command]
async fn git_stash_drop(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    stash_ref: String,
    expected_oid: Option<String>,
) -> Result<GitStashMutationResult, String> {
    state
        .core
        .git_stash_drop_on_host(&host_id, &cwd, stash_ref, expected_oid)
        .await
}

#[tauri::command]
async fn git_rebase_status(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitRebaseStatus, String> {
    state.core.git_rebase_status_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn git_rebase_start(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    onto_ref: String,
) -> Result<GitRebaseActionResult, String> {
    state
        .core
        .git_rebase_start_on_host(&host_id, &cwd, onto_ref)
        .await
}

#[tauri::command]
async fn git_rebase_continue(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitRebaseActionResult, String> {
    state.core.git_rebase_continue_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn git_rebase_abort(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitRebaseActionResult, String> {
    state.core.git_rebase_abort_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn git_rebase_skip(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    expected_step: Option<u32>,
) -> Result<GitRebaseActionResult, String> {
    state
        .core
        .git_rebase_skip_on_host(&host_id, &cwd, expected_step)
        .await
}

#[tauri::command]
async fn git_list_conflicts(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitConflictListResult, String> {
    state.core.git_list_conflicts_on_host(&host_id, &cwd).await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn git_resolve_conflict(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    block_index: Option<u32>,
    resolution: GitConflictResolution,
    auto_stage: Option<bool>,
    expected_sha256: Option<String>,
) -> Result<GitResolveConflictResult, String> {
    state
        .core
        .git_resolve_conflict_on_host(
            &host_id,
            &cwd,
            path,
            block_index,
            resolution,
            auto_stage.unwrap_or(true),
            expected_sha256,
        )
        .await
}

#[tauri::command]
async fn git_pr_status(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitPrStatus, String> {
    state.core.git_pr_status_on_host(&host_id, &cwd).await
}

#[tauri::command]
async fn git_generate_commit_message(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
) -> Result<GitGeneratedCommitMessage, String> {
    state
        .core
        .git_generate_commit_message_on_host(&host_id, &cwd)
        .await
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
fn write_terminal_input(
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
    data: String,
) -> Result<(), String> {
    let trimmed_host = host_id.trim();
    let trimmed_tid = terminal_id.trim();
    if trimmed_host.is_empty() || trimmed_tid.is_empty() {
        return Err("host_id and terminal_id are required".into());
    }
    state
        .core
        .send_terminal_input_by_id(trimmed_host, trimmed_tid, data.into_bytes());
    Ok(())
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
fn resize_terminal(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cols: u16,
    rows: u16,
    terminal_id: Option<String>,
) {
    if let Some(tid) = terminal_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        state.core.resize_terminal_by_id(&host_id, tid, cols, rows);
    } else {
        state.core.resize_terminal(&host_id, cols, rows);
    }
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

const DETACHED_TERMINAL_WINDOW_PREFIX: &str = "orbit-terminal-";
const REATTACH_TERMINAL_EVENT: &str = "orbit://reattach-terminal";

#[derive(Debug, Clone, serde::Serialize)]
struct ReattachTerminalPayload {
    host_id: String,
    terminal_id: String,
    title: String,
    cwd: String,
    slot: Option<u8>,
}

fn fnv1a_64(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (idx, part) in parts.iter().enumerate() {
        if idx > 0 {
            hash ^= 0x1f;
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        for &byte in *part {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// Deterministic, bounded, Tauri-safe window label (`orbit-terminal-<slug>-<hash16>`, <= 48 chars).
fn detached_terminal_window_label(host_id: &str, terminal_id: &str) -> String {
    let digest = fnv1a_64(&[host_id.trim().as_bytes(), terminal_id.trim().as_bytes()]);
    let slug: String = terminal_id
        .trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .take(12)
        .collect();
    if slug.is_empty() {
        format!("{DETACHED_TERMINAL_WINDOW_PREFIX}{digest:016x}")
    } else {
        format!("{DETACHED_TERMINAL_WINDOW_PREFIX}{slug}-{digest:016x}")
    }
}

fn encode_query_component(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for &b in raw.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
    }
    out
}

fn build_detached_terminal_url(
    host_id: &str,
    terminal_id: &str,
    title: &str,
    cwd: Option<&str>,
) -> String {
    let mut url = format!(
        "index.html?detachedTerminal=1&hostId={}&terminalId={}&title={}",
        encode_query_component(host_id),
        encode_query_component(terminal_id),
        encode_query_component(title),
    );
    if let Some(dir) = cwd.map(str::trim).filter(|s| !s.is_empty()) {
        url.push_str("&cwd=");
        url.push_str(&encode_query_component(dir));
    }
    url
}

#[tauri::command]
async fn open_detached_terminal_window(
    app: AppHandle,
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
    title: String,
    cwd: Option<String>,
) -> Result<String, String> {
    let trimmed_host = host_id.trim();
    let trimmed_tid = terminal_id.trim();
    if trimmed_host.is_empty() || trimmed_tid.is_empty() {
        return Err("host_id and terminal_id are required to detach terminal".into());
    }

    let clean_title: String = title
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(80)
        .collect();
    let effective_title = if clean_title.is_empty() {
        "Orbit Terminal".to_string()
    } else {
        clean_title
    };

    let _ = state
        .core
        .ensure_terminal_attached(trimmed_host, trimmed_tid);

    let label = detached_terminal_window_label(trimmed_host, trimmed_tid);
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.unminimize();
        let _ = existing.show();
        existing
            .set_focus()
            .map_err(|e| format!("Failed to focus detached terminal window: {e}"))?;
        return Ok(label);
    }

    let app_url =
        build_detached_terminal_url(trimmed_host, trimmed_tid, &effective_title, cwd.as_deref());
    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(PathBuf::from(app_url)))
        .title(&effective_title)
        .inner_size(860.0, 540.0)
        .min_inner_size(460.0, 300.0);

    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);

    let window = builder
        .build()
        .map_err(|e| format!("Failed to open detached terminal window: {e}"))?;
    zoom::inherit(&window);

    Ok(label)
}

#[tauri::command]
async fn reattach_detached_terminal(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, DesktopAppState>,
    host_id: String,
    terminal_id: String,
    title: String,
    cwd: Option<String>,
) -> Result<(), String> {
    let trimmed_host = host_id.trim().to_string();
    let trimmed_tid = terminal_id.trim().to_string();
    let slot = if !trimmed_host.is_empty() && !trimmed_tid.is_empty() {
        state
            .core
            .ensure_terminal_attached(&trimmed_host, &trimmed_tid)
    } else {
        None
    };
    let payload = ReattachTerminalPayload {
        host_id: trimmed_host,
        terminal_id: trimmed_tid,
        title: title.trim().to_string(),
        cwd: cwd.unwrap_or_default().trim().to_string(),
        slot,
    };
    let _ = app.emit_to("main", REATTACH_TERMINAL_EVENT, payload);
    let _ = tray::focus_main_window_impl(&app);
    if window.label() != "main" {
        state.cleanup_window_terminal_subscriptions(window.label());
        let _ = window.close();
    }
    Ok(())
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
        .read_workspace_file_on_host_prefix(&host_id, &cwd, &path)
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
async fn lsp_hover(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
    content: Option<String>,
) -> Result<Option<LspHoverInfo>, String> {
    state
        .core
        .lsp_hover_on_host(&host_id, &cwd, &path, line, col, content)
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn lsp_references(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
    include_declaration: Option<bool>,
    content: Option<String>,
) -> Result<Vec<LspReferenceLocation>, String> {
    state
        .core
        .lsp_references_on_host(
            &host_id,
            &cwd,
            &path,
            line,
            col,
            include_declaration.unwrap_or(true),
            content,
        )
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn lsp_completion(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
    trigger_character: Option<String>,
    content: Option<String>,
) -> Result<Vec<LspCompletionItem>, String> {
    state
        .core
        .lsp_completion_on_host(&host_id, &cwd, &path, line, col, trigger_character, content)
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn lsp_code_action(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
    end_line: u32,
    end_col: u32,
    diagnostics: Option<Vec<LspDiagnostic>>,
    content: Option<String>,
) -> Result<Vec<LspCodeAction>, String> {
    state
        .core
        .lsp_code_action_on_host(
            &host_id,
            &cwd,
            &path,
            line,
            col,
            end_line,
            end_col,
            diagnostics.unwrap_or_default(),
            content,
        )
        .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn lsp_rename(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    path: String,
    line: u32,
    col: u32,
    new_name: String,
    content: Option<String>,
) -> Result<LspWorkspaceEditResult, String> {
    state
        .core
        .lsp_rename_on_host(&host_id, &cwd, &path, line, col, &new_name, content)
        .await
}

#[tauri::command]
async fn lsp_apply_workspace_edit(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    edit: LspWorkspaceEdit,
) -> Result<LspWorkspaceEditResult, String> {
    state
        .core
        .lsp_apply_workspace_edit_on_host(&host_id, &cwd, edit)
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
async fn get_fim_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<FimConfig, String> {
    state.core.get_fim_config(&host_id).await
}
#[tauri::command]
async fn update_fim_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    config: FimConfig,
) -> Result<FimConfig, String> {
    state.core.update_fim_config(&host_id, config).await
}
#[tauri::command]
async fn predict_inline_edit(
    state: State<'_, DesktopAppState>,
    host_id: String,
    input: InlineEditInput,
) -> Result<InlineEditResult, String> {
    state.core.predict_inline_edit(&host_id, input).await
}
#[tauri::command]
async fn run_pr_patrol(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    base_ref: Option<String>,
    head_ref: Option<String>,
    pr_number: Option<u64>,
    post_github_comments: bool,
) -> Result<PrPatrolReport, String> {
    state
        .core
        .run_pr_patrol(
            &host_id,
            cwd,
            base_ref,
            head_ref,
            pr_number,
            post_github_comments,
        )
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
async fn list_permission_rules(
    state: State<'_, DesktopAppState>,
    host_id: String,
    project_cwd: Option<String>,
) -> Result<Vec<PermissionRuleRecord>, String> {
    state
        .core
        .list_permission_rules_on_host(&host_id, project_cwd)
        .await
}

#[tauri::command]
async fn upsert_permission_rule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    rule: UpsertPermissionRuleInput,
) -> Result<PermissionRuleRecord, String> {
    state
        .core
        .upsert_permission_rule_on_host(&host_id, rule)
        .await
}

#[tauri::command]
async fn delete_permission_rule(
    state: State<'_, DesktopAppState>,
    host_id: String,
    rule_id: String,
) -> Result<bool, String> {
    state
        .core
        .delete_permission_rule_on_host(&host_id, rule_id)
        .await
}

#[tauri::command]
async fn resolve_permissions_batch(
    state: State<'_, DesktopAppState>,
    host_id: String,
    items: Vec<(String, String)>,
    decision: String,
) -> Result<ResolvePermissionsBatchResult, String> {
    state
        .core
        .resolve_permissions_batch_on_host(&host_id, items, decision)
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
async fn get_personal_router_status(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<orbit_protocol::RouterStatusSnapshot, String> {
    state
        .core
        .get_personal_router_status_on_host(&host_id)
        .await
}

#[tauri::command]
async fn configure_personal_router(
    state: State<'_, DesktopAppState>,
    host_id: String,
    config: orbit_protocol::PersonalRouterConfig,
    external_webhook_secret: Option<String>,
    activate: bool,
) -> Result<orbit_protocol::RouterStatusSnapshot, String> {
    state
        .core
        .configure_personal_router_on_host(&host_id, config, external_webhook_secret, activate)
        .await
}

#[tauri::command]
async fn discover_fleet_router_hosts(
    state: State<'_, DesktopAppState>,
) -> Result<Vec<orbit_protocol::RouterHostCapabilities>, String> {
    state.core.discover_fleet_router_hosts().await
}

#[tauri::command]
async fn export_hermes_bridge_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<orbit_protocol::HermesBridgeBundle, String> {
    state
        .core
        .export_hermes_bridge_config_on_host(&host_id)
        .await
}

#[tauri::command]
fn get_primary_router_selection(
    state: State<'_, DesktopAppState>,
) -> Option<orbit_protocol::PrimaryRouterSelection> {
    state.core.get_primary_router_selection()
}

#[tauri::command]
fn set_primary_router_selection(
    state: State<'_, DesktopAppState>,
    selection: Option<orbit_protocol::PrimaryRouterSelection>,
) -> Result<Option<orbit_protocol::PrimaryRouterSelection>, String> {
    state.core.set_primary_router_selection(selection)
}

#[tauri::command]
async fn get_personal_agent_briefing(
    state: State<'_, DesktopAppState>,
    primary_router_host_id: Option<String>,
) -> Result<PersonalAgentBriefingView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.personal_agent_briefing(primary_router_host_id)
    })
    .await
    .map_err(|e| format!("personal agent briefing task error: {e}"))
}

#[tauri::command]
async fn list_personal_agent_activity(
    state: State<'_, DesktopAppState>,
    host_id: String,
    limit: Option<u32>,
    since_ms: Option<i64>,
) -> Result<Vec<orbit_protocol::PersonalAgentActivityEntry>, String> {
    state
        .core
        .list_personal_agent_activity(&host_id, limit.unwrap_or(50), since_ms)
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
    replace_existing: Option<bool>,
    dispatch_initial_prompt: Option<bool>,
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
            replace_existing: replace_existing.unwrap_or(false),
            dispatch_initial_prompt,
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
#[allow(clippy::too_many_arguments)]
async fn update_goal_run(
    state: State<'_, DesktopAppState>,
    host_id: String,
    goal_id: String,
    objective: Option<String>,
    criteria: Option<Vec<String>>,
    verify_command: Option<Option<String>>,
    max_iterations: Option<u32>,
    max_duration_secs: Option<u64>,
) -> Result<orbit_protocol::GoalRunRecord, String> {
    state
        .core
        .update_goal_run_on_host(orbit_client_core::contract::UpdateGoalRunRequest {
            host_id,
            goal_id,
            objective,
            criteria,
            verify_command,
            max_iterations,
            max_duration_secs,
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
#[allow(clippy::too_many_arguments)]
async fn create_browser_session(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: Option<String>,
    initial_url: Option<String>,
    headless: Option<bool>,
    viewport_width: Option<u32>,
    viewport_height: Option<u32>,
    workspace_root: Option<String>,
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
            workspace_root,
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
async fn open_remote_preview_tunnel(
    state: State<'_, DesktopAppState>,
    host_id: String,
    target_port: u16,
    proxy_host: Option<String>,
) -> Result<orbit_client_core::contract::RemotePreviewTunnelView, String> {
    state
        .core
        .open_remote_preview_tunnel(&host_id, target_port, proxy_host)
        .await
}

#[tauri::command]
fn close_remote_preview_tunnel(
    state: State<'_, DesktopAppState>,
    host_id: String,
    target_port: u16,
    proxy_host: Option<String>,
) -> Result<bool, String> {
    state
        .core
        .close_remote_preview_tunnel(&host_id, target_port, proxy_host)
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
    apply_to_all_hosts: Option<bool>,
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

    if apply_to_all_hosts.unwrap_or(true) {
        let _ = state
            .core
            .apply_relay_to_fleet(&relay_url)
            .map_err(|e| e.to_string())?;
    } else {
        state
            .core
            .update_host_relay(&target_host_id, Some(&relay_url))
            .map_err(|e| e.to_string())?;
    }

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
async fn cloud_sync_export_device_link_uri(
    state: State<'_, DesktopAppState>,
) -> Result<String, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_export_device_link_uri()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync export device link task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_import_device_link_uri(
    state: State<'_, DesktopAppState>,
    uri: String,
) -> Result<CloudSyncResultView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_import_device_link_uri(&uri)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync import device link task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_enable_manual_passphrase(
    state: State<'_, DesktopAppState>,
    passphrase: String,
) -> Result<CloudSyncStatusView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_enable_manual_passphrase(&passphrase)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync enable manual passphrase task error: {e}"))?
}

#[tauri::command]
async fn cloud_sync_set_manual_passphrase_mode(
    state: State<'_, DesktopAppState>,
    require_manual: bool,
) -> Result<CloudSyncStatusView, String> {
    let core = Arc::clone(&state.core);
    tauri::async_runtime::spawn_blocking(move || {
        core.cloud_sync_set_manual_passphrase_mode(require_manual)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("cloud sync mode update task error: {e}"))?
}

#[tauri::command]
async fn provision_cloudflare_tunnel_relay(
    state: State<'_, DesktopAppState>,
    target_host_id: String,
    ssh_alias: String,
    relay_port: u16,
    public_hostname: Option<String>,
    tunnel_token: Option<String>,
    apply_to_all_hosts: Option<bool>,
) -> Result<String, String> {
    let overview = state.core.fleet_overview();
    let pubkeys: Vec<String> = overview
        .hosts
        .into_iter()
        .map(|h| h.daemon_pubkey_b64)
        .collect();

    let relay_url = tauri::async_runtime::spawn_blocking(move || {
        orbit_client_core::desktop_ops::provision_cloudflare_tunnel_relay_over_ssh(
            &ssh_alias,
            relay_port,
            &pubkeys,
            public_hostname.as_deref(),
            tunnel_token.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("task join error: {e}"))??;

    if apply_to_all_hosts.unwrap_or(true) {
        let _ = state
            .core
            .apply_relay_to_fleet(&relay_url)
            .map_err(|e| e.to_string())?;
    } else {
        state
            .core
            .update_host_relay(&target_host_id, Some(&relay_url))
            .map_err(|e| e.to_string())?;
    }

    Ok(relay_url)
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

// --- M3 Security, Audit Chain, Diagnostics, Context Compaction & IDE Migration ---

#[tauri::command]
async fn get_sandbox_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<Option<SandboxPolicy>, String> {
    state.core.get_sandbox_config(&host_id).await
}

#[tauri::command]
async fn update_sandbox_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    sandbox_policy: Option<SandboxPolicy>,
) -> Result<Option<SandboxPolicy>, String> {
    state
        .core
        .update_sandbox_config(&host_id, sandbox_policy)
        .await
}

#[tauri::command]
async fn get_audit_chain_status(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<AuditChainVerification, String> {
    state.core.get_audit_chain_status(&host_id).await
}

#[tauri::command]
async fn export_audit_chain(
    state: State<'_, DesktopAppState>,
    host_id: String,
    after_seq: Option<u64>,
) -> Result<AuditChainExportView, String> {
    state
        .core
        .export_audit_chain(&host_id, after_seq.unwrap_or(0))
        .await
}

#[tauri::command]
async fn export_diagnostic_bundle(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<serde_json::Value, String> {
    state.core.export_diagnostic_bundle(&host_id).await
}

#[tauri::command]
async fn compact_agent_context(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
) -> Result<(), String> {
    state.core.compact_agent_context(&host_id, &agent_id).await
}

#[tauri::command]
async fn inspect_external_ide_migration(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source: Option<String>,
    cwd: Option<String>,
    conflict: Option<MigrationConflict>,
) -> Result<serde_json::Value, String> {
    state
        .core
        .inspect_external_ide_migration(&host_id, source, cwd, conflict.unwrap_or_default())
        .await
}

#[tauri::command]
async fn execute_external_ide_migration(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source: Option<String>,
    cwd: Option<String>,
    conflict: Option<MigrationConflict>,
) -> Result<serde_json::Value, String> {
    state
        .core
        .execute_external_ide_migration(&host_id, source, cwd, conflict.unwrap_or_default())
        .await
}

// --- M4 Project Memory ---

#[tauri::command]
async fn list_project_memories(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    status: Option<MemoryStatus>,
    category: Option<MemoryCategory>,
    query: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<ProjectMemoryItem>, String> {
    state
        .core
        .list_project_memories(&host_id, &cwd, status, category, query, limit)
        .await
}

#[tauri::command]
async fn upsert_project_memory(
    state: State<'_, DesktopAppState>,
    req: UpsertProjectMemoryRequest,
) -> Result<ProjectMemoryItem, String> {
    state.core.upsert_project_memory(req).await
}

#[tauri::command]
async fn confirm_project_memory(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    memory_id: String,
) -> Result<ProjectMemoryItem, String> {
    state
        .core
        .confirm_project_memory(&host_id, &cwd, &memory_id)
        .await
}

#[tauri::command]
async fn delete_project_memory(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    memory_id: String,
) -> Result<bool, String> {
    state
        .core
        .delete_project_memory(&host_id, &cwd, &memory_id)
        .await
}

#[tauri::command]
async fn recall_project_memory(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    query: String,
    limit: Option<u32>,
    include_unconfirmed: Option<bool>,
) -> Result<Vec<ProjectMemorySearchHit>, String> {
    state
        .core
        .recall_project_memory(&host_id, &cwd, &query, limit, include_unconfirmed)
        .await
}

// --- M5 WASM Plugins ---

#[tauri::command]
async fn list_plugins(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<Vec<InstalledPluginView>, String> {
    state.core.list_plugins(&host_id).await
}

#[tauri::command]
async fn install_plugin(
    state: State<'_, DesktopAppState>,
    host_id: String,
    source_path: String,
    enable: Option<bool>,
) -> Result<InstalledPluginView, String> {
    state
        .core
        .install_plugin(&host_id, &source_path, enable.unwrap_or(false))
        .await
}

#[tauri::command]
async fn set_plugin_enabled(
    state: State<'_, DesktopAppState>,
    host_id: String,
    plugin_id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .core
        .set_plugin_enabled(&host_id, &plugin_id, enabled)
        .await
}

#[tauri::command]
async fn uninstall_plugin(
    state: State<'_, DesktopAppState>,
    host_id: String,
    plugin_id: String,
) -> Result<(), String> {
    state.core.uninstall_plugin(&host_id, &plugin_id).await
}

#[tauri::command]
async fn invoke_plugin_op(
    state: State<'_, DesktopAppState>,
    host_id: String,
    plugin_id: String,
    op: String,
    args_json: Option<serde_json::Value>,
    cwd: Option<String>,
) -> Result<serde_json::Value, String> {
    state
        .core
        .invoke_plugin_op(
            &host_id,
            &plugin_id,
            &op,
            args_json.unwrap_or_else(|| serde_json::json!({})),
            cwd,
        )
        .await
}

#[tauri::command]
async fn render_plugin_ui(
    state: State<'_, DesktopAppState>,
    host_id: String,
    plugin_id: String,
    slot: OrbitPluginUiSlot,
    cwd: Option<String>,
    action_id: Option<String>,
    payload: Option<serde_json::Value>,
) -> Result<OrbitPluginUiSpec, String> {
    state
        .core
        .render_plugin_ui(&host_id, &plugin_id, slot, cwd, action_id, payload)
        .await
}

// --- M6 Voice Control Plane ---

fn format_voice_rpc_error(err: orbit_client_core::VoiceRpcError) -> String {
    serde_json::to_string(&err).unwrap_or_else(|_| err.to_string())
}

#[tauri::command]
async fn get_voice_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
) -> Result<VoiceConfigState, String> {
    state
        .core
        .get_voice_config(&host_id)
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn update_voice_config(
    state: State<'_, DesktopAppState>,
    host_id: String,
    config: VoiceConfig,
) -> Result<VoiceConfigState, String> {
    state
        .core
        .update_voice_config(&host_id, config)
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn download_voice_model(
    state: State<'_, DesktopAppState>,
    host_id: String,
    model_id: String,
) -> Result<VoiceConfigState, String> {
    state
        .core
        .download_voice_model(&host_id, model_id)
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn delete_voice_model(
    state: State<'_, DesktopAppState>,
    host_id: String,
    model_id: String,
) -> Result<VoiceConfigState, String> {
    state
        .core
        .delete_voice_model(&host_id, model_id)
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn transcribe_voice_audio(
    state: State<'_, DesktopAppState>,
    host_id: String,
    pcm16_le_base64: String,
    sample_rate_hz: u32,
    language: Option<String>,
    agent_id: Option<String>,
) -> Result<VoiceTranscriptionResult, String> {
    state
        .core
        .transcribe_voice_audio(
            &host_id,
            pcm16_le_base64,
            sample_rate_hz,
            language,
            agent_id,
        )
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn parse_voice_transcript(
    state: State<'_, DesktopAppState>,
    host_id: String,
    text: String,
    language: Option<String>,
    agent_id: Option<String>,
) -> Result<VoiceTranscriptionResult, String> {
    state
        .core
        .parse_voice_transcript(&host_id, text, language, agent_id)
        .await
        .map_err(format_voice_rpc_error)
}

#[tauri::command]
async fn approve_voice_permission(
    state: State<'_, DesktopAppState>,
    host_id: String,
    agent_id: String,
    permission_request_id: String,
    confirmed: bool,
) -> Result<VoiceApprovalSafety, String> {
    state
        .core
        .approve_voice_permission(&host_id, agent_id, permission_request_id, confirmed)
        .await
        .map_err(format_voice_rpc_error)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct SystemAudioInputDevice {
    name: String,
    is_default: bool,
    transport: Option<String>,
}

fn normalize_macos_audio_transport(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let stripped = trimmed
        .strip_prefix("coreaudio_device_type_")
        .unwrap_or(trimmed);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn parse_macos_audio_inputs(json: &str) -> Vec<SystemAudioInputDevice> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(groups) = root.get("SPAudioDataType").and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    let mut out: Vec<SystemAudioInputDevice> = Vec::new();
    for group in groups {
        let Some(items) = group.get("_items").and_then(|v| v.as_array()) else {
            continue;
        };
        for item in items {
            let has_input = item
                .get("coreaudio_device_input")
                .and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|n| n as u64)))
                .is_some_and(|channels| channels > 0);
            if !has_input {
                continue;
            }
            let Some(name) = item
                .get("_name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            else {
                continue;
            };
            let is_default = item
                .get("coreaudio_default_audio_input_device")
                .and_then(|v| v.as_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("spaudio_yes"));
            let transport = item
                .get("coreaudio_device_transport")
                .and_then(|v| v.as_str())
                .and_then(normalize_macos_audio_transport);

            if let Some(existing) = out
                .iter_mut()
                .find(|entry| entry.name.eq_ignore_ascii_case(name))
            {
                existing.is_default |= is_default;
                if existing.transport.is_none() {
                    existing.transport = transport;
                }
            } else {
                out.push(SystemAudioInputDevice {
                    name: name.to_string(),
                    is_default,
                    transport,
                });
            }
        }
    }

    out
}

#[tauri::command]
async fn list_system_audio_inputs() -> Vec<SystemAudioInputDevice> {
    #[cfg(target_os = "macos")]
    {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::process::Command::new("/usr/sbin/system_profiler")
                .args(["SPAudioDataType", "-json"])
                .output(),
        )
        .await;
        match output {
            Ok(Ok(out)) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                parse_macos_audio_inputs(&text)
            }
            _ => Vec::new(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[tauri::command]
async fn synthesize_voice_speech(
    state: State<'_, DesktopAppState>,
    host_id: String,
    text: String,
    language: Option<String>,
    voice_id: Option<String>,
) -> Result<VoiceSpeechResult, String> {
    state
        .core
        .synthesize_voice_speech(&host_id, text, language, voice_id)
        .await
        .map_err(format_voice_rpc_error)
}

fn init_desktop_file_logging() {
    let log_path = resolve_user_home()
        .map(|home| home.join(".orbit").join("logs").join("desktop.log"))
        .unwrap_or_else(|| PathBuf::from("orbit-desktop.log"));

    let mut env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    for directive in ["tungstenite=info", "tokio_tungstenite=info"] {
        if let Ok(parsed) = directive.parse() {
            env_filter = env_filter.add_directive(parsed);
        }
    }

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
    let core = OrbitAppCore::open(&db_path, false)
        .or_else(|err| {
            tracing::error!(error = %err, "Failed to open desktop replica SQLite; falling back to in-memory store");
            OrbitAppCore::open_in_memory_with_discovery(false)
        })
        .expect("failed to initialize OrbitAppCore");
    core.connect_all_hosts();

    let app = tauri::Builder::default()
        .manage(DesktopAppState::new(core))
        .manage(tray::TrayMenuState::default())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event
                && let Some(app_state) = window.try_state::<DesktopAppState>()
            {
                app_state.cleanup_window_terminal_subscriptions(window.label());
            }
        })
        .on_menu_event(app_menu::on_menu_event)
        .setup(|app| {
            let percent = window_state::setup_main_window_state(app);
            let persist = window_state::zoom_persistence(app);
            app.manage(zoom::ZoomController::new(percent, persist));
            #[cfg(target_os = "macos")]
            {
                let (menu, items) = app_menu::build_app_menu(app.handle())?;
                app.set_menu(menu)?;
                app.manage(items);
                app_menu::sync(&app.state::<app_menu::ZoomMenuItems>(), percent);
            }
            tray::setup_system_tray(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            zoom::get_ui_zoom,
            zoom::step_ui_zoom,
            zoom::set_ui_zoom,
            subscribe_core_events,
            subscribe_terminal_binary,
            unsubscribe_terminal_binary,
            ensure_terminal_slot,
            get_bridge_info,
            get_fleet_overview,
            search_messages,
            search_messages_remote,
            get_fleet_sidebar,
            get_host_detail,
            get_agent_stage,
            get_agent_stage_delta,
            get_agent_stage_turns_slice,
            load_older_agent_timeline,
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
            get_local_machine_info,
            pair_local_host,
            remove_host,
            update_host_config,
            update_host_relay,
            apply_relay_to_all_hosts,
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
            interrupt_agent_and_continue,
            set_session_mode,
            set_agent_config,
            set_agent_model,
            switch_agent_provider,
            archive_agent,
            unarchive_agent,
            delete_agent,
            detach_agent,
            harvest_conversation,
            mark_agent_read,
            update_agent,
            fetch_workspace_board,
            get_workspace_board,
            update_workspace_board,
            fetch_workspace_events,
            get_workspace_events,
            update_workspace_goal,
            reset_workspace_board,
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
            read_mcp_resource,
            call_artifact_mcp_tool,
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
            preview_rewind_agent,
            rewind_agent,
            send_native_approval_notification,
            fork_agent,
            preview_worktree_handoff,
            execute_worktree_handoff,
            resolve_permission,
            resolve_plan_approval,
            answer_question,
            fetch_git_status,
            fetch_git_diff,
            git_stage_files,
            git_unstage_files,
            git_apply_hunk,
            git_discard_files,
            git_commit,
            git_push,
            git_create_pr,
            git_stash_list,
            git_stash_push,
            git_stash_apply,
            git_stash_pop,
            git_stash_drop,
            git_rebase_status,
            git_rebase_start,
            git_rebase_continue,
            git_rebase_abort,
            git_rebase_skip,
            git_list_conflicts,
            git_resolve_conflict,
            git_pr_status,
            git_generate_commit_message,
            start_terminal,
            attach_terminal,
            kill_terminal,
            send_terminal_input,
            write_terminal_input,
            send_terminal_input_slot,
            resize_terminal,
            resize_terminal_slot,
            open_detached_terminal_window,
            reattach_detached_terminal,
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
            lsp_hover,
            lsp_references,
            lsp_completion,
            lsp_code_action,
            lsp_rename,
            lsp_apply_workspace_edit,
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
            get_fim_config,
            update_fim_config,
            predict_inline_edit,
            run_pr_patrol,
            get_guardian_config,
            update_guardian_config,
            list_permission_rules,
            upsert_permission_rule,
            delete_permission_rule,
            resolve_permissions_batch,
            list_im_channels,
            upsert_im_channel,
            delete_im_channel,
            test_im_channel,
            get_personal_router_status,
            configure_personal_router,
            discover_fleet_router_hosts,
            export_hermes_bridge_config,
            get_primary_router_selection,
            set_primary_router_selection,
            get_personal_agent_briefing,
            list_personal_agent_activity,
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
            update_goal_run,
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
            open_remote_preview_tunnel,
            close_remote_preview_tunnel,
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
            provision_cloudflare_tunnel_relay,
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
            cloud_sync_export_device_link_uri,
            cloud_sync_import_device_link_uri,
            cloud_sync_enable_manual_passphrase,
            cloud_sync_set_manual_passphrase_mode,
            cloud_sync_trigger_background,
            cloud_sync_logout,
            cloud_sync_get_preferences,
            cloud_sync_update_preferences,
            get_sandbox_config,
            update_sandbox_config,
            get_audit_chain_status,
            export_audit_chain,
            export_diagnostic_bundle,
            compact_agent_context,
            inspect_external_ide_migration,
            execute_external_ide_migration,
            list_project_memories,
            upsert_project_memory,
            confirm_project_memory,
            delete_project_memory,
            recall_project_memory,
            list_plugins,
            install_plugin,
            set_plugin_enabled,
            uninstall_plugin,
            invoke_plugin_op,
            render_plugin_ui,
            get_voice_config,
            update_voice_config,
            download_voice_model,
            delete_voice_model,
            transcribe_voice_audio,
            parse_voice_transcript,
            approve_voice_permission,
            synthesize_voice_speech,
            list_system_audio_inputs,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|handle, event| {
            if let tauri::RunEvent::Exit = event {
                window_state::flush_on_exit(handle);
            }
        }),
        Err(error) => {
            tracing::error!(%error, "Failed to build Orbit Tauri desktop application");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_terminal_window_label_is_bounded_sanitized_and_deterministic() {
        let label_a = detached_terminal_window_label("host-1", "term_abc/123");
        let label_a_again = detached_terminal_window_label("host-1", "term_abc/123");
        let label_b = detached_terminal_window_label("host-1", "term_abc/456");
        let label_other_host = detached_terminal_window_label("host-2", "term_abc/123");
        let label_long = detached_terminal_window_label(
            &"h".repeat(200),
            &format!("../../weird space 🚀 {}", "x".repeat(300)),
        );
        let label_symbols = detached_terminal_window_label("h1", "///!!!");

        assert_eq!(label_a, label_a_again);
        assert_ne!(label_a, label_b);
        assert_ne!(label_a, label_other_host);

        for lbl in [
            &label_a,
            &label_b,
            &label_other_host,
            &label_long,
            &label_symbols,
        ] {
            assert!(
                lbl.starts_with(DETACHED_TERMINAL_WINDOW_PREFIX),
                "unexpected prefix in {lbl}"
            );
            assert!(lbl.len() <= 48, "label exceeded 48 chars: {lbl}");
            assert!(
                lbl.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "unsafe chars in label: {lbl}"
            );
        }
    }

    #[test]
    fn detached_terminal_url_encodes_and_roundtrips_components() {
        let url = build_detached_terminal_url(
            "host/alpha?1",
            "term#42&ok",
            "Build & Test 🚀",
            Some("/Users/dev/my repo"),
        );
        assert!(url.starts_with("index.html?detachedTerminal=1&hostId="));
        assert!(!url.contains(' '));
        assert!(!url.contains('#'));

        let encoded = encode_query_component("Build & Test 🚀 /a?b=c");
        assert_eq!(decode_percent_utf8(&encoded), "Build & Test 🚀 /a?b=c");
    }

    #[test]
    fn window_destroyed_cleans_up_only_that_windows_terminal_subscriptions() {
        let core = OrbitAppCore::open_in_memory().expect("open core");
        let state = DesktopAppState::new(Arc::clone(&core));

        let mut rx_a1 = core.subscribe_terminal_binary("h1", "term-a1");
        let mut rx_a2 = core.subscribe_terminal_binary("h1", "term-a2");
        let rx_b1 = core.subscribe_terminal_binary("h1", "term-b1");

        state.register_window_terminal_subscription("orbit-terminal-a", rx_a1.subscription_id());
        state.register_window_terminal_subscription("orbit-terminal-a", rx_a2.subscription_id());
        state.register_window_terminal_subscription("orbit-terminal-b", rx_b1.subscription_id());
        assert_eq!(core.active_terminal_binary_subscriber_count(), 3);

        // Destroying window A unsubscribes & closes only window A's idle subscriptions
        let cleaned = state.cleanup_window_terminal_subscriptions("orbit-terminal-a");
        assert_eq!(cleaned, 2);
        assert_eq!(core.active_terminal_binary_subscriber_count(), 1);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("rt");
        rt.block_on(async {
            assert_eq!(rx_a1.recv().await, None);
            assert_eq!(rx_a2.recv().await, None);
        });

        // Window B's subscription remains live until unregistered/destroyed
        state.unregister_window_terminal_subscription(rx_b1.subscription_id());
        assert_eq!(
            state.cleanup_window_terminal_subscriptions("orbit-terminal-b"),
            0
        );
        core.unsubscribe_terminal_binary(rx_b1.subscription_id());
        assert_eq!(core.active_terminal_binary_subscriber_count(), 0);
    }

    #[test]
    fn parse_macos_audio_inputs_extracts_bluetooth_and_builtin_mics_only() {
        let sample = r#"{
          "SPAudioDataType": [
            {
              "_items": [
                {
                  "_name": "DELL S2721DGF",
                  "coreaudio_device_output": 2,
                  "coreaudio_device_transport": "coreaudio_device_type_hdmi"
                },
                {
                  "_name": "MacBook Pro麦克风",
                  "coreaudio_device_input": 1,
                  "coreaudio_device_transport": "coreaudio_device_type_builtin"
                },
                {
                  "_name": "Xiaomi Buds 6",
                  "coreaudio_default_audio_input_device": "spaudio_yes",
                  "coreaudio_device_input": 1,
                  "coreaudio_device_transport": "coreaudio_device_type_bluetooth"
                },
                {
                  "_name": "Xiaomi Buds 6",
                  "coreaudio_default_audio_output_device": "spaudio_yes",
                  "coreaudio_device_output": 2,
                  "coreaudio_device_transport": "coreaudio_device_type_bluetooth"
                }
              ]
            }
          ]
        }"#;
        let parsed = parse_macos_audio_inputs(sample);
        assert_eq!(
            parsed,
            vec![
                SystemAudioInputDevice {
                    name: "MacBook Pro麦克风".to_string(),
                    is_default: false,
                    transport: Some("builtin".to_string()),
                },
                SystemAudioInputDevice {
                    name: "Xiaomi Buds 6".to_string(),
                    is_default: true,
                    transport: Some("bluetooth".to_string()),
                },
            ]
        );
        assert!(parse_macos_audio_inputs("not json").is_empty());
    }
}
