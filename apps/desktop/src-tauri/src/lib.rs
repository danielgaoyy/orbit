use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use orbit_client_core::app_core::{
    AgentTimelineDeltaView, AgentTimelineView, CoreEventSubscriber, CoreNotification,
    CreateAgentRequest, FleetOverviewView, GitReviewView, HostCardView, HostDetailView,
    OrbitAppCore,
};
use orbit_client_core::desktop_ops::{
    ExternalEditorInfo, ExternalEditorKind, SshHostItem, StagedAttachmentInfo,
    detect_installed_editors, discover_user_ssh_hosts, list_local_workspace_files,
    open_in_external_editor, provision_relay_over_ssh, resolve_user_home, stage_local_attachment,
};
use orbit_protocol::{PromptBadge, ProviderKind, ReasoningEffort, SessionMode};
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
fn remove_host(state: State<'_, DesktopAppState>, host_id: String) -> Result<(), String> {
    state.core.remove_host(&host_id).map_err(|e| e.to_string())
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
    provider: ProviderKind,
    model: Option<String>,
    effort: Option<ReasoningEffort>,
    mode: SessionMode,
    initial_prompt: String,
    initial_display_prompt: Option<String>,
    initial_badges: Option<Vec<PromptBadge>>,
    worktree_branch: Option<String>,
) {
    state.core.create_agent_with_config(CreateAgentRequest {
        host_id: &host_id,
        cwd: &cwd,
        provider,
        model,
        effort,
        mode,
        initial_prompt: &initial_prompt,
        initial_display_prompt,
        initial_badges: initial_badges.unwrap_or_default(),
        worktree_branch: worktree_branch.as_deref(),
    });
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
) {
    state.core.send_prompt_with_meta(
        &host_id,
        &agent_id,
        &prompt,
        display_prompt,
        badges.unwrap_or_default(),
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
            remove_host,
            update_host_relay,
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
            switch_agent_provider,
            archive_agent,
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
            get_workspace_files,
            stage_attachment,
            provision_ssh_relay,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit Tauri desktop application");
}
