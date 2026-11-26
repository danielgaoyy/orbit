//! Unified Fleet Sidebar project tree, Launchpad workspace candidates, Attention & Pinned lists
//! shared across Desktop, iOS, Android, and HarmonyOS.

use std::collections::{HashMap, HashSet};

use chrono::DateTime;
use orbit_protocol::{
    AttentionState, DiscoveredProject, GitStatusSnapshot, NestedGitRepo, WorkspaceSummary,
};
use serde::{Deserialize, Serialize};

use crate::contract::GitReposInspectionView;
use crate::viewmodel::{AgentCardView, ClientProjectView, cwd_paths_equivalent};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarProjectSort {
    #[default]
    Recent,
    Alphabetical,
    Manual,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarThreadFilter {
    #[default]
    All,
    ActiveUnread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarConversationView {
    pub host_id: String,
    pub agent: AgentCardView,
    pub is_worktree: bool,
    pub is_pinned: bool,
    pub is_unread: bool,
    pub seen_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarWorkspaceGroupView {
    pub workspace_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_goal: Option<String>,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    pub is_worktree: bool,
    #[serde(default)]
    pub board_revision: u64,
    #[serde(default)]
    pub has_conflict: bool,
    pub peer_count: u32,
    pub running_count: u32,
    pub unread_count: u32,
    pub is_multi_agent: bool,
    #[serde(default)]
    pub is_curated: bool,
    pub conversations: Vec<SidebarConversationView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarProjectGroupView {
    pub id: String,
    pub host_id: String,
    pub name: String,
    pub primary_cwd: String,
    pub directories: Vec<String>,
    pub git_branch: Option<String>,
    #[serde(default)]
    pub is_git_repo: bool,
    #[serde(default)]
    pub nested_git_repos: Vec<NestedGitRepo>,
    pub pinned_project_id: Option<String>,
    pub is_home_workspace: bool,
    pub sort_weight_ms: i64,
    #[serde(default)]
    pub sort_order_key: String,
    pub conversations: Vec<SidebarConversationView>,
    #[serde(default)]
    pub workspaces: Vec<SidebarWorkspaceGroupView>,
    #[serde(default)]
    pub unclassified_conversations: Vec<SidebarConversationView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchpadProjectView {
    pub id: String,
    pub host_id: String,
    pub name: String,
    pub path: String,
    pub branch: Option<String>,
    #[serde(default)]
    pub is_git_repo: bool,
    #[serde(default)]
    pub nested_git_repos: Vec<NestedGitRepo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetSidebarView {
    pub revision: u64,
    pub projects: Vec<SidebarProjectGroupView>,
    pub launchpad_projects: Vec<LaunchpadProjectView>,
    pub needs_attention: Vec<SidebarConversationView>,
    pub pinned_conversations: Vec<SidebarConversationView>,
}

pub struct SidebarHostInput<'a> {
    pub host_id: &'a str,
    pub host_name: &'a str,
    pub workspaces: &'a [WorkspaceSummary],
    pub discovered_projects: &'a [DiscoveredProject],
    pub agents: Vec<(AgentCardView, i64)>,
    pub active_git_branch: Option<String>,
    pub git_repos_by_cwd: &'a HashMap<String, GitReposInspectionView>,
    pub workspace_conflict_ids: &'a HashSet<String>,
}

pub struct FleetSidebarParams<'a> {
    pub revision: u64,
    pub hosts: Vec<SidebarHostInput<'a>>,
    pub pinned_projects: &'a [ClientProjectView],
    pub project_order: &'a [String],
    pub sort: SidebarProjectSort,
    pub filter: SidebarThreadFilter,
    pub query: &'a str,
    pub active_host_id: Option<&'a str>,
    pub active_agent_id: Option<&'a str>,
    pub active_cwd: Option<&'a str>,
}

pub fn normalize_project_path(raw: &str) -> String {
    if raw.trim().is_empty() || raw == "~" {
        return "~".to_string();
    }
    if raw == "/" {
        return "/".to_string();
    }
    let clean = raw.trim_end_matches('/');
    if clean.is_empty() {
        "/".to_string()
    } else {
        clean.to_string()
    }
}

pub fn basename_of_project_path(raw: &str) -> String {
    let clean = normalize_project_path(raw);
    if clean == "~" || clean == "/" {
        return clean;
    }
    clean
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(&clean)
        .to_string()
}

pub fn is_bare_home_dir(raw: &str) -> bool {
    let norm = normalize_project_path(raw);
    if norm == "~" || norm == "/" || norm == "/root" {
        return true;
    }
    if let Some(rest) = norm.strip_prefix("/Users/") {
        return !rest.is_empty() && !rest.contains('/');
    }
    if let Some(rest) = norm.strip_prefix("/home/") {
        return !rest.is_empty() && !rest.contains('/');
    }
    false
}

pub fn matches_project_directory(directories: &[String], cwd: &str) -> bool {
    let clean_cwd = normalize_project_path(cwd);
    for dir in directories {
        let clean_dir = normalize_project_path(dir);
        if clean_dir == clean_cwd || cwd_paths_equivalent(&clean_dir, &clean_cwd) {
            return true;
        }
        let repo_base = basename_of_project_path(&clean_dir);
        if !repo_base.is_empty()
            && repo_base != "~"
            && clean_cwd.contains(&format!("/worktrees/{repo_base}-"))
        {
            return true;
        }
    }
    false
}

fn path_is_inside_directory(dir: &str, candidate: &str) -> bool {
    let clean_dir = normalize_project_path(dir);
    let clean_cand = normalize_project_path(candidate);
    if clean_dir == "~" || clean_dir == "/" || is_bare_home_dir(&clean_dir) {
        return false;
    }
    clean_cand == clean_dir
        || cwd_paths_equivalent(&clean_dir, &clean_cand)
        || clean_cand.starts_with(&format!("{clean_dir}/"))
}

fn resolve_git_capability_for_path(
    git_repos_by_cwd: &HashMap<String, GitReposInspectionView>,
    discovered_projects: &[DiscoveredProject],
    workspaces: &[WorkspaceSummary],
    raw_path: &str,
) -> (bool, Vec<NestedGitRepo>) {
    let clean = normalize_project_path(raw_path);
    if let Some(insp) = git_repos_by_cwd.get(&clean).or_else(|| {
        git_repos_by_cwd
            .iter()
            .find(|(k, _)| cwd_paths_equivalent(k, &clean))
            .map(|(_, v)| v)
    }) {
        return (insp.is_git_repo, insp.nested_git_repos.clone());
    }
    if let Some(dp) = discovered_projects
        .iter()
        .find(|p| cwd_paths_equivalent(&p.path, &clean))
    {
        return (dp.is_git_repo, dp.nested_git_repos.clone());
    }
    if workspaces
        .iter()
        .any(|w| !w.is_worktree && cwd_paths_equivalent(&w.cwd, &clean) && w.git_branch.is_some())
    {
        return (true, Vec::new());
    }
    (false, Vec::new())
}

fn resolve_worktree_base_path(raw_path: &str, known_repo_bases: &[String]) -> String {
    let clean = normalize_project_path(raw_path);
    if clean == "~" || clean == "/" {
        return clean;
    }
    if let Some(exact_parent) = known_repo_bases
        .iter()
        .find(|base| clean == **base || clean.starts_with(&format!("{base}/")))
    {
        return exact_parent.clone();
    }
    if let Some(idx) = clean.find("/worktrees/") {
        let after = &clean[idx + "/worktrees/".len()..];
        let seg = after.split('/').next().unwrap_or("");
        if !seg.is_empty()
            && let Some(matched) = known_repo_bases.iter().find(|base| {
                let repo_name = basename_of_project_path(base);
                seg == repo_name || seg.starts_with(&format!("{repo_name}-"))
            })
        {
            return matched.clone();
        }
    }
    clean
}

fn parse_rfc3339_ms(raw: &str) -> i64 {
    DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.timestamp_millis())
        .unwrap_or(0)
}

pub fn resolve_branch_for_path(
    workspaces: &[WorkspaceSummary],
    discovered_projects: &[DiscoveredProject],
    active_cwd: Option<&str>,
    active_git_branch: Option<&str>,
    raw_path: &str,
) -> Option<String> {
    if let (Some(a_cwd), Some(branch)) = (active_cwd, active_git_branch)
        && !branch.trim().is_empty()
        && cwd_paths_equivalent(raw_path, a_cwd)
    {
        return Some(branch.trim().to_string());
    }
    if let Some(ws) = workspaces
        .iter()
        .find(|w| !w.is_worktree && cwd_paths_equivalent(&w.cwd, raw_path))
        && let Some(ref b) = ws.git_branch
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_string());
    }
    if let Some(ws) = workspaces
        .iter()
        .find(|w| cwd_paths_equivalent(&w.cwd, raw_path))
        && let Some(ref b) = ws.git_branch
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_string());
    }
    if let Some(dp) = discovered_projects
        .iter()
        .find(|p| cwd_paths_equivalent(&p.path, raw_path))
        && let Some(ref b) = dp.git_branch
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_string());
    }
    None
}

pub fn active_git_branch_from_state(
    git_status: Option<&GitStatusSnapshot>,
    workspaces: &[WorkspaceSummary],
    discovered_projects: &[DiscoveredProject],
    cwd: &str,
) -> Option<String> {
    if let Some(st) = git_status
        && let Some(ref b) = st.branch
        && !b.trim().is_empty()
    {
        return Some(b.trim().to_string());
    }
    resolve_branch_for_path(workspaces, discovered_projects, None, None, cwd)
}

fn matches_conversation_filter(
    conv: &SidebarConversationView,
    filter: SidebarThreadFilter,
) -> bool {
    match filter {
        SidebarThreadFilter::All => true,
        SidebarThreadFilter::ActiveUnread => {
            conv.agent.is_running
                || conv.agent.has_background_tasks
                || conv.agent.has_running_subagents
                || conv.agent.is_awaiting_permission
                || conv.is_unread
        }
    }
}

/// Parsed representation of `FleetSidebarRequest.query`, supporting free-text keywords
/// combined with optional `@host:<substring>` and `@provider:<substring>` inline tokens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSidebarQuery {
    pub host_filters: Vec<String>,
    pub provider_filters: Vec<String>,
    pub terms: Vec<String>,
}

