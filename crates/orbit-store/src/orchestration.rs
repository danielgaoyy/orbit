//! Orchestration persistence layer inside `OrbitStore` (SQLite WAL):
//! - Durable finish subscriptions (cross-restart & cross-host)
//! - Host-scoped Agent Profiles
//! - Unified Cron Schedules & Heartbeats + Run Logs
//! - Daemon-to-Daemon Connected Hosts registry
//! - Idempotency request receipts
//! - Provider-native subagent descriptors & read-only timelines

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentProfile, AgentStatus, AgentSummary, AttentionState, McpToolPolicy,
    ProviderSubagentDescriptor, ProviderSubagentEvent, ProviderSubagentStatus, ReasoningEffort,
    ScheduleKind, ScheduleRunRecord, ScheduleRunStatus, ScheduleSpec, ScheduleStatus,
    ScheduleTarget, SessionMode, TimelinePayload, WorkspaceSetupStatus, WorkspaceSummary,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{OrbitStore, StoreError, map_agent_row, map_workspace_row, parse_ts};

/// Persisted finish/permission observation intent for an orchestrating caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFinishSubscription {
    pub id: String,
    pub caller_agent_id: String,
    pub child_agent_id: String,
    pub target_host_id: Option<String>,
    pub require_parent_owner: bool,
    pub delivered_perm_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
}

/// Persisted outbound Connected Host entry owned by a Daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredConnectedHost {
    pub registration_id: String,
    pub host_id: Option<String>,
    pub label: String,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Result of checking or reserving an idempotency key in `request_receipts`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptOutcome {
    /// Key was absent and has now been atomically reserved as `pending`.
    Reserved,
    /// Identical request already committed; contains serialized result JSON.
    Committed(String),
    /// Identical request is still pending (in flight or interrupted before commit).
    PendingInFlight,
    /// Same key was reused with different request parameters.
    Conflict,
}

