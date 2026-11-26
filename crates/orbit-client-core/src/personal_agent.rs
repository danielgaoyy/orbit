//! Pure-Rust Personal Agent Briefing projection shared across Desktop, Web (WASM), and mobile clients.
//!
//! Computes the Personal Agent's live chief-of-staff briefing strictly from in-memory host state
//! (zero disk or network I/O) so clients can refresh it on every store revision.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet, VecDeque};

use chrono::DateTime;
use orbit_protocol::{
    AGENT_ROLE_PERSONAL_ROUTER, AgentRole, AgentStatus, ConditionalTimerRecord, GoalRunRecord,
    LABEL_AGENT_ROLE, PendingPermissionSummary, ProviderKind, RouterRecommendation, ToolCallDetail,
};
use serde::{Deserialize, Serialize};

use crate::viewmodel::AgentCardView;

/// Maximum number of non-PA foreground sessions included in `other_active`.
pub const MAX_OTHER_ACTIVE_WORKERS: usize = 12;

/// Window (24 hours) for counting recently completed missions in `BriefingCounts.done_recent`.
const RECENT_DONE_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;

/// Per-host in-memory projection input passed into [`build_personal_agent_briefing`].
#[derive(Debug, Clone)]
pub struct BriefingHostInput<'a> {
    pub host_id: &'a str,
    pub host_name: &'a str,
    pub online: bool,
    pub agents: &'a [AgentCardView],
    pub pending_permissions: &'a [PendingPermissionSummary],
    pub conditional_timers: &'a [ConditionalTimerRecord],
    pub goal_runs: &'a [GoalRunRecord],
    /// Optional per-agent last-read timestamp (`agent_id -> updated_at_ms` from `ClientThreadPref`).
    pub agent_read_at_ms: &'a HashMap<String, i64>,
    /// Optional per-agent live/settled step preview from cached stage turns.
    pub step_previews: &'a HashMap<String, String>,
}

/// Summary reference to the active Personal Agent session across the fleet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingAgentRef {
    pub host_id: String,
    pub host_name: String,
    pub agent_id: String,
    pub title: String,
    pub provider: ProviderKind,
    pub status_label: String,
    pub is_busy: bool,
    pub is_awaiting: bool,
}

/// Classification of an item in the Personal Agent's "Needs You" triage queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingDecisionKind {
    Plan,
    Permission,
    Awaiting,
    Error,
}

impl BriefingDecisionKind {
    const fn priority_rank(self) -> u8 {
        match self {
            Self::Plan => 0,
            Self::Permission => 1,
            Self::Awaiting => 2,
            Self::Error => 3,
        }
    }
}

/// Single actionable decision card requiring human review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingDecision {
    pub key: String,
    pub kind: BriefingDecisionKind,
    pub host_id: String,
    pub host_name: String,
    pub agent_id: String,
    pub agent_title: String,
    pub request_id: Option<String>,
    pub summary: String,
    pub risk_level: Option<String>,
    pub human_only: bool,
    pub batch_eligible: bool,
    pub router_recommendation: Option<RouterRecommendation>,
    pub dispatched_by_personal_agent: bool,
    pub updated_at_ms: i64,
}

/// Live execution state of an individual worker row in the Briefing Rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingWorkerState {
    Running,
    Awaiting,
    Error,
    Done,
    Idle,
}

impl BriefingWorkerState {
    const fn sort_rank(self) -> u8 {
        match self {
            Self::Awaiting => 0,
            Self::Running => 1,
            Self::Error => 2,
            Self::Idle => 3,
            Self::Done => 4,
        }
    }
}

/// Compact representation of a worker session in a mission or `other_active`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingWorker {
    pub host_id: String,
    pub host_name: String,
    pub agent_id: String,
    pub title: String,
    pub provider: ProviderKind,
    pub state: BriefingWorkerState,
    pub step_preview: String,
    pub worktree_branch: Option<String>,
    pub updated_at_ms: i64,
}

/// Aggregate state of a PA-dispatched mission (root worker + cross-host descendants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingMissionState {
    Running,
    Awaiting,
    Error,
    Done,
}

impl BriefingMissionState {
    const fn sort_rank(self) -> u8 {
        match self {
            Self::Awaiting => 0,
            Self::Running => 1,
            Self::Error => 2,
            Self::Done => 3,
        }
    }
}

/// Group of workers rooted at one direct child dispatched by the Personal Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingMission {
    pub key: String,
    pub root: BriefingWorker,
    pub workers: Vec<BriefingWorker>,
    pub host_names: Vec<String>,
    pub done: u32,
    pub total: u32,
    pub state: BriefingMissionState,
    pub updated_at_ms: i64,
}

/// Category of an item in the Personal Agent's "Waiting" rail section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BriefingWaitKind {
    Timer,
    Goal,
    Background,
}

impl BriefingWaitKind {
    const fn sort_rank(self) -> u8 {
        match self {
            Self::Goal => 0,
            Self::Timer => 1,
            Self::Background => 2,
        }
    }
}

/// Armed conditional timer, active/paused goal, or background wait tracked for the Personal Agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingWaitItem {
    pub key: String,
    pub kind: BriefingWaitKind,
    pub host_id: String,
    pub agent_id: String,
    pub title: String,
    pub detail: String,
    pub due_at_ms: Option<i64>,
}

/// Compact host status entry for the Briefing Rail footer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingHost {
    pub host_id: String,
    pub name: String,
    pub online: bool,
    pub is_personal_agent_host: bool,
}

/// Top-level counters displayed in the Personal Agent compact pill and briefing summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingCounts {
    pub needs_you: u32,
    pub running: u32,
    pub done_recent: u32,
    pub hosts_online: u32,
    pub hosts_total: u32,
}

/// Summary of work completed since the user last visited the Personal Agent conversation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BriefingSinceLastVisit {
    pub completed: u32,
    pub auto_handled: u32,
}

/// Complete Personal Agent Home briefing snapshot computed purely in Rust.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalAgentBriefingView {
    pub personal_agent: Option<BriefingAgentRef>,
    pub needs_you: Vec<BriefingDecision>,
    pub missions: Vec<BriefingMission>,
    pub other_active: Vec<BriefingWorker>,
    pub waiting: Vec<BriefingWaitItem>,
    pub hosts: Vec<BriefingHost>,
    pub counts: BriefingCounts,
    pub since_last_visit: BriefingSinceLastVisit,
}

