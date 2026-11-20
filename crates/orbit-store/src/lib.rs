//! SQLite (WAL mode) persistence engine for Orbit Daemon and Multi-Host Client Replica Cache.
//!
//! Replaces Paseo's scattered JSON files with ACID event-sourced timelines and `< 1ms` cursor catch-up.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentStatus, AgentSummary, ProviderKind, ReasoningEffort, SessionMode, TerminalSummary,
    TimelineEvent, TimelinePayload, WorkspaceSummary,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Extra host configuration metadata persisted alongside `StoredHostProfile`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StoredHostMeta {
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default)]
    pub ssh_alias: Option<String>,
    #[serde(default)]
    pub route_mode: Option<String>,
    #[serde(default)]
    pub remote_daemon_port: Option<u16>,
    #[serde(default)]
    pub custom_direct_endpoint: Option<String>,
}

/// Persisted remote/local host profile stored in the client's local SQLite replica.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredHostProfile {
    pub host_id: String,
    pub display_name: String,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    #[serde(default)]
    pub meta: StoredHostMeta,
}

/// Persisted Paseo-style multi-directory Project (`1 primary cwd + 0..N extra_dirs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredClientProject {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct RewoundTimeline {
    pub agent: AgentSummary,
    pub events: Vec<TimelineEvent>,
    pub restored_prompt: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("store mutex poisoned")]
    LockPoisoned,
}

pub struct OrbitStore {
    conn: Mutex<Connection>,
}

