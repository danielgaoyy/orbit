//! One host's in-memory replica. JS owns the socket and encrypted IndexedDB transactions.
use std::collections::{BTreeMap, HashMap};

use orbit_client_core::{catalog, contract, personal_agent, stage, viewmodel::*};
use orbit_crypto::PairingOffer;
use orbit_protocol::{PROTOCOL_VERSION, TimelineEvent, TimelineOutlinePrompt, WireMessage};
use wasm_bindgen::prelude::*;

use super::{error, from_js, js};

#[derive(Default)]
struct Timeline {
    events: BTreeMap<u64, TimelineEvent>,
    epoch: u32,
    outline: Vec<TimelineOutlinePrompt>,
    older: bool,
}

impl Timeline {
    fn insert(&mut self, event: TimelineEvent) {
        if event.epoch < self.epoch {
            return;
        }
        if event.epoch > self.epoch {
            self.events.clear();
            self.outline.clear();
            self.older = false;
            self.epoch = event.epoch;
        }
        self.events.insert(event.seq, event);
    }
}

#[wasm_bindgen]
pub struct WasmHost {
    detail: HostDetailView,
    timelines: HashMap<String, Timeline>,
    git_statuses: HashMap<String, orbit_protocol::GitStatusSnapshot>,
    git_diffs: HashMap<(String, Option<String>), String>,
}

#[wasm_bindgen]
impl WasmHost {
    #[wasm_bindgen(constructor)]
    pub fn new(profile: JsValue, cached: JsValue) -> Result<Self, JsError> {
        let offer: PairingOffer = from_js(profile)?;
        let mut detail = if cached.is_null() || cached.is_undefined() {
            HostDetailView::default()
        } else {
            from_js(cached)?
        };
        detail.host = HostCardView {
            host_id: offer.host_id,
            display_name: offer.hostname.clone(),
            hostname: offer.hostname,
            daemon_pubkey_b64: offer.daemon_pubkey_b64,
            direct_endpoints: offer.direct_endpoints,
            relay_url: offer.relay_url,
            client_protocol_version: PROTOCOL_VERSION,
            ..Default::default()
        };
        Ok(Self {
            detail,
            timelines: HashMap::new(),
            git_statuses: HashMap::new(),
            git_diffs: HashMap::new(),
        })
    }

    pub fn git_review(&self, cwd: String, path: Option<String>) -> Result<JsValue, JsError> {
        let raw = self
            .git_diffs
            .get(&(cwd.clone(), path.clone()))
            .map_or("", String::as_str);
        js(&project_git_review(
            self.detail.revision,
            &self.detail.host.host_id,
            &cwd,
            path.as_deref(),
            raw,
            self.git_statuses.get(&cwd).cloned(),
        ))
    }
    pub fn snapshot(&self) -> Result<JsValue, JsError> {
        js(&self.detail)
    }
    pub fn card(&self) -> Result<JsValue, JsError> {
        js(&self.detail.host)
    }
    pub fn connection(&mut self, state: JsValue, message: Option<String>) -> Result<(), JsError> {
        self.detail.host.connection_state = from_js(state)?;
        self.detail.host.last_error = message;
        self.detail.revision += 1;
        Ok(())
    }