/// Pure projection builder for [`PersonalAgentBriefingView`].
#[must_use]
pub fn build_personal_agent_briefing(
    hosts: &[BriefingHostInput<'_>],
    primary_router_host_id: Option<&str>,
    now_ms: i64,
) -> PersonalAgentBriefingView {
    let primary_host = primary_router_host_id
        .map(str::trim)
        .filter(|s| !s.is_empty());

    // Index every non-archived agent by (host_id, agent_id) and precompute pending-permission lookup.
    struct IndexedAgent<'a> {
        host_idx: usize,
        host_id: &'a str,
        host_name: &'a str,
        agent: &'a AgentCardView,
        updated_at_ms: i64,
        has_pending_perm: bool,
        state: BriefingWorkerState,
    }

    let mut pending_agents: HashSet<(&str, &str)> = HashSet::new();
    for h in hosts {
        for perm in h.pending_permissions {
            pending_agents.insert((h.host_id, perm.agent_id.as_str()));
        }
    }

    let mut all_agents: HashMap<(&str, &str), IndexedAgent<'_>> = HashMap::new();
    for (host_idx, h) in hosts.iter().enumerate() {
        for ag in h.agents {
            if ag.archived_at_rfc3339.is_some() {
                continue;
            }
            let has_pending_perm = pending_agents.contains(&(h.host_id, ag.agent_id.as_str()));
            let state = classify_worker_own_state(ag, has_pending_perm);
            let updated_at_ms = parse_rfc3339_ms(&ag.updated_at_rfc3339);
            all_agents.insert(
                (h.host_id, ag.agent_id.as_str()),
                IndexedAgent {
                    host_idx,
                    host_id: h.host_id,
                    host_name: h.host_name,
                    agent: ag,
                    updated_at_ms,
                    has_pending_perm,
                    state,
                },
            );
        }
    }

    // 1. Resolve canonical Personal Agent:
    //    labels["orbit.role"] == "personal_router" > handle == "router" > title contains "Personal Router"
    //    Prefer primary_router_host_id, exclude archived.
    #[derive(PartialEq, Eq, PartialOrd, Ord)]
    struct PaCandidate<'a> {
        tier: u8,
        not_primary: bool,
        offline: bool,
        updated_desc: Reverse<i64>,
        host_id: &'a str,
        agent_id: &'a str,
    }
    let mut pa_candidates: Vec<PaCandidate<'_>> = Vec::new();
    for (&(hid, aid), entry) in &all_agents {
        let Some(tier) = personal_agent_match_tier(entry.agent) else {
            continue;
        };
        let not_primary = primary_host.is_some_and(|ph| ph != hid);
        let offline = !hosts[entry.host_idx].online;
        pa_candidates.push(PaCandidate {
            tier,
            not_primary,
            offline,
            updated_desc: Reverse(entry.updated_at_ms),
            host_id: hid,
            agent_id: aid,
        });
    }
    pa_candidates.sort();
    let pa_key: Option<(&str, &str)> = pa_candidates.first().map(|c| (c.host_id, c.agent_id));

    let personal_agent = pa_key.and_then(|key| {
        let entry = all_agents.get(&key)?;
        let is_busy = is_foreground_running(entry.agent)
            || entry.agent.has_running_subagents
            || entry.agent.is_awaiting_permission;
        Some(BriefingAgentRef {
            host_id: entry.host_id.to_string(),
            host_name: entry.host_name.to_string(),
            agent_id: entry.agent.agent_id.clone(),
            title: non_empty_or(&entry.agent.title, "Personal Agent"),
            provider: entry.agent.provider,
            status_label: entry.agent.status_label.clone(),
            is_busy,
            is_awaiting: entry.agent.is_awaiting_permission || entry.has_pending_perm,
        })
    });

    // 2. Build parent -> children map across all hosts and derive PA-dispatched subtree.
    let mut children_by_parent: HashMap<(&str, &str), Vec<(&str, &str)>> = HashMap::new();
    for (&(hid, aid), entry) in &all_agents {
        if let Some(parent_key) = parent_agent_key(hid, entry.agent)
            && parent_key != (hid, aid)
        {
            children_by_parent
                .entry(parent_key)
                .or_default()
                .push((hid, aid));
        }
    }
    for kids in children_by_parent.values_mut() {
        kids.sort();
    }

    let mut pa_subtree: HashSet<(&str, &str)> = HashSet::new();
    let mut pa_worker_states: HashMap<(&str, &str), BriefingWorkerState> = HashMap::new();
    let mut missions: Vec<BriefingMission> = Vec::new();
    let mut running_in_missions: u32 = 0;

    let descendant_activity = |root: (&str, &str)| -> (bool, bool) {
        let mut seen: HashSet<(&str, &str)> = HashSet::new();
        seen.insert(root);
        let mut q: VecDeque<(&str, &str)> = VecDeque::new();
        q.push_back(root);
        let mut any_active = false;
        let mut any_running = false;
        while let Some(curr) = q.pop_front() {
            if let Some(kids) = children_by_parent.get(&curr) {
                for &kid in kids {
                    if seen.insert(kid) {
                        if let Some(child_entry) = all_agents.get(&kid) {
                            match child_entry.state {
                                BriefingWorkerState::Running => {
                                    any_active = true;
                                    any_running = true;
                                }
                                BriefingWorkerState::Awaiting => {
                                    any_active = true;
                                }
                                _ => {}
                            }
                        }
                        q.push_back(kid);
                    }
                }
            }
        }
        (any_active, any_running)
    };

    if let Some(pa) = pa_key {
        let direct_children = children_by_parent.get(&pa).cloned().unwrap_or_default();
        for root_key in direct_children {
            let Some(root_entry) = all_agents.get(&root_key) else {
                continue;
            };
            let mut visited: HashSet<(&str, &str)> = HashSet::new();
            visited.insert(pa);
            visited.insert(root_key);
            pa_subtree.insert(root_key);

            let mut queue: VecDeque<(&str, &str)> = VecDeque::new();
            queue.push_back(root_key);
            let mut descendant_keys: Vec<(&str, &str)> = Vec::new();
            while let Some(curr) = queue.pop_front() {
                if let Some(kids) = children_by_parent.get(&curr) {
                    for &kid in kids {
                        if visited.insert(kid) {
                            pa_subtree.insert(kid);
                            descendant_keys.push(kid);
                            queue.push_back(kid);
                        }
                    }
                }
            }

            let resolve_pa_worker_state = |key: (&str, &str), entry: &IndexedAgent<'_>| {
                if matches!(
                    entry.state,
                    BriefingWorkerState::Done | BriefingWorkerState::Idle
                ) && entry.agent.has_running_subagents
                {
                    // Managed descendants are projected individually in `mission.workers`;
                    // only promote this worker to `Running` when `has_running_subagents`
                    // reflects a provider-native subagent not represented in `all_agents`.
                    let (any_active_managed, _) = descendant_activity(key);
                    if !any_active_managed {
                        return BriefingWorkerState::Running;
                    }
                }
                entry.state
            };

            let root_state = resolve_pa_worker_state(root_key, root_entry);
            pa_worker_states.insert(root_key, root_state);
            let root_worker = build_worker(
                root_entry.host_id,
                root_entry.host_name,
                root_entry.agent,
                root_state,
                root_entry.updated_at_ms,
                &hosts[root_entry.host_idx],
            );
            let mut workers: Vec<BriefingWorker> = descendant_keys
                .iter()
                .filter_map(|k| all_agents.get(k).map(|e| (*k, e)))
                .map(|(k, e)| {
                    let st = resolve_pa_worker_state(k, e);
                    pa_worker_states.insert(k, st);
                    build_worker(
                        e.host_id,
                        e.host_name,
                        e.agent,
                        st,
                        e.updated_at_ms,
                        &hosts[e.host_idx],
                    )
                })
                .collect();
            workers.sort_by(|a, b| {
                a.state
                    .sort_rank()
                    .cmp(&b.state.sort_rank())
                    .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
                    .then_with(|| a.host_id.cmp(&b.host_id))
                    .then_with(|| a.agent_id.cmp(&b.agent_id))
            });

            let mut host_names: Vec<String> = Vec::new();
            let mut seen_hosts: HashSet<&str> = HashSet::new();
            if seen_hosts.insert(root_worker.host_name.as_str()) {
                host_names.push(root_worker.host_name.clone());
            }
            for w in &workers {
                if seen_hosts.insert(w.host_name.as_str()) {
                    host_names.push(w.host_name.clone());
                }
            }

            let all_states =
                std::iter::once(root_worker.state).chain(workers.iter().map(|w| w.state));
            let mut done: u32 = 0;
            let mut has_awaiting = false;
            let mut has_error = false;
            let mut has_running = false;
            for st in all_states {
                match st {
                    BriefingWorkerState::Done => done += 1,
                    BriefingWorkerState::Awaiting => has_awaiting = true,
                    BriefingWorkerState::Error => has_error = true,
                    BriefingWorkerState::Running => {
                        has_running = true;
                        running_in_missions += 1;
                    }
                    BriefingWorkerState::Idle => {}
                }
            }
            let total = 1 + workers.len() as u32;
            let mission_state = if has_awaiting {
                BriefingMissionState::Awaiting
            } else if has_running {
                BriefingMissionState::Running
            } else if has_error {
                BriefingMissionState::Error
            } else {
                BriefingMissionState::Done
            };

            let updated_at_ms = workers
                .iter()
                .map(|w| w.updated_at_ms)
                .fold(root_worker.updated_at_ms, i64::max);

            missions.push(BriefingMission {
                key: format!("{}:{}", root_worker.host_id, root_worker.agent_id),
                root: root_worker,
                workers,
                host_names,
                done,
                total,
                state: mission_state,
                updated_at_ms,
            });
        }
    }

    missions.sort_by(|a, b| {
        a.state
            .sort_rank()
            .cmp(&b.state.sort_rank())
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.key.cmp(&b.key))
    });

    // 3. Build `needs_you`: plan > permission > awaiting > error, reverse time within same kind.
    let mut needs_you: Vec<BriefingDecision> = Vec::new();
    let mut seen_perm_agents: HashSet<(&str, &str)> = HashSet::new();

    for h in hosts {
        for perm in h.pending_permissions {
            let key = (h.host_id, perm.agent_id.as_str());
            let Some(entry) = all_agents.get(&key) else {
                continue;
            };
            seen_perm_agents.insert(key);

            let is_plan = perm.plan_approval.is_some();
            let kind = if is_plan {
                BriefingDecisionKind::Plan
            } else {
                BriefingDecisionKind::Permission
            };
            let human_only = perm.human_only || is_plan;
            let risk_level = perm
                .guardian_assessment
                .as_ref()
                .map(|a| a.risk_level.as_str().to_string());
            let batch_eligible = !is_plan
                && !human_only
                && risk_level
                    .as_deref()
                    .is_none_or(|r| r.eq_ignore_ascii_case("low"));

            let agent_title = non_empty_or(
                &entry.agent.title,
                non_empty_or(&perm.agent_title, &entry.agent.agent_id).as_str(),
            );
            let summary = if let Some(ref plan) = perm.plan_approval {
                plan.plan_title
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        plan.summary
                            .as_deref()
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                    })
                    .or_else(|| {
                        let t = perm.title.trim();
                        (!t.is_empty()).then_some(t)
                    })
                    .unwrap_or("Plan awaiting review")
                    .to_string()
            } else {
                describe_tool_detail_plainly(&perm.detail, &perm.title)
            };

            let perm_ts = perm.created_at.timestamp_millis();
            let updated_at_ms = if perm_ts > 0 {
                perm_ts
            } else {
                entry.updated_at_ms
            };

            needs_you.push(BriefingDecision {
                key: format!("perm:{}:{}:{}", h.host_id, perm.agent_id, perm.request_id),
                kind,
                host_id: h.host_id.to_string(),
                host_name: h.host_name.to_string(),
                agent_id: perm.agent_id.clone(),
                agent_title,
                request_id: Some(perm.request_id.clone()),
                summary,
                risk_level,
                human_only,
                batch_eligible,
                router_recommendation: perm.router_recommendation.clone(),
                dispatched_by_personal_agent: pa_subtree.contains(&key),
                updated_at_ms,
            });
        }
    }

    for (&key, entry) in &all_agents {
        let is_pa_worker = pa_subtree.contains(&key);
        if entry.agent.background && !is_pa_worker {
            continue;
        }
        let agent_title = non_empty_or(&entry.agent.title, &entry.agent.provider_label);
        if entry.agent.is_awaiting_permission && !seen_perm_agents.contains(&key) {
            needs_you.push(BriefingDecision {
                key: format!("await:{}:{}", entry.host_id, entry.agent.agent_id),
                kind: BriefingDecisionKind::Awaiting,
                host_id: entry.host_id.to_string(),
                host_name: entry.host_name.to_string(),
                agent_id: entry.agent.agent_id.clone(),
                agent_title,
                request_id: None,
                summary: non_empty_or(&entry.agent.status_label, "Waiting for decision"),
                risk_level: None,
                human_only: true,
                batch_eligible: false,
                router_recommendation: None,
                dispatched_by_personal_agent: is_pa_worker,
                updated_at_ms: entry.updated_at_ms,
            });
        } else if is_agent_errored(entry.agent) {
            let err_summary = match &entry.agent.status {
                AgentStatus::Error { message, .. } if !message.trim().is_empty() => {
                    compact_line(message, 160)
                }
                _ => entry
                    .agent
                    .task_summary
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| compact_line(s, 160))
                    .unwrap_or_else(|| non_empty_or(&entry.agent.status_label, "Session error")),
            };
            needs_you.push(BriefingDecision {
                key: format!("err:{}:{}", entry.host_id, entry.agent.agent_id),
                kind: BriefingDecisionKind::Error,
                host_id: entry.host_id.to_string(),
                host_name: entry.host_name.to_string(),
                agent_id: entry.agent.agent_id.clone(),
                agent_title,
                request_id: None,
                summary: err_summary,
                risk_level: Some("high".to_string()),
                human_only: true,
                batch_eligible: false,
                router_recommendation: None,
                dispatched_by_personal_agent: is_pa_worker,
                updated_at_ms: entry.updated_at_ms,
            });
        }
    }

    needs_you.sort_by(|a, b| {
        a.kind
            .priority_rank()
            .cmp(&b.kind.priority_rank())
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.key.cmp(&b.key))
    });

    // 4. `other_active`: non-PA foreground running/awaiting sessions, capped at 12.
    let mut other_active: Vec<BriefingWorker> = Vec::new();
    for (&key, entry) in &all_agents {
        if Some(key) == pa_key || pa_subtree.contains(&key) {
            continue;
        }
        if entry.agent.background {
            continue;
        }
        // Keep primary foreground threads crisp by excluding all Subagents (local, cross-host, or orphaned).
        if entry.agent.is_subagent() {
            continue;
        }
        let effective_state = if matches!(
            entry.state,
            BriefingWorkerState::Done | BriefingWorkerState::Idle
        ) {
            let (any_active_managed, any_running_managed) = descendant_activity(key);
            if any_running_managed || (entry.agent.has_running_subagents && !any_active_managed) {
                BriefingWorkerState::Running
            } else {
                entry.state
            }
        } else {
            entry.state
        };
        if !matches!(
            effective_state,
            BriefingWorkerState::Running | BriefingWorkerState::Awaiting
        ) {
            continue;
        }
        other_active.push(build_worker(
            entry.host_id,
            entry.host_name,
            entry.agent,
            effective_state,
            entry.updated_at_ms,
            &hosts[entry.host_idx],
        ));
    }
    other_active.sort_by(|a, b| {
        a.state
            .sort_rank()
            .cmp(&b.state.sort_rank())
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| a.host_id.cmp(&b.host_id))
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    other_active.truncate(MAX_OTHER_ACTIVE_WORKERS);

    let running_in_other = other_active
        .iter()
        .filter(|w| w.state == BriefingWorkerState::Running)
        .count() as u32;

    // 5. `waiting`: armed conditional timers, active/paused goals, and background tasks on PA + PA workers.
    let mut waiting: Vec<BriefingWaitItem> = Vec::new();
    if let Some(pa) = pa_key {
        let mut pa_and_workers: HashSet<(&str, &str)> = pa_subtree.clone();
        pa_and_workers.insert(pa);

        for h in hosts {
            for timer in h.conditional_timers {
                if !timer.state.is_armed() {
                    continue;
                }
                let key = (h.host_id, timer.agent_id.as_str());
                if !pa_and_workers.contains(&key) {
                    continue;
                }
                let title = all_agents
                    .get(&key)
                    .map(|e| non_empty_or(&e.agent.title, &e.agent.agent_id))
                    .unwrap_or_else(|| timer.agent_id.clone());
                let prompt_preview = compact_line(&timer.prompt, 80);
                let detail = if prompt_preview.is_empty() {
                    timer.condition.short_label()
                } else {
                    format!("{} · {prompt_preview}", timer.condition.short_label())
                };
                waiting.push(BriefingWaitItem {
                    key: format!("timer:{}:{}", h.host_id, timer.timer_id),
                    kind: BriefingWaitKind::Timer,
                    host_id: h.host_id.to_string(),
                    agent_id: timer.agent_id.clone(),
                    title,
                    detail,
                    due_at_ms: i64::try_from(timer.fire_at_ms).ok(),
                });
            }

            for goal in h.goal_runs {
                if goal.status.is_terminal() {
                    continue;
                }
                let key = (h.host_id, goal.agent_id.as_str());
                if !pa_and_workers.contains(&key) {
                    continue;
                }
                let title = non_empty_or(&compact_line(&goal.objective, 96), "Goal run");
                let mut detail = format!(
                    "{} · {}/{}",
                    goal.status.as_str(),
                    goal.current_iteration,
                    goal.max_iterations
                );
                if let Some(ref reason) = goal.pause_reason
                    && !reason.trim().is_empty()
                {
                    detail.push_str(" · ");
                    detail.push_str(&compact_line(reason, 60));
                }
                let due_at_ms = if goal.max_duration_secs > 0 {
                    goal.started_at_ms
                        .checked_add(goal.max_duration_secs.saturating_mul(1000))
                        .and_then(|v| i64::try_from(v).ok())
                } else {
                    None
                };
                waiting.push(BriefingWaitItem {
                    key: format!("goal:{}:{}", h.host_id, goal.goal_id),
                    kind: BriefingWaitKind::Goal,
                    host_id: h.host_id.to_string(),
                    agent_id: goal.agent_id.clone(),
                    title,
                    detail,
                    due_at_ms,
                });
            }
        }

        for &key in &pa_and_workers {
            let Some(entry) = all_agents.get(&key) else {
                continue;
            };
            if entry.agent.has_background_tasks {
                waiting.push(BriefingWaitItem {
                    key: format!("bg:{}:{}", entry.host_id, entry.agent.agent_id),
                    kind: BriefingWaitKind::Background,
                    host_id: entry.host_id.to_string(),
                    agent_id: entry.agent.agent_id.clone(),
                    title: non_empty_or(&entry.agent.title, &entry.agent.agent_id),
                    detail: non_empty_or(&entry.agent.status_label, "Waiting for background tasks"),
                    due_at_ms: None,
                });
            }
        }
    }

    waiting.sort_by(|a, b| {
        match (a.due_at_ms, b.due_at_ms) {
            (Some(da), Some(db)) => da.cmp(&db),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
        .then_with(|| a.kind.sort_rank().cmp(&b.kind.sort_rank()))
        .then_with(|| a.key.cmp(&b.key))
    });

    // 6. `hosts`
    let mut briefing_hosts: Vec<BriefingHost> = hosts
        .iter()
        .map(|h| BriefingHost {
            host_id: h.host_id.to_string(),
            name: h.host_name.to_string(),
            online: h.online,
            is_personal_agent_host: pa_key.is_some_and(|(pa_hid, _)| pa_hid == h.host_id),
        })
        .collect();
    briefing_hosts.sort_by(|a, b| {
        b.is_personal_agent_host
            .cmp(&a.is_personal_agent_host)
            .then_with(|| b.online.cmp(&a.online))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.host_id.cmp(&b.host_id))
    });

    // 7. `since_last_visit`
    let completed_since_last_visit = pa_key
        .and_then(|(pa_hid, pa_aid)| {
            let entry = all_agents.get(&(pa_hid, pa_aid))?;
            hosts[entry.host_idx]
                .agent_read_at_ms
                .get(pa_aid)
                .copied()
                .filter(|&ts| ts > 0)
        })
        .map_or(0, |read_ms| {
            pa_subtree
                .iter()
                .filter_map(|k| {
                    let entry = all_agents.get(k)?;
                    let st = pa_worker_states.get(k).copied().unwrap_or(entry.state);
                    Some((entry, st))
                })
                .filter(|(e, st)| *st == BriefingWorkerState::Done && e.updated_at_ms > read_ms)
                .count() as u32
        });

    // 8. `counts`
    let recent_cutoff = now_ms.saturating_sub(RECENT_DONE_WINDOW_MS);
    let done_recent = missions
        .iter()
        .filter(|m| {
            m.state == BriefingMissionState::Done
                && (now_ms <= 0 || m.updated_at_ms >= recent_cutoff)
        })
        .count() as u32;
    let hosts_online = briefing_hosts.iter().filter(|h| h.online).count() as u32;
    let hosts_total = briefing_hosts.len() as u32;

    PersonalAgentBriefingView {
        personal_agent,
        counts: BriefingCounts {
            needs_you: needs_you.len() as u32,
            running: running_in_missions + running_in_other,
            done_recent,
            hosts_online,
            hosts_total,
        },
        needs_you,
        missions,
        other_active,
        waiting,
        hosts: briefing_hosts,
        since_last_visit: BriefingSinceLastVisit {
            completed: completed_since_last_visit,
            auto_handled: 0,
        },
    }
}

