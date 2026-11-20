//! Mission Workspace Shared Blackboard (`BOARD.md`), Live Workspace Roster XML builder,
//! and Markdown section merger.

use std::path::{Path, PathBuf};

use orbit_protocol::{
    AgentStatus, AgentSummary, WorkspaceBoardSnapshot, WorkspaceEventKind, WorkspaceEventRecord,
    WorkspaceSummary,
};
use orbit_store::OrbitStore;
use sha2::{Digest, Sha256};

use crate::messaging::escape_xml;

/// Canonical on-disk location for a Mission Workspace's Shared Blackboard (`$ORBIT_HOME/workspaces/<ws_id>/BOARD.md`).
pub fn board_file_path(home_dir: &Path, workspace_id: &str) -> PathBuf {
    let safe_id: String = workspace_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    let dir_name = if safe_id.is_empty() {
        "default".to_string()
    } else {
        safe_id
    };
    home_dir.join("workspaces").join(dir_name).join("BOARD.md")
}

/// Default initial Markdown template for a workspace's Shared Blackboard.
pub fn default_board_template(title: &str, mission_goal: Option<&str>) -> String {
    let goal_line = mission_goal
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Coordinate peers in this Mission Workspace.");
    format!(
        "# {title}\n\n## Goal\n{goal_line}\n\n## Decisions & Contracts\n- (Record shared APIs, invariants, and architectural decisions here)\n\n## Tasks\n- (Track peer ownership and status here)\n\n## Findings\n- (Record review findings and verification results here)\n"
    )
}

/// Persist the workspace's Shared Blackboard to `$ORBIT_HOME/workspaces/<ws_id>/BOARD.md`.
pub fn write_board_file(home_dir: &Path, workspace_id: &str, content: &str) -> Option<String> {
    let path = board_file_path(home_dir, workspace_id);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if std::fs::write(&path, content).is_ok() {
        Some(path.to_string_lossy().to_string())
    } else {
        None
    }
}

/// Upsert a `## <section>` Markdown section inside `existing`, replacing its body if present
/// or appending a new `## <section>` block at the end if absent.
pub fn upsert_markdown_section(existing: &str, section_name: &str, body: &str) -> String {
    let clean_sec = section_name.trim().trim_start_matches('#').trim();
    if clean_sec.is_empty() {
        return body.to_string();
    }
    let clean_body = body.trim();
    let target_lower = clean_sec.to_ascii_lowercase();

    let lines: Vec<&str> = existing.lines().collect();
    let mut start_idx: Option<usize> = None;
    let mut end_idx: usize = lines.len();

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let heading = rest.trim().to_ascii_lowercase();
            if start_idx.is_none() {
                if heading == target_lower {
                    start_idx = Some(idx);
                }
            } else {
                end_idx = idx;
                break;
            }
        } else if start_idx.is_some() && trimmed.starts_with("# ") {
            end_idx = idx;
            break;
        }
    }

    if let Some(start) = start_idx {
        let mut out: Vec<String> = Vec::with_capacity(lines.len() + 4);
        for l in &lines[..start] {
            out.push((*l).to_string());
        }
        out.push(format!("## {clean_sec}"));
        if !clean_body.is_empty() {
            out.push(clean_body.to_string());
        }
        if end_idx < lines.len() {
            out.push(String::new());
            for l in &lines[end_idx..] {
                out.push((*l).to_string());
            }
        }
        let mut joined = out.join("\n");
        if !joined.ends_with('\n') {
            joined.push('\n');
        }
        joined
    } else {
        let mut out = existing.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("## {clean_sec}\n"));
        if !clean_body.is_empty() {
            out.push_str(clean_body);
            out.push('\n');
        }
        out
    }
}

