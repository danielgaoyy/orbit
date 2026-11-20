//! SQLite (WAL mode) persistence engine for Orbit Daemon and Multi-Host Client Replica Cache.
//!
//! Replaces Paseo's scattered JSON files with ACID event-sourced timelines and `< 1ms` cursor catch-up.

pub mod orchestration;
pub use orchestration::*;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentStatus, AgentSummary, AttentionState, ProviderKind, ReasoningEffort, SessionMode,
    TerminalSummary, TimelineEvent, TimelinePayload, WorkspaceSetupStatus, WorkspaceSummary,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

pub(crate) const AGENT_SELECT_COLS: &str = "agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
    status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
    parent_host_id, archived_at, background, auto_archive, profile_id, labels_json,
    attention_state, last_activity_at";

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

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
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
            ",
        )?;
        // Non-destructive forward-compatible column migrations for pre-existing databases
        let _ = conn.execute("ALTER TABLE agents ADD COLUMN effort_json TEXT", []);
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
        orchestration::migrate_orchestration_schema(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn upsert_workspace(&self, ws: &WorkspaceSummary) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let setup_status_str = serde_json::to_string(&ws.setup_status)?
            .trim_matches('"')
            .to_string();
        conn.execute(
            "
            INSERT INTO workspaces (
                workspace_id, title, cwd, git_branch, is_worktree, updated_at,
                source_cwd, base_ref, parent_workspace_id, archived_at, background, setup_status
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(workspace_id) DO UPDATE SET
                title = excluded.title,
                cwd = excluded.cwd,
                git_branch = excluded.git_branch,
                is_worktree = excluded.is_worktree,
                updated_at = excluded.updated_at,
                source_cwd = COALESCE(excluded.source_cwd, workspaces.source_cwd),
                base_ref = COALESCE(excluded.base_ref, workspaces.base_ref),
                parent_workspace_id = COALESCE(excluded.parent_workspace_id, workspaces.parent_workspace_id),
                archived_at = excluded.archived_at,
                background = excluded.background,
                setup_status = excluded.setup_status
            ",
            params![
                ws.workspace_id,
                ws.title,
                ws.cwd,
                ws.git_branch,
                i32::from(ws.is_worktree),
                ws.updated_at.to_rfc3339(),
                ws.source_cwd,
                ws.base_ref,
                ws.parent_workspace_id,
                ws.archived_at.map(|t| t.to_rfc3339()),
                i32::from(ws.background),
                setup_status_str,
            ],
        )?;
        Ok(())
    }

    pub fn find_workspace_by_cwd(&self, cwd: &str) -> Result<Option<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT workspace_id, title, cwd, git_branch, is_worktree, updated_at,
                    source_cwd, base_ref, parent_workspace_id, archived_at, background, setup_status
             FROM workspaces WHERE cwd = ?1 AND archived_at IS NULL LIMIT 1",
            params![cwd],
            map_workspace_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn list_workspaces(&self) -> Result<Vec<WorkspaceSummary>, StoreError> {
        self.list_workspaces_filtered(false, false)
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
        let labels_json = serde_json::to_string(&agent.labels)?;
        let attention_str = serde_json::to_string(&agent.attention)?
            .trim_matches('"')
            .to_string();

        conn.execute(
            "
            INSERT INTO agents (
                agent_id, workspace_id, cwd, title, provider_json, model, effort_json,
                mode_json, status_json, parent_agent_id, parent_host_id, persistence_ref,
                latest_seq, created_at, updated_at, archived_at, background, auto_archive,
                profile_id, labels_json, attention_state, last_activity_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22)
            ON CONFLICT(agent_id) DO UPDATE SET
                title = excluded.title,
                model = excluded.model,
                effort_json = excluded.effort_json,
                mode_json = excluded.mode_json,
                status_json = excluded.status_json,
                parent_agent_id = excluded.parent_agent_id,
                parent_host_id = excluded.parent_host_id,
                persistence_ref = COALESCE(excluded.persistence_ref, agents.persistence_ref),
                latest_seq = MAX(agents.latest_seq, excluded.latest_seq),
                updated_at = excluded.updated_at,
                archived_at = excluded.archived_at,
                background = excluded.background,
                auto_archive = excluded.auto_archive,
                profile_id = COALESCE(excluded.profile_id, agents.profile_id),
                labels_json = excluded.labels_json,
                attention_state = excluded.attention_state,
                last_activity_at = COALESCE(excluded.last_activity_at, agents.last_activity_at)
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
                agent.parent_host_id,
                persistence_ref,
                agent.latest_seq as i64,
                agent.created_at.to_rfc3339(),
                agent.updated_at.to_rfc3339(),
                agent.archived_at.map(|t| t.to_rfc3339()),
                i32::from(agent.background),
                i32::from(agent.auto_archive),
                agent.profile_id,
                labels_json,
                attention_str,
                agent.last_activity_at.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, agent_id: &str) -> Result<Option<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {AGENT_SELECT_COLS} FROM agents WHERE agent_id = ?1");
        conn.query_row(&sql, params![agent_id], map_agent_row)
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
        self.list_agents_filtered(workspace_id, false, false, None)
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

        let has_more = events.len() > limit;
        if has_more {
            events.truncate(limit);
        }
        Ok((events, has_more))
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
                    OR payload_json LIKE '%\"type\":\"agent_message\"%'
                    OR payload_json LIKE '%\"type\":\"tool_call\"%')"
        ),
        (true, false) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1
               AND (payload_json LIKE '%\"type\":\"user_prompt\"%'
                    OR payload_json LIKE '%\"type\":\"agent_message\"%'
                    OR payload_json LIKE '%\"type\":\"tool_call\"%')"
        ),
        (false, true) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1 AND seq < ?2
               AND (payload_json LIKE '%\"type\":\"user_prompt\"%'
                    OR payload_json LIKE '%\"type\":\"agent_message\"%')"
        ),
        (false, false) => format!(
            "SELECT COALESCE(MAX(seq), 0) FROM {table}
             WHERE agent_id = ?1
               AND (payload_json LIKE '%\"type\":\"user_prompt\"%'
                    OR payload_json LIKE '%\"type\":\"agent_message\"%')"
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

pub(crate) fn map_workspace_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceSummary> {
    let ts: String = row.get(5)?;
    let archived_opt: Option<String> = row.get(9)?;
    let setup_s: String = row.get(11)?;
    let setup_status = match setup_s.as_str() {
        "running" => WorkspaceSetupStatus::Running,
        "succeeded" => WorkspaceSetupStatus::Succeeded,
        "failed" => WorkspaceSetupStatus::Failed,
        _ => WorkspaceSetupStatus::Idle,
    };
    Ok(WorkspaceSummary {
        workspace_id: row.get(0)?,
        title: row.get(1)?,
        cwd: row.get(2)?,
        git_branch: row.get(3)?,
        is_worktree: row.get::<_, i32>(4)? != 0,
        updated_at: parse_ts(&ts),
        source_cwd: row.get(6)?,
        base_ref: row.get(7)?,
        parent_workspace_id: row.get(8)?,
        archived_at: archived_opt.as_deref().map(parse_ts),
        background: row.get::<_, i32>(10)? != 0,
        setup_status,
    })
}

pub(crate) fn map_agent_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSummary> {
    let provider_str: String = row.get(4)?;
    let mode_str: String = row.get(6)?;
    let status_str: String = row.get(7)?;
    let created_str: String = row.get(10)?;
    let updated_str: String = row.get(11)?;
    let effort_opt_str: Option<String> = row.get(12)?;
    let parent_host_id: Option<String> = row.get(13)?;
    let archived_opt: Option<String> = row.get(14)?;
    let background: bool = row.get::<_, i32>(15)? != 0;
    let auto_archive: bool = row.get::<_, i32>(16)? != 0;
    let profile_id: Option<String> = row.get(17)?;
    let labels_str: String = row.get(18)?;
    let attention_str: String = row.get(19)?;
    let last_activity_opt: Option<String> = row.get(20)?;

    let provider: ProviderKind =
        serde_json::from_str(&provider_str).unwrap_or(ProviderKind::Antigravity);
    let effort: Option<ReasoningEffort> = effort_opt_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::Default);
    let status: AgentStatus = serde_json::from_str(&status_str).unwrap_or(AgentStatus::Closed);
    let labels: HashMap<String, String> = serde_json::from_str(&labels_str).unwrap_or_default();
    let attention = match attention_str.as_str() {
        "finished" => AttentionState::Finished,
        "error" => AttentionState::Error,
        "permission" => AttentionState::Permission,
        _ => AttentionState::None,
    };

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
        parent_host_id,
        latest_seq: row.get::<_, i64>(9)? as u64,
        created_at: parse_ts(&created_str),
        updated_at: parse_ts(&updated_str),
        archived_at: archived_opt.as_deref().map(parse_ts),
        background,
        auto_archive,
        profile_id,
        labels,
        attention,
        last_activity_at: last_activity_opt.as_deref().map(parse_ts),
    })
}