/// Whether a conversation card is a Personal Agent session (role label, `router` handle, or
/// legacy/default title). The sidebar hides these rows because the Personal Agent has its own
/// top-level entry.
pub fn is_personal_agent_card(agent: &AgentCardView) -> bool {
    personal_agent_match_tier(agent).is_some()
}

fn personal_agent_match_tier(agent: &AgentCardView) -> Option<u8> {
    if agent.labels.get(LABEL_AGENT_ROLE).is_some_and(|v| {
        v == AGENT_ROLE_PERSONAL_ROUTER || AgentRole::from_slug(v) == AgentRole::PersonalRouter
    }) {
        return Some(0);
    }
    if agent
        .handle
        .as_deref()
        .is_some_and(|h| h.trim().eq_ignore_ascii_case("router"))
    {
        return Some(1);
    }
    if agent.title.contains("Personal Router")
        || agent.title.to_ascii_lowercase().contains("personal router")
    {
        return Some(2);
    }
    None
}

fn parent_agent_key<'a>(
    child_host_id: &'a str,
    child: &'a AgentCardView,
) -> Option<(&'a str, &'a str)> {
    if let Some(parent_aid) = child
        .parent_agent_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let parent_hid = child
            .parent_host_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(child_host_id);
        return Some((parent_hid, parent_aid));
    }
    let routed_aid = child
        .labels
        .get(orbit_protocol::LABEL_ROUTED_BY_AGENT)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let routed_hid = child
        .labels
        .get(orbit_protocol::LABEL_ROUTED_BY_HOST)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(child_host_id);
    Some((routed_hid, routed_aid))
}

