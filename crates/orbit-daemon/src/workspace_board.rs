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

/// Per-agent incremental tracking state for `<orbit-workspace>` prompt injection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkspaceRosterState {
    pub full_injected: bool,
    pub structural_fingerprint: String,
    pub board_revision: u64,
    pub board_hash: String,
    pub last_event_seq: u64,
}

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

fn sha256_hex_str(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn build_full_workspace_xml(
    store: &OrbitStore,
    home_dir: &Path,
    workspace: &WorkspaceSummary,
    current_agent_id: &str,
    active_peers: &[&AgentSummary],
    recent_events: &[WorkspaceEventRecord],
    board: Option<&WorkspaceBoardSnapshot>,
) -> String {
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
    for peer in active_peers {
        if peer.agent_id == current_agent_id {
            continue;
        }
        let handle = peer.handle.as_deref().unwrap_or("peer");
        let status_slug = status_short_slug(&peer.status);
        let last_summary = peer
            .task_summary
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .map(|s| single_line_excerpt(s, 140))
            .or_else(|| {
                store
                    .last_assistant_message_text(&peer.agent_id)
                    .ok()
                    .flatten()
                    .map(|s| single_line_excerpt(&s, 140))
            })
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

    xml.push_str("  <coordination-hint>You share this Mission Workspace with the peers above. Use Orbit MCP tools (`workspace_peers`, `message_peer`, `workspace_board`, `spawn_workspace_peer`, `handoff_worktree`, `orbit_timer`) to coordinate directly by @handle without polling.</coordination-hint>\n");
    xml.push_str("</orbit-workspace>");
    xml
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
    if !workspace.is_curated {
        return None;
    }

    let active_peers: Vec<&AgentSummary> = peers
        .iter()
        .filter(|a| a.archived_at.is_none() && !a.background && a.parent_agent_id.is_none())
        .collect();
    let has_other_peers = active_peers.iter().any(|a| a.agent_id != current_agent_id);
    let has_board = board.is_some_and(|b| !b.content.trim().is_empty());

    if !has_other_peers && !has_board {
        return None;
    }

    let xml = build_full_workspace_xml(
        store,
        home_dir,
        workspace,
        current_agent_id,
        &active_peers,
        recent_events,
        board,
    );
    let fingerprint = sha256_hex_str(&xml);
    Some((xml, fingerprint))
}

/// Build either the initial full `<orbit-workspace>` block (on Turn 1) or a compact
/// `<orbit-workspace-delta>` block (on Turn 2+) containing only structural peer membership changes,
/// shared board updates, or new high-signal workspace events — returning `Some((None, state))` when
/// nothing material changed so prompt prefixes stay lightweight and cache-friendly.
#[allow(clippy::too_many_arguments)]
pub fn build_workspace_roster_context_incremental(
    store: &OrbitStore,
    home_dir: &Path,
    workspace: &WorkspaceSummary,
    current_agent_id: &str,
    peers: &[AgentSummary],
    recent_events: &[WorkspaceEventRecord],
    board: Option<&WorkspaceBoardSnapshot>,
    prev: Option<&WorkspaceRosterState>,
) -> Option<(Option<String>, WorkspaceRosterState)> {
    if !workspace.is_curated {
        return None;
    }

    let active_peers: Vec<&AgentSummary> = peers
        .iter()
        .filter(|a| a.archived_at.is_none() && !a.background && a.parent_agent_id.is_none())
        .collect();
    let has_other_peers = active_peers.iter().any(|a| a.agent_id != current_agent_id);
    let has_board = board.is_some_and(|b| !b.content.trim().is_empty());

    if !has_other_peers && !has_board {
        return None;
    }

    let self_handle = active_peers
        .iter()
        .find(|a| a.agent_id == current_agent_id)
        .and_then(|a| a.handle.as_deref())
        .unwrap_or("peer");

    // Structural fingerprint intentionally excludes volatile `status` and `last_summary` fields
    // so parallel peer activity does not re-inject the entire roster on every turn.
    let mut sibling_peers: Vec<&AgentSummary> = active_peers
        .iter()
        .copied()
        .filter(|a| a.agent_id != current_agent_id)
        .collect();
    sibling_peers.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));

    let mut struct_seed = format!(
        "{}|{}|{}|{}",
        workspace.workspace_id,
        workspace.title,
        workspace.mission_goal.as_deref().unwrap_or(""),
        self_handle
    );
    for p in &sibling_peers {
        struct_seed.push_str(&format!(
            "|{}:{}:{}:{}",
            p.agent_id,
            p.handle.as_deref().unwrap_or("peer"),
            p.provider.slug(),
            p.cwd
        ));
    }
    let structural_fingerprint = sha256_hex_str(&struct_seed);

    let board_rev = board
        .map(|b| b.revision)
        .unwrap_or(workspace.board_revision);
    let board_trimmed = board.map(|b| b.content.trim()).unwrap_or("");
    let board_hash = sha256_hex_str(board_trimmed);

    let prev_last_seq = prev.map(|p| p.last_event_seq).unwrap_or(0);
    let max_event_seq = recent_events
        .iter()
        .map(|e| e.seq)
        .max()
        .unwrap_or(prev_last_seq)
        .max(prev_last_seq);

    let next_state = WorkspaceRosterState {
        full_injected: true,
        structural_fingerprint: structural_fingerprint.clone(),
        board_revision: board_rev,
        board_hash: board_hash.clone(),
        last_event_seq: max_event_seq,
    };

    let Some(prev_state) = prev.filter(|p| p.full_injected) else {
        let full_xml = build_full_workspace_xml(
            store,
            home_dir,
            workspace,
            current_agent_id,
            &active_peers,
            recent_events,
            board,
        );
        return Some((Some(full_xml), next_state));
    };

    let structural_changed = prev_state.structural_fingerprint != structural_fingerprint;
    let board_changed = !board_trimmed.is_empty()
        && (prev_state.board_revision != board_rev || prev_state.board_hash != board_hash);

    let new_events: Vec<&WorkspaceEventRecord> = recent_events
        .iter()
        .filter(|e| {
            e.seq > prev_state.last_event_seq
                && match e.kind {
                    WorkspaceEventKind::ConflictWarning => true,
                    WorkspaceEventKind::PeerJoined | WorkspaceEventKind::WorkspaceMerged => {
                        e.agent_id.as_deref() != Some(current_agent_id)
                    }
                    WorkspaceEventKind::BoardUpdated => {
                        !board_changed && e.agent_id.as_deref() != Some(current_agent_id)
                    }
                    WorkspaceEventKind::PeerFinished
                    | WorkspaceEventKind::PeerMessage
                    | WorkspaceEventKind::FileEdited => false,
                }
        })
        .rev()
        .take(4)
        .collect();

    if !structural_changed && !board_changed && new_events.is_empty() {
        return Some((None, next_state));
    }

    let mut delta = String::new();
    delta.push_str(&format!(
        "<orbit-workspace-delta id=\"{}\" board-revision=\"{}\">\n",
        escape_xml(&workspace.workspace_id),
        board_rev
    ));

    if structural_changed {
        delta.push_str("  <peers-updated>\n");
        for peer in &sibling_peers {
            let handle = peer.handle.as_deref().unwrap_or("peer");
            delta.push_str(&format!(
                "    <peer handle=\"@{}\" agent-id=\"{}\" provider=\"{}\" cwd=\"{}\" />\n",
                escape_xml(handle),
                escape_xml(&peer.agent_id),
                escape_xml(peer.provider.display_name()),
                escape_xml(&peer.cwd),
            ));
        }
        delta.push_str("  </peers-updated>\n");
    }

    if board_changed {
        let updated_by = board
            .and_then(|b| b.updated_by.as_deref())
            .unwrap_or("peer");
        let excerpt = truncate_chars(board_trimmed, 450);
        delta.push_str(&format!(
            "  <board-updated rev=\"{}\" by=\"{}\">\n{}\n  </board-updated>\n",
            board_rev,
            escape_xml(updated_by),
            escape_xml(&excerpt),
        ));
    }

    if !new_events.is_empty() {
        delta.push_str("  <new-events>\n");
        for ev in new_events.into_iter().rev() {
            let who = ev
                .agent_handle
                .as_deref()
                .map(|h| format!("@{h}"))
                .unwrap_or_else(|| "workspace".into());
            delta.push_str(&format!(
                "    <event seq=\"{}\" by=\"{}\">{}</event>\n",
                ev.seq,
                escape_xml(&who),
                escape_xml(&single_line_excerpt(&ev.summary, 160)),
            ));
        }
        delta.push_str("  </new-events>\n");
    }

    delta.push_str("</orbit-workspace-delta>");
    Some((Some(delta), next_state))
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

    #[test]
    fn incremental_roster_emits_zero_bytes_on_volatile_status_and_compact_delta_on_board_change() {
        use orbit_protocol::{AttentionState, ProviderKind, SessionMode, WorkspaceSetupStatus};
        use std::collections::HashMap;

        let store = OrbitStore::open_in_memory().expect("open");
        let now = chrono::Utc::now();
        let ws = WorkspaceSummary {
            workspace_id: "ws_inc".into(),
            title: "Incremental Test".into(),
            cwd: "/repo".into(),
            git_branch: Some("main".into()),
            is_worktree: false,
            updated_at: now,
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: WorkspaceSetupStatus::Idle,
            mission_goal: Some("Test incremental delta".into()),
            project_root: Some("/repo".into()),
            board_revision: 1,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        };
        let mk_peer = |id: &str, handle: &str, status: AgentStatus| AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_inc".into(),
            cwd: "/repo".into(),
            title: handle.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 1,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 1,
            is_pinned: false,
            pinned_order: 0,
            handle: Some(handle.into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };

        let peers_t1 = vec![
            mk_peer("ag_1", "proto", AgentStatus::Idle),
            mk_peer(
                "ag_2",
                "core",
                AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            ),
        ];
        let board_v1 = WorkspaceBoardSnapshot {
            workspace_id: "ws_inc".into(),
            content: "## Decisions\nUse i64".into(),
            revision: 1,
            updated_by: Some("proto".into()),
            updated_at: now,
            board_path: Some("/tmp/BOARD.md".into()),
        };

        // Turn 1: Full `<orbit-workspace>` block
        let (t1_xml, st1) = build_workspace_roster_context_incremental(
            &store,
            Path::new("/tmp"),
            &ws,
            "ag_1",
            &peers_t1,
            &[],
            Some(&board_v1),
            None,
        )
        .expect("t1");
        let full = t1_xml.expect("t1 must inject full xml");
        assert!(full.contains("<orbit-workspace "));
        assert!(full.contains("@core"));

        // Turn 2: @core flipped Running -> Idle and emitted FileEdited, board unchanged -> 0 bytes!
        let peers_t2 = vec![
            mk_peer("ag_1", "proto", AgentStatus::Idle),
            mk_peer("ag_2", "core", AgentStatus::Idle),
        ];
        let edit_ev = WorkspaceEventRecord {
            seq: 1,
            workspace_id: "ws_inc".into(),
            agent_id: Some("ag_2".into()),
            agent_handle: Some("core".into()),
            kind: WorkspaceEventKind::FileEdited,
            summary: "@core edited src/lib.rs".into(),
            file_path: Some("src/lib.rs".into()),
            created_at: now,
        };
        let (t2_xml, st2) = build_workspace_roster_context_incremental(
            &store,
            Path::new("/tmp"),
            &ws,
            "ag_1",
            &peers_t2,
            &[edit_ev],
            Some(&board_v1),
            Some(&st1),
        )
        .expect("t2");
        assert!(
            t2_xml.is_none(),
            "volatile status flip + FileEdited must inject 0 bytes on Turn 2"
        );

        // Turn 3: board updates to rev 2 -> compact `<orbit-workspace-delta>`
        let board_v2 = WorkspaceBoardSnapshot {
            workspace_id: "ws_inc".into(),
            content: "## Decisions\nUse i64 + CAS".into(),
            revision: 2,
            updated_by: Some("core".into()),
            updated_at: now,
            board_path: Some("/tmp/BOARD.md".into()),
        };
        let (t3_xml, _) = build_workspace_roster_context_incremental(
            &store,
            Path::new("/tmp"),
            &ws,
            "ag_1",
            &peers_t2,
            &[],
            Some(&board_v2),
            Some(&st2),
        )
        .expect("t3");
        let delta = t3_xml.expect("t3 delta");
        assert!(delta.contains("<orbit-workspace-delta "));
        assert!(delta.contains("<board-updated rev=\"2\""));
        assert!(!delta.contains("<coordination-hint>"));
    }
}
