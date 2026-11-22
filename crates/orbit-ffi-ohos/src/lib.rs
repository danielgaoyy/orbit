//! HarmonyOS 7+ (`API 26` / `aarch64-unknown-linux-ohos`) Native N-API Bridge (`liborbit_ohos.so`).
//!
//! Uses `ohos-rs` (`napi-ohos` + `ThreadsafeFunction`) on HarmonyOS targets to drive ArkTS `@ObservedV2`
//! state models, while remaining 100% buildable and testable on macOS/Linux host toolchains.

use std::sync::Arc;

use orbit_client_core::HostRouteMode;
use orbit_client_core::app_core::{
    AskSideQuestionRequest, ClientThreadPref, CoreEventSubscriber, CoreNotification,
    CreateAgentRequest, OrbitAppCore, RegisterPushDeviceRequest, TimelineItemView,
};
use orbit_client_core::catalog::{
    default_effort_description as core_default_effort_description,
    fallback_provider_catalog as core_fallback_provider_catalog,
    format_catalog_model_label as core_format_catalog_model_label,
    format_effort_label as core_format_effort_label,
    resolve_supported_effort_for_model as core_resolve_supported_effort_for_model,
};
use orbit_client_core::composer::{ClientPlatform, ComposePromptRequest};
use orbit_client_core::contract::{
    AgentStageDeltaRequest, CancelWorkflowRunRequest, ControlGoalRunRequest, CreateSessionRequest,
    CreateWorkspaceEntryRequest, DeleteWorkspaceEntryRequest, ExecuteWorktreeHandoffRequest,
    FleetSidebarRequest, ForkAgentRequest, MentionRankRequest, ModelSelectionRequest,
    PreviewWorktreeHandoffRequest, RenameWorkspaceEntryRequest, RenderDiagramRequest,
    SendPromptRequest, SpawnWorkspacePeerRequest, StopMonitorWatchRequest, UpdateHostConfigRequest,
    UpsertArtifactRequest, WriteWorkspaceFileRequest,
};
use orbit_client_core::diagram::{
    DiagramKind, detect_diagram_kind as core_detect_diagram_kind,
    render_diagram as core_render_diagram,
};
use orbit_client_core::export::ExportOptions;
use orbit_client_core::sidebar::{SidebarProjectSort, SidebarThreadFilter};
use orbit_client_core::stage::describe_tool_step;
use orbit_protocol::{
    AcpAgentSpec, PromptBadge, ProviderKind, ProviderModelCatalog, ProviderModelSpec,
    ReasoningEffort, ScheduleNewAgentConfig, ScheduleTarget, SessionMode,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::task::{Context, Poll, Wake, Waker};

struct OhosThreadWaker(std::thread::Thread);

impl Wake for OhosThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls an `OrbitAppCore` async RPC future on the calling thread without `unsafe` or a second
/// Tokio runtime.
///
/// # Precondition
/// Only pass **bounded** `OrbitAppCore` RPC futures that spawn their I/O work onto
/// `OrbitAppCore`'s internal Tokio runtime and await a bounded `oneshot` reply (with timeout).
/// Do **not** pass unbounded futures or futures that rely on an ambient Tokio reactor on the
/// calling thread, as `std::thread::park` will block the calling thread until the waker fires.
fn block_on_core_rpc<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(OhosThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(future);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn parse_provider_str(raw: &str) -> ProviderKind {
    ProviderKind::from_slug(raw).unwrap_or(ProviderKind::Antigravity)
}

fn parse_mode_str(raw: &str) -> SessionMode {
    match raw.trim() {
        "default" | "" => SessionMode::Default,
        "auto" => SessionMode::Auto,
        "plan" => SessionMode::Plan,
        "accept_edits" => SessionMode::AcceptEdits,
        "full_access" => SessionMode::FullAccess,
        custom => SessionMode::Custom(custom.to_string()),
    }
}

fn parse_effort_opt(raw: Option<&str>) -> Option<ReasoningEffort> {
    raw.and_then(ReasoningEffort::from_slug)
}

fn effort_to_slug(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max => "max",
        ReasoningEffort::Ultra => "ultra",
    }
}

fn parse_sidebar_sort(raw: Option<&str>) -> SidebarProjectSort {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "alphabetical" | "name" => SidebarProjectSort::Alphabetical,
        "manual" | "custom" => SidebarProjectSort::Manual,
        _ => SidebarProjectSort::Recent,
    }
}

fn parse_sidebar_filter(raw: Option<&str>) -> SidebarThreadFilter {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "active_unread" | "activeunread" | "unread" | "needs_attention" | "needsattention"
        | "active" | "running" => SidebarThreadFilter::ActiveUnread,
        _ => SidebarThreadFilter::All,
    }
}

fn parse_client_platform(raw: Option<&str>) -> ClientPlatform {
    match raw
        .unwrap_or("harmony")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "desktop" => ClientPlatform::Desktop,
        "ios" => ClientPlatform::Ios,
        "android" => ClientPlatform::Android,
        _ => ClientPlatform::Harmony,
    }
}

fn parse_string_vec_json(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<String>>(trimmed).map_err(|e| e.to_string())
}

fn parse_badges_json(raw: Option<&str>) -> Result<Vec<PromptBadge>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<PromptBadge>>(trimmed).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OhosCoreNotification {
    FleetChanged {
        revision: u64,
    },
    HostChanged {
        revision: u64,
        host_id: String,
    },
    TimelineChanged {
        revision: u64,
        host_id: String,
        agent_id: String,
    },
    GitChanged {
        revision: u64,
        host_id: String,
        cwd: String,
    },
    TerminalChanged {
        revision: u64,
        host_id: String,
        slot: u8,
    },
}