impl OrbitStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::init_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(conn)
    }

    fn init_connection(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS workspaces (
                workspace_id TEXT PRIMARY KEY,
                title        TEXT NOT NULL,
                cwd          TEXT NOT NULL,
                git_branch   TEXT,
                is_worktree  INTEGER NOT NULL DEFAULT 0,
                updated_at   TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agents (
                agent_id        TEXT PRIMARY KEY,
                workspace_id    TEXT NOT NULL REFERENCES workspaces(workspace_id) ON DELETE CASCADE,
                cwd             TEXT NOT NULL,
                title           TEXT NOT NULL,
                provider_json   TEXT NOT NULL,
                model           TEXT,
                effort_json     TEXT,
                mode_json       TEXT NOT NULL,
                status_json     TEXT NOT NULL,
                parent_agent_id TEXT,
                persistence_ref TEXT,
                latest_seq      INTEGER NOT NULL DEFAULT 0,
                timeline_epoch  INTEGER NOT NULL DEFAULT 0,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS timeline_events (
                seq          INTEGER PRIMARY KEY AUTOINCREMENT,
                epoch        INTEGER NOT NULL,
                agent_id     TEXT NOT NULL REFERENCES agents(agent_id) ON DELETE CASCADE,
                turn_id      TEXT,
                timestamp    TEXT NOT NULL,
                payload_json TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_timeline_agent_seq
                ON timeline_events(agent_id, seq);

            CREATE TABLE IF NOT EXISTS turn_checkpoints (
                agent_id   TEXT NOT NULL REFERENCES agents(agent_id) ON DELETE CASCADE,
                prompt_seq INTEGER NOT NULL,
                turn_id    TEXT,
                tree_oid   TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (agent_id, prompt_seq)
            );

            CREATE TABLE IF NOT EXISTS terminals (
                terminal_id TEXT PRIMARY KEY,
                cwd         TEXT NOT NULL,
                title       TEXT NOT NULL,
                cols        INTEGER NOT NULL,
                rows        INTEGER NOT NULL,
                created_at  TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS client_hosts (
                host_id               TEXT PRIMARY KEY,
                display_name          TEXT NOT NULL,
                daemon_pubkey_b64     TEXT NOT NULL,
                direct_endpoints_json TEXT NOT NULL,
                relay_url             TEXT,
                meta_json             TEXT,
                updated_at            TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS client_projects (
                project_id       TEXT PRIMARY KEY,
                host_id          TEXT NOT NULL,
                name             TEXT NOT NULL,
                directories_json TEXT NOT NULL,
                updated_at_ms    INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS client_replica_workspaces (
                host_id      TEXT NOT NULL,
                workspace_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY (host_id, workspace_id)
            );

            CREATE TABLE IF NOT EXISTS client_replica_agents (
                host_id      TEXT NOT NULL,
                agent_id     TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY (host_id, agent_id)
            );

            CREATE TABLE IF NOT EXISTS client_replica_events (
                agent_id     TEXT NOT NULL,
                seq          INTEGER NOT NULL,
                epoch        INTEGER NOT NULL,
                turn_id      TEXT,
                timestamp    TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                PRIMARY KEY (agent_id, seq)
            );

            CREATE TABLE IF NOT EXISTS client_replica_cursors (
                agent_id       TEXT PRIMARY KEY,
                contiguous_seq INTEGER NOT NULL DEFAULT 0
            );
            ",
        )?;
        // Non-destructive forward-compatible column migrations for pre-existing databases
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN effort_json TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE agents ADD COLUMN timeline_epoch INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute("ALTER TABLE client_hosts ADD COLUMN meta_json TEXT", []);
        // One-time schema v2 migration: self-heal legacy pre-v0.2.4 client replica caches once
        let user_ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if user_ver < 2 {
            let _ = conn.execute(
                "DELETE FROM client_replica_events
                 WHERE agent_id IN (
                     SELECT DISTINCT agent_id FROM client_replica_events
                     WHERE payload_json LIKE '%\"agy_msg_default\"%'
                        OR payload_json LIKE '%\"agy_thought_default\"%'
                 )",
                [],
            );
            let _ = conn.execute_batch("PRAGMA user_version = 2;");
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_workspace(&self, ws: &WorkspaceSummary) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "
            INSERT INTO workspaces (workspace_id, title, cwd, git_branch, is_worktree, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(workspace_id) DO UPDATE SET
                title = excluded.title,
                cwd = excluded.cwd,
                git_branch = excluded.git_branch,
                is_worktree = excluded.is_worktree,
                updated_at = excluded.updated_at
            ",
            params![
                ws.workspace_id,
                ws.title,
                ws.cwd,
                ws.git_branch,
                i32::from(ws.is_worktree),
                ws.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn find_workspace_by_cwd(&self, cwd: &str) -> Result<Option<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT workspace_id, title, cwd, git_branch, is_worktree, updated_at
             FROM workspaces WHERE cwd = ?1 LIMIT 1",
            params![cwd],
            |row| {
                let ts: String = row.get(5)?;
                Ok(WorkspaceSummary {
                    workspace_id: row.get(0)?,
                    title: row.get(1)?,
                    cwd: row.get(2)?,
                    git_branch: row.get(3)?,
                    is_worktree: row.get::<_, i32>(4)? != 0,
                    updated_at: parse_ts(&ts),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT workspace_id, title, cwd, git_branch, is_worktree, updated_at
             FROM workspaces ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let ts: String = row.get(5)?;
            Ok(WorkspaceSummary {
                workspace_id: row.get(0)?,
                title: row.get(1)?,
                cwd: row.get(2)?,
                git_branch: row.get(3)?,
                is_worktree: row.get::<_, i32>(4)? != 0,
                updated_at: parse_ts(&ts),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_agent(
        &self,
        agent: &AgentSummary,
        persistence_ref: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let provider_json = serde_json::to_string(&agent.provider)?;
        let effort_json = agent
            .effort
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let mode_json = serde_json::to_string(&agent.mode)?;
        let status_json = serde_json::to_string(&agent.status)?;

        conn.execute(
            "
            INSERT INTO agents (
                agent_id, workspace_id, cwd, title, provider_json, model, effort_json,
                mode_json, status_json, parent_agent_id, persistence_ref,
                latest_seq, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
            ON CONFLICT(agent_id) DO UPDATE SET
                title = excluded.title,
                model = excluded.model,
                effort_json = excluded.effort_json,
                mode_json = excluded.mode_json,
                status_json = excluded.status_json,
                persistence_ref = COALESCE(excluded.persistence_ref, agents.persistence_ref),
                latest_seq = MAX(agents.latest_seq, excluded.latest_seq),
                updated_at = excluded.updated_at
            ",
            params![
                agent.agent_id,
                agent.workspace_id,
                agent.cwd,
                agent.title,
                provider_json,
                agent.model,
                effort_json,
                mode_json,
                status_json,
                agent.parent_agent_id,
                persistence_ref,
                agent.latest_seq as i64,
                agent.created_at.to_rfc3339(),
                agent.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, agent_id: &str) -> Result<Option<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
                    status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
                    timeline_epoch
             FROM agents WHERE agent_id = ?1",
            params![agent_id],
            map_agent_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn update_agent_mode(&self, agent_id: &str, mode: &SessionMode) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mode_json = serde_json::to_string(mode)?;
        conn.execute(
            "UPDATE agents SET mode_json = ?1, updated_at = ?2 WHERE agent_id = ?3",
            params![mode_json, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn update_agent_config(
        &self,
        agent_id: &str,
        model: Option<&str>,
        effort: Option<ReasoningEffort>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let effort_json = effort.as_ref().map(serde_json::to_string).transpose()?;
        conn.execute(
            "UPDATE agents SET model = ?1, effort_json = ?2, updated_at = ?3 WHERE agent_id = ?4",
            params![model, effort_json, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn update_agent_model(
        &self,
        agent_id: &str,
        model: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE agents SET model = ?1, updated_at = ?2 WHERE agent_id = ?3",
            params![model, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn switch_agent_provider(
        &self,
        agent_id: &str,
        provider: ProviderKind,
        model: Option<&str>,
        effort: Option<ReasoningEffort>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let provider_json = serde_json::to_string(&provider)?;
        let effort_json = effort.as_ref().map(serde_json::to_string).transpose()?;
        let status_json = serde_json::to_string(&AgentStatus::Idle)?;
        conn.execute(
            "UPDATE agents
             SET provider_json = ?1, model = ?2, effort_json = ?3, persistence_ref = NULL,
                 status_json = ?4, updated_at = ?5
             WHERE agent_id = ?6",
            params![
                provider_json,
                model,
                effort_json,
                status_json,
                Utc::now().to_rfc3339(),
                agent_id
            ],
        )?;
        Ok(())
    }

    pub fn set_persistence_ref(&self, agent_id: &str, pref: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE agents SET persistence_ref = ?1, updated_at = ?2 WHERE agent_id = ?3",
            params![pref, Utc::now().to_rfc3339(), agent_id],
        )?;
        Ok(())
    }

    pub fn delete_agent(&self, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("DELETE FROM agents WHERE agent_id = ?1", params![agent_id])?;
        Ok(())
    }

    pub fn list_agents(&self, workspace_id: Option<&str>) -> Result<Vec<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = if workspace_id.is_some() {
            "SELECT agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
                    status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
                    timeline_epoch
             FROM agents WHERE workspace_id = ?1 ORDER BY updated_at DESC"
        } else {
            "SELECT agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
                    status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
                    timeline_epoch
             FROM agents ORDER BY updated_at DESC"
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(ws_id) = workspace_id {
            stmt.query_map(params![ws_id], map_agent_row)?
                .collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], map_agent_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    /// Atomically append a timeline event, supersede prior partial streaming chunks with the same
    /// `item_id` / `call_id`, assign a monotonic `seq`, and update the agent's `latest_seq`.
    pub fn append_timeline_event(
        &self,
        agent_id: &str,
        epoch: u32,
        turn_id: Option<&str>,
        payload: TimelinePayload,
    ) -> Result<TimelineEvent, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let payload_json = serde_json::to_string(&payload)?;

        if let Some((json_key, id_val)) = coalesce_identity(&payload) {
            let boundary_seq =
                query_coalesce_boundary_seq(&tx, "timeline_events", agent_id, &payload, None);
            let pattern = format!("%\"{json_key}\":\"{id_val}\"%");
            let _ = tx.execute(
                "DELETE FROM timeline_events WHERE agent_id = ?1 AND seq > ?2 AND payload_json LIKE ?3",
                params![agent_id, boundary_seq, pattern],
            );
        }

        tx.execute(
            "INSERT INTO timeline_events (epoch, agent_id, turn_id, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![epoch, agent_id, turn_id, now_str, payload_json],
        )?;
        let seq = tx.last_insert_rowid() as u64;

        if let TimelinePayload::StatusChanged { ref status } = payload {
            let status_json = serde_json::to_string(status)?;
            tx.execute(
                "UPDATE agents SET latest_seq = ?1, status_json = ?2, updated_at = ?3 WHERE agent_id = ?4",
                params![seq as i64, status_json, now_str, agent_id],
            )?;
        } else {
            tx.execute(
                "UPDATE agents SET latest_seq = ?1, updated_at = ?2 WHERE agent_id = ?3",
                params![seq as i64, now_str, agent_id],
            )?;
        }

        tx.commit()?;

        Ok(TimelineEvent {
            seq,
            epoch,
            agent_id: agent_id.to_string(),
            turn_id: turn_id.map(ToOwned::to_owned),
            timestamp: now,
            payload,
        })
    }

    /// Upsert a server-sequenced timeline event into a client's local SQLite replica cache (`0ms` instant load).
    pub fn upsert_replica_timeline_event(&self, ev: &TimelineEvent) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let payload_json = serde_json::to_string(&ev.payload)?;
        let ts_str = ev.timestamp.to_rfc3339();

        if let Some((json_key, id_val)) = coalesce_identity(&ev.payload) {
            let boundary_seq = query_coalesce_boundary_seq(
                &tx,
                "client_replica_events",
                &ev.agent_id,
                &ev.payload,
                Some(ev.seq),
            );
            let pattern = format!("%\"{json_key}\":\"{id_val}\"%");
            let _ = tx.execute(
                "DELETE FROM client_replica_events
                 WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND payload_json LIKE ?4",
                params![ev.agent_id, boundary_seq, ev.seq as i64, pattern],
            );
        }

        tx.execute(
            "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(agent_id, seq) DO UPDATE SET
                payload_json = excluded.payload_json",
            params![
                ev.agent_id,
                ev.seq as i64,
                ev.epoch,
                ev.turn_id,
                ts_str,
                payload_json
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Batch-upsert a page of replica timeline events in a single SQLite transaction.
    pub fn upsert_replica_timeline_events_batch(
        &self,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        for ev in events {
            let payload_json = serde_json::to_string(&ev.payload)?;
            let ts_str = ev.timestamp.to_rfc3339();
            if let Some((json_key, id_val)) = coalesce_identity(&ev.payload) {
                let boundary_seq = query_coalesce_boundary_seq(
                    &tx,
                    "client_replica_events",
                    &ev.agent_id,
                    &ev.payload,
                    Some(ev.seq),
                );
                let pattern = format!("%\"{json_key}\":\"{id_val}\"%");
                let _ = tx.execute(
                    "DELETE FROM client_replica_events
                     WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND payload_json LIKE ?4",
                    params![ev.agent_id, boundary_seq, ev.seq as i64, pattern],
                );
            }
            tx.execute(
                "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(agent_id, seq) DO UPDATE SET
                    payload_json = excluded.payload_json",
                params![
                    ev.agent_id,
                    ev.seq as i64,
                    ev.epoch,
                    ev.turn_id,
                    ts_str,
                    payload_json
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_replica_contiguous_seq(&self, agent_id: &str) -> Result<u64, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let seq: Option<i64> = conn
            .query_row(
                "SELECT contiguous_seq FROM client_replica_cursors WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(seq.unwrap_or(0).max(0) as u64)
    }

    pub fn set_replica_contiguous_seq(&self, agent_id: &str, seq: u64) -> Result<(), StoreError> {
        let seq_i64 = i64::try_from(seq)
            .map_err(|_| StoreError::InvalidArgument("contiguous_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO client_replica_cursors (agent_id, contiguous_seq)
             VALUES (?1, ?2)
             ON CONFLICT(agent_id) DO UPDATE SET contiguous_seq = excluded.contiguous_seq",
            params![agent_id, seq_i64],
        )?;
        Ok(())
    }

    /// Replace all cached replica events for an agent after an authoritative server-side rewind.
    pub fn replace_replica_timeline(
        &self,
        agent_id: &str,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM client_replica_events WHERE agent_id = ?1",
            params![agent_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
            params![agent_id],
        )?;
        for ev in events {
            let seq_i64 = i64::try_from(ev.seq)
                .map_err(|_| StoreError::InvalidArgument("sequence number exceeds i64".into()))?;
            let payload_json = serde_json::to_string(&ev.payload)?;
            let ts_str = ev.timestamp.to_rfc3339();
            tx.execute(
                "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![agent_id, seq_i64, ev.epoch, ev.turn_id, ts_str, payload_json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn fetch_replica_timeline(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<TimelineEvent>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM client_replica_events
             WHERE agent_id = ?1
             ORDER BY seq ASC
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![agent_id, limit as i64], |row| {
            let seq = row.get::<_, i64>(0)? as u64;
            let epoch = row.get::<_, u32>(1)?;
            let ag_id: String = row.get(2)?;
            let turn_id: Option<String> = row.get(3)?;
            let ts_str: String = row.get(4)?;
            let payload_str: String = row.get(5)?;
            Ok((seq, epoch, ag_id, turn_id, ts_str, payload_str))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (seq, epoch, ag_id, turn_id, ts_str, payload_str) = r?;
            if let Ok(payload) = serde_json::from_str::<TimelinePayload>(&payload_str) {
                out.push(TimelineEvent {
                    seq,
                    epoch,
                    agent_id: ag_id,
                    turn_id,
                    timestamp: parse_ts(&ts_str),
                    payload,
                });
            }
        }
        Ok(out)
    }

    pub fn upsert_replica_workspaces(
        &self,
        host_id: &str,
        workspaces: &[WorkspaceSummary],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM client_replica_workspaces WHERE host_id = ?1",
            params![host_id],
        )?;
        for ws in workspaces {
            let json = serde_json::to_string(ws)?;
            tx.execute(
                "INSERT INTO client_replica_workspaces (host_id, workspace_id, payload_json)
                 VALUES (?1, ?2, ?3)",
                params![host_id, ws.workspace_id, json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_replica_workspaces(
        &self,
        host_id: &str,
    ) -> Result<Vec<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt =
            conn.prepare("SELECT payload_json FROM client_replica_workspaces WHERE host_id = ?1")?;
        let rows = stmt.query_map(params![host_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(ws) = serde_json::from_str::<WorkspaceSummary>(&r?) {
                out.push(ws);
            }
        }
        Ok(out)
    }

    pub fn upsert_replica_agents(
        &self,
        host_id: &str,
        agents: &[AgentSummary],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        for ag in agents {
            let json = serde_json::to_string(ag)?;
            tx.execute(
                "INSERT INTO client_replica_agents (host_id, agent_id, payload_json)
                 VALUES (?1, ?2, ?3)",
                params![host_id, ag.agent_id, json],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_single_replica_agent(
        &self,
        host_id: &str,
        ag: &AgentSummary,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let json = serde_json::to_string(ag)?;
        conn.execute(
            "INSERT INTO client_replica_agents (host_id, agent_id, payload_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET payload_json = excluded.payload_json",
            params![host_id, ag.agent_id, json],
        )?;
        Ok(())
    }

    pub fn delete_replica_agent(&self, host_id: &str, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1 AND agent_id = ?2",
            params![host_id, agent_id],
        )?;
        Ok(())
    }

    pub fn list_replica_agents(&self, host_id: &str) -> Result<Vec<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt =
            conn.prepare("SELECT payload_json FROM client_replica_agents WHERE host_id = ?1")?;
        let rows = stmt.query_map(params![host_id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(ag) = serde_json::from_str::<AgentSummary>(&r?) {
                out.push(ag);
            }
        }
        Ok(out)
    }

    /// Instant incremental catch-up query (`seq > since_seq`) for Daemon timelines.
    pub fn fetch_timeline_since(
        &self,
        agent_id: &str,
        since_seq: u64,
        limit: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        self.fetch_timeline_since_budgeted(agent_id, since_seq, limit, usize::MAX)
    }

    /// Fetch timeline events strictly after `since_seq` bounded by both event count and an
    /// approximate uncompressed JSON byte budget (`max_bytes`), always yielding at least one event
    /// when any exist so oversized single events never stall catchup pagination.
    pub fn fetch_timeline_since_budgeted(
        &self,
        agent_id: &str,
        since_seq: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM timeline_events
             WHERE agent_id = ?1 AND seq > ?2
             ORDER BY seq ASC
             LIMIT ?3",
        )?;

        let mut events = Vec::new();
        let mut approx_bytes: usize = 0;
        let mut has_more = false;
        let rows = stmt.query_map(params![agent_id, since_seq as i64, fetch_limit], |row| {
            let seq = row.get::<_, i64>(0)? as u64;
            let epoch = row.get::<_, u32>(1)?;
            let ag_id: String = row.get(2)?;
            let turn_id: Option<String> = row.get(3)?;
            let ts_str: String = row.get(4)?;
            let payload_str: String = row.get(5)?;
            Ok((seq, epoch, ag_id, turn_id, ts_str, payload_str))
        })?;

        for row in rows {
            let (seq, epoch, ag_id, turn_id, ts_str, payload_str) = row?;
            let row_bytes = payload_str.len().saturating_add(128);
            if events.len() >= limit
                || (!events.is_empty() && approx_bytes.saturating_add(row_bytes) > max_bytes)
            {
                has_more = true;
                break;
            }
            let payload: TimelinePayload = serde_json::from_str(&payload_str)?;
            approx_bytes = approx_bytes.saturating_add(row_bytes);
            events.push(TimelineEvent {
                seq,
                epoch,
                agent_id: ag_id,
                turn_id,
                timestamp: parse_ts(&ts_str),
                payload,
            });
        }

        Ok((events, has_more))
    }

    /// Persist a pre-turn workspace tree snapshot keyed by `(agent_id, prompt_seq)`.
    pub fn save_turn_checkpoint(
        &self,
        agent_id: &str,
        prompt_seq: u64,
        turn_id: Option<&str>,
        tree_oid: &str,
    ) -> Result<(), StoreError> {
        let prompt_seq_i64 = i64::try_from(prompt_seq)
            .map_err(|_| StoreError::InvalidArgument("prompt_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO turn_checkpoints (agent_id, prompt_seq, turn_id, tree_oid, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(agent_id, prompt_seq) DO UPDATE SET
                turn_id = excluded.turn_id,
                tree_oid = excluded.tree_oid,
                created_at = excluded.created_at",
            params![
                agent_id,
                prompt_seq_i64,
                turn_id,
                tree_oid,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn get_turn_checkpoint(
        &self,
        agent_id: &str,
        prompt_seq: u64,
    ) -> Result<Option<String>, StoreError> {
        let prompt_seq_i64 = i64::try_from(prompt_seq)
            .map_err(|_| StoreError::InvalidArgument("prompt_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT tree_oid FROM turn_checkpoints WHERE agent_id = ?1 AND prompt_seq = ?2",
            params![agent_id, prompt_seq_i64],
            |r| r.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    /// Fetch all daemon timeline events at or after `from_seq` (used before rewind to inspect edits).
    pub fn fetch_timeline_from(
        &self,
        agent_id: &str,
        from_seq: u64,
    ) -> Result<Vec<TimelineEvent>, StoreError> {
        let from_seq_i64 = i64::try_from(from_seq)
            .map_err(|_| StoreError::InvalidArgument("from_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM timeline_events
             WHERE agent_id = ?1 AND seq >= ?2
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![agent_id, from_seq_i64], |row| {
            let seq = row.get::<_, i64>(0)? as u64;
            let epoch = row.get::<_, u32>(1)?;
            let ag_id: String = row.get(2)?;
            let turn_id: Option<String> = row.get(3)?;
            let ts_str: String = row.get(4)?;
            let payload_str: String = row.get(5)?;
            Ok((seq, epoch, ag_id, turn_id, ts_str, payload_str))
        })?;
        let mut events = Vec::new();
        for row in rows {
            let (seq, epoch, ag_id, turn_id, ts_str, payload_str) = row?;
            let payload: TimelinePayload = serde_json::from_str(&payload_str)?;
            events.push(TimelineEvent {
                seq,
                epoch,
                agent_id: ag_id,
                turn_id,
                timestamp: parse_ts(&ts_str),
                payload,
            });
        }
        Ok(events)
    }

    /// Atomically truncate `timeline_events` and `turn_checkpoints` from `to_before_seq` onward,
    /// increment `timeline_epoch`, clear `persistence_ref` so the next prompt starts cleanly from
    /// the rewound history, and return the updated `AgentSummary`, remaining events, and removed
    /// prompt text (if any).
    pub fn rewind_agent_timeline(
        &self,
        agent_id: &str,
        to_before_seq: u64,
    ) -> Result<Option<RewoundTimeline>, StoreError> {
        let to_before_i64 = i64::try_from(to_before_seq)
            .map_err(|_| StoreError::InvalidArgument("to_before_seq exceeds i64".into()))?;
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;

        let target_payload_str: Option<String> = tx
            .query_row(
                "SELECT payload_json FROM timeline_events WHERE agent_id = ?1 AND seq = ?2",
                params![agent_id, to_before_i64],
                |r| r.get(0),
            )
            .optional()?;

        let restored_prompt = target_payload_str.and_then(|s| {
            serde_json::from_str::<TimelinePayload>(&s)
                .ok()
                .and_then(|p| match p {
                    TimelinePayload::UserPrompt { text, .. } => Some(text),
                    _ => None,
                })
        });

        tx.execute(
            "DELETE FROM timeline_events WHERE agent_id = ?1 AND seq >= ?2",
            params![agent_id, to_before_i64],
        )?;
        tx.execute(
            "DELETE FROM turn_checkpoints WHERE agent_id = ?1 AND prompt_seq >= ?2",
            params![agent_id, to_before_i64],
        )?;

        let new_latest_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM timeline_events WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let status_json = serde_json::to_string(&AgentStatus::Idle)?;
        let now_str = Utc::now().to_rfc3339();

        let updated_rows = tx.execute(
            "UPDATE agents
             SET latest_seq = ?1,
                 timeline_epoch = timeline_epoch + 1,
                 persistence_ref = NULL,
                 status_json = ?2,
                 updated_at = ?3
             WHERE agent_id = ?4",
            params![new_latest_seq, status_json, now_str, agent_id],
        )?;
        if updated_rows == 0 {
            return Ok(None);
        }

        let agent = tx
            .query_row(
                "SELECT agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
                        status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
                        timeline_epoch
                 FROM agents WHERE agent_id = ?1",
                params![agent_id],
                map_agent_row,
            )
            .optional()?;
        let Some(agent) = agent else {
            return Ok(None);
        };

        let mut remaining = Vec::new();
        let mut approx_bytes: usize = 0;
        let max_inline_bytes = orbit_protocol::MAX_WIRE_TIMELINE_PAGE_BYTES;
        {
            let mut stmt = tx.prepare(
                "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
                 FROM timeline_events
                 WHERE agent_id = ?1
                 ORDER BY seq ASC",
            )?;
            let rows = stmt.query_map(params![agent_id], |row| {
                let seq = row.get::<_, i64>(0)? as u64;
                let epoch = row.get::<_, u32>(1)?;
                let ag_id: String = row.get(2)?;
                let turn_id: Option<String> = row.get(3)?;
                let ts_str: String = row.get(4)?;
                let payload_str: String = row.get(5)?;
                Ok((seq, epoch, ag_id, turn_id, ts_str, payload_str))
            })?;
            for row in rows {
                let (seq, epoch, ag_id, turn_id, ts_str, payload_str) = row?;
                let row_bytes = payload_str.len().saturating_add(128);
                if !remaining.is_empty()
                    && approx_bytes.saturating_add(row_bytes) > max_inline_bytes
                {
                    break;
                }
                let payload: TimelinePayload = serde_json::from_str(&payload_str)?;
                approx_bytes = approx_bytes.saturating_add(row_bytes);
                remaining.push(TimelineEvent {
                    seq,
                    epoch,
                    agent_id: ag_id,
                    turn_id,
                    timestamp: parse_ts(&ts_str),
                    payload,
                });
            }
        }

        tx.commit()?;
        Ok(Some(RewoundTimeline {
            agent,
            events: remaining,
            restored_prompt,
        }))
    }

    pub fn get_persistence_ref(&self, agent_id: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let res: Option<Option<String>> = conn
            .query_row(
                "SELECT persistence_ref FROM agents WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(res.flatten())
    }

    pub fn upsert_terminal(&self, term: &TerminalSummary) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO terminals (terminal_id, cwd, title, cols, rows, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(terminal_id) DO UPDATE SET
                cols = excluded.cols,
                rows = excluded.rows,
                title = excluded.title",
            params![
                term.terminal_id,
                term.cwd,
                term.title,
                i32::from(term.cols),
                i32::from(term.rows),
                term.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_terminals(&self) -> Result<Vec<TerminalSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT terminal_id, cwd, title, cols, rows, created_at
             FROM terminals ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let ts: String = row.get(5)?;
            Ok(TerminalSummary {
                terminal_id: row.get(0)?,
                cwd: row.get(1)?,
                title: row.get(2)?,
                cols: row.get::<_, u16>(3)?,
                rows: row.get::<_, u16>(4)?,
                created_at: parse_ts(&ts),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_terminal(&self, terminal_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM terminals WHERE terminal_id = ?1",
            params![terminal_id],
        )?;
        Ok(())
    }

    pub fn upsert_client_host(&self, host: &StoredHostProfile) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let endpoints_json = serde_json::to_string(&host.direct_endpoints)?;
        let meta_json = serde_json::to_string(&host.meta)?;
        conn.execute(
            "INSERT INTO client_hosts (
                host_id, display_name, daemon_pubkey_b64, direct_endpoints_json, relay_url, meta_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(host_id) DO UPDATE SET
                display_name = excluded.display_name,
                daemon_pubkey_b64 = excluded.daemon_pubkey_b64,
                direct_endpoints_json = excluded.direct_endpoints_json,
                relay_url = excluded.relay_url,
                meta_json = excluded.meta_json,
                updated_at = excluded.updated_at",
            params![
                host.host_id,
                host.display_name,
                host.daemon_pubkey_b64,
                endpoints_json,
                host.relay_url,
                meta_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_client_hosts(&self) -> Result<Vec<StoredHostProfile>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT host_id, display_name, daemon_pubkey_b64, direct_endpoints_json, relay_url, meta_json
             FROM client_hosts ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let endpoints_str: String = row.get(3)?;
            let meta_opt: Option<String> = row.get(5)?;
            let direct_endpoints: Vec<String> =
                serde_json::from_str(&endpoints_str).unwrap_or_default();
            let meta: StoredHostMeta = meta_opt
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default();
            Ok(StoredHostProfile {
                host_id: row.get(0)?,
                display_name: row.get(1)?,
                daemon_pubkey_b64: row.get(2)?,
                direct_endpoints,
                relay_url: row.get(4)?,
                meta,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_client_host(&self, host_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_hosts WHERE host_id = ?1",
            params![host_id],
        )?;
        conn.execute(
            "DELETE FROM client_projects WHERE host_id = ?1",
            params![host_id],
        )?;
        conn.execute(
            "DELETE FROM client_replica_workspaces WHERE host_id = ?1",
            params![host_id],
        )?;
        conn.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        Ok(())
    }

    pub fn clear_host_replica(&self, host_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_replica_workspaces WHERE host_id = ?1",
            params![host_id],
        )?;
        conn.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        Ok(())
    }

    pub fn upsert_client_project(&self, proj: &StoredClientProject) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let dirs_json = serde_json::to_string(&proj.directories)?;
        conn.execute(
            "INSERT INTO client_projects (project_id, host_id, name, directories_json, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(project_id) DO UPDATE SET
                host_id = excluded.host_id,
                name = excluded.name,
                directories_json = excluded.directories_json,
                updated_at_ms = excluded.updated_at_ms",
            params![
                proj.project_id,
                proj.host_id,
                proj.name,
                dirs_json,
                proj.updated_at_ms
            ],
        )?;
        Ok(())
    }

    pub fn list_client_projects(&self) -> Result<Vec<StoredClientProject>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT project_id, host_id, name, directories_json, updated_at_ms
             FROM client_projects ORDER BY updated_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let dirs_str: String = row.get(3)?;
            let directories: Vec<String> = serde_json::from_str(&dirs_str).unwrap_or_default();
            Ok(StoredClientProject {
                project_id: row.get(0)?,
                host_id: row.get(1)?,
                name: row.get(2)?,
                directories,
                updated_at_ms: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_client_project(&self, project_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_projects WHERE project_id = ?1",
            params![project_id],
        )?;
        Ok(())
    }

    /// Passive non-blocking WAL checkpoint safe to invoke when a mobile app enters background.
    pub fn checkpoint_passive(&self) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        Ok(())
    }

    /// Purge legacy daemon-schema `timeline_events` / `turn_checkpoints` rows that accumulated in
    /// older client replica databases (`desktop-client.sqlite`) before `client_replica_events` was
    /// introduced. Only call this on client-side replica DBs — never on the daemon's `orbit.db`.
    pub fn purge_legacy_client_timeline_events(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let legacy_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM timeline_events", [], |row| row.get(0))
            .unwrap_or(0);
        if legacy_count <= 0 {
            return Ok(0);
        }
        conn.execute("DELETE FROM turn_checkpoints", [])?;
        conn.execute("DELETE FROM timeline_events", [])?;
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        if legacy_count >= 100 {
            let _ = conn.execute_batch("VACUUM;");
        }
        Ok(legacy_count as usize)
    }
}

fn query_coalesce_boundary_seq(
    tx: &rusqlite::Transaction<'_>,
    table: &str,
    agent_id: &str,
    payload: &TimelinePayload,
    upper_seq: Option<u64>,
) -> i64 {
    let stop_at_tool = matches!(
        payload,
        TimelinePayload::AssistantMessage { .. } | TimelinePayload::AssistantThinking { .. }
    );
    let sql = match (stop_at_tool, upper_seq.is_some()) {
        (true, true) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1 AND seq < ?2
               AND (payload_json LIKE '%\"type\":\"user_prompt\"%'
                    OR payload_json LIKE '%\"type\":\"tool_call\"%')"
        ),
        (true, false) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1
               AND (payload_json LIKE '%\"type\":\"user_prompt\"%'
                    OR payload_json LIKE '%\"type\":\"tool_call\"%')"
        ),
        (false, true) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1 AND seq < ?2
               AND payload_json LIKE '%\"type\":\"user_prompt\"%'"
        ),
        (false, false) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1
               AND payload_json LIKE '%\"type\":\"user_prompt\"%'"
        ),
    };

    if let Some(max_s) = upper_seq {
        tx.query_row(&sql, params![agent_id, max_s as i64], |r| r.get(0))
            .unwrap_or(0)
    } else {
        tx.query_row(&sql, params![agent_id], |r| r.get(0))
            .unwrap_or(0)
    }
}

fn coalesce_identity(payload: &TimelinePayload) -> Option<(&'static str, &str)> {
    match payload {
        TimelinePayload::AssistantMessage { item_id, .. }
        | TimelinePayload::AssistantThinking { item_id, .. }
            if !item_id.is_empty() && !item_id.contains('"') && !item_id.contains('%') =>
        {
            Some(("item_id", item_id.as_str()))
        }
        TimelinePayload::ToolCall { call_id, .. }
            if !call_id.is_empty() && !call_id.contains('"') && !call_id.contains('%') =>
        {
            Some(("call_id", call_id.as_str()))
        }
        _ => None,
    }
}

fn map_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSummary> {
    let provider_str: String = row.get(4)?;
    let mode_str: String = row.get(6)?;
    let status_str: String = row.get(7)?;
    let created_str: String = row.get(10)?;
    let updated_str: String = row.get(11)?;
    let effort_opt_str: Option<String> = row.get(12)?;
    let timeline_epoch: u64 = row.get::<_, i64>(13).unwrap_or(0).max(0) as u64;

    let provider: ProviderKind =
        serde_json::from_str(&provider_str).unwrap_or(ProviderKind::Antigravity);
    let effort: Option<ReasoningEffort> = effort_opt_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::Default);
    let status: AgentStatus = serde_json::from_str(&status_str).unwrap_or(AgentStatus::Closed);

    Ok(AgentSummary {
        agent_id: row.get(0)?,
        workspace_id: row.get(1)?,
        cwd: row.get(2)?,
        title: row.get(3)?,
        provider,
        model: row.get(5)?,
        effort,
        mode,
        status,
        parent_agent_id: row.get(8)?,
        latest_seq: row.get::<_, i64>(9)? as u64,
        timeline_epoch,
        created_at: parse_ts(&created_str),
        updated_at: parse_ts(&updated_str),
    })
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_wal_timeline_cursor_catchup_and_coalescing() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();

        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "orbit".into(),
                cwd: "/workspace/orbit".into(),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
            })
            .expect("upsert ws");

        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_1".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/workspace/orbit".into(),
                    title: "Build Daemon".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: Some(ReasoningEffort::High),
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                },
                Some("session_abc"),
            )
            .expect("upsert agent");

        let e1 = store
            .append_timeline_event(
                "ag_1",
                1,
                Some("turn_1"),
                TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "Hello Orbit".into(),
                    badges: Vec::new(),
                },
            )
            .expect("append e1");
        let e2 = store
            .append_timeline_event(
                "ag_1",
                1,
                Some("turn_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Ready".into(),
                    streaming: true,
                },
            )
            .expect("append e2");
        let e3 = store
            .append_timeline_event(
                "ag_1",
                1,
                Some("turn_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Ready in Rust!".into(),
                    streaming: false,
                },
            )
            .expect("append e3");

        assert!(e3.seq > e2.seq && e2.seq > e1.seq);

        let (all, _) = store.fetch_timeline_since("ag_1", 0, 50).expect("catchup");
        // Partial streaming row e2 was superseded by e3!
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].seq, e3.seq);
    }

    #[test]
    fn rewind_timeline_truncates_events_checkpoints_and_clears_persistence_ref() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();

        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "orbit".into(),
                cwd: "/workspace/orbit".into(),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
            })
            .expect("upsert ws");

        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_rw".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/workspace/orbit".into(),
                    title: "Rewind Test".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                },
                Some("prov_sess_1"),
            )
            .expect("upsert agent");

        let p1 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t1"),
                TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "Turn 1".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p1");
        store
            .save_turn_checkpoint("ag_rw", p1.seq, Some("t1"), "tree_aaa")
            .expect("cp1");
        let a1 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "Done 1".into(),
                    streaming: false,
                },
            )
            .expect("a1");
        let p2 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t2"),
                TimelinePayload::UserPrompt {
                    message_id: "m2".into(),
                    text: "Turn 2".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p2");
        store
            .save_turn_checkpoint("ag_rw", p2.seq, Some("t2"), "tree_bbb")
            .expect("cp2");
        let _a2 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t2"),
                TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "Done 2".into(),
                    streaming: false,
                },
            )
            .expect("a2");

        // Verify replica replacement too
        store.upsert_replica_timeline_event(&p1).expect("rep p1");
        store.upsert_replica_timeline_event(&p2).expect("rep p2");

        let rewound = store
            .rewind_agent_timeline("ag_rw", p2.seq)
            .expect("rewind")
            .expect("agent exists");
        let updated_agent = rewound.agent;
        let remaining = rewound.events;
        let restored_prompt = rewound.restored_prompt;
        assert_eq!(restored_prompt.as_deref(), Some("Turn 2"));
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].seq, p1.seq);
        assert_eq!(remaining[1].seq, a1.seq);
        assert_eq!(updated_agent.latest_seq, a1.seq);
        assert_eq!(updated_agent.timeline_epoch, 1);
        assert_eq!(
            store.get_persistence_ref("ag_rw").expect("pref"),
            None,
            "persistence_ref must be cleared after rewind"
        );
        assert_eq!(
            store
                .get_turn_checkpoint("ag_rw", p1.seq)
                .expect("cp1")
                .as_deref(),
            Some("tree_aaa")
        );
        assert_eq!(
            store.get_turn_checkpoint("ag_rw", p2.seq).expect("cp2"),
            None
        );

        store
            .replace_replica_timeline("ag_rw", &remaining)
            .expect("replace replica");
        let rep = store
            .fetch_replica_timeline("ag_rw", 50)
            .expect("fetch replica");
        assert_eq!(rep.len(), 2);
        assert_eq!(rep[1].seq, a1.seq);
    }

    #[test]
    fn purge_legacy_client_timeline_events_preserves_replica_rows() {
        let store = OrbitStore::open_in_memory().expect("mem db");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_leg".into(),
                title: "demo".into(),
                cwd: "/tmp/demo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_leg".into(),
                    workspace_id: "ws_leg".into(),
                    cwd: "/tmp/demo".into(),
                    title: "Legacy".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                },
                None,
            )
            .expect("ag");

        let ev = store
            .append_timeline_event(
                "ag_leg",
                1,
                None,
                TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "legacy row".into(),
                    badges: vec![],
                },
            )
            .expect("append");
        store.upsert_replica_timeline_event(&ev).expect("replica");

        let purged = store.purge_legacy_client_timeline_events().expect("purge");
        assert_eq!(purged, 1);
        assert!(
            store
                .fetch_timeline_from("ag_leg", 0)
                .expect("fetch")
                .is_empty()
        );
        assert_eq!(
            store
                .fetch_replica_timeline("ag_leg", 50)
                .expect("replica")
                .len(),
            1
        );
    }

    #[test]
    fn budgeted_timeline_pagination_and_replica_cursor_reset() {
        let store = OrbitStore::open_in_memory().expect("in-memory store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_pg".into(),
                title: "ws".into(),
                cwd: "/tmp/pg".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_pg".into(),
                    workspace_id: "ws_pg".into(),
                    cwd: "/tmp/pg".into(),
                    title: "Big".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                },
                None,
            )
            .expect("ag");

        let big_text = "x".repeat(2000);
        let mut seqs = Vec::new();
        for idx in 0..5 {
            let ev = store
                .append_timeline_event(
                    "ag_pg",
                    1,
                    None,
                    TimelinePayload::AssistantMessage {
                        item_id: format!("m_{idx}"),
                        text: big_text.clone(),
                        streaming: false,
                    },
                )
                .expect("append");
            seqs.push(ev.seq);
        }

        // Budget of 5000 bytes (~2 events per page) must paginate across multiple chunks
        let (p1, more1) = store
            .fetch_timeline_since_budgeted("ag_pg", 0, 100, 5000)
            .expect("p1");
        assert!(more1);
        assert_eq!(p1.len(), 2);

        let (p2, more2) = store
            .fetch_timeline_since_budgeted("ag_pg", p1[1].seq, 100, 5000)
            .expect("p2");
        assert!(more2);
        assert_eq!(p2.len(), 2);

        let (p3, more3) = store
            .fetch_timeline_since_budgeted("ag_pg", p2[1].seq, 100, 5000)
            .expect("p3");
        assert!(!more3);
        assert_eq!(p3.len(), 1);
        assert_eq!(p3[0].seq, seqs[4]);

        // Even if a single event exceeds max_bytes, at least 1 event is always yielded
        let (oversized, has_more) = store
            .fetch_timeline_since_budgeted("ag_pg", 0, 100, 100)
            .expect("oversized");
        assert_eq!(oversized.len(), 1);
        assert!(has_more);

        // Replica cursor tracks contiguous sync and resets on replace_replica_timeline
        assert_eq!(store.get_replica_contiguous_seq("ag_pg").expect("get"), 0);
        store
            .upsert_replica_timeline_events_batch(&p1)
            .expect("batch");
        store
            .set_replica_contiguous_seq("ag_pg", p1[1].seq)
            .expect("set");
        assert_eq!(
            store.get_replica_contiguous_seq("ag_pg").expect("get"),
            p1[1].seq
        );
        store.replace_replica_timeline("ag_pg", &[]).expect("wipe");
        assert_eq!(store.get_replica_contiguous_seq("ag_pg").expect("get"), 0);
    }
}
