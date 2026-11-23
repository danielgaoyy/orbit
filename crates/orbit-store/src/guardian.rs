//! Idempotent Guardian audit log persistence without bumping SQLite `PRAGMA user_version`
//! (preserves `user_version = 6`).

use orbit_protocol::GuardianAuditEntry;
use rusqlite::{Connection, params};

use crate::{OrbitStore, StoreError};

/// Maximum persisted Guardian audit entries retained in SQLite.
const MAX_PERSISTED_GUARDIAN_AUDITS: i64 = 500;

/// Idempotently create the `guardian_audit_log` table and index.
/// Intentionally does NOT alter `PRAGMA user_version`.
pub(crate) fn migrate_guardian_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS guardian_audit_log (
            id              TEXT PRIMARY KEY,
            agent_id        TEXT NOT NULL,
            request_id      TEXT NOT NULL,
            tool_name       TEXT NOT NULL,
            target_summary  TEXT NOT NULL,
            assessment_json TEXT NOT NULL,
            decided_at_ms   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_guardian_audit_decided_at
            ON guardian_audit_log(decided_at_ms DESC);
        ",
    )?;
    Ok(())
}

impl OrbitStore {
    /// Persist a Guardian audit entry and trim older rows beyond the rolling retention cap.
    pub fn record_guardian_audit(&self, entry: &GuardianAuditEntry) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let assessment_json = serde_json::to_string(&entry.assessment)?;
        let decided_at_ms = i64::try_from(entry.decided_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            INSERT OR REPLACE INTO guardian_audit_log (
                id, agent_id, request_id, tool_name, target_summary, assessment_json, decided_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ",
            params![
                entry.id,
                entry.agent_id,
                entry.request_id,
                entry.tool_name,
                entry.target_summary,
                assessment_json,
                decided_at_ms,
            ],
        )?;
        conn.execute(
            "
            DELETE FROM guardian_audit_log
            WHERE id NOT IN (
                SELECT id FROM guardian_audit_log
                ORDER BY decided_at_ms DESC, rowid DESC
                LIMIT ?1
            )
            ",
            params![MAX_PERSISTED_GUARDIAN_AUDITS],
        )?;
        Ok(())
    }

    /// Return the most recent Guardian audit entries ordered newest-first.
    pub fn list_recent_guardian_audits(
        &self,
        limit: usize,
    ) -> Result<Vec<GuardianAuditEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let cap = i64::try_from(limit.clamp(1, 500)).unwrap_or(200);
        let mut stmt = conn.prepare(
            "
            SELECT id, agent_id, request_id, tool_name, target_summary, assessment_json, decided_at_ms
            FROM guardian_audit_log
            ORDER BY decided_at_ms DESC, rowid DESC
            LIMIT ?1
            ",
        )?;
        let rows = stmt.query_map(params![cap], |row| {
            let id: String = row.get(0)?;
            let agent_id: String = row.get(1)?;
            let request_id: String = row.get(2)?;
            let tool_name: String = row.get(3)?;
            let target_summary: String = row.get(4)?;
            let assessment_json: String = row.get(5)?;
            let decided_at_ms_raw: i64 = row.get(6)?;
            Ok((
                id,
                agent_id,
                request_id,
                tool_name,
                target_summary,
                assessment_json,
                decided_at_ms_raw,
            ))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (
                id,
                agent_id,
                request_id,
                tool_name,
                target_summary,
                assessment_json,
                decided_at_ms_raw,
            ) = row?;
            if let Ok(assessment) = serde_json::from_str(&assessment_json) {
                out.push(GuardianAuditEntry {
                    id,
                    agent_id,
                    request_id,
                    tool_name,
                    target_summary,
                    assessment,
                    decided_at_ms: u64::try_from(decided_at_ms_raw).unwrap_or(0),
                });
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::{GuardianAssessment, GuardianDecision, GuardianRiskLevel};

    #[test]
    fn guardian_audit_log_preserves_user_version_6_and_roundtrips() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let uv: i64 = store
            .conn
            .lock()
            .expect("lock")
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(uv, 6);

        let entry = GuardianAuditEntry {
            id: "ga_test_1".into(),
            agent_id: "ag_1".into(),
            request_id: "req_1".into(),
            tool_name: "shell".into(),
            target_summary: "cargo test".into(),
            assessment: GuardianAssessment {
                risk_level: GuardianRiskLevel::Low,
                decision: GuardianDecision::AutoAllow,
                confidence: 0.99,
                classifier_tier: "deterministic".into(),
                reason: "Safe workspace test command".into(),
                signals: vec!["workspace_test".into()],
                latency_ms: 0,
            },
            decided_at_ms: 1_760_000_000_001,
        };
        store
            .record_guardian_audit(&entry)
            .expect("record audit entry");
        let list = store
            .list_recent_guardian_audits(20)
            .expect("list audit entries");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], entry);
    }
}
