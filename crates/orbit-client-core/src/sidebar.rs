//! Unified Fleet Sidebar project tree, Launchpad workspace candidates, Attention & Pinned lists
//! shared across Desktop, iOS, Android, and HarmonyOS.

use std::collections::{HashMap, HashSet};

use chrono::DateTime;
use orbit_protocol::{
    AttentionState, DiscoveredProject, GitStatusSnapshot, NestedGitRepo, WorkspaceSummary,
};
use serde::{Deserialize, Serialize};

use crate::app_core::{AgentCardView, ClientProjectView, cwd_paths_equivalent};
use crate::contract::GitReposInspectionView;

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
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return "~".to_string();
    }
    if trimmed == "/" {
        return "/".to_string();
    }
    let clean = trimmed.trim_end_matches('/');
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
                || conv.agent.is_awaiting_permission
                || conv.is_unread
        }
    }
}

fn matches_agent_query(agent: &AgentCardView, q_lower: &str) -> bool {
    if q_lower.is_empty() {
        return true;
    }
    let title = if agent.title.trim().is_empty() {
        agent.provider_label.as_str()
    } else {
        agent.title.as_str()
    };
    title.to_lowercase().contains(q_lower)
        || agent.cwd.to_lowercase().contains(q_lower)
        || basename_of_project_path(&agent.cwd)
            .to_lowercase()
            .contains(q_lower)
        || agent.provider_label.to_lowercase().contains(q_lower)
        || agent
            .handle
            .as_deref()
            .is_some_and(|h| h.to_lowercase().contains(q_lower))
}