pub(crate) fn migrate_orchestration_schema(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    // Non-destructive column additions for pre-existing v1/v2 databases
    for stmt in [
        "ALTER TABLE workspaces ADD COLUMN source_cwd TEXT",
        "ALTER TABLE workspaces ADD COLUMN base_ref TEXT",
        "ALTER TABLE workspaces ADD COLUMN parent_workspace_id TEXT",
        "ALTER TABLE workspaces ADD COLUMN archived_at TEXT",
        "ALTER TABLE workspaces ADD COLUMN background INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE workspaces ADD COLUMN setup_status TEXT NOT NULL DEFAULT 'idle'",
        "ALTER TABLE agents ADD COLUMN parent_host_id TEXT",
        "ALTER TABLE agents ADD COLUMN archived_at TEXT",
        "ALTER TABLE agents ADD COLUMN background INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN auto_archive INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE agents ADD COLUMN profile_id TEXT",
        "ALTER TABLE agents ADD COLUMN extra_dirs_json TEXT NOT NULL DEFAULT '[]'",
        "ALTER TABLE agents ADD COLUMN labels_json TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE agents ADD COLUMN attention_state TEXT NOT NULL DEFAULT 'none'",
        "ALTER TABLE agents ADD COLUMN last_activity_at TEXT",
    ] {
        let _ = conn.execute(stmt, []);
    }

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS finish_subscriptions (
            id                      TEXT PRIMARY KEY,
            caller_agent_id         TEXT NOT NULL,
            child_agent_id          TEXT NOT NULL,
            target_host_id          TEXT,
            require_parent_owner    INTEGER NOT NULL DEFAULT 1,
            delivered_perm_ids_json TEXT NOT NULL DEFAULT '[]',
            created_at              TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_finsub_pair
            ON finish_subscriptions(caller_agent_id, child_agent_id, target_host_id);

        CREATE TABLE IF NOT EXISTS agent_profiles (
            profile_id           TEXT PRIMARY KEY,
            name                 TEXT NOT NULL UNIQUE,
            provider_json        TEXT NOT NULL,
            model                TEXT,
            effort_json          TEXT,
            mode_json            TEXT NOT NULL,
            when_to_use          TEXT NOT NULL DEFAULT '',
            system_prompt_append TEXT,
            mcp_policy_json      TEXT NOT NULL DEFAULT '{}',
            created_at           TEXT NOT NULL,
            updated_at           TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS schedules (
            schedule_id      TEXT PRIMARY KEY,
            kind             TEXT NOT NULL,
            name             TEXT NOT NULL,
            cron_expr        TEXT NOT NULL,
            timezone         TEXT,
            prompt           TEXT NOT NULL,
            status           TEXT NOT NULL,
            target_json      TEXT NOT NULL,
            max_runs         INTEGER,
            run_count        INTEGER NOT NULL DEFAULT 0,
            expires_at       TEXT,
            last_run_at      TEXT,
            next_run_at      TEXT,
            created_by_agent TEXT,
            created_at       TEXT NOT NULL,
            updated_at       TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS schedule_runs (
            run_id       TEXT PRIMARY KEY,
            schedule_id  TEXT NOT NULL REFERENCES schedules(schedule_id) ON DELETE CASCADE,
            agent_id     TEXT,
            workspace_id TEXT,
            status       TEXT NOT NULL,
            summary      TEXT,
            error        TEXT,
            started_at   TEXT NOT NULL,
            finished_at  TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_schedule_runs_sid
            ON schedule_runs(schedule_id, started_at DESC);

        CREATE TABLE IF NOT EXISTS connected_hosts (
            registration_id       TEXT PRIMARY KEY,
            host_id               TEXT,
            label                 TEXT NOT NULL,
            daemon_pubkey_b64     TEXT NOT NULL,
            direct_endpoints_json TEXT NOT NULL DEFAULT '[]',
            relay_url             TEXT,
            enabled               INTEGER NOT NULL DEFAULT 1,
            created_at            TEXT NOT NULL,
            updated_at            TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS request_receipts (
            scope           TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            request_hash    TEXT NOT NULL,
            status          TEXT NOT NULL,
            result_json     TEXT,
            created_at      TEXT NOT NULL,
            PRIMARY KEY (scope, idempotency_key)
        );

        CREATE TABLE IF NOT EXISTS provider_subagents (
            parent_agent_id    TEXT NOT NULL REFERENCES agents(agent_id) ON DELETE CASCADE,
            subagent_id        TEXT NOT NULL,
            parent_subagent_id TEXT,
            title              TEXT NOT NULL,
            subtitle           TEXT,
            status_json        TEXT NOT NULL,
            created_at         TEXT NOT NULL,
            updated_at         TEXT NOT NULL,
            PRIMARY KEY (parent_agent_id, subagent_id)
        );

        CREATE TABLE IF NOT EXISTS provider_subagent_events (
            seq             INTEGER PRIMARY KEY AUTOINCREMENT,
            parent_agent_id TEXT NOT NULL,
            subagent_id     TEXT NOT NULL,
            timestamp       TEXT NOT NULL,
            payload_json    TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_psub_events
            ON provider_subagent_events(parent_agent_id, subagent_id, seq);
        ",
    )?;
    let cur_ver: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap_or(0);
    if cur_ver < 3 {
        conn.execute_batch("PRAGMA user_version = 3;")?;
    }
    Ok(())
}

impl OrbitStore {
    // --- Extended Workspace Operations ---

    pub fn get_workspace(
        &self,
        workspace_id: &str,
    ) -> Result<Option<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {} FROM workspaces WHERE workspace_id = ?1",
            crate::WORKSPACE_SELECT_COLS
        );
        conn.query_row(&sql, params![workspace_id], map_workspace_row)
            .optional()
            .map_err(Into::into)
    }

    pub fn list_workspaces_filtered(
        &self,
        include_archived: bool,
        include_background: bool,
    ) -> Result<Vec<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut clauses = Vec::new();
        if !include_archived {
            clauses.push("archived_at IS NULL");
        }
        if !include_background {
            clauses.push("background = 0");
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT {} FROM workspaces {where_sql} ORDER BY updated_at DESC",
            crate::WORKSPACE_SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_workspace_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn rename_workspace(&self, workspace_id: &str, title: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE workspaces SET title = ?1, title_locked = 1, is_curated = 1, updated_at = ?2 WHERE workspace_id = ?3",
            params![title, Utc::now().to_rfc3339(), workspace_id],
        )?;
        Ok(())
    }

    pub fn set_workspace_setup_status(
        &self,
        workspace_id: &str,
        status: WorkspaceSetupStatus,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let status_str = serde_json::to_string(&status)?
            .trim_matches('"')
            .to_string();
        conn.execute(
            "UPDATE workspaces SET setup_status = ?1, updated_at = ?2 WHERE workspace_id = ?3",
            params![status_str, Utc::now().to_rfc3339(), workspace_id],
        )?;
        Ok(())
    }

    pub fn archive_workspace_soft(&self, workspace_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE workspaces SET archived_at = ?1, updated_at = ?1 WHERE workspace_id = ?2",
            params![now, workspace_id],
        )?;
        Ok(())
    }

    pub fn unarchive_workspace(&self, workspace_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE workspaces SET archived_at = NULL, updated_at = ?1 WHERE workspace_id = ?2",
            params![Utc::now().to_rfc3339(), workspace_id],
        )?;
        Ok(())
    }

    pub fn count_active_workspaces_for_cwd(&self, cwd: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM workspaces WHERE cwd = ?1 AND archived_at IS NULL",
            params![cwd],
            |r| r.get(0),
        )?;
        Ok(count.max(0) as usize)
    }

    // --- Extended Agent Operations ---

    pub fn set_agent_extra_dirs(
        &self,
        agent_id: &str,
        extra_dirs: &[String],
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let json = serde_json::to_string(extra_dirs)?;
        conn.execute(
            "UPDATE agents SET extra_dirs_json = ?1 WHERE agent_id = ?2",
            params![json, agent_id],
        )?;
        Ok(())
    }

    pub fn get_agent_extra_dirs(&self, agent_id: &str) -> Result<Vec<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT extra_dirs_json FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw
            .as_deref()
            .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            .unwrap_or_default())
    }

    pub fn list_agents_filtered(
        &self,
        workspace_id: Option<&str>,
        include_archived: bool,
        include_background: bool,
        parent_agent_id: Option<&str>,
    ) -> Result<Vec<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut clauses: Vec<String> = Vec::new();
        let mut bound: Vec<String> = Vec::new();
        if let Some(ws) = workspace_id {
            bound.push(ws.to_string());
            clauses.push(format!("workspace_id = ?{}", bound.len()));
        }
        if let Some(parent) = parent_agent_id {
            bound.push(parent.to_string());
            clauses.push(format!("parent_agent_id = ?{}", bound.len()));
        }
        if !include_archived {
            clauses.push("archived_at IS NULL".into());
        }
        if !include_background {
            clauses.push("background = 0".into());
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        let sql = format!(
            "SELECT {} FROM agents {where_sql} ORDER BY updated_at DESC",
            crate::AGENT_SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let mut rows = stmt
            .query_map(params_refs.as_slice(), map_agent_row)?
            .collect::<Result<Vec<_>, _>>()?;
        if !rows.is_empty()
            && let Ok(mut grouped) = crate::load_all_queued_prompts_grouped(&conn)
            && !grouped.is_empty()
        {
            for ag in &mut rows {
                if let Some(items) = grouped.remove(&ag.agent_id) {
                    ag.queued_prompts = items;
                }
            }
        }
        Ok(rows)
    }

    pub fn list_child_agents(
        &self,
        parent_agent_id: &str,
    ) -> Result<Vec<AgentSummary>, StoreError> {
        self.list_agents_filtered(None, false, true, Some(parent_agent_id))
    }

    pub fn archive_agent_soft(&self, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let now = Utc::now().to_rfc3339();
        let closed_json = serde_json::to_string(&AgentStatus::Closed)?;
        let current_status: Option<String> = conn
            .query_row(
                "SELECT status_json FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        let normalized_status = match current_status
            .as_deref()
            .and_then(|s| serde_json::from_str::<AgentStatus>(s).ok())
        {
            Some(
                AgentStatus::Running { .. }
                | AgentStatus::Initializing
                | AgentStatus::AutonomousWakeup { .. }
                | AgentStatus::AwaitingPermission { .. },
            ) => closed_json,
            _ => current_status.unwrap_or(closed_json),
        };
        conn.execute(
            "UPDATE agents
             SET archived_at = ?1, status_json = ?2, attention_state = 'none', updated_at = ?1
             WHERE agent_id = ?3",
            params![now, normalized_status, agent_id],
        )?;
        let _ = conn.execute(
            "DELETE FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
        );
        Ok(())
    }

    pub fn unarchive_agent(&self, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE agents SET archived_at = NULL, updated_at = ?1 WHERE agent_id = ?2",
            params![Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn detach_agent(&self, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let labels_raw: Option<String> = conn
            .query_row(
                "SELECT labels_json FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        let mut labels: HashMap<String, String> = labels_raw
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        labels.retain(|k, _| !k.starts_with("orbit.open_tab."));
        let labels_json = serde_json::to_string(&labels)?;
        conn.execute(
            "UPDATE agents
             SET parent_agent_id = NULL, parent_host_id = NULL, labels_json = ?1, updated_at = ?2
             WHERE agent_id = ?3",
            params![labels_json, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn set_agent_attention(
        &self,
        agent_id: &str,
        attention: AttentionState,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let state_str = serde_json::to_string(&attention)?
            .trim_matches('"')
            .to_string();
        conn.execute(
            "UPDATE agents SET attention_state = ?1 WHERE agent_id = ?2",
            params![state_str, agent_id],
        )?;
        Ok(())
    }

    pub fn touch_agent_activity(
        &self,
        agent_id: &str,
        at: DateTime<Utc>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE agents SET last_activity_at = ?1 WHERE agent_id = ?2",
            params![at.to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn update_agent_status(
        &self,
        agent_id: &str,
        status: &AgentStatus,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let status_json = serde_json::to_string(status)?;
        conn.execute(
            "UPDATE agents SET status_json = ?1, updated_at = ?2 WHERE agent_id = ?3",
            params![status_json, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent_fields(
        &self,
        agent_id: &str,
        title: Option<&str>,
        model: Option<Option<&str>>,
        effort: Option<Option<ReasoningEffort>>,
        mode: Option<&SessionMode>,
        labels: Option<&HashMap<String, String>>,
        handle: Option<&str>,
        task_summary: Option<&str>,
        fast_mode: Option<bool>,
    ) -> Result<(), StoreError> {
        use rusqlite::types::Value as SqlValue;

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;

        let mut set_clauses: Vec<String> = Vec::new();
        let mut bind_vals: Vec<SqlValue> = Vec::new();
        let mut push_field = |col: &'static str, val: SqlValue| {
            bind_vals.push(val);
            set_clauses.push(format!("{col} = ?{}", bind_vals.len()));
        };

        if let Some(t) = title {
            push_field("title", SqlValue::Text(t.to_string()));
        }
        if let Some(m) = model {
            push_field(
                "model",
                match m {
                    Some(s) => SqlValue::Text(s.to_string()),
                    None => SqlValue::Null,
                },
            );
        }
        if let Some(e) = effort {
            let eff_val = match e {
                Some(eff) => SqlValue::Text(serde_json::to_string(&eff)?),
                None => SqlValue::Null,
            };
            push_field("effort_json", eff_val);
        }
        if let Some(md) = mode {
            push_field("mode_json", SqlValue::Text(serde_json::to_string(md)?));
        }
        if let Some(lbls) = labels {
            let existing_raw: Option<String> = tx
                .query_row(
                    "SELECT labels_json FROM agents WHERE agent_id = ?1",
                    params![agent_id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(raw_json) = existing_raw else {
                tx.commit()?;
                return Ok(());
            };
            let mut merged: HashMap<String, String> =
                serde_json::from_str(&raw_json).unwrap_or_default();
            for (k, v) in lbls {
                if v.is_empty() {
                    merged.remove(k);
                } else {
                    merged.insert(k.clone(), v.clone());
                }
            }
            push_field(
                "labels_json",
                SqlValue::Text(serde_json::to_string(&merged)?),
            );
        }
        if let Some(h) = handle {
            let trimmed = h.trim();
            push_field(
                "handle",
                if trimmed.is_empty() {
                    SqlValue::Null
                } else {
                    SqlValue::Text(trimmed.to_string())
                },
            );
        }
        if let Some(ts) = task_summary {
            let trimmed = ts.trim();
            push_field(
                "task_summary",
                if trimmed.is_empty() {
                    SqlValue::Null
                } else {
                    SqlValue::Text(trimmed.to_string())
                },
            );
        }
        if let Some(fm) = fast_mode {
            push_field("fast_mode", SqlValue::Integer(i64::from(fm)));
        }

        if set_clauses.is_empty() {
            tx.commit()?;
            return Ok(());
        }

        bind_vals.push(SqlValue::Text(Utc::now().to_rfc3339()));
        set_clauses.push(format!("updated_at = ?{}", bind_vals.len()));
        bind_vals.push(SqlValue::Text(agent_id.to_string()));
        let sql = format!(
            "UPDATE agents SET {} WHERE agent_id = ?{}",
            set_clauses.join(", "),
            bind_vals.len()
        );
        tx.execute(&sql, rusqlite::params_from_iter(bind_vals))?;
        tx.commit()?;
        Ok(())
    }

    /// Return the most recent assistant message text for finish-notification summaries without loading a provider.
    pub fn last_assistant_message_text(
        &self,
        agent_id: &str,
    ) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT payload_json FROM timeline_events
             WHERE agent_id = ?1 AND payload_json LIKE '%\"type\":\"assistant_message\"%'
             ORDER BY seq DESC LIMIT 1",
        )?;
        let raw: Option<String> = stmt.query_row(params![agent_id], |r| r.get(0)).optional()?;
        if let Some(json) = raw
            && let Ok(TimelinePayload::AssistantMessage { text, .. }) =
                serde_json::from_str::<TimelinePayload>(&json)
        {
            return Ok(Some(text));
        }
        Ok(None)
    }

    // --- Finish Subscriptions ---

    pub fn upsert_finish_subscription(
        &self,
        sub: &StoredFinishSubscription,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let perms_json = serde_json::to_string(&sub.delivered_perm_ids)?;
        conn.execute(
            "INSERT INTO finish_subscriptions (
                id, caller_agent_id, child_agent_id, target_host_id,
                require_parent_owner, delivered_perm_ids_json, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                require_parent_owner = excluded.require_parent_owner,
                delivered_perm_ids_json = excluded.delivered_perm_ids_json",
            params![
                sub.id,
                sub.caller_agent_id,
                sub.child_agent_id,
                sub.target_host_id,
                i32::from(sub.require_parent_owner),
                perms_json,
                sub.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_finish_subscriptions(&self) -> Result<Vec<StoredFinishSubscription>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, caller_agent_id, child_agent_id, target_host_id,
                    require_parent_owner, delivered_perm_ids_json, created_at
             FROM finish_subscriptions ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let perms_str: String = row.get(5)?;
            let ts: String = row.get(6)?;
            Ok(StoredFinishSubscription {
                id: row.get(0)?,
                caller_agent_id: row.get(1)?,
                child_agent_id: row.get(2)?,
                target_host_id: row.get(3)?,
                require_parent_owner: row.get::<_, i32>(4)? != 0,
                delivered_perm_ids: serde_json::from_str(&perms_str).unwrap_or_default(),
                created_at: parse_ts(&ts),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn mark_finish_subscription_perm_delivered(
        &self,
        sub_id: &str,
        perm_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT delivered_perm_ids_json FROM finish_subscriptions WHERE id = ?1",
                params![sub_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(s) = raw {
            let mut ids: Vec<String> = serde_json::from_str(&s).unwrap_or_default();
            if !ids.iter().any(|x| x == perm_id) {
                ids.push(perm_id.to_string());
                let json = serde_json::to_string(&ids)?;
                conn.execute(
                    "UPDATE finish_subscriptions SET delivered_perm_ids_json = ?1 WHERE id = ?2",
                    params![json, sub_id],
                )?;
            }
        }
        Ok(())
    }

    pub fn delete_finish_subscription(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM finish_subscriptions WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn delete_finish_subscriptions_for_caller(
        &self,
        caller_agent_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM finish_subscriptions WHERE caller_agent_id = ?1",
            params![caller_agent_id],
        )?;
        Ok(())
    }

    pub fn has_finish_subscriptions_for_caller(
        &self,
        caller_agent_id: &str,
    ) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM finish_subscriptions WHERE caller_agent_id = ?1",
            params![caller_agent_id],
            |r| r.get(0),
        )?;
        Ok(count > 0)
    }

    // --- Agent Profiles ---

    pub fn upsert_profile(&self, profile: &AgentProfile) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let provider_json = serde_json::to_string(&profile.provider)?;
        let effort_json = profile
            .effort
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let mode_json = serde_json::to_string(&profile.mode)?;
        let mcp_policy_json = serde_json::to_string(&profile.mcp_policy)?;
        conn.execute(
            "INSERT INTO agent_profiles (
                profile_id, name, provider_json, model, effort_json, mode_json,
                when_to_use, system_prompt_append, mcp_policy_json, created_at, updated_at,
                fast_mode, acp_agent_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(profile_id) DO UPDATE SET
                name = excluded.name,
                provider_json = excluded.provider_json,
                model = excluded.model,
                effort_json = excluded.effort_json,
                mode_json = excluded.mode_json,
                when_to_use = excluded.when_to_use,
                system_prompt_append = excluded.system_prompt_append,
                mcp_policy_json = excluded.mcp_policy_json,
                updated_at = excluded.updated_at,
                fast_mode = excluded.fast_mode,
                acp_agent_id = excluded.acp_agent_id",
            params![
                profile.profile_id,
                profile.name,
                provider_json,
                profile.model,
                effort_json,
                mode_json,
                profile.when_to_use,
                profile.system_prompt_append,
                mcp_policy_json,
                profile.created_at.to_rfc3339(),
                profile.updated_at.to_rfc3339(),
                i32::from(profile.fast_mode),
                profile.acp_agent_id,
            ],
        )?;
        Ok(())
    }

    pub fn list_profiles(&self) -> Result<Vec<AgentProfile>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT profile_id, name, provider_json, model, effort_json, mode_json,
                    when_to_use, system_prompt_append, mcp_policy_json, created_at, updated_at,
                    fast_mode, acp_agent_id
             FROM agent_profiles ORDER BY name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], map_profile_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_profile(&self, profile_id: &str) -> Result<Option<AgentProfile>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT profile_id, name, provider_json, model, effort_json, mode_json,
                    when_to_use, system_prompt_append, mcp_policy_json, created_at, updated_at,
                    fast_mode, acp_agent_id
             FROM agent_profiles WHERE profile_id = ?1",
            params![profile_id],
            map_profile_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_profile_by_name(&self, name: &str) -> Result<Option<AgentProfile>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT profile_id, name, provider_json, model, effort_json, mode_json,
                    when_to_use, system_prompt_append, mcp_policy_json, created_at, updated_at,
                    fast_mode, acp_agent_id
             FROM agent_profiles WHERE LOWER(name) = LOWER(?1)",
            params![name],
            map_profile_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn delete_profile(&self, profile_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM agent_profiles WHERE profile_id = ?1",
            params![profile_id],
        )?;
        Ok(())
    }

    // --- Schedules & Heartbeats ---

    pub fn upsert_schedule(&self, sched: &ScheduleSpec) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let kind_str = match sched.kind {
            ScheduleKind::Schedule => "schedule",
            ScheduleKind::Heartbeat => "heartbeat",
        };
        let status_str = match sched.status {
            ScheduleStatus::Active => "active",
            ScheduleStatus::Paused => "paused",
            ScheduleStatus::Completed => "completed",
        };
        let target_json = serde_json::to_string(&sched.target)?;
        conn.execute(
            "INSERT INTO schedules (
                schedule_id, kind, name, cron_expr, timezone, prompt, status, target_json,
                max_runs, run_count, expires_at, last_run_at, next_run_at,
                created_by_agent, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
             ON CONFLICT(schedule_id) DO UPDATE SET
                name = excluded.name,
                cron_expr = excluded.cron_expr,
                timezone = excluded.timezone,
                prompt = excluded.prompt,
                status = excluded.status,
                target_json = excluded.target_json,
                max_runs = excluded.max_runs,
                run_count = excluded.run_count,
                expires_at = excluded.expires_at,
                last_run_at = excluded.last_run_at,
                next_run_at = excluded.next_run_at,
                updated_at = excluded.updated_at",
            params![
                sched.schedule_id,
                kind_str,
                sched.name,
                sched.cron_expr,
                sched.timezone,
                sched.prompt,
                status_str,
                target_json,
                sched.max_runs.map(i64::from),
                i64::from(sched.run_count),
                sched.expires_at.map(|t| t.to_rfc3339()),
                sched.last_run_at.map(|t| t.to_rfc3339()),
                sched.next_run_at.map(|t| t.to_rfc3339()),
                sched.created_by_agent,
                sched.created_at.to_rfc3339(),
                sched.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_schedule(&self, schedule_id: &str) -> Result<Option<ScheduleSpec>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT schedule_id, kind, name, cron_expr, timezone, prompt, status, target_json,
                    max_runs, run_count, expires_at, last_run_at, next_run_at,
                    created_by_agent, created_at, updated_at
             FROM schedules WHERE schedule_id = ?1",
            params![schedule_id],
            map_schedule_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_schedules(&self, include_completed: bool) -> Result<Vec<ScheduleSpec>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = if include_completed {
            "SELECT schedule_id, kind, name, cron_expr, timezone, prompt, status, target_json,
                    max_runs, run_count, expires_at, last_run_at, next_run_at,
                    created_by_agent, created_at, updated_at
             FROM schedules ORDER BY created_at DESC"
        } else {
            "SELECT schedule_id, kind, name, cron_expr, timezone, prompt, status, target_json,
                    max_runs, run_count, expires_at, last_run_at, next_run_at,
                    created_by_agent, created_at, updated_at
             FROM schedules WHERE status != 'completed' ORDER BY created_at DESC"
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], map_schedule_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_schedule(&self, schedule_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM schedules WHERE schedule_id = ?1",
            params![schedule_id],
        )?;
        Ok(())
    }

    pub fn delete_heartbeats_for_agent(&self, agent_id: &str) -> Result<Vec<String>, StoreError> {
        let all = self.list_schedules(true)?;
        let mut removed = Vec::new();
        for s in all {
            if s.kind == ScheduleKind::Heartbeat
                && matches!(&s.target, ScheduleTarget::ExistingAgent { agent_id: aid } if aid == agent_id)
            {
                self.delete_schedule(&s.schedule_id)?;
                removed.push(s.schedule_id);
            }
        }
        Ok(removed)
    }

    pub fn has_active_heartbeats_for_agent(&self, agent_id: &str) -> Result<bool, StoreError> {
        let active = self.list_schedules(false)?;
        Ok(active.iter().any(|s| {
            s.kind == ScheduleKind::Heartbeat
                && s.status == ScheduleStatus::Active
                && matches!(&s.target, ScheduleTarget::ExistingAgent { agent_id: aid } if aid == agent_id)
        }))
    }

    pub fn insert_schedule_run(&self, run: &ScheduleRunRecord) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let status_str = schedule_run_status_str(run.status);
        conn.execute(
            "INSERT INTO schedule_runs (
                run_id, schedule_id, agent_id, workspace_id, status,
                summary, error, started_at, finished_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                run.run_id,
                run.schedule_id,
                run.agent_id,
                run.workspace_id,
                status_str,
                run.summary,
                run.error,
                run.started_at.to_rfc3339(),
                run.finished_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn finish_schedule_run(
        &self,
        run_id: &str,
        status: ScheduleRunStatus,
        agent_id: Option<&str>,
        workspace_id: Option<&str>,
        summary: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let status_str = schedule_run_status_str(status);
        conn.execute(
            "UPDATE schedule_runs
             SET status = ?1,
                 agent_id = COALESCE(?2, agent_id),
                 workspace_id = COALESCE(?3, workspace_id),
                 summary = COALESCE(?4, summary),
                 error = COALESCE(?5, error),
                 finished_at = ?6
             WHERE run_id = ?7",
            params![
                status_str,
                agent_id,
                workspace_id,
                summary,
                error,
                Utc::now().to_rfc3339(),
                run_id,
            ],
        )?;
        Ok(())
    }

    pub fn list_schedule_runs(
        &self,
        schedule_id: &str,
        limit: usize,
    ) -> Result<Vec<ScheduleRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT run_id, schedule_id, agent_id, workspace_id, status,
                    summary, error, started_at, finished_at
             FROM schedule_runs
             WHERE schedule_id = ?1
             ORDER BY started_at DESC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![schedule_id, limit as i64], |row| {
            let status_s: String = row.get(4)?;
            let started_s: String = row.get(7)?;
            let finished_opt: Option<String> = row.get(8)?;
            Ok(ScheduleRunRecord {
                run_id: row.get(0)?,
                schedule_id: row.get(1)?,
                agent_id: row.get(2)?,
                workspace_id: row.get(3)?,
                status: parse_schedule_run_status(&status_s),
                summary: row.get(5)?,
                error: row.get(6)?,
                started_at: parse_ts(&started_s),
                finished_at: finished_opt.as_deref().map(parse_ts),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // --- Daemon-to-Daemon Connected Hosts ---

    pub fn upsert_connected_host(&self, host: &StoredConnectedHost) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let endpoints_json = serde_json::to_string(&host.direct_endpoints)?;
        conn.execute(
            "INSERT INTO connected_hosts (
                registration_id, host_id, label, daemon_pubkey_b64,
                direct_endpoints_json, relay_url, enabled, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(registration_id) DO UPDATE SET
                host_id = COALESCE(excluded.host_id, connected_hosts.host_id),
                label = excluded.label,
                daemon_pubkey_b64 = excluded.daemon_pubkey_b64,
                direct_endpoints_json = excluded.direct_endpoints_json,
                relay_url = excluded.relay_url,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at",
            params![
                host.registration_id,
                host.host_id,
                host.label,
                host.daemon_pubkey_b64,
                endpoints_json,
                host.relay_url,
                i32::from(host.enabled),
                host.created_at.to_rfc3339(),
                host.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_connected_hosts(&self) -> Result<Vec<StoredConnectedHost>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT registration_id, host_id, label, daemon_pubkey_b64,
                    direct_endpoints_json, relay_url, enabled, created_at, updated_at
             FROM connected_hosts ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], map_connected_host_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_connected_host(
        &self,
        registration_id: &str,
    ) -> Result<Option<StoredConnectedHost>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT registration_id, host_id, label, daemon_pubkey_b64,
                    direct_endpoints_json, relay_url, enabled, created_at, updated_at
             FROM connected_hosts WHERE registration_id = ?1",
            params![registration_id],
            map_connected_host_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_connected_host_by_host_id(
        &self,
        host_id: &str,
    ) -> Result<Option<StoredConnectedHost>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT registration_id, host_id, label, daemon_pubkey_b64,
                    direct_endpoints_json, relay_url, enabled, created_at, updated_at
             FROM connected_hosts WHERE host_id = ?1 LIMIT 1",
            params![host_id],
            map_connected_host_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn pin_connected_host_id(
        &self,
        registration_id: &str,
        host_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE connected_hosts SET host_id = ?1, updated_at = ?2 WHERE registration_id = ?3",
            params![host_id, Utc::now().to_rfc3339(), registration_id],
        )?;
        Ok(())
    }

    pub fn delete_connected_host(&self, registration_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM connected_hosts WHERE registration_id = ?1",
            params![registration_id],
        )?;
        Ok(())
    }

    // --- Idempotency Receipts ---

    pub fn check_or_reserve_receipt(
        &self,
        scope: &str,
        key: &str,
        request_hash: &str,
    ) -> Result<ReceiptOutcome, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let existing: Option<(String, String, Option<String>)> = tx
            .query_row(
                "SELECT request_hash, status, result_json
                 FROM request_receipts WHERE scope = ?1 AND idempotency_key = ?2",
                params![scope, key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((saved_hash, status, result_json)) = existing {
            if saved_hash != request_hash {
                return Ok(ReceiptOutcome::Conflict);
            }
            if status == "committed"
                && let Some(res) = result_json
            {
                return Ok(ReceiptOutcome::Committed(res));
            }
            return Ok(ReceiptOutcome::PendingInFlight);
        }
        tx.execute(
            "INSERT INTO request_receipts (scope, idempotency_key, request_hash, status, result_json, created_at)
             VALUES (?1, ?2, ?3, 'pending', NULL, ?4)",
            params![scope, key, request_hash, Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(ReceiptOutcome::Reserved)
    }

    pub fn commit_receipt(
        &self,
        scope: &str,
        key: &str,
        result_json: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE request_receipts SET status = 'committed', result_json = ?1
             WHERE scope = ?2 AND idempotency_key = ?3",
            params![result_json, scope, key],
        )?;
        Ok(())
    }

    pub fn delete_pending_receipt(&self, scope: &str, key: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM request_receipts
             WHERE scope = ?1 AND idempotency_key = ?2 AND status = 'pending'",
            params![scope, key],
        )?;
        Ok(())
    }

    // --- Provider-Native Subagents ---

    pub fn upsert_provider_subagent(
        &self,
        desc: &ProviderSubagentDescriptor,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let status_json = serde_json::to_string(&desc.status)?;
        conn.execute(
            "INSERT INTO provider_subagents (
                parent_agent_id, subagent_id, parent_subagent_id, title, subtitle,
                status_json, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(parent_agent_id, subagent_id) DO UPDATE SET
                parent_subagent_id = COALESCE(excluded.parent_subagent_id, provider_subagents.parent_subagent_id),
                title = excluded.title,
                subtitle = COALESCE(excluded.subtitle, provider_subagents.subtitle),
                status_json = excluded.status_json,
                updated_at = excluded.updated_at",
            params![
                desc.parent_agent_id,
                desc.subagent_id,
                desc.parent_subagent_id,
                desc.title,
                desc.subtitle,
                status_json,
                desc.created_at.to_rfc3339(),
                desc.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_provider_subagents(
        &self,
        parent_agent_id: &str,
    ) -> Result<Vec<ProviderSubagentDescriptor>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT parent_agent_id, subagent_id, parent_subagent_id, title, subtitle,
                    status_json, created_at, updated_at
             FROM provider_subagents
             WHERE parent_agent_id = ?1
             ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![parent_agent_id], |row| {
            let status_str: String = row.get(5)?;
            let created_s: String = row.get(6)?;
            let updated_s: String = row.get(7)?;
            Ok(ProviderSubagentDescriptor {
                parent_agent_id: row.get(0)?,
                subagent_id: row.get(1)?,
                parent_subagent_id: row.get(2)?,
                title: row.get(3)?,
                subtitle: row.get(4)?,
                status: serde_json::from_str(&status_str)
                    .unwrap_or(ProviderSubagentStatus::Running),
                created_at: parse_ts(&created_s),
                updated_at: parse_ts(&updated_s),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn append_provider_subagent_event(
        &self,
        parent_agent_id: &str,
        subagent_id: &str,
        payload: TimelinePayload,
    ) -> Result<ProviderSubagentEvent, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let now = Utc::now();
        let payload_json = serde_json::to_string(&payload)?;
        conn.execute(
            "INSERT INTO provider_subagent_events (parent_agent_id, subagent_id, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![parent_agent_id, subagent_id, now.to_rfc3339(), payload_json],
        )?;
        let seq = conn.last_insert_rowid() as u64;
        Ok(ProviderSubagentEvent {
            seq,
            parent_agent_id: parent_agent_id.to_string(),
            subagent_id: subagent_id.to_string(),
            timestamp: now,
            payload,
        })
    }

    pub fn fetch_provider_subagent_events_since(
        &self,
        parent_agent_id: &str,
        subagent_id: &str,
        since_seq: u64,
        limit: usize,
    ) -> Result<(Vec<ProviderSubagentEvent>, bool), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, parent_agent_id, subagent_id, timestamp, payload_json
             FROM provider_subagent_events
             WHERE parent_agent_id = ?1 AND subagent_id = ?2 AND seq > ?3
             ORDER BY seq ASC
             LIMIT ?4",
        )?;
        let mut events = Vec::new();
        let rows = stmt.query_map(
            params![parent_agent_id, subagent_id, since_seq as i64, fetch_limit],
            |row| {
                Ok((
                    row.get::<_, i64>(0)? as u64,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )?;
        for r in rows {
            let (seq, pid, sid, ts_s, payload_s) = r?;
            if let Ok(payload) = serde_json::from_str::<TimelinePayload>(&payload_s) {
                events.push(ProviderSubagentEvent {
                    seq,
                    parent_agent_id: pid,
                    subagent_id: sid,
                    timestamp: parse_ts(&ts_s),
                    payload,
                });
            }
        }
        let has_more = events.len() > limit;
        if has_more {
            events.truncate(limit);
        }
        Ok((events, has_more))
    }
}

fn map_profile_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentProfile> {
    let provider_s: String = row.get(2)?;
    let effort_opt: Option<String> = row.get(4)?;
    let mode_s: String = row.get(5)?;
    let mcp_s: String = row.get(8)?;
    let created_s: String = row.get(9)?;
    let updated_s: String = row.get(10)?;
    let fast_mode: bool = row.get::<_, i32>(11).unwrap_or(0) != 0;
    let acp_agent_id: Option<String> = row.get(12).unwrap_or(None);
    Ok(AgentProfile {
        profile_id: row.get(0)?,
        name: row.get(1)?,
        provider: serde_json::from_str(&provider_s)
            .unwrap_or(orbit_protocol::ProviderKind::Antigravity),
        acp_agent_id,
        model: row.get(3)?,
        effort: effort_opt
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        fast_mode,
        mode: serde_json::from_str(&mode_s).unwrap_or(SessionMode::AcceptEdits),
        when_to_use: row.get(6)?,
        system_prompt_append: row.get(7)?,
        mcp_policy: serde_json::from_str::<McpToolPolicy>(&mcp_s).unwrap_or_default(),
        created_at: parse_ts(&created_s),
        updated_at: parse_ts(&updated_s),
    })
}

fn map_schedule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduleSpec> {
    let kind_s: String = row.get(1)?;
    let status_s: String = row.get(6)?;
    let target_s: String = row.get(7)?;
    let max_runs_opt: Option<i64> = row.get(8)?;
    let run_count: i64 = row.get(9)?;
    let expires_opt: Option<String> = row.get(10)?;
    let last_run_opt: Option<String> = row.get(11)?;
    let next_run_opt: Option<String> = row.get(12)?;
    let created_s: String = row.get(14)?;
    let updated_s: String = row.get(15)?;
    let target: ScheduleTarget =
        serde_json::from_str(&target_s).unwrap_or_else(|_| ScheduleTarget::ExistingAgent {
            agent_id: String::new(),
        });
    Ok(ScheduleSpec {
        schedule_id: row.get(0)?,
        kind: if kind_s == "heartbeat" {
            ScheduleKind::Heartbeat
        } else {
            ScheduleKind::Schedule
        },
        name: row.get(2)?,
        cron_expr: row.get(3)?,
        timezone: row.get(4)?,
        prompt: row.get(5)?,
        status: match status_s.as_str() {
            "paused" => ScheduleStatus::Paused,
            "completed" => ScheduleStatus::Completed,
            _ => ScheduleStatus::Active,
        },
        target,
        max_runs: max_runs_opt.map(|v| v.max(0) as u32),
        run_count: run_count.max(0) as u32,
        expires_at: expires_opt.as_deref().map(parse_ts),
        last_run_at: last_run_opt.as_deref().map(parse_ts),
        next_run_at: next_run_opt.as_deref().map(parse_ts),
        created_by_agent: row.get(13)?,
        created_at: parse_ts(&created_s),
        updated_at: parse_ts(&updated_s),
    })
}

fn map_connected_host_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredConnectedHost> {
    let ep_s: String = row.get(4)?;
    let created_s: String = row.get(7)?;
    let updated_s: String = row.get(8)?;
    Ok(StoredConnectedHost {
        registration_id: row.get(0)?,
        host_id: row.get(1)?,
        label: row.get(2)?,
        daemon_pubkey_b64: row.get(3)?,
        direct_endpoints: serde_json::from_str(&ep_s).unwrap_or_default(),
        relay_url: row.get(5)?,
        enabled: row.get::<_, i32>(6)? != 0,
        created_at: parse_ts(&created_s),
        updated_at: parse_ts(&updated_s),
    })
}

fn schedule_run_status_str(status: ScheduleRunStatus) -> &'static str {
    match status {
        ScheduleRunStatus::Running => "running",
        ScheduleRunStatus::Succeeded => "succeeded",
        ScheduleRunStatus::Failed => "failed",
        ScheduleRunStatus::Skipped => "skipped",
    }
}

fn parse_schedule_run_status(s: &str) -> ScheduleRunStatus {
    match s {
        "succeeded" => ScheduleRunStatus::Succeeded,
        "failed" => ScheduleRunStatus::Failed,
        "skipped" => ScheduleRunStatus::Skipped,
        _ => ScheduleRunStatus::Running,
    }
}
