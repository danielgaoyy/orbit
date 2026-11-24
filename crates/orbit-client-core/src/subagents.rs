//! Cycle-safe, depth-bounded recursive multi-level subagent lineage computation shared across
//! Desktop, iOS, Android, and HarmonyOS.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::viewmodel::AgentCardView;

/// Hard cap on recursive subagent traversal depth (Parent -> Child -> ... up to 8 levels).
pub const MAX_SUBAGENT_LINEAGE_DEPTH: u32 = 8;

/// Single hop in a subagent lineage breadcrumb (`Root -> Child -> Grandchild`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentLineageStepView {
    pub host_id: String,
    pub agent_id: String,
    pub title: String,
}

/// Fleet-scoped agent snapshot input used for recursive cross-host lineage resolution.
#[derive(Debug, Clone)]
pub struct FleetAgentNodeRef {
    pub host_id: String,
    pub host_name: String,
    pub agent: AgentCardView,
}

/// Flattened, pre-order recursive descendant entry emitted by `collect_descendant_subagents`.
#[derive(Debug, Clone)]
pub struct ResolvedDescendantSubagent {
    pub host_id: String,
    pub host_name: String,
    pub agent: AgentCardView,
    pub parent_host_id: String,
    pub parent_agent_id: String,
    /// 1 = direct child, 2 = grandchild, ..., up to `MAX_SUBAGENT_LINEAGE_DEPTH`.
    pub depth: u32,
    /// Ordered lineage breadcrumb starting from the root inspected agent down to this descendant.
    pub lineage: Vec<SubagentLineageStepView>,
}

fn effective_agent_title(agent: &AgentCardView) -> String {
    let title = agent.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    if let Some(handle) = agent
        .handle
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return format!("@{handle}");
    }
    agent.agent_id.clone()
}

fn parent_key_of(node: &FleetAgentNodeRef) -> Option<(String, String)> {
    let parent_id = node.agent.parent_agent_id.as_deref()?.trim();
    if parent_id.is_empty() {
        return None;
    }
    let parent_host = node
        .agent
        .parent_host_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(node.host_id.as_str());
    Some((parent_host.to_string(), parent_id.to_string()))
}

/// Compute the ancestor breadcrumb chain `[Root, ..., Parent, Self]` for `(host_id, agent_id)`,
/// bounded to `MAX_SUBAGENT_LINEAGE_DEPTH` and protected against cycles.
/// Returns an empty vector if the agent has no parent (is already a top-level root).
pub fn collect_ancestor_lineage(
    root_host_id: &str,
    root_agent_id: &str,
    fleet_nodes: &[FleetAgentNodeRef],
) -> Vec<SubagentLineageStepView> {
    let by_key: HashMap<(&str, &str), &FleetAgentNodeRef> = fleet_nodes
        .iter()
        .map(|n| ((n.host_id.as_str(), n.agent.agent_id.as_str()), n))
        .collect();

    let Some(start) = by_key.get(&(root_host_id, root_agent_id)).copied() else {
        return Vec::new();
    };
    if parent_key_of(start).is_none() {
        return Vec::new();
    }

    let mut visited: HashSet<(String, String)> = HashSet::new();
    visited.insert((start.host_id.clone(), start.agent.agent_id.clone()));

    let mut rev_chain = vec![SubagentLineageStepView {
        host_id: start.host_id.clone(),
        agent_id: start.agent.agent_id.clone(),
        title: effective_agent_title(&start.agent),
    }];

    let mut cursor = start;
    for _ in 0..MAX_SUBAGENT_LINEAGE_DEPTH {
        let Some((p_host, p_id)) = parent_key_of(cursor) else {
            break;
        };
        if !visited.insert((p_host.clone(), p_id.clone())) {
            // Cycle detected — stop safely.
            break;
        }
        if let Some(parent_node) = by_key.get(&(p_host.as_str(), p_id.as_str())).copied() {
            rev_chain.push(SubagentLineageStepView {
                host_id: parent_node.host_id.clone(),
                agent_id: parent_node.agent.agent_id.clone(),
                title: effective_agent_title(&parent_node.agent),
            });
            cursor = parent_node;
        } else {
            rev_chain.push(SubagentLineageStepView {
                host_id: p_host,
                agent_id: p_id.clone(),
                title: p_id,
            });
            break;
        }
    }

    rev_chain.reverse();
    rev_chain
}

