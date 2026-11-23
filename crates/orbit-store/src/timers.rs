//! SQLite persistence for Early-Wakeup One-Shot Conditional Timers.
//!
//! Creates `conditional_timers` idempotently via `CREATE TABLE IF NOT EXISTS`
//! without bumping `PRAGMA user_version` (keeps `user_version = 6`).

use orbit_protocol::{
    ConditionalTimerRecord, ConditionalTimerState, MAX_ARMED_TIMERS_GLOBAL,
    MAX_ARMED_TIMERS_PER_AGENT, TimerCondition,
};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::Type};

use crate::{OrbitStore, StoreError};

/// Maximum number of resolved conditional timers returned when `include_resolved = true`.
const MAX_RESOLVED_TIMERS_LISTED: i64 = 100;

/// Idempotent schema bootstrap for `conditional_timers`.
///
/// Intentionally does NOT alter `PRAGMA user_version`.
pub(crate) fn migrate_conditional_timers_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS conditional_timers (
            timer_id         TEXT PRIMARY KEY,
            agent_id         TEXT NOT NULL,
            workspace_id     TEXT NOT NULL,
            prompt           TEXT NOT NULL,
            duration_seconds INTEGER NOT NULL,
            condition_json   TEXT NOT NULL,
            state            TEXT NOT NULL,
            created_at_ms    INTEGER NOT NULL,
            fire_at_ms       INTEGER NOT NULL,
            resolved_at_ms   INTEGER,
            resolved_reason  TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_conditional_timers_agent_state
            ON conditional_timers(agent_id, state, fire_at_ms);

        CREATE INDEX IF NOT EXISTS idx_conditional_timers_state_fire
            ON conditional_timers(state, fire_at_ms);
        ",
    )?;
    Ok(())
}

fn map_conditional_timer_row(row: &Row<'_>) -> rusqlite::Result<ConditionalTimerRecord> {
    let duration_i64: i64 = row.get(4)?;
    let condition_json: String = row.get(5)?;
    let state_raw: String = row.get(6)?;
    let created_i64: i64 = row.get(7)?;
    let fire_i64: i64 = row.get(8)?;
    let resolved_i64: Option<i64> = row.get(9)?;
    let condition: TimerCondition = serde_json::from_str(&condition_json)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(err)))?;
    let state = ConditionalTimerState::from_slug(&state_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            6,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown conditional timer state `{state_raw}`"),
            )),
        )
    })?;

    Ok(ConditionalTimerRecord {
        timer_id: row.get(0)?,
        agent_id: row.get(1)?,
        workspace_id: row.get(2)?,
        prompt: row.get(3)?,
        duration_seconds: duration_i64.max(0) as u64,
        condition,
        state,
        created_at_ms: created_i64.max(0) as u64,
        fire_at_ms: fire_i64.max(0) as u64,
        resolved_at_ms: resolved_i64.map(|v| v.max(0) as u64),
        resolved_reason: row.get(10)?,
    })
}

const TIMER_SELECT_COLS: &str = "timer_id, agent_id, workspace_id, prompt, duration_seconds, condition_json, state, created_at_ms, fire_at_ms, resolved_at_ms, resolved_reason";