/// Build a concise `<orbit-workspace>` live awareness block and its SHA-256 fingerprint for
/// deduplicated injection into a peer agent's turn prompt.
///
/// Returns `None` for isolated single-agent workspaces that have no shared board content yet,
/// ensuring zero prompt bloat for simple standalone conversations.
pub fn build_workspace_roster_context(
    store: &OrbitStore,
    home_dir: &Path,
    workspace: &WorkspaceSummary,
    current_agent_id: &str,
    peers: &[AgentSummary],
    recent_events: &[WorkspaceEventRecord],
    board: Option<&WorkspaceBoardSnapshot>,
) -> Option<(String, String)> {
    let active_peers: Vec<&AgentSummary> = peers
        .iter()
        .filter(|a| a.archived_at.is_none() && !a.background)
        .collect();
    let has_other_peers = active_peers.iter().any(|a| a.agent_id != current_agent_id);
    let has_board = board.is_some_and(|b| !b.content.trim().is_empty());

    if !has_other_peers && !has_board {
        return None;
    }

    let board_path = board.and_then(|b| b.board_path.clone()).unwrap_or_else(|| {
        board_file_path(home_dir, &workspace.workspace_id)
            .to_string_lossy()
            .to_string()
    });
    let board_rev = board
        .map(|b| b.revision)
        .unwrap_or(workspace.board_revision);

    let mut xml = String::new();
    xml.push_str(&format!(
        "<orbit-workspace id=\"{}\" title=\"{}\" board-path=\"{}\" board-revision=\"{}\"",
        escape_xml(&workspace.workspace_id),
        escape_xml(&workspace.title),
        escape_xml(&board_path),
        board_rev
    ));
    if let Some(ref goal) = workspace.mission_goal
        && !goal.trim().is_empty()
    {
        xml.push_str(&format!(" goal=\"{}\"", escape_xml(goal.trim())));
    }
    xml.push_str(">\n");

    // Current agent identity + sibling peers
    let self_handle = active_peers
        .iter()
        .find(|a| a.agent_id == current_agent_id)
        .and_then(|a| a.handle.as_deref())
        .unwrap_or("peer");
    xml.push_str(&format!(
        "  <self agent-id=\"{}\" handle=\"@{}\" />\n",
        escape_xml(current_agent_id),
        escape_xml(self_handle)
    ));

    xml.push_str("  <peers>\n");
    for peer in &active_peers {
        if peer.agent_id == current_agent_id {
            continue;
        }
        let handle = peer.handle.as_deref().unwrap_or("peer");
        let status_slug = status_short_slug(&peer.status);
        let last_summary = store
            .last_assistant_message_text(&peer.agent_id)
            .ok()
            .flatten()
            .map(|s| single_line_excerpt(&s, 140))
            .unwrap_or_else(|| single_line_excerpt(&peer.title, 100));
        xml.push_str(&format!(
            "    <peer handle=\"@{}\" agent-id=\"{}\" provider=\"{}\" status=\"{}\" cwd=\"{}\">{}</peer>\n",
            escape_xml(handle),
            escape_xml(&peer.agent_id),
            escape_xml(peer.provider.display_name()),
            escape_xml(status_slug),
            escape_xml(&peer.cwd),
            escape_xml(&last_summary),
        ));
    }
    xml.push_str("  </peers>\n");

    if let Some(b) = board
        && !b.content.trim().is_empty()
    {
        let excerpt = truncate_chars(b.content.trim(), 900);
        xml.push_str("  <shared-board>\n");
        xml.push_str(&escape_xml(&excerpt));
        xml.push_str("\n  </shared-board>\n");
    }

    let peer_events: Vec<&WorkspaceEventRecord> = recent_events
        .iter()
        .filter(|e| {
            e.agent_id.as_deref() != Some(current_agent_id)
                || e.kind == WorkspaceEventKind::ConflictWarning
        })
        .rev()
        .take(6)
        .collect();
    if !peer_events.is_empty() {
        xml.push_str("  <recent-events>\n");
        for ev in peer_events.into_iter().rev() {
            let who = ev
                .agent_handle
                .as_deref()
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| "workspace".into());
            xml.push_str(&format!(
                "    <event seq=\"{}\" by=\"{}\">{}</event>\n",
                ev.seq,
                escape_xml(&who),
                escape_xml(&single_line_excerpt(&ev.summary, 160)),
            ));
        }
        xml.push_str("  </recent-events>\n");
    }

    xml.push_str("  <coordination-hint>You share this Mission Workspace with the peers above. Use Orbit MCP tools (`workspace_peers`, `message_peer`, `read_peer_context`, `workspace_board_read`, `workspace_board_update`, `spawn_workspace_peer`) to coordinate directly by @handle without polling.</coordination-hint>\n");
    xml.push_str("</orbit-workspace>");

    let mut hasher = Sha256::new();
    hasher.update(xml.as_bytes());
    let fingerprint = format!("{:x}", hasher.finalize());
    Some((xml, fingerprint))
}

fn status_short_slug(status: &AgentStatus) -> &'static str {
    match status {
        AgentStatus::Initializing => "initializing",
        AgentStatus::Idle => "idle",
        AgentStatus::Running { .. } => "running",
        AgentStatus::AutonomousWakeup { .. } => "waking",
        AgentStatus::AwaitingPermission { .. } => "awaiting_permission",
        AgentStatus::Error { .. } => "error",
        AgentStatus::Closed => "closed",
    }
}

fn single_line_excerpt(raw: &str, max_chars: usize) -> String {
    let collapsed: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, max_chars)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        raw.to_string()
    } else {
        let mut s: String = raw.chars().take(max_chars.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_section_upsert_replaces_or_appends_cleanly() {
        let initial = default_board_template("Sync Mission", Some("Fix cross-device sync"));
        let updated = upsert_markdown_section(
            &initial,
            "Decisions & Contracts",
            "- `read_seq` is strictly monotonic\n- `ManualUnread` preserves `read_seq`",
        );
        assert!(updated.contains("## Goal\nFix cross-device sync"));
        assert!(updated.contains("- `read_seq` is strictly monotonic"));
        assert!(updated.contains("## Tasks\n"));
        assert!(!updated.contains("Record shared APIs"));

        let appended = upsert_markdown_section(&updated, "Review Verdict", "LGTM from @reviewer");
        assert!(appended.contains("## Review Verdict\nLGTM from @reviewer"));
    }
}