pub(crate) fn parse_ts(s: &str) -> DateTime<Utc> {
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
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
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
                    parent_host_id: None,
                    latest_seq: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
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
    fn orchestration_tables_soft_archive_detach_and_receipts() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        let ws = WorkspaceSummary {
            workspace_id: "ws_orch".into(),
            title: "orch".into(),
            cwd: "/tmp/orch".into(),
            git_branch: Some("feat/orch".into()),
            is_worktree: true,
            updated_at: now,
            source_cwd: Some("/tmp/src".into()),
            base_ref: Some("origin/main".into()),
            parent_workspace_id: None,
            archived_at: None,
            background: false,
            setup_status: WorkspaceSetupStatus::Idle,
        };
        store.upsert_workspace(&ws).expect("upsert ws");

        let mut parent = AgentSummary {
            agent_id: "ag_parent".into(),
            workspace_id: "ws_orch".into(),
            cwd: "/tmp/orch".into(),
            title: "Parent Lead".into(),
            provider: ProviderKind::ClaudeCode,
            model: Some("claude-opus".into()),
            effort: Some(ReasoningEffort::High),
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Running {
                turn_id: "t1".into(),
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::Finished,
            last_activity_at: Some(now),
        };
        store.upsert_agent(&parent, Some("ref_p")).expect("parent");

        let mut child = parent.clone();
        child.agent_id = "ag_child".into();
        child.title = "Child Worker".into();
        child.parent_agent_id = Some("ag_parent".into());
        child.labels.insert("orbit.open_tab.desktop".into(), "true".into());
        store.upsert_agent(&child, Some("ref_c")).expect("child");

        // Detach strips parent & open_tab labels
        store.detach_agent("ag_child").expect("detach");
        let detached = store.get_agent("ag_child").expect("get").expect("some");
        assert!(detached.parent_agent_id.is_none());
        assert!(!detached.labels.contains_key("orbit.open_tab.desktop"));

        // Soft archive normalizes running -> closed, hides from default list, and preserves events
        store.archive_agent_soft("ag_parent").expect("soft archive");
        let archived = store.get_agent("ag_parent").expect("get").expect("some");
        assert!(archived.archived_at.is_some());
        assert_eq!(archived.status, AgentStatus::Closed);
        assert_eq!(archived.attention, AttentionState::None);
        assert_eq!(store.list_agents(None).expect("active").len(), 1);
        assert_eq!(
            store
                .list_agents_filtered(None, true, true, None)
                .expect("all")
                .len(),
            2
        );
        store.unarchive_agent("ag_parent").expect("unarchive");
        assert_eq!(store.list_agents(None).expect("active again").len(), 2);

        // Finish subscriptions
        let sub = StoredFinishSubscription {
            id: "fs_1".into(),
            caller_agent_id: "ag_parent".into(),
            child_agent_id: "ag_child".into(),
            target_host_id: Some("host_b".into()),
            require_parent_owner: true,
            delivered_perm_ids: Vec::new(),
            created_at: now,
        };
        store.upsert_finish_subscription(&sub).expect("upsert sub");
        assert!(
            store
                .has_finish_subscriptions_for_caller("ag_parent")
                .expect("has sub")
        );
        store
            .mark_finish_subscription_perm_delivered("fs_1", "perm_42")
            .expect("mark perm");
        let subs = store.list_finish_subscriptions().expect("list subs");
        assert_eq!(subs[0].delivered_perm_ids, vec!["perm_42".to_string()]);

        // Profiles
        let profile = orbit_protocol::AgentProfile {
            profile_id: "prof_rev".into(),
            name: "Independent Reviewer".into(),
            provider: ProviderKind::Codex,
            model: Some("gpt-5.4".into()),
            effort: Some(ReasoningEffort::Xhigh),
            mode: SessionMode::Plan,
            when_to_use: "Review diffs independently without editing files".into(),
            system_prompt_append: Some("Never edit files.".into()),
            mcp_policy: orbit_protocol::McpToolPolicy {
                enabled: true,
                disabled_tools: vec!["create_agent".into()],
            },
            created_at: now,
            updated_at: now,
        };
        store.upsert_profile(&profile).expect("upsert profile");
        assert_eq!(
            store
                .get_profile_by_name("independent reviewer")
                .expect("by name")
                .expect("found")
                .profile_id,
            "prof_rev"
        );

        // Idempotency receipts
        assert_eq!(
            store
                .check_or_reserve_receipt("create_agent", "k1", "hash_a")
                .expect("reserve"),
            ReceiptOutcome::Reserved
        );
        assert_eq!(
            store
                .check_or_reserve_receipt("create_agent", "k1", "hash_b")
                .expect("conflict"),
            ReceiptOutcome::Conflict
        );
        assert_eq!(
            store
                .check_or_reserve_receipt("create_agent", "k1", "hash_a")
                .expect("pending"),
            ReceiptOutcome::PendingInFlight
        );
        store
            .commit_receipt("create_agent", "k1", "{\"agent_id\":\"ag_99\"}")
            .expect("commit");
        assert_eq!(
            store
                .check_or_reserve_receipt("create_agent", "k1", "hash_a")
                .expect("committed"),
            ReceiptOutcome::Committed("{\"agent_id\":\"ag_99\"}".into())
        );

        // Provider subagents
        let psub = orbit_protocol::ProviderSubagentDescriptor {
            subagent_id: "task_1".into(),
            parent_agent_id: "ag_parent".into(),
            parent_subagent_id: None,
            title: "Codebase Explorer".into(),
            subtitle: Some("Claude Sonnet · high".into()),
            status: orbit_protocol::ProviderSubagentStatus::Running,
            created_at: now,
            updated_at: now,
        };
        store.upsert_provider_subagent(&psub).expect("upsert psub");
        let ev = store
            .append_provider_subagent_event(
                "ag_parent",
                "task_1",
                TimelinePayload::AssistantMessage {
                    item_id: "m1".into(),
                    text: "Scanning files...".into(),
                    streaming: false,
                },
            )
            .expect("append psub ev");
        let (evs, more) = store
            .fetch_provider_subagent_events_since("ag_parent", "task_1", 0, 50)
            .expect("fetch psub");
        assert!(!more);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].seq, ev.seq);

        parent.title = "Updated".into();
        let _ = parent;
    }
}
