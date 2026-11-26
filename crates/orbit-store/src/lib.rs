//! SQLite (WAL mode) persistence engine for Orbit Daemon and Multi-Host Client Replica Cache.
//!
//! Replaces Paseo's scattered JSON files with ACID event-sourced timelines and `< 1ms` cursor catch-up.

pub mod artifacts;
pub mod goals;
pub mod guardian;
pub mod im_channels;
pub mod memory;
pub mod orchestration;
pub mod permission_rules;
pub mod push;
pub mod search;
mod secrets;
pub mod timers;
pub mod triggers;
pub mod usage;
pub mod workflows;
pub mod workspace_sync;
pub use artifacts::{
    MAX_ARTIFACT_CONTENT_BYTES, MAX_ARTIFACT_ID_LEN, MAX_ARTIFACT_REVISIONS, UpsertArtifactInput,
    validate_artifact_id,
};
pub use im_channels::StoredImChannel;
pub use orchestration::*;
pub use permission_rules::{
    PathResolutionError, canonicalize_project_cwd, resolve_path_preserving_symlinks,
};
pub use push::{StoredPushDevice, StoredPushPreferences};
pub use search::{
    DaemonMessageSearchHit, FtsBackfillProgress, MessageSearchHit, build_fts5_match_query,
};
pub use triggers::{
    DEFAULT_RECENT_DELIVERIES_LIMIT, StoredEventTrigger, UpsertEventTriggerParams,
    mask_trigger_secret,
};
pub use usage::{
    AgentContextTelemetry, QUOTA_SAMPLES_RETENTION_MS, QuotaSampleRecord, USAGE_LEDGER_RETENTION_MS,
};
pub use workspace_sync::{AgentContextSnippet, LABEL_HANDLE_ALIASES, SemanticIdentityOptions};

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use orbit_protocol::{
    AgentStatus, AgentSummary, BackgroundTaskInfo, PromptBadge, ProviderKind, QueuedPromptView,
    ReasoningEffort, SessionMode, TerminalSummary, TimelineEvent, TimelineOutlinePrompt,
    TimelinePayload, ToolCallDetail, ToolCallStatus, WorkspaceClusterSuggestion,
    WorkspaceSetupStatus, WorkspaceSummary,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

pub(crate) const WORKSPACE_SELECT_COLS: &str =
    "workspace_id, title, cwd, git_branch, is_worktree, updated_at,
    source_cwd, base_ref, parent_workspace_id, archived_at, background, setup_status,
    mission_goal, project_root, board_revision, title_locked, auto_clustered, is_curated";

pub(crate) const AGENT_SELECT_COLS: &str =
    "agent_id, workspace_id, cwd, title, provider_json, model, mode_json,
    status_json, parent_agent_id, latest_seq, created_at, updated_at, effort_json,
    parent_host_id, archived_at, background, auto_archive, profile_id, labels_json,
    attention_state, last_activity_at, timeline_epoch, read_seq, is_pinned, pinned_order,
    handle, cluster_suggestion_json, task_summary, forked_from_agent_id, forked_at_seq, fast_mode, acp_agent_id";

/// Atomic result of [`OrbitStore::clone_timeline_events_up_to`], computed inside the same
/// `IMMEDIATE` transaction as the copy so callers avoid TOCTOU races.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClonedTimelineOutcome {
    pub new_latest_seq: u64,
    pub copied_count: usize,
    pub cloned_full_tail: bool,
    pub effective_source_seq: Option<u64>,
}

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
    #[serde(default)]
    pub pairing_secret_updated_at_ms: i64,
    /// Peer `host_id`s able to bridge to this host (e.g. for LAN-isolated nodes).
    #[serde(default)]
    pub bridge_host_ids: Vec<String>,
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
    pub pairing_secret: String,
    #[serde(default)]
    pub meta: StoredHostMeta,
}

/// Persisted multi-directory Project (`1 primary cwd + 0..N extra_dirs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredClientProject {
    pub project_id: String,
    pub host_id: String,
    pub name: String,
    pub directories: Vec<String>,
    pub updated_at_ms: i64,
}

/// Persisted per-thread client UI state (`seen_seq`, pinned status & ordering).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientThreadPref {
    pub host_id: String,
    pub agent_id: String,
    pub seen_seq: u64,
    pub is_pinned: bool,
    pub pinned_order: i64,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct RewoundTimeline {
    pub agent: AgentSummary,
    pub events: Vec<TimelineEvent>,
    pub restored_prompt: Option<String>,
}

/// Full daemon-persisted queued follow-up prompt (including full wire prompt for offline drain).
#[derive(Debug, Clone)]
pub struct StoredQueuedPrompt {
    pub queue_id: String,
    pub agent_id: String,
    pub wire_prompt: String,
    pub display_prompt: String,
    pub badges: Vec<PromptBadge>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("store mutex poisoned")]
    LockPoisoned,
}

impl StoreError {
    /// Returns `true` when this error is a transient SQLite lock/busy contention error.
    pub fn is_busy(&self) -> bool {
        match self {
            Self::Sqlite(rusqlite::Error::SqliteFailure(err, _)) => matches!(
                err.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ),
            Self::Sqlite(err) => {
                let msg = err.to_string().to_ascii_lowercase();
                msg.contains("database is locked") || msg.contains("database table is locked")
            }
            _ => false,
        }
    }
}

pub mod audit_chain;

#[derive(Debug)]
pub struct PendingOutboxEntry {
    pub id: i64,
    pub host_id: String,
    pub payload_json: String,
    pub created_at_ms: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

impl OrbitStore {
    pub fn enqueue_pending_outbox(
        &self,
        host_id: &str,
        payload_json: &str,
    ) -> Result<i64, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM client_pending_outbox WHERE host_id=?1",
            [host_id],
            |row| row.get(0),
        )?;
        if count >= 100 {
            return Err(StoreError::InvalidArgument(
                "host outbox is full (100 pending messages)".into(),
            ));
        }
        let stored = self
            .secrets
            .seal(&format!("outbox/{host_id}"), payload_json)?;
        tx.execute("INSERT INTO client_pending_outbox(host_id,payload_json,created_at_ms) VALUES(?1,?2,?3)",params![host_id,stored,Utc::now().timestamp_millis()])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn has_pending_outbox(&self, host_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM client_pending_outbox WHERE host_id=?1)",
            [host_id],
            |r| r.get(0),
        )?)
    }

    pub fn list_pending_outbox(
        &self,
        host_id: &str,
    ) -> Result<Vec<PendingOutboxEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare("SELECT id,payload_json,created_at_ms,attempts,last_error FROM client_pending_outbox WHERE host_id=?1 ORDER BY id")?;
        let mut rows = stmt.query([host_id])?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next()? {
            entries.push(PendingOutboxEntry {
                id: row.get(0)?,
                host_id: host_id.to_owned(),
                payload_json: self
                    .secrets
                    .open(&format!("outbox/{host_id}"), &row.get::<_, String>(1)?)?,
                created_at_ms: row.get(2)?,
                attempts: row.get(3)?,
                last_error: row.get(4)?,
            });
        }
        Ok(entries)
    }

    pub fn remove_pending_outbox(&self, id: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("DELETE FROM client_pending_outbox WHERE id=?1", [id])?;
        Ok(())
    }

    pub fn mark_outbox_attempt(&self, id: i64, err: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE client_pending_outbox SET attempts=attempts+1,last_error=?2 WHERE id=?1",
            params![id, err],
        )?;
        Ok(())
    }

    pub fn prune_stale_outbox(&self, max_age_ms: i64) -> Result<usize, StoreError> {
        if max_age_ms < 0 {
            return Err(StoreError::InvalidArgument(
                "negative outbox retention".into(),
            ));
        }
        let cutoff = Utc::now().timestamp_millis().saturating_sub(max_age_ms);
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.execute(
            "DELETE FROM client_pending_outbox WHERE created_at_ms < ?1",
            [cutoff],
        )?)
    }

    pub fn cloud_dirty_revision(&self) -> Result<Option<i64>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("SELECT updated_at_ms FROM client_kv WHERE key='cloud_sync_dirty' AND value_json='true'", [], |r|r.get(0)).optional()?)
    }

    /// Clear only the exact generation that was included in a successful sync.
    pub fn clear_cloud_dirty_revision(&self, revision: i64) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("UPDATE client_kv SET value_json='false' WHERE key='cloud_sync_dirty' AND updated_at_ms=?1",[revision])?;
        Ok(())
    }
}

fn install_cloud_dirty_triggers(conn: &Connection) -> Result<(), StoreError> {
    for table in [
        "client_hosts",
        "client_thread_prefs",
        "client_projects",
        "client_kv",
    ] {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let row = if event == "DELETE" { "OLD" } else { "NEW" };
            let filter = if table == "client_kv" {
                format!(
                    "WHEN {row}.key IN ('cloud_sync_prefs_v1','project_order','primary_router_selection','cloud_sync_fleet_network_v1')"
                )
            } else {
                String::new()
            };
            conn.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS dirty_{table}_{event}
                AFTER {event} ON {table} {filter} BEGIN
                INSERT INTO client_kv(key,value_json,updated_at_ms) VALUES('cloud_sync_dirty','true',CAST(strftime('%s','now') AS INTEGER)*1000)
                ON CONFLICT(key) DO UPDATE SET value_json='true', updated_at_ms=MAX(updated_at_ms+1,excluded.updated_at_ms);
                END;"))?;
        }
    }
    Ok(())
}

pub struct OrbitStore {
    audit_identity: std::sync::OnceLock<(String, ed25519_dalek::SigningKey)>,
    conn: Mutex<Connection>,
    secrets: secrets::SecretEnvelope,
}

#[cfg(unix)]
fn is_under_orbit_home(dir: &Path) -> bool {
    if let Ok(explicit) = std::env::var("ORBIT_HOME") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() && dir.starts_with(Path::new(trimmed)) {
            return true;
        }
    }
    if let Some(home) = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        && dir.starts_with(home.join(".orbit"))
    {
        return true;
    }
    dir.file_name().is_some_and(|n| n == ".orbit")
}