impl ParsedSidebarQuery {
    pub fn parse(raw: &str) -> Self {
        let mut parsed = Self::default();
        for tok in raw.split_whitespace() {
            let lower = tok.to_lowercase();
            if let Some(rest) = lower.strip_prefix("@host:") {
                if !rest.is_empty() {
                    parsed.host_filters.push(rest.to_string());
                }
            } else if let Some(rest) = lower.strip_prefix("@provider:") {
                if !rest.is_empty() {
                    parsed.provider_filters.push(rest.to_string());
                }
            } else if !lower.is_empty() {
                parsed.terms.push(lower);
            }
        }
        parsed
    }

    pub fn is_empty(&self) -> bool {
        self.host_filters.is_empty() && self.provider_filters.is_empty() && self.terms.is_empty()
    }

    pub fn matches_host(&self, host_id: &str, host_name: &str) -> bool {
        if self.host_filters.is_empty() {
            return true;
        }
        let id_l = host_id.to_lowercase();
        let name_l = host_name.to_lowercase();
        self.host_filters
            .iter()
            .all(|f| name_l.contains(f) || id_l.contains(f))
    }

    pub fn matches_provider(&self, agent: &AgentCardView) -> bool {
        if self.provider_filters.is_empty() {
            return true;
        }
        let label_l = agent.provider_label.to_lowercase();
        let slug_l = agent.provider.slug().to_lowercase();
        let acp_l = agent
            .acp_agent_id
            .as_deref()
            .map(str::to_lowercase)
            .unwrap_or_default();
        self.provider_filters.iter().all(|f| {
            label_l.contains(f) || slug_l.contains(f) || (!acp_l.is_empty() && acp_l.contains(f))
        })
    }
}

fn opt_contains(opt: Option<&str>, needle: &str) -> bool {
    opt.is_some_and(|s| s.to_lowercase().contains(needle))
}

struct SidebarAgentSearchTarget<'a> {
    host_id: &'a str,
    host_name: &'a str,
    agent: &'a AgentCardView,
    ws_meta: Option<&'a WorkspaceSummary>,
    project_name: Option<&'a str>,
    project_cwd: Option<&'a str>,
    project_branch: Option<&'a str>,
}

fn matches_agent_query_full(
    parsed: &ParsedSidebarQuery,
    target: &SidebarAgentSearchTarget<'_>,
) -> bool {
    let SidebarAgentSearchTarget {
        host_id,
        host_name,
        agent,
        ws_meta,
        project_name,
        project_cwd,
        project_branch,
    } = *target;
    if parsed.is_empty() {
        return true;
    }
    if !parsed.matches_host(host_id, host_name) || !parsed.matches_provider(agent) {
        return false;
    }
    if parsed.terms.is_empty() {
        return true;
    }

    let title = if agent.title.trim().is_empty() {
        agent.provider_label.as_str()
    } else {
        agent.title.as_str()
    };
    let title_l = title.to_lowercase();
    let cwd_l = agent.cwd.to_lowercase();
    let base_l = basename_of_project_path(&agent.cwd).to_lowercase();
    let prov_label_l = agent.provider_label.to_lowercase();
    let prov_slug_l = agent.provider.slug().to_lowercase();
    let host_id_l = host_id.to_lowercase();
    let host_name_l = host_name.to_lowercase();
    let label_wt_branch = agent
        .labels
        .get("orbit.worktree_branch")
        .map(String::as_str);

    parsed.terms.iter().all(|term| {
        title_l.contains(term)
            || cwd_l.contains(term)
            || base_l.contains(term)
            || prov_label_l.contains(term)
            || prov_slug_l.contains(term)
            || host_name_l.contains(term)
            || host_id_l.contains(term)
            || opt_contains(agent.acp_agent_id.as_deref(), term)
            || opt_contains(agent.handle.as_deref(), term)
            || opt_contains(agent.worktree_branch.as_deref(), term)
            || opt_contains(label_wt_branch, term)
            || opt_contains(ws_meta.map(|w| w.title.as_str()), term)
            || opt_contains(ws_meta.and_then(|w| w.mission_goal.as_deref()), term)
            || opt_contains(ws_meta.and_then(|w| w.git_branch.as_deref()), term)
            || opt_contains(project_name, term)
            || opt_contains(project_cwd, term)
            || opt_contains(project_branch, term)
    })
}

fn matches_project_query(
    parsed: &ParsedSidebarQuery,
    host_id: &str,
    host_name: &str,
    group: &SidebarProjectGroupView,
    curated_ws: &[&WorkspaceSummary],
) -> bool {
    if parsed.is_empty() {
        return true;
    }
    if !parsed.matches_host(host_id, host_name) || !parsed.provider_filters.is_empty() {
        return false;
    }
    if parsed.terms.is_empty() {
        return true;
    }
    let host_id_l = host_id.to_lowercase();
    let host_name_l = host_name.to_lowercase();
    let name_l = group.name.to_lowercase();
    let cwd_l = group.primary_cwd.to_lowercase();
    parsed.terms.iter().all(|term| {
        name_l.contains(term)
            || cwd_l.contains(term)
            || host_name_l.contains(term)
            || host_id_l.contains(term)
            || opt_contains(group.git_branch.as_deref(), term)
            || curated_ws.iter().any(|w| {
                w.title.to_lowercase().contains(term)
                    || opt_contains(w.mission_goal.as_deref(), term)
                    || opt_contains(w.git_branch.as_deref(), term)
            })
    })
}