    /// Returns the affected conversation, an empty string for host metadata, or null for a pure RPC response.
    pub fn receive(&mut self, json: &str) -> Result<Option<String>, JsError> {
        use WireMessage::*;
        let wire = serde_json::from_str(json).map_err(error)?;
        let h = &mut self.detail;
        let mut changed = String::new();
        match wire {
            Welcome { host } => {
                if host.host_id != h.host.host_id || host.public_key_b64 != h.host.daemon_pubkey_b64
                {
                    return Err(error("paired host identity mismatch"));
                }
                h.host.daemon_version = Some(host.daemon_version);
                h.host.daemon_protocol_version = Some(host.protocol_version);
                h.host.available_providers = host.available_providers;
                h.host.os_arch = Some(format!("{} {}", host.os, host.arch));
                h.host.supports_server_queue = host.protocol_version >= 4;
                h.host.board_cas_supported = host.board_cas_supported;
            }
            AgentsSnapshot { agents, .. } => {
                h.agents = agents
                    .iter()
                    .map(|a| project_agent_card(a, 0, false, false, false))
                    .collect()
            }
            AgentCreated { agent, .. } | AgentUpdated { agent } | AgentForked { agent, .. } => {
                h.agents.retain(|a| a.agent_id != agent.agent_id);
                h.agents
                    .push(project_agent_card(&agent, 0, false, false, false));
            }
            AgentDeleted { agent_id } => {
                h.agents.retain(|a| a.agent_id != agent_id);
                self.timelines.remove(&agent_id);
            }
            WorkspacesSnapshot { workspaces, .. } => h.workspaces = workspaces,
            WorkspaceCreated { workspace, .. } | WorkspaceUpdated { workspace } => {
                h.workspaces
                    .retain(|w| w.workspace_id != workspace.workspace_id);
                h.workspaces.push(workspace);
            }
            DiscoveredProjectsSnapshot { projects, .. } => h.discovered_projects = projects,
            GitStatusResponse { status, .. } => {
                self.git_statuses.insert(status.cwd.clone(), status);
                return Ok(None);
            }
            GitDiffResponse {
                cwd,
                path,
                unified_diff,
                ..
            } => {
                self.git_diffs.insert((cwd, path), unified_diff);
                return Ok(None);
            }
            ModelsSnapshot { models, .. } => h.models_by_provider = models,
            ProviderModelsSnapshot { catalogs, .. } => h.provider_catalogs = catalogs,
            QuotaReport { snapshots, .. } => h.quotas = snapshots,
            PendingPermissionsSnapshot { permissions, .. } => h.pending_permissions = permissions,
            ConditionalTimersSnapshot { timers, .. } => h.conditional_timers = timers,
            ConditionalTimerCreated { timer, .. } | ConditionalTimerUpdated { timer, .. } => {
                if let Some(slot) = h
                    .conditional_timers
                    .iter_mut()
                    .find(|existing| existing.timer_id == timer.timer_id)
                {
                    *slot = timer;
                } else {
                    h.conditional_timers.insert(0, timer);
                }
            }
            GoalRunsSnapshot { goals, .. } => h.goal_runs = goals,
            GoalRunUpdated { goal, .. } | GoalRunEvent { goal } => {
                if let Some(slot) = h
                    .goal_runs
                    .iter_mut()
                    .find(|existing| existing.goal_id == goal.goal_id)
                {
                    *slot = goal;
                } else {
                    h.goal_runs.insert(0, goal);
                }
            }
            DirectoryEntriesResponse {
                current_path,
                parent_path,
                entries,
                ..
            } => {
                h.directory_listing = Some(DirectoryListingView {
                    current_path,
                    parent_path,
                    entries,
                })
            }
            TimelineLive { event } => {
                changed.clone_from(&event.agent_id);
                self.timelines
                    .entry(changed.clone())
                    .or_default()
                    .insert(event);
            }
            TimelineCatchup {
                agent_id,
                events,
                outline_prompts,
                has_older_history,
                ..
            } => {
                let timeline = self.timelines.entry(agent_id.clone()).or_default();
                for event in events {
                    timeline.insert(event);
                }
                if !outline_prompts.is_empty() {
                    timeline.outline = outline_prompts;
                }
                timeline.older |= has_older_history;
                changed = agent_id;
            }
            TimelineHistoryPage {
                agent_id,
                events,
                has_more_older,
                ..
            } => {
                let timeline = self.timelines.entry(agent_id.clone()).or_default();
                for event in events {
                    timeline.insert(event);
                }
                timeline.older = has_more_older;
                changed = agent_id;
            }
            AgentRewound { agent, events, .. } => {
                changed.clone_from(&agent.agent_id);
                let mut timeline = Timeline::default();
                for event in events {
                    timeline.insert(event);
                }
                self.timelines.insert(changed.clone(), timeline);
                h.agents.retain(|a| a.agent_id != changed);
                h.agents
                    .push(project_agent_card(&agent, 0, false, false, false));
            }
            _ => return Ok(None),
        }
        h.revision += 1;
        h.host.agent_count = h.agents.len() as u32;
        h.host.workspace_count = h.workspaces.len() as u32;
        h.host.pending_approval_count = h.pending_permissions.len() as u32;
        Ok(Some(changed))
    }

    pub fn cursor(&self, agent: &str) -> f64 {
        self.timelines
            .get(agent)
            .and_then(|t| t.events.last_key_value())
            .map_or(0., |(seq, _)| *seq as f64)
    }
    pub fn events(&self, agent: &str) -> Result<JsValue, JsError> {
        js(&self.event_list(agent))
    }
    pub fn stage(&self, agent_id: &str) -> Result<JsValue, JsError> {
        js(&self.project_stage(agent_id))
    }

    pub fn stage_delta(
        &self,
        agent: &str,
        since_revision: f64,
        tail_limit: Option<u32>,
    ) -> Result<JsValue, JsError> {
        let stage = self.project_stage(agent);
        let count = stage.turns.len() as u32;
        let current = since_revision == stage.revision as f64 && since_revision != 0.0;
        let start = if current {
            count
        } else {
            count.saturating_sub(tail_limit.filter(|n| *n > 0).unwrap_or(count))
        };
        let oldest = self
            .timelines
            .get(agent)
            .and_then(|t| t.events.first_key_value())
            .map_or(0, |(seq, _)| *seq);
        js(&stage::AgentStageDeltaView {
            revision: stage.revision,
            host_id: stage.host_id,
            agent: stage.agent,
            replace_from_turn_index: start,
            window_start_turn_index: start,
            changed_turns: stage.turns.into_iter().skip(start as usize).collect(),
            total_turn_count: count,
            pending_permissions: stage.pending_permissions,
            pending_questions: stage.pending_questions,
            pending_plan_approval: stage.pending_plan_approval,
            session_turn_diffs: stage.session_turn_diffs,
            insights: stage.insights,
            latest_seq: stage.latest_seq,
            reset: !current,
            aux_revision: stage.revision,
            aux_unchanged: false,
            has_older_turns: start > 0 && !current,
            has_older_history: self.oldest(agent) > 0.,
            oldest_loaded_seq: oldest,
        })
    }

