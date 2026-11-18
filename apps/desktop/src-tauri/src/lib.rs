mod updater;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use crate::updater::{
    AppUpdateProgress, CURRENT_APP_VERSION, ReleaseUpdateInfo, check_latest_release,
    perform_one_click_update,
};

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::{
    AgentTimelineView, ClientProjectView, CoreEventSubscriber, CoreNotification, FleetOverviewView,
    GitReviewView, HostCardView, HostDetailView, OrbitAppCore,
};
use orbit_client_core::desktop_ops::{
    SshHostItem, discover_user_ssh_hosts, provision_relay_over_ssh, resolve_user_home,
    upgrade_remote_daemon_over_ssh,
};
use orbit_protocol::{ProviderKind, SessionMode};
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
#[allow(clippy::too_many_arguments)]
fn create_agent(
    state: State<'_, DesktopAppState>,
    host_id: String,
    cwd: String,
    extra_dirs: Option<Vec<String>>,
    provider: ProviderKind,
    model: Option<String>,
    mode: SessionMode,
    initial_prompt: String,
    worktree_branch: Option<String>,
) -> String {
    state.core.create_agent_ext(
        &host_id,
        &cwd,
        extra_dirs.unwrap_or_default(),
        provider,
        model,
        mode,
        &initial_prompt,
        worktree_branch.as_deref(),
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
) {
    state.core.send_prompt(&host_id, &agent_id, &prompt);
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
) {
    state
        .core
        .switch_agent_provider(&host_id, &agent_id, provider);
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
fn send_terminal_input(state: State<'_, DesktopAppState>, host_id: String, data: String) {
    state.core.send_terminal_input(&host_id, data.into_bytes());
}

#[tauri::command]
fn resize_terminal(state: State<'_, DesktopAppState>, host_id: String, cols: u16, rows: u16) {
    state.core.resize_terminal(&host_id, cols, rows);
}

#[tauri::command]
fn list_ssh_hosts() -> Vec<SshHostItem> {
    discover_user_ssh_hosts()
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
            create_agent,
            open_agent_timeline,
            send_prompt,
            interrupt_agent,
            set_session_mode,
            set_agent_model,
            switch_agent_provider,
            archive_agent,
            resolve_permission,
            answer_question,
            fetch_git_status,
            fetch_git_diff,
            start_terminal,
            send_terminal_input,
            resize_terminal,
            list_ssh_hosts,
            provision_ssh_relay,
            get_app_version,
            check_app_update,
            perform_app_self_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Orbit Tauri desktop application");
}