/// Collect all non-archived (or still-active) managed descendants of `(root_host_id, root_agent_id)`
/// across all hosts, cycle-safe and strictly bounded to `MAX_SUBAGENT_LINEAGE_DEPTH`.
/// Returned in deterministic parent-before-child DFS pre-order so UI trees render indented cleanly.
pub fn collect_descendant_subagents(
    root_host_id: &str,
    root_agent_id: &str,
    root_title: &str,
    fleet_nodes: &[FleetAgentNodeRef],
) -> Vec<ResolvedDescendantSubagent> {
    let mut children_by_parent: HashMap<(String, String), Vec<&FleetAgentNodeRef>> = HashMap::new();
    for node in fleet_nodes {
        let is_active = node.agent.is_running
            || node.agent.is_awaiting_permission
            || node.agent.has_running_subagents;
        if node.agent.archived_at_rfc3339.is_some() && !is_active {
            continue;
        }
        if let Some(pkey) = parent_key_of(node) {
            children_by_parent.entry(pkey).or_default().push(node);
        }
    }

    // Deterministic sibling ordering: awaiting/running first, then newest activity, then ID.
    for children in children_by_parent.values_mut() {
        children.sort_by(|a, b| {
            let rank = |ag: &AgentCardView| -> u8 {
                if ag.is_awaiting_permission {
                    0
                } else if ag.is_running || ag.has_running_subagents {
                    1
                } else {
                    2
                }
            };
            rank(&a.agent)
                .cmp(&rank(&b.agent))
                .then_with(|| b.agent.updated_at_rfc3339.cmp(&a.agent.updated_at_rfc3339))
                .then_with(|| a.agent.agent_id.cmp(&b.agent.agent_id))
        });
    }

    let root_step = SubagentLineageStepView {
        host_id: root_host_id.to_string(),
        agent_id: root_agent_id.to_string(),
        title: if root_title.trim().is_empty() {
            root_agent_id.to_string()
        } else {
            root_title.trim().to_string()
        },
    };

    let mut visited: HashSet<(String, String)> = HashSet::new();
    visited.insert((root_host_id.to_string(), root_agent_id.to_string()));

    struct PendingDescendantFrame {
        host_id: String,
        agent_id: String,
        depth: u32,
        lineage: Vec<SubagentLineageStepView>,
        entry: Option<ResolvedDescendantSubagent>,
    }

    let mut out = Vec::new();
    // Explicit stack for exact iterative pre-order DFS (Child-1 -> Grandchild-1 -> Child-2),
    // with zero recursion stack risk.
    let mut pending: Vec<PendingDescendantFrame> = vec![PendingDescendantFrame {
        host_id: root_host_id.to_string(),
        agent_id: root_agent_id.to_string(),
        depth: 0,
        lineage: vec![root_step],
        entry: None,
    }];

    while let Some(frame) = pending.pop() {
        let PendingDescendantFrame {
            host_id: cur_host,
            agent_id: cur_id,
            depth: cur_depth,
            lineage: cur_lineage,
            entry: maybe_entry,
        } = frame;
        if let Some(entry) = maybe_entry {
            out.push(entry);
        }
        if cur_depth >= MAX_SUBAGENT_LINEAGE_DEPTH {
            continue;
        }
        let Some(children) = children_by_parent.get(&(cur_host.clone(), cur_id.clone())) else {
            continue;
        };
        let mut child_frames = Vec::new();
        for child in children {
            let ckey = (child.host_id.clone(), child.agent.agent_id.clone());
            if !visited.insert(ckey) {
                continue;
            }
            let mut lineage = cur_lineage.clone();
            lineage.push(SubagentLineageStepView {
                host_id: child.host_id.clone(),
                agent_id: child.agent.agent_id.clone(),
                title: effective_agent_title(&child.agent),
            });
            let entry = ResolvedDescendantSubagent {
                host_id: child.host_id.clone(),
                host_name: child.host_name.clone(),
                agent: child.agent.clone(),
                parent_host_id: cur_host.clone(),
                parent_agent_id: cur_id.clone(),
                depth: cur_depth + 1,
                lineage: lineage.clone(),
            };
            child_frames.push(PendingDescendantFrame {
                host_id: child.host_id.clone(),
                agent_id: child.agent.agent_id.clone(),
                depth: cur_depth + 1,
                lineage,
                entry: Some(entry),
            });
        }
        for frame in child_frames.into_iter().rev() {
            pending.push(frame);
        }
    }

    out
}