    pub fn timeline(&self, agent_id: &str) -> Result<JsValue, JsError> {
        let agent = self.detail.agents.iter().find(|a| a.agent_id == agent_id);
        let items =
            project_timeline_items(&self.event_list(agent_id), agent.map(|a| a.cwd.as_str()));
        js(
            &serde_json::json!({"host_id":self.detail.host.host_id,"agent":agent,"items":items,"revision":self.detail.revision}),
        )
    }
    pub fn oldest(&self, agent: &str) -> f64 {
        self.timelines
            .get(agent)
            .filter(|t| t.older)
            .and_then(|t| t.events.first_key_value())
            .map_or(0., |(seq, _)| *seq as f64)
    }
}

impl WasmHost {
    fn project_stage(&self, agent_id: &str) -> stage::AgentStageView {
        let agent = self.detail.agents.iter().find(|a| a.agent_id == agent_id);
        let cwd = agent.map(|a| a.cwd.as_str());
        let items = project_timeline_items(&self.event_list(agent_id), cwd);
        let subs = self
            .detail
            .provider_subagents
            .get(agent_id)
            .map_or(&[][..], Vec::as_slice);
        let (
            pending_permissions,
            pending_questions,
            pending_plan_approval,
            session_turn_diffs,
            mut insights,
        ) = stage::collect_stage_auxiliary_state(&items, cwd, agent, &self.detail.agents, subs);
        if let Some(t) = self.timelines.get(agent_id) {
            insights.merge_outline(&t.outline);
        }
        let split_finish_reports = agent.is_some_and(|a| {
            a.labels
                .get(orbit_protocol::LABEL_AGENT_ROLE)
                .map(|r| r.trim().to_ascii_lowercase())
                .is_some_and(|r| {
                    matches!(r.as_str(), "personal_router" | "router" | "chief_of_staff")
                })
                || a.handle
                    .as_deref()
                    .is_some_and(|h| h.trim().eq_ignore_ascii_case("router"))
        });
        stage::AgentStageView {
            host_id: self.detail.host.host_id.clone(),
            revision: self.detail.revision,
            agent: agent.cloned(),
            turns: stage::process_timeline_turns_slice_with_options(
                &items,
                0,
                0,
                cwd,
                split_finish_reports,
            )
            .turns,
            latest_seq: self.cursor(agent_id) as u64,
            pending_permissions,
            pending_questions,
            pending_plan_approval,
            session_turn_diffs,
            insights,
        }
    }

    fn event_list(&self, agent: &str) -> Vec<TimelineEvent> {
        self.timelines
            .get(agent)
            .map(|t| t.events.values().cloned().collect())
            .unwrap_or_default()
    }
}

#[wasm_bindgen]
pub fn resolve_model_selection(req: JsValue, live: JsValue) -> Result<JsValue, JsError> {
    let req: contract::ModelSelectionRequest = from_js(req)?;
    let catalogs = catalog::effective_provider_catalogs(&from_js::<Vec<_>>(live)?);
    js(&catalog::resolve_model_selection(
        &catalogs,
        req.provider,
        req.desired_model.as_deref(),
        req.desired_effort,
    ))
}

#[wasm_bindgen]
pub fn get_personal_agent_briefing(
    hosts: JsValue,
    primary_router_host_id: Option<String>,
    now_ms: Option<f64>,
) -> Result<JsValue, JsError> {
    let details: Vec<HostDetailView> = from_js(hosts)?;
    let empty_read = HashMap::new();
    let empty_previews = HashMap::new();
    let inputs: Vec<personal_agent::BriefingHostInput<'_>> = details
        .iter()
        .map(|d| personal_agent::BriefingHostInput {
            host_id: d.host.host_id.as_str(),
            host_name: d.host.display_name.as_str(),
            online: matches!(
                d.host.connection_state,
                HostConnectionState::OnlineDirect | HostConnectionState::OnlineRelay
            ),
            agents: d.agents.as_slice(),
            pending_permissions: d.pending_permissions.as_slice(),
            conditional_timers: d.conditional_timers.as_slice(),
            goal_runs: d.goal_runs.as_slice(),
            agent_read_at_ms: &empty_read,
            step_previews: &empty_previews,
        })
        .collect();
    let now = now_ms
        .filter(|v| v.is_finite() && *v > 0.0)
        .map(|v| v as i64)
        .unwrap_or(0);
    js(&personal_agent::build_personal_agent_briefing(
        &inputs,
        primary_router_host_id.as_deref(),
        now,
    ))
}