pub fn build_fleet_sidebar(params: FleetSidebarParams<'_>) -> FleetSidebarView {
    let q_lower = params.query.trim().to_lowercase();
    let mut all_groups: Vec<SidebarProjectGroupView> = Vec::new();
    let mut launchpad_projects: Vec<LaunchpadProjectView> = Vec::new();
    let mut needs_attention: Vec<SidebarConversationView> = Vec::new();
    let mut pinned_with_order: Vec<(i64, SidebarConversationView)> = Vec::new();

    for host in &params.hosts {
        let host_id = host.host_id;
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
            };
            group_idx_by_id.insert(group.id.clone(), host_groups.len());
            host_groups.push(group);
        }

        // 2. Group active conversations
        let agents_by_id: HashMap<&str, &AgentCardView> = host
            .agents
            .iter()
            .map(|(a, _)| (a.agent_id.as_str(), a))
            .collect();

        for (raw_agent, pinned_order) in &host.agents {
            if raw_agent.background || raw_agent.archived_at_rfc3339.is_some() {
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

            if agent.is_awaiting_permission {
                needs_attention.push(conv.clone());
            }

            // Collapse same-workspace managed subagents under their parent's Subagents Track
            if let Some(ref parent_id) = agent.parent_agent_id
                && agent.parent_host_id.is_none()
                && let Some(parent) = agents_by_id.get(parent_id.as_str())
                && parent.archived_at_rfc3339.is_none()
                && parent.workspace_id == agent.workspace_id
            {
                continue;
            }

            if agent.is_pinned && matches_conversation_filter(&conv, params.filter) {
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

        // 3. Fallback discovered repositories when host has neither pinned projects nor active sessions
        if host_groups.is_empty() {
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
                group_idx_by_id.insert(id.clone(), host_groups.len());
                host_groups.push(SidebarProjectGroupView {
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
                });
                added += 1;
            }
        }

        // Apply conversation filter + search query + Mission Workspace grouping to host groups
        for mut group in host_groups {
            group
                .conversations
                .retain(|c| matches_conversation_filter(c, params.filter));
            group.conversations.sort_by(|a, b| {
                let ta = parse_rfc3339_ms(&a.agent.updated_at_rfc3339);
                let tb = parse_rfc3339_ms(&b.agent.updated_at_rfc3339);
                tb.cmp(&ta)
                    .then_with(|| b.agent.latest_seq.cmp(&a.agent.latest_seq))
            });

            if !q_lower.is_empty() {
                let proj_match = group.name.to_lowercase().contains(&q_lower)
                    || group.primary_cwd.to_lowercase().contains(&q_lower)
                    || group
                        .git_branch
                        .as_deref()
                        .is_some_and(|b| b.to_lowercase().contains(&q_lower));
                let thread_matches: Vec<SidebarConversationView> = group
                    .conversations
                    .iter()
                    .filter(|c| {
                        matches_agent_query(&c.agent, &q_lower)
                            || ws_by_id
                                .get(c.agent.workspace_id.as_str())
                                .is_some_and(|w| {
                                    w.title.to_lowercase().contains(&q_lower)
                                        || w.mission_goal
                                            .as_deref()
                                            .is_some_and(|g| g.to_lowercase().contains(&q_lower))
                                })
                    })
                    .cloned()
                    .collect();
                if !thread_matches.is_empty() {
                    group.conversations = thread_matches;
                } else if !proj_match {
                    continue;
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

            // Build Mission Workspace groups preserving recency order of conversations
            let mut ws_order: Vec<String> = Vec::new();
            let mut ws_buckets: HashMap<String, Vec<SidebarConversationView>> = HashMap::new();
            for conv in &group.conversations {
                let ws_id = conv.agent.workspace_id.clone();
                if !ws_buckets.contains_key(&ws_id) {
                    ws_order.push(ws_id.clone());
                }
                ws_buckets.entry(ws_id).or_default().push(conv.clone());
            }
            group.workspaces = ws_order
                .into_iter()
                .filter_map(|ws_id| {
                    let convs = ws_buckets.remove(&ws_id)?;
                    let first = convs.first()?;
                    let ws_meta = ws_by_id.get(ws_id.as_str()).copied();
                    let peer_count = convs.len() as u32;
                    let running_count = convs
                        .iter()
                        .filter(|c| c.agent.is_running || c.agent.is_awaiting_permission)
                        .count() as u32;
                    let unread_count = convs.iter().filter(|c| c.is_unread).count() as u32;
                    let title = ws_meta
                        .map(|w| w.title.clone())
                        .filter(|t| !t.trim().is_empty())
                        .unwrap_or_else(|| first.agent.title.clone());
                    let git_branch = ws_meta
                        .and_then(|w| w.git_branch.clone())
                        .or_else(|| first.agent.worktree_branch.clone());
                    let is_worktree = ws_meta.is_some_and(|w| w.is_worktree)
                        || convs.iter().all(|c| c.is_worktree);
                    let has_conflict = host.workspace_conflict_ids.contains(&ws_id);
                    Some(SidebarWorkspaceGroupView {
                        workspace_id: ws_id,
                        title,
                        mission_goal: ws_meta.and_then(|w| w.mission_goal.clone()),
                        cwd: ws_meta
                            .map(|w| w.cwd.clone())
                            .unwrap_or_else(|| first.agent.cwd.clone()),
                        git_branch,
                        is_worktree,
                        board_revision: ws_meta.map(|w| w.board_revision).unwrap_or(0),
                        has_conflict,
                        peer_count,
                        running_count,
                        unread_count,
                        is_multi_agent: peer_count > 1,
                        conversations: convs,
                    })
                })
                .collect();

            all_groups.push(group);
        }

        // 4. Launchpad candidate projects for the selected host (or single host)
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
            for ws in host.workspaces {
                if !ws.is_worktree && !is_bare_home_dir(&ws.cwd) {
                    push_lp(
                        &mut host_lp,
                        format!("{host_id}::ws::{}", ws.workspace_id),
                        &ws.title,
                        &ws.cwd,
                        ws.git_branch.clone(),
                    );
                }
            }
            for dp in host.discovered_projects {
                if !is_bare_home_dir(&dp.path) && !dp.path.contains("/worktrees/") {
                    push_lp(
                        &mut host_lp,
                        format!("{host_id}::dp::{}", dp.path),
                        &dp.name,
                        &dp.path,
                        dp.git_branch.clone(),
                    );
                }
            }
            if let Some(a_cwd) = params.active_cwd
                && !a_cwd.trim().is_empty()
                && !is_bare_home_dir(a_cwd)
            {
                push_lp(
                    &mut host_lp,
                    format!("{host_id}::active::{a_cwd}"),
                    &basename_of_project_path(a_cwd),
                    a_cwd,
                    host.active_git_branch.clone(),
                );
            }
            if host_lp.is_empty()
                && let Some(first_dp) = host.discovered_projects.first()
            {
                push_lp(
                    &mut host_lp,
                    format!("{host_id}::dp::{}", first_dp.path),
                    &first_dp.name,
                    &first_dp.path,
                    first_dp.git_branch.clone(),
                );
            }
            launchpad_projects.extend(host_lp);
        }
    }

    // Sort projects uniformly
    match params.sort {
        SidebarProjectSort::Alphabetical => {
            all_groups.sort_by(|a, b| {
                a.is_home_workspace
                    .cmp(&b.is_home_workspace)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                    .then_with(|| a.primary_cwd.cmp(&b.primary_cwd))
            });
        }
        SidebarProjectSort::Recent => {
            all_groups.sort_by(|a, b| {
                a.is_home_workspace
                    .cmp(&b.is_home_workspace)
                    .then_with(|| b.conversations.is_empty().cmp(&a.conversations.is_empty()))
                    .then_with(|| b.sort_weight_ms.cmp(&a.sort_weight_ms))
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
        }
        SidebarProjectSort::Manual => {
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
                        .then_with(|| b.conversations.is_empty().cmp(&a.conversations.is_empty()))
                        .then_with(|| b.sort_weight_ms.cmp(&a.sort_weight_ms))
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
                }
            });
        }
    }

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
            updated_at_rfc3339: updated_at_rfc3339.to_string(),
            is_pinned,
            seen_seq: if is_unread { 2 } else { 5 },
            read_seq: if is_unread { 2 } else { 5 },
            is_unread,
            queued_prompts: Vec::new(),
            handle: None,
            cluster_suggestion: None,
            worktree_branch: None,
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

        // Only `parent` appears in the project list; subagent still surfaces under needs_attention
        assert_eq!(view.projects.len(), 1);
        assert_eq!(view.projects[0].conversations.len(), 1);
        assert_eq!(view.projects[0].conversations[0].agent.agent_id, "parent");
        assert!(view.projects[0].conversations[0].is_unread);
        assert_eq!(view.pinned_conversations.len(), 0);
        assert_eq!(view.needs_attention.len(), 1);
        assert_eq!(view.needs_attention[0].agent.agent_id, "sub_same");
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
    }
}