pub fn build_fleet_sidebar(params: FleetSidebarParams<'_>) -> FleetSidebarView {
    let parsed_query = ParsedSidebarQuery::parse(params.query);
    let mut all_groups: Vec<SidebarProjectGroupView> = Vec::new();
    let mut fallback_discovered_groups: Vec<(String, SidebarProjectGroupView)> = Vec::new();
    let mut has_any_real_project = false;
    let mut launchpad_projects: Vec<LaunchpadProjectView> = Vec::new();
    let mut needs_attention: Vec<SidebarConversationView> = Vec::new();
    let mut pinned_with_order: Vec<(i64, SidebarConversationView)> = Vec::new();

    // Pre-resolve root ancestor conversations for any active Subagent awaiting permission across
    // the Fleet so subagent permission gates bubble onto their root Conversation in `Needs Attention`
    // without leaking subagents as standalone Left Sidebar threads.
    let mut subagent_approval_roots: HashSet<(String, String)> = HashSet::new();
    let has_awaiting_subagent = params.hosts.iter().any(|h| {
        h.agents.iter().any(|(a, _)| {
            !a.background
                && a.archived_at_rfc3339.is_none()
                && a.is_subagent()
                && a.is_awaiting_permission
        })
    });
    if has_awaiting_subagent {
        let fleet_by_key: HashMap<(&str, &str), &AgentCardView> = params
            .hosts
            .iter()
            .flat_map(|h| {
                h.agents
                    .iter()
                    .map(move |(a, _)| ((h.host_id, a.agent_id.as_str()), a))
            })
            .collect();
        for h in &params.hosts {
            for (a, _) in &h.agents {
                if a.background
                    || a.archived_at_rfc3339.is_some()
                    || !a.is_subagent()
                    || !a.is_awaiting_permission
                {
                    continue;
                }
                let mut cur_host = h.host_id;
                let mut cur_agent = a;
                let mut visited: HashSet<(&str, &str)> = HashSet::new();
                visited.insert((cur_host, cur_agent.agent_id.as_str()));
                for _ in 0..crate::subagents::MAX_SUBAGENT_LINEAGE_DEPTH {
                    let Some(pid) = cur_agent
                        .parent_agent_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                    else {
                        break;
                    };
                    let phost = cur_agent
                        .parent_host_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or(cur_host);
                    if !visited.insert((phost, pid)) {
                        break;
                    }
                    let Some(parent_ag) = fleet_by_key.get(&(phost, pid)).copied() else {
                        break;
                    };
                    cur_host = phost;
                    cur_agent = parent_ag;
                    if !cur_agent.is_subagent() {
                        if !cur_agent.background && cur_agent.archived_at_rfc3339.is_none() {
                            subagent_approval_roots
                                .insert((cur_host.to_string(), cur_agent.agent_id.clone()));
                        }
                        break;
                    }
                }
            }
        }
    }

    for host in &params.hosts {
        let host_id = host.host_id;
        let host_name = host.host_name;
        let host_pinned: Vec<&ClientProjectView> = params
            .pinned_projects
            .iter()
            .filter(|p| p.host_id == host_id)
            .collect();

        // Build known repository base paths longest-first so worktrees nest cleanly under parents.
        let mut non_worktree_bases: Vec<String> = Vec::new();
        let mut seen_bases: HashSet<String> = HashSet::new();
        for proj in &host_pinned {
            for dir in &proj.directories {
                let norm = normalize_project_path(dir);
                if !is_bare_home_dir(&norm)
                    && !norm.contains("/worktrees/")
                    && seen_bases.insert(norm.clone())
                {
                    non_worktree_bases.push(norm);
                }
            }
        }
        for dp in host.discovered_projects {
            let norm = normalize_project_path(&dp.path);
            if !is_bare_home_dir(&norm)
                && !norm.contains("/worktrees/")
                && seen_bases.insert(norm.clone())
            {
                non_worktree_bases.push(norm);
            }
        }
        for ws in host.workspaces {
            if !ws.is_worktree {
                let norm = normalize_project_path(&ws.cwd);
                if !is_bare_home_dir(&norm)
                    && !norm.contains("/worktrees/")
                    && seen_bases.insert(norm.clone())
                {
                    non_worktree_bases.push(norm);
                }
            }
        }
        non_worktree_bases.sort_by_key(|b| std::cmp::Reverse(b.len()));

        let ws_by_id: HashMap<&str, &WorkspaceSummary> = host
            .workspaces
            .iter()
            .map(|w| (w.workspace_id.as_str(), w))
            .collect();
        let ws_worktree_map: HashMap<String, bool> = host
            .workspaces
            .iter()
            .map(|w| (normalize_project_path(&w.cwd), w.is_worktree))
            .collect();

        let mut host_groups: Vec<SidebarProjectGroupView> = Vec::new();
        let mut group_idx_by_id: HashMap<String, usize> = HashMap::new();

        // 1. Pinned projects
        for proj in &host_pinned {
            let primary =
                normalize_project_path(proj.directories.first().map(String::as_str).unwrap_or("~"));
            let dirs: Vec<String> = if proj.directories.is_empty() {
                vec![primary.clone()]
            } else {
                proj.directories
                    .iter()
                    .map(|d| normalize_project_path(d))
                    .collect()
            };
            let name = if proj.name.trim().is_empty() {
                basename_of_project_path(&primary)
            } else {
                proj.name.clone()
            };
            let branch = resolve_branch_for_path(
                host.workspaces,
                host.discovered_projects,
                params.active_cwd,
                host.active_git_branch.as_deref(),
                &primary,
            );
            let (is_git_repo, nested_git_repos) = resolve_git_capability_for_path(
                host.git_repos_by_cwd,
                host.discovered_projects,
                host.workspaces,
                &primary,
            );
            let group = SidebarProjectGroupView {
                id: proj.project_id.clone(),
                host_id: host_id.to_string(),
                name,
                is_home_workspace: is_bare_home_dir(&primary),
                primary_cwd: primary,
                directories: dirs,
                git_branch: branch,
                is_git_repo,
                nested_git_repos,
                pinned_project_id: Some(proj.project_id.clone()),
                sort_weight_ms: proj.updated_at_ms.max(1),
                sort_order_key: proj.project_id.clone(),
                conversations: Vec::new(),
                workspaces: Vec::new(),
                unclassified_conversations: Vec::new(),
            };
            group_idx_by_id.insert(group.id.clone(), host_groups.len());
            host_groups.push(group);
        }

        // 2. Group active primary conversations (strictly excluding background tasks and subagents)
        for (raw_agent, pinned_order) in &host.agents {
            if raw_agent.background
                || raw_agent.archived_at_rfc3339.is_some()
                || raw_agent.is_subagent()
            {
                continue;
            }
            let mut agent = raw_agent.clone();
            let is_selected = params.active_host_id == Some(host_id)
                && params.active_agent_id == Some(agent.agent_id.as_str());
            if is_selected || agent.is_running || agent.is_awaiting_permission {
                agent.is_unread = false;
            } else if agent.attention != AttentionState::None {
                agent.is_unread = true;
            }
            if subagent_approval_roots.contains(&(host_id.to_string(), agent.agent_id.clone())) {
                agent.is_awaiting_permission = true;
            }

            let clean_cwd = normalize_project_path(&agent.cwd);
            let ws_meta = ws_by_id.get(agent.workspace_id.as_str()).copied();
            let is_worktree = agent.worktree_branch.is_some()
                || agent.labels.contains_key("orbit.worktree_branch")
                || ws_meta.is_some_and(|w| w.is_worktree)
                || ws_worktree_map
                    .get(&clean_cwd)
                    .copied()
                    .unwrap_or_else(|| clean_cwd.contains("/worktrees/"));

            let conv = SidebarConversationView {
                host_id: host_id.to_string(),
                is_pinned: agent.is_pinned,
                is_unread: agent.is_unread,
                seen_seq: agent.seen_seq,
                is_worktree,
                agent: agent.clone(),
            };

            if agent.is_awaiting_permission
                && matches_agent_query_full(
                    &parsed_query,
                    &SidebarAgentSearchTarget {
                        host_id,
                        host_name,
                        agent: &agent,
                        ws_meta,
                        project_name: None,
                        project_cwd: None,
                        project_branch: None,
                    },
                )
            {
                needs_attention.push(conv.clone());
            }

            // The Personal Agent has its own top-level sidebar entry, so its conversation is
            // never listed under Pinned or Projects (it still surfaces in Needs Attention).
            if crate::personal_agent::is_personal_agent_card(&agent) {
                continue;
            }
            if agent.is_pinned
                && matches_conversation_filter(&conv, params.filter)
                && matches_agent_query_full(
                    &parsed_query,
                    &SidebarAgentSearchTarget {
                        host_id,
                        host_name,
                        agent: &agent,
                        ws_meta,
                        project_name: None,
                        project_cwd: None,
                        project_branch: None,
                    },
                )
            {
                pinned_with_order.push((*pinned_order, conv.clone()));
            }

            // Prefer explicit Mission Workspace `project_root` (so nested-repo worktrees fold cleanly
            // under their parent non-git project), then direct directory match, then source_cwd match.
            let ws_project_root = ws_meta
                .and_then(|w| w.project_root.as_deref())
                .map(normalize_project_path)
                .filter(|p| !p.is_empty() && !is_bare_home_dir(p));
            let ws_source_cwd = agent
                .labels
                .get("orbit.worktree_source_cwd")
                .map(String::as_str)
                .or_else(|| ws_meta.and_then(|w| w.source_cwd.as_deref()))
                .map(normalize_project_path)
                .filter(|p| !p.is_empty() && !is_bare_home_dir(p));

            let mut matched_idx = ws_project_root
                .as_deref()
                .and_then(|root| {
                    host_groups
                        .iter()
                        .position(|g| matches_project_directory(&g.directories, root))
                })
                .or_else(|| {
                    host_groups
                        .iter()
                        .position(|g| matches_project_directory(&g.directories, &clean_cwd))
                })
                .or_else(|| {
                    ws_source_cwd.as_deref().and_then(|src| {
                        host_groups
                            .iter()
                            .position(|g| matches_project_directory(&g.directories, src))
                    })
                })
                .or_else(|| {
                    // Fold nested git repo worktrees under enclosing pinned/known project directories
                    // (pick the most specific / longest matching directory).
                    ws_source_cwd.as_deref().and_then(|src| {
                        host_groups
                            .iter()
                            .enumerate()
                            .filter_map(|(idx, g)| {
                                g.directories
                                    .iter()
                                    .filter(|d| path_is_inside_directory(d, src))
                                    .map(|d| (idx, d.len()))
                                    .max_by_key(|(_, len)| *len)
                            })
                            .max_by_key(|(_, len)| *len)
                            .map(|(idx, _)| idx)
                    })
                });

            if matched_idx.is_none() && (is_worktree || ws_project_root.is_some()) {
                let candidate_repo = ws_project_root
                    .clone()
                    .or_else(|| ws_source_cwd.clone())
                    .or_else(|| {
                        host.workspaces
                            .iter()
                            .find(|w| {
                                !w.is_worktree
                                    && matches_project_directory(
                                        std::slice::from_ref(&w.cwd),
                                        &clean_cwd,
                                    )
                            })
                            .map(|w| w.cwd.clone())
                    })
                    .or_else(|| {
                        host.discovered_projects
                            .iter()
                            .find(|dp| {
                                matches_project_directory(
                                    std::slice::from_ref(&dp.path),
                                    &clean_cwd,
                                )
                            })
                            .map(|dp| dp.path.clone())
                    })
                    .or_else(|| {
                        let base = resolve_worktree_base_path(&clean_cwd, &non_worktree_bases);
                        (base != clean_cwd).then_some(base)
                    });

                if let Some(raw_repo) = candidate_repo {
                    let clean_repo = normalize_project_path(&raw_repo);
                    let id = format!("{host_id}::{clean_repo}");
                    if let Some(&existing_idx) = group_idx_by_id.get(&id) {
                        matched_idx = Some(existing_idx);
                    } else {
                        let branch = resolve_branch_for_path(
                            host.workspaces,
                            host.discovered_projects,
                            params.active_cwd,
                            host.active_git_branch.as_deref(),
                            &clean_repo,
                        );
                        let (is_git_repo, nested_git_repos) = resolve_git_capability_for_path(
                            host.git_repos_by_cwd,
                            host.discovered_projects,
                            host.workspaces,
                            &clean_repo,
                        );
                        let is_home = is_bare_home_dir(&clean_repo);
                        let name = if is_home {
                            "Workspace (~)".to_string()
                        } else {
                            basename_of_project_path(&clean_repo)
                        };
                        let idx = host_groups.len();
                        group_idx_by_id.insert(id.clone(), idx);
                        host_groups.push(SidebarProjectGroupView {
                            id: id.clone(),
                            host_id: host_id.to_string(),
                            name,
                            is_home_workspace: is_home,
                            primary_cwd: clean_repo.clone(),
                            directories: vec![clean_repo],
                            git_branch: branch,
                            is_git_repo,
                            nested_git_repos,
                            pinned_project_id: None,
                            sort_weight_ms: 0,
                            sort_order_key: id,
                            conversations: Vec::new(),
                            workspaces: Vec::new(),
                            unclassified_conversations: Vec::new(),
                        });
                        matched_idx = Some(idx);
                    }
                }
            }

            let target_idx = if let Some(idx) = matched_idx {
                idx
            } else {
                let id = format!("{host_id}::{clean_cwd}");
                if let Some(&existing_idx) = group_idx_by_id.get(&id) {
                    existing_idx
                } else {
                    let branch = resolve_branch_for_path(
                        host.workspaces,
                        host.discovered_projects,
                        params.active_cwd,
                        host.active_git_branch.as_deref(),
                        &clean_cwd,
                    );
                    let (is_git_repo, nested_git_repos) = resolve_git_capability_for_path(
                        host.git_repos_by_cwd,
                        host.discovered_projects,
                        host.workspaces,
                        &clean_cwd,
                    );
                    let is_home = is_bare_home_dir(&clean_cwd);
                    let name = if is_home {
                        "Workspace (~)".to_string()
                    } else {
                        basename_of_project_path(&clean_cwd)
                    };
                    let idx = host_groups.len();
                    group_idx_by_id.insert(id.clone(), idx);
                    host_groups.push(SidebarProjectGroupView {
                        id: id.clone(),
                        host_id: host_id.to_string(),
                        name,
                        is_home_workspace: is_home,
                        primary_cwd: clean_cwd.clone(),
                        directories: vec![clean_cwd],
                        git_branch: branch,
                        is_git_repo,
                        nested_git_repos,
                        pinned_project_id: None,
                        sort_weight_ms: 0,
                        sort_order_key: id,
                        conversations: Vec::new(),
                        workspaces: Vec::new(),
                        unclassified_conversations: Vec::new(),
                    });
                    idx
                }
            };

            let ts_ms = parse_rfc3339_ms(&agent.updated_at_rfc3339);
            let group = &mut host_groups[target_idx];
            if ts_ms > group.sort_weight_ms {
                group.sort_weight_ms = ts_ms;
            }
            group.conversations.push(conv);
        }

        // 2.5 Ensure empty curated Mission Workspaces attach to or create their enclosing Project Group
        let mut curated_ws_by_group_idx: HashMap<usize, Vec<&WorkspaceSummary>> = HashMap::new();
        for ws in host.workspaces {
            if !ws.is_curated || ws.archived_at.is_some() || ws.background {
                continue;
            }
            let ws_root = ws
                .project_root
                .as_deref()
                .or(ws.source_cwd.as_deref())
                .unwrap_or(&ws.cwd);
            let clean_root = normalize_project_path(ws_root);
            let clean_ws_cwd = normalize_project_path(&ws.cwd);

            let matched_idx = host_groups
                .iter()
                .position(|g| matches_project_directory(&g.directories, &clean_root))
                .or_else(|| {
                    host_groups
                        .iter()
                        .position(|g| matches_project_directory(&g.directories, &clean_ws_cwd))
                })
                .or_else(|| {
                    host_groups
                        .iter()
                        .enumerate()
                        .filter_map(|(idx, g)| {
                            g.directories
                                .iter()
                                .filter(|d| path_is_inside_directory(d, &clean_root))
                                .map(|d| (idx, d.len()))
                                .max_by_key(|(_, len)| *len)
                        })
                        .max_by_key(|(_, len)| *len)
                        .map(|(idx, _)| idx)
                });

            let target_idx = if let Some(idx) = matched_idx {
                idx
            } else {
                let id = format!("{host_id}::{clean_root}");
                if let Some(&existing_idx) = group_idx_by_id.get(&id) {
                    existing_idx
                } else {
                    let branch = resolve_branch_for_path(
                        host.workspaces,
                        host.discovered_projects,
                        params.active_cwd,
                        host.active_git_branch.as_deref(),
                        &clean_root,
                    );
                    let (is_git_repo, nested_git_repos) = resolve_git_capability_for_path(
                        host.git_repos_by_cwd,
                        host.discovered_projects,
                        host.workspaces,
                        &clean_root,
                    );
                    let is_home = is_bare_home_dir(&clean_root);
                    let name = if is_home {
                        "Workspace (~)".to_string()
                    } else {
                        basename_of_project_path(&clean_root)
                    };
                    let idx = host_groups.len();
                    group_idx_by_id.insert(id.clone(), idx);
                    host_groups.push(SidebarProjectGroupView {
                        id: id.clone(),
                        host_id: host_id.to_string(),
                        name,
                        is_home_workspace: is_home,
                        primary_cwd: clean_root.clone(),
                        directories: vec![clean_root],
                        git_branch: branch,
                        is_git_repo,
                        nested_git_repos,
                        pinned_project_id: None,
                        sort_weight_ms: ws.updated_at.timestamp_millis(),
                        sort_order_key: id,
                        conversations: Vec::new(),
                        workspaces: Vec::new(),
                        unclassified_conversations: Vec::new(),
                    });
                    idx
                }
            };

            let ws_ts = ws.updated_at.timestamp_millis();
            if ws_ts > host_groups[target_idx].sort_weight_ms {
                host_groups[target_idx].sort_weight_ms = ws_ts;
            }
            curated_ws_by_group_idx
                .entry(target_idx)
                .or_default()
                .push(ws);
        }

        // 3. Collect fallback discovered repositories only for hosts with neither pinned projects
        //    nor active sessions/curated workspaces. These are ONLY surfaced later when the ENTIRE
        //    fleet has zero real projects (first-time onboarding), so adding a fresh unused host
        //    never floods an already-populated multi-host sidebar with auto-scanned directories.
        if !host_groups.is_empty() {
            has_any_real_project = true;
        } else {
            let mut added = 0usize;
            for dp in host.discovered_projects {
                if added >= 4 {
                    break;
                }
                let clean_path = normalize_project_path(&dp.path);
                if is_bare_home_dir(&clean_path) && host.discovered_projects.len() > 1 {
                    continue;
                }
                let id = format!("{host_id}::{clean_path}");
                if group_idx_by_id.contains_key(&id) {
                    continue;
                }
                let name = if dp.name.trim().is_empty() {
                    basename_of_project_path(&clean_path)
                } else {
                    dp.name.clone()
                };
                group_idx_by_id.insert(id.clone(), fallback_discovered_groups.len());
                fallback_discovered_groups.push((
                    host_name.to_string(),
                    SidebarProjectGroupView {
                        id: id.clone(),
                        host_id: host_id.to_string(),
                        name,
                        is_home_workspace: is_bare_home_dir(&clean_path),
                        primary_cwd: clean_path.clone(),
                        directories: vec![clean_path],
                        git_branch: dp.git_branch.clone(),
                        is_git_repo: dp.is_git_repo,
                        nested_git_repos: dp.nested_git_repos.clone(),
                        pinned_project_id: None,
                        sort_weight_ms: dp.last_modified_unix * 1000,
                        sort_order_key: id,
                        conversations: Vec::new(),
                        workspaces: Vec::new(),
                        unclassified_conversations: Vec::new(),
                    },
                ));
                added += 1;
            }
        }

        // Apply conversation filter + search query + Curated Mission Workspace & Unfiled grouping
        for (group_idx, mut group) in host_groups.into_iter().enumerate() {
            group
                .conversations
                .retain(|c| matches_conversation_filter(c, params.filter));
            group.conversations.sort_by(|a, b| {
                if params.sort == SidebarProjectSort::Alphabetical {
                    a.agent
                        .title
                        .to_lowercase()
                        .cmp(&b.agent.title.to_lowercase())
                        .then_with(|| b.agent.latest_seq.cmp(&a.agent.latest_seq))
                } else {
                    let ta = parse_rfc3339_ms(&a.agent.updated_at_rfc3339);
                    let tb = parse_rfc3339_ms(&b.agent.updated_at_rfc3339);
                    tb.cmp(&ta)
                        .then_with(|| b.agent.latest_seq.cmp(&a.agent.latest_seq))
                }
            });

            let project_curated_ws = curated_ws_by_group_idx
                .remove(&group_idx)
                .unwrap_or_default();

            if !parsed_query.is_empty() {
                if !parsed_query.matches_host(host_id, host_name) {
                    continue;
                }
                let proj_match = matches_project_query(
                    &parsed_query,
                    host_id,
                    host_name,
                    &group,
                    &project_curated_ws,
                );
                let thread_matches: Vec<SidebarConversationView> = group
                    .conversations
                    .iter()
                    .filter(|c| {
                        matches_agent_query_full(
                            &parsed_query,
                            &SidebarAgentSearchTarget {
                                host_id,
                                host_name,
                                agent: &c.agent,
                                ws_meta: ws_by_id.get(c.agent.workspace_id.as_str()).copied(),
                                project_name: Some(&group.name),
                                project_cwd: Some(&group.primary_cwd),
                                project_branch: group.git_branch.as_deref(),
                            },
                        )
                    })
                    .cloned()
                    .collect();
                if !thread_matches.is_empty() {
                    group.conversations = thread_matches;
                } else if !proj_match {
                    continue;
                } else {
                    group.conversations = Vec::new();
                }
            }

            if params.filter == SidebarThreadFilter::ActiveUnread && group.conversations.is_empty()
            {
                continue;
            }

            if let Some(max_thread_ts) = group
                .conversations
                .iter()
                .map(|c| parse_rfc3339_ms(&c.agent.updated_at_rfc3339))
                .max()
                && max_thread_ts > group.sort_weight_ms
            {
                group.sort_weight_ms = max_thread_ts;
            }

            // Partition conversations into Curated Mission Workspaces vs Unfiled inbox
            let mut curated_ws_buckets: HashMap<String, Vec<SidebarConversationView>> =
                HashMap::new();
            let mut curated_ws_seen: HashSet<String> = HashSet::new();
            let mut curated_ws_list: Vec<(i64, SidebarWorkspaceGroupView)> = Vec::new();
            let mut unclassified: Vec<SidebarConversationView> = Vec::new();

            for conv in &group.conversations {
                let ws_id = conv.agent.workspace_id.as_str();
                if ws_by_id
                    .get(ws_id)
                    .is_some_and(|w| w.is_curated && w.archived_at.is_none())
                {
                    curated_ws_buckets
                        .entry(ws_id.to_string())
                        .or_default()
                        .push(conv.clone());
                } else {
                    unclassified.push(conv.clone());
                }
            }
            group.unclassified_conversations = unclassified;

            // Include ALL curated workspaces belonging to this project (0, 1, or N conversations!)
            let mut all_curated_ids: Vec<String> = Vec::new();
            for ws in &project_curated_ws {
                if curated_ws_seen.insert(ws.workspace_id.clone()) {
                    all_curated_ids.push(ws.workspace_id.clone());
                }
            }
            for ws_id in curated_ws_buckets.keys() {
                if curated_ws_seen.insert(ws_id.clone()) {
                    all_curated_ids.push(ws_id.clone());
                }
            }

            for ws_id in all_curated_ids {
                let convs = curated_ws_buckets.remove(&ws_id).unwrap_or_default();
                let ws_meta = ws_by_id.get(ws_id.as_str()).copied();
                if params.filter == SidebarThreadFilter::ActiveUnread && convs.is_empty() {
                    continue;
                }
                if !parsed_query.is_empty() && convs.is_empty() {
                    let single_ws = ws_meta.into_iter().collect::<Vec<_>>();
                    if !matches_project_query(&parsed_query, host_id, host_name, &group, &single_ws)
                    {
                        continue;
                    }
                }
                let peer_count = convs.len() as u32;
                let running_count = convs
                    .iter()
                    .filter(|c| {
                        c.agent.is_running
                            || c.agent.has_running_subagents
                            || c.agent.is_awaiting_permission
                    })
                    .count() as u32;
                let unread_count = convs.iter().filter(|c| c.is_unread).count() as u32;
                let title = ws_meta
                    .map(|w| w.title.clone())
                    .filter(|t| !t.trim().is_empty())
                    .or_else(|| convs.first().map(|c| c.agent.title.clone()))
                    .unwrap_or_else(|| "Workspace".into());
                let git_branch = ws_meta
                    .and_then(|w| w.git_branch.clone())
                    .or_else(|| convs.first().and_then(|c| c.agent.worktree_branch.clone()));
                let is_worktree = ws_meta.is_some_and(|w| w.is_worktree)
                    || (!convs.is_empty() && convs.iter().all(|c| c.is_worktree));
                let has_conflict = host.workspace_conflict_ids.contains(&ws_id);
                let sort_ts = convs
                    .iter()
                    .map(|c| parse_rfc3339_ms(&c.agent.updated_at_rfc3339))
                    .max()
                    .unwrap_or(0)
                    .max(
                        ws_meta
                            .map(|w| w.updated_at.timestamp_millis())
                            .unwrap_or(0),
                    );
                curated_ws_list.push((
                    sort_ts,
                    SidebarWorkspaceGroupView {
                        workspace_id: ws_id,
                        title,
                        mission_goal: ws_meta.and_then(|w| w.mission_goal.clone()),
                        cwd: ws_meta
                            .map(|w| w.cwd.clone())
                            .or_else(|| convs.first().map(|c| c.agent.cwd.clone()))
                            .unwrap_or_else(|| group.primary_cwd.clone()),
                        git_branch,
                        is_worktree,
                        board_revision: ws_meta.map(|w| w.board_revision).unwrap_or(0),
                        has_conflict,
                        peer_count,
                        running_count,
                        unread_count,
                        is_multi_agent: peer_count > 1,
                        is_curated: true,
                        conversations: convs,
                    },
                ));
            }
            curated_ws_list.sort_by(|(ta, wa), (tb, wb)| {
                if params.sort == SidebarProjectSort::Alphabetical {
                    wa.title
                        .to_lowercase()
                        .cmp(&wb.title.to_lowercase())
                        .then_with(|| tb.cmp(ta))
                } else {
                    tb.cmp(ta)
                        .then_with(|| wa.title.to_lowercase().cmp(&wb.title.to_lowercase()))
                }
            });
            group.workspaces = curated_ws_list.into_iter().map(|(_, w)| w).collect();

            all_groups.push(group);
        }

        // 4. Launchpad candidate projects for the selected host (or single host).
        //    Projects are strictly directory/codebase roots — NEVER curated Mission Workspace titles.
        let include_launchpad =
            params.active_host_id.is_none() || params.active_host_id == Some(host_id);
        if include_launchpad {
            let mut host_lp: Vec<LaunchpadProjectView> = Vec::new();
            let push_lp = |host_lp: &mut Vec<LaunchpadProjectView>,
                           id: String,
                           name: &str,
                           raw_path: &str,
                           branch: Option<String>| {
                let clean = normalize_project_path(raw_path);
                if clean.is_empty() {
                    return;
                }
                if host_lp
                    .iter()
                    .any(|p| cwd_paths_equivalent(&p.path, &clean))
                {
                    return;
                }
                let resolved_name = if name.trim().is_empty() {
                    basename_of_project_path(&clean)
                } else {
                    name.trim().to_string()
                };
                let resolved_branch = branch.filter(|b| !b.trim().is_empty()).or_else(|| {
                    resolve_branch_for_path(
                        host.workspaces,
                        host.discovered_projects,
                        params.active_cwd,
                        host.active_git_branch.as_deref(),
                        &clean,
                    )
                });
                let (is_git_repo, nested_git_repos) = resolve_git_capability_for_path(
                    host.git_repos_by_cwd,
                    host.discovered_projects,
                    host.workspaces,
                    &clean,
                );
                host_lp.push(LaunchpadProjectView {
                    id,
                    host_id: host_id.to_string(),
                    name: resolved_name,
                    path: clean,
                    branch: resolved_branch,
                    is_git_repo,
                    nested_git_repos,
                });
            };

            let project_name_for_path = |clean_path: &str| -> String {
                if let Some(dp) = host
                    .discovered_projects
                    .iter()
                    .find(|p| cwd_paths_equivalent(&p.path, clean_path))
                    && !dp.name.trim().is_empty()
                {
                    return dp.name.trim().to_string();
                }
                basename_of_project_path(clean_path)
            };

            // 4a. Pinned projects always come first with their explicit user-facing project name.
            for proj in &host_pinned {
                if let Some(primary) = proj.directories.first() {
                    push_lp(
                        &mut host_lp,
                        proj.project_id.clone(),
                        &proj.name,
                        primary,
                        None,
                    );
                }
            }

            // 4b. Active workspace project roots (derived strictly from directory path, NEVER `ws.title`).
            //     Skip archived, background, bare-home, and raw worktree paths.
            for ws in host.workspaces {
                if ws.archived_at.is_some() || ws.background {
                    continue;
                }
                let raw_root = ws
                    .project_root
                    .as_deref()
                    .or(ws.source_cwd.as_deref())
                    .unwrap_or(&ws.cwd);
                let clean_root = normalize_project_path(raw_root);
                if (ws.is_worktree && ws.project_root.is_none() && ws.source_cwd.is_none())
                    || is_bare_home_dir(&clean_root)
                    || clean_root.contains("/worktrees/")
                {
                    continue;
                }
                let proj_name = project_name_for_path(&clean_root);
                let branch = if cwd_paths_equivalent(&ws.cwd, &clean_root) && !ws.is_worktree {
                    ws.git_branch.clone()
                } else {
                    None
                };
                push_lp(
                    &mut host_lp,
                    format!("{host_id}::{clean_root}"),
                    &proj_name,
                    &clean_root,
                    branch,
                );
            }

            // 4c. Discovered directories on this host (excluding bare home & worktrees).
            for dp in host.discovered_projects {
                if !is_bare_home_dir(&dp.path) && !dp.path.contains("/worktrees/") {
                    let clean_dp = normalize_project_path(&dp.path);
                    push_lp(
                        &mut host_lp,
                        format!("{host_id}::{clean_dp}"),
                        &dp.name,
                        &clean_dp,
                        dp.git_branch.clone(),
                    );
                }
            }

            // 4d. Currently selected active directory if not already represented.
            if let Some(a_cwd) = params.active_cwd
                && !a_cwd.trim().is_empty()
                && !is_bare_home_dir(a_cwd)
                && !a_cwd.contains("/worktrees/")
            {
                let clean_active = normalize_project_path(a_cwd);
                let proj_name = project_name_for_path(&clean_active);
                push_lp(
                    &mut host_lp,
                    format!("{host_id}::{clean_active}"),
                    &proj_name,
                    &clean_active,
                    host.active_git_branch.clone(),
                );
            }

            if host_lp.is_empty()
                && let Some(first_dp) = host.discovered_projects.first()
            {
                let clean_dp = normalize_project_path(&first_dp.path);
                push_lp(
                    &mut host_lp,
                    format!("{host_id}::{clean_dp}"),
                    &first_dp.name,
                    &clean_dp,
                    first_dp.git_branch.clone(),
                );
            }
            launchpad_projects.extend(host_lp);
        }
    }

    if !has_any_real_project && params.filter == SidebarThreadFilter::All {
        for (fb_host_name, group) in fallback_discovered_groups {
            if !parsed_query.is_empty()
                && !matches_project_query(&parsed_query, &group.host_id, &fb_host_name, &group, &[])
            {
                continue;
            }
            all_groups.push(group);
        }
    }

    // Projects ALWAYS follow stable manual order (representing project priority).
    // Conversation / Workspace activity NEVER reorders Projects automatically.
    let order_index: HashMap<&str, usize> = params
        .project_order
        .iter()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i))
        .collect();
    let lookup_order = |g: &SidebarProjectGroupView| -> Option<usize> {
        order_index
            .get(g.sort_order_key.as_str())
            .or_else(|| order_index.get(g.id.as_str()))
            .or_else(|| {
                let fallback_key = format!("{}::{}", g.host_id, g.primary_cwd);
                order_index.get(fallback_key.as_str())
            })
            .copied()
    };
    all_groups.sort_by(|a, b| {
        let oa = lookup_order(a);
        let ob = lookup_order(b);
        match (oa, ob) {
            (Some(ia), Some(ib)) => ia.cmp(&ib),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a
                .is_home_workspace
                .cmp(&b.is_home_workspace)
                .then_with(|| {
                    a.pinned_project_id
                        .is_none()
                        .cmp(&b.pinned_project_id.is_none())
                })
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.primary_cwd.cmp(&b.primary_cwd)),
        }
    });

    needs_attention.sort_by(|a, b| {
        let ta = parse_rfc3339_ms(&a.agent.updated_at_rfc3339);
        let tb = parse_rfc3339_ms(&b.agent.updated_at_rfc3339);
        tb.cmp(&ta)
    });

    pinned_with_order.sort_by(|(ord_a, ca), (ord_b, cb)| {
        ord_b.cmp(ord_a).then_with(|| {
            let ta = parse_rfc3339_ms(&ca.agent.updated_at_rfc3339);
            let tb = parse_rfc3339_ms(&cb.agent.updated_at_rfc3339);
            tb.cmp(&ta)
        })
    });
    let pinned_conversations = pinned_with_order.into_iter().map(|(_, c)| c).collect();

    FleetSidebarView {
        revision: params.revision,
        projects: all_groups,
        launchpad_projects,
        needs_attention,
        pinned_conversations,
    }
}