impl From<CoreNotification> for OhosCoreNotification {
    fn from(ev: CoreNotification) -> Self {
        match ev {
            CoreNotification::FleetChanged { revision } => Self::FleetChanged { revision },
            CoreNotification::HostChanged { revision, host_id } => {
                Self::HostChanged { revision, host_id }
            }
            CoreNotification::TimelineChanged {
                revision,
                host_id,
                agent_id,
            } => Self::TimelineChanged {
                revision,
                host_id,
                agent_id,
            },
            CoreNotification::GitChanged {
                revision,
                host_id,
                cwd,
            } => Self::GitChanged {
                revision,
                host_id,
                cwd,
            },
            CoreNotification::TerminalChanged {
                revision,
                host_id,
                slot,
                ..
            } => Self::TerminalChanged {
                revision,
                host_id,
                slot,
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct OhosThreadPrefSeed {
    host_id: String,
    agent_id: String,
    #[serde(default)]
    seen_seq: u64,
    #[serde(default)]
    is_pinned: bool,
    #[serde(default)]
    pinned_order: i64,
    #[serde(default)]
    updated_at_ms: i64,
}

#[derive(Debug, Deserialize)]
struct OhosAskSideQuestionPayload {
    host_id: String,
    #[serde(default)]
    agent_id: Option<String>,
    cwd: String,
    provider: String,
    #[serde(default)]
    model: Option<String>,
    question: String,
    #[serde(default)]
    context_summary: Option<String>,
}

fn default_true_bool() -> bool {
    true
}

#[derive(Debug, Deserialize)]
struct OhosCreateSchedulePayload {
    host_id: String,
    name: String,
    cron_expr: String,
    #[serde(default)]
    timezone: Option<String>,
    prompt: String,
    #[serde(default)]
    existing_agent_id: Option<String>,
    #[serde(default)]
    cwd: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    provider: String,
    #[serde(default)]
    acp_agent_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    fast_mode: bool,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    worktree: bool,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(default = "default_true_bool")]
    archive_on_finish: bool,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    max_runs: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OhosUpdateSchedulePayload {
    host_id: String,
    schedule_id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    cron_expr: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    max_runs: Option<u32>,
    #[serde(default)]
    archive_on_finish: Option<bool>,
}

/// Pure-Rust JSON facade shared by both host unit tests and the HarmonyOS N-API wrapper.
#[derive(Clone)]
pub struct OhosCoreFacade {
    inner: Arc<OrbitAppCore>,
}

impl OhosCoreFacade {
    pub fn open(db_path: &str) -> Result<Self, String> {
        Self::open_with_options(db_path, false)
    }

    pub fn open_with_options(db_path: &str, auto_discover_local: bool) -> Result<Self, String> {
        let inner = OrbitAppCore::open(db_path, auto_discover_local).map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn subscribe(&self, callback: impl Fn(String) + Send + Sync + 'static) {
        self.subscribe_coalesced(0, callback);
    }

    pub fn subscribe_coalesced(
        &self,
        min_interval_ms: u64,
        callback: impl Fn(String) + Send + Sync + 'static,
    ) {
        struct FnSub<F>(F);
        impl<F: Fn(String) + Send + Sync> CoreEventSubscriber for FnSub<F> {
            fn on_notification(&self, event: CoreNotification) {
                let compact: OhosCoreNotification = event.into();
                if let Ok(json) = serde_json::to_string(&compact) {
                    (self.0)(json);
                }
            }
        }
        self.inner
            .subscribe_coalesced(min_interval_ms, Arc::new(FnSub(callback)));
    }

    pub fn discover_local_daemon_uri(&self) -> Option<String> {
        self.inner.discover_local_daemon_uri()
    }

    pub fn fleet_overview_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_overview()).map_err(|e| e.to_string())
    }

    pub fn bridge_info_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.bridge_info()).map_err(|e| e.to_string())
    }

    pub fn search_messages_json(&self, query: &str, limit: Option<u32>) -> Result<String, String> {
        serde_json::to_string(&self.inner.search_messages(query, limit.unwrap_or(20)))
            .map_err(|e| e.to_string())
    }

    pub fn export_agent_transcript_json(
        &self,
        host_id: &str,
        agent_id: &str,
        options_json: &str,
    ) -> Result<String, String> {
        let opts: ExportOptions = if options_json.trim().is_empty() {
            ExportOptions::default()
        } else {
            serde_json::from_str(options_json).map_err(|e| e.to_string())?
        };
        let exported = self
            .inner
            .export_agent_transcript(host_id, agent_id, &opts)?;
        serde_json::to_string(&exported).map_err(|e| e.to_string())
    }

    pub fn list_agent_code_blocks_json(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        let blocks = self.inner.list_agent_code_blocks(host_id, agent_id)?;
        serde_json::to_string(&blocks).map_err(|e| e.to_string())
    }

    pub fn fleet_sidebar_by_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: FleetSidebarRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.fleet_sidebar_by_request(&req)).map_err(|e| e.to_string())
    }

    pub fn agent_stage_delta_by_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: AgentStageDeltaRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.agent_stage_delta_by_request(&req))
            .map_err(|e| e.to_string())
    }

    pub fn create_session_json(&self, req_json: &str) -> Result<String, String> {
        let req: CreateSessionRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        Ok(self.inner.create_session(req))
    }

    pub fn send_prompt_json(&self, req_json: &str) -> Result<(), String> {
        let req: SendPromptRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        self.inner.send_prompt_request(req);
        Ok(())
    }

    pub fn update_host_config_request_json(&self, req_json: &str) -> Result<String, String> {
        let req: UpdateHostConfigRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let card = self
            .inner
            .update_host_config_request(req)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
    }

    pub fn resolve_model_selection_json(&self, req_json: &str) -> Result<String, String> {
        let req: ModelSelectionRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.resolve_model_selection(&req)).map_err(|e| e.to_string())
    }

    pub fn slash_commands_json(
        &self,
        platform: Option<&str>,
        query: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .slash_commands(parse_client_platform(platform), query),
        )
        .map_err(|e| e.to_string())
    }

    pub fn compose_prompt_json(&self, req_json: &str) -> Result<String, String> {
        let req: ComposePromptRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.compose_prompt_for_host(&req)).map_err(|e| e.to_string())
    }

    pub fn rank_mention_candidates_json(&self, req_json: &str) -> Result<String, String> {
        let req: MentionRankRequest = serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        serde_json::to_string(&self.inner.rank_mention_candidates(&req)).map_err(|e| e.to_string())
    }

    pub fn host_detail_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.host_detail(host_id)).map_err(|e| e.to_string())
    }

    pub fn agent_stage_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_stage(host_id, agent_id)).map_err(|e| e.to_string())
    }

    pub fn agent_stage_delta_json(
        &self,
        host_id: &str,
        agent_id: &str,
        since_revision: u64,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .agent_stage_delta(host_id, agent_id, since_revision),
        )
        .map_err(|e| e.to_string())
    }

    pub fn fleet_sidebar_json(
        &self,
        sort: Option<&str>,
        filter: Option<&str>,
        query: Option<&str>,
        active_host_id: Option<&str>,
        active_agent_id: Option<&str>,
        active_cwd: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.fleet_sidebar(
            parse_sidebar_sort(sort),
            parse_sidebar_filter(filter),
            query.unwrap_or(""),
            active_host_id,
            active_agent_id,
            active_cwd,
        ))
        .map_err(|e| e.to_string())
    }

    pub fn mark_agent_seen(&self, host_id: &str, agent_id: &str, seq: u64) {
        self.inner.mark_agent_seen(host_id, agent_id, seq);
    }

    pub fn mark_agent_unread(&self, host_id: &str, agent_id: &str) {
        self.inner.mark_agent_unread(host_id, agent_id);
    }

    pub fn set_agent_pinned(&self, host_id: &str, agent_id: &str, pinned: bool) {
        self.inner.set_agent_pinned(host_id, agent_id, pinned);
    }

    pub fn toggle_pin_agent(&self, host_id: &str, agent_id: &str) -> bool {
        self.inner.toggle_pin_agent(host_id, agent_id)
    }

    pub fn seed_thread_prefs_json(&self, prefs_json: &str) -> Result<(), String> {
        let seeds: Vec<OhosThreadPrefSeed> =
            serde_json::from_str(prefs_json).map_err(|e| e.to_string())?;
        let prefs: Vec<ClientThreadPref> = seeds
            .into_iter()
            .map(|s| ClientThreadPref {
                host_id: s.host_id,
                agent_id: s.agent_id,
                seen_seq: s.seen_seq,
                is_pinned: s.is_pinned,
                pinned_order: s.pinned_order,
                updated_at_ms: s.updated_at_ms,
            })
            .collect();
        self.inner.seed_thread_prefs(prefs);
        Ok(())
    }

    pub fn fallback_provider_catalog_json(&self, provider: &str) -> Result<String, String> {
        serde_json::to_string(&core_fallback_provider_catalog(parse_provider_str(
            provider,
        )))
        .map_err(|e| e.to_string())
    }

    pub fn fallback_provider_catalogs_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.fallback_provider_catalogs()).map_err(|e| e.to_string())
    }

    pub fn format_catalog_model_label(
        &self,
        catalog_json: &str,
        raw_model_id: Option<&str>,
    ) -> Result<String, String> {
        let catalog: ProviderModelCatalog =
            serde_json::from_str(catalog_json).map_err(|e| e.to_string())?;
        Ok(core_format_catalog_model_label(&catalog, raw_model_id))
    }

    pub fn format_effort_label(&self, effort: &str) -> String {
        match parse_effort_opt(Some(effort)) {
            Some(eff) => core_format_effort_label(eff).to_string(),
            None => effort.trim().to_string(),
        }
    }

    pub fn default_effort_description(&self, effort: &str) -> String {
        match parse_effort_opt(Some(effort)) {
            Some(eff) => core_default_effort_description(eff).to_string(),
            None => String::new(),
        }
    }

    pub fn resolve_supported_effort_for_model(
        &self,
        spec_json: Option<&str>,
        desired: Option<&str>,
        catalog_default: Option<&str>,
    ) -> Result<Option<String>, String> {
        let spec: Option<ProviderModelSpec> = match spec_json.map(str::trim) {
            Some(s) if !s.is_empty() && s != "null" => {
                Some(serde_json::from_str(s).map_err(|e| e.to_string())?)
            }
            _ => None,
        };
        Ok(core_resolve_supported_effort_for_model(
            spec.as_ref(),
            parse_effort_opt(desired),
            parse_effort_opt(catalog_default),
        )
        .map(|e| effort_to_slug(e).to_string()))
    }

    pub fn agent_timeline_json(&self, host_id: &str, agent_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.agent_timeline(host_id, agent_id))
            .map_err(|e| e.to_string())
    }

    pub fn agent_timeline_delta_json(
        &self,
        host_id: &str,
        agent_id: &str,
        since_seq: u64,
    ) -> Result<String, String> {
        serde_json::to_string(
            &self
                .inner
                .agent_timeline_delta(host_id, agent_id, since_seq),
        )
        .map_err(|e| e.to_string())
    }

    pub fn describe_tool_item_json(
        &self,
        item_json: &str,
        cwd: Option<&str>,
    ) -> Result<Option<String>, String> {
        let item: TimelineItemView = serde_json::from_str(item_json).map_err(|e| e.to_string())?;
        match describe_tool_step(&item, cwd) {
            Some(desc) => serde_json::to_string(&desc)
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    pub fn git_review_json(
        &self,
        host_id: &str,
        cwd: &str,
        path: Option<&str>,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.git_review(host_id, cwd, path)).map_err(|e| e.to_string())
    }

    pub fn terminal_screen_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.terminal_screen(host_id)).map_err(|e| e.to_string())
    }

    pub fn terminal_screen_slot_json(&self, host_id: &str, slot: u8) -> Result<String, String> {
        serde_json::to_string(&self.inner.terminal_screen_slot(host_id, slot))
            .map_err(|e| e.to_string())
    }

    pub fn pair_host_json(&self, pairing_uri: &str) -> Result<String, String> {
        let card = self
            .inner
            .pair_host(pairing_uri)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
    }

    pub fn remove_host(&self, host_id: &str) -> Result<(), String> {
        self.inner.remove_host(host_id).map_err(|e| e.to_string())
    }

    pub fn update_host_relay(&self, host_id: &str, relay_url: Option<&str>) -> Result<(), String> {
        self.inner
            .update_host_relay(host_id, relay_url)
            .map_err(|e| e.to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_host_config_json(
        &self,
        host_id: &str,
        custom_name: Option<String>,
        route_mode: Option<&str>,
        ssh_alias: Option<String>,
        remote_daemon_port: Option<u16>,
        custom_direct_endpoint: Option<String>,
        relay_url: Option<String>,
        reconnect: bool,
    ) -> Result<String, String> {
        let parsed_route = route_mode.map(|rm| HostRouteMode::from_opt_str(Some(rm)));
        let card = self
            .inner
            .update_host_config(
                host_id,
                custom_name,
                parsed_route,
                ssh_alias,
                remote_daemon_port,
                custom_direct_endpoint,
                relay_url,
                reconnect,
            )
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&card).map_err(|e| e.to_string())
    }

    pub fn upsert_project_json(
        &self,
        project_id: Option<&str>,
        host_id: &str,
        name: &str,
        directories_json: &str,
    ) -> Result<String, String> {
        let dirs = parse_string_vec_json(Some(directories_json))?;
        let proj = self
            .inner
            .upsert_project(project_id, host_id, name, dirs)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&proj).map_err(|e| e.to_string())
    }

    pub fn remove_project(&self, project_id: &str) -> Result<(), String> {
        self.inner
            .remove_project(project_id)
            .map_err(|e| e.to_string())
    }

    pub fn list_directory(&self, host_id: &str, path: Option<String>) {
        self.inner.list_directory(host_id, path);
    }

    pub fn connect_all_hosts(&self) {
        self.inner.connect_all_hosts();
    }

    pub fn connect_host(&self, host_id: &str) {
        self.inner.connect_host_async(host_id.to_string());
    }

    pub fn refresh_host(&self, host_id: &str) {
        self.inner.refresh_host(host_id);
    }

    pub fn refresh_quotas(&self, host_id: &str) {
        self.inner.refresh_quotas(host_id);
    }

    pub fn refresh_provider_models(&self, host_id: &str, force_refresh: bool) {
        self.inner.refresh_provider_models(host_id, force_refresh);
    }

    pub fn create_agent(
        &self,
        host_id: &str,
        cwd: &str,
        provider_str: &str,
        mode_str: &str,
        initial_prompt: &str,
        worktree_branch: Option<&str>,
    ) {
        let provider = parse_provider_str(provider_str);
        let mode = parse_mode_str(mode_str);
        self.inner.create_agent(
            host_id,
            cwd,
            provider,
            mode,
            initial_prompt,
            worktree_branch,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_full(
        &self,
        host_id: &str,
        cwd: &str,
        extra_dirs_json: Option<&str>,
        provider_str: &str,
        acp_agent_id: Option<&str>,
        model: Option<String>,
        effort_str: Option<&str>,
        mode_str: &str,
        initial_prompt: &str,
        initial_display_prompt: Option<String>,
        initial_badges_json: Option<&str>,
        worktree_branch: Option<&str>,
    ) -> Result<String, String> {
        let extra_dirs = parse_string_vec_json(extra_dirs_json)?;
        let initial_badges = parse_badges_json(initial_badges_json)?;
        let opt_id = self.inner.create_agent_with_config(CreateAgentRequest {
            host_id,
            cwd,
            extra_dirs,
            provider: parse_provider_str(provider_str),
            acp_agent_id: acp_agent_id.map(ToOwned::to_owned),
            model,
            effort: parse_effort_opt(effort_str),
            fast_mode: None,
            mode: parse_mode_str(mode_str),
            initial_prompt,
            initial_display_prompt,
            initial_badges,
            worktree_branch,
            workspace_id: None,
            handle: None,
            auto_cluster: None,
        });
        Ok(opt_id)
    }

    pub fn open_agent_timeline(&self, host_id: &str, agent_id: &str) {
        self.inner.open_agent_timeline(host_id, agent_id);
    }

    pub fn send_prompt(&self, host_id: &str, agent_id: &str, prompt: &str) {
        self.inner.send_prompt(host_id, agent_id, prompt);
    }

    pub fn send_prompt_with_meta(
        &self,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges_json: Option<&str>,
    ) -> Result<(), String> {
        let badges = parse_badges_json(badges_json)?;
        self.inner
            .send_prompt_with_meta(host_id, agent_id, prompt, display_prompt, badges);
        Ok(())
    }

    pub fn enqueue_prompt(
        &self,
        host_id: &str,
        agent_id: &str,
        prompt: &str,
        display_prompt: Option<String>,
        badges_json: Option<&str>,
        queue_id: Option<String>,
    ) -> Result<String, String> {
        let badges = parse_badges_json(badges_json)?;
        Ok(self
            .inner
            .enqueue_prompt(host_id, agent_id, prompt, display_prompt, badges, queue_id))
    }

    pub fn remove_queued_prompt(&self, host_id: &str, agent_id: &str, queue_id: &str) {
        self.inner.remove_queued_prompt(host_id, agent_id, queue_id);
    }

    pub fn flush_queued_prompt_now(&self, host_id: &str, agent_id: &str, queue_id: &str) {
        self.inner
            .flush_queued_prompt_now(host_id, agent_id, queue_id);
    }

    pub fn reorder_queued_prompts(
        &self,
        host_id: &str,
        agent_id: &str,
        queue_ids_json: &str,
    ) -> Result<(), String> {
        let queue_ids: Vec<String> = serde_json::from_str(queue_ids_json)
            .map_err(|e| format!("invalid queue_ids_json: {e}"))?;
        self.inner
            .reorder_queued_prompts(host_id, agent_id, queue_ids);
        Ok(())
    }

    pub fn interrupt_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.interrupt_agent(host_id, agent_id);
    }

    pub fn set_session_mode(&self, host_id: &str, agent_id: &str, mode_str: &str) {
        self.inner
            .set_session_mode(host_id, agent_id, parse_mode_str(mode_str));
    }

    pub fn set_agent_config(
        &self,
        host_id: &str,
        agent_id: &str,
        model: Option<String>,
        effort_str: Option<&str>,
    ) {
        self.inner
            .set_agent_config(host_id, agent_id, model, parse_effort_opt(effort_str));
    }

    pub fn set_agent_fast_mode(&self, host_id: &str, agent_id: &str, fast_mode: bool) {
        self.inner.set_agent_fast_mode(host_id, agent_id, fast_mode);
    }

    pub fn switch_agent_provider(
        &self,
        host_id: &str,
        agent_id: &str,
        provider_str: &str,
        model: Option<String>,
        effort_str: Option<&str>,
        acp_agent_id: Option<String>,
    ) {
        self.inner.switch_agent_provider_with_config(
            host_id,
            agent_id,
            parse_provider_str(provider_str),
            model,
            parse_effort_opt(effort_str),
            acp_agent_id,
        );
    }

    pub fn archive_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.archive_agent(host_id, agent_id);
    }

    pub fn unarchive_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.unarchive_agent(host_id, agent_id);
    }

    pub fn delete_agent(&self, host_id: &str, agent_id: &str) {
        self.inner.delete_agent(host_id, agent_id);
    }

    pub fn resolve_permission(
        &self,
        host_id: &str,
        agent_id: &str,
        permission_request_id: &str,
        decision: &str,
    ) {
        self.inner
            .resolve_permission(host_id, agent_id, permission_request_id, decision);
    }

    pub fn answer_question(
        &self,
        host_id: &str,
        agent_id: &str,
        question_request_id: &str,
        answers_json: &str,
    ) -> Result<(), String> {
        let answers: Vec<Vec<String>> =
            serde_json::from_str(answers_json).map_err(|e| e.to_string())?;
        self.inner
            .answer_question(host_id, agent_id, question_request_id, answers);
        Ok(())
    }

    pub fn fetch_git_status(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_git_status(host_id, cwd);
    }

    pub fn fetch_git_diff(&self, host_id: &str, cwd: &str, path: Option<&str>, staged: bool) {
        self.inner.fetch_git_diff(host_id, cwd, path, staged);
    }

    pub fn fetch_workspace_files(&self, host_id: &str, cwd: &str) {
        self.inner.fetch_workspace_files(host_id, cwd);
    }

    pub fn workspace_files_json(&self, host_id: &str, cwd: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_files(host_id, cwd)).map_err(|e| e.to_string())
    }

    pub fn stage_attachment_json(
        &self,
        host_id: &str,
        filename: &str,
        mime_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<String, String> {
        let staged = self
            .inner
            .stage_attachment(host_id, filename, mime_type, bytes)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&staged).map_err(|e| e.to_string())
    }

    pub fn start_terminal(&self, host_id: &str, cwd: &str, cols: u16, rows: u16) {
        self.inner.start_terminal(host_id, cwd, cols, rows);
    }

    pub fn attach_terminal(&self, host_id: &str, terminal_id: &str, slot: u8) {
        self.inner.attach_terminal(host_id, terminal_id, slot);
    }

    pub fn kill_terminal(&self, host_id: &str, terminal_id: &str) {
        self.inner.kill_terminal(host_id, terminal_id);
    }

    pub fn send_terminal_keys(&self, host_id: &str, keys: &str) {
        self.inner
            .send_terminal_input(host_id, keys.as_bytes().to_vec());
    }

    pub fn send_terminal_keys_slot(&self, host_id: &str, slot: u8, keys: &str) {
        self.inner
            .send_terminal_input_slot(host_id, slot, keys.as_bytes().to_vec());
    }

    pub fn resize_terminal(&self, host_id: &str, cols: u16, rows: u16) {
        self.inner.resize_terminal(host_id, cols, rows);
    }

    pub fn resize_terminal_slot(&self, host_id: &str, slot: u8, cols: u16, rows: u16) {
        self.inner.resize_terminal_slot(host_id, slot, cols, rows);
    }

    pub fn rewind_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
        to_before_seq: u64,
        restore_files: bool,
    ) -> Result<String, String> {
        let res =
            self.inner
                .rewind_agent_blocking(host_id, agent_id, to_before_seq, restore_files)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn fork_agent_json(&self, req_json: &str) -> Result<String, String> {
        let req: ForkAgentRequest =
            serde_json::from_str(req_json).map_err(|e| format!("invalid ForkAgentRequest: {e}"))?;
        let res = self.inner.fork_agent_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn preview_worktree_handoff_json(&self, req_json: &str) -> Result<String, String> {
        let req: PreviewWorktreeHandoffRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid PreviewWorktreeHandoffRequest: {e}"))?;
        let res = self.inner.preview_worktree_handoff_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn execute_worktree_handoff_json(&self, req_json: &str) -> Result<String, String> {
        let req: ExecuteWorktreeHandoffRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid ExecuteWorktreeHandoffRequest: {e}"))?;
        let res = self.inner.execute_worktree_handoff_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn read_workspace_file_json(
        &self,
        host_id: &str,
        cwd: &str,
        path: &str,
    ) -> Result<String, String> {
        let file = self
            .inner
            .read_workspace_file_blocking(host_id, cwd, path)?;
        serde_json::to_string(&file).map_err(|e| e.to_string())
    }

    pub fn write_workspace_file_json(&self, req_json: &str) -> Result<String, String> {
        let req: WriteWorkspaceFileRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid WriteWorkspaceFileRequest: {e}"))?;
        let res = self.inner.write_workspace_file_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn create_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: CreateWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid CreateWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.create_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn rename_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: RenameWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid RenameWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.rename_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn delete_workspace_entry_json(&self, req_json: &str) -> Result<String, String> {
        let req: DeleteWorkspaceEntryRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid DeleteWorkspaceEntryRequest: {e}"))?;
        let res = self.inner.delete_workspace_entry_blocking(req)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn build_side_question_context(
        &self,
        host_id: &str,
        agent_id: &str,
        max_chars: Option<usize>,
    ) -> String {
        self.inner
            .build_side_question_context(host_id, agent_id, max_chars)
    }

    pub fn detect_diagram_kind(&self, language: &str, source: &str) -> Option<String> {
        core_detect_diagram_kind(language, source).map(|kind| match kind {
            DiagramKind::Mermaid => "mermaid".to_string(),
            DiagramKind::Svg => "svg".to_string(),
            DiagramKind::Graphviz => "graphviz".to_string(),
        })
    }

    pub fn render_diagram_json(&self, req_json: &str) -> Result<String, String> {
        let req: RenderDiagramRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let rendered = core_render_diagram(&req);
        serde_json::to_string(&rendered).map_err(|e| e.to_string())
    }

    pub fn ask_side_question(
        &self,
        req_json: &str,
        on_event: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<String, String> {
        let payload: OhosAskSideQuestionPayload =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let context_summary = match payload.context_summary.filter(|s| !s.trim().is_empty()) {
            Some(ctx) => ctx,
            None => payload
                .agent_id
                .as_deref()
                .map(|ag_id| {
                    self.inner
                        .build_side_question_context(&payload.host_id, ag_id, Some(3000))
                })
                .unwrap_or_default(),
        };
        self.inner.ask_side_question_with_callback(
            AskSideQuestionRequest {
                host_id: payload.host_id,
                agent_id: payload.agent_id,
                cwd: payload.cwd,
                provider: parse_provider_str(&payload.provider),
                model: payload.model,
                question: payload.question,
                context_summary,
            },
            move |ev| {
                if let Ok(json) = serde_json::to_string(&ev) {
                    on_event(json);
                }
            },
        )
    }

    pub fn on_app_foreground(&self) {
        self.inner.on_app_foreground();
    }

    pub fn on_app_background(&self) {
        self.inner.on_app_background();
    }

    pub fn on_network_changed(&self) {
        self.inner.on_network_changed();
    }

    pub fn reorder_projects_json(&self, ordered_keys_json: &str) -> Result<String, String> {
        let keys = parse_string_vec_json(Some(ordered_keys_json))?;
        let saved = self
            .inner
            .reorder_projects(keys)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&saved).map_err(|e| e.to_string())
    }

    pub fn project_order_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.project_order()).map_err(|e| e.to_string())
    }

    pub fn move_agent_to_workspace(
        &self,
        host_id: &str,
        agent_id: &str,
        target_workspace_id: Option<String>,
    ) {
        self.inner
            .move_agent_to_workspace(host_id, agent_id, target_workspace_id);
    }

    pub fn merge_workspaces(
        &self,
        host_id: &str,
        source_workspace_id: &str,
        target_workspace_id: &str,
    ) {
        self.inner
            .merge_workspaces(host_id, source_workspace_id, target_workspace_id);
    }

    pub fn dismiss_cluster_suggestion(&self, host_id: &str, agent_id: &str) {
        self.inner.dismiss_cluster_suggestion(host_id, agent_id);
    }

    // --- Zero-Server E2EE Cloud Sync ---

    pub fn cloud_sync_status_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.cloud_sync_status()).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_generate_passphrase(&self) -> String {
        self.inner.cloud_sync_generate_passphrase()
    }

    pub fn cloud_sync_set_github_client_id_json(
        &self,
        client_id: Option<String>,
    ) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_set_github_client_id(client_id)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_set_auto_sync_json(&self, enabled: bool) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_set_auto_sync(enabled)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_start_github_device_flow_json(
        &self,
        client_id_override: Option<String>,
    ) -> Result<String, String> {
        let started = self
            .inner
            .cloud_sync_start_github_device_flow(client_id_override)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&started).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_poll_github_device_flow_json(
        &self,
        device_code: &str,
        client_id_override: Option<String>,
    ) -> Result<String, String> {
        let polled = self
            .inner
            .cloud_sync_poll_github_device_flow(device_code, client_id_override)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&polled).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_login_with_token_json(&self, token: &str) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_login_with_token(token)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_unlock_with_passphrase_json(
        &self,
        passphrase: &str,
    ) -> Result<String, String> {
        let res = self
            .inner
            .cloud_sync_unlock_with_passphrase(passphrase)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_now_json(&self) -> Result<String, String> {
        let res = self.inner.cloud_sync_now().map_err(|e| e.to_string())?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_logout_json(&self, clear_local_hosts: bool) -> Result<String, String> {
        let status = self
            .inner
            .cloud_sync_logout(clear_local_hosts)
            .map_err(|e| e.to_string())?;
        serde_json::to_string(&status).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_get_preferences_json(&self) -> Result<String, String> {
        serde_json::to_string(&self.inner.cloud_sync_get_preferences()).map_err(|e| e.to_string())
    }

    pub fn cloud_sync_update_preferences_json(&self, patch_json: &str) -> Result<(), String> {
        let patch: std::collections::HashMap<String, serde_json::Value> =
            serde_json::from_str(patch_json).map_err(|e| e.to_string())?;
        self.inner
            .cloud_sync_update_preferences(patch)
            .map_err(|e| e.to_string())
    }

    // --- Universal ACP Agent Registry & Probing ---

    pub fn list_acp_agents_json(&self, host_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.list_acp_agents(host_id)).map_err(|e| e.to_string())
    }

    pub fn refresh_acp_agents(&self, host_id: &str) {
        self.inner.refresh_acp_agents(host_id);
    }

    pub fn upsert_custom_acp_agent_json(
        &self,
        host_id: &str,
        spec_json: &str,
    ) -> Result<String, String> {
        let spec: AcpAgentSpec = serde_json::from_str(spec_json).map_err(|e| e.to_string())?;
        let agents = self.inner.upsert_custom_acp_agent_blocking(host_id, spec)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    pub fn delete_custom_acp_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<String, String> {
        let agents = self
            .inner
            .delete_custom_acp_agent_blocking(host_id, agent_id)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    pub fn test_acp_agent_json(
        &self,
        host_id: &str,
        agent_id: &str,
        cwd: Option<String>,
    ) -> Result<String, String> {
        let res = self.inner.test_acp_agent_blocking(host_id, agent_id, cwd)?;
        serde_json::to_string(&res).map_err(|e| e.to_string())
    }

    pub fn refresh_acp_registry_json(
        &self,
        host_id: &str,
        registry_url: Option<String>,
    ) -> Result<String, String> {
        let agents = self
            .inner
            .refresh_acp_registry_blocking(host_id, registry_url)?;
        serde_json::to_string(&agents).map_err(|e| e.to_string())
    }

    // --- First-Class Artifacts & SEP-1865 MCP Apps ---

    pub fn list_artifacts_json(
        &self,
        host_id: &str,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let artifacts = self
            .inner
            .list_artifacts_blocking(host_id, workspace_id, agent_id)?;
        serde_json::to_string(&artifacts).map_err(|e| e.to_string())
    }

    pub fn get_artifact_json(
        &self,
        host_id: &str,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Result<String, String> {
        let detail = self
            .inner
            .get_artifact_blocking(host_id, artifact_id, version)?;
        serde_json::to_string(&detail).map_err(|e| e.to_string())
    }

    pub fn upsert_artifact_json(&self, req_json: &str) -> Result<String, String> {
        let req: UpsertArtifactRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let summary = self.inner.upsert_artifact_blocking(req)?;
        serde_json::to_string(&summary).map_err(|e| e.to_string())
    }

    pub fn delete_artifact(&self, host_id: &str, artifact_id: &str) -> Result<String, String> {
        self.inner.delete_artifact_blocking(host_id, artifact_id)
    }

    // --- Mission Workspace Shared Board (`BOARD.md`), Events & Peers ---

    pub fn fetch_workspace_board(&self, host_id: &str, workspace_id: &str) {
        self.inner.fetch_workspace_board(host_id, workspace_id);
    }

    pub fn workspace_board_json(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_board(host_id, workspace_id))
            .map_err(|e| e.to_string())
    }

    pub fn update_workspace_board(
        &self,
        host_id: &str,
        workspace_id: &str,
        content: String,
        section: Option<String>,
        updated_by: Option<String>,
    ) {
        self.inner
            .update_workspace_board(host_id, workspace_id, content, section, updated_by);
    }

    pub fn fetch_workspace_events(&self, host_id: &str, workspace_id: &str, limit: Option<u32>) {
        self.inner
            .fetch_workspace_events(host_id, workspace_id, limit);
    }

    pub fn workspace_events_json(
        &self,
        host_id: &str,
        workspace_id: &str,
    ) -> Result<String, String> {
        serde_json::to_string(&self.inner.workspace_events(host_id, workspace_id))
            .map_err(|e| e.to_string())
    }

    pub fn spawn_workspace_peer_json(&self, req_json: &str) -> Result<String, String> {
        let req: SpawnWorkspacePeerRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid SpawnWorkspacePeerRequest: {e}"))?;
        Ok(self.inner.spawn_workspace_peer(req))
    }

    // --- Automations: Schedules, Conditional Timers, Goals, Monitors & Workflows ---

    pub fn create_schedule_json(&self, req_json: &str) -> Result<(), String> {
        let payload: OhosCreateSchedulePayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid create schedule payload: {e}"))?;
        let target = match payload
            .existing_agent_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
        {
            Some(agent_id) => ScheduleTarget::ExistingAgent { agent_id },
            None => ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
                cwd: if payload.cwd.trim().is_empty() {
                    "~".to_string()
                } else {
                    payload.cwd
                },
                workspace_id: payload.workspace_id,
                provider: parse_provider_str(&payload.provider),
                acp_agent_id: payload.acp_agent_id,
                model: payload.model,
                effort: parse_effort_opt(payload.effort.as_deref()),
                fast_mode: payload.fast_mode,
                mode: parse_mode_str(&payload.mode),
                profile_id: None,
                worktree: payload.worktree,
                base_ref: payload.base_ref,
                archive_on_finish: payload.archive_on_finish,
                background: payload.background,
            }),
        };
        self.inner.create_schedule(
            &payload.host_id,
            payload.name,
            payload.cron_expr,
            payload.timezone,
            payload.prompt,
            target,
            payload.max_runs,
        );
        Ok(())
    }

    pub fn update_schedule_json(&self, req_json: &str) -> Result<(), String> {
        let payload: OhosUpdateSchedulePayload = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid update schedule payload: {e}"))?;
        self.inner.update_schedule(
            &payload.host_id,
            &payload.schedule_id,
            payload.name,
            payload.cron_expr,
            payload.prompt,
            payload.max_runs,
            payload.archive_on_finish,
        );
        Ok(())
    }

    pub fn pause_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.pause_schedule(host_id, schedule_id);
    }

    pub fn resume_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.resume_schedule(host_id, schedule_id);
    }

    pub fn run_schedule_once(&self, host_id: &str, schedule_id: &str) {
        self.inner.run_schedule_once(host_id, schedule_id);
    }

    pub fn delete_schedule(&self, host_id: &str, schedule_id: &str) {
        self.inner.delete_schedule(host_id, schedule_id);
    }

    pub fn fetch_schedule_runs(&self, host_id: &str, schedule_id: &str, limit: Option<u32>) {
        self.inner.fetch_schedule_runs(host_id, schedule_id, limit);
    }

    pub fn schedule_runs_json(&self, host_id: &str, schedule_id: &str) -> Result<String, String> {
        serde_json::to_string(&self.inner.schedule_runs(host_id, schedule_id))
            .map_err(|e| e.to_string())
    }

    pub fn list_conditional_timers_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
        include_resolved: bool,
    ) -> Result<String, String> {
        let timers = block_on_core_rpc(self.inner.list_conditional_timers_on_host(
            host_id,
            agent_id,
            include_resolved,
        ))?;
        serde_json::to_string(&timers).map_err(|e| e.to_string())
    }

    pub fn cancel_conditional_timer_json(
        &self,
        host_id: &str,
        timer_id: &str,
    ) -> Result<String, String> {
        let record = block_on_core_rpc(
            self.inner
                .cancel_conditional_timer_on_host(host_id, timer_id),
        )?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn fire_conditional_timer_now_json(
        &self,
        host_id: &str,
        timer_id: &str,
    ) -> Result<String, String> {
        let record = block_on_core_rpc(
            self.inner
                .fire_conditional_timer_now_on_host(host_id, timer_id),
        )?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_goal_runs_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let goals = block_on_core_rpc(self.inner.list_goal_runs_on_host(host_id, agent_id))?;
        serde_json::to_string(&goals).map_err(|e| e.to_string())
    }

    pub fn control_goal_run_json(&self, req_json: &str) -> Result<String, String> {
        let req: ControlGoalRunRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid ControlGoalRunRequest: {e}"))?;
        let record = block_on_core_rpc(self.inner.control_goal_run_on_host(req))?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_monitor_watches_json(
        &self,
        host_id: &str,
        agent_id: Option<String>,
    ) -> Result<String, String> {
        let monitors =
            block_on_core_rpc(self.inner.list_monitor_watches_on_host(host_id, agent_id))?;
        serde_json::to_string(&monitors).map_err(|e| e.to_string())
    }

    pub fn stop_monitor_watch_json(&self, req_json: &str) -> Result<String, String> {
        let req: StopMonitorWatchRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid StopMonitorWatchRequest: {e}"))?;
        let record = block_on_core_rpc(self.inner.stop_monitor_watch_on_host(req))?;
        serde_json::to_string(&record).map_err(|e| e.to_string())
    }

    pub fn list_workflow_runs_json(
        &self,
        host_id: &str,
        include_completed: bool,
    ) -> Result<String, String> {
        let runs = block_on_core_rpc(
            self.inner
                .list_workflow_runs_on_host(host_id, include_completed),
        )?;
        serde_json::to_string(&runs).map_err(|e| e.to_string())
    }

    pub fn cancel_workflow_run_json(&self, req_json: &str) -> Result<String, String> {
        let req: CancelWorkflowRunRequest = serde_json::from_str(req_json)
            .map_err(|e| format!("invalid CancelWorkflowRunRequest: {e}"))?;
        let run = block_on_core_rpc(self.inner.cancel_workflow_run_on_host(req))?;
        serde_json::to_string(&run).map_err(|e| e.to_string())
    }

    // --- Native Mobile Background Push (APNs / FCM / HarmonyOS Push Kit) ---

    pub fn register_push_device_json(
        &self,
        host_id: &str,
        req_json: &str,
    ) -> Result<String, String> {
        let req: RegisterPushDeviceRequest =
            serde_json::from_str(req_json).map_err(|e| e.to_string())?;
        let snap = self.inner.register_push_device_blocking(host_id, req)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }

    pub fn unregister_push_device_json(
        &self,
        host_id: &str,
        device_id: &str,
    ) -> Result<String, String> {
        let snap = self
            .inner
            .unregister_push_device_blocking(host_id, device_id)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }

    pub fn list_push_devices_json(&self, host_id: &str) -> Result<String, String> {
        let snap = self.inner.list_push_devices_blocking(host_id)?;
        serde_json::to_string(&snap).map_err(|e| e.to_string())
    }
}

#[cfg(target_env = "ohos")]
mod ohos_napi {
    use std::sync::Arc;

    use napi_derive_ohos::napi;
    use napi_ohos::bindgen_prelude::*;
    use napi_ohos::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};

    use super::OhosCoreFacade;

    #[napi]
    pub struct OrbitHarmonyCore {
        facade: OhosCoreFacade,
    }

    pub struct StageAttachmentTask {
        facade: OhosCoreFacade,
        host_id: String,
        filename: String,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    }

    pub struct RegisterPushDeviceTask {
        facade: OhosCoreFacade,
        host_id: String,
        req_json: String,
    }

    impl Task for RegisterPushDeviceTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .register_push_device_json(&self.host_id, &self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct UnregisterPushDeviceTask {
        facade: OhosCoreFacade,
        host_id: String,
        device_id: String,
    }

    impl Task for UnregisterPushDeviceTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .unregister_push_device_json(&self.host_id, &self.device_id)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListPushDevicesTask {
        facade: OhosCoreFacade,
        host_id: String,
    }

    impl Task for ListPushDevicesTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_push_devices_json(&self.host_id)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    impl Task for StageAttachmentTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            let bytes = std::mem::take(&mut self.bytes);
            self.facade
                .stage_attachment_json(
                    &self.host_id,
                    &self.filename,
                    self.mime_type.as_deref(),
                    bytes,
                )
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct RewindAgentTask {
        facade: OhosCoreFacade,
        host_id: String,
        agent_id: String,
        to_before_seq: u64,
        restore_files: bool,
    }

    impl Task for RewindAgentTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .rewind_agent_json(
                    &self.host_id,
                    &self.agent_id,
                    self.to_before_seq,
                    self.restore_files,
                )
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ForkAgentTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for ForkAgentTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .fork_agent_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct PreviewWorktreeHandoffTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for PreviewWorktreeHandoffTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .preview_worktree_handoff_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ExecuteWorktreeHandoffTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for ExecuteWorktreeHandoffTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .execute_worktree_handoff_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ReadWorkspaceFileTask {
        facade: OhosCoreFacade,
        host_id: String,
        cwd: String,
        path: String,
    }

    impl Task for ReadWorkspaceFileTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .read_workspace_file_json(&self.host_id, &self.cwd, &self.path)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct WriteWorkspaceFileTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for WriteWorkspaceFileTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .write_workspace_file_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CreateWorkspaceEntryTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for CreateWorkspaceEntryTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .create_workspace_entry_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct RenameWorkspaceEntryTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for RenameWorkspaceEntryTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .rename_workspace_entry_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct DeleteWorkspaceEntryTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for DeleteWorkspaceEntryTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .delete_workspace_entry_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct RenderDiagramTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for RenderDiagramTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .render_diagram_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListConditionalTimersTask {
        facade: OhosCoreFacade,
        host_id: String,
        agent_id: Option<String>,
        include_resolved: bool,
    }

    impl Task for ListConditionalTimersTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_conditional_timers_json(
                    &self.host_id,
                    self.agent_id.clone(),
                    self.include_resolved,
                )
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CancelConditionalTimerTask {
        facade: OhosCoreFacade,
        host_id: String,
        timer_id: String,
    }

    impl Task for CancelConditionalTimerTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cancel_conditional_timer_json(&self.host_id, &self.timer_id)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct FireConditionalTimerNowTask {
        facade: OhosCoreFacade,
        host_id: String,
        timer_id: String,
    }

    impl Task for FireConditionalTimerNowTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .fire_conditional_timer_now_json(&self.host_id, &self.timer_id)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListGoalRunsTask {
        facade: OhosCoreFacade,
        host_id: String,
        agent_id: Option<String>,
    }

    impl Task for ListGoalRunsTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_goal_runs_json(&self.host_id, self.agent_id.clone())
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ControlGoalRunTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for ControlGoalRunTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .control_goal_run_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListMonitorWatchesTask {
        facade: OhosCoreFacade,
        host_id: String,
        agent_id: Option<String>,
    }

    impl Task for ListMonitorWatchesTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_monitor_watches_json(&self.host_id, self.agent_id.clone())
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct StopMonitorWatchTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for StopMonitorWatchTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .stop_monitor_watch_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListWorkflowRunsTask {
        facade: OhosCoreFacade,
        host_id: String,
        include_completed: bool,
    }

    impl Task for ListWorkflowRunsTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_workflow_runs_json(&self.host_id, self.include_completed)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CancelWorkflowRunTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for CancelWorkflowRunTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cancel_workflow_run_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CloudSyncStartGithubDeviceFlowTask {
        facade: OhosCoreFacade,
        client_id_override: Option<String>,
    }

    impl Task for CloudSyncStartGithubDeviceFlowTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cloud_sync_start_github_device_flow_json(self.client_id_override.clone())
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CloudSyncPollGithubDeviceFlowTask {
        facade: OhosCoreFacade,
        device_code: String,
        client_id_override: Option<String>,
    }

    impl Task for CloudSyncPollGithubDeviceFlowTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cloud_sync_poll_github_device_flow_json(
                    &self.device_code,
                    self.client_id_override.clone(),
                )
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CloudSyncLoginWithTokenTask {
        facade: OhosCoreFacade,
        token: String,
    }

    impl Task for CloudSyncLoginWithTokenTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cloud_sync_login_with_token_json(&self.token)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CloudSyncUnlockWithPassphraseTask {
        facade: OhosCoreFacade,
        passphrase: String,
    }

    impl Task for CloudSyncUnlockWithPassphraseTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cloud_sync_unlock_with_passphrase_json(&self.passphrase)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct CloudSyncNowTask {
        facade: OhosCoreFacade,
    }

    impl Task for CloudSyncNowTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .cloud_sync_now_json()
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct ListArtifactsTask {
        facade: OhosCoreFacade,
        host_id: String,
        workspace_id: Option<String>,
        agent_id: Option<String>,
    }

    impl Task for ListArtifactsTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .list_artifacts_json(
                    &self.host_id,
                    self.workspace_id.clone(),
                    self.agent_id.clone(),
                )
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct GetArtifactTask {
        facade: OhosCoreFacade,
        host_id: String,
        artifact_id: String,
        version: Option<u32>,
    }

    impl Task for GetArtifactTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .get_artifact_json(&self.host_id, &self.artifact_id, self.version)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct UpsertArtifactTask {
        facade: OhosCoreFacade,
        req_json: String,
    }

    impl Task for UpsertArtifactTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .upsert_artifact_json(&self.req_json)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    pub struct DeleteArtifactTask {
        facade: OhosCoreFacade,
        host_id: String,
        artifact_id: String,
    }

    impl Task for DeleteArtifactTask {
        type Output = String;
        type JsValue = String;

        fn compute(&mut self) -> Result<Self::Output> {
            self.facade
                .delete_artifact(&self.host_id, &self.artifact_id)
                .map_err(Error::from_reason)
        }

        fn resolve(&mut self, _env: Env, output: Self::Output) -> Result<Self::JsValue> {
            Ok(output)
        }
    }

    #[napi]
    impl OrbitHarmonyCore {
        #[napi(constructor)]
        pub fn new(db_path: String, auto_discover_local: Option<bool>) -> Result<Self> {
            let facade =
                OhosCoreFacade::open_with_options(&db_path, auto_discover_local.unwrap_or(false))
                    .map_err(Error::from_reason)?;
            Ok(Self { facade })
        }

        #[napi(factory)]
        pub fn open_with_options(db_path: String, auto_discover_local: bool) -> Result<Self> {
            let facade = OhosCoreFacade::open_with_options(&db_path, auto_discover_local)
                .map_err(Error::from_reason)?;
            Ok(Self { facade })
        }

        #[napi]
        pub fn subscribe_events(&self, callback: ThreadsafeFunction<String, ()>) -> Result<()> {
            let tsfn = Arc::new(callback);
            self.facade.subscribe_coalesced(24, move |payload_json| {
                let _ = tsfn.call(Ok(payload_json), ThreadsafeFunctionCallMode::NonBlocking);
            });
            Ok(())
        }

        #[napi]
        pub fn discover_local_daemon_uri(&self) -> Option<String> {
            self.facade.discover_local_daemon_uri()
        }

        #[napi]
        pub fn fleet_overview_json(&self) -> Result<String> {
            self.facade
                .fleet_overview_json()
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn bridge_info_json(&self) -> Result<String> {
            self.facade.bridge_info_json().map_err(Error::from_reason)
        }

        #[napi]
        pub fn search_messages_json(&self, query: String, limit: Option<u32>) -> Result<String> {
            self.facade
                .search_messages_json(&query, limit)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn export_agent_transcript_json(
            &self,
            host_id: String,
            agent_id: String,
            options_json: String,
        ) -> Result<String> {
            self.facade
                .export_agent_transcript_json(&host_id, &agent_id, &options_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn list_agent_code_blocks_json(
            &self,
            host_id: String,
            agent_id: String,
        ) -> Result<String> {
            self.facade
                .list_agent_code_blocks_json(&host_id, &agent_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn fleet_sidebar_by_request_json(&self, req_json: String) -> Result<String> {
            self.facade
                .fleet_sidebar_by_request_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_stage_delta_by_request_json(&self, req_json: String) -> Result<String> {
            self.facade
                .agent_stage_delta_by_request_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn create_session_json(&self, req_json: String) -> Result<String> {
            self.facade
                .create_session_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn send_prompt_json(&self, req_json: String) -> Result<()> {
            self.facade
                .send_prompt_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn update_host_config_request_json(&self, req_json: String) -> Result<String> {
            self.facade
                .update_host_config_request_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn resolve_model_selection_json(&self, req_json: String) -> Result<String> {
            self.facade
                .resolve_model_selection_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn slash_commands_json(
            &self,
            platform: Option<String>,
            query: Option<String>,
        ) -> Result<String> {
            self.facade
                .slash_commands_json(platform.as_deref(), query.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn compose_prompt_json(&self, req_json: String) -> Result<String> {
            self.facade
                .compose_prompt_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn rank_mention_candidates_json(&self, req_json: String) -> Result<String> {
            self.facade
                .rank_mention_candidates_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn host_detail_json(&self, host_id: String) -> Result<String> {
            self.facade
                .host_detail_json(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_stage_json(&self, host_id: String, agent_id: String) -> Result<String> {
            self.facade
                .agent_stage_json(&host_id, &agent_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_stage_delta_json(
            &self,
            host_id: String,
            agent_id: String,
            since_revision: i64,
        ) -> Result<String> {
            self.facade
                .agent_stage_delta_json(&host_id, &agent_id, since_revision.max(0) as u64)
                .map_err(Error::from_reason)
        }

        #[napi]
        #[allow(clippy::too_many_arguments)]
        pub fn fleet_sidebar_json(
            &self,
            sort: Option<String>,
            filter: Option<String>,
            query: Option<String>,
            active_host_id: Option<String>,
            active_agent_id: Option<String>,
            active_cwd: Option<String>,
        ) -> Result<String> {
            self.facade
                .fleet_sidebar_json(
                    sort.as_deref(),
                    filter.as_deref(),
                    query.as_deref(),
                    active_host_id.as_deref(),
                    active_agent_id.as_deref(),
                    active_cwd.as_deref(),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn mark_agent_seen(&self, host_id: String, agent_id: String, seq: i64) {
            self.facade
                .mark_agent_seen(&host_id, &agent_id, seq.max(0) as u64);
        }

        #[napi]
        pub fn mark_agent_unread(&self, host_id: String, agent_id: String) {
            self.facade.mark_agent_unread(&host_id, &agent_id);
        }

        #[napi]
        pub fn set_agent_pinned(&self, host_id: String, agent_id: String, pinned: bool) {
            self.facade.set_agent_pinned(&host_id, &agent_id, pinned);
        }

        #[napi]
        pub fn toggle_pin_agent(&self, host_id: String, agent_id: String) -> bool {
            self.facade.toggle_pin_agent(&host_id, &agent_id)
        }

        #[napi]
        pub fn seed_thread_prefs_json(&self, prefs_json: String) -> Result<()> {
            self.facade
                .seed_thread_prefs_json(&prefs_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn fallback_provider_catalog_json(&self, provider: String) -> Result<String> {
            self.facade
                .fallback_provider_catalog_json(&provider)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn fallback_provider_catalogs_json(&self) -> Result<String> {
            self.facade
                .fallback_provider_catalogs_json()
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn format_catalog_model_label(
            &self,
            catalog_json: String,
            raw_model_id: Option<String>,
        ) -> Result<String> {
            self.facade
                .format_catalog_model_label(&catalog_json, raw_model_id.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn format_effort_label(&self, effort: String) -> String {
            self.facade.format_effort_label(&effort)
        }

        #[napi]
        pub fn default_effort_description(&self, effort: String) -> String {
            self.facade.default_effort_description(&effort)
        }

        #[napi]
        pub fn resolve_supported_effort_for_model(
            &self,
            spec_json: Option<String>,
            desired: Option<String>,
            catalog_default: Option<String>,
        ) -> Result<Option<String>> {
            self.facade
                .resolve_supported_effort_for_model(
                    spec_json.as_deref(),
                    desired.as_deref(),
                    catalog_default.as_deref(),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_timeline_json(&self, host_id: String, agent_id: String) -> Result<String> {
            self.facade
                .agent_timeline_json(&host_id, &agent_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn agent_timeline_delta_json(
            &self,
            host_id: String,
            agent_id: String,
            since_seq: i64,
        ) -> Result<String> {
            self.facade
                .agent_timeline_delta_json(&host_id, &agent_id, since_seq.max(0) as u64)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn describe_tool_item_json(
            &self,
            item_json: String,
            cwd: Option<String>,
        ) -> Result<Option<String>> {
            self.facade
                .describe_tool_item_json(&item_json, cwd.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn git_review_json(
            &self,
            host_id: String,
            cwd: String,
            path: Option<String>,
        ) -> Result<String> {
            self.facade
                .git_review_json(&host_id, &cwd, path.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn terminal_screen_json(&self, host_id: String) -> Result<String> {
            self.facade
                .terminal_screen_json(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn terminal_screen_slot_json(&self, host_id: String, slot: u32) -> Result<String> {
            self.facade
                .terminal_screen_slot_json(&host_id, slot.max(1) as u8)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn pair_host_json(&self, pairing_uri: String) -> Result<String> {
            self.facade
                .pair_host_json(&pairing_uri)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn remove_host(&self, host_id: String) -> Result<()> {
            self.facade
                .remove_host(&host_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn update_host_relay(&self, host_id: String, relay_url: Option<String>) -> Result<()> {
            self.facade
                .update_host_relay(&host_id, relay_url.as_deref())
                .map_err(Error::from_reason)
        }

        #[napi]
        #[allow(clippy::too_many_arguments)]
        pub fn update_host_config_json(
            &self,
            host_id: String,
            custom_name: Option<String>,
            route_mode: Option<String>,
            ssh_alias: Option<String>,
            remote_daemon_port: Option<u32>,
            custom_direct_endpoint: Option<String>,
            relay_url: Option<String>,
            reconnect: Option<bool>,
        ) -> Result<String> {
            self.facade
                .update_host_config_json(
                    &host_id,
                    custom_name,
                    route_mode.as_deref(),
                    ssh_alias,
                    remote_daemon_port.map(|p| p as u16),
                    custom_direct_endpoint,
                    relay_url,
                    reconnect.unwrap_or(true),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn upsert_project_json(
            &self,
            project_id: Option<String>,
            host_id: String,
            name: String,
            directories_json: String,
        ) -> Result<String> {
            self.facade
                .upsert_project_json(project_id.as_deref(), &host_id, &name, &directories_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn remove_project(&self, project_id: String) -> Result<()> {
            self.facade
                .remove_project(&project_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn list_directory(&self, host_id: String, path: Option<String>) {
            self.facade.list_directory(&host_id, path);
        }

        #[napi]
        pub fn connect_all_hosts(&self) {
            self.facade.connect_all_hosts();
        }

        #[napi]
        pub fn connect_host(&self, host_id: String) {
            self.facade.connect_host(&host_id);
        }

        #[napi]
        pub fn refresh_host(&self, host_id: String) {
            self.facade.refresh_host(&host_id);
        }

        #[napi]
        pub fn refresh_quotas(&self, host_id: String) {
            self.facade.refresh_quotas(&host_id);
        }

        #[napi]
        pub fn refresh_provider_models(&self, host_id: String, force_refresh: bool) {
            self.facade.refresh_provider_models(&host_id, force_refresh);
        }

        #[napi]
        pub fn create_agent(
            &self,
            host_id: String,
            cwd: String,
            provider: String,
            mode: String,
            initial_prompt: String,
            worktree_branch: Option<String>,
        ) {
            self.facade.create_agent(
                &host_id,
                &cwd,
                &provider,
                &mode,
                &initial_prompt,
                worktree_branch.as_deref(),
            );
        }

        #[napi]
        #[allow(clippy::too_many_arguments)]
        pub fn create_agent_full(
            &self,
            host_id: String,
            cwd: String,
            extra_dirs_json: Option<String>,
            provider: String,
            acp_agent_id: Option<String>,
            model: Option<String>,
            effort: Option<String>,
            mode: String,
            initial_prompt: String,
            initial_display_prompt: Option<String>,
            initial_badges_json: Option<String>,
            worktree_branch: Option<String>,
        ) -> Result<String> {
            self.facade
                .create_agent_full(
                    &host_id,
                    &cwd,
                    extra_dirs_json.as_deref(),
                    &provider,
                    acp_agent_id.as_deref(),
                    model,
                    effort.as_deref(),
                    &mode,
                    &initial_prompt,
                    initial_display_prompt,
                    initial_badges_json.as_deref(),
                    worktree_branch.as_deref(),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn open_agent_timeline(&self, host_id: String, agent_id: String) {
            self.facade.open_agent_timeline(&host_id, &agent_id);
        }

        #[napi]
        pub fn send_prompt(&self, host_id: String, agent_id: String, prompt: String) {
            self.facade.send_prompt(&host_id, &agent_id, &prompt);
        }

        #[napi]
        pub fn send_prompt_with_meta(
            &self,
            host_id: String,
            agent_id: String,
            prompt: String,
            display_prompt: Option<String>,
            badges_json: Option<String>,
        ) -> Result<()> {
            self.facade
                .send_prompt_with_meta(
                    &host_id,
                    &agent_id,
                    &prompt,
                    display_prompt,
                    badges_json.as_deref(),
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn enqueue_prompt(
            &self,
            host_id: String,
            agent_id: String,
            prompt: String,
            display_prompt: Option<String>,
            badges_json: Option<String>,
            queue_id: Option<String>,
        ) -> Result<String> {
            self.facade
                .enqueue_prompt(
                    &host_id,
                    &agent_id,
                    &prompt,
                    display_prompt,
                    badges_json.as_deref(),
                    queue_id,
                )
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn remove_queued_prompt(&self, host_id: String, agent_id: String, queue_id: String) {
            self.facade
                .remove_queued_prompt(&host_id, &agent_id, &queue_id);
        }

        #[napi]
        pub fn flush_queued_prompt_now(&self, host_id: String, agent_id: String, queue_id: String) {
            self.facade
                .flush_queued_prompt_now(&host_id, &agent_id, &queue_id);
        }

        #[napi]
        pub fn reorder_queued_prompts(
            &self,
            host_id: String,
            agent_id: String,
            queue_ids_json: String,
        ) -> Result<()> {
            self.facade
                .reorder_queued_prompts(&host_id, &agent_id, &queue_ids_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn interrupt_agent(&self, host_id: String, agent_id: String) {
            self.facade.interrupt_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn set_session_mode(&self, host_id: String, agent_id: String, mode: String) {
            self.facade.set_session_mode(&host_id, &agent_id, &mode);
        }

        #[napi]
        pub fn set_agent_config(
            &self,
            host_id: String,
            agent_id: String,
            model: Option<String>,
            effort: Option<String>,
        ) {
            self.facade
                .set_agent_config(&host_id, &agent_id, model, effort.as_deref());
        }

        #[napi]
        pub fn set_agent_fast_mode(&self, host_id: String, agent_id: String, fast_mode: bool) {
            self.facade
                .set_agent_fast_mode(&host_id, &agent_id, fast_mode);
        }

        #[napi]
        pub fn switch_agent_provider(
            &self,
            host_id: String,
            agent_id: String,
            provider: String,
            model: Option<String>,
            effort: Option<String>,
            acp_agent_id: Option<String>,
        ) {
            self.facade.switch_agent_provider(
                &host_id,
                &agent_id,
                &provider,
                model,
                effort.as_deref(),
                acp_agent_id,
            );
        }

        #[napi]
        pub fn archive_agent(&self, host_id: String, agent_id: String) {
            self.facade.archive_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn unarchive_agent(&self, host_id: String, agent_id: String) {
            self.facade.unarchive_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn delete_agent(&self, host_id: String, agent_id: String) {
            self.facade.delete_agent(&host_id, &agent_id);
        }

        #[napi]
        pub fn resolve_permission(
            &self,
            host_id: String,
            agent_id: String,
            permission_request_id: String,
            decision: String,
        ) {
            self.facade
                .resolve_permission(&host_id, &agent_id, &permission_request_id, &decision);
        }

        #[napi]
        pub fn answer_question(
            &self,
            host_id: String,
            agent_id: String,
            question_request_id: String,
            answers_json: String,
        ) -> Result<()> {
            self.facade
                .answer_question(&host_id, &agent_id, &question_request_id, &answers_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn fetch_git_status(&self, host_id: String, cwd: String) {
            self.facade.fetch_git_status(&host_id, &cwd);
        }

        #[napi]
        pub fn fetch_git_diff(
            &self,
            host_id: String,
            cwd: String,
            path: Option<String>,
            staged: bool,
        ) {
            self.facade
                .fetch_git_diff(&host_id, &cwd, path.as_deref(), staged);
        }

        #[napi]
        pub fn fetch_workspace_files(&self, host_id: String, cwd: String) {
            self.facade.fetch_workspace_files(&host_id, &cwd);
        }

        #[napi]
        pub fn workspace_files_json(&self, host_id: String, cwd: String) -> Result<String> {
            self.facade
                .workspace_files_json(&host_id, &cwd)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn stage_attachment_json(
            &self,
            host_id: String,
            filename: String,
            mime_type: Option<String>,
            bytes: Uint8Array,
        ) -> AsyncTask<StageAttachmentTask> {
            AsyncTask::new(StageAttachmentTask {
                facade: self.facade.clone(),
                host_id,
                filename,
                mime_type,
                bytes: bytes.to_vec(),
            })
        }

        #[napi]
        pub fn start_terminal(&self, host_id: String, cwd: String, cols: u32, rows: u32) {
            self.facade
                .start_terminal(&host_id, &cwd, cols as u16, rows as u16);
        }

        #[napi]
        pub fn attach_terminal(&self, host_id: String, terminal_id: String, slot: u32) {
            self.facade
                .attach_terminal(&host_id, &terminal_id, slot as u8);
        }

        #[napi]
        pub fn kill_terminal(&self, host_id: String, terminal_id: String) {
            self.facade.kill_terminal(&host_id, &terminal_id);
        }

        #[napi]
        pub fn send_terminal_keys(&self, host_id: String, keys: String) {
            self.facade.send_terminal_keys(&host_id, &keys);
        }

        #[napi]
        pub fn send_terminal_keys_slot(&self, host_id: String, slot: u32, keys: String) {
            self.facade
                .send_terminal_keys_slot(&host_id, slot.max(1) as u8, &keys);
        }

        #[napi]
        pub fn resize_terminal(&self, host_id: String, cols: u32, rows: u32) {
            self.facade
                .resize_terminal(&host_id, cols as u16, rows as u16);
        }

        #[napi]
        pub fn resize_terminal_slot(&self, host_id: String, slot: u32, cols: u32, rows: u32) {
            self.facade
                .resize_terminal_slot(&host_id, slot.max(1) as u8, cols as u16, rows as u16);
        }

        #[napi]
        pub fn rewind_agent_json(
            &self,
            host_id: String,
            agent_id: String,
            to_before_seq: i64,
            restore_files: bool,
        ) -> AsyncTask<RewindAgentTask> {
            AsyncTask::new(RewindAgentTask {
                facade: self.facade.clone(),
                host_id,
                agent_id,
                to_before_seq: to_before_seq.max(0) as u64,
                restore_files,
            })
        }

        #[napi]
        pub fn fork_agent_json(&self, req_json: String) -> AsyncTask<ForkAgentTask> {
            AsyncTask::new(ForkAgentTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn preview_worktree_handoff_json(
            &self,
            req_json: String,
        ) -> AsyncTask<PreviewWorktreeHandoffTask> {
            AsyncTask::new(PreviewWorktreeHandoffTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn execute_worktree_handoff_json(
            &self,
            req_json: String,
        ) -> AsyncTask<ExecuteWorktreeHandoffTask> {
            AsyncTask::new(ExecuteWorktreeHandoffTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn read_workspace_file_json(
            &self,
            host_id: String,
            cwd: String,
            path: String,
        ) -> AsyncTask<ReadWorkspaceFileTask> {
            AsyncTask::new(ReadWorkspaceFileTask {
                facade: self.facade.clone(),
                host_id,
                cwd,
                path,
            })
        }

        #[napi]
        pub fn write_workspace_file_json(
            &self,
            req_json: String,
        ) -> AsyncTask<WriteWorkspaceFileTask> {
            AsyncTask::new(WriteWorkspaceFileTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn create_workspace_entry_json(
            &self,
            req_json: String,
        ) -> AsyncTask<CreateWorkspaceEntryTask> {
            AsyncTask::new(CreateWorkspaceEntryTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn rename_workspace_entry_json(
            &self,
            req_json: String,
        ) -> AsyncTask<RenameWorkspaceEntryTask> {
            AsyncTask::new(RenameWorkspaceEntryTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn delete_workspace_entry_json(
            &self,
            req_json: String,
        ) -> AsyncTask<DeleteWorkspaceEntryTask> {
            AsyncTask::new(DeleteWorkspaceEntryTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn build_side_question_context(
            &self,
            host_id: String,
            agent_id: String,
            max_chars: Option<u32>,
        ) -> String {
            self.facade.build_side_question_context(
                &host_id,
                &agent_id,
                max_chars.map(|v| v as usize),
            )
        }

        #[napi]
        pub fn ask_side_question(
            &self,
            req_json: String,
            callback: ThreadsafeFunction<String, ()>,
        ) -> Result<String> {
            let tsfn = Arc::new(callback);
            self.facade
                .ask_side_question(&req_json, move |payload_json| {
                    let _ = tsfn.call(Ok(payload_json), ThreadsafeFunctionCallMode::NonBlocking);
                })
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn on_app_foreground(&self) {
            self.facade.on_app_foreground();
        }

        #[napi]
        pub fn on_app_background(&self) {
            self.facade.on_app_background();
        }

        #[napi]
        pub fn on_network_changed(&self) {
            self.facade.on_network_changed();
        }

        #[napi]
        pub fn detect_diagram_kind(&self, language: String, source: String) -> Option<String> {
            self.facade.detect_diagram_kind(&language, &source)
        }

        #[napi]
        pub fn render_diagram_json(&self, req_json: String) -> AsyncTask<RenderDiagramTask> {
            AsyncTask::new(RenderDiagramTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn cloud_sync_status_json(&self) -> Result<String> {
            self.facade
                .cloud_sync_status_json()
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn cloud_sync_generate_passphrase(&self) -> String {
            self.facade.cloud_sync_generate_passphrase()
        }

        #[napi]
        pub fn cloud_sync_set_github_client_id_json(
            &self,
            client_id: Option<String>,
        ) -> Result<String> {
            self.facade
                .cloud_sync_set_github_client_id_json(client_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn cloud_sync_set_auto_sync_json(&self, enabled: bool) -> Result<String> {
            self.facade
                .cloud_sync_set_auto_sync_json(enabled)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn cloud_sync_start_github_device_flow_json(
            &self,
            client_id_override: Option<String>,
        ) -> AsyncTask<CloudSyncStartGithubDeviceFlowTask> {
            AsyncTask::new(CloudSyncStartGithubDeviceFlowTask {
                facade: self.facade.clone(),
                client_id_override,
            })
        }

        #[napi]
        pub fn cloud_sync_poll_github_device_flow_json(
            &self,
            device_code: String,
            client_id_override: Option<String>,
        ) -> AsyncTask<CloudSyncPollGithubDeviceFlowTask> {
            AsyncTask::new(CloudSyncPollGithubDeviceFlowTask {
                facade: self.facade.clone(),
                device_code,
                client_id_override,
            })
        }

        #[napi]
        pub fn cloud_sync_login_with_token_json(
            &self,
            token: String,
        ) -> AsyncTask<CloudSyncLoginWithTokenTask> {
            AsyncTask::new(CloudSyncLoginWithTokenTask {
                facade: self.facade.clone(),
                token,
            })
        }

        #[napi]
        pub fn cloud_sync_unlock_with_passphrase_json(
            &self,
            passphrase: String,
        ) -> AsyncTask<CloudSyncUnlockWithPassphraseTask> {
            AsyncTask::new(CloudSyncUnlockWithPassphraseTask {
                facade: self.facade.clone(),
                passphrase,
            })
        }

        #[napi]
        pub fn cloud_sync_now_json(&self) -> AsyncTask<CloudSyncNowTask> {
            AsyncTask::new(CloudSyncNowTask {
                facade: self.facade.clone(),
            })
        }

        #[napi]
        pub fn cloud_sync_logout_json(&self, clear_local_hosts: bool) -> Result<String> {
            self.facade
                .cloud_sync_logout_json(clear_local_hosts)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn cloud_sync_get_preferences_json(&self) -> Result<String> {
            self.facade
                .cloud_sync_get_preferences_json()
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn cloud_sync_update_preferences_json(&self, patch_json: String) -> Result<()> {
            self.facade
                .cloud_sync_update_preferences_json(&patch_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn list_artifacts_json(
            &self,
            host_id: String,
            workspace_id: Option<String>,
            agent_id: Option<String>,
        ) -> AsyncTask<ListArtifactsTask> {
            AsyncTask::new(ListArtifactsTask {
                facade: self.facade.clone(),
                host_id,
                workspace_id,
                agent_id,
            })
        }

        #[napi]
        pub fn get_artifact_json(
            &self,
            host_id: String,
            artifact_id: String,
            version: Option<u32>,
        ) -> AsyncTask<GetArtifactTask> {
            AsyncTask::new(GetArtifactTask {
                facade: self.facade.clone(),
                host_id,
                artifact_id,
                version,
            })
        }

        #[napi]
        pub fn upsert_artifact_json(&self, req_json: String) -> AsyncTask<UpsertArtifactTask> {
            AsyncTask::new(UpsertArtifactTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn delete_artifact(
            &self,
            host_id: String,
            artifact_id: String,
        ) -> AsyncTask<DeleteArtifactTask> {
            AsyncTask::new(DeleteArtifactTask {
                facade: self.facade.clone(),
                host_id,
                artifact_id,
            })
        }

        #[napi]
        pub fn fetch_workspace_board(&self, host_id: String, workspace_id: String) {
            self.facade.fetch_workspace_board(&host_id, &workspace_id);
        }

        #[napi]
        pub fn workspace_board_json(
            &self,
            host_id: String,
            workspace_id: String,
        ) -> Result<String> {
            self.facade
                .workspace_board_json(&host_id, &workspace_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn update_workspace_board(
            &self,
            host_id: String,
            workspace_id: String,
            content: String,
            section: Option<String>,
            updated_by: Option<String>,
        ) {
            self.facade.update_workspace_board(
                &host_id,
                &workspace_id,
                content,
                section,
                updated_by,
            );
        }

        #[napi]
        pub fn fetch_workspace_events(
            &self,
            host_id: String,
            workspace_id: String,
            limit: Option<u32>,
        ) {
            self.facade
                .fetch_workspace_events(&host_id, &workspace_id, limit);
        }

        #[napi]
        pub fn workspace_events_json(
            &self,
            host_id: String,
            workspace_id: String,
        ) -> Result<String> {
            self.facade
                .workspace_events_json(&host_id, &workspace_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn spawn_workspace_peer_json(&self, req_json: String) -> Result<String> {
            self.facade
                .spawn_workspace_peer_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn create_schedule_json(&self, req_json: String) -> Result<()> {
            self.facade
                .create_schedule_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn update_schedule_json(&self, req_json: String) -> Result<()> {
            self.facade
                .update_schedule_json(&req_json)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn pause_schedule(&self, host_id: String, schedule_id: String) {
            self.facade.pause_schedule(&host_id, &schedule_id);
        }

        #[napi]
        pub fn resume_schedule(&self, host_id: String, schedule_id: String) {
            self.facade.resume_schedule(&host_id, &schedule_id);
        }

        #[napi]
        pub fn run_schedule_once(&self, host_id: String, schedule_id: String) {
            self.facade.run_schedule_once(&host_id, &schedule_id);
        }

        #[napi]
        pub fn delete_schedule(&self, host_id: String, schedule_id: String) {
            self.facade.delete_schedule(&host_id, &schedule_id);
        }

        #[napi]
        pub fn fetch_schedule_runs(
            &self,
            host_id: String,
            schedule_id: String,
            limit: Option<u32>,
        ) {
            self.facade
                .fetch_schedule_runs(&host_id, &schedule_id, limit);
        }

        #[napi]
        pub fn schedule_runs_json(&self, host_id: String, schedule_id: String) -> Result<String> {
            self.facade
                .schedule_runs_json(&host_id, &schedule_id)
                .map_err(Error::from_reason)
        }

        #[napi]
        pub fn list_conditional_timers_json(
            &self,
            host_id: String,
            agent_id: Option<String>,
            include_resolved: bool,
        ) -> AsyncTask<ListConditionalTimersTask> {
            AsyncTask::new(ListConditionalTimersTask {
                facade: self.facade.clone(),
                host_id,
                agent_id,
                include_resolved,
            })
        }

        #[napi]
        pub fn cancel_conditional_timer_json(
            &self,
            host_id: String,
            timer_id: String,
        ) -> AsyncTask<CancelConditionalTimerTask> {
            AsyncTask::new(CancelConditionalTimerTask {
                facade: self.facade.clone(),
                host_id,
                timer_id,
            })
        }

        #[napi]
        pub fn fire_conditional_timer_now_json(
            &self,
            host_id: String,
            timer_id: String,
        ) -> AsyncTask<FireConditionalTimerNowTask> {
            AsyncTask::new(FireConditionalTimerNowTask {
                facade: self.facade.clone(),
                host_id,
                timer_id,
            })
        }

        #[napi]
        pub fn list_goal_runs_json(
            &self,
            host_id: String,
            agent_id: Option<String>,
        ) -> AsyncTask<ListGoalRunsTask> {
            AsyncTask::new(ListGoalRunsTask {
                facade: self.facade.clone(),
                host_id,
                agent_id,
            })
        }

        #[napi]
        pub fn control_goal_run_json(&self, req_json: String) -> AsyncTask<ControlGoalRunTask> {
            AsyncTask::new(ControlGoalRunTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn list_monitor_watches_json(
            &self,
            host_id: String,
            agent_id: Option<String>,
        ) -> AsyncTask<ListMonitorWatchesTask> {
            AsyncTask::new(ListMonitorWatchesTask {
                facade: self.facade.clone(),
                host_id,
                agent_id,
            })
        }

        #[napi]
        pub fn stop_monitor_watch_json(&self, req_json: String) -> AsyncTask<StopMonitorWatchTask> {
            AsyncTask::new(StopMonitorWatchTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn list_workflow_runs_json(
            &self,
            host_id: String,
            include_completed: bool,
        ) -> AsyncTask<ListWorkflowRunsTask> {
            AsyncTask::new(ListWorkflowRunsTask {
                facade: self.facade.clone(),
                host_id,
                include_completed,
            })
        }

        #[napi]
        pub fn cancel_workflow_run_json(
            &self,
            req_json: String,
        ) -> AsyncTask<CancelWorkflowRunTask> {
            AsyncTask::new(CancelWorkflowRunTask {
                facade: self.facade.clone(),
                req_json,
            })
        }

        #[napi]
        pub fn register_push_device_json(
            &self,
            host_id: String,
            req_json: String,
        ) -> AsyncTask<RegisterPushDeviceTask> {
            AsyncTask::new(RegisterPushDeviceTask {
                facade: self.facade.clone(),
                host_id,
                req_json,
            })
        }

        #[napi]
        pub fn unregister_push_device_json(
            &self,
            host_id: String,
            device_id: String,
        ) -> AsyncTask<UnregisterPushDeviceTask> {
            AsyncTask::new(UnregisterPushDeviceTask {
                facade: self.facade.clone(),
                host_id,
                device_id,
            })
        }

        #[napi]
        pub fn list_push_devices_json(&self, host_id: String) -> AsyncTask<ListPushDevicesTask> {
            AsyncTask::new(ListPushDevicesTask {
                facade: self.facade.clone(),
                host_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ohos_facade_exports_valid_fleet_json() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let json = facade.fleet_overview_json().expect("json");
        assert!(json.contains("\"hosts\":[]"));
        assert!(json.contains("\"projects\":[]"));
    }

    #[test]
    fn ohos_facade_agent_stage_project_and_lifecycle_roundtrip() {
        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_ohos".into(),
            hostname: "matebook-fold".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        let uri = offer.to_qr_uri().expect("uri");
        let paired_json = facade.pair_host_json(&uri).expect("pair");
        assert!(paired_json.contains("host_ohos"));

        let proj_json = facade
            .upsert_project_json(
                None,
                "host_ohos",
                "orbit-harmony",
                r#"["/workspace/orbit","/workspace/shared"]"#,
            )
            .expect("upsert project");
        assert!(proj_json.contains("orbit-harmony"));

        let opt_id = facade
            .create_agent_full(
                "host_ohos",
                "/workspace/orbit",
                Some(r#"["/workspace/shared"]"#),
                "claude_code",
                None,
                Some("claude-opus-5-5".into()),
                Some("high"),
                "accept_edits",
                "Build HarmonyOS NEXT stage UI",
                Some("/plan Build HarmonyOS NEXT stage UI".into()),
                Some(r#"[{"kind":"slash","label":"/plan"}]"#),
                None,
            )
            .expect("create_agent_full");
        assert!(opt_id.starts_with("opt_"));

        let stage_json = facade
            .agent_stage_json("host_ohos", &opt_id)
            .expect("stage json");
        let stage_val: serde_json::Value = serde_json::from_str(&stage_json).expect("valid json");
        assert_eq!(stage_val["host_id"], "host_ohos");
        assert_eq!(stage_val["turns"].as_array().map(Vec::len), Some(1));

        let delta_json = facade
            .agent_timeline_delta_json("host_ohos", &opt_id, 0)
            .expect("delta json");
        assert!(delta_json.contains("Build HarmonyOS NEXT stage UI"));

        facade.on_app_background();
        facade.on_app_foreground();
        facade.on_network_changed();

        facade
            .remove_project("host_ohos::/workspace/orbit")
            .expect("remove project");
    }

    #[test]
    fn ohos_facade_stage_delta_sidebar_prefs_catalog_and_compact_events() {
        let notif: OhosCoreNotification = CoreNotification::TerminalChanged {
            revision: 9,
            host_id: "h1".into(),
            slot: 2,
            raw_bytes: b"\x1b[31msecret\x1b[0m".to_vec(),
        }
        .into();
        let notif_json = serde_json::to_string(&notif).expect("serialize notif");
        assert!(!notif_json.contains("raw_bytes"));
        assert!(notif_json.contains("\"slot\":2"));

        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_ohos_2".into(),
            hostname: "ohos-dev".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        facade
            .pair_host_json(&offer.to_qr_uri().expect("uri"))
            .expect("pair");
        let opt_id = facade
            .create_agent_full(
                "host_ohos_2",
                "/repo/orbit",
                None,
                "claude_code",
                None,
                None,
                None,
                "default",
                "Inspect sidebar",
                None,
                None,
                None,
            )
            .expect("create");

        let d0: serde_json::Value = serde_json::from_str(
            &facade
                .agent_stage_delta_json("host_ohos_2", &opt_id, 0)
                .expect("delta0"),
        )
        .expect("json");
        assert_eq!(d0["reset"], true);
        let rev = d0["revision"].as_u64().expect("rev");

        let d_same: serde_json::Value = serde_json::from_str(
            &facade
                .agent_stage_delta_json("host_ohos_2", &opt_id, rev)
                .expect("delta same"),
        )
        .expect("json");
        assert_eq!(d_same["changed_turns"].as_array().map(Vec::len), Some(0));

        assert!(facade.toggle_pin_agent("host_ohos_2", &opt_id));
        facade
            .seed_thread_prefs_json(
                r#"[{"host_id":"host_ohos_2","agent_id":"ag_hist","seen_seq":12,"is_pinned":true}]"#,
            )
            .expect("seed prefs");
        facade.mark_agent_seen("host_ohos_2", "ag_hist", 20);

        let sidebar: serde_json::Value = serde_json::from_str(
            &facade
                .fleet_sidebar_json(
                    Some("recent"),
                    Some("all"),
                    None,
                    Some("host_ohos_2"),
                    Some(&opt_id),
                    Some("/repo/orbit"),
                )
                .expect("sidebar"),
        )
        .expect("json");
        assert_eq!(
            sidebar["pinned_conversations"].as_array().map(Vec::len),
            Some(1)
        );

        let catalogs_json = facade.fallback_provider_catalogs_json().expect("catalogs");
        assert!(catalogs_json.contains("claude_code"));
        let cat_json = facade
            .fallback_provider_catalog_json("claude_code")
            .expect("cat");
        let label = facade
            .format_catalog_model_label(&cat_json, Some("claude-opus-5-5"))
            .expect("label");
        assert_eq!(label, "Opus 5.5");
        assert_eq!(facade.format_effort_label("high"), "High");
        assert!(!facade.default_effort_description("medium").is_empty());

        let term_slot = facade
            .terminal_screen_slot_json("host_ohos_2", 2)
            .expect("slot 2");
        assert!(term_slot.contains("\"rows_data\""));
        let ctx = facade.build_side_question_context("host_ohos_2", &opt_id, Some(500));
        assert!(ctx.contains("Workspace: /repo/orbit"));

        let bridge_json = facade.bridge_info_json().expect("bridge info");
        assert!(bridge_json.contains("\"protocol_version\":2"));

        let slash_json = facade
            .slash_commands_json(Some("harmony"), Some("plan"))
            .expect("slash");
        assert!(slash_json.contains("/plan"));

        let sel_json = facade
            .resolve_model_selection_json(r#"{"provider":"claude_code","desired_model":"sonnet"}"#)
            .expect("model sel");
        assert!(sel_json.contains("\"claude-opus-5-5\""));

        let comp_json = facade
            .compose_prompt_json(
                r#"{"provider":"codex","raw_text":"/plan refactor router","host_id":"host_ohos_2","cwd":"/repo/orbit"}"#,
            )
            .expect("compose");
        assert!(comp_json.contains("<WORKFLOW_INSTRUCTION command=\\\"/plan\\\">"));

        let mention_json = facade
            .rank_mention_candidates_json(
                r#"{"host_id":"host_ohos_2","cwd":"/repo/orbit","query":"diff","limit":5}"#,
            )
            .expect("mention");
        assert!(mention_json.contains("@working-diff"));

        assert_eq!(
            facade
                .detect_diagram_kind("mermaid", "flowchart LR; A-->B")
                .as_deref(),
            Some("mermaid")
        );
        let diagram_json = facade
            .render_diagram_json(
                r#"{"kind":"mermaid","source":"flowchart LR\nA-->B","theme":"dark"}"#,
            )
            .expect("diagram");
        assert!(diagram_json.contains("<svg"));
    }

    #[test]
    fn ohos_facade_parity_automations_board_archive_and_modes() {
        assert_eq!(parse_mode_str("auto"), SessionMode::Auto);
        assert_eq!(parse_mode_str("accept_edits"), SessionMode::AcceptEdits);
        assert_eq!(parse_mode_str("full_access"), SessionMode::FullAccess);

        let facade = OhosCoreFacade::open_in_memory().expect("open");
        let kp = orbit_crypto::HostIdentityKeypair::generate().expect("keypair");
        let offer = orbit_crypto::PairingOffer {
            host_id: "host_parity".into(),
            hostname: "ohos-parity".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: Vec::new(),
            relay_url: None,
            pairing_secret: "secret".into(),
        };
        facade
            .pair_host_json(&offer.to_qr_uri().expect("uri"))
            .expect("pair");

        let opt_id = facade
            .create_agent_full(
                "host_parity",
                "/workspace/parity",
                None,
                "claude_code",
                None,
                None,
                None,
                "auto",
                "Test parity controls",
                None,
                None,
                None,
            )
            .expect("create");
        facade.set_agent_fast_mode("host_parity", &opt_id, true);
        facade.unarchive_agent("host_parity", &opt_id);
        facade.delete_agent("host_parity", &opt_id);

        // Workspace board & events roundtrip
        facade.fetch_workspace_board("host_parity", "ws_1");
        let board_json = facade
            .workspace_board_json("host_parity", "ws_1")
            .expect("board json");
        assert_eq!(board_json, "null");
        facade.update_workspace_board(
            "host_parity",
            "ws_1",
            "## Goal\nShip HarmonyOS parity".into(),
            None,
            Some("harmony-client".into()),
        );
        facade.fetch_workspace_events("host_parity", "ws_1", Some(20));
        let events_json = facade
            .workspace_events_json("host_parity", "ws_1")
            .expect("events json");
        assert_eq!(events_json, "[]");

        // Schedules CRUD fire-and-forget & runs snapshot
        facade
            .create_schedule_json(
                r#"{"host_id":"host_parity","name":"Nightly Check","cron_expr":"0 9 * * *","prompt":"Run tests","cwd":"/workspace/parity","provider":"claude_code","mode":"auto"}"#,
            )
            .expect("create schedule");
        facade.pause_schedule("host_parity", "sched_1");
        facade.resume_schedule("host_parity", "sched_1");
        facade.run_schedule_once("host_parity", "sched_1");
        facade.delete_schedule("host_parity", "sched_1");
        facade.fetch_schedule_runs("host_parity", "sched_1", Some(10));
        let runs_json = facade
            .schedule_runs_json("host_parity", "sched_1")
            .expect("runs json");
        assert_eq!(runs_json, "[]");

        // Offline RPCs return structured errors cleanly without panicking
        assert!(
            facade
                .list_conditional_timers_json("host_parity", None, true)
                .is_err()
        );
        assert!(
            facade
                .cancel_conditional_timer_json("host_parity", "tmr_1")
                .is_err()
        );
        assert!(facade.list_goal_runs_json("host_parity", None).is_err());
        assert!(
            facade
                .control_goal_run_json(
                    r#"{"host_id":"host_parity","goal_id":"g1","action":"pause"}"#,
                )
                .is_err()
        );
        assert!(
            facade
                .list_monitor_watches_json("host_parity", None)
                .is_err()
        );
        assert!(
            facade
                .stop_monitor_watch_json(r#"{"host_id":"host_parity","monitor_id":"m1"}"#)
                .is_err()
        );
        assert!(facade.list_workflow_runs_json("host_parity", true).is_err());
        assert!(
            facade
                .cancel_workflow_run_json(r#"{"host_id":"host_parity","run_id":"wf1"}"#)
                .is_err()
        );
    }
}