impl OrbitStore {
    /// Atomically validate armed-timer caps and persist a new conditional timer record inside
    /// an `IMMEDIATE` transaction.
    pub fn insert_conditional_timer(
        &self,
        timer: &ConditionalTimerRecord,
    ) -> Result<(), StoreError> {
        let condition_json = serde_json::to_string(&timer.condition)?;
        let duration_i64 = i64::try_from(timer.duration_seconds)
            .map_err(|_| StoreError::InvalidArgument("duration_seconds exceeds i64".into()))?;
        let created_i64 = i64::try_from(timer.created_at_ms)
            .map_err(|_| StoreError::InvalidArgument("created_at_ms exceeds i64".into()))?;
        let fire_i64 = i64::try_from(timer.fire_at_ms)
            .map_err(|_| StoreError::InvalidArgument("fire_at_ms exceeds i64".into()))?;
        let resolved_i64 = timer
            .resolved_at_ms
            .map(i64::try_from)
            .transpose()
            .map_err(|_| StoreError::InvalidArgument("resolved_at_ms exceeds i64".into()))?;

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if timer.state == ConditionalTimerState::Armed {
            let agent_armed: i64 = tx.query_row(
                "SELECT COUNT(*) FROM conditional_timers WHERE agent_id = ?1 AND state = 'armed'",
                params![timer.agent_id],
                |r| r.get(0),
            )?;
            if agent_armed >= MAX_ARMED_TIMERS_PER_AGENT as i64 {
                return Err(StoreError::InvalidArgument(format!(
                    "agent `{}` already has {MAX_ARMED_TIMERS_PER_AGENT} armed conditional timers (maximum per agent)",
                    timer.agent_id
                )));
            }
            let global_armed: i64 = tx.query_row(
                "SELECT COUNT(*) FROM conditional_timers WHERE state = 'armed'",
                [],
                |r| r.get(0),
            )?;
            if global_armed >= MAX_ARMED_TIMERS_GLOBAL as i64 {
                return Err(StoreError::InvalidArgument(format!(
                    "host already has {MAX_ARMED_TIMERS_GLOBAL} armed conditional timers (global maximum)"
                )));
            }
        }

        tx.execute(
            "INSERT INTO conditional_timers (
                timer_id, agent_id, workspace_id, prompt, duration_seconds,
                condition_json, state, created_at_ms, fire_at_ms, resolved_at_ms, resolved_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                timer.timer_id,
                timer.agent_id,
                timer.workspace_id,
                timer.prompt,
                duration_i64,
                condition_json,
                timer.state.slug(),
                created_i64,
                fire_i64,
                resolved_i64,
                timer.resolved_reason,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Fetch a single conditional timer by `timer_id`.
    pub fn get_conditional_timer(
        &self,
        timer_id: &str,
    ) -> Result<Option<ConditionalTimerRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {TIMER_SELECT_COLS} FROM conditional_timers WHERE timer_id = ?1");
        conn.query_row(&sql, params![timer_id], map_conditional_timer_row)
            .optional()
            .map_err(Into::into)
    }

    /// List conditional timers (optionally filtered by `agent_id`).
    ///
    /// Armed timers are returned first (ordered by `fire_at_ms ASC`), followed by recent
    /// resolved timers (ordered by `resolved_at_ms DESC`, capped to 100 rows) when
    /// `include_resolved` is `true`.
    pub fn list_conditional_timers(
        &self,
        agent_id: Option<&str>,
        include_resolved: bool,
    ) -> Result<Vec<ConditionalTimerRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut results = Vec::new();

        // 1. Armed timers ordered by soonest fire time
        if let Some(aid) = agent_id {
            let sql = format!(
                "SELECT {TIMER_SELECT_COLS} FROM conditional_timers
                 WHERE agent_id = ?1 AND state = 'armed'
                 ORDER BY fire_at_ms ASC, created_at_ms ASC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![aid], map_conditional_timer_row)?;
            for row in rows {
                results.push(row?);
            }
        } else {
            let sql = format!(
                "SELECT {TIMER_SELECT_COLS} FROM conditional_timers
                 WHERE state = 'armed'
                 ORDER BY fire_at_ms ASC, created_at_ms ASC"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], map_conditional_timer_row)?;
            for row in rows {
                results.push(row?);
            }
        }

        if include_resolved {
            if let Some(aid) = agent_id {
                let sql = format!(
                    "SELECT {TIMER_SELECT_COLS} FROM conditional_timers
                     WHERE agent_id = ?1 AND state != 'armed'
                     ORDER BY COALESCE(resolved_at_ms, created_at_ms) DESC, created_at_ms DESC
                     LIMIT ?2"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(
                    params![aid, MAX_RESOLVED_TIMERS_LISTED],
                    map_conditional_timer_row,
                )?;
                for row in rows {
                    results.push(row?);
                }
            } else {
                let sql = format!(
                    "SELECT {TIMER_SELECT_COLS} FROM conditional_timers
                     WHERE state != 'armed'
                     ORDER BY COALESCE(resolved_at_ms, created_at_ms) DESC, created_at_ms DESC
                     LIMIT ?1"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(
                    params![MAX_RESOLVED_TIMERS_LISTED],
                    map_conditional_timer_row,
                )?;
                for row in rows {
                    results.push(row?);
                }
            }
        }

        Ok(results)
    }

    /// List all currently armed conditional timers (optionally filtered by `agent_id`).
    pub fn list_armed_conditional_timers(
        &self,
        agent_id: Option<&str>,
    ) -> Result<Vec<ConditionalTimerRecord>, StoreError> {
        self.list_conditional_timers(agent_id, false)
    }

    /// Atomically transition a timer from `expected_state` to `next_state` inside an
    /// `IMMEDIATE` transaction. Returns `Ok(Some(updated))` iff the transition won the race,
    /// or `Ok(None)` if the timer does not exist or was already in a different state.
    pub fn transition_conditional_timer(
        &self,
        timer_id: &str,
        expected_state: ConditionalTimerState,
        next_state: ConditionalTimerState,
        resolved_at_ms: u64,
        resolved_reason: Option<&str>,
    ) -> Result<Option<ConditionalTimerRecord>, StoreError> {
        let resolved_i64 = i64::try_from(resolved_at_ms)
            .map_err(|_| StoreError::InvalidArgument("resolved_at_ms exceeds i64".into()))?;
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let affected = tx.execute(
            "UPDATE conditional_timers
             SET state = ?1,
                 resolved_at_ms = ?2,
                 resolved_reason = ?3
             WHERE timer_id = ?4 AND state = ?5",
            params![
                next_state.slug(),
                resolved_i64,
                resolved_reason,
                timer_id,
                expected_state.slug(),
            ],
        )?;
        let updated = if affected == 1 {
            let sql =
                format!("SELECT {TIMER_SELECT_COLS} FROM conditional_timers WHERE timer_id = ?1");
            tx.query_row(&sql, params![timer_id], map_conditional_timer_row)
                .optional()?
        } else {
            None
        };
        tx.commit()?;
        Ok(updated)
    }

    /// Delete all conditional timer records owned by `agent_id`.
    pub fn delete_conditional_timers_for_agent(&self, agent_id: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM conditional_timers WHERE agent_id = ?1",
            params![agent_id],
        )?;
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_timer(
        id: &str,
        agent_id: &str,
        condition: TimerCondition,
        fire_at_ms: u64,
    ) -> ConditionalTimerRecord {
        ConditionalTimerRecord {
            timer_id: id.to_string(),
            agent_id: agent_id.to_string(),
            workspace_id: "ws_1".to_string(),
            prompt: "Check build status".to_string(),
            duration_seconds: 300,
            condition,
            state: ConditionalTimerState::Armed,
            created_at_ms: 1_760_000_000_000,
            fire_at_ms,
            resolved_at_ms: None,
            resolved_reason: None,
        }
    }

    #[test]
    fn conditional_timers_keep_user_version_6_and_roundtrip_all_conditions() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let user_version: i64 = {
            let conn = store.conn.lock().expect("lock conn");
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("read user_version")
        };
        assert_eq!(
            user_version, 6,
            "conditional_timers must keep PRAGMA user_version = 6"
        );

        let conditions = [
            TimerCondition::Never,
            TimerCondition::AnyMessage,
            TimerCondition::Sender {
                sender_id: "ag_worker".into(),
            },
            TimerCondition::AllSubagentsSettled,
            TimerCondition::FileChanged {
                path: "Cargo.toml".into(),
            },
        ];

        for (idx, cond) in conditions.iter().enumerate() {
            let timer = sample_timer(
                &format!("tmr_{idx}"),
                "ag_main",
                cond.clone(),
                1_760_000_100_000 + (idx as u64) * 1000,
            );
            store
                .insert_conditional_timer(&timer)
                .expect("insert timer");
            let loaded = store
                .get_conditional_timer(&timer.timer_id)
                .expect("get timer")
                .expect("timer exists");
            assert_eq!(loaded, timer);
        }

        let listed = store
            .list_conditional_timers(Some("ag_main"), false)
            .expect("list armed");
        assert_eq!(listed.len(), 5);
        // Ordered by fire_at_ms ascending
        assert_eq!(listed[0].timer_id, "tmr_0");
        assert_eq!(listed[4].timer_id, "tmr_4");
    }

