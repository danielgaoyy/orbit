//! SQLite persistence for Unattended `/goal` runs and streaming `Monitor` watches.
//!
//! Creates `goal_runs` and `monitor_watches` idempotently via `CREATE TABLE IF NOT EXISTS`
//! without altering `PRAGMA user_version` (keeps `user_version = 6`).

use orbit_protocol::{
    GoalCriterion, GoalRunRecord, GoalStatus, MonitorKind, MonitorStatus, MonitorWatchRecord,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{OrbitStore, StoreError};

/// Idempotent schema bootstrap for `goal_runs` and `monitor_watches`.
///
/// Intentionally does NOT alter `PRAGMA user_version` (stays at `6`).
pub(crate) fn migrate_goals_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS goal_runs (
            goal_id                        TEXT PRIMARY KEY,
            agent_id                       TEXT NOT NULL,
            workspace_id                   TEXT NOT NULL,
            objective                      TEXT NOT NULL,
            criteria_json                  TEXT NOT NULL DEFAULT '[]',
            verify_command                 TEXT,
            max_iterations                 INTEGER NOT NULL,
            current_iteration              INTEGER NOT NULL DEFAULT 0,
            max_duration_secs              INTEGER NOT NULL,
            status                         TEXT NOT NULL,
            last_verify_exit_code          INTEGER,
            last_verify_output             TEXT,
            consecutive_identical_failures INTEGER NOT NULL DEFAULT 0,
            last_failure_hash              TEXT,
            started_at_ms                  INTEGER NOT NULL,
            updated_at_ms                  INTEGER NOT NULL,
            finished_at_ms                 INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_goal_runs_agent_updated
            ON goal_runs(agent_id, updated_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_goal_runs_status_updated
            ON goal_runs(status, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS monitor_watches (
            monitor_id           TEXT PRIMARY KEY,
            agent_id             TEXT NOT NULL,
            name                 TEXT NOT NULL,
            kind_json            TEXT NOT NULL,
            status               TEXT NOT NULL,
            match_count          INTEGER NOT NULL DEFAULT 0,
            last_event_preview   TEXT,
            last_triggered_at_ms INTEGER,
            expires_at_ms        INTEGER,
            created_at_ms        INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_monitor_watches_agent_created
            ON monitor_watches(agent_id, created_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_monitor_watches_status_created
            ON monitor_watches(status, created_at_ms DESC);
        ",
    )?;
    Ok(())
}

fn map_goal_run_row(row: &Row<'_>) -> rusqlite::Result<(GoalRunRecord, Option<String>)> {
    let criteria_json: String = row.get(4)?;
    let criteria: Vec<GoalCriterion> = serde_json::from_str(&criteria_json).unwrap_or_default();
    let max_iterations_i64: i64 = row.get(6)?;
    let current_iteration_i64: i64 = row.get(7)?;
    let max_duration_i64: i64 = row.get(8)?;
    let status_raw: String = row.get(9)?;
    let status = GoalStatus::from_str_opt(&status_raw).unwrap_or(GoalStatus::Failed);
    let last_verify_exit_code: Option<i32> = row.get(10)?;
    let last_verify_output: Option<String> = row.get(11)?;
    let consecutive_i64: i64 = row.get(12)?;
    let last_failure_hash: Option<String> = row.get(13)?;
    let started_at_ms: i64 = row.get(14)?;
    let updated_at_ms: i64 = row.get(15)?;
    let finished_at_ms: Option<i64> = row.get(16)?;

    Ok((
        GoalRunRecord {
            goal_id: row.get(0)?,
            agent_id: row.get(1)?,
            workspace_id: row.get(2)?,
            objective: row.get(3)?,
            criteria,
            verify_command: row.get(5)?,
            max_iterations: max_iterations_i64.clamp(1, i64::from(u32::MAX)) as u32,
            current_iteration: current_iteration_i64.clamp(0, i64::from(u32::MAX)) as u32,
            max_duration_secs: max_duration_i64.max(0) as u64,
            status,
            last_verify_exit_code,
            last_verify_output,
            consecutive_identical_failures: consecutive_i64.clamp(0, i64::from(u32::MAX)) as u32,
            started_at_ms: started_at_ms.max(0) as u64,
            updated_at_ms: updated_at_ms.max(0) as u64,
            finished_at_ms: finished_at_ms.map(|v| v.max(0) as u64),
        },
        last_failure_hash,
    ))
}

fn map_monitor_watch_row(row: &Row<'_>) -> rusqlite::Result<MonitorWatchRecord> {
    let kind_json: String = row.get(3)?;
    let kind: MonitorKind = serde_json::from_str(&kind_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let status_raw: String = row.get(4)?;
    let status = MonitorStatus::from_str_opt(&status_raw).unwrap_or(MonitorStatus::Stopped);
    let match_count_i64: i64 = row.get(5)?;
    let last_triggered_at_ms: Option<i64> = row.get(7)?;
    let expires_at_ms: Option<i64> = row.get(8)?;
    let created_at_ms: i64 = row.get(9)?;

    Ok(MonitorWatchRecord {
        monitor_id: row.get(0)?,
        agent_id: row.get(1)?,
        name: row.get(2)?,
        kind,
        status,
        match_count: match_count_i64.max(0) as u64,
        last_event_preview: row.get(6)?,
        last_triggered_at_ms: last_triggered_at_ms.map(|v| v.max(0) as u64),
        expires_at_ms: expires_at_ms.map(|v| v.max(0) as u64),
        created_at_ms: created_at_ms.max(0) as u64,
    })
}

const GOAL_SELECT_COLS: &str = "
    goal_id, agent_id, workspace_id, objective, criteria_json, verify_command,
    max_iterations, current_iteration, max_duration_secs, status,
    last_verify_exit_code, last_verify_output, consecutive_identical_failures,
    last_failure_hash, started_at_ms, updated_at_ms, finished_at_ms
";

const MONITOR_SELECT_COLS: &str = "
    monitor_id, agent_id, name, kind_json, status, match_count,
    last_event_preview, last_triggered_at_ms, expires_at_ms, created_at_ms
";

impl OrbitStore {
    /// Insert or update a Goal run record together with its optional failure-dedup hash.
    pub fn upsert_goal_run(
        &self,
        goal: &GoalRunRecord,
        last_failure_hash: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let criteria_json = serde_json::to_string(&goal.criteria)?;
        conn.execute(
            "INSERT INTO goal_runs (
                goal_id, agent_id, workspace_id, objective, criteria_json, verify_command,
                max_iterations, current_iteration, max_duration_secs, status,
                last_verify_exit_code, last_verify_output, consecutive_identical_failures,
                last_failure_hash, started_at_ms, updated_at_ms, finished_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
            ON CONFLICT(goal_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                workspace_id = excluded.workspace_id,
                objective = excluded.objective,
                criteria_json = excluded.criteria_json,
                verify_command = excluded.verify_command,
                max_iterations = excluded.max_iterations,
                current_iteration = excluded.current_iteration,
                max_duration_secs = excluded.max_duration_secs,
                status = excluded.status,
                last_verify_exit_code = excluded.last_verify_exit_code,
                last_verify_output = excluded.last_verify_output,
                consecutive_identical_failures = excluded.consecutive_identical_failures,
                last_failure_hash = excluded.last_failure_hash,
                started_at_ms = excluded.started_at_ms,
                updated_at_ms = excluded.updated_at_ms,
                finished_at_ms = excluded.finished_at_ms",
            params![
                goal.goal_id,
                goal.agent_id,
                goal.workspace_id,
                goal.objective,
                criteria_json,
                goal.verify_command,
                i64::from(goal.max_iterations),
                i64::from(goal.current_iteration),
                goal.max_duration_secs as i64,
                goal.status.as_str(),
                goal.last_verify_exit_code,
                goal.last_verify_output,
                i64::from(goal.consecutive_identical_failures),
                last_failure_hash,
                goal.started_at_ms as i64,
                goal.updated_at_ms as i64,
                goal.finished_at_ms.map(|v| v as i64),
            ],
        )?;
        Ok(())
    }

    /// Fetch a Goal run record and its internal failure-dedup hash by `goal_id`.
    pub fn get_goal_run(
        &self,
        goal_id: &str,
    ) -> Result<Option<(GoalRunRecord, Option<String>)>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {GOAL_SELECT_COLS} FROM goal_runs WHERE goal_id = ?1");
        conn.query_row(&sql, params![goal_id], map_goal_run_row)
            .optional()
            .map_err(Into::into)
    }

    /// Fetch the most recent non-terminal (`running`, `verifying`, `paused`) Goal run for an agent.
    pub fn get_active_goal_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<(GoalRunRecord, Option<String>)>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {GOAL_SELECT_COLS}
             FROM goal_runs
             WHERE agent_id = ?1 AND status IN ('running', 'verifying', 'paused')
             ORDER BY updated_at_ms DESC
             LIMIT 1"
        );
        conn.query_row(&sql, params![agent_id], map_goal_run_row)
            .optional()
            .map_err(Into::into)
    }

    /// List Goal runs (most recently updated first), optionally scoped to one `agent_id`.
    pub fn list_goal_runs(&self, agent_id: Option<&str>) -> Result<Vec<GoalRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let ag = agent_id.map(str::trim).filter(|s| !s.is_empty());
        match ag {
            Some(aid) => {
                let sql = format!(
                    "SELECT {GOAL_SELECT_COLS}
                     FROM goal_runs
                     WHERE agent_id = ?1
                     ORDER BY updated_at_ms DESC, goal_id DESC
                     LIMIT 200"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![aid], map_goal_run_row)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?.0);
                }
                Ok(out)
            }
            None => {
                let sql = format!(
                    "SELECT {GOAL_SELECT_COLS}
                     FROM goal_runs
                     ORDER BY updated_at_ms DESC, goal_id DESC
                     LIMIT 200"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], map_goal_run_row)?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row?.0);
                }
                Ok(out)
            }
        }
    }

    /// List ALL non-terminal (`running`, `verifying`, `paused`) Goal runs with no row-count cap,
    /// ensuring recovery and shutdown never miss active goals behind historical completed rows.
    pub fn list_active_goal_runs(&self) -> Result<Vec<GoalRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {GOAL_SELECT_COLS}
             FROM goal_runs
             WHERE status IN ('running', 'verifying', 'paused')
             ORDER BY updated_at_ms DESC, goal_id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_goal_run_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?.0);
        }
        Ok(out)
    }

    /// Delete a Goal run record by `goal_id`.
    pub fn delete_goal_run(&self, goal_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected =
            conn.execute("DELETE FROM goal_runs WHERE goal_id = ?1", params![goal_id])?;
        Ok(affected > 0)
    }

    /// Insert or update a streaming Monitor watch record.
    pub fn upsert_monitor_watch(&self, watch: &MonitorWatchRecord) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let kind_json = serde_json::to_string(&watch.kind)?;
        conn.execute(
            "INSERT INTO monitor_watches (
                monitor_id, agent_id, name, kind_json, status, match_count,
                last_event_preview, last_triggered_at_ms, expires_at_ms, created_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(monitor_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                name = excluded.name,
                kind_json = excluded.kind_json,
                status = excluded.status,
                match_count = excluded.match_count,
                last_event_preview = excluded.last_event_preview,
                last_triggered_at_ms = excluded.last_triggered_at_ms,
                expires_at_ms = excluded.expires_at_ms,
                created_at_ms = excluded.created_at_ms",
            params![
                watch.monitor_id,
                watch.agent_id,
                watch.name,
                kind_json,
                watch.status.as_str(),
                watch.match_count as i64,
                watch.last_event_preview,
                watch.last_triggered_at_ms.map(|v| v as i64),
                watch.expires_at_ms.map(|v| v as i64),
                watch.created_at_ms as i64,
            ],
        )?;
        Ok(())
    }

    /// Fetch a Monitor watch record by `monitor_id`.
    pub fn get_monitor_watch(
        &self,
        monitor_id: &str,
    ) -> Result<Option<MonitorWatchRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql =
            format!("SELECT {MONITOR_SELECT_COLS} FROM monitor_watches WHERE monitor_id = ?1");
        conn.query_row(&sql, params![monitor_id], map_monitor_watch_row)
            .optional()
            .map_err(Into::into)
    }

    /// List Monitor watches (most recently created first), optionally scoped to one `agent_id`.
    pub fn list_monitor_watches(
        &self,
        agent_id: Option<&str>,
    ) -> Result<Vec<MonitorWatchRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let ag = agent_id.map(str::trim).filter(|s| !s.is_empty());
        match ag {
            Some(aid) => {
                let sql = format!(
                    "SELECT {MONITOR_SELECT_COLS}
                     FROM monitor_watches
                     WHERE agent_id = ?1
                     ORDER BY created_at_ms DESC, monitor_id DESC
                     LIMIT 200"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![aid], map_monitor_watch_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
            None => {
                let sql = format!(
                    "SELECT {MONITOR_SELECT_COLS}
                     FROM monitor_watches
                     ORDER BY created_at_ms DESC, monitor_id DESC
                     LIMIT 200"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], map_monitor_watch_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
        }
    }

    /// List ALL running (`active`, `triggered`) Monitor watches with no row-count cap,
    /// ensuring startup recovery and expiry reaping never miss active watches behind old history.
    pub fn list_running_monitor_watches(&self) -> Result<Vec<MonitorWatchRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {MONITOR_SELECT_COLS}
             FROM monitor_watches
             WHERE status IN ('active', 'triggered')
             ORDER BY created_at_ms DESC, monitor_id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_monitor_watch_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Delete a Monitor watch record by `monitor_id`.
    pub fn delete_monitor_watch(&self, monitor_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM monitor_watches WHERE monitor_id = ?1",
            params![monitor_id],
        )?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_and_monitor_tables_preserve_user_version_6_and_roundtrip() {
        let store = OrbitStore::open_in_memory().expect("open in memory store");
        let user_ver: i64 = {
            let conn = store.conn.lock().expect("lock conn");
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("read user_version")
        };
        assert_eq!(user_ver, 6, "PRAGMA user_version must remain 6");

        let goal = GoalRunRecord {
            goal_id: "goal_test01".into(),
            agent_id: "ag_1".into(),
            workspace_id: "ws_1".into(),
            objective: "Make all unit tests pass".into(),
            criteria: vec![
                GoalCriterion {
                    id: "c1".into(),
                    label: "cargo test passes".into(),
                    passed: false,
                },
                GoalCriterion {
                    id: "c2".into(),
                    label: "no warnings".into(),
                    passed: true,
                },
            ],
            verify_command: Some("cargo test".into()),
            max_iterations: 10,
            current_iteration: 2,
            max_duration_secs: 3600,
            status: GoalStatus::Running,
            last_verify_exit_code: Some(1),
            last_verify_output: Some("1 test failed".into()),
            consecutive_identical_failures: 1,
            started_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_005_000,
            finished_at_ms: None,
        };
        store
            .upsert_goal_run(&goal, Some("hash_abc"))
            .expect("upsert goal");

        let (loaded, hash) = store
            .get_goal_run("goal_test01")
            .expect("get goal")
            .expect("goal exists");
        assert_eq!(loaded, goal);
        assert_eq!(hash.as_deref(), Some("hash_abc"));

        let (active, _) = store
            .get_active_goal_for_agent("ag_1")
            .expect("get active goal")
            .expect("active goal exists");
        assert_eq!(active.goal_id, "goal_test01");

        let list_ag = store.list_goal_runs(Some("ag_1")).expect("list ag_1");
        assert_eq!(list_ag.len(), 1);
        let list_other = store.list_goal_runs(Some("ag_other")).expect("list other");
        assert!(list_other.is_empty());

        // Monitor watches round-trip
        let watch = MonitorWatchRecord {
            monitor_id: "mon_test01".into(),
            agent_id: "ag_1".into(),
            name: "CI Build Watch".into(),
            kind: MonitorKind::CommandStream {
                command: "echo".into(),
                args: vec!["hello".into()],
                pattern: Some("ERROR|FAIL".into()),
            },
            status: MonitorStatus::Active,
            match_count: 3,
            last_event_preview: Some("FAIL: test_alpha".into()),
            last_triggered_at_ms: Some(1_760_000_010_000),
            expires_at_ms: Some(1_760_003_600_000),
            created_at_ms: 1_760_000_000_000,
        };
        store.upsert_monitor_watch(&watch).expect("upsert monitor");
        let loaded_watch = store
            .get_monitor_watch("mon_test01")
            .expect("get monitor")
            .expect("monitor exists");
        assert_eq!(loaded_watch, watch);
        assert_eq!(
            store
                .list_monitor_watches(Some("ag_1"))
                .expect("list monitors")
                .len(),
            1
        );
        assert!(
            store
                .delete_monitor_watch("mon_test01")
                .expect("delete monitor")
        );
        assert!(store.delete_goal_run("goal_test01").expect("delete goal"));
    }

    #[test]
    fn uncapped_active_queries_return_active_rows_behind_over_200_inactive_rows() {
        let store = OrbitStore::open_in_memory().expect("open in memory store");

        // Insert 1 older active goal & 1 older running monitor
        let active_goal = GoalRunRecord {
            goal_id: "goal_active_old".into(),
            agent_id: "ag_old".into(),
            workspace_id: "ws_1".into(),
            objective: "Keep running".into(),
            criteria: Vec::new(),
            verify_command: None,
            max_iterations: 5,
            current_iteration: 0,
            max_duration_secs: 600,
            status: GoalStatus::Running,
            last_verify_exit_code: None,
            last_verify_output: None,
            consecutive_identical_failures: 0,
            started_at_ms: 1000,
            updated_at_ms: 1000,
            finished_at_ms: None,
        };
        store
            .upsert_goal_run(&active_goal, None)
            .expect("insert active goal");

        let running_watch = MonitorWatchRecord {
            monitor_id: "mon_running_old".into(),
            agent_id: "ag_old".into(),
            name: "Active Watch".into(),
            kind: MonitorKind::LogFileTail {
                path: "app.log".into(),
                pattern: "ERR".into(),
            },
            status: MonitorStatus::Active,
            match_count: 0,
            last_event_preview: None,
            last_triggered_at_ms: None,
            expires_at_ms: None,
            created_at_ms: 1000,
        };
        store
            .upsert_monitor_watch(&running_watch)
            .expect("insert running watch");

        // Insert 210 newer terminal goals and stopped monitors
        for i in 0..210u64 {
            let ts = 10_000 + i;
            store
                .upsert_goal_run(
                    &GoalRunRecord {
                        goal_id: format!("goal_done_{i:03}"),
                        agent_id: "ag_hist".into(),
                        workspace_id: "ws_1".into(),
                        objective: "Done".into(),
                        criteria: Vec::new(),
                        verify_command: None,
                        max_iterations: 1,
                        current_iteration: 1,
                        max_duration_secs: 60,
                        status: GoalStatus::Succeeded,
                        last_verify_exit_code: Some(0),
                        last_verify_output: None,
                        consecutive_identical_failures: 0,
                        started_at_ms: ts,
                        updated_at_ms: ts,
                        finished_at_ms: Some(ts),
                    },
                    None,
                )
                .expect("insert historical goal");

            store
                .upsert_monitor_watch(&MonitorWatchRecord {
                    monitor_id: format!("mon_stop_{i:03}"),
                    agent_id: "ag_hist".into(),
                    name: "Stopped".into(),
                    kind: MonitorKind::LogFileTail {
                        path: "app.log".into(),
                        pattern: "ERR".into(),
                    },
                    status: MonitorStatus::Stopped,
                    match_count: 0,
                    last_event_preview: None,
                    last_triggered_at_ms: None,
                    expires_at_ms: None,
                    created_at_ms: ts,
                })
                .expect("insert historical monitor");
        }

        // Standard UI listing is capped at 200 and won't include the older active rows
        assert_eq!(store.list_goal_runs(None).expect("list goals").len(), 200);
        assert_eq!(
            store
                .list_monitor_watches(None)
                .expect("list monitors")
                .len(),
            200
        );

        // Uncapped active queries MUST find both active records!
        let active_goals = store.list_active_goal_runs().expect("list active goals");
        assert_eq!(active_goals.len(), 1);
        assert_eq!(active_goals[0].goal_id, "goal_active_old");

        let running_monitors = store
            .list_running_monitor_watches()
            .expect("list running monitors");
        assert_eq!(running_monitors.len(), 1);
        assert_eq!(running_monitors[0].monitor_id, "mon_running_old");
    }
}