/// Propagate running-subagent status upward through multi-level parent chains (cycle-safe, max depth 8).
pub fn propagate_active_ancestors(
    initial_active_parents: HashSet<(String, String)>,
    parent_by_agent: &HashMap<(String, String), (String, String)>,
) -> HashSet<(String, String)> {
    let mut result = initial_active_parents;
    let mut queue: VecDeque<((String, String), u32)> =
        result.iter().cloned().map(|k| (k, 1)).collect();
    while let Some((key, depth)) = queue.pop_front() {
        if depth >= MAX_SUBAGENT_LINEAGE_DEPTH {
            continue;
        }
        if let Some(parent) = parent_by_agent.get(&key)
            && result.insert(parent.clone())
        {
            queue.push_back((parent.clone(), depth + 1));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use orbit_protocol::{AgentStatus, AttentionState, ProviderKind, SessionMode};

    use super::*;

    fn mk_node(
        host_id: &str,
        host_name: &str,
        agent_id: &str,
        title: &str,
        parent_host: Option<&str>,
        parent_id: Option<&str>,
        running: bool,
    ) -> FleetAgentNodeRef {
        FleetAgentNodeRef {
            host_id: host_id.into(),
            host_name: host_name.into(),
            agent: AgentCardView {
                agent_id: agent_id.into(),
                workspace_id: "ws_1".into(),
                cwd: "/repo".into(),
                title: title.into(),
                provider: ProviderKind::ClaudeCode,
                provider_label: "Claude Code".into(),
                model: None,
                effort: None,
                fast_mode: false,
                mode: SessionMode::AcceptEdits,
                status: if running {
                    AgentStatus::Running {
                        turn_id: "t1".into(),
                    }
                } else {
                    AgentStatus::Idle
                },
                status_label: if running {
                    "Working".into()
                } else {
                    "Idle".into()
                },
                is_running: running,
                has_background_tasks: false,
                has_armed_timers: false,
                has_running_subagents: false,
                is_awaiting_permission: false,
                parent_agent_id: parent_id.map(ToOwned::to_owned),
                parent_host_id: parent_host.map(ToOwned::to_owned),
                profile_id: None,
                background: false,
                auto_archive: false,
                attention: AttentionState::None,
                archived_at_rfc3339: None,
                labels: HashMap::new(),
                latest_seq: 10,
                created_at_rfc3339: None,
                updated_at_rfc3339: "2026-11-24T12:00:00Z".into(),
                is_pinned: false,
                seen_seq: 10,
                read_seq: 10,
                is_unread: false,
                queued_prompts: Vec::new(),
                handle: None,
                cluster_suggestion: None,
                worktree_branch: None,
                task_summary: None,
                forked_from_agent_id: None,
                forked_at_seq: None,
                acp_agent_id: None,
            },
        }
    }

    #[test]
    fn collects_multilevel_cross_host_descendants_and_ancestor_breadcrumbs() {
        let nodes = vec![
            mk_node("mac", "Mac", "root", "Lead", None, None, true),
            mk_node(
                "mac",
                "Mac",
                "child_a",
                "Researcher",
                None,
                Some("root"),
                true,
            ),
            mk_node(
                "linux",
                "Linux",
                "grand_a1",
                "Benchmarker",
                Some("mac"),
                Some("child_a"),
                false,
            ),
            mk_node(
                "mac",
                "Mac",
                "child_b",
                "Reviewer",
                None,
                Some("root"),
                false,
            ),
        ];

        let desc = collect_descendant_subagents("mac", "root", "Lead", &nodes);
        assert_eq!(desc.len(), 3);
        // Pre-order: child_a (depth 1) -> grand_a1 (depth 2) -> child_b (depth 1)
        assert_eq!(desc[0].agent.agent_id, "child_a");
        assert_eq!(desc[0].depth, 1);
        assert_eq!(desc[1].agent.agent_id, "grand_a1");
        assert_eq!(desc[1].depth, 2);
        assert_eq!(desc[1].host_id, "linux");
        assert_eq!(
            desc[1]
                .lineage
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Lead", "Researcher", "Benchmarker"]
        );
        assert_eq!(desc[2].agent.agent_id, "child_b");
        assert_eq!(desc[2].depth, 1);

        let ancestors = collect_ancestor_lineage("linux", "grand_a1", &nodes);
        assert_eq!(
            ancestors
                .iter()
                .map(|s| s.agent_id.as_str())
                .collect::<Vec<_>>(),
            vec!["root", "child_a", "grand_a1"]
        );
        assert!(collect_ancestor_lineage("mac", "root", &nodes).is_empty());
    }

    #[test]
    fn cycle_safe_and_enforces_max_depth_8() {
        // Cycle: a <-> b
        let cycle_nodes = vec![
            mk_node("h1", "H1", "a", "A", None, Some("b"), true),
            mk_node("h1", "H1", "b", "B", None, Some("a"), true),
        ];
        let desc = collect_descendant_subagents("h1", "a", "A", &cycle_nodes);
        assert_eq!(desc.len(), 1);
        assert_eq!(desc[0].agent.agent_id, "b");
        let anc = collect_ancestor_lineage("h1", "a", &cycle_nodes);
        assert_eq!(anc.len(), 2);

        // Deep chain of 12 levels: must stop at depth 8
        let mut chain = vec![mk_node("h1", "H1", "n0", "N0", None, None, true)];
        for i in 1..=12 {
            chain.push(mk_node(
                "h1",
                "H1",
                &format!("n{i}"),
                &format!("N{i}"),
                None,
                Some(&format!("n{}", i - 1)),
                false,
            ));
        }
        let deep_desc = collect_descendant_subagents("h1", "n0", "N0", &chain);
        assert_eq!(deep_desc.len(), MAX_SUBAGENT_LINEAGE_DEPTH as usize);
        assert_eq!(
            deep_desc.last().map(|d| d.depth),
            Some(MAX_SUBAGENT_LINEAGE_DEPTH)
        );
    }
}
