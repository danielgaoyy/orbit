//! Multi-device sync persistence (`read_seq`, `ManualUnread`, pinned threads, host projects & order)
//! and Mission Workspace collaboration persistence (Shared Blackboard `BOARD.md`, workspace activity
//! feed, file-conflict queries, agent moves & workspace merges).

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentSummary, AttentionState, HostProjectEntry, HostSyncState, WorkspaceBoardSnapshot,
    WorkspaceClusterSuggestion, WorkspaceEventKind, WorkspaceEventRecord, WorkspaceSummary,
};
use rusqlite::{OptionalExtension, params};

use crate::{
    AGENT_SELECT_COLS, OrbitStore, StoreError, WORKSPACE_SELECT_COLS,
    load_queued_prompts_for_agent, map_agent_row, map_workspace_row, parse_ts,
};

fn query_agent_with_queue(
    tx: &rusqlite::Transaction<'_>,
    agent_id: &str,
) -> Result<Option<AgentSummary>, StoreError> {
    let select_sql = format!("SELECT {AGENT_SELECT_COLS} FROM agents WHERE agent_id = ?1");
    let mut updated = tx
        .query_row(&select_sql, params![agent_id], map_agent_row)
        .optional()?;
    if let Some(ref mut ag) = updated {
        ag.queued_prompts = load_queued_prompts_for_agent(tx, &ag.agent_id)?;
    }
    Ok(updated)
}

/// Compact context extracted from an agent's timeline for AI semantic summarization and triage.
#[derive(Debug, Clone, Default)]
pub struct AgentContextSnippet {
    pub first_user_prompt: Option<String>,
    pub latest_user_prompt: Option<String>,
    pub latest_assistant_message: Option<String>,
    pub edited_files: Vec<String>,
}

/// Options controlling atomic semantic identity updates (`set_agent_semantic_identity_with_options`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SemanticIdentityOptions {
    pub mark_auto_named: bool,
    pub force_title: bool,
}