#[cfg(test)]
mod tests {
    use orbit_protocol::{AgentStatus, ProviderKind, SessionMode};

    use super::*;

    fn sample_agent(
        agent_id: &str,
        cwd: &str,
        updated_at_rfc3339: &str,
        is_pinned: bool,
        is_unread: bool,
        is_running: bool,
        is_awaiting_permission: bool,
    ) -> AgentCardView {
        AgentCardView {
            agent_id: agent_id.to_string(),
            workspace_id: "ws_1".to_string(),
            cwd: cwd.to_string(),
            title: format!("Agent {agent_id}"),
            provider: ProviderKind::ClaudeCode,
            provider_label: "Claude Code".to_string(),
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            status_label: if is_running {
                "Working".to_string()
            } else if is_awaiting_permission {
                "Needs approval".to_string()
            } else {
                "Idle".to_string()
            },
            is_running,
            has_background_tasks: false,
            has_armed_timers: false,
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
            created_at_rfc3339: None,
            updated_at_rfc3339: updated_at_rfc3339.to_string(),
            is_pinned,
            seen_seq: if is_unread { 2 } else { 5 },
            read_seq: if is_unread { 2 } else { 5 },
            is_unread,
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
    fn groups_worktrees_filters_bare_home_and_dedups_launchpad() {
        let pinned = vec![ClientProjectView {
            project_id: "proj_orbit".into(),
            host_id: "h1".into(),
            name: "Orbit".into(),
            directories: vec!["/Users/jess/Code/orbit/".into()],
            updated_at_ms: 1_000,
        }];
        let workspaces = vec![
            WorkspaceSummary {
                workspace_id: "ws_home".into(),
                title: "Home".into(),
                mission_goal: None,
                cwd: "/Users/jess".into(),
                project_root: None,
                git_branch: None,
                is_worktree: false,
                updated_at: chrono::Utc::now(),
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            },
            WorkspaceSummary {
                workspace_id: "ws_wt".into(),
                title: "Orbit Worktree".into(),
                mission_goal: None,
                cwd: "/Users/jess/.orbit/worktrees/orbit-wt-1".into(),
                project_root: None,
                git_branch: Some("feat/ui".into()),
                is_worktree: true,
                updated_at: chrono::Utc::now(),
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            },
        ];
        let discovered = vec![
            DiscoveredProject {
                path: "/Users/jess/Code/orbit".into(),
                name: "orbit".into(),
                git_branch: Some("main".into()),
                is_git_repo: true,
                nested_git_repos: Vec::new(),
                last_modified_unix: 100,
            },
            DiscoveredProject {
                path: "/Users/jess".into(),
                name: "jess".into(),
                git_branch: None,
                is_git_repo: false,
                nested_git_repos: Vec::new(),
                last_modified_unix: 90,
            },
        ];
        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();

        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 1,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &discovered,
                agents: vec![
                    (
                        sample_agent(
                            "a1",
                            "/Users/jess/.orbit/worktrees/orbit-wt-1",
                            "2026-11-20T10:00:00Z",
                            true,
                            false,
                            false,
                            true,
                        ),
                        500,
                    ),
                    (
                        sample_agent(
                            "a2",
                            "/Users/jess/Code/other",
                            "2026-11-20T09:00:00Z",
                            false,
                            true,
                            false,
                            false,
                        ),
                        0,
                    ),
                ],
                active_git_branch: Some("main".into()),
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &pinned,
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: Some("/Users/jess/Code/orbit"),
        });

        assert_eq!(view.projects.len(), 2);
        assert_eq!(view.projects[0].id, "proj_orbit");
        assert_eq!(view.projects[0].conversations.len(), 1);
        assert!(view.projects[0].conversations[0].is_worktree);
        assert_eq!(view.needs_attention.len(), 1);
        assert_eq!(view.pinned_conversations.len(), 1);
        // Bare home filtered from launchpad, Orbit deduplicated
        assert_eq!(view.launchpad_projects.len(), 1);
        assert_eq!(view.launchpad_projects[0].path, "/Users/jess/Code/orbit");

        // ActiveUnread filter retains both needs-approval and unread threads
        let filtered = build_fleet_sidebar(FleetSidebarParams {
            revision: 2,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &discovered,
                agents: vec![
                    (
                        sample_agent(
                            "a1",
                            "/Users/jess/.orbit/worktrees/orbit-wt-1",
                            "2026-11-20T10:00:00Z",
                            true,
                            false,
                            false,
                            false,
                        ),
                        500,
                    ),
                    (
                        sample_agent(
                            "a2",
                            "/Users/jess/Code/other",
                            "2026-11-20T09:00:00Z",
                            false,
                            true,
                            false,
                            false,
                        ),
                        0,
                    ),
                ],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &pinned,
            project_order: &[],
            sort: SidebarProjectSort::Alphabetical,
            filter: SidebarThreadFilter::ActiveUnread,
            query: "other",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: None,
        });
        assert_eq!(filtered.projects.len(), 1);
        assert_eq!(filtered.projects[0].conversations[0].agent.agent_id, "a2");
    }

