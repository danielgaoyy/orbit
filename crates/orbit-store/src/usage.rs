//! SQLite persistence for turn-level token/cost ledger (`usage_ledger`) and observed
//! provider quota snapshots (`quota_samples`).
//!
//! Creates tables and indexes idempotently via `CREATE TABLE IF NOT EXISTS` /
//! `CREATE INDEX IF NOT EXISTS` without bumping `PRAGMA user_version` (keeps `user_version = 6`).

use orbit_protocol::UsageLedgerEntry;
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{OrbitStore, StoreError};

/// Retention horizon for `usage_ledger` rows (90 days in milliseconds).
pub const USAGE_LEDGER_RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1_000;

/// Retention horizon for `quota_samples` rows (14 days in milliseconds).
pub const QUOTA_SAMPLES_RETENTION_MS: i64 = 14 * 24 * 60 * 60 * 1_000;

/// Observed provider quota window sample persisted over time for burn-rate forecasting.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaSampleRecord {
    pub provider: String,
    pub window_label: String,
    pub used_percent: f64,
    pub resets_at_ms: Option<i64>,
    pub sampled_at_ms: i64,
}

/// Persisted latest single-call provider context telemetry for an agent session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentContextTelemetry {
    pub prompt_tokens: u64,
    pub context_window_tokens: Option<u64>,
    pub recorded_at_ms: i64,
}

