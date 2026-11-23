//! SQLite persistence for Batch CSV Fan-Out, Programmable DAG Workflows, and Best-of-N Arena.
//!
//! Creates `workflow_runs` and `workflow_steps` idempotently via `CREATE TABLE IF NOT EXISTS`
//! without altering `PRAGMA user_version` (keeps `user_version = 6`).

use orbit_protocol::{
    ProviderKind, WorkflowKind, WorkflowRunRecord, WorkflowRunStatus, WorkflowStepRecord,
    WorkflowStepStatus,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{OrbitStore, StoreError, ensure_table_column};

/// Idempotent schema bootstrap for `workflow_runs` and `workflow_steps`.
///
/// Intentionally does NOT alter `PRAGMA user_version` (stays at `6`).
pub(crate) fn migrate_workflows_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS workflow_runs (
            run_id          TEXT PRIMARY KEY,
            name            TEXT NOT NULL,
            kind            TEXT NOT NULL,
            workspace_cwd   TEXT NOT NULL,
            owner_agent_id  TEXT,
            status          TEXT NOT NULL,
            max_concurrency INTEGER NOT NULL DEFAULT 3,
            verify_command  TEXT,
            winner_step_id  TEXT,
            created_at_ms   INTEGER NOT NULL,
            updated_at_ms   INTEGER NOT NULL,
            finished_at_ms  INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_workflow_runs_updated
            ON workflow_runs(updated_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_workflow_runs_status_updated
            ON workflow_runs(status, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS workflow_steps (
            run_id          TEXT NOT NULL REFERENCES workflow_runs(run_id) ON DELETE CASCADE,
            step_id         TEXT NOT NULL,
            step_order      INTEGER NOT NULL DEFAULT 0,
            name            TEXT NOT NULL,
            status          TEXT NOT NULL,
            depends_on_json TEXT NOT NULL DEFAULT '[]',
            agent_id        TEXT,
            worktree_cwd    TEXT,
            provider_json   TEXT NOT NULL,
            model           TEXT,
            output_preview  TEXT,
            diff_additions  INTEGER NOT NULL DEFAULT 0,
            diff_deletions  INTEGER NOT NULL DEFAULT 0,
            diff_files      INTEGER NOT NULL DEFAULT 0,
            verify_passed   INTEGER,
            verify_output   TEXT,
            error           TEXT,
            is_winner       INTEGER NOT NULL DEFAULT 0,
            started_at_ms   INTEGER,
            finished_at_ms  INTEGER,
            PRIMARY KEY (run_id, step_id)
        );

        CREATE INDEX IF NOT EXISTS idx_workflow_steps_run_order
            ON workflow_steps(run_id, step_order ASC);

        CREATE INDEX IF NOT EXISTS idx_workflow_steps_agent_id
            ON workflow_steps(agent_id);
        ",
    )?;
    // Forward-compatible column addition for DBs created before `owner_agent_id` existed.
    ensure_table_column(conn, "workflow_runs", "owner_agent_id", "TEXT")?;
    Ok(())
}

const WORKFLOW_RUN_SELECT_COLS: &str = "
    run_id, name, kind, workspace_cwd, owner_agent_id, status, max_concurrency,
    verify_command, winner_step_id, created_at_ms, updated_at_ms, finished_at_ms
";

const WORKFLOW_STEP_SELECT_COLS: &str = "
    run_id, step_id, name, status, depends_on_json, agent_id, worktree_cwd,
    provider_json, model, output_preview, diff_additions, diff_deletions,
    diff_files, verify_passed, verify_output, error, is_winner,
    started_at_ms, finished_at_ms
";

fn map_workflow_run_row(row: &Row<'_>) -> rusqlite::Result<WorkflowRunRecord> {
    let kind_raw: String = row.get(2)?;
    let kind = WorkflowKind::from_str_opt(&kind_raw).unwrap_or(WorkflowKind::Dag);
    let owner_agent_id: Option<String> = row.get(4)?;
    let status_raw: String = row.get(5)?;
    let status = WorkflowRunStatus::from_str_opt(&status_raw).unwrap_or(WorkflowRunStatus::Failed);
    let max_conc_i64: i64 = row.get(6)?;
    let created_at_ms: i64 = row.get(9)?;
    let updated_at_ms: i64 = row.get(10)?;
    let finished_at_ms: Option<i64> = row.get(11)?;

    Ok(WorkflowRunRecord {
        run_id: row.get(0)?,
        name: row.get(1)?,
        kind,
        workspace_cwd: row.get(3)?,
        owner_agent_id,
        status,
        max_concurrency: max_conc_i64.clamp(1, i64::from(u32::MAX)) as u32,
        verify_command: row.get(7)?,
        winner_step_id: row.get(8)?,
        steps: Vec::new(),
        created_at_ms: created_at_ms.max(0) as u64,
        updated_at_ms: updated_at_ms.max(0) as u64,
        finished_at_ms: finished_at_ms.map(|v| v.max(0) as u64),
    })
}

fn map_workflow_step_row(row: &Row<'_>) -> rusqlite::Result<WorkflowStepRecord> {
    let status_raw: String = row.get(3)?;
    let status =
        WorkflowStepStatus::from_str_opt(&status_raw).unwrap_or(WorkflowStepStatus::Failed);
    let depends_json: String = row.get(4)?;
    let depends_on: Vec<String> = serde_json::from_str(&depends_json).unwrap_or_default();
    let provider_json: String = row.get(7)?;
    let provider: ProviderKind = serde_json::from_str(&provider_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let diff_additions_i64: i64 = row.get(10)?;
    let diff_deletions_i64: i64 = row.get(11)?;
    let diff_files_i64: i64 = row.get(12)?;
    let verify_passed_opt: Option<i64> = row.get(13)?;
    let is_winner_i64: i64 = row.get(16)?;
    let started_at_ms: Option<i64> = row.get(17)?;
    let finished_at_ms: Option<i64> = row.get(18)?;

    Ok(WorkflowStepRecord {
        run_id: row.get(0)?,
        step_id: row.get(1)?,
        name: row.get(2)?,
        status,
        depends_on,
        agent_id: row.get(5)?,
        worktree_cwd: row.get(6)?,
        provider,
        model: row.get(8)?,
        output_preview: row.get(9)?,
        diff_additions: diff_additions_i64.clamp(0, i64::from(u32::MAX)) as u32,
        diff_deletions: diff_deletions_i64.clamp(0, i64::from(u32::MAX)) as u32,
        diff_files: diff_files_i64.clamp(0, i64::from(u32::MAX)) as u32,
        verify_passed: verify_passed_opt.map(|v| v != 0),
        verify_output: row.get(14)?,
        error: row.get(15)?,
        is_winner: is_winner_i64 != 0,
        started_at_ms: started_at_ms.map(|v| v.max(0) as u64),
        finished_at_ms: finished_at_ms.map(|v| v.max(0) as u64),
    })
}

fn load_steps_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<WorkflowStepRecord>, StoreError> {
    let sql = format!(
        "SELECT {WORKFLOW_STEP_SELECT_COLS}
         FROM workflow_steps
         WHERE run_id = ?1
         ORDER BY step_order ASC, step_id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![run_id], map_workflow_step_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

impl OrbitStore {
    /// Atomically insert or update a workflow run and all of its steps.
    pub fn upsert_workflow_run(&self, run: &WorkflowRunRecord) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO workflow_runs (
                run_id, name, kind, workspace_cwd, owner_agent_id, status, max_concurrency,
                verify_command, winner_step_id, created_at_ms, updated_at_ms, finished_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(run_id) DO UPDATE SET
                name = excluded.name,
                kind = excluded.kind,
                workspace_cwd = excluded.workspace_cwd,
                owner_agent_id = excluded.owner_agent_id,
                status = excluded.status,
                max_concurrency = excluded.max_concurrency,
                verify_command = excluded.verify_command,
                winner_step_id = excluded.winner_step_id,
                created_at_ms = excluded.created_at_ms,
                updated_at_ms = excluded.updated_at_ms,
                finished_at_ms = excluded.finished_at_ms",
            params![
                run.run_id,
                run.name,
                run.kind.as_str(),
                run.workspace_cwd,
                run.owner_agent_id,
                run.status.as_str(),
                i64::from(run.max_concurrency),
                run.verify_command,
                run.winner_step_id,
                run.created_at_ms as i64,
                run.updated_at_ms as i64,
                run.finished_at_ms.map(|v| v as i64),
            ],
        )?;

        for (idx, step) in run.steps.iter().enumerate() {
            let depends_json = serde_json::to_string(&step.depends_on)?;
            let provider_json = serde_json::to_string(&step.provider)?;
            tx.execute(
                "INSERT INTO workflow_steps (
                    run_id, step_id, step_order, name, status, depends_on_json, agent_id,
                    worktree_cwd, provider_json, model, output_preview, diff_additions,
                    diff_deletions, diff_files, verify_passed, verify_output, error,
                    is_winner, started_at_ms, finished_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
                ON CONFLICT(run_id, step_id) DO UPDATE SET
                    step_order = excluded.step_order,
                    name = excluded.name,
                    status = excluded.status,
                    depends_on_json = excluded.depends_on_json,
                    agent_id = excluded.agent_id,
                    worktree_cwd = excluded.worktree_cwd,
                    provider_json = excluded.provider_json,
                    model = excluded.model,
                    output_preview = excluded.output_preview,
                    diff_additions = excluded.diff_additions,
                    diff_deletions = excluded.diff_deletions,
                    diff_files = excluded.diff_files,
                    verify_passed = excluded.verify_passed,
                    verify_output = excluded.verify_output,
                    error = excluded.error,
                    is_winner = excluded.is_winner,
                    started_at_ms = excluded.started_at_ms,
                    finished_at_ms = excluded.finished_at_ms",
                params![
                    run.run_id,
                    step.step_id,
                    idx as i64,
                    step.name,
                    step.status.as_str(),
                    depends_json,
                    step.agent_id,
                    step.worktree_cwd,
                    provider_json,
                    step.model,
                    step.output_preview,
                    i64::from(step.diff_additions),
                    i64::from(step.diff_deletions),
                    i64::from(step.diff_files),
                    step.verify_passed.map(i64::from),
                    step.verify_output,
                    step.error,
                    i64::from(step.is_winner),
                    step.started_at_ms.map(|v| v as i64),
                    step.finished_at_ms.map(|v| v as i64),
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Fetch a single workflow run with all of its ordered steps.
    pub fn get_workflow_run(&self, run_id: &str) -> Result<Option<WorkflowRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {WORKFLOW_RUN_SELECT_COLS} FROM workflow_runs WHERE run_id = ?1");
        let Some(mut run) = conn
            .query_row(&sql, params![run_id], map_workflow_run_row)
            .optional()?
        else {
            return Ok(None);
        };
        run.steps = load_steps_for_run(&conn, &run.run_id)?;
        Ok(Some(run))
    }

    /// Returns `true` if `agent_id` was spawned as a step worker of any workflow run.
    pub fn is_workflow_step_agent(&self, agent_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM workflow_steps WHERE agent_id = ?1 LIMIT 1",
                params![agent_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// List workflow runs (most recently updated first), capped at 100 runs.
    pub fn list_workflow_runs(
        &self,
        include_completed: bool,
    ) -> Result<Vec<WorkflowRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = if include_completed {
            format!(
                "SELECT {WORKFLOW_RUN_SELECT_COLS}
                 FROM workflow_runs
                 ORDER BY updated_at_ms DESC, run_id DESC
                 LIMIT 100"
            )
        } else {
            format!(
                "SELECT {WORKFLOW_RUN_SELECT_COLS}
                 FROM workflow_runs
                 WHERE status IN ('pending', 'running')
                 ORDER BY updated_at_ms DESC, run_id DESC
                 LIMIT 100"
            )
        };
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_workflow_run_row)?;
        let mut runs = Vec::new();
        for row in rows {
            let mut run = row?;
            run.steps = load_steps_for_run(&conn, &run.run_id)?;
            runs.push(run);
        }
        Ok(runs)
    }

    /// List ALL non-terminal (`pending`, `running`) workflow runs with no row-count cap,
    /// used on daemon startup recovery so interrupted runs are cleanly reconciled.
    pub fn list_active_workflow_runs(&self) -> Result<Vec<WorkflowRunRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {WORKFLOW_RUN_SELECT_COLS}
             FROM workflow_runs
             WHERE status IN ('pending', 'running')
             ORDER BY updated_at_ms DESC, run_id DESC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_workflow_run_row)?;
        let mut runs = Vec::new();
        for row in rows {
            let mut run = row?;
            run.steps = load_steps_for_run(&conn, &run.run_id)?;
            runs.push(run);
        }
        Ok(runs)
    }

    /// Delete a workflow run (cascades to `workflow_steps`).
    pub fn delete_workflow_run(&self, run_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM workflow_runs WHERE run_id = ?1",
            params![run_id],
        )?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_tables_preserve_user_version_6_and_roundtrip() {
        let store = OrbitStore::open_in_memory().expect("open in memory store");
        let user_ver: i64 = {
            let conn = store.conn.lock().expect("lock conn");
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("read user_version")
        };
        assert_eq!(user_ver, 6, "PRAGMA user_version must remain 6");

        let run = WorkflowRunRecord {
            run_id: "wf_test01".into(),
            name: "Best-of-2 Refactor".into(),
            kind: WorkflowKind::BestOfN,
            workspace_cwd: "/tmp/repo".into(),
            owner_agent_id: Some("ag_parent".into()),
            status: WorkflowRunStatus::Running,
            max_concurrency: 2,
            verify_command: Some("cargo test".into()),
            winner_step_id: None,
            steps: vec![
                WorkflowStepRecord {
                    run_id: "wf_test01".into(),
                    step_id: "cand_1".into(),
                    name: "Claude Opus".into(),
                    status: WorkflowStepStatus::Succeeded,
                    depends_on: Vec::new(),
                    agent_id: Some("ag_c1".into()),
                    worktree_cwd: Some("/tmp/wt-1".into()),
                    provider: ProviderKind::ClaudeCode,
                    model: Some("claude-opus-4-6".into()),
                    output_preview: Some("Refactored cleanly".into()),
                    diff_additions: 42,
                    diff_deletions: 18,
                    diff_files: 3,
                    verify_passed: Some(true),
                    verify_output: Some("ok. 12 passed".into()),
                    error: None,
                    is_winner: false,
                    started_at_ms: Some(1_760_000_001_000),
                    finished_at_ms: Some(1_760_000_010_000),
                },
                WorkflowStepRecord {
                    run_id: "wf_test01".into(),
                    step_id: "cand_2".into(),
                    name: "Codex".into(),
                    status: WorkflowStepStatus::Running,
                    depends_on: Vec::new(),
                    agent_id: Some("ag_c2".into()),
                    worktree_cwd: Some("/tmp/wt-2".into()),
                    provider: ProviderKind::Codex,
                    model: Some("gpt-5.4".into()),
                    output_preview: None,
                    diff_additions: 0,
                    diff_deletions: 0,
                    diff_files: 0,
                    verify_passed: None,
                    verify_output: None,
                    error: None,
                    is_winner: false,
                    started_at_ms: Some(1_760_000_002_000),
                    finished_at_ms: None,
                },
            ],
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_010_000,
            finished_at_ms: None,
        };

        store.upsert_workflow_run(&run).expect("upsert run");
        let loaded = store
            .get_workflow_run("wf_test01")
            .expect("get run")
            .expect("run exists");
        assert_eq!(loaded, run);
        assert!(
            store
                .is_workflow_step_agent("ag_c1")
                .expect("step agent check")
        );
        assert!(
            !store
                .is_workflow_step_agent("ag_unrelated")
                .expect("non-step agent check")
        );

        let active = store.list_workflow_runs(false).expect("list active");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].steps.len(), 2);

        // Complete the run and mark winner
        let mut completed = run.clone();
        completed.status = WorkflowRunStatus::Succeeded;
        completed.winner_step_id = Some("cand_1".into());
        completed.steps[0].is_winner = true;
        completed.steps[1].status = WorkflowStepStatus::Succeeded;
        completed.finished_at_ms = Some(1_760_000_020_000);
        completed.updated_at_ms = 1_760_000_020_000;
        store
            .upsert_workflow_run(&completed)
            .expect("upsert completed");

        assert!(
            store
                .list_workflow_runs(false)
                .expect("list active after finish")
                .is_empty()
        );
        let all = store.list_workflow_runs(true).expect("list all");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].winner_step_id.as_deref(), Some("cand_1"));
        assert!(all[0].steps[0].is_winner);

        assert!(
            store
                .delete_workflow_run("wf_test01")
                .expect("delete workflow run")
        );
        assert!(
            store
                .get_workflow_run("wf_test01")
                .expect("get deleted")
                .is_none()
        );
    }
}