fn is_foreground_running(agent: &AgentCardView) -> bool {
    !agent.has_background_tasks
        && (agent.status.is_foreground_busy()
            || (agent.is_running
                && !matches!(
                    agent.status,
                    AgentStatus::Idle
                        | AgentStatus::Closed
                        | AgentStatus::Error { .. }
                        | AgentStatus::AwaitingPermission { .. }
                )))
}

fn is_agent_errored(agent: &AgentCardView) -> bool {
    if matches!(agent.status, AgentStatus::Error { .. }) {
        return true;
    }
    let lbl = agent.status_label.to_ascii_lowercase();
    lbl.contains("error") || lbl.contains("fail")
}

fn classify_worker_own_state(agent: &AgentCardView, has_pending_perm: bool) -> BriefingWorkerState {
    if agent.is_awaiting_permission
        || has_pending_perm
        || matches!(agent.status, AgentStatus::AwaitingPermission { .. })
    {
        return BriefingWorkerState::Awaiting;
    }
    if is_agent_errored(agent) {
        return BriefingWorkerState::Error;
    }
    if is_foreground_running(agent) {
        return BriefingWorkerState::Running;
    }
    if matches!(agent.status, AgentStatus::Closed)
        || (!agent.has_background_tasks
            && !agent.has_armed_timers
            && matches!(agent.status, AgentStatus::Idle)
            && agent.latest_seq > 0)
    {
        return BriefingWorkerState::Done;
    }
    BriefingWorkerState::Idle
}