/// Idempotent schema bootstrap for `usage_ledger` and `quota_samples`.
///
/// Intentionally does NOT alter `PRAGMA user_version`.
pub(crate) fn migrate_usage_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS usage_ledger (
            id                 TEXT PRIMARY KEY,
            agent_id           TEXT NOT NULL,
            workspace_cwd      TEXT NOT NULL,
            provider           TEXT NOT NULL,
            model              TEXT NOT NULL,
            input_tokens       INTEGER NOT NULL DEFAULT 0,
            output_tokens      INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens  INTEGER NOT NULL DEFAULT 0,
            cache_write_tokens INTEGER NOT NULL DEFAULT 0,
            estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
            cache_savings_usd  REAL NOT NULL DEFAULT 0.0,
            recorded_at_ms     INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_usage_ledger_recorded_at
            ON usage_ledger(recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_usage_ledger_agent
            ON usage_ledger(agent_id, recorded_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_usage_ledger_provider_model
            ON usage_ledger(provider, model);

        CREATE TABLE IF NOT EXISTS quota_samples (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            provider      TEXT NOT NULL,
            window_label  TEXT NOT NULL,
            used_percent  REAL NOT NULL,
            resets_at_ms  INTEGER,
            sampled_at_ms INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_quota_samples_series
            ON quota_samples(provider, window_label, sampled_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_quota_samples_sampled_at
            ON quota_samples(sampled_at_ms DESC);

        CREATE TABLE IF NOT EXISTS agent_context_telemetry (
            agent_id              TEXT PRIMARY KEY,
            prompt_tokens         INTEGER NOT NULL,
            context_window_tokens INTEGER,
            recorded_at_ms        INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn map_usage_ledger_row(row: &Row<'_>) -> rusqlite::Result<UsageLedgerEntry> {
    let input_i64: i64 = row.get(5)?;
    let output_i64: i64 = row.get(6)?;
    let cache_read_i64: i64 = row.get(7)?;
    let cache_write_i64: i64 = row.get(8)?;
    Ok(UsageLedgerEntry {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        workspace_cwd: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        input_tokens: input_i64.max(0) as u64,
        output_tokens: output_i64.max(0) as u64,
        cache_read_tokens: cache_read_i64.max(0) as u64,
        cache_write_tokens: cache_write_i64.max(0) as u64,
        estimated_cost_usd: row.get(9)?,
        cache_savings_usd: row.get(10)?,
        recorded_at_ms: row.get(11)?,
    })
}

fn map_quota_sample_row(row: &Row<'_>) -> rusqlite::Result<QuotaSampleRecord> {
    Ok(QuotaSampleRecord {
        provider: row.get(0)?,
        window_label: row.get(1)?,
        used_percent: row.get(2)?,
        resets_at_ms: row.get(3)?,
        sampled_at_ms: row.get(4)?,
    })
}

fn sanitize_non_negative_f64(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

impl OrbitStore {
    /// Persist a turn-level token & cost ledger row and prune rows older than 90 days.
    pub fn claim_budget_alert(
        &self,
        period: &str,
        start: i64,
        end: i64,
        budget: f64,
    ) -> Result<bool, StoreError> {
        if !budget.is_finite() || budget <= 0. {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.execute("INSERT OR IGNORE INTO spend_budget_alerts(period) SELECT ?1 WHERE (SELECT COALESCE(SUM(estimated_cost_usd),0) FROM usage_ledger WHERE recorded_at_ms>=?2 AND recorded_at_ms<?3)>=?4",params![period,start,end,budget])?>0)
    }

    pub fn record_usage_entry(&self, entry: &UsageLedgerEntry) -> Result<(), StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let estimated_cost_usd = sanitize_non_negative_f64(entry.estimated_cost_usd);
        let cache_savings_usd = sanitize_non_negative_f64(entry.cache_savings_usd);
        tx.execute(
            "INSERT OR REPLACE INTO usage_ledger (
                id, agent_id, workspace_cwd, provider, model,
                input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                estimated_cost_usd, cache_savings_usd, recorded_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                entry.id,
                entry.agent_id,
                entry.workspace_cwd,
                entry.provider,
                entry.model,
                entry.input_tokens.min(i64::MAX as u64) as i64,
                entry.output_tokens.min(i64::MAX as u64) as i64,
                entry.cache_read_tokens.min(i64::MAX as u64) as i64,
                entry.cache_write_tokens.min(i64::MAX as u64) as i64,
                estimated_cost_usd,
                cache_savings_usd,
                entry.recorded_at_ms,
            ],
        )?;
        let prune_before = entry
            .recorded_at_ms
            .saturating_sub(USAGE_LEDGER_RETENTION_MS);
        tx.execute(
            "DELETE FROM usage_ledger WHERE recorded_at_ms < ?1",
            params![prune_before],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Return all `usage_ledger` entries with `recorded_at_ms >= since_ms` ordered chronologically.
    pub fn list_usage_entries_since(
        &self,
        since_ms: i64,
    ) -> Result<Vec<UsageLedgerEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, workspace_cwd, provider, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    estimated_cost_usd, cache_savings_usd, recorded_at_ms
             FROM usage_ledger
             WHERE recorded_at_ms >= ?1
             ORDER BY recorded_at_ms ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![since_ms], map_usage_ledger_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Return the most recent `usage_ledger` entry for `agent_id`, if any.
    pub fn session_usage(
        &self,
        agent_id: &str,
    ) -> Result<orbit_protocol::SessionUsage, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("SELECT COUNT(*),COALESCE(SUM(input_tokens),0),COALESCE(SUM(output_tokens),0),COALESCE(SUM(cache_read_tokens),0),COALESCE(SUM(cache_write_tokens),0),TOTAL(estimated_cost_usd),TOTAL(cache_savings_usd) FROM usage_ledger WHERE agent_id=?1",[agent_id],|r| Ok(orbit_protocol::SessionUsage{agent_id:agent_id.into(),turns:r.get(0)?,input_tokens:r.get(1)?,output_tokens:r.get(2)?,cache_read_tokens:r.get(3)?,cache_write_tokens:r.get(4)?,estimated_cost_usd:r.get(5)?,cache_savings_usd:r.get(6)?}))?)
    }

    pub fn latest_usage_entry_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Option<UsageLedgerEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT id, agent_id, workspace_cwd, provider, model,
                    input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                    estimated_cost_usd, cache_savings_usd, recorded_at_ms
             FROM usage_ledger
             WHERE agent_id = ?1
             ORDER BY recorded_at_ms DESC, id DESC
             LIMIT 1",
            params![agent_id],
            map_usage_ledger_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Persist a batch of observed provider quota samples and prune samples older than 14 days.
    pub fn record_quota_samples(&self, samples: &[QuotaSampleRecord]) -> Result<(), StoreError> {
        if samples.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let mut max_ts = 0i64;
        for sample in samples {
            if !sample.used_percent.is_finite() {
                continue;
            }
            max_ts = max_ts.max(sample.sampled_at_ms);
            tx.execute(
                "INSERT INTO quota_samples (
                    provider, window_label, used_percent, resets_at_ms, sampled_at_ms
                ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    sample.provider,
                    sample.window_label,
                    sample.used_percent.clamp(0.0, 100.0),
                    sample.resets_at_ms,
                    sample.sampled_at_ms,
                ],
            )?;
        }
        if max_ts > 0 {
            let prune_before = max_ts.saturating_sub(QUOTA_SAMPLES_RETENTION_MS);
            tx.execute(
                "DELETE FROM quota_samples WHERE sampled_at_ms < ?1",
                params![prune_before],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Return all quota samples with `sampled_at_ms >= since_ms`, ordered chronologically.
    pub fn list_recent_quota_samples(
        &self,
        since_ms: i64,
    ) -> Result<Vec<QuotaSampleRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT provider, window_label, used_percent, resets_at_ms, sampled_at_ms
             FROM quota_samples
             WHERE sampled_at_ms >= ?1
             ORDER BY sampled_at_ms ASC, id ASC",
        )?;
        let rows = stmt.query_map(params![since_ms], map_quota_sample_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Persist latest single-call provider context telemetry for `agent_id` so context occupancy
    /// survives daemon restarts without regressing to character estimates.
    pub fn upsert_agent_context_telemetry(
        &self,
        agent_id: &str,
        telemetry: AgentContextTelemetry,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let window_i64 = telemetry
            .context_window_tokens
            .filter(|&w| w > 0)
            .map(|w| w.min(i64::MAX as u64) as i64);
        conn.execute(
            "INSERT INTO agent_context_telemetry (
                agent_id, prompt_tokens, context_window_tokens, recorded_at_ms
             ) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id) DO UPDATE SET
                prompt_tokens = excluded.prompt_tokens,
                context_window_tokens = COALESCE(excluded.context_window_tokens, agent_context_telemetry.context_window_tokens),
                recorded_at_ms = excluded.recorded_at_ms",
            params![
                agent_id,
                telemetry.prompt_tokens.min(i64::MAX as u64) as i64,
                window_i64,
                telemetry.recorded_at_ms,
            ],
        )?;
        Ok(())
    }

    /// Retrieve persisted latest single-call provider context telemetry for `agent_id`.
    pub fn get_agent_context_telemetry(
        &self,
        agent_id: &str,
    ) -> Result<Option<AgentContextTelemetry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT prompt_tokens, context_window_tokens, recorded_at_ms
             FROM agent_context_telemetry
             WHERE agent_id = ?1",
            params![agent_id],
            |row| {
                let prompt_i64: i64 = row.get(0)?;
                let window_opt: Option<i64> = row.get(1)?;
                let recorded_at_ms: i64 = row.get(2)?;
                Ok(AgentContextTelemetry {
                    prompt_tokens: prompt_i64.max(0) as u64,
                    context_window_tokens: window_opt.filter(|&w| w > 0).map(|w| w as u64),
                    recorded_at_ms,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_migration_preserves_user_version_6() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let conn = store.conn.lock().expect("lock");
        let user_ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(user_ver, 6);
    }

    #[test]
    fn usage_ledger_persistence_and_90_day_retention() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now_ms: i64 = 1_760_000_000_000;
        let stale_ms = now_ms - USAGE_LEDGER_RETENTION_MS - 10_000;

        let old_entry = UsageLedgerEntry {
            id: "u_old".into(),
            agent_id: "ag_1".into(),
            workspace_cwd: "/repo/a".into(),
            provider: "claude_code".into(),
            model: "claude-sonnet-4-5".into(),
            input_tokens: 1_000,
            output_tokens: 500,
            cache_read_tokens: 2_000,
            cache_write_tokens: 100,
            estimated_cost_usd: 0.0114,
            cache_savings_usd: 0.0054,
            recorded_at_ms: stale_ms,
        };
        store
            .record_usage_entry(&old_entry)
            .expect("record old entry");

        let fresh_entry = UsageLedgerEntry {
            id: "u_fresh".into(),
            agent_id: "ag_1".into(),
            workspace_cwd: "/repo/a".into(),
            provider: "claude_code".into(),
            model: "claude-opus-4-6".into(),
            input_tokens: 10_000,
            output_tokens: 2_500,
            cache_read_tokens: 40_000,
            cache_write_tokens: 1_000,
            estimated_cost_usd: 0.41625,
            cache_savings_usd: 0.54,
            recorded_at_ms: now_ms,
        };
        store
            .record_usage_entry(&fresh_entry)
            .expect("record fresh entry");

        let rows = store
            .list_usage_entries_since(0)
            .expect("list usage entries");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "u_fresh");
        assert_eq!(rows[0].input_tokens, 10_000);
        assert_eq!(rows[0].cache_read_tokens, 40_000);

        let latest = store
            .latest_usage_entry_for_agent("ag_1")
            .expect("latest")
            .expect("present");
        assert_eq!(latest.id, "u_fresh");
    }

    #[test]
    fn quota_samples_persistence_and_pruning() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now_ms: i64 = 1_760_000_000_000;
        let stale_ms = now_ms - QUOTA_SAMPLES_RETENTION_MS - 5_000;

        store
            .record_quota_samples(&[
                QuotaSampleRecord {
                    provider: "claude_code".into(),
                    window_label: "5h window".into(),
                    used_percent: 10.0,
                    resets_at_ms: Some(stale_ms + 3_600_000),
                    sampled_at_ms: stale_ms,
                },
                QuotaSampleRecord {
                    provider: "claude_code".into(),
                    window_label: "5h window".into(),
                    used_percent: 32.5,
                    resets_at_ms: Some(now_ms + 3 * 3_600_000),
                    sampled_at_ms: now_ms - 1_800_000,
                },
                QuotaSampleRecord {
                    provider: "claude_code".into(),
                    window_label: "5h window".into(),
                    used_percent: 45.0,
                    resets_at_ms: Some(now_ms + 3 * 3_600_000),
                    sampled_at_ms: now_ms,
                },
            ])
            .expect("record quota samples");

        let samples = store
            .list_recent_quota_samples(0)
            .expect("list quota samples");
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].used_percent, 32.5);
        assert_eq!(samples[1].used_percent, 45.0);
    }

    #[test]
    fn agent_context_telemetry_persists_prompt_and_window_across_reads() {
        let store = OrbitStore::open_in_memory().expect("open store");
        assert_eq!(
            store
                .get_agent_context_telemetry("ag_missing")
                .expect("query missing"),
            None
        );

        let rec = AgentContextTelemetry {
            prompt_tokens: 48_500,
            context_window_tokens: Some(1_000_000),
            recorded_at_ms: 1_760_000_123_000,
        };
        store
            .upsert_agent_context_telemetry("ag_ctx", rec)
            .expect("upsert telemetry");
        assert_eq!(
            store
                .get_agent_context_telemetry("ag_ctx")
                .expect("read telemetry"),
            Some(rec)
        );
    }
}