    #[test]
    fn conditional_timer_atomic_transition_prevents_double_resolution() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let timer = sample_timer(
            "tmr_atomic",
            "ag_1",
            TimerCondition::AnyMessage,
            1_760_000_300_000,
        );
        store.insert_conditional_timer(&timer).expect("insert");

        let first = store
            .transition_conditional_timer(
                "tmr_atomic",
                ConditionalTimerState::Armed,
                ConditionalTimerState::CanceledEarly,
                1_760_000_120_000,
                Some("Canceled early by message from ag_peer"),
            )
            .expect("first transition")
            .expect("first transition succeeds");
        assert_eq!(first.state, ConditionalTimerState::CanceledEarly);
        assert_eq!(first.resolved_at_ms, Some(1_760_000_120_000));
        assert_eq!(
            first.resolved_reason.as_deref(),
            Some("Canceled early by message from ag_peer")
        );

        // Second transition from Armed must fail atomically (returns None)
        let second = store
            .transition_conditional_timer(
                "tmr_atomic",
                ConditionalTimerState::Armed,
                ConditionalTimerState::Fired,
                1_760_000_300_000,
                Some("Timer elapsed"),
            )
            .expect("second transition");
        assert!(second.is_none());

        // Armed-only listing excludes resolved; include_resolved includes it
        assert!(
            store
                .list_conditional_timers(Some("ag_1"), false)
                .expect("list armed")
                .is_empty()
        );
        let with_resolved = store
            .list_conditional_timers(Some("ag_1"), true)
            .expect("list with resolved");
        assert_eq!(with_resolved.len(), 1);
        assert_eq!(with_resolved[0].state, ConditionalTimerState::CanceledEarly);
    }

    #[test]
    fn conditional_timer_caps_enforced_per_agent_and_globally() {
        let store = OrbitStore::open_in_memory().expect("open store");

        // 1. Per-agent cap (16 armed timers)
        for i in 0..MAX_ARMED_TIMERS_PER_AGENT {
            let t = sample_timer(
                &format!("tmr_a0_{i}"),
                "ag_cap_0",
                TimerCondition::Never,
                1_760_000_100_000 + i as u64,
            );
            store
                .insert_conditional_timer(&t)
                .expect("within per-agent cap");
        }
        let overflow_agent = sample_timer(
            "tmr_a0_overflow",
            "ag_cap_0",
            TimerCondition::Never,
            1_760_000_999_000,
        );
        assert!(
            store.insert_conditional_timer(&overflow_agent).is_err(),
            "17th armed timer for same agent must be rejected"
        );

        // Canceling one frees a slot for that agent
        store
            .transition_conditional_timer(
                "tmr_a0_0",
                ConditionalTimerState::Armed,
                ConditionalTimerState::CanceledManual,
                1_760_000_150_000,
                Some("freed"),
            )
            .expect("cancel")
            .expect("transitioned");
        store
            .insert_conditional_timer(&overflow_agent)
            .expect("slot freed after cancel");

        // 2. Global cap (256 armed timers across 16 distinct agents x 16 timers each)
        // ag_cap_0 already holds 16 armed timers; fill ag_cap_1..ag_cap_15 with 16 each (= 256 total)
        let remaining_agents = (MAX_ARMED_TIMERS_GLOBAL / MAX_ARMED_TIMERS_PER_AGENT) - 1;
        for a in 1..=remaining_agents {
            for i in 0..MAX_ARMED_TIMERS_PER_AGENT {
                let t = sample_timer(
                    &format!("tmr_a{a}_{i}"),
                    &format!("ag_cap_{a}"),
                    TimerCondition::Never,
                    1_760_000_100_000 + (a * 100 + i) as u64,
                );
                store
                    .insert_conditional_timer(&t)
                    .expect("within global cap");
            }
        }
        let overflow_global = sample_timer(
            "tmr_global_overflow",
            "ag_cap_fresh",
            TimerCondition::Never,
            1_760_000_999_999,
        );
        assert!(
            store.insert_conditional_timer(&overflow_global).is_err(),
            "257th global armed timer must be rejected"
        );
    }

    #[test]
    fn conditional_timer_corrupt_state_or_condition_returns_sql_conversion_error() {
        let store = OrbitStore::open_in_memory().expect("open store");
        {
            let conn = store.conn.lock().expect("lock");
            conn.execute(
                "INSERT INTO conditional_timers (
                    timer_id, agent_id, workspace_id, prompt, duration_seconds,
                    condition_json, state, created_at_ms, fire_at_ms
                ) VALUES ('tmr_bad_state', 'ag_1', 'ws_1', 'p', 10, '{\"kind\":\"never\"}', 'bogus_state', 1, 2)",
                [],
            )
            .expect("insert bad state");
            conn.execute(
                "INSERT INTO conditional_timers (
                    timer_id, agent_id, workspace_id, prompt, duration_seconds,
                    condition_json, state, created_at_ms, fire_at_ms
                ) VALUES ('tmr_bad_cond', 'ag_1', 'ws_1', 'p', 10, '{not json', 'armed', 1, 2)",
                [],
            )
            .expect("insert bad cond");
        }
        assert!(store.get_conditional_timer("tmr_bad_state").is_err());
        assert!(store.get_conditional_timer("tmr_bad_cond").is_err());
    }
}