#[cfg(unix)]
fn harden_sqlite_parent_and_precreate(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
        if is_under_orbit_home(parent) {
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    if !path.exists() {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true).mode(0o600);
        match opts.open(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(unix)]
fn harden_sqlite_db_and_sidecars(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let perms = std::fs::Permissions::from_mode(0o600);
    if path.exists() {
        std::fs::set_permissions(path, perms.clone())?;
    }
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar_path = std::path::PathBuf::from(sidecar);
        if sidecar_path.exists() {
            std::fs::set_permissions(&sidecar_path, perms.clone())?;
        }
    }
    Ok(())
}

impl OrbitStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_master_key(path, None)
    }

    pub fn open_with_master_key(
        path: impl AsRef<Path>,
        key: Option<[u8; 32]>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref();
        #[cfg(unix)]
        harden_sqlite_parent_and_precreate(path)?;
        let conn = Connection::open(path)?;
        let store = Self::init_connection(conn, key)?;
        #[cfg(unix)]
        harden_sqlite_db_and_sidecars(path)?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::init_connection(conn, None)
    }

    fn init_connection(mut conn: Connection, key: Option<[u8; 32]>) -> Result<Self, StoreError> {
        conn.busy_timeout(std::time::Duration::from_millis(4000))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA secure_delete = ON;
            PRAGMA foreign_keys = ON;
            PRAGMA wal_autocheckpoint = 1000;
            PRAGMA journal_size_limit = 16777216;
            PRAGMA cache_size = -16000;
            PRAGMA temp_store = MEMORY;

            CREATE TABLE IF NOT EXISTS client_pending_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                host_id TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_outbox_host_id ON client_pending_outbox(host_id,id);
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
                payload_json TEXT NOT NULL,
                event_type   TEXT,
                coalesce_key TEXT
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
                pairing_secret        TEXT NOT NULL DEFAULT '',
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
                event_type   TEXT,
                coalesce_key TEXT,
                PRIMARY KEY (agent_id, seq)
            );

            CREATE TABLE IF NOT EXISTS client_replica_cursors (
                agent_id       TEXT PRIMARY KEY,
                contiguous_seq INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS client_thread_prefs (
                host_id       TEXT NOT NULL,
                agent_id      TEXT NOT NULL,
                seen_seq      INTEGER NOT NULL DEFAULT 0,
                is_pinned     INTEGER NOT NULL DEFAULT 0,
                pinned_order  INTEGER NOT NULL DEFAULT 0,
                updated_at_ms INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (host_id, agent_id)
            );

            CREATE TABLE IF NOT EXISTS agent_queued_prompts (
                queue_id       TEXT PRIMARY KEY,
                agent_id       TEXT NOT NULL,
                sort_order     INTEGER NOT NULL,
                wire_prompt    TEXT NOT NULL,
                display_prompt TEXT NOT NULL,
                badges_json    TEXT NOT NULL DEFAULT '[]',
                created_at     TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_agent_queued_prompts_agent
                ON agent_queued_prompts (agent_id, sort_order);

            CREATE TABLE IF NOT EXISTS composer_drafts (
                draft_key     TEXT PRIMARY KEY,
                draft_json    TEXT NOT NULL,
                updated_at_ms INTEGER NOT NULL
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
        let _ = conn.execute("ALTER TABLE timeline_events ADD COLUMN event_type TEXT", []);
        let _ = conn.execute(
            "ALTER TABLE timeline_events ADD COLUMN coalesce_key TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE client_replica_events ADD COLUMN event_type TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE client_replica_events ADD COLUMN coalesce_key TEXT",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE client_replica_cursors ADD COLUMN has_older INTEGER NOT NULL DEFAULT 0",
            [],
        );
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
        workspace_sync::migrate_sync_and_mission_schema(&conn)?;
        let post_orch_ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if post_orch_ver < 4 {
            conn.execute_batch(
                "
                BEGIN IMMEDIATE;

                UPDATE timeline_events
                SET event_type = json_extract(payload_json, '$.type'),
                    coalesce_key = CASE
                        WHEN json_extract(payload_json, '$.type') IN ('assistant_message', 'assistant_thinking')
                             AND COALESCE(json_extract(payload_json, '$.item_id'), '') != ''
                            THEN 'item_id:' || json_extract(payload_json, '$.item_id')
                        WHEN json_extract(payload_json, '$.type') = 'tool_call'
                             AND COALESCE(json_extract(payload_json, '$.call_id'), '') != ''
                            THEN 'call_id:' || json_extract(payload_json, '$.call_id')
                        ELSE NULL
                    END
                WHERE event_type IS NULL AND json_valid(payload_json);

                UPDATE client_replica_events
                SET event_type = json_extract(payload_json, '$.type'),
                    coalesce_key = CASE
                        WHEN json_extract(payload_json, '$.type') IN ('assistant_message', 'assistant_thinking')
                             AND COALESCE(json_extract(payload_json, '$.item_id'), '') != ''
                            THEN 'item_id:' || json_extract(payload_json, '$.item_id')
                        WHEN json_extract(payload_json, '$.type') = 'tool_call'
                             AND COALESCE(json_extract(payload_json, '$.call_id'), '') != ''
                            THEN 'call_id:' || json_extract(payload_json, '$.call_id')
                        ELSE NULL
                    END
                WHERE event_type IS NULL AND json_valid(payload_json);

                PRAGMA user_version = 4;

                COMMIT;
                ",
            )?;
        }
        let post_v4_ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if post_v4_ver < 5 {
            // One-time schema v5 migration: reset only `client_replica_cursors` while preserving
            // all cached `client_replica_events` so previously gapped threads self-heal via a
            // single silent `since_seq = 0` catchup pass without losing 0ms instant open.
            conn.execute_batch(
                "
                BEGIN IMMEDIATE;
                DELETE FROM client_replica_cursors;
                PRAGMA user_version = 5;
                COMMIT;
                ",
            )?;
        }
        conn.execute_batch(
            "
            CREATE INDEX IF NOT EXISTS idx_timeline_agent_type_seq
                ON timeline_events(agent_id, event_type, seq);
            CREATE INDEX IF NOT EXISTS idx_timeline_agent_coalesce_seq
                ON timeline_events(agent_id, coalesce_key, seq);
            CREATE INDEX IF NOT EXISTS idx_replica_agent_type_seq
                ON client_replica_events(agent_id, event_type, seq);
            CREATE INDEX IF NOT EXISTS idx_replica_agent_coalesce_seq
                ON client_replica_events(agent_id, coalesce_key, seq);
            CREATE INDEX IF NOT EXISTS idx_agents_workspace_id
                ON agents(workspace_id);
            CREATE INDEX IF NOT EXISTS idx_agents_parent_agent_id
                ON agents(parent_agent_id);
            CREATE INDEX IF NOT EXISTS idx_workspaces_cwd
                ON workspaces(cwd);
            CREATE INDEX IF NOT EXISTS idx_schedules_status_next_run
                ON schedules(status, next_run_at);
            ",
        )?;
        search::migrate_fts5_schema(&mut conn)?;
        memory::migrate(&conn)?;
        // Idempotent Wave 2 column migrations checked via `PRAGMA table_info` (NO `PRAGMA user_version` bump)
        for (table, col, col_type) in [
            ("agents", "forked_from_agent_id", "TEXT"),
            ("agents", "forked_at_seq", "INTEGER"),
            ("agents", "fast_mode", "INTEGER NOT NULL DEFAULT 0"),
            ("agents", "acp_agent_id", "TEXT"),
            ("client_replica_agents", "forked_from_agent_id", "TEXT"),
            ("client_replica_agents", "forked_at_seq", "INTEGER"),
            (
                "client_replica_agents",
                "fast_mode",
                "INTEGER NOT NULL DEFAULT 0",
            ),
            ("client_replica_agents", "acp_agent_id", "TEXT"),
            ("agent_profiles", "fast_mode", "INTEGER NOT NULL DEFAULT 0"),
            ("agent_profiles", "acp_agent_id", "TEXT"),
            ("client_hosts", "pairing_secret", "TEXT NOT NULL DEFAULT ''"),
        ] {
            ensure_table_column(&conn, table, col, col_type)?;
        }
        artifacts::migrate_artifacts_schema(&conn)?;
        usage::migrate_usage_schema(&conn)?;
        timers::migrate_conditional_timers_schema(&conn)?;
        guardian::migrate_guardian_schema(&conn)?;
        permission_rules::migrate_permission_rules_schema(&conn)?;
        goals::migrate_goals_schema(&conn)?;
        triggers::migrate_event_triggers_schema(&conn)?;
        workflows::migrate_workflows_schema(&conn)?;
        im_channels::migrate_im_channels_schema(&conn)?;
        push::migrate_push_schema(&conn)?;
        conn.execute_batch(audit_chain::SCHEMA)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS spend_budget_alerts(period TEXT PRIMARY KEY)",
        )?;
        let secrets = secrets::SecretEnvelope(key.map(zeroize::Zeroizing::new));
        secrets.initialize(&mut conn)?;
        install_cloud_dirty_triggers(&conn)?;
        Ok(Self {
            audit_identity: Default::default(),
            secrets,
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
                source_cwd, base_ref, parent_workspace_id, archived_at, background, setup_status,
                mission_goal, project_root, board_revision, title_locked, auto_clustered, is_curated
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
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
                setup_status = excluded.setup_status,
                mission_goal = COALESCE(excluded.mission_goal, workspaces.mission_goal),
                project_root = COALESCE(excluded.project_root, workspaces.project_root),
                board_revision = MAX(workspaces.board_revision, excluded.board_revision),
                title_locked = excluded.title_locked,
                auto_clustered = excluded.auto_clustered,
                is_curated = excluded.is_curated
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
                ws.mission_goal,
                ws.project_root,
                ws.board_revision as i64,
                i32::from(ws.title_locked),
                i32::from(ws.auto_clustered),
                i32::from(ws.is_curated),
            ],
        )?;
        Ok(())
    }

    pub fn find_workspace_by_cwd(&self, cwd: &str) -> Result<Option<WorkspaceSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {WORKSPACE_SELECT_COLS} FROM workspaces WHERE cwd = ?1 AND archived_at IS NULL LIMIT 1"
        );
        conn.query_row(&sql, params![cwd], map_workspace_row)
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
        let cluster_suggestion_json = agent
            .cluster_suggestion
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        let forked_at_seq_i64 = agent
            .forked_at_seq
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StoreError::InvalidArgument("forked_at_seq exceeds i64".into()))?;

        conn.execute(
            "
            INSERT INTO agents (
                agent_id, workspace_id, cwd, title, provider_json, model, effort_json,
                mode_json, status_json, parent_agent_id, parent_host_id, persistence_ref,
                latest_seq, timeline_epoch, created_at, updated_at, archived_at, background, auto_archive,
                profile_id, labels_json, attention_state, last_activity_at,
                read_seq, is_pinned, pinned_order, handle, cluster_suggestion_json, task_summary,
                forked_from_agent_id, forked_at_seq, fast_mode, acp_agent_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33)
            ON CONFLICT(agent_id) DO UPDATE SET
                workspace_id = excluded.workspace_id,
                title = excluded.title,
                model = excluded.model,
                effort_json = excluded.effort_json,
                mode_json = excluded.mode_json,
                status_json = excluded.status_json,
                parent_agent_id = excluded.parent_agent_id,
                parent_host_id = excluded.parent_host_id,
                persistence_ref = COALESCE(excluded.persistence_ref, agents.persistence_ref),
                latest_seq = MAX(agents.latest_seq, excluded.latest_seq),
                timeline_epoch = MAX(agents.timeline_epoch, excluded.timeline_epoch),
                updated_at = excluded.updated_at,
                archived_at = excluded.archived_at,
                background = excluded.background,
                auto_archive = excluded.auto_archive,
                profile_id = COALESCE(excluded.profile_id, agents.profile_id),
                labels_json = excluded.labels_json,
                attention_state = excluded.attention_state,
                last_activity_at = COALESCE(excluded.last_activity_at, agents.last_activity_at),
                read_seq = MAX(agents.read_seq, excluded.read_seq),
                is_pinned = excluded.is_pinned,
                pinned_order = excluded.pinned_order,
                handle = COALESCE(excluded.handle, agents.handle),
                cluster_suggestion_json = excluded.cluster_suggestion_json,
                task_summary = COALESCE(excluded.task_summary, agents.task_summary),
                forked_from_agent_id = COALESCE(excluded.forked_from_agent_id, agents.forked_from_agent_id),
                forked_at_seq = COALESCE(excluded.forked_at_seq, agents.forked_at_seq),
                fast_mode = excluded.fast_mode,
                acp_agent_id = COALESCE(excluded.acp_agent_id, agents.acp_agent_id)
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
                agent.timeline_epoch as i64,
                agent.created_at.to_rfc3339(),
                agent.updated_at.to_rfc3339(),
                agent.archived_at.map(|t| t.to_rfc3339()),
                i32::from(agent.background),
                i32::from(agent.auto_archive),
                agent.profile_id,
                labels_json,
                attention_str,
                agent.last_activity_at.map(|t| t.to_rfc3339()),
                agent.read_seq as i64,
                i32::from(agent.is_pinned),
                agent.pinned_order,
                agent.handle,
                cluster_suggestion_json,
                agent.task_summary,
                agent.forked_from_agent_id,
                forked_at_seq_i64,
                i32::from(agent.fast_mode),
                agent.acp_agent_id,
            ],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, agent_id: &str) -> Result<Option<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {AGENT_SELECT_COLS} FROM agents WHERE agent_id = ?1");
        let mut agent = conn
            .query_row(&sql, params![agent_id], map_agent_row)
            .optional()?;
        if let Some(ref mut ag) = agent {
            ag.queued_prompts = load_queued_prompts_for_agent(&conn, &ag.agent_id)?;
        }
        Ok(agent)
    }

    pub fn enqueue_agent_prompt(
        &self,
        agent_id: &str,
        queue_id: &str,
        wire_prompt: &str,
        display_prompt: &str,
        badges: &[PromptBadge],
    ) -> Result<QueuedPromptView, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let next_order: i64 = tx.query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
            |r| r.get(0),
        )?;
        // Never let a client-supplied `queue_id` collide with and overwrite another agent's row.
        let existing_owner: Option<String> = tx
            .query_row(
                "SELECT agent_id FROM agent_queued_prompts WHERE queue_id = ?1",
                params![queue_id],
                |r| r.get(0),
            )
            .optional()?;
        let now = Utc::now();
        let effective_qid = match existing_owner {
            Some(owner) if owner != agent_id => {
                format!(
                    "{agent_id}_{queue_id}_{}",
                    now.timestamp_nanos_opt().unwrap_or(0)
                )
            }
            _ => queue_id.to_string(),
        };
        let badges_json = serde_json::to_string(badges)?;
        tx.execute(
            "INSERT OR REPLACE INTO agent_queued_prompts
             (queue_id, agent_id, sort_order, wire_prompt, display_prompt, badges_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                effective_qid,
                agent_id,
                next_order,
                wire_prompt,
                display_prompt,
                badges_json,
                now.to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(QueuedPromptView {
            queue_id: effective_qid,
            display_text: display_prompt.to_string(),
            badges: badges.to_vec(),
            created_at: now,
        })
    }

    /// Re-insert a previously taken queued prompt at the front of `agent_id`'s FIFO queue
    /// (preserving its `queue_id`, `created_at`, and ordering ahead of later queued items).
    pub fn requeue_agent_prompt_front(
        &self,
        agent_id: &str,
        item: &StoredQueuedPrompt,
    ) -> Result<QueuedPromptView, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let min_order: Option<i64> = tx.query_row(
            "SELECT MIN(sort_order) FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
            |r| r.get(0),
        )?;
        let front_order = min_order.map_or(1, |m| m.saturating_sub(1));
        let badges_json = serde_json::to_string(&item.badges)?;
        tx.execute(
            "INSERT OR REPLACE INTO agent_queued_prompts
             (queue_id, agent_id, sort_order, wire_prompt, display_prompt, badges_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                item.queue_id,
                agent_id,
                front_order,
                item.wire_prompt,
                item.display_prompt,
                badges_json,
                item.created_at.to_rfc3339(),
            ],
        )?;
        tx.commit()?;
        Ok(QueuedPromptView {
            queue_id: item.queue_id.clone(),
            display_text: item.display_prompt.clone(),
            badges: item.badges.clone(),
            created_at: item.created_at,
        })
    }

    pub fn remove_queued_prompt(&self, agent_id: &str, queue_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM agent_queued_prompts WHERE agent_id = ?1 AND queue_id = ?2",
            params![agent_id, queue_id],
        )?;
        Ok(affected > 0)
    }

    pub fn peek_queued_prompt(
        &self,
        agent_id: &str,
        queue_id: &str,
    ) -> Result<Option<StoredQueuedPrompt>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let item = conn
            .query_row(
                "SELECT queue_id, agent_id, wire_prompt, display_prompt, badges_json, created_at
                 FROM agent_queued_prompts
                 WHERE agent_id = ?1 AND queue_id = ?2",
                params![agent_id, queue_id],
                map_stored_queued_prompt_row,
            )
            .optional()?;
        Ok(item)
    }

    pub fn take_queued_prompt(
        &self,
        agent_id: &str,
        queue_id: &str,
    ) -> Result<Option<StoredQueuedPrompt>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let item = tx
            .query_row(
                "SELECT queue_id, agent_id, wire_prompt, display_prompt, badges_json, created_at
                 FROM agent_queued_prompts
                 WHERE agent_id = ?1 AND queue_id = ?2",
                params![agent_id, queue_id],
                map_stored_queued_prompt_row,
            )
            .optional()?;
        if item.is_some() {
            tx.execute(
                "DELETE FROM agent_queued_prompts WHERE agent_id = ?1 AND queue_id = ?2",
                params![agent_id, queue_id],
            )?;
        }
        tx.commit()?;
        Ok(item)
    }

    /// Read-only peek of the next FIFO queued prompt for `agent_id` without deleting it.
    /// Used by cancellable async ingestion paths so aborting mid-await never loses a queued prompt.
    pub fn peek_next_queued_prompt(
        &self,
        agent_id: &str,
    ) -> Result<Option<StoredQueuedPrompt>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT queue_id, agent_id, wire_prompt, display_prompt, badges_json, created_at
             FROM agent_queued_prompts
             WHERE agent_id = ?1
             ORDER BY sort_order ASC, rowid ASC
             LIMIT 1",
            params![agent_id],
            map_stored_queued_prompt_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn pop_next_queued_prompt(
        &self,
        agent_id: &str,
    ) -> Result<Option<StoredQueuedPrompt>, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let item = tx
            .query_row(
                "SELECT queue_id, agent_id, wire_prompt, display_prompt, badges_json, created_at
                 FROM agent_queued_prompts
                 WHERE agent_id = ?1
                 ORDER BY sort_order ASC, rowid ASC
                 LIMIT 1",
                params![agent_id],
                map_stored_queued_prompt_row,
            )
            .optional()?;
        if let Some(ref found) = item {
            tx.execute(
                "DELETE FROM agent_queued_prompts WHERE queue_id = ?1",
                params![found.queue_id],
            )?;
        }
        tx.commit()?;
        Ok(item)
    }

    pub fn clear_queued_prompts(&self, agent_id: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
        )?;
        Ok(affected)
    }

    pub fn reorder_queued_prompts(
        &self,
        agent_id: &str,
        queue_ids: &[String],
    ) -> Result<bool, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let current_ids: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT queue_id FROM agent_queued_prompts
                 WHERE agent_id = ?1
                 ORDER BY sort_order ASC, rowid ASC",
            )?;
            let rows = stmt.query_map(params![agent_id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if current_ids.len() < 2 {
            return Ok(false);
        }

        let current_set: std::collections::HashSet<&str> =
            current_ids.iter().map(String::as_str).collect();
        let mut next_order: Vec<String> = Vec::with_capacity(current_ids.len());
        let mut seen: std::collections::HashSet<&str> =
            std::collections::HashSet::with_capacity(current_ids.len());
        for qid in queue_ids {
            if current_set.contains(qid.as_str()) && seen.insert(qid.as_str()) {
                next_order.push(qid.clone());
            }
        }
        // Preserve any concurrently enqueued items not yet known to the caller at the tail.
        for qid in &current_ids {
            if seen.insert(qid.as_str()) {
                next_order.push(qid.clone());
            }
        }

        if next_order == current_ids {
            return Ok(false);
        }

        for (idx, qid) in next_order.iter().enumerate() {
            tx.execute(
                "UPDATE agent_queued_prompts SET sort_order = ?1 WHERE agent_id = ?2 AND queue_id = ?3",
                params![(idx as i64) + 1, agent_id, qid],
            )?;
        }
        tx.commit()?;
        Ok(true)
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
        fast_mode: Option<bool>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let effort_json = effort.as_ref().map(serde_json::to_string).transpose()?;
        let now = Utc::now().to_rfc3339();
        if let Some(fm) = fast_mode {
            conn.execute(
                "UPDATE agents SET model = ?1, effort_json = ?2, fast_mode = ?3, updated_at = ?4 WHERE agent_id = ?5",
                params![model, effort_json, i64::from(fm), now, agent_id],
            )?;
        } else {
            conn.execute(
                "UPDATE agents SET model = ?1, effort_json = ?2, updated_at = ?3 WHERE agent_id = ?4",
                params![model, effort_json, now, agent_id],
            )?;
        }
        Ok(())
    }

    pub fn update_agent_model(
        &self,
        agent_id: &str,
        model: Option<&str>,
        fast_mode: Option<bool>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let now = Utc::now().to_rfc3339();
        if let Some(fm) = fast_mode {
            conn.execute(
                "UPDATE agents SET model = ?1, fast_mode = ?2, updated_at = ?3 WHERE agent_id = ?4",
                params![model, i64::from(fm), now, agent_id],
            )?;
        } else {
            conn.execute(
                "UPDATE agents SET model = ?1, updated_at = ?2 WHERE agent_id = ?3",
                params![model, now, agent_id],
            )?;
        }
        Ok(())
    }

    pub fn switch_agent_provider(
        &self,
        agent_id: &str,
        provider: ProviderKind,
        model: Option<&str>,
        effort: Option<ReasoningEffort>,
        fast_mode: bool,
        acp_agent_id: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let provider_json = serde_json::to_string(&provider)?;
        let effort_json = effort.as_ref().map(serde_json::to_string).transpose()?;
        let status_json = serde_json::to_string(&AgentStatus::Idle)?;
        let clean_acp_id = if provider == ProviderKind::Acp {
            acp_agent_id
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
        } else {
            None
        };
        conn.execute(
            "UPDATE agents
             SET provider_json = ?1, model = ?2, effort_json = ?3, fast_mode = ?4,
                 persistence_ref = NULL, status_json = ?5, updated_at = ?6, acp_agent_id = ?7
             WHERE agent_id = ?8",
            params![
                provider_json,
                model,
                effort_json,
                i64::from(fast_mode),
                status_json,
                Utc::now().to_rfc3339(),
                clean_acp_id,
                agent_id
            ],
        )?;
        Ok(())
    }

    pub fn latest_user_prompt_seq(&self, agent_id: &str) -> Result<u64, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("SELECT COALESCE(MAX(seq),0) FROM timeline_events WHERE agent_id=? AND event_type='user_prompt'",[agent_id],|r|r.get(0))?)
    }

    pub fn clear_context_for_handoff(&self, agent_id: &str) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE agents SET persistence_ref=NULL WHERE agent_id=?",
            [agent_id],
        )?;
        tx.execute(
            "DELETE FROM agent_context_telemetry WHERE agent_id=?",
            [agent_id],
        )?;
        tx.commit()?;
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

    pub fn get_agent_shim_cursor(&self, agent_id: &str) -> Option<u64> {
        let key = format!("shim_cursor:{agent_id}");
        let conn = self.conn.lock().ok()?;
        let raw: Option<String> = conn
            .query_row(
                "SELECT value_json FROM host_kv WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        raw.and_then(|s| serde_json::from_str::<u64>(&s).ok())
    }

    pub fn set_agent_shim_cursor(&self, agent_id: &str, seq: u64) -> Result<(), StoreError> {
        let key = format!("shim_cursor:{agent_id}");
        let val = serde_json::to_string(&seq)?;
        let now_ms = Utc::now().timestamp_millis();
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO host_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at_ms = excluded.updated_at_ms",
            params![key, val, now_ms],
        )?;
        Ok(())
    }

    pub fn clear_agent_shim_cursor(&self, agent_id: &str) -> Result<(), StoreError> {
        let key = format!("shim_cursor:{agent_id}");
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("DELETE FROM host_kv WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Find the highest numeric segment suffix (`_N`) already persisted for an active turn across
    /// all shim-backed providers (`agy_msg_`, `pi_msg_`, `acp_msg_`, `{slug}_msg_`, and `_th_`) so a
    /// reattached driver continues segment numbering cleanly without ID collisions.
    pub fn max_turn_segment_step(&self, agent_id: &str, turn_id: &str) -> u64 {
        let Ok(conn) = self.conn.lock() else {
            return 0;
        };
        let pattern_msg = format!("item_id:%_msg_{turn_id}_%");
        let pattern_th = format!("item_id:%_th_{turn_id}_%");
        let msg_marker = format!("_msg_{turn_id}_");
        let th_marker = format!("_th_{turn_id}_");
        let Ok(mut stmt) = conn.prepare(
            "SELECT coalesce_key FROM timeline_events
             WHERE agent_id = ?1 AND (coalesce_key LIKE ?2 OR coalesce_key LIKE ?3)",
        ) else {
            return 0;
        };
        let mut max_step = 0u64;
        if let Ok(rows) = stmt.query_map(params![agent_id, pattern_msg, pattern_th], |r| {
            r.get::<_, String>(0)
        }) {
            for key in rows.flatten() {
                let Some(rest) = key.strip_prefix("item_id:") else {
                    continue;
                };
                let suffix = rest
                    .find(&msg_marker)
                    .filter(|&i| i > 0)
                    .map(|i| &rest[i + msg_marker.len()..])
                    .or_else(|| {
                        rest.find(&th_marker)
                            .filter(|&i| i > 0)
                            .map(|i| &rest[i + th_marker.len()..])
                    });
                if let Some(s) = suffix
                    && let Ok(n) = s.parse::<u64>()
                {
                    max_step = max_step.max(n);
                }
            }
        }
        max_step
    }

    /// Fetch the most recent `limit` timeline events for an agent in chronological (`seq ASC`) order.
    pub fn fetch_recent_timeline(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<Vec<TimelineEvent>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM timeline_events
             WHERE agent_id = ?1
             ORDER BY seq DESC
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
        events.reverse();
        Ok(events)
    }

    pub fn delete_agent(&self, agent_id: &str) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        search::delete_daemon_fts_for_agent(&tx, agent_id)?;
        tx.execute(
            "DELETE FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
        )?;
        tx.execute("DELETE FROM agents WHERE agent_id = ?1", params![agent_id])?;
        tx.commit()?;
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
        self.append_timeline_event_deduped(agent_id, epoch, turn_id, payload)
            .map(|(ev, _)| ev)
    }

    /// Same as [`Self::append_timeline_event`], additionally returning `mutated == false` when an
    /// identical `AssistantThinking` heartbeat matches the latest persisted row so callers can
    /// avoid redundant SQLite writes and duplicate `TimelineLive` broadcasts. Any mutated payload
    /// still executes `DELETE + INSERT` to strictly preserve monotonic `seq > since_seq`.
    pub fn append_timeline_event_deduped(
        &self,
        agent_id: &str,
        epoch: u32,
        turn_id: Option<&str>,
        mut payload: TimelinePayload,
    ) -> Result<(TimelineEvent, bool), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let now = Utc::now();
        let now_str = now.to_rfc3339();
        let mut payload_json = serde_json::to_string(&payload)?;
        let event_type = payload_event_type(&payload);
        let ckey = coalesce_key(&payload);
        if let TimelinePayload::PermissionResolved { decision, .. } = &payload {
            self.audit_insert(
                &tx,
                "permission",
                "resolve",
                "daemon",
                Some(agent_id),
                decision,
                &serde_json::Value::Null,
            )?;
        }

        if matches!(payload, TimelinePayload::AssistantThinking { .. })
            && let Some(ref key_val) = ckey
        {
            let latest_row: Option<(i64, Option<String>, String, String)> = tx
                .query_row(
                    "SELECT seq, coalesce_key, payload_json, timestamp
                     FROM timeline_events
                     WHERE agent_id = ?1
                     ORDER BY seq DESC
                     LIMIT 1",
                    params![agent_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            if let Some((latest_seq, latest_ckey, latest_json, latest_ts)) = latest_row
                && latest_ckey.as_deref() == Some(key_val.as_str())
                && latest_json == payload_json
            {
                tx.commit()?;
                return Ok((
                    TimelineEvent {
                        seq: latest_seq as u64,
                        epoch,
                        agent_id: agent_id.to_string(),
                        turn_id: turn_id.map(ToOwned::to_owned),
                        timestamp: parse_ts(&latest_ts),
                        payload,
                    },
                    false,
                ));
            }
        }

        let mut effective_now_str = now_str.clone();
        let mut effective_ts = now;

        if let Some(ref key_val) = ckey {
            let boundary_seq = query_coalesce_boundary_seq(&tx, true, agent_id, &payload, None);
            let superseded_rows: Vec<(i64, String, String)> = {
                if let Ok(mut sel_stmt) = tx.prepare_cached(
                    "SELECT seq, timestamp, payload_json FROM timeline_events WHERE agent_id = ?1 AND seq > ?2 AND coalesce_key = ?3 ORDER BY seq ASC",
                ) {
                    sel_stmt
                        .query_map(params![agent_id, boundary_seq, key_val], |r| {
                            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
                        })
                        .ok()
                        .map(|rows| rows.flatten().collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            };
            let superseded_seqs: Vec<i64> =
                superseded_rows.iter().map(|(seq, _, _)| *seq).collect();
            let is_turn_scoped_late_finalization = match &payload {
                TimelinePayload::AssistantMessage { item_id, .. }
                | TimelinePayload::AssistantThinking { item_id, .. } => {
                    !item_id.is_empty() && !is_legacy_shared_item_id(item_id)
                }
                TimelinePayload::ToolCall { call_id, .. } => !call_id.is_empty(),
                _ => false,
            };
            if is_turn_scoped_late_finalization && boundary_seq > 0 && superseded_seqs.is_empty() {
                let existing_pre_boundary: Option<(i64, u32, Option<String>, String, String)> = tx
                    .query_row(
                        "SELECT seq, epoch, turn_id, timestamp, payload_json
                         FROM timeline_events
                         WHERE agent_id = ?1 AND seq <= ?2 AND coalesce_key = ?3 AND event_type = ?4
                         ORDER BY seq DESC
                         LIMIT 1",
                        params![agent_id, boundary_seq, key_val, event_type],
                        |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, i64>(1)? as u32,
                                r.get::<_, Option<String>>(2)?,
                                r.get::<_, String>(3)?,
                                r.get::<_, String>(4)?,
                            ))
                        },
                    )
                    .optional()?;
                if let Some((
                    existing_seq,
                    existing_epoch,
                    existing_turn_id,
                    existing_ts,
                    existing_json,
                )) = existing_pre_boundary
                {
                    let turns_compatible = match (existing_turn_id.as_deref(), turn_id) {
                        (Some(existing_tid), Some(incoming_tid)) => existing_tid == incoming_tid,
                        _ => true,
                    };
                    if turns_compatible {
                        if let Ok(prev_payload) =
                            serde_json::from_str::<TimelinePayload>(&existing_json)
                            && matches!(payload, TimelinePayload::ToolCall { .. })
                        {
                            payload =
                                orbit_protocol::merge_tool_call_payload(prev_payload, payload);
                            payload_json = serde_json::to_string(&payload)?;
                        }
                        if existing_json == payload_json {
                            tx.commit()?;
                            return Ok((
                                TimelineEvent {
                                    seq: existing_seq as u64,
                                    epoch: existing_epoch,
                                    agent_id: agent_id.to_string(),
                                    turn_id: existing_turn_id
                                        .or_else(|| turn_id.map(ToOwned::to_owned)),
                                    timestamp: parse_ts(&existing_ts),
                                    payload,
                                },
                                false,
                            ));
                        }
                        tx.execute(
                            "UPDATE timeline_events SET payload_json = ?1 WHERE seq = ?2",
                            params![payload_json, existing_seq],
                        )?;
                        search::delete_daemon_fts_by_seqs(&tx, &[existing_seq])?;
                        search::sync_daemon_fts_upsert(&tx, existing_seq, agent_id, &payload)?;
                        tx.execute(
                            "UPDATE agents
                             SET timeline_epoch = timeline_epoch + 1, updated_at = ?1
                             WHERE agent_id = ?2",
                            params![now_str, agent_id],
                        )?;
                        tx.commit()?;
                        return Ok((
                            TimelineEvent {
                                seq: existing_seq as u64,
                                epoch: existing_epoch,
                                agent_id: agent_id.to_string(),
                                turn_id: existing_turn_id
                                    .or_else(|| turn_id.map(ToOwned::to_owned)),
                                timestamp: parse_ts(&existing_ts),
                                payload,
                            },
                            true,
                        ));
                    }
                }
            }
            if matches!(payload, TimelinePayload::ToolCall { .. })
                && let Some((_, earliest_ts, _)) = superseded_rows.first()
            {
                effective_now_str = earliest_ts.clone();
                effective_ts = parse_ts(earliest_ts);
                for (_, _, prev_json) in &superseded_rows {
                    if let Ok(prev_payload) = serde_json::from_str::<TimelinePayload>(prev_json) {
                        payload = orbit_protocol::merge_tool_call_payload(prev_payload, payload);
                    }
                }
                payload_json = serde_json::to_string(&payload)?;
            }
            search::delete_daemon_fts_by_seqs(&tx, &superseded_seqs)?;
            if let Ok(mut del_stmt) = tx.prepare_cached(
                "DELETE FROM timeline_events WHERE agent_id = ?1 AND seq > ?2 AND coalesce_key = ?3",
            ) {
                let _ = del_stmt.execute(params![agent_id, boundary_seq, key_val]);
            }
        }

        patch_agent_message_delivery_in_table(&tx, true, agent_id, &payload)?;

        {
            let mut ins_stmt = tx.prepare_cached(
                "INSERT INTO timeline_events (epoch, agent_id, turn_id, timestamp, payload_json, event_type, coalesce_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            ins_stmt.execute(params![
                epoch,
                agent_id,
                turn_id,
                effective_now_str,
                payload_json,
                event_type,
                ckey.as_deref()
            ])?;
        }
        let seq = tx.last_insert_rowid() as u64;
        search::sync_daemon_fts_upsert(&tx, seq as i64, agent_id, &payload)?;

        if let TimelinePayload::StatusChanged { ref status } = payload {
            let status_json = serde_json::to_string(status)?;
            let mut upd_stmt = tx.prepare_cached(
                "UPDATE agents SET latest_seq = ?1, status_json = ?2, updated_at = ?3 WHERE agent_id = ?4",
            )?;
            upd_stmt.execute(params![seq as i64, status_json, now_str, agent_id])?;
        } else {
            let mut upd_stmt = tx.prepare_cached(
                "UPDATE agents SET latest_seq = ?1, updated_at = ?2 WHERE agent_id = ?3",
            )?;
            upd_stmt.execute(params![seq as i64, now_str, agent_id])?;
        }

        tx.commit()?;

        Ok((
            TimelineEvent {
                seq,
                epoch,
                agent_id: agent_id.to_string(),
                turn_id: turn_id.map(ToOwned::to_owned),
                timestamp: effective_ts,
                payload,
            },
            true,
        ))
    }

    /// Returns true if a `UserPrompt` with `message_id` is already persisted for `agent_id`.
    pub fn has_user_prompt_message_id(
        &self,
        agent_id: &str,
        message_id: &str,
    ) -> Result<bool, StoreError> {
        if message_id.trim().is_empty() {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT payload_json
             FROM timeline_events
             WHERE agent_id = ?1
               AND event_type = 'user_prompt'",
        )?;
        let mut rows = stmt.query(params![agent_id])?;
        while let Some(row) = rows.next()? {
            let payload_str: String = row.get(0)?;
            if let Ok(TimelinePayload::UserPrompt {
                message_id: existing_id,
                ..
            }) = serde_json::from_str::<TimelinePayload>(&payload_str)
                && existing_id == message_id
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns all persisted `ToolCall` entries for `agent_id` (both foreground and background)
    /// whose latest chronological status in `timeline_events` is still `ToolCallStatus::Running`.
    pub fn list_open_running_tool_calls(
        &self,
        agent_id: &str,
    ) -> Result<Vec<(String, ToolCallDetail, Option<BackgroundTaskInfo>)>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, payload_json
             FROM timeline_events
             WHERE agent_id = ?1
               AND payload_json LIKE '%\"type\":\"tool_call\"%'
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![agent_id], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, String>(1)?))
        })?;

        let mut open_by_call: HashMap<String, (u64, ToolCallDetail, Option<BackgroundTaskInfo>)> =
            HashMap::new();
        let mut finished_task_ids: HashSet<String> = HashSet::new();

        for row in rows {
            let (seq, payload_str) = row?;
            let Ok(TimelinePayload::ToolCall {
                call_id,
                status,
                detail,
                background_task,
            }) = serde_json::from_str::<TimelinePayload>(&payload_str)
            else {
                continue;
            };
            if status == ToolCallStatus::Running {
                let already_finished = background_task
                    .as_ref()
                    .and_then(|bg| bg.task_id.as_deref())
                    .filter(|s| !s.is_empty())
                    .is_some_and(|tid| finished_task_ids.contains(tid));
                if !already_finished {
                    open_by_call.insert(call_id, (seq, detail, background_task));
                }
            } else {
                open_by_call.remove(&call_id);
                if let Some(tid) = background_task
                    .as_ref()
                    .and_then(|b| b.task_id.as_deref())
                    .filter(|s| !s.is_empty())
                {
                    finished_task_ids.insert(tid.to_string());
                    open_by_call.retain(|_, (_, _, bg)| {
                        bg.as_ref().and_then(|b| b.task_id.as_deref()) != Some(tid)
                    });
                }
            }
        }

        let mut out: Vec<(u64, String, ToolCallDetail, Option<BackgroundTaskInfo>)> = open_by_call
            .into_iter()
            .map(|(cid, (seq, det, bg))| (seq, cid, det, bg))
            .collect();
        out.sort_by_key(|(seq, _, _, _)| *seq);
        Ok(out
            .into_iter()
            .map(|(_, cid, det, bg)| (cid, det, bg))
            .collect())
    }

    /// Returns all persisted background-task `ToolCall` entries for `agent_id` whose latest
    /// chronological status in `timeline_events` is still `ToolCallStatus::Running`.
    pub fn list_running_background_tool_calls(
        &self,
        agent_id: &str,
    ) -> Result<Vec<(String, ToolCallDetail, BackgroundTaskInfo)>, StoreError> {
        Ok(self
            .list_open_running_tool_calls(agent_id)?
            .into_iter()
            .filter_map(|(cid, det, bg)| bg.map(|info| (cid, det, info)))
            .collect())
    }

    /// Reconcile stale historical `ToolCallStatus::Running` rows in place (preserving their
    /// original chronological `seq`) and bump `agents.timeline_epoch` when rows change so all
    /// connected clients refresh their replicas cleanly without appending cards to the latest turn.
    pub fn settle_timeline_tool_calls_in_place(
        &self,
        agent_id: &str,
        settlements: &[(String, ToolCallStatus)],
    ) -> Result<usize, StoreError> {
        if settlements.is_empty() {
            return Ok(0);
        }
        let target_map: HashMap<&str, ToolCallStatus> = settlements
            .iter()
            .map(|(cid, st)| (cid.as_str(), *st))
            .collect();

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;

        let mut pending_updates: Vec<(i64, String)> = Vec::new();
        {
            let mut stmt = tx.prepare(
                "SELECT seq, payload_json
                 FROM timeline_events
                 WHERE agent_id = ?1
                   AND payload_json LIKE '%\"type\":\"tool_call\"%'
                   AND payload_json LIKE '%\"status\":\"running\"%'",
            )?;
            let rows = stmt.query_map(params![agent_id], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (seq, payload_str) = row?;
                if let Ok(TimelinePayload::ToolCall {
                    call_id,
                    status: ToolCallStatus::Running,
                    detail,
                    background_task,
                }) = serde_json::from_str::<TimelinePayload>(&payload_str)
                    && let Some(&final_status) = target_map.get(call_id.as_str())
                {
                    let updated_payload = TimelinePayload::ToolCall {
                        call_id,
                        status: final_status,
                        detail,
                        background_task,
                    };
                    let updated_json = serde_json::to_string(&updated_payload)?;
                    pending_updates.push((seq, updated_json));
                }
            }
        }

        let updated_count = pending_updates.len();
        for (seq, updated_json) in pending_updates {
            tx.execute(
                "UPDATE timeline_events SET payload_json = ?1 WHERE seq = ?2",
                params![updated_json, seq],
            )?;
        }

        if updated_count > 0 {
            let now_str = Utc::now().to_rfc3339();
            tx.execute(
                "UPDATE agents
                 SET timeline_epoch = timeline_epoch + 1, updated_at = ?1
                 WHERE agent_id = ?2",
                params![now_str, agent_id],
            )?;
        }

        tx.commit()?;
        Ok(updated_count)
    }

    /// Upsert a server-sequenced timeline event into a client's local SQLite replica cache (`0ms` instant load).
    pub fn upsert_replica_timeline_event(&self, ev: &TimelineEvent) -> Result<(), StoreError> {
        self.upsert_replica_timeline_event_for_host("", ev)
    }

    /// Host-aware variant of `upsert_replica_timeline_event` that tags the FTS5 index with `host_id`.
    pub fn upsert_replica_timeline_event_for_host(
        &self,
        host_id: &str,
        ev: &TimelineEvent,
    ) -> Result<(), StoreError> {
        let seq_i64 = i64::try_from(ev.seq)
            .map_err(|_| StoreError::InvalidArgument("sequence number exceeds i64".into()))?;
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resolved_host = search::resolve_replica_host_id(&tx, host_id, &ev.agent_id);
        let payload_json = serde_json::to_string(&ev.payload)?;
        let ts_str = ev.timestamp.to_rfc3339();
        let event_type = payload_event_type(&ev.payload);
        let ckey = coalesce_key(&ev.payload);

        if let Some(ref key_val) = ckey {
            let boundary_seq =
                query_coalesce_boundary_seq(&tx, false, &ev.agent_id, &ev.payload, Some(ev.seq));
            let superseded_rowids: Vec<i64> = {
                if let Ok(mut sel_stmt) = tx.prepare_cached(
                    "SELECT rowid FROM client_replica_events
                     WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND coalesce_key = ?4",
                ) {
                    sel_stmt
                        .query_map(params![ev.agent_id, boundary_seq, seq_i64, key_val], |r| {
                            r.get::<_, i64>(0)
                        })
                        .ok()
                        .map(|rows| rows.flatten().collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            };
            search::delete_replica_fts_by_rowids(&tx, &superseded_rowids)?;
            if let Ok(mut del_stmt) = tx.prepare_cached(
                "DELETE FROM client_replica_events
                 WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND coalesce_key = ?4",
            ) {
                let _ = del_stmt.execute(params![ev.agent_id, boundary_seq, seq_i64, key_val]);
            }
        }

        patch_agent_message_delivery_in_table(&tx, false, &ev.agent_id, &ev.payload)?;

        {
            let mut ins_stmt = tx.prepare_cached(
                "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json, event_type, coalesce_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(agent_id, seq) DO UPDATE SET
                    payload_json = excluded.payload_json,
                    event_type = excluded.event_type,
                    coalesce_key = excluded.coalesce_key",
            )?;
            ins_stmt.execute(params![
                ev.agent_id,
                seq_i64,
                ev.epoch,
                ev.turn_id,
                ts_str,
                payload_json,
                event_type,
                ckey.as_deref()
            ])?;
        }
        let rowid: i64 = {
            let mut rid_stmt = tx.prepare_cached(
                "SELECT rowid FROM client_replica_events WHERE agent_id = ?1 AND seq = ?2",
            )?;
            rid_stmt.query_row(params![ev.agent_id, seq_i64], |r| r.get(0))?
        };
        search::sync_replica_fts_upsert(
            &tx,
            rowid,
            &resolved_host,
            &ev.agent_id,
            ev.seq,
            &ev.payload,
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Batch-upsert a page of replica timeline events in a single SQLite transaction.
    pub fn upsert_replica_timeline_events_batch(
        &self,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        self.upsert_replica_timeline_events_batch_for_host("", events)
    }

    /// Host-aware batch-upsert of replica timeline events in a single SQLite transaction.
    pub fn upsert_replica_timeline_events_batch_for_host(
        &self,
        host_id: &str,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        if events.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut host_cache: HashMap<String, String> = HashMap::new();
        for ev in events {
            let seq_i64 = i64::try_from(ev.seq)
                .map_err(|_| StoreError::InvalidArgument("sequence number exceeds i64".into()))?;
            let resolved_host = if !host_id.is_empty() {
                host_id.to_string()
            } else if let Some(cached) = host_cache.get(&ev.agent_id) {
                cached.clone()
            } else {
                let resolved = search::resolve_replica_host_id(&tx, "", &ev.agent_id);
                host_cache.insert(ev.agent_id.clone(), resolved.clone());
                resolved
            };
            let payload_json = serde_json::to_string(&ev.payload)?;
            let ts_str = ev.timestamp.to_rfc3339();
            let event_type = payload_event_type(&ev.payload);
            let ckey = coalesce_key(&ev.payload);
            if let Some(ref key_val) = ckey {
                let boundary_seq = query_coalesce_boundary_seq(
                    &tx,
                    false,
                    &ev.agent_id,
                    &ev.payload,
                    Some(ev.seq),
                );
                let superseded_rowids: Vec<i64> = {
                    if let Ok(mut sel_stmt) = tx.prepare_cached(
                        "SELECT rowid FROM client_replica_events
                         WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND coalesce_key = ?4",
                    ) {
                        sel_stmt
                            .query_map(params![ev.agent_id, boundary_seq, seq_i64, key_val], |r| {
                                r.get::<_, i64>(0)
                            })
                            .ok()
                            .map(|rows| rows.flatten().collect())
                            .unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                search::delete_replica_fts_by_rowids(&tx, &superseded_rowids)?;
                if let Ok(mut del_stmt) = tx.prepare_cached(
                    "DELETE FROM client_replica_events
                     WHERE agent_id = ?1 AND seq > ?2 AND seq < ?3 AND coalesce_key = ?4",
                ) {
                    let _ = del_stmt.execute(params![ev.agent_id, boundary_seq, seq_i64, key_val]);
                }
            }
            patch_agent_message_delivery_in_table(&tx, false, &ev.agent_id, &ev.payload)?;
            {
                let mut ins_stmt = tx.prepare_cached(
                    "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json, event_type, coalesce_key)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(agent_id, seq) DO UPDATE SET
                        payload_json = excluded.payload_json,
                        event_type = excluded.event_type,
                        coalesce_key = excluded.coalesce_key",
                )?;
                ins_stmt.execute(params![
                    ev.agent_id,
                    seq_i64,
                    ev.epoch,
                    ev.turn_id,
                    ts_str,
                    payload_json,
                    event_type,
                    ckey.as_deref()
                ])?;
            }
            let rowid: i64 = {
                let mut rid_stmt = tx.prepare_cached(
                    "SELECT rowid FROM client_replica_events WHERE agent_id = ?1 AND seq = ?2",
                )?;
                rid_stmt.query_row(params![ev.agent_id, seq_i64], |r| r.get(0))?
            };
            search::sync_replica_fts_upsert(
                &tx,
                rowid,
                &resolved_host,
                &ev.agent_id,
                ev.seq,
                &ev.payload,
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

    pub fn get_replica_has_older(&self, agent_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let res: Option<i64> = conn
            .query_row(
                "SELECT has_older FROM client_replica_cursors WHERE agent_id = ?1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(res.is_some_and(|v| v != 0))
    }

    pub fn set_replica_has_older(&self, agent_id: &str, has_older: bool) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO client_replica_cursors (agent_id, contiguous_seq, has_older)
             VALUES (?1, 0, ?2)
             ON CONFLICT(agent_id) DO UPDATE SET has_older = excluded.has_older",
            params![agent_id, i32::from(has_older)],
        )?;
        Ok(())
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

    pub fn clear_replica_contiguous_seq(&self, agent_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
            params![agent_id],
        )?;
        Ok(())
    }

    /// Monotonically advance `contiguous_seq` for `agent_id`.
    /// When `require_event_present` is true, verifies that `(agent_id, seq)` was actually persisted
    /// into `client_replica_events`; if missing (e.g., an earlier replica write failed due to lock
    /// contention), clears the cursor so the next open heals via a full backfill and returns `Ok(false)`.
    pub fn advance_replica_contiguous_seq(
        &self,
        agent_id: &str,
        seq: u64,
        require_event_present: bool,
    ) -> Result<bool, StoreError> {
        if seq == 0 {
            return Ok(false);
        }
        let seq_i64 = i64::try_from(seq)
            .map_err(|_| StoreError::InvalidArgument("contiguous_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        if require_event_present {
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM client_replica_events WHERE agent_id = ?1 AND seq = ?2 LIMIT 1",
                    params![agent_id, seq_i64],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_none() {
                let _ = conn.execute(
                    "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
                    params![agent_id],
                );
                return Ok(false);
            }
        }
        conn.execute(
            "INSERT INTO client_replica_cursors (agent_id, contiguous_seq)
             VALUES (?1, ?2)
             ON CONFLICT(agent_id) DO UPDATE SET
                 contiguous_seq = MAX(client_replica_cursors.contiguous_seq, excluded.contiguous_seq)",
            params![agent_id, seq_i64],
        )?;
        Ok(true)
    }

    /// Replace all cached replica events for an agent after an authoritative server-side rewind.
    pub fn replace_replica_timeline(
        &self,
        agent_id: &str,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        self.replace_replica_timeline_for_host("", agent_id, events)
    }

    /// Host-aware variant of `replace_replica_timeline` that keeps `client_timeline_fts` in sync.
    pub fn replace_replica_timeline_for_host(
        &self,
        host_id: &str,
        agent_id: &str,
        events: &[TimelineEvent],
    ) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let resolved_host = search::resolve_replica_host_id(&tx, host_id, agent_id);
        search::delete_replica_fts_for_agent(&tx, agent_id)?;
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
            let event_type = payload_event_type(&ev.payload);
            let ckey = coalesce_key(&ev.payload);
            tx.execute(
                "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json, event_type, coalesce_key)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    agent_id,
                    seq_i64,
                    ev.epoch,
                    ev.turn_id,
                    ts_str,
                    payload_json,
                    event_type,
                    ckey.as_deref()
                ],
            )?;
            let rowid = tx.last_insert_rowid();
            search::sync_replica_fts_upsert(
                &tx,
                rowid,
                &resolved_host,
                agent_id,
                ev.seq,
                &ev.payload,
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Read a contiguous slice of timeline events matching a prepared query and byte/count limits.
    fn collect_timeline_rows_budgeted<'a, P>(
        stmt: &mut rusqlite::Statement<'a>,
        params: P,
        limit: usize,
        max_bytes: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError>
    where
        P: rusqlite::Params,
    {
        let mut events = Vec::new();
        let mut approx_bytes: usize = 0;
        let mut has_more = false;
        let rows = stmt.query_map(params, |row| {
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
            if let Ok(payload) = serde_json::from_str::<TimelinePayload>(&payload_str) {
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
        }
        Ok((events, has_more))
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
        let (events, _) = Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, limit as i64],
            limit,
            usize::MAX,
        )?;
        Ok(events)
    }

    /// Fetch the newest `limit` events from the local client replica in ascending `seq` order,
    /// along with `has_older` indicating whether earlier history exists in `client_replica_events`.
    pub fn fetch_replica_timeline_tail(
        &self,
        agent_id: &str,
        limit: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        let limit = limit.max(1);
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM client_replica_events
             WHERE agent_id = ?1
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let (mut events, has_older) = Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, fetch_limit],
            limit,
            usize::MAX,
        )?;
        events.reverse();
        Ok((events, has_older))
    }

    /// Fetch up to `limit` events strictly older than `before_seq` from the local client replica
    /// in ascending `seq` order, returning `(events, has_more_older)`.
    pub fn fetch_replica_timeline_before(
        &self,
        agent_id: &str,
        before_seq: u64,
        limit: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        if before_seq <= 1 {
            return Ok((Vec::new(), false));
        }
        let limit = limit.max(1);
        let before_i64 = i64::try_from(before_seq)
            .map_err(|_| StoreError::InvalidArgument("before_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM client_replica_events
             WHERE agent_id = ?1 AND seq < ?2
             ORDER BY seq DESC
             LIMIT ?3",
        )?;
        let (mut events, has_more_older) = Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, before_i64, fetch_limit],
            limit,
            usize::MAX,
        )?;
        events.reverse();
        Ok((events, has_more_older))
    }

    pub fn has_replica_timeline_before(
        &self,
        agent_id: &str,
        before_seq: u64,
    ) -> Result<bool, StoreError> {
        if before_seq <= 1 {
            return Ok(false);
        }
        let before_i64 = i64::try_from(before_seq)
            .map_err(|_| StoreError::InvalidArgument("before_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM client_replica_events WHERE agent_id = ?1 AND seq < ?2 LIMIT 1",
                params![agent_id, before_i64],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Fetch the entire locally replicated timeline for an agent without an artificial row cap
    /// (used for full-fidelity conversation export and code-block extraction).
    pub fn fetch_replica_timeline_all(
        &self,
        agent_id: &str,
    ) -> Result<Vec<TimelineEvent>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM client_replica_events
             WHERE agent_id = ?1
             ORDER BY seq ASC",
        )?;
        let rows = stmt.query_map(params![agent_id], |row| {
            let seq = u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0);
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
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
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

    pub fn upsert_single_replica_workspace(
        &self,
        host_id: &str,
        ws: &WorkspaceSummary,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let json = serde_json::to_string(ws)?;
        conn.execute(
            "INSERT INTO client_replica_workspaces (host_id, workspace_id, payload_json)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(host_id, workspace_id) DO UPDATE SET payload_json = excluded.payload_json",
            params![host_id, ws.workspace_id, json],
        )?;
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
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing_ids: Vec<String> = {
            let mut stmt =
                tx.prepare_cached("SELECT agent_id FROM client_replica_agents WHERE host_id = ?1")?;
            let rows = stmt.query_map(params![host_id], |r| r.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let incoming_ids: HashSet<&str> = agents.iter().map(|a| a.agent_id.as_str()).collect();
        for old_id in &existing_ids {
            if !incoming_ids.contains(old_id.as_str()) {
                search::delete_replica_fts_for_agent(&tx, old_id)?;
                tx.execute(
                    "DELETE FROM client_replica_events WHERE agent_id = ?1",
                    params![old_id],
                )?;
                tx.execute(
                    "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
                    params![old_id],
                )?;
            }
        }
        tx.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        for ag in agents {
            let json = serde_json::to_string(ag)?;
            let forked_at_i64 = ag.forked_at_seq.and_then(|s| i64::try_from(s).ok());
            tx.execute(
                "INSERT INTO client_replica_agents (
                    host_id, agent_id, payload_json, forked_from_agent_id, forked_at_seq, fast_mode, acp_agent_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    host_id,
                    ag.agent_id,
                    json,
                    ag.forked_from_agent_id,
                    forked_at_i64,
                    i32::from(ag.fast_mode),
                    ag.acp_agent_id,
                ],
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
        let forked_at_i64 = ag.forked_at_seq.and_then(|s| i64::try_from(s).ok());
        conn.execute(
            "INSERT INTO client_replica_agents (
                host_id, agent_id, payload_json, forked_from_agent_id, forked_at_seq, fast_mode, acp_agent_id
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                payload_json = excluded.payload_json,
                forked_from_agent_id = excluded.forked_from_agent_id,
                forked_at_seq = excluded.forked_at_seq,
                fast_mode = excluded.fast_mode,
                acp_agent_id = excluded.acp_agent_id",
            params![
                host_id,
                ag.agent_id,
                json,
                ag.forked_from_agent_id,
                forked_at_i64,
                i32::from(ag.fast_mode),
                ag.acp_agent_id,
            ],
        )?;
        Ok(())
    }

    pub fn delete_replica_agent(&self, host_id: &str, agent_id: &str) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        search::delete_replica_fts_for_agent(&tx, agent_id)?;
        tx.execute(
            "DELETE FROM client_replica_events WHERE agent_id = ?1",
            params![agent_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
            params![agent_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1 AND agent_id = ?2",
            params![host_id, agent_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn list_replica_agents(&self, host_id: &str) -> Result<Vec<AgentSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT payload_json, fast_mode, acp_agent_id FROM client_replica_agents WHERE host_id = ?1",
        )?;
        let rows = stmt.query_map(params![host_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i32>(1).unwrap_or(0) != 0,
                r.get::<_, Option<String>>(2).unwrap_or(None),
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (raw_json, fast_col, col_acp_id) = r?;
            if let Ok(mut ag) = serde_json::from_str::<AgentSummary>(&raw_json) {
                if fast_col {
                    ag.fast_mode = true;
                }
                if ag.acp_agent_id.is_none() {
                    ag.acp_agent_id = col_acp_id;
                }
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

    /// Return the latest non-empty `AssistantMessage` text for `agent_id` by scanning from the
    /// newest timeline sequence downward (works even when timelines exceed 500+ events).
    pub fn latest_assistant_message(&self, agent_id: &str) -> Result<Option<String>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT payload_json
             FROM timeline_events
             WHERE agent_id = ?1
               AND (event_type = 'assistant_message' OR event_type IS NULL)
             ORDER BY seq DESC
             LIMIT 200",
        )?;
        let mut rows = stmt.query(params![agent_id])?;
        while let Some(row) = rows.next()? {
            let payload_json: String = row.get(0)?;
            if let Ok(TimelinePayload::AssistantMessage { text, .. }) =
                serde_json::from_str::<TimelinePayload>(&payload_json)
            {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Ok(Some(trimmed.to_string()));
                }
            }
        }
        Ok(None)
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

        Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, since_seq as i64, fetch_limit],
            limit,
            max_bytes,
        )
    }

    /// Read user-prompt anchors in chronological order using the agent/type/sequence index.
    pub fn fetch_timeline_outline_prompts(
        &self,
        agent_id: &str,
    ) -> Result<Vec<TimelineOutlinePrompt>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare_cached(
            "SELECT seq, payload_json FROM timeline_events
             WHERE agent_id = ?1 AND event_type = 'user_prompt' ORDER BY seq ASC",
        )?;
        Self::collect_outline_prompts(&mut stmt, agent_id)
    }

    pub fn has_prompt_history(&self, agent_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM timeline_events WHERE agent_id=?1 AND event_type IN ('user_prompt','agent_message'))",
            [agent_id], |row| row.get(0))?)
    }

    /// Read user-prompt anchors from the local client replica in chronological order using
    /// `idx_replica_agent_type_seq` so tail-hydrated stages retain pre-tail outline anchors.
    pub fn fetch_replica_outline_prompts(
        &self,
        agent_id: &str,
    ) -> Result<Vec<TimelineOutlinePrompt>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare_cached(
            "SELECT seq, payload_json FROM client_replica_events
             WHERE agent_id = ?1 AND event_type = 'user_prompt' ORDER BY seq ASC",
        )?;
        Self::collect_outline_prompts(&mut stmt, agent_id)
    }

    fn collect_outline_prompts(
        stmt: &mut rusqlite::CachedStatement<'_>,
        agent_id: &str,
    ) -> Result<Vec<TimelineOutlinePrompt>, StoreError> {
        let mut rows = stmt.query([agent_id])?;
        let mut prompts = Vec::new();
        while let Some(row) = rows.next()? {
            let seq = u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0);
            if seq == 0 {
                continue;
            }
            let json = row.get_ref(1)?.as_str().map_err(rusqlite::Error::from)?;
            if let Ok(TimelinePayload::UserPrompt { text, badges, .. }) = serde_json::from_str(json)
                && let Some(prompt) = TimelineOutlinePrompt::from_user_prompt(seq, &text, &badges)
            {
                prompts.push(prompt);
            }
        }
        Ok(prompts)
    }

    /// Fetch the newest timeline events (tail-first) for `agent_id`, bounded by `limit` and
    /// `max_bytes`, returning `(ascending_events, has_older)`.
    pub fn fetch_timeline_tail_budgeted(
        &self,
        agent_id: &str,
        limit: usize,
        max_bytes: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        let limit = limit.max(1);
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM timeline_events
             WHERE agent_id = ?1
             ORDER BY seq DESC
             LIMIT ?2",
        )?;
        let (mut events, has_older) = Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, fetch_limit],
            limit,
            max_bytes,
        )?;
        events.reverse();
        Ok((events, has_older))
    }

    /// Fetch timeline events strictly older than `before_seq` bounded by `limit` and `max_bytes`,
    /// returning `(ascending_events, has_more_older)`.
    pub fn fetch_timeline_before_budgeted(
        &self,
        agent_id: &str,
        before_seq: u64,
        limit: usize,
        max_bytes: usize,
    ) -> Result<(Vec<TimelineEvent>, bool), StoreError> {
        if before_seq <= 1 {
            return Ok((Vec::new(), false));
        }
        let limit = limit.max(1);
        let before_i64 = i64::try_from(before_seq)
            .map_err(|_| StoreError::InvalidArgument("before_seq exceeds i64".into()))?;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let fetch_limit = (limit.saturating_add(1)) as i64;
        let mut stmt = conn.prepare(
            "SELECT seq, epoch, agent_id, turn_id, timestamp, payload_json
             FROM timeline_events
             WHERE agent_id = ?1 AND seq < ?2
             ORDER BY seq DESC
             LIMIT ?3",
        )?;
        let (mut events, has_more_older) = Self::collect_timeline_rows_budgeted(
            &mut stmt,
            params![agent_id, before_i64, fetch_limit],
            limit,
            max_bytes,
        )?;
        events.reverse();
        Ok((events, has_more_older))
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

        search::delete_daemon_fts_from_seq(&tx, agent_id, to_before_i64)?;
        tx.execute(
            "DELETE FROM timeline_events WHERE agent_id = ?1 AND seq >= ?2",
            params![agent_id, to_before_i64],
        )?;
        tx.execute(
            "DELETE FROM turn_checkpoints WHERE agent_id = ?1 AND prompt_seq >= ?2",
            params![agent_id, to_before_i64],
        )?;
        tx.execute(
            "DELETE FROM agent_queued_prompts WHERE agent_id = ?1",
            params![agent_id],
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

        let select_sql = format!("SELECT {AGENT_SELECT_COLS} FROM agents WHERE agent_id = ?1");
        let agent = tx
            .query_row(&select_sql, params![agent_id], map_agent_row)
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

    /// Result returned by [`OrbitStore::clone_timeline_events_up_to`].
    ///
    /// Computed atomically inside the same `IMMEDIATE` transaction as the copy so callers
    /// never suffer a TOCTOU race deciding whether the full tail was cloned.
    pub fn clone_timeline_events_up_to(
        &self,
        source_agent_id: &str,
        target_agent_id: &str,
        up_to_seq: Option<u64>,
    ) -> Result<ClonedTimelineOutcome, StoreError> {
        if source_agent_id == target_agent_id {
            return Err(StoreError::InvalidArgument(
                "source_agent_id and target_agent_id must be distinct".into(),
            ));
        }
        let upper_i64 = up_to_seq
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StoreError::InvalidArgument("up_to_seq exceeds i64".into()))?;

        struct RawClonedRow {
            seq: u64,
            epoch: u32,
            turn_id: Option<String>,
            timestamp: String,
            payload: TimelinePayload,
        }

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let source_max_seq_i64: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM timeline_events WHERE agent_id = ?1",
            params![source_agent_id],
            |r| r.get(0),
        )?;
        let source_max_seq = u64::try_from(source_max_seq_i64).unwrap_or(0);
        let cloned_full_tail = up_to_seq.is_none_or(|u| u >= source_max_seq);

        let raw_rows: Vec<RawClonedRow> = {
            let mut stmt = tx.prepare(
                "SELECT seq, epoch, turn_id, timestamp, payload_json
                 FROM timeline_events
                 WHERE agent_id = ?1 AND (?2 IS NULL OR seq <= ?2)
                 ORDER BY seq ASC",
            )?;
            let mapped = stmt.query_map(params![source_agent_id, upper_i64], |row| {
                let seq_i64: i64 = row.get(0)?;
                let epoch: u32 = row.get(1)?;
                let turn_id: Option<String> = row.get(2)?;
                let timestamp: String = row.get(3)?;
                let payload_json: String = row.get(4)?;
                Ok((seq_i64, epoch, turn_id, timestamp, payload_json))
            })?;
            let mut out = Vec::new();
            for item in mapped {
                let (seq_i64, epoch, turn_id, timestamp, payload_json) = item?;
                let payload: TimelinePayload = serde_json::from_str(&payload_json)?;
                out.push(RawClonedRow {
                    seq: u64::try_from(seq_i64).unwrap_or(0),
                    epoch,
                    turn_id,
                    timestamp,
                    payload,
                });
            }
            out
        };

        let effective_source_seq = raw_rows.last().map(|r| r.seq).filter(|&s| s > 0);

        // Scan the selected prefix to normalize unfinished in-flight states cleanly:
        // - Unresolved PermissionRequest / QuestionRequest rows are stripped.
        // - ToolCall rows still Running at the end of the prefix are settled as Failed with
        //   `background_task: None` so forked agents never hijack parent background tasks.
        // - Streaming AssistantMessage rows are finalized (`streaming: false`) so they render
        //   cleanly and are indexed in FTS5.
        // - Trailing transient StatusChanged rows (Running/Initializing/AwaitingPermission/
        //   AutonomousWakeup) are dropped.
        let mut resolved_perm_ids = std::collections::HashSet::<String>::new();
        let mut resolved_question_ids = std::collections::HashSet::<String>::new();
        let mut final_tool_status = std::collections::HashMap::<String, ToolCallStatus>::new();
        for r in &raw_rows {
            match &r.payload {
                TimelinePayload::PermissionResolved { request_id, .. } => {
                    resolved_perm_ids.insert(request_id.clone());
                }
                TimelinePayload::QuestionResolved { request_id, .. } => {
                    resolved_question_ids.insert(request_id.clone());
                }
                TimelinePayload::ToolCall {
                    call_id, status, ..
                } => {
                    final_tool_status.insert(call_id.clone(), *status);
                }
                _ => {}
            }
        }

        let mut normalized: Vec<RawClonedRow> = Vec::with_capacity(raw_rows.len());
        for mut r in raw_rows {
            match &mut r.payload {
                TimelinePayload::PermissionRequest { request_id, .. }
                    if !resolved_perm_ids.contains(request_id) =>
                {
                    continue;
                }
                TimelinePayload::QuestionRequest { request_id, .. }
                    if !resolved_question_ids.contains(request_id) =>
                {
                    continue;
                }
                TimelinePayload::ToolCall {
                    call_id,
                    status,
                    background_task,
                    ..
                } => {
                    if *status == ToolCallStatus::Running
                        && final_tool_status.get(call_id) == Some(&ToolCallStatus::Running)
                    {
                        *status = ToolCallStatus::Failed;
                        *background_task = None;
                    }
                }
                TimelinePayload::AssistantMessage { streaming, .. } if *streaming => {
                    *streaming = false;
                }
                _ => {}
            }
            normalized.push(r);
        }

        while let Some(last) = normalized.last() {
            let is_transient_status = matches!(
                &last.payload,
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Running { .. }
                        | AgentStatus::Initializing
                        | AgentStatus::AwaitingPermission { .. }
                        | AgentStatus::AutonomousWakeup { .. }
                }
            );
            if is_transient_status {
                normalized.pop();
            } else {
                break;
            }
        }

        let mut new_latest_seq_i64: i64 = 0;
        let mut copied_count: usize = 0;

        for row in normalized {
            let payload_json = serde_json::to_string(&row.payload)?;
            let eff_type = payload_event_type(&row.payload);
            let eff_ckey = crate::coalesce_key(&row.payload);

            {
                let mut ins_stmt = tx.prepare_cached(
                    "INSERT INTO timeline_events (
                        epoch, agent_id, turn_id, timestamp, payload_json, event_type, coalesce_key
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                ins_stmt.execute(params![
                    row.epoch,
                    target_agent_id,
                    row.turn_id,
                    row.timestamp,
                    payload_json,
                    eff_type,
                    eff_ckey.as_deref(),
                ])?;
            }
            let new_seq_i64 = tx.last_insert_rowid();
            search::sync_daemon_fts_upsert(&tx, new_seq_i64, target_agent_id, &row.payload)?;
            new_latest_seq_i64 = new_seq_i64;
            copied_count += 1;
        }

        let effective_source_seq_i64 = effective_source_seq
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StoreError::InvalidArgument("effective_source_seq exceeds i64".into()))?;
        let now_str = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE agents
             SET latest_seq = MAX(latest_seq, ?1),
                 read_seq = MAX(read_seq, ?1),
                 forked_at_seq = ?2,
                 updated_at = ?3
             WHERE agent_id = ?4",
            params![
                new_latest_seq_i64,
                effective_source_seq_i64,
                now_str,
                target_agent_id
            ],
        )?;

        tx.commit()?;
        Ok(ClonedTimelineOutcome {
            new_latest_seq: u64::try_from(new_latest_seq_i64).unwrap_or(0),
            copied_count,
            cloned_full_tail,
            effective_source_seq,
        })
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
                host_id, display_name, daemon_pubkey_b64, direct_endpoints_json, relay_url, pairing_secret, meta_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(host_id) DO UPDATE SET
                display_name = excluded.display_name,
                daemon_pubkey_b64 = excluded.daemon_pubkey_b64,
                direct_endpoints_json = excluded.direct_endpoints_json,
                relay_url = excluded.relay_url,
                pairing_secret = excluded.pairing_secret,
                meta_json = excluded.meta_json,
                updated_at = excluded.updated_at",
            params![
                host.host_id,
                host.display_name,
                host.daemon_pubkey_b64,
                endpoints_json,
                host.relay_url,
                self.secrets.seal(&format!("pairing_secret/{}", host.host_id), &host.pairing_secret)?,
                meta_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_client_hosts(&self) -> Result<Vec<StoredHostProfile>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT host_id, display_name, daemon_pubkey_b64, direct_endpoints_json, relay_url, meta_json, pairing_secret
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
                pairing_secret: row.get(6)?,
                meta,
            })
        })?;
        rows.map(|row| {
            let mut host = row?;
            host.pairing_secret = self.secrets.open(
                &format!("pairing_secret/{}", host.host_id),
                &host.pairing_secret,
            )?;
            Ok(host)
        })
        .collect()
    }

    pub fn delete_client_host(&self, host_id: &str) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM client_pending_outbox WHERE host_id=?1",
            [host_id],
        )?;
        search::delete_replica_fts_and_events_for_host(&tx, host_id)?;
        tx.execute(
            "DELETE FROM client_hosts WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_projects WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_workspaces WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_thread_prefs WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_host_replica(&self, host_id: &str) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        search::delete_replica_fts_and_events_for_host(&tx, host_id)?;
        tx.execute(
            "DELETE FROM client_replica_workspaces WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_agents WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.execute(
            "DELETE FROM client_thread_prefs WHERE host_id = ?1",
            params![host_id],
        )?;
        tx.commit()?;
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

    pub fn list_client_thread_prefs(&self) -> Result<Vec<ClientThreadPref>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             FROM client_thread_prefs
             ORDER BY pinned_order DESC, updated_at_ms DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ClientThreadPref {
                host_id: row.get(0)?,
                agent_id: row.get(1)?,
                seen_seq: row.get::<_, i64>(2)?.max(0) as u64,
                is_pinned: row.get::<_, i64>(3)? != 0,
                pinned_order: row.get(4)?,
                updated_at_ms: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn upsert_client_thread_pref(&self, pref: &ClientThreadPref) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO client_thread_prefs (
                host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                seen_seq = excluded.seen_seq,
                is_pinned = excluded.is_pinned,
                pinned_order = excluded.pinned_order,
                updated_at_ms = excluded.updated_at_ms",
            params![
                pref.host_id,
                pref.agent_id,
                pref.seen_seq as i64,
                i32::from(pref.is_pinned),
                pref.pinned_order,
                pref.updated_at_ms,
            ],
        )?;
        Ok(())
    }

    pub fn set_client_thread_seen_seq(
        &self,
        host_id: &str,
        agent_id: &str,
        seen_seq: u64,
        force_reset: bool,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = if force_reset {
            "INSERT INTO client_thread_prefs (
                host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             ) VALUES (?1, ?2, ?3, 0, 0, ?4)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                seen_seq = excluded.seen_seq,
                updated_at_ms = excluded.updated_at_ms"
        } else {
            "INSERT INTO client_thread_prefs (
                host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             ) VALUES (?1, ?2, ?3, 0, 0, ?4)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                seen_seq = MAX(client_thread_prefs.seen_seq, excluded.seen_seq),
                updated_at_ms = MAX(client_thread_prefs.updated_at_ms, excluded.updated_at_ms)"
        };
        conn.execute(sql, params![host_id, agent_id, seen_seq as i64, now_ms])?;
        Ok(())
    }

    pub fn set_client_thread_pinned(
        &self,
        host_id: &str,
        agent_id: &str,
        is_pinned: bool,
        pinned_order: i64,
        now_ms: i64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "INSERT INTO client_thread_prefs (
                host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             ) VALUES (?1, ?2, 0, ?3, ?4, ?5)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                is_pinned = excluded.is_pinned,
                pinned_order = excluded.pinned_order,
                updated_at_ms = excluded.updated_at_ms",
            params![
                host_id,
                agent_id,
                i32::from(is_pinned),
                pinned_order,
                now_ms,
            ],
        )?;
        Ok(())
    }

    pub fn migrate_client_thread_pref(
        &self,
        host_id: &str,
        old_agent_id: &str,
        new_agent_id: &str,
    ) -> Result<(), StoreError> {
        if old_agent_id == new_agent_id {
            return Ok(());
        }
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;

        let read_pref = |tx: &rusqlite::Transaction<'_>,
                         aid: &str|
         -> Result<Option<ClientThreadPref>, StoreError> {
            tx.query_row(
                "SELECT host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
                 FROM client_thread_prefs
                 WHERE host_id = ?1 AND agent_id = ?2",
                params![host_id, aid],
                |row| {
                    Ok(ClientThreadPref {
                        host_id: row.get(0)?,
                        agent_id: row.get(1)?,
                        seen_seq: row.get::<_, i64>(2)?.max(0) as u64,
                        is_pinned: row.get::<_, i64>(3)? != 0,
                        pinned_order: row.get(4)?,
                        updated_at_ms: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        };

        let Some(old_pref) = read_pref(&tx, old_agent_id)? else {
            return Ok(());
        };
        let new_pref = read_pref(&tx, new_agent_id)?;
        let merged = match new_pref {
            Some(existing) => ClientThreadPref {
                host_id: host_id.to_string(),
                agent_id: new_agent_id.to_string(),
                seen_seq: existing.seen_seq.max(old_pref.seen_seq),
                is_pinned: existing.is_pinned || old_pref.is_pinned,
                pinned_order: match (existing.is_pinned, old_pref.is_pinned) {
                    (true, true) => existing.pinned_order.max(old_pref.pinned_order),
                    (true, false) => existing.pinned_order,
                    (false, true) => old_pref.pinned_order,
                    (false, false) => 0,
                },
                updated_at_ms: existing.updated_at_ms.max(old_pref.updated_at_ms),
            },
            None => ClientThreadPref {
                host_id: host_id.to_string(),
                agent_id: new_agent_id.to_string(),
                seen_seq: old_pref.seen_seq,
                is_pinned: old_pref.is_pinned,
                pinned_order: old_pref.pinned_order,
                updated_at_ms: old_pref.updated_at_ms,
            },
        };

        tx.execute(
            "INSERT INTO client_thread_prefs (
                host_id, agent_id, seen_seq, is_pinned, pinned_order, updated_at_ms
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(host_id, agent_id) DO UPDATE SET
                seen_seq = excluded.seen_seq,
                is_pinned = excluded.is_pinned,
                pinned_order = excluded.pinned_order,
                updated_at_ms = excluded.updated_at_ms",
            params![
                merged.host_id,
                merged.agent_id,
                merged.seen_seq as i64,
                i32::from(merged.is_pinned),
                merged.pinned_order,
                merged.updated_at_ms,
            ],
        )?;
        tx.execute(
            "DELETE FROM client_thread_prefs WHERE host_id = ?1 AND agent_id = ?2",
            params![host_id, old_agent_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_client_thread_pref(
        &self,
        host_id: &str,
        agent_id: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_thread_prefs WHERE host_id = ?1 AND agent_id = ?2",
            params![host_id, agent_id],
        )?;
        Ok(())
    }

    pub fn delete_client_thread_prefs_for_host(&self, host_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "DELETE FROM client_thread_prefs WHERE host_id = ?1",
            params![host_id],
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
        search::clear_daemon_fts(&conn)?;
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
    is_daemon_table: bool,
    agent_id: &str,
    payload: &TimelinePayload,
    upper_seq: Option<u64>,
) -> i64 {
    let stop_at_tool = matches!(
        payload,
        TimelinePayload::AssistantMessage { .. } | TimelinePayload::AssistantThinking { .. }
    );
    let sql = match (is_daemon_table, stop_at_tool, upper_seq.is_some()) {
        (true, true, true) => {
            "SELECT COALESCE(MAX(seq), 0) FROM timeline_events
             WHERE agent_id = ?1 AND seq < ?2
               AND event_type IN ('user_prompt', 'agent_message', 'tool_call')"
        }
        (true, true, false) => {
            "SELECT COALESCE(MAX(seq), 0) FROM timeline_events
             WHERE agent_id = ?1
               AND event_type IN ('user_prompt', 'agent_message', 'tool_call')"
        }
        (true, false, true) => {
            "SELECT COALESCE(MAX(seq), 0) FROM timeline_events
             WHERE agent_id = ?1 AND seq < ?2
               AND event_type IN ('user_prompt', 'agent_message')"
        }
        (true, false, false) => {
            "SELECT COALESCE(MAX(seq), 0) FROM timeline_events
             WHERE agent_id = ?1
               AND event_type IN ('user_prompt', 'agent_message')"
        }
        (false, true, true) => {
            "SELECT COALESCE(MAX(seq), 0) FROM client_replica_events
             WHERE agent_id = ?1 AND seq < ?2
               AND event_type IN ('user_prompt', 'agent_message', 'tool_call')"
        }
        (false, true, false) => {
            "SELECT COALESCE(MAX(seq), 0) FROM client_replica_events
             WHERE agent_id = ?1
               AND event_type IN ('user_prompt', 'agent_message', 'tool_call')"
        }
        (false, false, true) => {
            "SELECT COALESCE(MAX(seq), 0) FROM client_replica_events
             WHERE agent_id = ?1 AND seq < ?2
               AND event_type IN ('user_prompt', 'agent_message')"
        }
        (false, false, false) => {
            "SELECT COALESCE(MAX(seq), 0) FROM client_replica_events
             WHERE agent_id = ?1
               AND event_type IN ('user_prompt', 'agent_message')"
        }
    };

    let Ok(mut stmt) = tx.prepare_cached(sql) else {
        return 0;
    };
    if let Some(max_s) = upper_seq {
        stmt.query_row(params![agent_id, max_s as i64], |r| r.get(0))
            .unwrap_or(0)
    } else {
        stmt.query_row(params![agent_id], |r| r.get(0)).unwrap_or(0)
    }
}

fn patch_agent_message_delivery_in_table(
    tx: &rusqlite::Transaction<'_>,
    is_daemon_table: bool,
    agent_id: &str,
    delivery_payload: &TimelinePayload,
) -> Result<(), StoreError> {
    let TimelinePayload::AgentMessageDeliveryUpdated {
        message_id,
        status,
        delivery_turn_id,
        blocking_turn_id,
        timestamp_ms,
    } = delivery_payload
    else {
        return Ok(());
    };
    if message_id.trim().is_empty() {
        return Ok(());
    }
    let select_sql = if is_daemon_table {
        "SELECT seq, payload_json
         FROM timeline_events
         WHERE agent_id = ?1
           AND event_type = 'agent_message'
           AND json_extract(payload_json, '$.message_id') = ?2
         ORDER BY seq DESC
         LIMIT 1"
    } else {
        "SELECT seq, payload_json
         FROM client_replica_events
         WHERE agent_id = ?1
           AND event_type = 'agent_message'
           AND json_extract(payload_json, '$.message_id') = ?2
         ORDER BY seq DESC
         LIMIT 1"
    };
    let existing: Option<(i64, String)> = tx
        .prepare_cached(select_sql)?
        .query_row(params![agent_id, message_id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((target_seq, raw_json)) = existing else {
        return Ok(());
    };
    let Ok(mut target_payload) = serde_json::from_str::<TimelinePayload>(&raw_json) else {
        return Ok(());
    };
    if let TimelinePayload::AgentMessage {
        ref mut delivery_status,
        delivery_turn_id: ref mut d_turn,
        blocking_turn_id: ref mut b_turn,
        ref mut dispatched_at_ms,
        ref mut settled_at_ms,
        ..
    } = target_payload
    {
        *delivery_status = Some(*status);
        if let Some(tid) = delivery_turn_id {
            *d_turn = Some(tid.clone());
        }
        match status {
            orbit_protocol::AgentMessageDeliveryStatus::Queued => {
                if let Some(btid) = blocking_turn_id {
                    *b_turn = Some(btid.clone());
                }
            }
            orbit_protocol::AgentMessageDeliveryStatus::Dispatched => {
                *b_turn = None;
                if dispatched_at_ms.is_none() {
                    *dispatched_at_ms = Some(*timestamp_ms);
                }
            }
            orbit_protocol::AgentMessageDeliveryStatus::Settled => {
                *b_turn = None;
                if dispatched_at_ms.is_none() {
                    *dispatched_at_ms = Some(*timestamp_ms);
                }
                *settled_at_ms = Some(*timestamp_ms);
            }
            orbit_protocol::AgentMessageDeliveryStatus::Dropped => {
                *b_turn = None;
                *settled_at_ms = Some(*timestamp_ms);
            }
        }
        let updated_json = serde_json::to_string(&target_payload)?;
        let update_sql = if is_daemon_table {
            "UPDATE timeline_events SET payload_json = ?1 WHERE agent_id = ?2 AND seq = ?3"
        } else {
            "UPDATE client_replica_events SET payload_json = ?1 WHERE agent_id = ?2 AND seq = ?3"
        };
        tx.prepare_cached(update_sql)?
            .execute(params![updated_json, agent_id, target_seq])?;
    }
    Ok(())
}

fn payload_event_type(payload: &TimelinePayload) -> &'static str {
    match payload {
        TimelinePayload::UserPrompt { .. } => "user_prompt",
        TimelinePayload::AssistantThinking { .. } => "assistant_thinking",
        TimelinePayload::AssistantMessage { .. } => "assistant_message",
        TimelinePayload::ToolCall { .. } => "tool_call",
        TimelinePayload::PermissionRequest { .. } => "permission_request",
        TimelinePayload::PermissionResolved { .. } => "permission_resolved",
        TimelinePayload::QuestionRequest { .. } => "question_request",
        TimelinePayload::QuestionResolved { .. } => "question_resolved",
        TimelinePayload::Notice { .. } => "notice",
        TimelinePayload::AgentMessage { .. } => "agent_message",
        TimelinePayload::AgentMessageDeliveryUpdated { .. } => "agent_message_delivery_updated",
        TimelinePayload::StatusChanged { .. } => "status_changed",
    }
}

fn is_legacy_shared_item_id(raw_id: &str) -> bool {
    matches!(
        raw_id,
        "agy_msg_default" | "agy_thought_default" | "codex_msg" | "codex_rsn" | "item"
    )
}

fn coalesce_key(payload: &TimelinePayload) -> Option<String> {
    coalesce_identity(payload).map(|(k, v)| format!("{k}:{v}"))
}

fn coalesce_identity(payload: &TimelinePayload) -> Option<(&'static str, &str)> {
    match payload {
        TimelinePayload::AssistantMessage { item_id, .. }
        | TimelinePayload::AssistantThinking { item_id, .. }
            if !item_id.is_empty() =>
        {
            Some(("item_id", item_id.as_str()))
        }
        TimelinePayload::ToolCall { call_id, .. } if !call_id.is_empty() => {
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
        mission_goal: row.get(12).unwrap_or(None),
        project_root: row.get(13).unwrap_or(None),
        board_revision: row.get::<_, i64>(14).unwrap_or(0).max(0) as u64,
        title_locked: row.get::<_, i32>(15).unwrap_or(0) != 0,
        auto_clustered: row.get::<_, i32>(16).unwrap_or(0) != 0,
        is_curated: row.get::<_, i32>(17).unwrap_or(0) != 0,
    })
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool, StoreError> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name.eq_ignore_ascii_case(column) {
            return Ok(true);
        }
    }
    Ok(false)
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
    let timeline_epoch: u64 = row.get::<_, i64>(21).unwrap_or(0).max(0) as u64;
    let read_seq: u64 = row.get::<_, i64>(22).unwrap_or(0).max(0) as u64;
    let is_pinned: bool = row.get::<_, i32>(23).unwrap_or(0) != 0;
    let pinned_order: i64 = row.get::<_, i64>(24).unwrap_or(0);
    let handle: Option<String> = row.get(25).unwrap_or(None);
    let cluster_suggestion_opt: Option<String> = row.get(26).unwrap_or(None);
    let task_summary: Option<String> = row.get(27).unwrap_or(None);
    let forked_from_agent_id: Option<String> = row.get(28).unwrap_or(None);
    let forked_at_seq: Option<u64> = row
        .get::<_, Option<i64>>(29)
        .unwrap_or(None)
        .filter(|&v| v >= 0)
        .map(|v| v as u64);
    let fast_mode: bool = row.get::<_, i32>(30).unwrap_or(0) != 0;
    let acp_agent_id: Option<String> = row.get(31).unwrap_or(None);

    let provider: ProviderKind =
        serde_json::from_str(&provider_str).unwrap_or(ProviderKind::Antigravity);
    let effort: Option<ReasoningEffort> = effort_opt_str
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::Default);
    let status: AgentStatus = serde_json::from_str(&status_str).unwrap_or(AgentStatus::Closed);
    let labels: HashMap<String, String> = serde_json::from_str(&labels_str).unwrap_or_default();
    let attention = crate::workspace_sync::attention_state_from_str(&attention_str);
    let cluster_suggestion: Option<WorkspaceClusterSuggestion> = cluster_suggestion_opt
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    Ok(AgentSummary {
        agent_id: row.get(0)?,
        workspace_id: row.get(1)?,
        cwd: row.get(2)?,
        title: row.get(3)?,
        provider,
        model: row.get(5)?,
        effort,
        fast_mode,
        mode,
        status,
        parent_agent_id: row.get(8)?,
        parent_host_id,
        latest_seq: row.get::<_, i64>(9)? as u64,
        timeline_epoch,
        created_at: parse_ts(&created_str),
        updated_at: parse_ts(&updated_str),
        archived_at: archived_opt.as_deref().map(parse_ts),
        background,
        auto_archive,
        profile_id,
        labels,
        attention,
        last_activity_at: last_activity_opt.as_deref().map(parse_ts),
        queued_prompts: Vec::new(),
        read_seq,
        is_pinned,
        pinned_order,
        handle,
        cluster_suggestion,
        task_summary,
        forked_from_agent_id,
        forked_at_seq,
        acp_agent_id,
    })
}

/// Idempotently add a nullable/defaulted column by inspecting `PRAGMA table_info(<table>)` first.
/// Never bumps `PRAGMA user_version` so parallel feature branches merge cleanly in any order.
pub(crate) fn ensure_table_column(
    conn: &Connection,
    table: &str,
    column: &str,
    column_def: &str,
) -> Result<(), StoreError> {
    if !table_has_column(conn, table, column)? {
        let alter_sql = format!("ALTER TABLE {table} ADD COLUMN {column} {column_def}");
        conn.execute(&alter_sql, [])?;
    }
    Ok(())
}

fn map_stored_queued_prompt_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredQueuedPrompt> {
    let badges_str: String = row.get(4)?;
    let created_str: String = row.get(5)?;
    let badges: Vec<PromptBadge> = serde_json::from_str(&badges_str).unwrap_or_default();
    Ok(StoredQueuedPrompt {
        queue_id: row.get(0)?,
        agent_id: row.get(1)?,
        wire_prompt: row.get(2)?,
        display_prompt: row.get(3)?,
        badges,
        created_at: parse_ts(&created_str),
    })
}

pub(crate) fn load_queued_prompts_for_agent(
    conn: &Connection,
    agent_id: &str,
) -> Result<Vec<QueuedPromptView>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT queue_id, display_prompt, badges_json, created_at
         FROM agent_queued_prompts
         WHERE agent_id = ?1
         ORDER BY sort_order ASC, rowid ASC",
    )?;
    let rows = stmt
        .query_map(params![agent_id], |row| {
            let badges_str: String = row.get(2)?;
            let created_str: String = row.get(3)?;
            Ok(QueuedPromptView {
                queue_id: row.get(0)?,
                display_text: row.get(1)?,
                badges: serde_json::from_str(&badges_str).unwrap_or_default(),
                created_at: parse_ts(&created_str),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(crate) fn load_all_queued_prompts_grouped(
    conn: &Connection,
) -> Result<HashMap<String, Vec<QueuedPromptView>>, StoreError> {
    let mut stmt = conn.prepare_cached(
        "SELECT agent_id, queue_id, display_prompt, badges_json, created_at
         FROM agent_queued_prompts
         ORDER BY sort_order ASC, rowid ASC",
    )?;
    let mut grouped: HashMap<String, Vec<QueuedPromptView>> = HashMap::new();
    let rows = stmt.query_map([], |row| {
        let agent_id: String = row.get(0)?;
        let badges_str: String = row.get(3)?;
        let created_str: String = row.get(4)?;
        Ok((
            agent_id,
            QueuedPromptView {
                queue_id: row.get(1)?,
                display_text: row.get(2)?,
                badges: serde_json::from_str(&badges_str).unwrap_or_default(),
                created_at: parse_ts(&created_str),
            },
        ))
    })?;
    for entry in rows {
        let (agent_id, item) = entry?;
        grouped.entry(agent_id).or_default().push(item);
    }
    Ok(grouped)
}

pub(crate) fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::AttentionState;

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
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
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
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
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
            mission_goal: None,
            project_root: None,
            board_revision: 0,
            title_locked: false,
            auto_clustered: false,
            is_curated: false,
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
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Running {
                turn_id: "t1".into(),
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
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
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store.upsert_agent(&parent, Some("ref_p")).expect("parent");

        let mut child = parent.clone();
        child.agent_id = "ag_child".into();
        child.title = "Child Worker".into();
        child.parent_agent_id = Some("ag_parent".into());
        child
            .labels
            .insert("orbit.open_tab.desktop".into(), "true".into());
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
            acp_agent_id: None,
            model: Some("gpt-5.4".into()),
            effort: Some(ReasoningEffort::Xhigh),
            fast_mode: true,
            mode: SessionMode::Plan,
            when_to_use: "Review diffs independently without editing files".into(),
            system_prompt_append: Some("Never edit files.".into()),
            mcp_policy: orbit_protocol::McpToolPolicy {
                enabled: true,
                disabled_tools: vec!["create_agent".into()],
                ..orbit_protocol::McpToolPolicy::default()
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
            model: Some("claude-sonnet-4-6".into()),
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
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
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
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
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
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
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
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
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
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
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
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
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
        assert!(
            store
                .advance_replica_contiguous_seq("ag_pg", p1[1].seq, true)
                .expect("advance present")
        );
        assert_eq!(
            store.get_replica_contiguous_seq("ag_pg").expect("get"),
            p1[1].seq
        );
        // Refuses to advance when target seq was not actually persisted in client_replica_events
        // and clears the cursor to 0 so the next open heals via full catchup.
        assert!(
            !store
                .advance_replica_contiguous_seq("ag_pg", p3[0].seq, true)
                .expect("advance missing")
        );
        assert_eq!(store.get_replica_contiguous_seq("ag_pg").expect("get"), 0);

        store
            .set_replica_contiguous_seq("ag_pg", p1[1].seq)
            .expect("set");
        store.replace_replica_timeline("ag_pg", &[]).expect("wipe");
        assert_eq!(store.get_replica_contiguous_seq("ag_pg").expect("get"), 0);
    }

    #[test]
    fn client_thread_prefs_crud_monotonic_reset_and_migration() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let pref = ClientThreadPref {
            host_id: "h1".into(),
            agent_id: "opt_1".into(),
            seen_seq: 3,
            is_pinned: true,
            pinned_order: 100,
            updated_at_ms: 1000,
        };
        store.upsert_client_thread_pref(&pref).expect("upsert");

        // Monotonic seen_seq ignores smaller values unless force_reset = true
        store
            .set_client_thread_seen_seq("h1", "opt_1", 1, false, 1100)
            .expect("ignore lower seen");
        store
            .set_client_thread_seen_seq("h1", "opt_1", 7, false, 1200)
            .expect("advance seen");

        let prefs = store.list_client_thread_prefs().expect("list");
        assert_eq!(prefs.len(), 1);
        assert_eq!(prefs[0].seen_seq, 7);
        assert!(prefs[0].is_pinned);

        // Migrate opt_1 -> ag_1 merges pinned + max(seen_seq) and deletes opt_1
        store
            .set_client_thread_seen_seq("h1", "ag_1", 10, false, 1300)
            .expect("seed target");
        store
            .migrate_client_thread_pref("h1", "opt_1", "ag_1")
            .expect("migrate");
        let after = store.list_client_thread_prefs().expect("list after");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].agent_id, "ag_1");
        assert_eq!(after[0].seen_seq, 10);
        assert!(after[0].is_pinned);
        assert_eq!(after[0].pinned_order, 100);

        // force_reset allows rewind to reduce seen_seq
        store
            .set_client_thread_seen_seq("h1", "ag_1", 2, true, 1400)
            .expect("reset");
        let after_reset = store.list_client_thread_prefs().expect("list reset");
        assert_eq!(after_reset[0].seen_seq, 2);

        store
            .set_client_thread_pinned("h1", "ag_1", false, 0, 1500)
            .expect("unpin");
        let after_unpin = store.list_client_thread_prefs().expect("list unpin");
        assert!(!after_unpin[0].is_pinned);

        store
            .delete_client_thread_pref("h1", "ag_1")
            .expect("delete");
        assert!(
            store
                .list_client_thread_prefs()
                .expect("list empty")
                .is_empty()
        );

        store
            .upsert_client_thread_pref(&ClientThreadPref {
                host_id: "h2".into(),
                agent_id: "ag_2".into(),
                seen_seq: 5,
                is_pinned: false,
                pinned_order: 0,
                updated_at_ms: 1600,
            })
            .expect("insert h2");
        store
            .delete_client_thread_prefs_for_host("h2")
            .expect("delete host");
        assert!(
            store
                .list_client_thread_prefs()
                .expect("list after host wipe")
                .is_empty()
        );
    }

    #[test]
    fn queued_prompts_persist_hydrate_fifo_pop_and_clear_on_archive() {
        let store = OrbitStore::open_in_memory().expect("open");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_q".into(),
                title: "Queue WS".into(),
                cwd: "/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::default(),
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_q".into(),
                    workspace_id: "ws_q".into(),
                    cwd: "/repo".into(),
                    title: "Queue Agent".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("agent");

        store
            .enqueue_agent_prompt("ag_q", "q1", "wire 1", "First follow-up", &[])
            .expect("enqueue 1");
        store
            .enqueue_agent_prompt("ag_q", "q2", "wire 2", "Second follow-up", &[])
            .expect("enqueue 2");
        store
            .enqueue_agent_prompt("ag_q", "q3", "wire 3", "Third follow-up", &[])
            .expect("enqueue 3");

        let ag = store.get_agent("ag_q").expect("get").expect("some");
        assert_eq!(ag.queued_prompts.len(), 3);
        assert_eq!(ag.queued_prompts[0].queue_id, "q1");
        assert_eq!(ag.queued_prompts[1].queue_id, "q2");
        assert_eq!(ag.queued_prompts[2].queue_id, "q3");

        let listed = store.list_agents(None).expect("list");
        assert_eq!(listed[0].queued_prompts.len(), 3);

        // Reorder queue: move q3 to front, q2 to back (partial list preserves remaining q2 at tail)
        let changed = store
            .reorder_queued_prompts("ag_q", &["q3".to_string(), "q1".to_string()])
            .expect("reorder");
        assert!(changed);
        let reordered = store.get_agent("ag_q").expect("get").expect("some");
        let ids: Vec<&str> = reordered
            .queued_prompts
            .iter()
            .map(|q| q.queue_id.as_str())
            .collect();
        assert_eq!(ids, vec!["q3", "q1", "q2"]);

        // Reordering with identical order is a no-op
        assert!(
            !store
                .reorder_queued_prompts(
                    "ag_q",
                    &["q3".to_string(), "q1".to_string(), "q2".to_string()]
                )
                .expect("noop reorder")
        );

        // Take specific item out of order (flush now)
        let taken = store
            .take_queued_prompt("ag_q", "q1")
            .expect("take")
            .expect("found q1");
        assert_eq!(taken.wire_prompt, "wire 1");

        // Sync mutations (read cursor, pin, handle, unread, move) must NEVER drop queued prompts!
        let after_read = store
            .mark_agent_read_seq("ag_q", Some(1))
            .expect("mark read")
            .expect("some");
        assert_eq!(after_read.queued_prompts.len(), 2);
        let after_pin = store
            .set_agent_pinned("ag_q", true, Some(100))
            .expect("pin")
            .expect("some");
        assert_eq!(after_pin.queued_prompts.len(), 2);
        let after_unread = store
            .mark_agent_unread("ag_q")
            .expect("unread")
            .expect("some");
        assert_eq!(after_unread.queued_prompts.len(), 2);
        let after_handle = store
            .set_agent_handle("ag_q", Some("lead"))
            .expect("handle")
            .expect("some");
        assert_eq!(after_handle.queued_prompts.len(), 2);

        // Read-only peek must NOT delete q3; take_queued_prompt claims it once atomically
        let peeked = store
            .peek_next_queued_prompt("ag_q")
            .expect("peek")
            .expect("found q3");
        assert_eq!(peeked.queue_id, "q3");
        let peeked_again = store
            .peek_next_queued_prompt("ag_q")
            .expect("peek again")
            .expect("still q3");
        assert_eq!(peeked_again.queue_id, "q3");

        let claimed = store
            .take_queued_prompt("ag_q", "q3")
            .expect("take q3")
            .expect("claimed q3");
        assert_eq!(claimed.queue_id, "q3");
        assert!(
            store
                .take_queued_prompt("ag_q", "q3")
                .expect("second take")
                .is_none(),
            "second take of same queue_id must be idempotent no-op"
        );

        // FIFO pop now gets q2 (remaining item)
        let popped = store
            .pop_next_queued_prompt("ag_q")
            .expect("pop")
            .expect("found q2");
        assert_eq!(popped.queue_id, "q2");

        // Archiving clears remaining queued prompts
        store.archive_agent_soft("ag_q").expect("archive");
        let after_archive = store.get_agent("ag_q").expect("get").expect("some");
        assert!(after_archive.queued_prompts.is_empty());
    }

    #[test]
    fn running_background_tool_calls_list_and_settle_in_place_bump_epoch() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_bg".into(),
                title: "bg".into(),
                cwd: "/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_bg".into(),
                    workspace_id: "ws_bg".into(),
                    cwd: "/repo".into(),
                    title: "BG Test".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: orbit_protocol::AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                Some("sess_bg"),
            )
            .expect("agent");

        // Stale timer left `running` in Turn 1
        let ev1 = store
            .append_timeline_event(
                "ag_bg",
                1,
                None,
                TimelinePayload::ToolCall {
                    call_id: "wake_tool_57_0".into(),
                    status: ToolCallStatus::Running,
                    detail: ToolCallDetail::Generic {
                        tool_name: "schedule".into(),
                        arguments_json: "{}".into(),
                    },
                    background_task: Some(BackgroundTaskInfo {
                        task_id: Some("sess_bg/task-58".into()),
                        label: Some("Timer: 4m".into()),
                        log_path: None,
                    }),
                },
            )
            .expect("append stale timer");

        // Turn 2 opens with a new user prompt and launches a genuinely live task
        store
            .append_timeline_event(
                "ag_bg",
                1,
                None,
                TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "continue".into(),
                    badges: Vec::new(),
                },
            )
            .expect("append prompt");
        let ev3 = store
            .append_timeline_event(
                "ag_bg",
                1,
                None,
                TimelinePayload::ToolCall {
                    call_id: "call_live_1".into(),
                    status: ToolCallStatus::Running,
                    detail: ToolCallDetail::Shell {
                        command: "python run.py".into(),
                        cwd: None,
                        exit_code: None,
                        output_preview: None,
                    },
                    background_task: Some(BackgroundTaskInfo {
                        task_id: Some("sess_bg/task-290".into()),
                        label: Some("python run.py".into()),
                        log_path: None,
                    }),
                },
            )
            .expect("append live task");

        let running = store
            .list_running_background_tool_calls("ag_bg")
            .expect("list running");
        assert_eq!(running.len(), 2);
        assert_eq!(running[0].0, "wake_tool_57_0");
        assert_eq!(running[1].0, "call_live_1");

        // Settle only the stale timer in place -> preserves original `seq`, bumps `timeline_epoch`
        let updated = store
            .settle_timeline_tool_calls_in_place(
                "ag_bg",
                &[("wake_tool_57_0".into(), ToolCallStatus::Completed)],
            )
            .expect("settle in place");
        assert_eq!(updated, 1);

        let ag_after = store.get_agent("ag_bg").expect("get").expect("some");
        assert_eq!(ag_after.timeline_epoch, 1);

        let remaining = store
            .list_running_background_tool_calls("ag_bg")
            .expect("remaining");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].0, "call_live_1");

        let all_events = store.fetch_timeline_from("ag_bg", 0).expect("fetch all");
        assert_eq!(all_events.len(), 3);
        assert_eq!(all_events[0].seq, ev1.seq);
        assert!(matches!(
            all_events[0].payload,
            TimelinePayload::ToolCall {
                status: ToolCallStatus::Completed,
                ..
            }
        ));
        assert_eq!(all_events[2].seq, ev3.seq);
        assert!(matches!(
            all_events[2].payload,
            TimelinePayload::ToolCall {
                status: ToolCallStatus::Running,
                ..
            }
        ));
    }

    #[test]
    fn shim_cursor_max_turn_segment_step_and_recent_timeline() {
        let store = OrbitStore::open_in_memory().expect("open");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_cur".into(),
                title: "cur".into(),
                cwd: "/tmp".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_cur".into(),
                    workspace_id: "ws_cur".into(),
                    cwd: "/tmp".into(),
                    title: "Cursor test".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::FullAccess,
                    status: AgentStatus::Running {
                        turn_id: "turn_abc".into(),
                    },
                    parent_agent_id: None,
                    parent_host_id: None,
                    profile_id: None,
                    background: false,
                    auto_archive: false,
                    archived_at: None,
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    task_summary: None,
                    cluster_suggestion: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                    labels: Default::default(),
                    latest_seq: 0,
                    timeline_epoch: 0,
                    queued_prompts: vec![],
                    created_at: now,
                    updated_at: now,
                },
                None,
            )
            .expect("upsert");

        assert_eq!(store.get_agent_shim_cursor("ag_cur"), None);
        store.set_agent_shim_cursor("ag_cur", 42).expect("set");
        assert_eq!(store.get_agent_shim_cursor("ag_cur"), Some(42));
        store.set_agent_shim_cursor("ag_cur", 128).expect("update");
        assert_eq!(store.get_agent_shim_cursor("ag_cur"), Some(128));
        store.clear_agent_shim_cursor("ag_cur").expect("clear");
        assert_eq!(store.get_agent_shim_cursor("ag_cur"), None);

        for i in 1..=5 {
            store
                .append_timeline_event(
                    "ag_cur",
                    0,
                    Some("turn_abc"),
                    TimelinePayload::AssistantThinking {
                        item_id: format!("agy_th_turn_abc_{i}"),
                        text: format!("thought {i}"),
                    },
                )
                .expect("append");
        }
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_abc"),
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_turn_abc_12".into(),
                    text: "latest message".into(),
                    streaming: false,
                },
            )
            .expect("append msg");

        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_abc"), 12);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_other"), 0);

        // Multi-provider segment tracking: `pi_msg_`, `opencode_msg_`, `gemini_cli_th_`,
        // `acp_th_`, `acp_msg_`, while ignoring unrelated turns that share a prefix.
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_pi"),
                TimelinePayload::AssistantMessage {
                    item_id: "pi_msg_turn_pi_7".into(),
                    text: "pi chunk".into(),
                    streaming: false,
                },
            )
            .expect("append pi msg");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp"),
                TimelinePayload::AssistantMessage {
                    item_id: "opencode_msg_turn_acp_9".into(),
                    text: "opencode chunk".into(),
                    streaming: false,
                },
            )
            .expect("append opencode msg");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp"),
                TimelinePayload::AssistantThinking {
                    item_id: "gemini_cli_th_turn_acp_15".into(),
                    text: "gemini thought".into(),
                },
            )
            .expect("append gemini th");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp_extra"),
                TimelinePayload::AssistantMessage {
                    item_id: "opencode_msg_turn_acp_extra_99".into(),
                    text: "different turn".into(),
                    streaming: false,
                },
            )
            .expect("append extra turn");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp_1"),
                TimelinePayload::AssistantThinking {
                    item_id: "acp_th_turn_acp_1_7".into(),
                    text: "acp thought".into(),
                },
            )
            .expect("append acp th");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "acp_msg_turn_acp_1_19".into(),
                    text: "acp reply".into(),
                    streaming: false,
                },
            )
            .expect("append acp msg");
        store
            .append_timeline_event(
                "ag_cur",
                0,
                Some("turn_acp_10"),
                TimelinePayload::AssistantMessage {
                    item_id: "acp_msg_turn_acp_10_99".into(),
                    text: "other turn".into(),
                    streaming: false,
                },
            )
            .expect("append other acp turn");

        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_pi"), 7);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_acp"), 15);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_acp_extra"), 99);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_acp_1"), 19);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_acp_10"), 99);
        assert_eq!(store.max_turn_segment_step("ag_cur", "turn_abc"), 12);

        let recent = store.fetch_recent_timeline("ag_cur", 2).expect("recent");
        assert_eq!(recent.len(), 2);
        assert!(matches!(
            &recent[0].payload,
            TimelinePayload::AssistantMessage { item_id, .. } if item_id == "acp_msg_turn_acp_1_19"
        ));
        assert!(matches!(
            &recent[1].payload,
            TimelinePayload::AssistantMessage { item_id, .. } if item_id == "acp_msg_turn_acp_10_99"
        ));
    }

    #[test]
    fn clone_timeline_events_up_to_truncates_strictly_and_syncs_fts5() {
        let store = OrbitStore::open_in_memory().expect("open");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_fk".into(),
                title: "fork".into(),
                cwd: "/tmp/fork".into(),
                git_branch: Some("main".into()),
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let mk_ag = |id: &str, from: Option<&str>, at_seq: Option<u64>| AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_fk".into(),
            cwd: "/tmp/fork".into(),
            title: format!("Title {id}"),
            provider: ProviderKind::ClaudeCode,
            model: Some("sonnet".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
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
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: from.map(ToOwned::to_owned),
            forked_at_seq: at_seq,
            acp_agent_id: None,
        };

        store
            .upsert_agent(&mk_ag("ag_src", None, None), Some("sess_src"))
            .expect("src");

        let e1 = store
            .append_timeline_event(
                "ag_src",
                1,
                Some("t1"),
                TimelinePayload::UserPrompt {
                    message_id: "m1".into(),
                    text: "AlphaTurnOneToken design question".into(),
                    badges: Vec::new(),
                },
            )
            .expect("e1");
        let e2 = store
            .append_timeline_event(
                "ag_src",
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "a1".into(),
                    text: "AlphaReplyOneToken detailed answer".into(),
                    streaming: false,
                },
            )
            .expect("e2");
        let _e3 = store
            .append_timeline_event(
                "ag_src",
                1,
                Some("t2"),
                TimelinePayload::UserPrompt {
                    message_id: "m2".into(),
                    text: "SecretPostForkTurnTwoToken should never appear in prefix fork".into(),
                    badges: Vec::new(),
                },
            )
            .expect("e3");
        let _e4 = store
            .append_timeline_event(
                "ag_src",
                1,
                Some("t2"),
                TimelinePayload::AssistantMessage {
                    item_id: "a2".into(),
                    text: "SecretPostForkReplyTwoToken also excluded".into(),
                    streaming: false,
                },
            )
            .expect("e4");

        // 1. Mid-thread prefix fork strictly up to e2.seq
        store
            .upsert_agent(&mk_ag("ag_mid", Some("ag_src"), Some(e2.seq)), None)
            .expect("mid");
        let mid_res = store
            .clone_timeline_events_up_to("ag_src", "ag_mid", Some(e2.seq))
            .expect("clone mid");
        assert_eq!(mid_res.copied_count, 2);
        assert!(!mid_res.cloned_full_tail);
        assert_eq!(mid_res.effective_source_seq, Some(e2.seq));
        assert!(mid_res.new_latest_seq > e2.seq);
        let mid_latest = mid_res.new_latest_seq;

        let mid_agent = store.get_agent("ag_mid").expect("get").expect("exists");
        assert_eq!(mid_agent.forked_from_agent_id.as_deref(), Some("ag_src"));
        assert_eq!(mid_agent.forked_at_seq, Some(e2.seq));
        assert_eq!(mid_agent.latest_seq, mid_latest);
        assert_eq!(mid_agent.read_seq, mid_latest);

        let (mid_events, has_more) = store.fetch_timeline_since("ag_mid", 0, 50).expect("fetch");
        assert!(!has_more);
        assert_eq!(mid_events.len(), 2);
        assert!(mid_events[0].seq < mid_events[1].seq);
        assert_eq!(mid_events[1].seq, mid_latest);
        assert!(matches!(
            &mid_events[0].payload,
            TimelinePayload::UserPrompt { text, .. } if text.contains("AlphaTurnOneToken")
        ));
        assert!(matches!(
            &mid_events[1].payload,
            TimelinePayload::AssistantMessage { text, .. } if text.contains("AlphaReplyOneToken")
        ));

        // FTS5 sync: prefix tokens searchable on ag_mid, post-fork tokens strictly NOT in ag_mid
        let alpha_hits = store
            .search_daemon_messages("AlphaReplyOneToken", 20)
            .expect("fts alpha");
        assert!(alpha_hits.iter().any(|h| h.agent_id == "ag_src"));
        assert!(alpha_hits.iter().any(|h| h.agent_id == "ag_mid"));

        let post_hits = store
            .search_daemon_messages("SecretPostForkTurnTwoToken", 20)
            .expect("fts post");
        assert!(post_hits.iter().any(|h| h.agent_id == "ag_src"));
        assert!(!post_hits.iter().any(|h| h.agent_id == "ag_mid"));

        // 2. Full fork (up_to_seq = None) copies all 4 events in exact order
        store
            .upsert_agent(&mk_ag("ag_full", Some("ag_src"), None), None)
            .expect("full");
        let full_res = store
            .clone_timeline_events_up_to("ag_src", "ag_full", None)
            .expect("clone full");
        assert_eq!(full_res.copied_count, 4);
        assert!(full_res.cloned_full_tail);
        assert_eq!(full_res.effective_source_seq, Some(_e4.seq));
        assert!(full_res.new_latest_seq > mid_latest);
        let (full_events, _) = store.fetch_timeline_since("ag_full", 0, 50).expect("fetch");
        assert_eq!(full_events.len(), 4);
        for w in full_events.windows(2) {
            assert!(w[0].seq < w[1].seq);
        }

        // Self-clone rejected cleanly
        assert!(
            store
                .clone_timeline_events_up_to("ag_src", "ag_src", Some(e1.seq))
                .is_err()
        );

        // Replica provenance roundtrip & column persistence
        store
            .upsert_single_replica_agent("host_1", &mid_agent)
            .expect("replica upsert");
        let rep = store.list_replica_agents("host_1").expect("replica list");
        assert_eq!(rep.len(), 1);
        assert_eq!(rep[0].forked_from_agent_id.as_deref(), Some("ag_src"));
        assert_eq!(rep[0].forked_at_seq, Some(e2.seq));
    }

    #[test]
    fn clone_timeline_events_up_to_normalizes_inflight_events_and_tracks_full_tail() {
        let store = OrbitStore::open_in_memory().expect("open");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_norm".into(),
                title: "norm".into(),
                cwd: "/tmp/norm".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let mk_ag = |id: &str| AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_norm".into(),
            cwd: "/tmp/norm".into(),
            title: id.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
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
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store
            .upsert_agent(&mk_ag("ag_live_src"), None)
            .expect("src");
        store
            .upsert_agent(&mk_ag("ag_live_fork"), None)
            .expect("fork");

        // 1. UserPrompt
        store
            .append_timeline_event(
                "ag_live_src",
                1,
                Some("t1"),
                TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Run dev server and edit".into(),
                    badges: Vec::new(),
                },
            )
            .expect("prompt");
        // 2. Open Running background ToolCall
        store
            .append_timeline_event(
                "ag_live_src",
                1,
                Some("t1"),
                TimelinePayload::ToolCall {
                    call_id: "call_bg_1".into(),
                    status: ToolCallStatus::Running,
                    detail: orbit_protocol::ToolCallDetail::Shell {
                        command: "npm run dev".into(),
                        cwd: None,
                        exit_code: None,
                        output_preview: None,
                    },
                    background_task: Some(orbit_protocol::BackgroundTaskInfo {
                        task_id: Some("bg_99".into()),
                        label: Some("dev server".into()),
                        log_path: None,
                    }),
                },
            )
            .expect("bg tool");
        // 3. Unresolved PermissionRequest
        store
            .append_timeline_event(
                "ag_live_src",
                1,
                Some("t1"),
                TimelinePayload::PermissionRequest {
                    request_id: "perm_open_1".into(),
                    title: "Run rm -rf".into(),
                    detail: orbit_protocol::ToolCallDetail::Generic {
                        tool_name: "Bash".into(),
                        arguments_json: "{\"command\":\"rm -rf\"}".into(),
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                },
            )
            .expect("perm req");
        // 4. Streaming AssistantMessage (not yet finalized!)
        store
            .append_timeline_event(
                "ag_live_src",
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "msg_stream_1".into(),
                    text: "StreamingPartialTokenUnique42 in progress reply".into(),
                    streaming: true,
                },
            )
            .expect("streaming reply");
        // 5. Trailing transient StatusChanged::Running
        let last_ev = store
            .append_timeline_event(
                "ag_live_src",
                1,
                Some("t1"),
                TimelinePayload::StatusChanged {
                    status: AgentStatus::Running {
                        turn_id: "t1".into(),
                    },
                },
            )
            .expect("running status");

        // Oversized up_to_seq still reports cloned_full_tail = true and clamps effective_source_seq
        let outcome = store
            .clone_timeline_events_up_to("ag_live_src", "ag_live_fork", Some(last_ev.seq + 500))
            .expect("clone inflight");
        assert!(outcome.cloned_full_tail);
        assert_eq!(outcome.effective_source_seq, Some(last_ev.seq));
        // Unresolved PermissionRequest + trailing StatusChanged::Running were stripped -> 3 rows remain
        assert_eq!(outcome.copied_count, 3);

        let (events, _) = store
            .fetch_timeline_since("ag_live_fork", 0, 50)
            .expect("fetch normalized");
        assert_eq!(events.len(), 3);

        // Running background ToolCall was rewritten to Failed with background_task cleared
        assert!(matches!(
            &events[1].payload,
            TimelinePayload::ToolCall {
                call_id,
                status: ToolCallStatus::Failed,
                background_task: None,
                ..
            } if call_id == "call_bg_1"
        ));
        assert!(
            store
                .list_running_background_tool_calls("ag_live_fork")
                .expect("bg list")
                .is_empty(),
            "forked agent must not inherit running background tool calls"
        );

        // Streaming reply was finalized (`streaming: false`) and indexed in FTS5!
        assert!(matches!(
            &events[2].payload,
            TimelinePayload::AssistantMessage {
                item_id,
                streaming: false,
                ..
            } if item_id == "msg_stream_1"
        ));
        let fts_hits = store
            .search_daemon_messages("StreamingPartialTokenUnique42", 10)
            .expect("fts search finalized stream");
        assert!(
            fts_hits.iter().any(|h| h.agent_id == "ag_live_fork"),
            "finalized streaming reply must be indexed in FTS5 for forked agent"
        );

        // Target agent's forked_at_seq was clamped to actual source max seq inside the transaction
        let forked_ag = store
            .get_agent("ag_live_fork")
            .expect("get")
            .expect("exists");
        assert_eq!(forked_ag.forked_at_seq, Some(last_ev.seq));
    }

    #[test]
    fn idempotent_fork_column_migration_preserves_user_version() {
        let dir = std::env::temp_dir().join(format!(
            "orbit_store_fork_mig_{}_{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let db_path = dir.join("orbit.db");

        // Open once (applies all migrations), verify user_version is unchanged (6)
        let store1 = OrbitStore::open(&db_path).expect("open 1");
        let ver1: i64 = {
            let conn = store1.conn.lock().expect("lock");
            assert!(table_has_column(&conn, "agents", "forked_from_agent_id").expect("col"));
            assert!(table_has_column(&conn, "agents", "forked_at_seq").expect("col"));
            assert!(
                table_has_column(&conn, "client_replica_agents", "forked_from_agent_id")
                    .expect("col")
            );
            assert!(
                table_has_column(&conn, "client_replica_agents", "forked_at_seq").expect("col")
            );
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("ver")
        };
        drop(store1);

        // Re-open existing DB -> idempotent migration succeeds cleanly without bumping user_version
        let store2 = OrbitStore::open(&db_path).expect("open 2");
        let ver2: i64 = store2
            .conn
            .lock()
            .expect("lock")
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("ver");
        assert_eq!(ver1, ver2);
        drop(store2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fast_mode_migration_idempotency_and_crud_persistence() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "orbit_fast_mode_test_{}_{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("orbit.sqlite");

        // 1. Seed a pre-existing database WITHOUT fast_mode columns and with user_version = 6
        {
            let raw = Connection::open(&db_path).expect("open raw");
            raw.execute_batch(
                "
                CREATE TABLE workspaces (
                    workspace_id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    cwd TEXT NOT NULL,
                    git_branch TEXT,
                    is_worktree INTEGER NOT NULL DEFAULT 0,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE agents (
                    agent_id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    cwd TEXT NOT NULL,
                    title TEXT NOT NULL,
                    provider_json TEXT NOT NULL,
                    model TEXT,
                    effort_json TEXT,
                    mode_json TEXT NOT NULL,
                    status_json TEXT NOT NULL,
                    parent_agent_id TEXT,
                    persistence_ref TEXT,
                    latest_seq INTEGER NOT NULL DEFAULT 0,
                    timeline_epoch INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE client_replica_agents (
                    host_id TEXT NOT NULL,
                    agent_id TEXT NOT NULL,
                    payload_json TEXT NOT NULL,
                    PRIMARY KEY (host_id, agent_id)
                );
                CREATE TABLE agent_profiles (
                    profile_id TEXT PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    provider_json TEXT NOT NULL,
                    model TEXT,
                    effort_json TEXT,
                    mode_json TEXT NOT NULL,
                    when_to_use TEXT NOT NULL DEFAULT '',
                    system_prompt_append TEXT,
                    mcp_policy_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                PRAGMA user_version = 6;
                ",
            )
            .expect("seed legacy schema");
        }

        // 2. Open with OrbitStore twice to confirm idempotent PRAGMA table_info migration
        //    and confirm PRAGMA user_version stays 6 (not bumped in parallel with other Wave 2 work).
        let store = OrbitStore::open(&db_path).expect("first open");
        drop(store);
        let store = OrbitStore::open(&db_path).expect("second idempotent open");
        {
            let conn = store.conn.lock().expect("lock");
            let ver: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("user_version");
            assert_eq!(ver, 6);
            for tbl in ["agents", "client_replica_agents", "agent_profiles"] {
                let mut stmt = conn
                    .prepare(&format!("PRAGMA table_info({tbl})"))
                    .expect("pragma");
                let has_col = stmt
                    .query_map([], |r| r.get::<_, String>(1))
                    .expect("query")
                    .filter_map(Result::ok)
                    .any(|c| c == "fast_mode");
                assert!(has_col, "missing fast_mode column on {tbl}");
            }
        }

        // 3. Verify CRUD persistence on agents, client_replica_agents, and agent_profiles
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_fast".into(),
                title: "Fast".into(),
                cwd: "/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let ag = AgentSummary {
            agent_id: "ag_fast".into(),
            workspace_id: "ws_fast".into(),
            cwd: "/repo".into(),
            title: "Fast Session".into(),
            provider: ProviderKind::Codex,
            model: Some("gpt-5.4".into()),
            effort: Some(ReasoningEffort::Medium),
            fast_mode: true,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
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
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store.upsert_agent(&ag, None).expect("upsert agent");
        let loaded = store.get_agent("ag_fast").expect("get").expect("some");
        assert!(loaded.fast_mode);

        store
            .update_agent_fields(
                "ag_fast",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(false),
            )
            .expect("disable fast");
        let disabled = store.get_agent("ag_fast").expect("get").expect("some");
        assert!(!disabled.fast_mode);

        // Replica persistence
        store
            .upsert_replica_agents("h_fast", std::slice::from_ref(&ag))
            .expect("upsert replica batch");
        let rep1 = store.list_replica_agents("h_fast").expect("list replica");
        assert_eq!(rep1.len(), 1);
        assert!(rep1[0].fast_mode);

        let mut ag_off = ag.clone();
        ag_off.fast_mode = false;
        store
            .upsert_single_replica_agent("h_fast", &ag_off)
            .expect("upsert single replica");
        let rep2 = store.list_replica_agents("h_fast").expect("list replica 2");
        assert_eq!(rep2.len(), 1);
        assert!(!rep2[0].fast_mode);

        // Profile persistence
        let prof = orbit_protocol::AgentProfile {
            profile_id: "prof_fast".into(),
            name: "Fast Coder".into(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: Some("claude-opus-4-6".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: true,
            mode: SessionMode::AcceptEdits,
            when_to_use: "Rapid iteration".into(),
            system_prompt_append: None,
            mcp_policy: orbit_protocol::McpToolPolicy::default(),
            created_at: now,
            updated_at: now,
        };
        store.upsert_profile(&prof).expect("upsert prof");
        let loaded_prof = store
            .get_profile("prof_fast")
            .expect("get prof")
            .expect("some prof");
        assert!(loaded_prof.fast_mode);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_agent_fields_preserves_unrelated_runtime_columns() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "orbit_atomic_update_test_{}_{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let db_path = dir.join("orbit.sqlite");
        let store = OrbitStore::open(&db_path).expect("open store");

        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_atomic".into(),
                title: "Atomic".into(),
                cwd: "/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let mut labels = HashMap::new();
        labels.insert("env".into(), "prod".into());
        let ag = AgentSummary {
            agent_id: "ag_atomic".into(),
            workspace_id: "ws_atomic".into(),
            cwd: "/repo".into(),
            title: "Original Title".into(),
            provider: ProviderKind::ClaudeCode,
            model: Some("claude-opus-4-6".into()),
            effort: Some(ReasoningEffort::High),
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Running {
                turn_id: "turn_active".into(),
            },
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 9,
            timeline_epoch: 2,
            created_at: now,
            updated_at: now,
            archived_at: Some(now),
            background: false,
            auto_archive: false,
            profile_id: Some("prof_1".into()),
            labels,
            attention: AttentionState::Permission,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 5,
            is_pinned: true,
            pinned_order: 7,
            handle: Some("lead".into()),
            cluster_suggestion: None,
            task_summary: Some("Working".into()),
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        store
            .upsert_agent(&ag, Some("native_sess_atomic"))
            .expect("upsert agent");

        // Targeted update changing ONLY fast_mode must leave all other runtime columns intact
        store
            .update_agent_fields(
                "ag_atomic",
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(true),
            )
            .expect("targeted fast_mode update");

        let updated = store
            .get_agent("ag_atomic")
            .expect("get")
            .expect("agent exists");
        assert!(updated.fast_mode);
        assert_eq!(updated.title, "Original Title");
        assert_eq!(
            updated.status,
            AgentStatus::Running {
                turn_id: "turn_active".into()
            }
        );
        assert_eq!(updated.attention, AttentionState::Permission);
        assert!(updated.is_pinned);
        assert_eq!(updated.pinned_order, 7);
        assert!(updated.archived_at.is_some());
        assert_eq!(updated.labels.get("env").map(String::as_str), Some("prod"));
        assert_eq!(updated.handle.as_deref(), Some("lead"));
        assert_eq!(updated.task_summary.as_deref(), Some("Working"));
        assert_eq!(
            store.get_persistence_ref("ag_atomic").expect("pref"),
            Some("native_sess_atomic".to_string())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn acp_agent_id_roundtrip_agents_replica_and_profiles() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_acp".into(),
                title: "ACP Workspace".into(),
                cwd: "/tmp/acp".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let acp_agent = AgentSummary {
            agent_id: "ag_acp_1".into(),
            workspace_id: "ws_acp".into(),
            cwd: "/tmp/acp".into(),
            title: "OpenCode ACP Session".into(),
            provider: ProviderKind::Acp,
            model: Some("default".into()),
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
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
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: Some("opencode".into()),
        };

        store
            .upsert_agent(&acp_agent, Some("acp_sess_1"))
            .expect("upsert acp agent");
        let loaded = store
            .get_agent("ag_acp_1")
            .expect("get acp agent")
            .expect("exists");
        assert_eq!(loaded.provider, ProviderKind::Acp);
        assert_eq!(loaded.acp_agent_id.as_deref(), Some("opencode"));

        // Verify client_replica_agents roundtrip (both batch and single upsert)
        store
            .upsert_replica_agents("host_1", std::slice::from_ref(&acp_agent))
            .expect("upsert replica batch");
        let replica_list = store.list_replica_agents("host_1").expect("list replica");
        assert_eq!(replica_list.len(), 1);
        assert_eq!(replica_list[0].provider, ProviderKind::Acp);
        assert_eq!(replica_list[0].acp_agent_id.as_deref(), Some("opencode"));

        let mut updated_replica = acp_agent.clone();
        updated_replica.acp_agent_id = Some("gemini-cli".into());
        store
            .upsert_single_replica_agent("host_1", &updated_replica)
            .expect("upsert single replica");
        let replica_list2 = store.list_replica_agents("host_1").expect("list replica 2");
        assert_eq!(replica_list2[0].acp_agent_id.as_deref(), Some("gemini-cli"));

        // Verify switch_agent_provider updates acp_agent_id cleanly
        store
            .switch_agent_provider(
                "ag_acp_1",
                ProviderKind::Acp,
                None,
                None,
                false,
                Some("goose"),
            )
            .expect("switch to goose");
        let switched = store
            .get_agent("ag_acp_1")
            .expect("get switched")
            .expect("exists");
        assert_eq!(switched.acp_agent_id.as_deref(), Some("goose"));

        // Verify AgentProfile acp_agent_id roundtrip
        let profile = orbit_protocol::AgentProfile {
            profile_id: "prof_acp".into(),
            name: "OpenCode Worker".into(),
            provider: ProviderKind::Acp,
            acp_agent_id: Some("opencode".into()),
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            when_to_use: "Use for OpenCode ACP tasks".into(),
            system_prompt_append: None,
            mcp_policy: orbit_protocol::McpToolPolicy::default(),
            created_at: now,
            updated_at: now,
        };
        store.upsert_profile(&profile).expect("upsert acp profile");
        let loaded_prof = store
            .get_profile("prof_acp")
            .expect("get profile")
            .expect("exists");
        assert_eq!(loaded_prof.provider, ProviderKind::Acp);
        assert_eq!(loaded_prof.acp_agent_id.as_deref(), Some("opencode"));

        // Ensure PRAGMA user_version remains untouched (v6 from FTS5) and ensure_table_column is idempotent
        let conn = store.conn.lock().expect("lock");
        let ver_before: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("user_version");
        assert_eq!(ver_before, 6);
        ensure_table_column(&conn, "agents", "acp_agent_id", "TEXT").expect("idempotent 1");
        ensure_table_column(&conn, "client_replica_agents", "acp_agent_id", "TEXT")
            .expect("idempotent 2");
        let ver_after: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("user_version");
        assert_eq!(ver_after, ver_before);
    }

    #[test]
    fn provider_subagent_events_dedupe_historical_replay_and_coalesce_thinking_heartbeats() {
        let store = OrbitStore::open_in_memory().expect("open");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_sub_dedupe".into(),
                title: "Dedupe".into(),
                cwd: "/tmp".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_parent_dedupe".into(),
                    workspace_id: "ws_sub_dedupe".into(),
                    cwd: "/tmp".into(),
                    title: "Parent".into(),
                    provider: ProviderKind::Antigravity,
                    acp_agent_id: None,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    parent_host_id: None,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: orbit_protocol::AttentionState::None,
                    last_activity_at: None,
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    queued_prompts: Vec::new(),
                },
                None,
            )
            .expect("agent");

        assert!(
            !store
                .has_provider_subagent_events("ag_parent_dedupe", "sub_1")
                .expect("empty check")
        );

        // 1. Streaming thinking heartbeats for the same `wake_th_5` keep row count O(1) and
        //    preserve chronological `seq` while advancing `updated_seq` so reconnecting clients
        //    with `since_seq = ev1.seq` still observe updates in their original timeline slot.
        let (ev1, m1) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_5".into(),
                    text: "Analyzing step 1...".into(),
                },
            )
            .expect("hb1");
        assert!(m1);
        let (ev2, m2) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_5".into(),
                    text: "Analyzing step 1... and step 2".into(),
                },
            )
            .expect("hb2");
        assert!(m2);
        assert_eq!(
            ev2.seq, ev1.seq,
            "mutated thinking heartbeat must update in place keeping original seq"
        );
        assert_eq!(
            store
                .provider_subagent_event_count("ag_parent_dedupe", "sub_1")
                .expect("count after mutated thinking"),
            1
        );
        // Client catching up with `since_seq = ev1.seq` MUST observe the mutated thinking event!
        let (caught_up, _) = store
            .fetch_provider_subagent_events_since("ag_parent_dedupe", "sub_1", ev1.seq, 50)
            .expect("catchup after mutation");
        assert_eq!(caught_up.len(), 1);
        assert_eq!(caught_up[0].seq, ev1.seq);
        assert!(matches!(
            &caught_up[0].payload,
            TimelinePayload::AssistantThinking { text, .. } if text.contains("step 2")
        ));

        let (ev3, m3) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_5".into(),
                    text: "Analyzing step 1... and step 2".into(),
                },
            )
            .expect("hb3 identical");
        assert!(!m3, "identical heartbeat must be a zero-write no-op");
        assert_eq!(ev3.seq, ev2.seq);

        // 2. Tool call transition (Running -> Completed) keeps O(1) row and original `seq` before reply
        let (tool_run, m_tr) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                TimelinePayload::ToolCall {
                    call_id: "wake_tool_5_0".into(),
                    status: ToolCallStatus::Running,
                    detail: ToolCallDetail::FileRead {
                        path: "src/lib.rs".into(),
                        start_line: None,
                        end_line: None,
                    },
                    background_task: None,
                },
            )
            .expect("tool run");
        assert!(m_tr);
        let msg_payload = TimelinePayload::AssistantMessage {
            item_id: "wake_msg_5".into(),
            text: "Done!".into(),
            streaming: false,
        };
        let tool_payload = TimelinePayload::ToolCall {
            call_id: "wake_tool_5_0".into(),
            status: ToolCallStatus::Completed,
            detail: ToolCallDetail::FileRead {
                path: "src/lib.rs".into(),
                start_line: None,
                end_line: None,
            },
            background_task: None,
        };
        let (tool_done, m_tool) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                tool_payload.clone(),
            )
            .expect("tool");
        let (msg_ev, m_msg) = store
            .append_provider_subagent_event_upsert("ag_parent_dedupe", "sub_1", msg_payload.clone())
            .expect("msg");
        assert!(m_msg && m_tool);
        assert_eq!(
            tool_done.seq, tool_run.seq,
            "completing a subagent tool call must preserve its chronological seq"
        );
        assert!(
            tool_done.seq < msg_ev.seq,
            "completed tool card must stay above assistant reply"
        );
        assert_eq!(
            store
                .provider_subagent_event_count("ag_parent_dedupe", "sub_1")
                .expect("count"),
            3
        );

        // Replaying a stale `Running` update for an already-completed tool call must be ignored.
        let (downgrade_ev, downgrade_mut) = store
            .append_provider_subagent_event_upsert(
                "ag_parent_dedupe",
                "sub_1",
                TimelinePayload::ToolCall {
                    call_id: "wake_tool_5_0".into(),
                    status: ToolCallStatus::Running,
                    detail: ToolCallDetail::FileRead {
                        path: "src/lib.rs".into(),
                        start_line: None,
                        end_line: None,
                    },
                    background_task: None,
                },
            )
            .expect("stale running downgrade replay");
        assert!(
            !downgrade_mut,
            "terminal subagent tool call must not downgrade back to Running"
        );
        assert!(matches!(
            downgrade_ev.payload,
            TimelinePayload::ToolCall {
                status: ToolCallStatus::Completed,
                ..
            }
        ));

        // Replay the entire historical sequence twice (simulating 2 daemon restarts)
        for _ in 0..2 {
            let (_, rm1) = store
                .append_provider_subagent_event_upsert(
                    "ag_parent_dedupe",
                    "sub_1",
                    TimelinePayload::AssistantThinking {
                        item_id: "wake_th_5".into(),
                        text: "Analyzing step 1... and step 2".into(),
                    },
                )
                .expect("replay th");
            let (_, rm2) = store
                .append_provider_subagent_event_upsert(
                    "ag_parent_dedupe",
                    "sub_1",
                    msg_payload.clone(),
                )
                .expect("replay msg");
            let (_, rm3) = store
                .append_provider_subagent_event_upsert(
                    "ag_parent_dedupe",
                    "sub_1",
                    tool_payload.clone(),
                )
                .expect("replay tool");
            assert!(
                !rm1 && !rm2 && !rm3,
                "historical replay must not mutate or duplicate"
            );
        }
        assert_eq!(
            store
                .provider_subagent_event_count("ag_parent_dedupe", "sub_1")
                .expect("count after replay"),
            3
        );
        let (ordered, _) = store
            .fetch_provider_subagent_events_since("ag_parent_dedupe", "sub_1", 0, 50)
            .expect("ordered fetch");
        assert_eq!(ordered.len(), 3);
        assert!(matches!(
            ordered[0].payload,
            TimelinePayload::AssistantThinking { .. }
        ));
        assert!(matches!(
            ordered[1].payload,
            TimelinePayload::ToolCall {
                status: ToolCallStatus::Completed,
                ..
            }
        ));
        assert!(matches!(
            ordered[2].payload,
            TimelinePayload::AssistantMessage { .. }
        ));
        assert!(
            store
                .has_provider_subagent_events("ag_parent_dedupe", "sub_1")
                .expect("has events")
        );

        // 3. Main timeline `AssistantThinking` heartbeats advance seq on mutation (1 row) and no-op on identical
        let (t1, tm1) = store
            .append_timeline_event_deduped(
                "ag_parent_dedupe",
                1,
                None,
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_main_9".into(),
                    text: "Thinking part A".into(),
                },
            )
            .expect("main th1");
        assert!(tm1);
        let (t1_dup, tm1_dup) = store
            .append_timeline_event_deduped(
                "ag_parent_dedupe",
                1,
                None,
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_main_9".into(),
                    text: "Thinking part A".into(),
                },
            )
            .expect("main th1 dup");
        assert!(!tm1_dup);
        assert_eq!(t1_dup.seq, t1.seq);
        let (t2, tm2) = store
            .append_timeline_event_deduped(
                "ag_parent_dedupe",
                1,
                None,
                TimelinePayload::AssistantThinking {
                    item_id: "wake_th_main_9".into(),
                    text: "Thinking part A + B".into(),
                },
            )
            .expect("main th2");
        assert!(tm2);
        assert!(
            t2.seq > t1.seq,
            "mutated main timeline thinking must advance seq past since_seq"
        );
        let (rows, _) = store
            .fetch_timeline_since("ag_parent_dedupe", 0, 50)
            .expect("fetch main");
        assert_eq!(rows.len(), 1);
        assert!(matches!(
            &rows[0].payload,
            TimelinePayload::AssistantThinking { item_id, text }
                if item_id == "wake_th_main_9" && text == "Thinking part A + B"
        ));

        // 4. `upsert_provider_subagent` preserves existing real title when post-restart fallback passes title == subagent_id
        store
            .upsert_provider_subagent(&orbit_protocol::ProviderSubagentDescriptor {
                subagent_id: "sub_title_keep".into(),
                parent_agent_id: "ag_parent_dedupe".into(),
                parent_subagent_id: None,
                title: "Database Migration Engineer".into(),
                subtitle: None,
                model: Some("claude-opus-4-6".into()),
                status: orbit_protocol::ProviderSubagentStatus::Running,
                created_at: now,
                updated_at: now,
            })
            .expect("upsert real title");
        store
            .upsert_provider_subagent(&orbit_protocol::ProviderSubagentDescriptor {
                subagent_id: "sub_title_keep".into(),
                parent_agent_id: "ag_parent_dedupe".into(),
                parent_subagent_id: None,
                title: "sub_title_keep".into(),
                subtitle: None,
                model: None,
                status: orbit_protocol::ProviderSubagentStatus::Completed,
                created_at: now,
                updated_at: now,
            })
            .expect("upsert synthetic id title");
        let listed = store
            .list_provider_subagents("ag_parent_dedupe")
            .expect("list subagents");
        let kept = listed
            .iter()
            .find(|s| s.subagent_id == "sub_title_keep")
            .expect("found");
        assert_eq!(kept.title, "Database Migration Engineer");
        assert_eq!(kept.model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(
            kept.status,
            orbit_protocol::ProviderSubagentStatus::Completed
        );

        // Ensure PRAGMA user_version remains 6
        let conn = store.conn.lock().expect("lock");
        let ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("user_version");
        assert_eq!(ver, 6);
    }

    #[test]
    fn latest_assistant_message_reads_newest_past_500_events() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_long".into(),
                title: "Long".into(),
                cwd: "/tmp/long".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_long".into(),
                    workspace_id: "ws_long".into(),
                    cwd: "/tmp/long".into(),
                    title: "Long Turn".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: orbit_protocol::AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        // Append an early assistant message, then 550 tool calls, then the final assistant message.
        store
            .append_timeline_event(
                "ag_long",
                1,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "m_early".into(),
                    text: "Early message".into(),
                    streaming: false,
                },
            )
            .expect("early msg");
        for i in 0..550 {
            store
                .append_timeline_event(
                    "ag_long",
                    1,
                    None,
                    TimelinePayload::ToolCall {
                        call_id: format!("c_{i}"),
                        status: ToolCallStatus::Completed,
                        detail: ToolCallDetail::FileRead {
                            path: format!("src/f{i}.rs"),
                            start_line: Some(1),
                            end_line: Some(10),
                        },
                        background_task: None,
                    },
                )
                .expect("tool event");
        }
        store
            .append_timeline_event(
                "ag_long",
                1,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "m_final".into(),
                    text: "Final summary past 500 events".into(),
                    streaming: false,
                },
            )
            .expect("final msg");

        let latest = store
            .latest_assistant_message("ag_long")
            .expect("query latest assistant message");
        assert_eq!(latest.as_deref(), Some("Final summary past 500 events"));
    }

    #[test]
    fn client_and_connected_hosts_persist_pairing_secret_and_migrate_legacy_dbs() {
        let tmp = std::env::temp_dir().join(format!(
            "orbit_store_pairing_mig_{}.db",
            uuid::Uuid::new_v4()
        ));

        // Simulate a pre-auth database lacking `pairing_secret` columns
        {
            let raw = Connection::open(&tmp).expect("open raw db");
            raw.execute_batch(
                "
                CREATE TABLE client_hosts (
                    host_id               TEXT PRIMARY KEY,
                    display_name          TEXT NOT NULL,
                    daemon_pubkey_b64     TEXT NOT NULL,
                    direct_endpoints_json TEXT NOT NULL,
                    relay_url             TEXT,
                    meta_json             TEXT,
                    updated_at            TEXT NOT NULL
                );
                INSERT INTO client_hosts (
                    host_id, display_name, daemon_pubkey_b64, direct_endpoints_json, relay_url, meta_json, updated_at
                ) VALUES ('host_legacy', 'legacy-box', 'pk_legacy', '[]', NULL, NULL, '2026-01-01T00:00:00Z');

                CREATE TABLE connected_hosts (
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
                INSERT INTO connected_hosts (
                    registration_id, host_id, label, daemon_pubkey_b64, direct_endpoints_json, relay_url, enabled, created_at, updated_at
                ) VALUES ('ch_legacy', 'host_peer_legacy', 'peer', 'pk_peer', '[]', NULL, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                ",
            )
            .expect("seed legacy schema");
        }

        // Opening through `OrbitStore::open` must migrate cleanly without data loss
        let store = OrbitStore::open(&tmp).expect("migrate legacy db");
        let legacy_clients = store.list_client_hosts().expect("list legacy clients");
        assert_eq!(legacy_clients.len(), 1);
        assert_eq!(legacy_clients[0].host_id, "host_legacy");
        assert_eq!(legacy_clients[0].pairing_secret, "");

        let legacy_connected = store.list_connected_hosts().expect("list legacy connected");
        assert_eq!(legacy_connected.len(), 1);
        assert_eq!(legacy_connected[0].registration_id, "ch_legacy");
        assert_eq!(legacy_connected[0].pairing_secret, "");

        // Upserting with a real secret persists and updates it directly
        let mut updated = legacy_clients[0].clone();
        updated.pairing_secret = "persisted-secret-value-0123456789abcdef0123".into();
        updated.meta.pairing_secret_updated_at_ms = 12345;
        store.upsert_client_host(&updated).expect("upsert secret");
        let loaded = &store.list_client_hosts().expect("list")[0];
        assert_eq!(
            loaded.pairing_secret,
            "persisted-secret-value-0123456789abcdef0123"
        );
        assert_eq!(loaded.meta.pairing_secret_updated_at_ms, 12345);
        updated.pairing_secret = "rotated-secret-value-9876543210fedcba98765".into();
        store.upsert_client_host(&updated).expect("rotate secret");
        assert_eq!(
            store.list_client_hosts().expect("list")[0].pairing_secret,
            "rotated-secret-value-9876543210fedcba98765"
        );

        let mut peer = legacy_connected[0].clone();
        peer.pairing_secret = "peer-secret-value-0123456789abcdef01234567".into();
        store
            .upsert_connected_host(&peer)
            .expect("upsert connected secret");
        assert_eq!(
            store
                .get_connected_host("ch_legacy")
                .expect("get")
                .expect("some")
                .pairing_secret,
            "peer-secret-value-0123456789abcdef01234567"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_open_enforces_0600_permissions_on_db_and_sidecars() {
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!("orbit_store_perm_{}", uuid::Uuid::new_v4()));
        let orbit_dir = base.join(".orbit");
        std::fs::create_dir_all(&orbit_dir).expect("mkdir .orbit");
        std::fs::set_permissions(&orbit_dir, std::fs::Permissions::from_mode(0o755))
            .expect("set 0755");
        let db_path = orbit_dir.join("desktop-client.sqlite");

        // 1. Fresh DB is born 0600 and `.orbit` parent dir is hardened to 0700
        let store = OrbitStore::open(&db_path).expect("open fresh store");
        let dir_mode = std::fs::metadata(&orbit_dir)
            .expect("dir meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
        let db_mode = std::fs::metadata(&db_path)
            .expect("db meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(db_mode, 0o600);
        drop(store);

        // 2. Pre-existing lax permissions (0644) on DB and `-wal` / `-shm` sidecars are hardened to 0600 on reopen
        let mut wal_os = db_path.as_os_str().to_os_string();
        wal_os.push("-wal");
        let wal_path = std::path::PathBuf::from(wal_os);
        let mut shm_os = db_path.as_os_str().to_os_string();
        shm_os.push("-shm");
        let shm_path = std::path::PathBuf::from(shm_os);
        std::fs::write(&wal_path, b"").expect("touch wal");
        std::fs::write(&shm_path, b"").expect("touch shm");
        for p in [&db_path, &wal_path, &shm_path] {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o644)).expect("set 0644");
        }

        let _reopened = OrbitStore::open(&db_path).expect("reopen store");
        for p in [&db_path, &wal_path, &shm_path] {
            if p.exists() {
                let mode = std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "expected 0600 on {}", p.display());
            }
        }

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn late_assistant_finalization_after_user_prompt_updates_pre_prompt_row_in_place() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();

        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_late_fin".into(),
                title: "orbit".into(),
                cwd: "/workspace/orbit".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert ws");

        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_late_fin".into(),
                    workspace_id: "ws_late_fin".into(),
                    cwd: "/workspace/orbit".into(),
                    title: "Late Finalization Agent".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");

        // Turn 1: prompt + streaming assistant reply
        let p1 = store
            .append_timeline_event(
                "ag_late_fin",
                1,
                Some("turn_1"),
                TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "First question".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p1");
        let a1_stream = store
            .append_timeline_event(
                "ag_late_fin",
                1,
                Some("turn_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_turn_1_1".into(),
                    text: "DraftAnswerTokenBeforePrompt".into(),
                    streaming: true,
                },
            )
            .expect("a1 stream");

        // Turn 2 prompt lands BEFORE Turn 1's `streaming: false` finalization arrives
        let p2 = store
            .append_timeline_event(
                "ag_late_fin",
                1,
                Some("turn_2"),
                TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "Second question".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p2");

        // Late finalization for Turn 1's `item_id` arrives AFTER `p2`
        let (a1_final, mutated) = store
            .append_timeline_event_deduped(
                "ag_late_fin",
                1,
                Some("turn_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_turn_1_1".into(),
                    text: "FinalAnswerTokenUnique99".into(),
                    streaming: false,
                },
            )
            .expect("late final");
        assert!(mutated);
        assert_eq!(
            a1_final.seq, a1_stream.seq,
            "late finalization MUST update original pre-prompt row in place"
        );

        let ag = store
            .get_agent("ag_late_fin")
            .expect("get agent")
            .expect("agent exists");
        assert_eq!(
            ag.timeline_epoch, 1,
            "in-place update must bump timeline_epoch"
        );
        assert_eq!(
            ag.latest_seq, p2.seq,
            "latest_seq must stay at p2 without inserting a duplicate row after p2"
        );

        let (events, _) = store
            .fetch_timeline_since("ag_late_fin", 0, 50)
            .expect("fetch timeline");
        assert_eq!(
            events.len(),
            3,
            "timeline must contain exactly [p1, a1_final, p2] with zero duplicate after p2"
        );
        assert_eq!(events[0].seq, p1.seq);
        assert_eq!(events[1].seq, a1_stream.seq);
        assert_eq!(events[2].seq, p2.seq);
        assert!(matches!(
            &events[1].payload,
            TimelinePayload::AssistantMessage {
                item_id,
                text,
                streaming: false,
            } if item_id == "agy_msg_turn_1_1" && text == "FinalAnswerTokenUnique99"
        ));

        // FTS5 index reflects the finalized text at the original pre-prompt seq
        let hits = store
            .search_daemon_messages("FinalAnswerTokenUnique99", 10)
            .expect("fts search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].seq, a1_stream.seq);

        // Re-sending the exact same finalized payload is a no-op (`mutated == false`) and does not duplicate
        let (a1_dup, mutated_dup) = store
            .append_timeline_event_deduped(
                "ag_late_fin",
                1,
                Some("turn_1"),
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_turn_1_1".into(),
                    text: "FinalAnswerTokenUnique99".into(),
                    streaming: false,
                },
            )
            .expect("dup final");
        assert!(!mutated_dup);
        assert_eq!(a1_dup.seq, a1_stream.seq);
        let (events_after_dup, _) = store
            .fetch_timeline_since("ag_late_fin", 0, 50)
            .expect("fetch timeline after dup");
        assert_eq!(events_after_dup.len(), 3);

        // Turn-scoped protection: a DIFFERENT turn reusing the same `item_id` MUST NOT overwrite
        // Turn 1's reply — it inserts a fresh row after `p2` instead!
        let (a2_same_id, mutated_t2) = store
            .append_timeline_event_deduped(
                "ag_late_fin",
                1,
                Some("turn_2"),
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_turn_1_1".into(),
                    text: "TurnTwoDistinctAnswer".into(),
                    streaming: false,
                },
            )
            .expect("turn 2 reused item_id");
        assert!(mutated_t2);
        assert!(
            a2_same_id.seq > p2.seq,
            "different turn_id must append after p2 and never clobber turn_1's row"
        );
        let (events_t2, _) = store
            .fetch_timeline_since("ag_late_fin", 0, 50)
            .expect("fetch timeline t2");
        assert_eq!(events_t2.len(), 4);
        assert!(matches!(
            &events_t2[1].payload,
            TimelinePayload::AssistantMessage { text, .. } if text == "FinalAnswerTokenUnique99"
        ));
        assert!(matches!(
            &events_t2[3].payload,
            TimelinePayload::AssistantMessage { text, .. } if text == "TurnTwoDistinctAnswer"
        ));

        // Legacy shared item IDs (e.g. `agy_msg_default`) never coalesce back across turn boundaries
        let legacy_1 = store
            .append_timeline_event(
                "ag_late_fin",
                1,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_default".into(),
                    text: "Legacy turn 2".into(),
                    streaming: false,
                },
            )
            .expect("legacy 1");
        let p3 = store
            .append_timeline_event(
                "ag_late_fin",
                1,
                Some("turn_3"),
                TimelinePayload::UserPrompt {
                    message_id: "u3".into(),
                    text: "Third question".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p3");
        let (legacy_2, _) = store
            .append_timeline_event_deduped(
                "ag_late_fin",
                1,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "agy_msg_default".into(),
                    text: "Legacy turn 3".into(),
                    streaming: false,
                },
            )
            .expect("legacy 2");
        assert!(
            legacy_2.seq > p3.seq && legacy_2.seq != legacy_1.seq,
            "legacy shared item_id must never overwrite a pre-boundary row"
        );
    }

    fn store_with_tail_agent() -> OrbitStore {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "ws".into(),
                cwd: "/tmp/tail".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_tail".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp/tail".into(),
                    title: "Tail".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
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
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert");

        store
    }

    #[test]
    fn outline_query_reads_only_this_agents_prompts_across_pages() {
        let store = store_with_tail_agent();
        let mut other = store
            .get_agent("ag_tail")
            .expect("read")
            .expect("seed agent");
        other.agent_id = "other".into();
        store.upsert_agent(&other, None).expect("second agent");
        let append = |agent: &str, text: &str, badges| {
            store
                .append_timeline_event(
                    agent,
                    0,
                    None,
                    TimelinePayload::UserPrompt {
                        message_id: text.into(),
                        text: text.into(),
                        badges,
                    },
                )
                .expect("append prompt")
                .seq
        };
        let first = append(
            "ag_tail",
            "<orbit-context>hidden</orbit-context> first\n prompt",
            vec![],
        );
        append("other", "different agent", vec![]);
        append(
            "ag_tail",
            "",
            vec![PromptBadge {
                kind: "slash".into(),
                label: "/compact".into(),
                path: None,
            }],
        );
        let last = append("ag_tail", "last", vec![]);
        store
            .append_timeline_event(
                "ag_tail",
                0,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "reply".into(),
                    text: "tail".into(),
                    streaming: false,
                },
            )
            .expect("append reply");
        assert!(
            store
                .fetch_timeline_tail_budgeted("ag_tail", 1, usize::MAX)
                .expect("tail")
                .1
        );
        assert_eq!(
            store
                .fetch_timeline_outline_prompts("ag_tail")
                .expect("outline"),
            vec![
                TimelineOutlinePrompt {
                    seq: first,
                    preview: "first prompt".into()
                },
                TimelineOutlinePrompt {
                    seq: last,
                    preview: "last".into()
                },
            ]
        );

        let all_events = store
            .fetch_timeline_since("ag_tail", 0, 100)
            .expect("all")
            .0;
        store
            .upsert_replica_timeline_events_batch_for_host("host_1", &all_events)
            .expect("replica upsert");
        let (rep_tail, rep_has_older) = store
            .fetch_replica_timeline_tail("ag_tail", 1)
            .expect("replica tail");
        assert_eq!(rep_tail.len(), 1);
        assert!(rep_has_older);
        assert_eq!(
            store
                .fetch_replica_outline_prompts("ag_tail")
                .expect("replica outline"),
            vec![
                TimelineOutlinePrompt {
                    seq: first,
                    preview: "first prompt".into()
                },
                TimelineOutlinePrompt {
                    seq: last,
                    preview: "last".into()
                },
            ]
        );
    }

    #[test]
    fn tail_and_before_timeline_pagination_respect_limits_and_ascending_order() {
        let store = store_with_tail_agent();
        let mut seqs = Vec::new();
        for i in 1..=5 {
            let ev = store
                .append_timeline_event(
                    "ag_tail",
                    0,
                    None,
                    TimelinePayload::AssistantMessage {
                        item_id: format!("m{i}"),
                        text: format!("msg-{i}-{}", "x".repeat(400)),
                        streaming: false,
                    },
                )
                .expect("append");
            seqs.push(ev.seq);
        }

        // Tail count cap returns newest 2 events in ascending order + has_older=true
        let (tail, has_older) = store
            .fetch_timeline_tail_budgeted("ag_tail", 2, usize::MAX)
            .expect("tail");
        assert!(has_older);
        assert_eq!(
            tail.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![seqs[3], seqs[4]]
        );

        // Backward page before tail[0] yields the next 2 older events in ascending order
        let (older1, more1) = store
            .fetch_timeline_before_budgeted("ag_tail", tail[0].seq, 2, usize::MAX)
            .expect("before 1");
        assert!(more1);
        assert_eq!(
            older1.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![seqs[1], seqs[2]]
        );

        // Final backward page reaches seq[0] with has_more_older=false
        let (older2, more2) = store
            .fetch_timeline_before_budgeted("ag_tail", older1[0].seq, 10, usize::MAX)
            .expect("before 2");
        assert!(!more2);
        assert_eq!(
            older2.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![seqs[0]]
        );

        // Replica tail & before work symmetrically
        let mut all_evs = older2;
        all_evs.extend(older1);
        all_evs.extend(tail);
        store
            .upsert_replica_timeline_events_batch(&all_evs)
            .expect("seed replica");

        let (rep_tail, rep_has_older) = store
            .fetch_replica_timeline_tail("ag_tail", 2)
            .expect("rep tail");
        assert!(rep_has_older);
        assert_eq!(
            rep_tail.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![seqs[3], seqs[4]]
        );
        assert!(
            store
                .has_replica_timeline_before("ag_tail", rep_tail[0].seq)
                .expect("has before")
        );
        let (rep_before, rep_more) = store
            .fetch_replica_timeline_before("ag_tail", rep_tail[0].seq, 10)
            .expect("rep before");
        assert!(!rep_more);
        assert_eq!(
            rep_before.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![seqs[0], seqs[1], seqs[2]]
        );
    }

    #[test]
    fn agent_message_delivery_updates_patch_original_row_preserving_seq() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "ws".into(),
                cwd: "/tmp/w".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert ws");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: "ag_deliv".into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp/w".into(),
                    title: "Parent".into(),
                    provider: ProviderKind::Antigravity,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    acp_agent_id: None,
                    mode: SessionMode::AcceptEdits,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    queued_prompts: vec![],
                    latest_seq: 0,
                    timeline_epoch: 0,
                    created_at: now,
                    updated_at: now,
                },
                None,
            )
            .expect("upsert agent");

        let ev_msg = store
            .append_timeline_event(
                "ag_deliv",
                1,
                Some("turn_fin"),
                TimelinePayload::AgentMessage {
                    message_id: "fin_42".into(),
                    kind: orbit_protocol::AgentMessageKind::FinishNotification,
                    source_agent_id: Some("ag_child".into()),
                    source_host_id: None,
                    source_title: Some("Child".into()),
                    event: Some("finished".into()),
                    text: "Child completed".into(),
                    permission_context: None,
                    delivery_status: Some(orbit_protocol::AgentMessageDeliveryStatus::Queued),
                    delivery_turn_id: Some("turn_fin".into()),
                    blocking_turn_id: Some("turn_active".into()),
                    queued_at_ms: Some(1_000),
                    dispatched_at_ms: None,
                    settled_at_ms: None,
                },
            )
            .expect("append AgentMessage");

        let ev_reply = store
            .append_timeline_event(
                "ag_deliv",
                1,
                Some("turn_active"),
                TimelinePayload::AssistantMessage {
                    item_id: "msg_1".into(),
                    text: "Parent active reply finished".into(),
                    streaming: false,
                },
            )
            .expect("append AssistantMessage");

        let ev_disp = store
            .append_timeline_event(
                "ag_deliv",
                1,
                None,
                TimelinePayload::AgentMessageDeliveryUpdated {
                    message_id: "fin_42".into(),
                    status: orbit_protocol::AgentMessageDeliveryStatus::Dispatched,
                    delivery_turn_id: Some("turn_fin".into()),
                    blocking_turn_id: None,
                    timestamp_ms: 2_500,
                },
            )
            .expect("append Dispatched");

        let ev_settled = store
            .append_timeline_event(
                "ag_deliv",
                1,
                None,
                TimelinePayload::AgentMessageDeliveryUpdated {
                    message_id: "fin_42".into(),
                    status: orbit_protocol::AgentMessageDeliveryStatus::Settled,
                    delivery_turn_id: Some("turn_fin".into()),
                    blocking_turn_id: None,
                    timestamp_ms: 4_200,
                },
            )
            .expect("append Settled");

        assert!(ev_msg.seq < ev_reply.seq);
        assert!(ev_reply.seq < ev_disp.seq);
        assert!(ev_disp.seq < ev_settled.seq);

        let (events, _) = store
            .fetch_timeline_since("ag_deliv", 0, 50)
            .expect("fetch timeline");
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].seq, ev_msg.seq);
        match &events[0].payload {
            TimelinePayload::AgentMessage {
                delivery_status,
                blocking_turn_id,
                queued_at_ms,
                dispatched_at_ms,
                settled_at_ms,
                ..
            } => {
                assert_eq!(
                    *delivery_status,
                    Some(orbit_protocol::AgentMessageDeliveryStatus::Settled)
                );
                assert!(blocking_turn_id.is_none());
                assert_eq!(*queued_at_ms, Some(1_000));
                assert_eq!(*dispatched_at_ms, Some(2_500));
                assert_eq!(*settled_at_ms, Some(4_200));
            }
            other => panic!("unexpected payload: {other:?}"),
        }

        // Verify replica table also patches in place on batch upsert
        store
            .upsert_replica_timeline_events_batch(&[
                ev_msg.clone(),
                ev_reply.clone(),
                ev_disp,
                ev_settled,
            ])
            .expect("upsert replica batch");
        let (rep_events, _) = store
            .fetch_replica_timeline_tail("ag_deliv", 10)
            .expect("fetch replica");
        match &rep_events[0].payload {
            TimelinePayload::AgentMessage {
                delivery_status,
                dispatched_at_ms,
                settled_at_ms,
                ..
            } => {
                assert_eq!(
                    *delivery_status,
                    Some(orbit_protocol::AgentMessageDeliveryStatus::Settled)
                );
                assert_eq!(*dispatched_at_ms, Some(2_500));
                assert_eq!(*settled_at_ms, Some(4_200));
            }
            other => panic!("unexpected replica payload: {other:?}"),
        }
    }
}
