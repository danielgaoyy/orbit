//! SQLite (WAL mode) persistence engine for Orbit Daemon and Multi-Host Client Replica Cache.
//!
//! Replaces Paseo's scattered JSON files with ACID event-sourced timelines and `< 1ms` cursor catch-up.

use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentStatus, AgentSummary, ProviderKind, SessionMode, TerminalSummary, TimelineEvent,
    TimelinePayload, WorkspaceSummary,
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
        // Seamless forward-compatible column migration for existing SQLite files
        let _ = conn.execute("ALTER TABLE client_hosts ADD COLUMN meta_json TEXT", []);
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
        let mode_json = serde_json::to_string(&agent.mode)?;
        let status_json = serde_json::to_string(&agent.status)?;

        conn.execute(
            "
            INSERT INTO agents (
                agent_id, workspace_id, cwd, title, provider_json, model,
                mode_json, status_json, parent_agent_id, persistence_ref,
                latest_seq, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(agent_id) DO UPDATE SET
                title = excluded.title,
                model = excluded.model,
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
                    status_json, parent_agent_id, latest_seq, created_at, updated_at
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
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let provider_json = serde_json::to_string(&provider)?;
        let status_json = serde_json::to_string(&AgentStatus::Idle)?;
        conn.execute(
            "UPDATE agents
             SET provider_json = ?1, model = NULL, persistence_ref = NULL,
                 status_json = ?2, updated_at = ?3
             WHERE agent_id = ?4",
            params![
                provider_json,
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
                    status_json, parent_agent_id, latest_seq, created_at, updated_at
             FROM agents WHERE workspace_id = ?1 ORDER BY updated_at DESC"
        } else {
            "SELECT agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
                    status_json, parent_agent_id, latest_seq, created_at, updated_at
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
            let pattern = format!("%\"{json_key}\":\"{id_val}\"%");
            let _ = tx.execute(
                "DELETE FROM timeline_events WHERE agent_id = ?1 AND payload_json LIKE ?2",
                params![agent_id, pattern],
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
            let pattern = format!("%\"{json_key}\":\"{id_val}\"%");
            let _ = tx.execute(
                "DELETE FROM client_replica_events WHERE agent_id = ?1 AND payload_json LIKE ?2",
                params![ev.agent_id, pattern],
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

    let provider: ProviderKind =
        serde_json::from_str(&provider_str).unwrap_or(ProviderKind::Antigravity);
    let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::Default);
    let status: AgentStatus = serde_json::from_str(&status_str).unwrap_or(AgentStatus::Closed);

    Ok(AgentSummary {
        agent_id: row.get(0)?,
        workspace_id: row.get(1)?,
        cwd: row.get(2)?,
        title: row.get(3)?,
        provider,
        model: row.get(5)?,
        mode,
        status,
        parent_agent_id: row.get(8)?,
        latest_seq: row.get::<_, i64>(9)? as u64,
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
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
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
}