fn build_worker(
    host_id: &str,
    host_name: &str,
    ag: &AgentCardView,
    state: BriefingWorkerState,
    updated_at_ms: i64,
    host: &BriefingHostInput<'_>,
) -> BriefingWorker {
    let step_preview = host
        .step_previews
        .get(&ag.agent_id)
        .map(String::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            ag.task_summary
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or(ag.status_label.as_str())
        .to_string();

    BriefingWorker {
        host_id: host_id.to_string(),
        host_name: host_name.to_string(),
        agent_id: ag.agent_id.clone(),
        title: non_empty_or(&ag.title, &ag.provider_label),
        provider: ag.provider,
        state,
        step_preview,
        worktree_branch: ag.worktree_branch.clone(),
        updated_at_ms,
    }
}

fn parse_rfc3339_ms(raw: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

fn non_empty_or(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn compact_line(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (idx, ch) in collapsed.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn describe_tool_detail_plainly(detail: &ToolCallDetail, fallback_title: &str) -> String {
    let fallback = non_empty_or(fallback_title, "Permission request");
    match detail {
        ToolCallDetail::Shell { command, .. } if !command.trim().is_empty() => {
            format!("Run command: {}", compact_line(command, 140))
        }
        ToolCallDetail::FileEdit { path, .. } if !path.trim().is_empty() => {
            format!("Edit file: {}", path.trim())
        }
        ToolCallDetail::FileRead { path, .. } if !path.trim().is_empty() => {
            format!("Read file: {}", path.trim())
        }
        ToolCallDetail::Search { query, .. } if !query.trim().is_empty() => {
            format!("Search: {}", compact_line(query, 140))
        }
        ToolCallDetail::Subagent { role, .. } if !role.trim().is_empty() => {
            format!("Launch specialist: {}", role.trim())
        }
        ToolCallDetail::Generic { tool_name, .. } if !tool_name.trim().is_empty() => {
            if fallback_title.trim().is_empty() {
                tool_name.trim().to_string()
            } else {
                format!(
                    "{}: {}",
                    tool_name.trim(),
                    compact_line(fallback_title, 120)
                )
            }
        }
        _ => fallback,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use orbit_protocol::{
        AttentionState, ConditionalTimerState, GoalStatus, GuardianAssessment, GuardianDecision,
        GuardianRiskLevel, PlanApprovalPayload, SessionMode, TimerCondition,
    };

    fn make_agent(
        agent_id: &str,
        title: &str,
        status: AgentStatus,
        updated_at_ms: i64,
    ) -> AgentCardView {
        let dt = Utc
            .timestamp_millis_opt(updated_at_ms)
            .single()
            .expect("valid test timestamp");
        let is_running = matches!(
            status,
            AgentStatus::Initializing | AgentStatus::Running { .. }
        );
        let has_background_tasks = matches!(
            &status,
            AgentStatus::AutonomousWakeup { source, .. } if source.starts_with("waiting:")
        );
        let is_awaiting_permission = matches!(status, AgentStatus::AwaitingPermission { .. });
        let status_label = match &status {
            AgentStatus::Idle => "Idle".into(),
            AgentStatus::Running { .. } => "Working".into(),
            AgentStatus::AwaitingPermission { .. } => "Needs approval".into(),
            AgentStatus::Error { .. } => "Error".into(),
            AgentStatus::AutonomousWakeup { source, .. } => {
                if let Some(rest) = source.strip_prefix("waiting:") {
                    format!("Waiting for {}", rest.trim())
                } else {
                    format!("Waking ({source})")
                }
            }
            AgentStatus::Initializing => "Starting".into(),
            AgentStatus::Closed => "Closed".into(),
        };
        AgentCardView {
            agent_id: agent_id.into(),
            workspace_id: "ws_1".into(),
            cwd: "/tmp/repo".into(),
            title: title.into(),
            provider: ProviderKind::Codex,
            provider_label: "Codex".into(),
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Auto,
            status,
            status_label,
            is_running,
            has_background_tasks,
            has_armed_timers: has_background_tasks,
            has_running_subagents: false,
            is_awaiting_permission,
            parent_agent_id: None,
            parent_host_id: None,
            profile_id: None,
            background: false,
            auto_archive: false,
            attention: AttentionState::None,
            archived_at_rfc3339: None,
            labels: HashMap::new(),
            latest_seq: 5,
            created_at_rfc3339: Some(dt.to_rfc3339()),
            updated_at_rfc3339: dt.to_rfc3339(),
            is_pinned: false,
            seen_seq: 0,
            read_seq: 0,
            is_unread: false,
            queued_prompts: Vec::new(),
            handle: None,
            cluster_suggestion: None,
            worktree_branch: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        }
    }

    #[test]
    fn resolves_personal_agent_priority_primary_host_and_excludes_archived() {
        let mut archived_pa =
            make_agent("ag_archived", "Personal Router", AgentStatus::Idle, 9_000);
        archived_pa
            .labels
            .insert(LABEL_AGENT_ROLE.into(), AGENT_ROLE_PERSONAL_ROUTER.into());
        archived_pa.archived_at_rfc3339 = Some("2026-11-26T00:00:00Z".into());

        let mut handle_pa = make_agent("ag_handle", "My Assistant", AgentStatus::Idle, 8_000);
        handle_pa.handle = Some("router".into());

        let mut role_pa_b = make_agent("ag_role_b", "PA on B", AgentStatus::Idle, 7_000);
        role_pa_b
            .labels
            .insert(LABEL_AGENT_ROLE.into(), AGENT_ROLE_PERSONAL_ROUTER.into());

        let mut role_pa_a = make_agent("ag_role_a", "PA on A", AgentStatus::Idle, 6_000);
        role_pa_a
            .labels
            .insert(LABEL_AGENT_ROLE.into(), AGENT_ROLE_PERSONAL_ROUTER.into());

        let empty_read = HashMap::new();
        let empty_preview = HashMap::new();

        // Role label beats handle even when handle is on primary host.
        let host_b_agents = [archived_pa.clone(), role_pa_b.clone()];
        let hosts_tier = vec![
            BriefingHostInput {
                host_id: "h_a",
                host_name: "Host A",
                online: true,
                agents: std::slice::from_ref(&handle_pa),
                pending_permissions: &[],
                conditional_timers: &[],
                goal_runs: &[],
                agent_read_at_ms: &empty_read,
                step_previews: &empty_preview,
            },
            BriefingHostInput {
                host_id: "h_b",
                host_name: "Host B",
                online: true,
                agents: &host_b_agents,
                pending_permissions: &[],
                conditional_timers: &[],
                goal_runs: &[],
                agent_read_at_ms: &empty_read,
                step_previews: &empty_preview,
            },
        ];
        let view = build_personal_agent_briefing(&hosts_tier, Some("h_a"), 10_000);
        assert_eq!(
            view.personal_agent.as_ref().map(|p| p.agent_id.as_str()),
            Some("ag_role_b")
        );

        // When both hosts have role-labeled PA, primary_router_host_id wins.
        let hosts_primary = vec![
            BriefingHostInput {
                host_id: "h_a",
                host_name: "Host A",
                online: true,
                agents: std::slice::from_ref(&role_pa_a),
                pending_permissions: &[],
                conditional_timers: &[],
                goal_runs: &[],
                agent_read_at_ms: &empty_read,
                step_previews: &empty_preview,
            },
            BriefingHostInput {
                host_id: "h_b",
                host_name: "Host B",
                online: true,
                agents: std::slice::from_ref(&role_pa_b),
                pending_permissions: &[],
                conditional_timers: &[],
                goal_runs: &[],
                agent_read_at_ms: &empty_read,
                step_previews: &empty_preview,
            },
        ];
        let view_primary = build_personal_agent_briefing(&hosts_primary, Some("h_a"), 10_000);
        assert_eq!(
            view_primary
                .personal_agent
                .as_ref()
                .map(|p| (p.host_id.as_str(), p.agent_id.as_str())),
            Some(("h_a", "ag_role_a"))
        );
        assert!(view_primary.hosts[0].is_personal_agent_host);
        assert_eq!(view_primary.hosts[0].host_id, "h_a");
    }

    #[test]
    fn projects_cross_host_missions_decisions_waiting_and_since_last_visit() {
        let now_ms = 1_700_000_000_000_i64;
        let read_ms = now_ms - 60_000;

        let mut pa = make_agent(
            "ag_pa",
            "Personal Agent",
            AgentStatus::Idle,
            now_ms - 10_000,
        );
        pa.labels
            .insert(LABEL_AGENT_ROLE.into(), AGENT_ROLE_PERSONAL_ROUTER.into());

        // Direct same-host PA child (finished AFTER PA read_ms -> counts in since_last_visit.completed)
        let mut local_done =
            make_agent("ag_m1", "Fix login bug", AgentStatus::Idle, now_ms - 5_000);
        local_done.parent_agent_id = Some("ag_pa".into());
        local_done.task_summary = Some("Merged fix and verified tests".into());

        // Direct cross-host PA child on host_b (running) with nested child awaiting permission
        let mut remote_root = make_agent(
            "ag_m2",
            "Refactor indexer",
            AgentStatus::Running {
                turn_id: "t1".into(),
            },
            now_ms - 2_000,
        );
        remote_root.parent_agent_id = Some("ag_pa".into());
        remote_root.parent_host_id = Some("host_a".into());
        remote_root.worktree_branch = Some("feat/indexer".into());

        let mut remote_leaf = make_agent(
            "ag_m2_sub",
            "Run migration",
            AgentStatus::AwaitingPermission {
                turn_id: "t2".into(),
                request_id: "req_tier3".into(),
            },
            now_ms - 1_000,
        );
        remote_leaf.parent_agent_id = Some("ag_m2".into());
        // parent_host_id None -> same host as child (host_b), which matches remote_root!

        // Non-PA foreground running session + errored session
        let other_running = make_agent(
            "ag_other_run",
            "Ad-hoc exploration",
            AgentStatus::Running {
                turn_id: "t3".into(),
            },
            now_ms - 3_000,
        );
        let other_err = make_agent(
            "ag_other_err",
            "Broken build",
            AgentStatus::Error {
                message: "compiler crashed".into(),
                recoverable: true,
            },
            now_ms - 4_000,
        );
        let other_plan = make_agent(
            "ag_other_plan",
            "Design spec",
            AgentStatus::AwaitingPermission {
                turn_id: "t4".into(),
                request_id: "req_plan".into(),
            },
            now_ms - 8_000,
        );

        let rec = RouterRecommendation {
            decision: "deny".into(),
            reason: "Drops production table".into(),
            created_at: Utc.timestamp_millis_opt(now_ms - 500).single().expect("ts"),
        };
        let perm_tier3 = PendingPermissionSummary {
            agent_id: "ag_m2_sub".into(),
            agent_title: "Run migration".into(),
            workspace_id: "ws_b".into(),
            request_id: "req_tier3".into(),
            title: "Drop table".into(),
            detail: ToolCallDetail::Shell {
                command: "psql -c 'DROP TABLE users'".into(),
                cwd: None,
                exit_code: None,
                output_preview: None,
            },
            allowed_decisions: vec!["allow".into(), "deny".into()],
            guardian_assessment: Some(GuardianAssessment {
                risk_level: GuardianRiskLevel::High,
                decision: GuardianDecision::RequireHuman,
                confidence: 0.99,
                classifier_tier: "deterministic_tier1".into(),
                reason: "Destructive SQL".into(),
                signals: vec![],
                latency_ms: 1,
            }),
            plan_approval: None,
            human_only: true,
            router_recommendation: Some(rec.clone()),
            created_at: Utc
                .timestamp_millis_opt(now_ms - 1_000)
                .single()
                .expect("ts"),
        };

        let perm_plan = PendingPermissionSummary {
            agent_id: "ag_other_plan".into(),
            agent_title: "Design spec".into(),
            workspace_id: "ws_a".into(),
            request_id: "req_plan".into(),
            title: "Plan".into(),
            detail: ToolCallDetail::Generic {
                tool_name: "ExitPlanMode".into(),
                arguments_json: "{}".into(),
            },
            allowed_decisions: vec!["allow".into(), "deny".into()],
            guardian_assessment: None,
            plan_approval: Some(PlanApprovalPayload {
                plan_markdown: "# Plan".into(),
                plan_title: Some("Migration rollout plan".into()),
                plan_file_path: None,
                summary: None,
                plan_file_backed: false,
                allowed_prompts: Vec::new(),
                source: None,
            }),
            human_only: true,
            router_recommendation: None,
            created_at: Utc
                .timestamp_millis_opt(now_ms - 8_000)
                .single()
                .expect("ts"),
        };

        let pa_timer = ConditionalTimerRecord {
            timer_id: "tm_1".into(),
            agent_id: "ag_pa".into(),
            workspace_id: "ws_a".into(),
            prompt: "Check CI status".into(),
            duration_seconds: 300,
            condition: TimerCondition::AllSubagentsSettled,
            state: ConditionalTimerState::Armed,
            created_at_ms: (now_ms - 10_000) as u64,
            fire_at_ms: (now_ms + 290_000) as u64,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        let pa_goal = GoalRunRecord {
            goal_id: "goal_1".into(),
            agent_id: "ag_pa".into(),
            workspace_id: "ws_a".into(),
            objective: "Keep main green".into(),
            criteria: Vec::new(),
            verify_command: None,
            max_iterations: 10,
            current_iteration: 2,
            max_duration_secs: 0,
            status: GoalStatus::Running,
            last_verify_exit_code: None,
            last_verify_output: None,
            consecutive_identical_failures: 0,
            pause_reason: None,
            paused_by_user: false,
            active_duration_secs: 45,
            started_at_ms: (now_ms - 45_000) as u64,
            updated_at_ms: now_ms as u64,
            finished_at_ms: None,
        };

        let mut read_a = HashMap::new();
        read_a.insert("ag_pa".to_string(), read_ms);
        let empty_read = HashMap::new();
        let empty_preview = HashMap::new();

        let hosts_a_agents = vec![pa, local_done, other_running, other_err, other_plan];
        let hosts_b_agents = vec![remote_root, remote_leaf];

        let view = build_personal_agent_briefing(
            &[
                BriefingHostInput {
                    host_id: "host_a",
                    host_name: "Mac Studio",
                    online: true,
                    agents: &hosts_a_agents,
                    pending_permissions: &[perm_plan],
                    conditional_timers: &[pa_timer],
                    goal_runs: &[pa_goal],
                    agent_read_at_ms: &read_a,
                    step_previews: &empty_preview,
                },
                BriefingHostInput {
                    host_id: "host_b",
                    host_name: "Linux Devbox",
                    online: true,
                    agents: &hosts_b_agents,
                    pending_permissions: &[perm_tier3],
                    conditional_timers: &[],
                    goal_runs: &[],
                    agent_read_at_ms: &empty_read,
                    step_previews: &empty_preview,
                },
            ],
            Some("host_a"),
            now_ms,
        );

        // Decision ordering: plan > permission > error
        let kinds: Vec<BriefingDecisionKind> = view.needs_you.iter().map(|d| d.kind).collect();
        assert_eq!(
            kinds,
            vec![
                BriefingDecisionKind::Plan,
                BriefingDecisionKind::Permission,
                BriefingDecisionKind::Error
            ]
        );
        assert!(view.needs_you[1].dispatched_by_personal_agent);
        assert_eq!(view.needs_you[1].router_recommendation, Some(rec));
        assert!(!view.needs_you[0].dispatched_by_personal_agent);

        // Missions: remote awaiting mission sorts before local done mission
        assert_eq!(view.missions.len(), 2);
        assert_eq!(view.missions[0].root.agent_id, "ag_m2");
        assert_eq!(view.missions[0].workers.len(), 1);
        assert_eq!(view.missions[0].workers[0].agent_id, "ag_m2_sub");
        assert_eq!(view.missions[0].state, BriefingMissionState::Awaiting);
        assert_eq!(view.missions[0].done, 0);
        assert_eq!(view.missions[0].total, 2);
        assert_eq!(view.missions[0].host_names, vec!["Linux Devbox"]);

        assert_eq!(view.missions[1].root.agent_id, "ag_m1");
        assert_eq!(view.missions[1].state, BriefingMissionState::Done);
        assert_eq!(view.missions[1].done, 1);
        assert_eq!(view.missions[1].total, 1);

        // Other active includes non-PA running + awaiting plan session, excludes PA and PA workers
        assert_eq!(view.other_active.len(), 2);
        assert_eq!(view.other_active[0].agent_id, "ag_other_plan");
        assert_eq!(view.other_active[1].agent_id, "ag_other_run");

        // Waiting includes armed timer and active goal on PA
        assert_eq!(view.waiting.len(), 2);
        assert_eq!(view.waiting[0].kind, BriefingWaitKind::Timer);
        assert_eq!(view.waiting[1].kind, BriefingWaitKind::Goal);

        // Since last visit: ag_m1 completed after PA read_ms
        assert_eq!(view.since_last_visit.completed, 1);
        assert_eq!(view.since_last_visit.auto_handled, 0);

        // Counts
        assert_eq!(view.counts.needs_you, 3);
        assert_eq!(view.counts.running, 2); // ag_m2 + ag_other_run
        assert_eq!(view.counts.done_recent, 1);
        assert_eq!(view.counts.hosts_online, 2);
        assert_eq!(view.counts.hosts_total, 2);

        // Verify snake_case JSON wire shape
        let json = serde_json::to_value(&view).expect("serialize briefing");
        assert_eq!(json["needs_you"][0]["kind"], "plan");
        assert_eq!(json["needs_you"][1]["kind"], "permission");
        assert_eq!(
            json["needs_you"][1]["router_recommendation"]["decision"],
            "deny"
        );
        assert_eq!(json["missions"][0]["state"], "awaiting");
        assert_eq!(json["waiting"][0]["kind"], "timer");
    }

    #[test]
    fn non_running_idle_closed_and_timer_waiting_workers_never_mark_running() {
        let now_ms = 1_700_000_000_000_i64;

        let mut pa = make_agent(
            "ag_pa",
            "Personal Agent",
            AgentStatus::Idle,
            now_ms - 10_000,
        );
        pa.labels
            .insert(LABEL_AGENT_ROLE.into(), AGENT_ROLE_PERSONAL_ROUTER.into());

        // 1. Idle PA worker with latest_seq == 0 -> Idle worker, mission must NOT be Running.
        let mut idle_empty = make_agent(
            "ag_idle_empty",
            "Fresh worker",
            AgentStatus::Idle,
            now_ms - 9_000,
        );
        idle_empty.parent_agent_id = Some("ag_pa".into());
        idle_empty.latest_seq = 0;

        // 2. Idle PA worker waiting on background task -> Idle worker, mission must NOT be Running.
        let mut waiting_worker = make_agent(
            "ag_bg_wait",
            "Waiting worker",
            AgentStatus::AutonomousWakeup {
                turn_id: "t_wait".into(),
                source: "waiting:CI job".into(),
            },
            now_ms - 8_000,
        );
        waiting_worker.parent_agent_id = Some("ag_pa".into());

        // 3. Closed PA worker (even with lingering timer flag) -> Done worker, mission Done.
        let mut closed_worker = make_agent(
            "ag_closed",
            "Stopped worker",
            AgentStatus::Closed,
            now_ms - 7_000,
        );
        closed_worker.parent_agent_id = Some("ag_pa".into());
        closed_worker.has_armed_timers = true;

        // 4. Finished parent (Idle, latest_seq > 0) with has_running_subagents=true ONLY because
        //    its managed child is Running -> parent stays Done, child is Running, running count = 1.
        let mut done_parent = make_agent(
            "ag_parent_done",
            "Parent finished",
            AgentStatus::Idle,
            now_ms - 6_000,
        );
        done_parent.parent_agent_id = Some("ag_pa".into());
        done_parent.has_running_subagents = true;

        let mut running_child = make_agent(
            "ag_child_run",
            "Child running",
            AgentStatus::Running {
                turn_id: "t_child".into(),
            },
            now_ms - 1_000,
        );
        running_child.parent_agent_id = Some("ag_parent_done".into());

        // 5. Idle PA worker with provider-native running subagent (no active managed child) -> Running.
        let mut native_sub_worker = make_agent(
            "ag_native_sub",
            "Native subagent host",
            AgentStatus::Idle,
            now_ms - 2_000,
        );
        native_sub_worker.parent_agent_id = Some("ag_pa".into());
        native_sub_worker.has_running_subagents = true;

        // 6. Active Initializing + active AutonomousWakeup (non-waiting) PA workers -> Running.
        let mut init_worker = make_agent(
            "ag_init",
            "Starting worker",
            AgentStatus::Initializing,
            now_ms - 3_000,
        );
        init_worker.parent_agent_id = Some("ag_pa".into());

        let mut wakeup_worker = make_agent(
            "ag_wakeup",
            "Waking worker",
            AgentStatus::AutonomousWakeup {
                turn_id: "t_wake".into(),
                source: "timer".into(),
            },
            now_ms - 4_000,
        );
        wakeup_worker.parent_agent_id = Some("ag_pa".into());

        // 7. Non-PA Idle thread with armed timer -> must NOT appear in `other_active`.
        let mut other_timer_idle = make_agent(
            "ag_other_timer",
            "Idle timer watcher",
            AgentStatus::Idle,
            now_ms - 5_000,
        );
        other_timer_idle.has_armed_timers = true;
        other_timer_idle.has_background_tasks = true;

        let agents = vec![
            pa,
            idle_empty,
            waiting_worker,
            closed_worker,
            done_parent,
            running_child,
            native_sub_worker,
            init_worker,
            wakeup_worker,
            other_timer_idle,
        ];
        let empty_read = HashMap::new();
        let empty_preview = HashMap::new();

        let view = build_personal_agent_briefing(
            &[BriefingHostInput {
                host_id: "h1",
                host_name: "Mac Studio",
                online: true,
                agents: &agents,
                pending_permissions: &[],
                conditional_timers: &[],
                goal_runs: &[],
                agent_read_at_ms: &empty_read,
                step_previews: &empty_preview,
            }],
            Some("h1"),
            now_ms,
        );

        let mission_by_root: HashMap<&str, &BriefingMission> = view
            .missions
            .iter()
            .map(|m| (m.root.agent_id.as_str(), m))
            .collect();

        // Idle & waiting & closed missions must all be Done (never Running).
        assert_eq!(
            mission_by_root["ag_idle_empty"].root.state,
            BriefingWorkerState::Idle
        );
        assert_eq!(
            mission_by_root["ag_idle_empty"].state,
            BriefingMissionState::Done
        );

        assert_eq!(
            mission_by_root["ag_bg_wait"].root.state,
            BriefingWorkerState::Idle
        );
        assert_eq!(
            mission_by_root["ag_bg_wait"].state,
            BriefingMissionState::Done
        );

        assert_eq!(
            mission_by_root["ag_closed"].root.state,
            BriefingWorkerState::Done
        );
        assert_eq!(
            mission_by_root["ag_closed"].state,
            BriefingMissionState::Done
        );

        // Finished parent with running managed child: parent stays Done, child is Running, mission is Running.
        let mixed = mission_by_root["ag_parent_done"];
        assert_eq!(mixed.root.state, BriefingWorkerState::Done);
        assert_eq!(mixed.workers.len(), 1);
        assert_eq!(mixed.workers[0].agent_id, "ag_child_run");
        assert_eq!(mixed.workers[0].state, BriefingWorkerState::Running);
        assert_eq!(mixed.state, BriefingMissionState::Running);

        // Provider-native running subagent, Initializing, and active Wakeup all stay Running.
        assert_eq!(
            mission_by_root["ag_native_sub"].root.state,
            BriefingWorkerState::Running
        );
        assert_eq!(
            mission_by_root["ag_init"].root.state,
            BriefingWorkerState::Running
        );
        assert_eq!(
            mission_by_root["ag_wakeup"].root.state,
            BriefingWorkerState::Running
        );

        // Non-PA idle timer watcher must NOT leak into `other_active`.
        assert!(view.other_active.is_empty());

        // Running count must be exactly 4: ag_child_run + ag_native_sub + ag_init + ag_wakeup
        // (done_parent is NOT double-counted).
        assert_eq!(view.counts.running, 4);
    }
}