pub(crate) fn migrate_sync_and_mission_schema(
    conn: &rusqlite::Connection,
) -> Result<(), StoreError> {
    for stmt in [
        "ALTER TABLE workspaces ADD COLUMN mission_goal TEXT",
        "ALTER TABLE workspaces ADD COLUMN project_root TEXT",
        "ALTER TABLE workspaces ADD COLUMN board_revision INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE workspaces ADD COLUMN title_locked INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE workspaces ADD COLUMN auto_clustered INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE workspaces ADD COLUMN is_curated INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN read_seq INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN pinned_order INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN handle TEXT",
        "ALTER TABLE agents ADD COLUMN cluster_suggestion_json TEXT",
        "ALTER TABLE agents ADD COLUMN task_summary TEXT",
        "ALTER TABLE client_projects ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0",
    ] {
        let _ = conn.execute(stmt, []);
    }

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS host_projects (
            project_id       TEXT PRIMARY KEY,
            name             TEXT NOT NULL,
            directories_json TEXT NOT NULL,
            sort_order       INTEGER NOT NULL DEFAULT 0,
            hidden           INTEGER NOT NULL DEFAULT 0,
            updated_at_ms    INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS host_kv (
            key           TEXT PRIMARY KEY,
            value_json    TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS client_kv (
            key           TEXT PRIMARY KEY,
            value_json    TEXT NOT NULL,
            updated_at_ms INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS workspace_boards (
            workspace_id TEXT PRIMARY KEY REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
            content      TEXT NOT NULL,
            revision     INTEGER NOT NULL DEFAULT 1,
            updated_by   TEXT,
            board_path   TEXT,
            updated_at   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS workspace_events (
            seq          INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_id TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
            agent_id     TEXT,
            agent_handle TEXT,
            kind         TEXT NOT NULL,
            summary      TEXT NOT NULL,
            file_path    TEXT,
            created_at   TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_workspace_events_ws_seq
            ON workspace_events(workspace_id, seq DESC);

        CREATE INDEX IF NOT EXISTS idx_workspace_events_ws_file
            ON workspace_events(workspace_id, file_path, created_at DESC);
        ",
    )?;
    Ok(())
}

impl OrbitStore {
    // --- Authoritative Multi-Device Read / Unread / Pin Sync ---

    /// Advance an agent's monotonic `read_seq` cursor and clear non-permission attention flags.
    pub fn mark_agent_read_seq(
        &self,
        agent_id: &str,
        seen_seq: Option<u64>,
    ) -> Result<Option<AgentSummary>, StoreError> {
        Ok(self
            .mark_agent_read_seq_with_changed(agent_id, seen_seq)?
            .map(|(ag, _)| ag))
    }

    /// Advance an agent's monotonic `read_seq` cursor and report whether the stored row actually changed.
    /// When neither `read_seq` nor `attention_state` changes, skips the SQLite `UPDATE` completely so
    /// no WAL page is dirtied and callers can suppress redundant `AgentUpdated` broadcasts.
    pub fn mark_agent_read_seq_with_changed(
        &self,
        agent_id: &str,
        seen_seq: Option<u64>,
    ) -> Result<Option<(AgentSummary, bool)>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let row_opt: Option<(i64, i64, String)> = tx
            .query_row(
                "SELECT latest_seq, read_seq, attention_state FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((latest_seq_i64, prev_read_i64, prev_attention)) = row_opt else {
            return Ok(None);
        };
        let target_seq = seen_seq
            .map(|s| s as i64)
            .unwrap_or(latest_seq_i64)
            .max(prev_read_i64);
        let next_attention = if prev_attention == "permission" {
            "permission"
        } else if target_seq >= latest_seq_i64 || prev_attention == "manual_unread" {
            "none"
        } else {
            prev_attention.as_str()
        };
        let changed = target_seq != prev_read_i64 || next_attention != prev_attention;
        if changed {
            tx.execute(
                "UPDATE agents SET read_seq = ?1, attention_state = ?2 WHERE agent_id = ?3",
                params![target_seq, next_attention, agent_id],
            )?;
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        if changed {
            tx.commit()?;
        }
        Ok(updated.map(|ag| (ag, changed)))
    }

    /// Explicitly mark an agent unread across all devices (`AttentionState::ManualUnread`)
    /// without regressing its monotonic `read_seq`.
    pub fn mark_agent_unread(&self, agent_id: &str) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let rows = tx.execute(
            "UPDATE agents
             SET attention_state = CASE
                 WHEN attention_state = 'permission' THEN 'permission'
                 ELSE 'manual_unread'
             END
             WHERE agent_id = ?1",
            params![agent_id],
        )?;
        if rows == 0 {
            return Ok(None);
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Sync agent pin state and ordering across all connected devices.
    pub fn set_agent_pinned(
        &self,
        agent_id: &str,
        pinned: bool,
        pinned_order: Option<i64>,
    ) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let order = if pinned {
            pinned_order.unwrap_or_else(|| Utc::now().timestamp_millis())
        } else {
            0
        };
        let rows = tx.execute(
            "UPDATE agents SET is_pinned = ?1, pinned_order = ?2 WHERE agent_id = ?3",
            params![i32::from(pinned), order, agent_id],
        )?;
        if rows == 0 {
            return Ok(None);
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Update an agent's workspace-local collaborative handle (e.g. `lead`, `reviewer`).
    pub fn set_agent_handle(
        &self,
        agent_id: &str,
        handle: Option<&str>,
    ) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let clean = handle
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let rows = tx.execute(
            "UPDATE agents SET handle = ?1, updated_at = ?2 WHERE agent_id = ?3",
            params![clean, Utc::now().to_rfc3339(), agent_id],
        )?;
        if rows == 0 {
            return Ok(None);
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Persist or clear an auto-clustering recommendation / reversible auto-group banner.
    pub fn set_agent_cluster_suggestion(
        &self,
        agent_id: &str,
        suggestion: Option<&WorkspaceClusterSuggestion>,
    ) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let json = suggestion.map(serde_json::to_string).transpose()?;
        let rows = tx.execute(
            "UPDATE agents SET cluster_suggestion_json = ?1 WHERE agent_id = ?2",
            params![json, agent_id],
        )?;
        if rows == 0 {
            return Ok(None);
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Move an agent into another Mission Workspace (updating `workspace_id`, `updated_at`).
    pub fn move_agent_to_workspace(
        &self,
        agent_id: &str,
        target_workspace_id: &str,
        clear_suggestion: bool,
    ) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        let rows = if clear_suggestion {
            tx.execute(
                "UPDATE agents
                 SET workspace_id = ?1, cluster_suggestion_json = NULL, updated_at = ?2
                 WHERE agent_id = ?3",
                params![target_workspace_id, now, agent_id],
            )?
        } else {
            tx.execute(
                "UPDATE agents
                 SET workspace_id = ?1, updated_at = ?2
                 WHERE agent_id = ?3",
                params![target_workspace_id, now, agent_id],
            )?
        };
        if rows == 0 {
            return Ok(None);
        }
        tx.execute(
            "UPDATE workspaces SET updated_at = ?1 WHERE workspace_id = ?2",
            params![now, target_workspace_id],
        )?;
        let updated = query_agent_with_queue(&tx, agent_id)?;
        tx.commit()?;
        Ok(updated)
    }

    /// Merge all agents and workspace events from `source_workspace_id` into `target_workspace_id`,
    /// then soft-archive `source_workspace_id`. Returns the number of agents moved.
    pub fn merge_workspaces(
        &self,
        source_workspace_id: &str,
        target_workspace_id: &str,
    ) -> Result<usize, StoreError> {
        if source_workspace_id == target_workspace_id {
            return Ok(0);
        }
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        let moved = tx.execute(
            "UPDATE agents
             SET workspace_id = ?1, cluster_suggestion_json = NULL, updated_at = ?2
             WHERE workspace_id = ?3",
            params![target_workspace_id, now, source_workspace_id],
        )?;
        tx.execute(
            "UPDATE workspace_events SET workspace_id = ?1 WHERE workspace_id = ?2",
            params![target_workspace_id, source_workspace_id],
        )?;
        tx.execute(
            "UPDATE workspaces SET archived_at = ?1, updated_at = ?1 WHERE workspace_id = ?2",
            params![now, source_workspace_id],
        )?;
        tx.execute(
            "UPDATE workspaces SET is_curated = 1, updated_at = ?1 WHERE workspace_id = ?2",
            params![now, target_workspace_id],
        )?;
        tx.commit()?;
        Ok(moved)
    }

    /// Explicitly mark a workspace as curated (`true`) or unclassified (`false`).
    pub fn set_workspace_curated(
        &self,
        workspace_id: &str,
        is_curated: bool,
    ) -> Result<Option<WorkspaceSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE workspaces SET is_curated = ?1, updated_at = ?2 WHERE workspace_id = ?3",
            params![i32::from(is_curated), now, workspace_id],
        )?;
        let select_sql =
            format!("SELECT {WORKSPACE_SELECT_COLS} FROM workspaces WHERE workspace_id = ?1");
        let updated = tx
            .query_row(&select_sql, params![workspace_id], map_workspace_row)
            .optional()?;
        tx.commit()?;
        Ok(updated)
    }

    /// Apply AI-derived semantic handle, title, and/or live `task_summary` while strictly
    /// respecting user locks (`labels["orbit.handle_locked"] == "1"` and `labels["orbit.title_locked"] == "1"`).
    pub fn set_agent_semantic_identity(
        &self,
        agent_id: &str,
        handle: Option<&str>,
        title: Option<&str>,
        task_summary: Option<&str>,
    ) -> Result<Option<AgentSummary>, StoreError> {
        self.set_agent_semantic_identity_with_options(
            agent_id,
            handle,
            title,
            task_summary,
            SemanticIdentityOptions::default(),
        )
    }

    /// Atomically apply AI-derived semantic handle, title, and/or live `task_summary` inside a
    /// single SQLite transaction using a targeted `UPDATE` (never touching `status_json`,
    /// `archived_at`, `attention_state`, `is_pinned`, or other concurrent lifecycle columns).
    pub fn set_agent_semantic_identity_with_options(
        &self,
        agent_id: &str,
        handle: Option<&str>,
        title: Option<&str>,
        task_summary: Option<&str>,
        options: SemanticIdentityOptions,
    ) -> Result<Option<AgentSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let row_opt: Option<(Option<String>, String, Option<String>, String)> = tx
            .query_row(
                "SELECT handle, title, task_summary, labels_json FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((mut cur_handle, mut cur_title, mut cur_summary, labels_json)) = row_opt else {
            return Ok(None);
        };
        let mut labels: std::collections::HashMap<String, String> =
            serde_json::from_str(&labels_json).unwrap_or_default();

        let handle_locked = labels
            .get("orbit.handle_locked")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let title_locked = labels
            .get("orbit.title_locked")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

        let mut changed = false;
        if !handle_locked
            && let Some(h) = handle.map(str::trim).filter(|s| !s.is_empty())
            && cur_handle.as_deref() != Some(h)
        {
            cur_handle = Some(h.to_string());
            changed = true;
        }
        if (options.force_title || !title_locked)
            && let Some(t) = title.map(str::trim).filter(|s| !s.is_empty())
            && cur_title != t
        {
            cur_title = t.to_string();
            changed = true;
        }
        if let Some(ts) = task_summary.map(str::trim).filter(|s| !s.is_empty())
            && cur_summary.as_deref() != Some(ts)
        {
            cur_summary = Some(ts.to_string());
            changed = true;
        }
        if options.mark_auto_named
            && labels.get("orbit.auto_named").map(String::as_str) != Some("1")
        {
            labels.insert("orbit.auto_named".into(), "1".into());
            changed = true;
        }
        if changed {
            let next_labels_json = serde_json::to_string(&labels)?;
            tx.execute(
                "UPDATE agents
                 SET handle = ?1,
                     title = ?2,
                     task_summary = ?3,
                     labels_json = ?4,
                     updated_at = ?5
                 WHERE agent_id = ?6",
                params![
                    cur_handle,
                    cur_title,
                    cur_summary,
                    next_labels_json,
                    Utc::now().to_rfc3339(),
                    agent_id,
                ],
            )?;
        }
        let updated = query_agent_with_queue(&tx, agent_id)?;
        if changed {
            tx.commit()?;
        }
        Ok(updated)
    }

    /// Extract a compact conversation context snippet (first user prompt, latest user prompt,
    /// latest assistant reply, and recently edited file paths) for fast AI summarization & triage.
    pub fn agent_context_snippet(&self, agent_id: &str) -> Result<AgentContextSnippet, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut snippet = AgentContextSnippet::default();

        // First user prompt
        if let Ok(Some(raw)) = conn
            .query_row(
                "SELECT payload_json FROM timeline_events
                 WHERE agent_id = ?1 AND event_type = 'user_prompt'
                 ORDER BY seq ASC LIMIT 1",
                params![agent_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            && let Ok(orbit_protocol::TimelinePayload::UserPrompt { text, .. }) =
                serde_json::from_str::<orbit_protocol::TimelinePayload>(&raw)
        {
            snippet.first_user_prompt = Some(text);
        }

        // Latest user prompt
        if let Ok(Some(raw)) = conn
            .query_row(
                "SELECT payload_json FROM timeline_events
                 WHERE agent_id = ?1 AND event_type = 'user_prompt'
                 ORDER BY seq DESC LIMIT 1",
                params![agent_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            && let Ok(orbit_protocol::TimelinePayload::UserPrompt { text, .. }) =
                serde_json::from_str::<orbit_protocol::TimelinePayload>(&raw)
        {
            snippet.latest_user_prompt = Some(text);
        }

        // Latest assistant reply
        if let Ok(Some(raw)) = conn
            .query_row(
                "SELECT payload_json FROM timeline_events
                 WHERE agent_id = ?1 AND event_type = 'assistant_message'
                 ORDER BY seq DESC LIMIT 1",
                params![agent_id],
                |r| r.get::<_, String>(0),
            )
            .optional()
            && let Ok(orbit_protocol::TimelinePayload::AssistantMessage { text, .. }) =
                serde_json::from_str::<orbit_protocol::TimelinePayload>(&raw)
        {
            snippet.latest_assistant_message = Some(text);
        }

        // Recently edited files (deduplicated, up to 8)
        if let Ok(mut stmt) = conn.prepare(
            "SELECT payload_json FROM timeline_events
             WHERE agent_id = ?1 AND event_type = 'tool_call'
             ORDER BY seq DESC LIMIT 40",
        ) && let Ok(rows) = stmt.query_map(params![agent_id], |r| r.get::<_, String>(0))
        {
            let mut seen = std::collections::HashSet::new();
            for raw in rows.flatten() {
                if let Ok(orbit_protocol::TimelinePayload::ToolCall {
                    detail: orbit_protocol::ToolCallDetail::FileEdit { path, .. },
                    ..
                }) = serde_json::from_str::<orbit_protocol::TimelinePayload>(&raw)
                    && !path.trim().is_empty()
                    && seen.insert(path.clone())
                {
                    snippet.edited_files.push(path);
                    if snippet.edited_files.len() >= 8 {
                        break;
                    }
                }
            }
        }

        Ok(snippet)
    }

    /// Update Mission Workspace title, goal, and optional `title_locked` flag.
    pub fn update_workspace_goal(
        &self,
        workspace_id: &str,
        title: Option<&str>,
        mission_goal: Option<&str>,
        title_locked: Option<bool>,
        auto_clustered: Option<bool>,
    ) -> Result<Option<WorkspaceSummary>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        if let Some(t) = title.map(str::trim).filter(|s| !s.is_empty()) {
            tx.execute(
                "UPDATE workspaces SET title = ?1, is_curated = 1, updated_at = ?2 WHERE workspace_id = ?3",
                params![t, now, workspace_id],
            )?;
        }
        if let Some(g) = mission_goal {
            let clean = g.trim();
            let val = (!clean.is_empty()).then_some(clean);
            tx.execute(
                "UPDATE workspaces SET mission_goal = ?1, is_curated = 1, updated_at = ?2 WHERE workspace_id = ?3",
                params![val, now, workspace_id],
            )?;
        }
        if let Some(locked) = title_locked {
            tx.execute(
                "UPDATE workspaces SET title_locked = ?1 WHERE workspace_id = ?2",
                params![i32::from(locked), workspace_id],
            )?;
        }
        if let Some(ac) = auto_clustered {
            tx.execute(
                "UPDATE workspaces SET auto_clustered = ?1 WHERE workspace_id = ?2",
                params![i32::from(ac), workspace_id],
            )?;
        }
        let select_sql =
            format!("SELECT {WORKSPACE_SELECT_COLS} FROM workspaces WHERE workspace_id = ?1");
        let updated = tx
            .query_row(&select_sql, params![workspace_id], map_workspace_row)
            .optional()?;
        tx.commit()?;
        Ok(updated)
    }

    // --- Host-Authoritative Projects & Ordering Sync ---

    pub fn upsert_host_project(
        &self,
        entry: &HostProjectEntry,
    ) -> Result<HostSyncState, StoreError> {
        {
            let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            let dirs_json = serde_json::to_string(&entry.directories)?;
            conn.execute(
                "INSERT INTO host_projects (
                    project_id, name, directories_json, sort_order, hidden, updated_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(project_id) DO UPDATE SET
                    name = excluded.name,
                    directories_json = excluded.directories_json,
                    sort_order = excluded.sort_order,
                    hidden = excluded.hidden,
                    updated_at_ms = MAX(host_projects.updated_at_ms, excluded.updated_at_ms)",
                params![
                    entry.project_id,
                    entry.name,
                    dirs_json,
                    entry.sort_order,
                    i32::from(entry.hidden),
                    entry.updated_at_ms,
                ],
            )?;
        }
        self.get_host_sync_state()
    }

    pub fn delete_host_project(&self, project_id: &str) -> Result<HostSyncState, StoreError> {
        {
            let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            conn.execute(
                "DELETE FROM host_projects WHERE project_id = ?1",
                params![project_id],
            )?;
        }
        self.get_host_sync_state()
    }

    pub fn set_host_project_order(
        &self,
        project_order: &[String],
        project_sort_mode: Option<&str>,
    ) -> Result<HostSyncState, StoreError> {
        {
            let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            let tx = conn.transaction()?;
            let now_ms = Utc::now().timestamp_millis();
            let order_json = serde_json::to_string(project_order)?;
            tx.execute(
                "INSERT INTO host_kv (key, value_json, updated_at_ms)
                 VALUES ('project_order', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET
                    value_json = excluded.value_json,
                    updated_at_ms = excluded.updated_at_ms",
                params![order_json, now_ms],
            )?;
            if let Some(mode) = project_sort_mode.map(str::trim).filter(|s| !s.is_empty()) {
                let mode_json = serde_json::to_string(mode)?;
                tx.execute(
                    "INSERT INTO host_kv (key, value_json, updated_at_ms)
                     VALUES ('project_sort_mode', ?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET
                        value_json = excluded.value_json,
                        updated_at_ms = excluded.updated_at_ms",
                    params![mode_json, now_ms],
                )?;
            }
            for (idx, key) in project_order.iter().enumerate() {
                let _ = tx.execute(
                    "UPDATE host_projects SET sort_order = ?1 WHERE project_id = ?2",
                    params![idx as i64, key],
                );
            }
            tx.commit()?;
        }
        self.get_host_sync_state()
    }

    pub fn get_host_sync_state(&self) -> Result<HostSyncState, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT project_id, name, directories_json, sort_order, hidden, updated_at_ms
             FROM host_projects
             ORDER BY sort_order ASC, updated_at_ms DESC",
        )?;
        let mut max_updated: i64 = 0;
        let projects: Vec<HostProjectEntry> = stmt
            .query_map([], |row| {
                let dirs_str: String = row.get(2)?;
                let updated_at_ms: i64 = row.get(5)?;
                Ok(HostProjectEntry {
                    project_id: row.get(0)?,
                    name: row.get(1)?,
                    directories: serde_json::from_str(&dirs_str).unwrap_or_default(),
                    sort_order: row.get(3)?,
                    hidden: row.get::<_, i32>(4)? != 0,
                    updated_at_ms,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for p in &projects {
            max_updated = max_updated.max(p.updated_at_ms);
        }

        let order_row: Option<(String, i64)> = conn
            .query_row(
                "SELECT value_json, updated_at_ms FROM host_kv WHERE key = 'project_order'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let project_order: Vec<String> = match order_row {
            Some((raw, ts)) => {
                max_updated = max_updated.max(ts);
                serde_json::from_str(&raw).unwrap_or_default()
            }
            None => Vec::new(),
        };

        let mode_row: Option<(String, i64)> = conn
            .query_row(
                "SELECT value_json, updated_at_ms FROM host_kv WHERE key = 'project_sort_mode'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let project_sort_mode: Option<String> = match mode_row {
            Some((raw, ts)) => {
                max_updated = max_updated.max(ts);
                serde_json::from_str(&raw).ok()
            }
            None => None,
        };

        Ok(HostSyncState {
            projects,
            project_order,
            project_sort_mode,
            updated_at_ms: max_updated,
        })
    }

    // --- Client Local KV (global cross-host project order & sort prefs) ---

    pub fn get_client_kv(&self, key: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT value_json FROM client_kv WHERE key = ?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn set_client_kv(
        &self,
        key: &str,
        value_json: &str,
        updated_at_ms: i64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO client_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at_ms = excluded.updated_at_ms",
            params![key, value_json, updated_at_ms],
        )?;
        Ok(())
    }

    pub fn delete_client_kv(&self, key: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("DELETE FROM client_kv WHERE key = ?1", params![key])?;
        Ok(())
    }

    // --- Mission Workspace Shared Blackboard & Activity Events ---

    pub fn get_workspace_board(
        &self,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceBoardSnapshot>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT workspace_id, content, revision, updated_by, board_path, updated_at
             FROM workspace_boards WHERE workspace_id = ?1",
            params![workspace_id],
            |row| {
                let ts: String = row.get(5)?;
                Ok(WorkspaceBoardSnapshot {
                    workspace_id: row.get(0)?,
                    content: row.get(1)?,
                    revision: row.get::<_, i64>(2)?.max(0) as u64,
                    updated_by: row.get(3)?,
                    board_path: row.get(4)?,
                    updated_at: parse_ts(&ts),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn upsert_workspace_board(
        &self,
        workspace_id: &str,
        content: &str,
        updated_by: Option<&str>,
        board_path: Option<&str>,
    ) -> Result<WorkspaceBoardSnapshot, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let prev_rev: i64 = tx
            .query_row(
                "SELECT revision FROM workspace_boards WHERE workspace_id = ?1",
                params![workspace_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let next_rev = prev_rev.saturating_add(1);
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        tx.execute(
            "INSERT INTO workspace_boards (
                workspace_id, content, revision, updated_by, board_path, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(workspace_id) DO UPDATE SET
                content = excluded.content,
                revision = excluded.revision,
                updated_by = excluded.updated_by,
                board_path = COALESCE(excluded.board_path, workspace_boards.board_path),
                updated_at = excluded.updated_at",
            params![
                workspace_id,
                content,
                next_rev,
                updated_by,
                board_path,
                now_str
            ],
        )?;
        tx.execute(
            "UPDATE workspaces SET board_revision = ?1, updated_at = ?2 WHERE workspace_id = ?3",
            params![next_rev, now_str, workspace_id],
        )?;
        let final_path: Option<String> = tx
            .query_row(
                "SELECT board_path FROM workspace_boards WHERE workspace_id = ?1",
                params![workspace_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        tx.commit()?;
        Ok(WorkspaceBoardSnapshot {
            workspace_id: workspace_id.to_string(),
            content: content.to_string(),
            revision: next_rev as u64,
            updated_by: updated_by.map(ToOwned::to_owned),
            updated_at: now,
            board_path: final_path,
        })
    }

    pub fn append_workspace_event(
        &self,
        workspace_id: &str,
        agent_id: Option<&str>,
        agent_handle: Option<&str>,
        kind: WorkspaceEventKind,
        summary: &str,
        file_path: Option<&str>,
    ) -> Result<WorkspaceEventRecord, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now();
        let kind_str = workspace_event_kind_str(kind);
        tx.execute(
            "INSERT INTO workspace_events (
                workspace_id, agent_id, agent_handle, kind, summary, file_path, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                workspace_id,
                agent_id,
                agent_handle,
                kind_str,
                summary,
                file_path,
                now.to_rfc3339(),
            ],
        )?;
        let seq = tx.last_insert_rowid() as u64;
        // Keep bounded history per workspace (latest 200 events)
        tx.execute(
            "DELETE FROM workspace_events
             WHERE workspace_id = ?1
               AND seq NOT IN (
                   SELECT seq FROM workspace_events
                   WHERE workspace_id = ?1
                   ORDER BY seq DESC
                   LIMIT 200
               )",
            params![workspace_id],
        )?;
        tx.commit()?;
        Ok(WorkspaceEventRecord {
            seq,
            workspace_id: workspace_id.to_string(),
            agent_id: agent_id.map(ToOwned::to_owned),
            agent_handle: agent_handle.map(ToOwned::to_owned),
            kind,
            summary: summary.to_string(),
            file_path: file_path.map(ToOwned::to_owned),
            created_at: now,
        })
    }

    pub fn list_workspace_events(
        &self,
        workspace_id: &str,
        limit: usize,
    ) -> Result<Vec<WorkspaceEventRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, workspace_id, agent_id, agent_handle, kind, summary, file_path, created_at
             FROM workspace_events
             WHERE workspace_id = ?1
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let mut rows: Vec<WorkspaceEventRecord> = stmt
            .query_map(params![workspace_id, limit.clamp(1, 200) as i64], |row| {
                let kind_s: String = row.get(4)?;
                let ts_s: String = row.get(7)?;
                Ok(WorkspaceEventRecord {
                    seq: row.get::<_, i64>(0)? as u64,
                    workspace_id: row.get(1)?,
                    agent_id: row.get(2)?,
                    agent_handle: row.get(3)?,
                    kind: parse_workspace_event_kind(&kind_s),
                    summary: row.get(5)?,
                    file_path: row.get(6)?,
                    created_at: parse_ts(&ts_s),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.reverse();
        Ok(rows)
    }

    /// Return distinct peer `(agent_id, agent_handle)` pairs who edited `file_path` in `workspace_id`
    /// after `since` (excluding `exclude_agent_id`). Used by the live Conflict Radar.
    pub fn recent_workspace_file_editors(
        &self,
        workspace_id: &str,
        file_path: &str,
        exclude_agent_id: &str,
        since: DateTime<Utc>,
    ) -> Result<Vec<(String, Option<String>)>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT agent_id, agent_handle
             FROM workspace_events
             WHERE workspace_id = ?1
               AND kind = 'file_edited'
               AND file_path = ?2
               AND agent_id IS NOT NULL
               AND agent_id != ?3
               AND created_at >= ?4",
        )?;
        let rows = stmt.query_map(
            params![
                workspace_id,
                file_path,
                exclude_agent_id,
                since.to_rfc3339()
            ],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Return distinct recently edited file paths in `workspace_id` (used by auto-clustering).
    pub fn list_workspace_edited_files(
        &self,
        workspace_id: &str,
        limit: usize,
    ) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT DISTINCT file_path
             FROM workspace_events
             WHERE workspace_id = ?1
               AND kind = 'file_edited'
               AND file_path IS NOT NULL
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![workspace_id, limit as i64], |r| {
            r.get::<_, String>(0)
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

pub(crate) fn attention_state_from_str(raw: &str) -> AttentionState {
    match raw {
        "finished" => AttentionState::Finished,
        "error" => AttentionState::Error,
        "permission" => AttentionState::Permission,
        "manual_unread" => AttentionState::ManualUnread,
        _ => AttentionState::None,
    }
}

fn workspace_event_kind_str(kind: WorkspaceEventKind) -> &'static str {
    match kind {
        WorkspaceEventKind::PeerJoined => "peer_joined",
        WorkspaceEventKind::PeerFinished => "peer_finished",
        WorkspaceEventKind::BoardUpdated => "board_updated",
        WorkspaceEventKind::PeerMessage => "peer_message",
        WorkspaceEventKind::FileEdited => "file_edited",
        WorkspaceEventKind::ConflictWarning => "conflict_warning",
        WorkspaceEventKind::WorkspaceMerged => "workspace_merged",
    }
}

fn parse_workspace_event_kind(raw: &str) -> WorkspaceEventKind {
    match raw {
        "peer_joined" => WorkspaceEventKind::PeerJoined,
        "peer_finished" => WorkspaceEventKind::PeerFinished,
        "board_updated" => WorkspaceEventKind::BoardUpdated,
        "peer_message" => WorkspaceEventKind::PeerMessage,
        "file_edited" => WorkspaceEventKind::FileEdited,
        "conflict_warning" => WorkspaceEventKind::ConflictWarning,
        "workspace_merged" => WorkspaceEventKind::WorkspaceMerged,
        _ => WorkspaceEventKind::PeerMessage,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, Utc};
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, HostProjectEntry, ProviderKind, SessionMode,
        WorkspaceClusterSuggestion, WorkspaceEventKind, WorkspaceSetupStatus, WorkspaceSummary,
    };

    use crate::OrbitStore;

    fn sample_ws(id: &str, title: &str, cwd: &str) -> WorkspaceSummary {
        WorkspaceSummary {
            workspace_id: id.into(),
            title: title.into(),
            cwd: cwd.into(),
            git_branch: Some("main".into()),
            is_worktree: false,
            updated_at: Utc::now(),
            source_cwd: None,
            base_ref: None,
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: WorkspaceSetupStatus::Idle,
            mission_goal: Some("Sync across devices".into()),
            project_root: Some(cwd.into()),
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: true,
        }
    }

    fn sample_agent(id: &str, ws_id: &str, cwd: &str, latest_seq: u64) -> AgentSummary {
        let now = Utc::now();
        AgentSummary {
            agent_id: id.into(),
            workspace_id: ws_id.into(),
            cwd: cwd.into(),
            title: format!("Agent {id}"),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::Finished,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: Some("lead".into()),
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
        }
    }

    #[test]
    fn read_seq_monotonic_manual_unread_and_pin_sync() {
        let store = OrbitStore::open_in_memory().expect("open");
        store
            .upsert_workspace(&sample_ws("ws_1", "Sync", "/repo"))
            .expect("ws");
        store
            .upsert_agent(&sample_agent("ag_1", "ws_1", "/repo", 42), None)
            .expect("ag");

        // Mark read to 42 clears Finished attention and advances read_seq
        let after_read = store
            .mark_agent_read_seq("ag_1", Some(42))
            .expect("read")
            .expect("agent");
        assert_eq!(after_read.read_seq, 42);
        assert_eq!(after_read.attention, AttentionState::None);

        // Stale lower read_seq must NEVER regress read_seq and must report changed=false
        let (after_stale, stale_changed) = store
            .mark_agent_read_seq_with_changed("ag_1", Some(10))
            .expect("stale")
            .expect("agent");
        assert_eq!(after_stale.read_seq, 42);
        assert!(!stale_changed);

        // Idempotent same read_seq with AttentionState::None must also report changed=false
        let (after_same, same_changed) = store
            .mark_agent_read_seq_with_changed("ag_1", Some(42))
            .expect("same")
            .expect("agent");
        assert_eq!(after_same.read_seq, 42);
        assert!(!same_changed);

        // Manual unread preserves read_seq=42 and sets AttentionState::ManualUnread
        let after_unread = store
            .mark_agent_unread("ag_1")
            .expect("unread")
            .expect("agent");
        assert_eq!(after_unread.read_seq, 42);
        assert_eq!(after_unread.attention, AttentionState::ManualUnread);

        // Reading again clears ManualUnread
        let cleared = store
            .mark_agent_read_seq("ag_1", Some(42))
            .expect("clear")
            .expect("agent");
        assert_eq!(cleared.attention, AttentionState::None);

        // Pin sync
        let pinned = store
            .set_agent_pinned("ag_1", true, Some(999))
            .expect("pin")
            .expect("agent");
        assert!(pinned.is_pinned);
        assert_eq!(pinned.pinned_order, 999);
    }

    #[test]
    fn host_projects_order_workspace_board_events_and_merge() {
        let store = OrbitStore::open_in_memory().expect("open");
        store
            .upsert_host_project(&HostProjectEntry {
                project_id: "p_a".into(),
                name: "Alpha".into(),
                directories: vec!["/alpha".into()],
                sort_order: 0,
                hidden: false,
                updated_at_ms: 100,
            })
            .expect("p_a");
        store
            .upsert_host_project(&HostProjectEntry {
                project_id: "p_b".into(),
                name: "Beta".into(),
                directories: vec!["/beta".into()],
                sort_order: 1,
                hidden: false,
                updated_at_ms: 200,
            })
            .expect("p_b");
        let sync = store
            .set_host_project_order(&["p_b".into(), "p_a".into()], Some("manual"))
            .expect("order");
        assert_eq!(sync.project_order, vec!["p_b", "p_a"]);
        assert_eq!(sync.project_sort_mode.as_deref(), Some("manual"));
        assert_eq!(sync.projects[0].project_id, "p_b");

        // Workspace board + events + conflict editors + merge
        store
            .upsert_workspace(&sample_ws("ws_src", "Source", "/repo"))
            .expect("ws_src");
        store
            .upsert_workspace(&sample_ws("ws_dst", "Target", "/repo"))
            .expect("ws_dst");
        store
            .upsert_agent(&sample_agent("ag_a", "ws_src", "/repo", 5), None)
            .expect("ag_a");
        store
            .upsert_agent(&sample_agent("ag_b", "ws_dst", "/repo", 8), None)
            .expect("ag_b");

        let b1 = store
            .upsert_workspace_board(
                "ws_dst",
                "## Goal\nShip v0.6",
                Some("lead"),
                Some("/tmp/BOARD.md"),
            )
            .expect("board1");
        assert_eq!(b1.revision, 1);
        let b2 = store
            .upsert_workspace_board(
                "ws_dst",
                "## Goal\nShip v0.6\n## Done",
                Some("reviewer"),
                None,
            )
            .expect("board2");
        assert_eq!(b2.revision, 2);
        assert_eq!(b2.board_path.as_deref(), Some("/tmp/BOARD.md"));

        store
            .append_workspace_event(
                "ws_dst",
                Some("ag_b"),
                Some("backend"),
                WorkspaceEventKind::FileEdited,
                "Edited src/lib.rs",
                Some("src/lib.rs"),
            )
            .expect("ev1");
        let editors = store
            .recent_workspace_file_editors(
                "ws_dst",
                "src/lib.rs",
                "ag_a",
                Utc::now() - Duration::minutes(5),
            )
            .expect("editors");
        assert_eq!(editors.len(), 1);
        assert_eq!(editors[0].0, "ag_b");

        store
            .set_agent_cluster_suggestion(
                "ag_a",
                Some(&WorkspaceClusterSuggestion {
                    target_workspace_id: "ws_dst".into(),
                    target_workspace_title: "Target".into(),
                    confidence: 0.88,
                    reason: "Shared files".into(),
                    auto_applied: false,
                    previous_workspace_id: None,
                }),
            )
            .expect("suggest");
        let moved = store.merge_workspaces("ws_src", "ws_dst").expect("merge");
        assert_eq!(moved, 1);
        let ag_a = store.get_agent("ag_a").expect("get").expect("some");
        assert_eq!(ag_a.workspace_id, "ws_dst");
        assert!(ag_a.cluster_suggestion.is_none());
        let src_ws = store.get_workspace("ws_src").expect("get").expect("some");
        assert!(src_ws.archived_at.is_some());
    }
}