    #[test]
    fn hides_background_archived_and_same_workspace_subagents_and_promotes_attention_unread() {
        let workspaces = vec![WorkspaceSummary {
            workspace_id: "ws_1".into(),
            title: "Orbit".into(),
            mission_goal: None,
            cwd: "/Users/jess/Code/orbit".into(),
            project_root: None,
            git_branch: Some("main".into()),
            is_worktree: false,
            updated_at: chrono::Utc::now(),
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }];
        let mut parent = sample_agent(
            "parent",
            "/Users/jess/Code/orbit",
            "2026-11-20T10:00:00Z",
            false,
            false,
            false,
            false,
        );
        parent.attention = AttentionState::Finished;

        let mut bg = sample_agent(
            "bg",
            "/Users/jess/Code/orbit",
            "2026-11-20T10:01:00Z",
            true,
            true,
            false,
            true,
        );
        bg.background = true;

        let mut archived = sample_agent(
            "arch",
            "/Users/jess/Code/orbit",
            "2026-11-20T10:02:00Z",
            true,
            true,
            false,
            true,
        );
        archived.archived_at_rfc3339 = Some("2026-11-20T10:02:00Z".into());

        let mut sub_same_ws = sample_agent(
            "sub_same",
            "/Users/jess/Code/orbit",
            "2026-11-20T10:03:00Z",
            true,
            true,
            false,
            true,
        );
        sub_same_ws.parent_agent_id = Some("parent".into());

        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 3,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &[],
                agents: vec![(parent, 0), (bg, 10), (archived, 20), (sub_same_ws, 30)],
                active_git_branch: Some("main".into()),
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: Some("/Users/jess/Code/orbit"),
        });

        // Only `parent` appears in the project list; subagent never leaks into `needs_attention`
        // and instead bubbles its pending approval onto `parent`.
        assert_eq!(view.projects.len(), 1);
        assert_eq!(view.projects[0].conversations.len(), 1);
        assert_eq!(view.projects[0].conversations[0].agent.agent_id, "parent");
        assert!(view.projects[0].conversations[0].is_unread);
        assert!(
            view.projects[0].conversations[0]
                .agent
                .is_awaiting_permission
        );
        assert_eq!(view.pinned_conversations.len(), 0);
        assert_eq!(view.needs_attention.len(), 1);
        assert_eq!(view.needs_attention[0].agent.agent_id, "parent");
    }

    #[test]
    fn manual_sort_nested_worktree_folding_and_multi_agent_mission_workspace_promotion() {
        let pinned = vec![
            ClientProjectView {
                project_id: "proj_alpha".into(),
                host_id: "h1".into(),
                name: "Alpha".into(),
                directories: vec!["/work/alpha".into()],
                updated_at_ms: 5_000,
            },
            ClientProjectView {
                project_id: "proj_research".into(),
                host_id: "h1".into(),
                name: "Research".into(),
                directories: vec!["/work/research".into()],
                updated_at_ms: 1_000,
            },
        ];
        let workspaces = vec![WorkspaceSummary {
            workspace_id: "ws_mission".into(),
            title: "Auth Migration".into(),
            mission_goal: Some("Migrate JWT to OAuth2".into()),
            cwd: "/work/research".into(),
            project_root: Some("/work/research".into()),
            git_branch: Some("feat/oauth".into()),
            is_worktree: false,
            updated_at: chrono::Utc::now(),
            source_cwd: Some("/work/research/repos/paseo".into()),
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            board_revision: 2,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }];
        let mut lead = sample_agent(
            "ag_lead",
            "/work/research",
            "2026-11-20T10:00:00Z",
            false,
            false,
            true,
            false,
        );
        lead.workspace_id = "ws_mission".into();
        lead.handle = Some("lead".into());

        let mut worker = sample_agent(
            "ag_worker",
            "/home/u/.orbit/worktrees/paseo-oauth-1",
            "2026-11-20T10:05:00Z",
            false,
            true,
            false,
            false,
        );
        worker.workspace_id = "ws_mission".into();
        worker.handle = Some("codex".into());
        worker.worktree_branch = Some("feat/oauth".into());
        worker.labels.insert(
            "orbit.worktree_source_cwd".into(),
            "/work/research/repos/paseo".into(),
        );

        let empty_git_repos = HashMap::new();
        let conflicts: HashSet<String> = ["ws_mission".to_string()].into_iter().collect();
        let manual_order = vec!["proj_research".to_string(), "proj_alpha".to_string()];

        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 4,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &[],
                agents: vec![(lead, 0), (worker, 0)],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &conflicts,
            }],
            pinned_projects: &pinned,
            project_order: &manual_order,
            sort: SidebarProjectSort::Manual,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: None,
        });

        // Manual order puts Research before Alpha despite Alpha's higher updated_at
        assert_eq!(view.projects.len(), 2);
        assert_eq!(view.projects[0].id, "proj_research");
        assert_eq!(view.projects[1].id, "proj_alpha");

        // Both lead AND nested-repo worktree worker folded into Research under 1 multi-agent Mission Workspace
        assert_eq!(view.projects[0].conversations.len(), 2);
        assert_eq!(view.projects[0].workspaces.len(), 1);
        let ws = &view.projects[0].workspaces[0];
        assert_eq!(ws.workspace_id, "ws_mission");
        assert!(ws.is_multi_agent);
        assert_eq!(ws.peer_count, 2);
        assert_eq!(ws.running_count, 1);
        assert_eq!(ws.unread_count, 1);
        assert!(ws.has_conflict);
        assert_eq!(ws.board_revision, 2);
        assert!(ws.is_curated);
        assert!(view.projects[0].unclassified_conversations.is_empty());
    }

    #[test]
    fn surfaces_empty_curated_workspaces_and_separates_unclassified_inbox() {
        let now = chrono::Utc::now();
        let pinned = vec![ClientProjectView {
            project_id: "proj_main".into(),
            host_id: "h1".into(),
            name: "Orbit".into(),
            directories: vec!["/work/orbit".into()],
            updated_at_ms: 1_000,
        }];
        let workspaces = vec![
            WorkspaceSummary {
                workspace_id: "ws_empty_curated".into(),
                title: "Release v0.7".into(),
                mission_goal: Some("Ship Workspace Management".into()),
                cwd: "/work/orbit".into(),
                project_root: Some("/work/orbit".into()),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: true,
                auto_clustered: false,
                is_curated: true,
            },
            WorkspaceSummary {
                workspace_id: "ws_unclassified".into(),
                title: "Quick question".into(),
                mission_goal: None,
                cwd: "/work/orbit".into(),
                project_root: Some("/work/orbit".into()),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            },
        ];
        let mut inbox_agent = sample_agent(
            "ag_inbox",
            "/work/orbit",
            "2026-11-20T12:00:00Z",
            false,
            false,
            false,
            false,
        );
        inbox_agent.workspace_id = "ws_unclassified".into();

        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 5,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &[],
                agents: vec![(inbox_agent, 0)],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &pinned,
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: None,
        });

        assert_eq!(view.projects.len(), 1);
        let proj = &view.projects[0];
        // Empty curated workspace MUST surface in project.workspaces with peer_count == 0
        assert_eq!(proj.workspaces.len(), 1);
        assert_eq!(proj.workspaces[0].workspace_id, "ws_empty_curated");
        assert!(proj.workspaces[0].is_curated);
        assert_eq!(proj.workspaces[0].peer_count, 0);
        // Unclassified conversation MUST surface in unclassified_conversations, NOT in workspaces
        assert_eq!(proj.unclassified_conversations.len(), 1);
        assert_eq!(
            proj.unclassified_conversations[0].agent.agent_id,
            "ag_inbox"
        );
    }

    #[test]
    fn launchpad_projects_never_leak_curated_workspace_titles() {
        let now = chrono::Utc::now();
        let workspaces = vec![
            WorkspaceSummary {
                workspace_id: "ws_curated_1".into(),
                title: "Orbit 客户端缺陷修复与版本发布".into(),
                mission_goal: Some("修复缺陷并发布版本".into()),
                cwd: "/home/gzc/workspace/research".into(),
                project_root: Some("/home/gzc/workspace/research".into()),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: true,
                auto_clustered: true,
                is_curated: true,
            },
            WorkspaceSummary {
                workspace_id: "ws_curated_2".into(),
                title: "Orbit 工作区体验与 v0.7.0 升级".into(),
                mission_goal: None,
                cwd: "/home/gzc/workspace/research".into(),
                project_root: Some("/home/gzc/workspace/research".into()),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: true,
                auto_clustered: true,
                is_curated: true,
            },
            WorkspaceSummary {
                workspace_id: "ws_archived".into(),
                title: "Old Archived Mission".into(),
                mission_goal: None,
                cwd: "/home/gzc/workspace/stale_dir".into(),
                project_root: None,
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: Some(now),
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: true,
            },
        ];
        let discovered = vec![
            DiscoveredProject {
                path: "/home/gzc/workspace/research".into(),
                name: "research".into(),
                git_branch: None,
                is_git_repo: false,
                nested_git_repos: Vec::new(),
                last_modified_unix: 200,
            },
            DiscoveredProject {
                path: "/home/gzc/workspace/orbit".into(),
                name: "orbit".into(),
                git_branch: Some("main".into()),
                is_git_repo: true,
                nested_git_repos: Vec::new(),
                last_modified_unix: 100,
            },
        ];
        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();

        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 6,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &discovered,
                agents: Vec::new(),
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: Some("/home/gzc/workspace/research"),
        });

        assert_eq!(view.launchpad_projects.len(), 2);
        let research = view
            .launchpad_projects
            .iter()
            .find(|p| p.path == "/home/gzc/workspace/research")
            .expect("research project present");
        assert_eq!(research.name, "research");
        assert_eq!(research.id, "h1::/home/gzc/workspace/research");
        assert!(
            view.launchpad_projects
                .iter()
                .all(|p| !p.name.contains("Orbit 客户端") && !p.name.contains("工作区体验")),
            "Curated workspace titles must never appear in launchpad_projects: {:?}",
            view.launchpad_projects
        );
        assert!(
            view.launchpad_projects
                .iter()
                .all(|p| p.path != "/home/gzc/workspace/stale_dir"),
            "Archived workspace directories must not pollute launchpad_projects"
        );
    }

    #[test]
    fn project_order_stays_stable_when_conversations_and_workspaces_update() {
        let now = chrono::Utc::now();
        let pinned = vec![
            ClientProjectView {
                project_id: "proj_primary".into(),
                host_id: "h1".into(),
                name: "Primary".into(),
                directories: vec!["/work/primary".into()],
                updated_at_ms: 1_000,
            },
            ClientProjectView {
                project_id: "proj_secondary".into(),
                host_id: "h1".into(),
                name: "Secondary".into(),
                directories: vec!["/work/secondary".into()],
                updated_at_ms: 500,
            },
        ];
        let manual_order = vec!["proj_primary".to_string(), "proj_secondary".to_string()];

        // Secondary has much newer conversation & workspace timestamps than Primary,
        // yet even under `SidebarProjectSort::Recent`, Projects MUST keep their manual order
        // while conversations inside each project sort by recency.
        let workspaces = vec![
            WorkspaceSummary {
                workspace_id: "ws_sec_old".into(),
                title: "Older WS".into(),
                mission_goal: None,
                cwd: "/work/secondary".into(),
                project_root: Some("/work/secondary".into()),
                git_branch: None,
                is_worktree: false,
                updated_at: now - chrono::Duration::hours(2),
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: true,
            },
            WorkspaceSummary {
                workspace_id: "ws_sec_new".into(),
                title: "Newer WS".into(),
                mission_goal: None,
                cwd: "/work/secondary".into(),
                project_root: Some("/work/secondary".into()),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: true,
            },
        ];
        let mut ag_primary = sample_agent(
            "ag_p",
            "/work/primary",
            "2026-11-19T08:00:00Z",
            false,
            false,
            false,
            false,
        );
        ag_primary.workspace_id = "ws_p".into();
        let mut ag_sec_new = sample_agent(
            "ag_s_new",
            "/work/secondary",
            "2026-11-21T12:00:00Z",
            false,
            false,
            false,
            false,
        );
        ag_sec_new.workspace_id = "ws_sec_new".into();
        let mut ag_sec_old = sample_agent(
            "ag_s_old",
            "/work/secondary",
            "2026-11-20T09:00:00Z",
            false,
            false,
            false,
            false,
        );
        ag_sec_old.workspace_id = "ws_sec_old".into();

        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 7,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "MacBook Pro",
                workspaces: &workspaces,
                discovered_projects: &[],
                agents: vec![(ag_primary, 0), (ag_sec_old, 0), (ag_sec_new, 0)],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &pinned,
            project_order: &manual_order,
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h1"),
            active_agent_id: None,
            active_cwd: None,
        });

        assert_eq!(view.projects.len(), 2);
        assert_eq!(view.projects[0].id, "proj_primary");
        assert_eq!(view.projects[1].id, "proj_secondary");
        // Inside Secondary, workspaces & conversations still sort by recency
        assert_eq!(view.projects[1].workspaces[0].workspace_id, "ws_sec_new");
        assert_eq!(view.projects[1].workspaces[1].workspace_id, "ws_sec_old");
    }

    #[test]
    fn multi_host_unused_host_does_not_flood_sidebar_with_discovered_projects() {
        let pinned = vec![ClientProjectView {
            project_id: "h_devbox::/home/ec2-user/go/src/mono_repo".into(),
            host_id: "h_devbox".into(),
            name: "mono_repo".into(),
            directories: vec!["/home/ec2-user/go/src/mono_repo".into()],
            updated_at_ms: 1_000,
        }];
        let local_discovered = vec![
            DiscoveredProject {
                path: "/Users/dev/code/mono_repo".into(),
                name: "mono_repo".into(),
                git_branch: Some("main".into()),
                is_git_repo: true,
                nested_git_repos: Vec::new(),
                last_modified_unix: 400,
            },
            DiscoveredProject {
                path: "/Users/dev/code/deepseek-harness".into(),
                name: "deepseek-harness".into(),
                git_branch: Some("main".into()),
                is_git_repo: true,
                nested_git_repos: Vec::new(),
                last_modified_unix: 300,
            },
        ];
        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();

        // When another host already has real projects, an unused host's auto-discovered repos
        // must NOT be injected into the main sidebar Projects list (they stay available in Launchpad).
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 8,
            hosts: vec![
                SidebarHostInput {
                    host_id: "h_devbox",
                    host_name: "Linux DevBox",
                    workspaces: &[],
                    discovered_projects: &[],
                    agents: Vec::new(),
                    active_git_branch: None,
                    git_repos_by_cwd: &empty_git_repos,
                    workspace_conflict_ids: &empty_conflicts,
                },
                SidebarHostInput {
                    host_id: "h_local",
                    host_name: "Local Mac",
                    workspaces: &[],
                    discovered_projects: &local_discovered,
                    agents: Vec::new(),
                    active_git_branch: None,
                    git_repos_by_cwd: &empty_git_repos,
                    workspace_conflict_ids: &empty_conflicts,
                },
            ],
            pinned_projects: &pinned,
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h_local"),
            active_agent_id: None,
            active_cwd: None,
        });

        assert_eq!(view.projects.len(), 1);
        assert_eq!(view.projects[0].host_id, "h_devbox");
        assert_eq!(view.projects[0].name, "mono_repo");
        // Launchpad for the selected host still surfaces its discovered directories.
        assert_eq!(view.launchpad_projects.len(), 2);

        // When the entire fleet has zero real projects, onboarding fallback still surfaces up to 4.
        let onboarding = build_fleet_sidebar(FleetSidebarParams {
            revision: 9,
            hosts: vec![SidebarHostInput {
                host_id: "h_local",
                host_name: "Local Mac",
                workspaces: &[],
                discovered_projects: &local_discovered,
                agents: Vec::new(),
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h_local"),
            active_agent_id: None,
            active_cwd: None,
        });
        assert_eq!(onboarding.projects.len(), 2);
    }

    #[test]
    fn personal_agent_conversation_is_hidden_from_pinned_and_projects() {
        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        // The Personal Agent usually runs in the bare home directory; hiding it must not leave
        // behind an empty "Workspace (~)" project group.
        let mut pa = sample_agent(
            "ag_pa",
            "/Users/dev",
            "2026-01-01T00:00:10Z",
            true,
            false,
            false,
            true,
        );
        pa.labels.insert(
            orbit_protocol::LABEL_AGENT_ROLE.into(),
            orbit_protocol::AGENT_ROLE_PERSONAL_ROUTER.into(),
        );
        let worker = sample_agent(
            "ag_worker",
            "/Users/dev/code/orbit",
            "2026-01-01T00:00:05Z",
            true,
            false,
            false,
            false,
        );

        // Mirrors real data: the Personal Agent's own non-curated workspace rooted at `~`.
        let home_ws = vec![WorkspaceSummary {
            workspace_id: "ws_1".into(),
            title: "Personal Agent".into(),
            mission_goal: None,
            cwd: "/Users/dev".into(),
            project_root: Some("/Users/dev".into()),
            git_branch: None,
            is_worktree: false,
            updated_at: chrono::Utc::now(),
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: false,
        }];

        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 1,
            hosts: vec![SidebarHostInput {
                host_id: "h_local",
                host_name: "Mac Studio",
                workspaces: &home_ws,
                discovered_projects: &[],
                agents: vec![(pa, 1), (worker, 2)],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: Some("h_local"),
            active_agent_id: None,
            active_cwd: None,
        });

        let pinned: Vec<&str> = view
            .pinned_conversations
            .iter()
            .map(|c| c.agent.agent_id.as_str())
            .collect();
        assert_eq!(pinned, vec!["ag_worker"]);
        let in_projects = view
            .projects
            .iter()
            .flat_map(|p| {
                p.conversations
                    .iter()
                    .chain(p.unclassified_conversations.iter())
            })
            .any(|c| c.agent.agent_id == "ag_pa");
        assert!(
            !in_projects,
            "Personal Agent must not appear under Projects"
        );
        assert!(
            view.projects.iter().all(|p| !p.is_home_workspace),
            "hiding the Personal Agent must not leave an empty Workspace (~) group"
        );
        // A Personal Agent awaiting approval still surfaces in Needs Attention.
        assert!(
            view.needs_attention
                .iter()
                .any(|c| c.agent.agent_id == "ag_pa")
        );
    }

    #[test]
    fn sidebar_search_matches_plain_fields_and_inline_host_provider_tokens() {
        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();

        let ws_linux = vec![WorkspaceSummary {
            workspace_id: "ws_auth".into(),
            title: "Auth Rewrite".into(),
            mission_goal: Some("Harden OAuth tokens".into()),
            cwd: "/srv/api".into(),
            project_root: None,
            git_branch: Some("feat/oauth-v2".into()),
            is_worktree: false,
            updated_at: chrono::Utc::now(),
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }];

        let mut linux_codex = sample_agent(
            "ag_linux_codex",
            "/srv/api",
            "2026-11-24T10:00:00Z",
            true,
            false,
            true,
            true,
        );
        linux_codex.workspace_id = "ws_auth".into();
        linux_codex.title = "Fix auth middleware".into();
        linux_codex.provider = ProviderKind::Codex;
        linux_codex.provider_label = "Codex".into();
        linux_codex.worktree_branch = Some("fix/auth-token".into());

        let mut linux_claude = sample_agent(
            "ag_linux_claude",
            "/srv/api",
            "2026-11-24T09:00:00Z",
            false,
            false,
            false,
            false,
        );
        linux_claude.workspace_id = "ws_auth".into();
        linux_claude.title = "Refactor billing".into();
        linux_claude.provider = ProviderKind::ClaudeCode;
        linux_claude.provider_label = "Claude Code".into();

        let mut mac_codex = sample_agent(
            "ag_mac_codex",
            "/Users/jess/ui",
            "2026-11-24T11:00:00Z",
            false,
            false,
            false,
            true,
        );
        mac_codex.title = "Auth login modal".into();
        mac_codex.provider = ProviderKind::Codex;
        mac_codex.provider_label = "Codex".into();

        let run_query = |query: &str| {
            build_fleet_sidebar(FleetSidebarParams {
                revision: 10,
                hosts: vec![
                    SidebarHostInput {
                        host_id: "h_linux_gpu",
                        host_name: "Linux Build Box",
                        workspaces: &ws_linux,
                        discovered_projects: &[],
                        agents: vec![(linux_codex.clone(), 100), (linux_claude.clone(), 0)],
                        active_git_branch: None,
                        git_repos_by_cwd: &empty_git_repos,
                        workspace_conflict_ids: &empty_conflicts,
                    },
                    SidebarHostInput {
                        host_id: "h_mac_studio",
                        host_name: "Mac Studio",
                        workspaces: &[],
                        discovered_projects: &[],
                        agents: vec![(mac_codex.clone(), 0)],
                        active_git_branch: None,
                        git_repos_by_cwd: &empty_git_repos,
                        workspace_conflict_ids: &empty_conflicts,
                    },
                ],
                pinned_projects: &[],
                project_order: &[],
                sort: SidebarProjectSort::Recent,
                filter: SidebarThreadFilter::All,
                query,
                active_host_id: None,
                active_agent_id: None,
                active_cwd: None,
            })
        };

        // 1. Plain match on host name
        let by_host = run_query("Studio");
        assert_eq!(by_host.projects.len(), 1);
        assert_eq!(by_host.projects[0].host_id, "h_mac_studio");

        // 2. Plain match on git branch (worktree_branch)
        let by_branch = run_query("auth-token");
        assert_eq!(by_branch.projects.len(), 1);
        assert_eq!(by_branch.projects[0].conversations.len(), 1);
        assert_eq!(
            by_branch.projects[0].conversations[0].agent.agent_id,
            "ag_linux_codex"
        );

        // 3. Plain match on workspace name
        let by_ws = run_query("Auth Rewrite");
        assert_eq!(by_ws.projects.len(), 1);
        assert_eq!(by_ws.projects[0].conversations.len(), 2);

        // 4. Combined power-user tokens: `@host:linux @provider:codex auth`
        let combo = run_query("@host:linux @provider:codex auth");
        assert_eq!(combo.projects.len(), 1);
        assert_eq!(combo.projects[0].conversations.len(), 1);
        assert_eq!(
            combo.projects[0].conversations[0].agent.agent_id,
            "ag_linux_codex"
        );
        assert_eq!(combo.needs_attention.len(), 1);
        assert_eq!(combo.needs_attention[0].agent.agent_id, "ag_linux_codex");
        assert_eq!(combo.pinned_conversations.len(), 1);

        // 5. Provider token excludes non-matching provider threads even inside matching host/project
        let claude_only = run_query("@provider:claude");
        assert_eq!(claude_only.projects.len(), 1);
        assert_eq!(claude_only.projects[0].conversations.len(), 1);
        assert_eq!(
            claude_only.projects[0].conversations[0].agent.agent_id,
            "ag_linux_claude"
        );
        assert!(claude_only.needs_attention.is_empty());

        // 6. Non-matching host token returns empty fleet
        let none = run_query("@host:windows");
        assert!(none.projects.is_empty());
        assert!(none.needs_attention.is_empty());
        assert!(none.pinned_conversations.is_empty());
    }

    #[test]
    fn archived_curated_workspace_never_resurfaces_with_unarchived_thread() {
        let now = chrono::Utc::now();
        let workspaces = vec![WorkspaceSummary {
            workspace_id: "ws_archived_curated".into(),
            title: "Archived Group".into(),
            mission_goal: None,
            cwd: "/work/app".into(),
            project_root: Some("/work/app".into()),
            git_branch: None,
            is_worktree: false,
            updated_at: now,
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: Some(now),
            background: false,
            setup_status: orbit_protocol::WorkspaceSetupStatus::default(),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }];
        let mut stray_agent = sample_agent(
            "ag_stray",
            "/work/app",
            "2026-11-25T10:00:00Z",
            false,
            false,
            false,
            false,
        );
        stray_agent.workspace_id = "ws_archived_curated".into();

        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 1,
            hosts: vec![SidebarHostInput {
                host_id: "h1",
                host_name: "spark",
                workspaces: &workspaces,
                discovered_projects: &[],
                agents: vec![(stray_agent, 0)],
                active_git_branch: None,
                git_repos_by_cwd: &empty_git_repos,
                workspace_conflict_ids: &empty_conflicts,
            }],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: None,
            active_agent_id: None,
            active_cwd: None,
        });

        assert_eq!(view.projects.len(), 1);
        let proj = &view.projects[0];
        assert!(
            proj.workspaces.is_empty(),
            "archived curated workspace must not appear in project.workspaces"
        );
        assert_eq!(proj.unclassified_conversations.len(), 1);
        assert_eq!(
            proj.unclassified_conversations[0].agent.agent_id,
            "ag_stray"
        );
    }

    #[test]
    fn cross_host_and_orphaned_subagents_stay_hidden_while_routed_conversations_remain_visible() {
        let mut root_conv = sample_agent(
            "conv_root",
            "/work/app",
            "2026-11-26T10:00:00Z",
            false,
            false,
            false,
            false,
        );
        root_conv.labels.insert(
            orbit_protocol::LABEL_SESSION_TIER.into(),
            orbit_protocol::SESSION_TIER_CONVERSATION.into(),
        );

        let mut archived_parent = sample_agent(
            "conv_archived",
            "/work/app",
            "2026-11-26T09:00:00Z",
            false,
            false,
            false,
            false,
        );
        archived_parent.archived_at_rfc3339 = Some("2026-11-26T09:30:00Z".into());

        // Orphaned same-host subagent whose parent was archived (must NOT surface as surprise thread)
        let mut orphan_sub = sample_agent(
            "sub_orphan",
            "/work/app/worktree-1",
            "2026-11-26T10:01:00Z",
            false,
            false,
            true,
            false,
        );
        orphan_sub.parent_agent_id = Some("conv_archived".into());
        orphan_sub.labels.insert(
            orbit_protocol::LABEL_SESSION_TIER.into(),
            orbit_protocol::SESSION_TIER_SUBAGENT.into(),
        );

        // Cross-host subagent dispatched onto `h2` by `conv_root` on `h1`, awaiting permission (even if pinned)
        let mut remote_sub = sample_agent(
            "sub_remote_h2",
            "/remote/gpu-repo",
            "2026-11-26T10:02:00Z",
            true,
            false,
            true,
            true,
        );
        remote_sub.parent_agent_id = Some("conv_root".into());
        remote_sub.parent_host_id = Some("h1".into());
        remote_sub.labels.insert(
            orbit_protocol::LABEL_SESSION_TIER.into(),
            orbit_protocol::SESSION_TIER_SUBAGENT.into(),
        );

        // Conversation dispatched by PersonalRouter (must be visible on both local and remote hosts)
        let mut routed_conv = sample_agent(
            "conv_routed_h2",
            "/remote/gpu-repo",
            "2026-11-26T10:03:00Z",
            false,
            false,
            true,
            false,
        );
        routed_conv.labels.insert(
            orbit_protocol::LABEL_ROUTED_BY_AGENT.into(),
            "ag_router_h1".into(),
        );
        routed_conv.labels.insert(
            orbit_protocol::LABEL_SESSION_TIER.into(),
            orbit_protocol::SESSION_TIER_CONVERSATION.into(),
        );

        let empty_git_repos = HashMap::new();
        let empty_conflicts = HashSet::new();
        let view = build_fleet_sidebar(FleetSidebarParams {
            revision: 7,
            hosts: vec![
                SidebarHostInput {
                    host_id: "h1",
                    host_name: "MacBook",
                    workspaces: &[],
                    discovered_projects: &[],
                    agents: vec![(root_conv, 0), (archived_parent, 0), (orphan_sub, 10)],
                    active_git_branch: None,
                    git_repos_by_cwd: &empty_git_repos,
                    workspace_conflict_ids: &empty_conflicts,
                },
                SidebarHostInput {
                    host_id: "h2",
                    host_name: "Linux GPU",
                    workspaces: &[],
                    discovered_projects: &[],
                    agents: vec![(remote_sub, 20), (routed_conv, 0)],
                    active_git_branch: None,
                    git_repos_by_cwd: &empty_git_repos,
                    workspace_conflict_ids: &empty_conflicts,
                },
            ],
            pinned_projects: &[],
            project_order: &[],
            sort: SidebarProjectSort::Recent,
            filter: SidebarThreadFilter::All,
            query: "",
            active_host_id: None,
            active_agent_id: None,
            active_cwd: None,
        });

        let visible_ids: Vec<&str> = view
            .projects
            .iter()
            .flat_map(|p| p.conversations.iter().map(|c| c.agent.agent_id.as_str()))
            .collect();
        assert_eq!(visible_ids, vec!["conv_root", "conv_routed_h2"]);
        assert!(view.pinned_conversations.is_empty());
        // Cross-host subagent approval bubbles onto `conv_root` on `h1`
        assert_eq!(view.needs_attention.len(), 1);
        assert_eq!(view.needs_attention[0].host_id, "h1");
        assert_eq!(view.needs_attention[0].agent.agent_id, "conv_root");
    }
}
