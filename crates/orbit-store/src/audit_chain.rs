//! Host security decisions, ordered and signed under an immediate SQLite transaction.
use super::{OrbitStore, StoreError};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use orbit_protocol::AuditChainVerification;
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(super) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS security_audit_chain (
seq INTEGER PRIMARY KEY AUTOINCREMENT, timestamp_ms INTEGER NOT NULL, category TEXT NOT NULL,
action TEXT NOT NULL, actor TEXT NOT NULL, host_id TEXT, agent_id TEXT, outcome TEXT NOT NULL,
details_json TEXT NOT NULL, prev_hash TEXT NOT NULL, entry_hash TEXT NOT NULL, checkpoint_sig TEXT);
CREATE INDEX IF NOT EXISTS idx_audit_category_outcome ON security_audit_chain(category, outcome);";

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub seq: u64,
    pub timestamp_ms: i64,
    pub category: String,
    pub action: String,
    pub actor: String,
    pub host_id: Option<String>,
    pub agent_id: Option<String>,
    pub outcome: String,
    pub details_json: String,
    pub prev_hash: String,
    pub entry_hash: String,
    pub checkpoint_sig: Option<String>,
}

impl AuditEntry {
    fn from_row(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            seq: r.get(0)?,
            timestamp_ms: r.get(1)?,
            category: r.get(2)?,
            action: r.get(3)?,
            actor: r.get(4)?,
            host_id: r.get(5)?,
            agent_id: r.get(6)?,
            outcome: r.get(7)?,
            details_json: r.get(8)?,
            prev_hash: r.get(9)?,
            entry_hash: r.get(10)?,
            checkpoint_sig: r.get(11)?,
        })
    }

    fn digest(&self) -> String {
        let mut payload = json!({ "category": self.category, "action":self.action, "actor":self.actor,
            "host_id":self.host_id, "agent_id":self.agent_id, "outcome":self.outcome,"details_json":self.details_json });
        payload.sort_all_objects();
        let digest = Sha256::digest(format!(
            "{}|{}|{}|{}",
            self.seq, self.timestamp_ms, self.prev_hash, payload
        ));
        format!("{digest:x}")
    }
}

fn audit_high_water(conn: &Connection) -> Result<u64, StoreError> {
    Ok(conn.query_row(
        "SELECT COALESCE((SELECT seq FROM sqlite_sequence WHERE name='security_audit_chain'),0)",
        [],
        |r| r.get(0),
    )?)
}

impl OrbitStore {
    pub fn set_audit_identity(&self, host_id: String, key: SigningKey) -> Result<(), StoreError> {
        self.audit_identity
            .set((host_id, key))
            .map_err(|_| StoreError::InvalidArgument("audit identity already configured".into()))
    }
    pub fn audit_verifying_key(&self) -> Option<VerifyingKey> {
        self.audit_identity.get().map(|(_, k)| k.verifying_key())
    }

    pub fn append_audit_event(
        &self,
        category: &str,
        action: &str,
        actor: &str,
        agent_id: Option<&str>,
        outcome: &str,
        details: &Value,
    ) -> Result<u64, StoreError> {
        self.append_audit_event_with_host(category, action, actor, None, agent_id, outcome, details)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_audit_event_with_host(
        &self,
        category: &str,
        action: &str,
        actor: &str,
        host_id: Option<&str>,
        agent_id: Option<&str>,
        outcome: &str,
        details: &Value,
    ) -> Result<u64, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let seq = self.audit_insert_with_host(
            &tx, category, action, actor, host_id, agent_id, outcome, details,
        )?;
        tx.commit()?;
        Ok(seq)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn audit_insert(
        &self,
        tx: &Connection,
        category: &str,
        action: &str,
        actor: &str,
        agent_id: Option<&str>,
        outcome: &str,
        details: &Value,
    ) -> Result<u64, StoreError> {
        self.audit_insert_with_host(
            tx, category, action, actor, None, agent_id, outcome, details,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn audit_insert_with_host(
        &self,
        tx: &Connection,
        category: &str,
        action: &str,
        actor: &str,
        host_id: Option<&str>,
        agent_id: Option<&str>,
        outcome: &str,
        details: &Value,
    ) -> Result<u64, StoreError> {
        let head: Option<(u64, String)> = tx
            .query_row(
                "SELECT seq,entry_hash FROM security_audit_chain ORDER BY seq DESC LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (last, prev_hash) = head.unwrap_or_else(|| (0, "0".repeat(64)));
        let high_water = audit_high_water(tx)?;
        if last != high_water {
            return Err(StoreError::InvalidArgument(
                "audit chain tail missing".into(),
            ));
        }
        let seq = last
            .checked_add(1)
            .ok_or_else(|| StoreError::InvalidArgument("audit sequence exhausted".into()))?;
        let id = self.audit_identity.get();
        let resolved_host_id = host_id
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| id.map(|(h, _)| h.clone()));
        let mut details = details.clone();
        details.sort_all_objects();
        let mut e = AuditEntry {
            seq,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            category: category.into(),
            action: action.into(),
            actor: actor.into(),
            host_id: resolved_host_id,
            agent_id: agent_id.map(str::to_owned),
            outcome: outcome.into(),
            details_json: serde_json::to_string(&details)?,
            prev_hash,
            entry_hash: String::new(),
            checkpoint_sig: None,
        };
        e.entry_hash = e.digest();
        e.checkpoint_sig =
            id.map(|(_, k)| STANDARD.encode(k.sign(e.entry_hash.as_bytes()).to_bytes()));
        tx.execute("INSERT INTO security_audit_chain (seq,timestamp_ms,category,action,actor,host_id,agent_id,outcome,details_json,prev_hash,entry_hash,checkpoint_sig) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            params![e.seq,e.timestamp_ms,e.category,e.action,e.actor,e.host_id,e.agent_id,e.outcome,e.details_json,e.prev_hash,e.entry_hash,e.checkpoint_sig])?;
        Ok(seq)
    }

    pub fn verify_audit_chain(
        &self,
        verifying_key: Option<&VerifyingKey>,
    ) -> Result<AuditChainVerification, StoreError> {
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction()?;
        let high_water = audit_high_water(&tx)?;
        let mut stmt = tx.prepare("SELECT * FROM security_audit_chain ORDER BY seq")?;
        let mut status = AuditChainVerification {
            verified_entries: 0,
            broken_seq: None,
            signed_checkpoints: 0,
        };
        let mut prev = "0".repeat(64);
        for row in stmt.query_map([], AuditEntry::from_row)? {
            let e = row?;
            let seq = status.verified_entries + 1;
            let valid_sig = verifying_key.is_none_or(|k| {
                e.checkpoint_sig
                    .as_ref()
                    .and_then(|s| STANDARD.decode(s).ok())
                    .and_then(|b| Signature::from_slice(&b).ok())
                    .is_some_and(|s| k.verify_strict(e.entry_hash.as_bytes(), &s).is_ok())
            });
            if e.seq != seq || e.prev_hash != prev || e.entry_hash != e.digest() || !valid_sig {
                status.broken_seq = Some(seq);
                break;
            }
            prev = e.entry_hash;
            status.verified_entries += 1;
            status.signed_checkpoints +=
                u64::from(e.checkpoint_sig.is_some() && verifying_key.is_some());
        }
        if status.broken_seq.is_none() && status.verified_entries != high_water {
            status.broken_seq = Some(status.verified_entries + 1);
        }
        Ok(status)
    }

    pub fn list_audit_events(
        &self,
        limit: u32,
        before_seq: Option<u64>,
        category: Option<&str>,
    ) -> Result<Vec<AuditEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt=conn.prepare("SELECT * FROM security_audit_chain WHERE (?1 IS NULL OR seq < ?1) AND (?2 IS NULL OR category=?2) ORDER BY seq DESC LIMIT ?3")?;
        Ok(stmt
            .query_map(
                params![before_seq, category, limit.min(1000)],
                AuditEntry::from_row,
            )?
            .collect::<Result<_, _>>()?)
    }

    pub fn list_audit_events_since(
        &self,
        limit: u32,
        category: Option<&str>,
        since_ms: Option<i64>,
    ) -> Result<Vec<AuditEntry>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "SELECT * FROM security_audit_chain \
             WHERE (?1 IS NULL OR category=?1) AND (?2 IS NULL OR timestamp_ms >= ?2) \
             ORDER BY seq DESC LIMIT ?3",
        )?;
        Ok(stmt
            .query_map(
                params![category, since_ms, limit.min(1000)],
                AuditEntry::from_row,
            )?
            .collect::<Result<_, _>>()?)
    }

    pub fn export_audit_chain_jsonl(
        &self,
        after_seq: u64,
    ) -> Result<(String, Option<u64>), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt =
            conn.prepare("SELECT * FROM security_audit_chain WHERE seq>? ORDER BY seq LIMIT 257")?;
        let mut out = String::new();
        let mut last = after_seq;
        for (i, row) in stmt
            .query_map([after_seq], AuditEntry::from_row)?
            .enumerate()
        {
            let row = row?;
            if i == 256 {
                return Ok((out, Some(last)));
            }
            last = row.seq;
            out.push_str(&serde_json::to_string(&row)?);
            out.push('\n');
        }
        Ok((out, None))
    }
    pub fn integrity_ok(&self) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? == "ok")
    }
    pub fn audit_count(
        &self,
        category: Option<&str>,
        outcome: Option<&str>,
    ) -> Result<u64, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        Ok(conn.query_row("SELECT count(*) FROM security_audit_chain WHERE (?1 IS NULL OR category=?1) AND (?2 IS NULL OR outcome=?2)",params![category,outcome],|r|r.get(0))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_first_altered_or_missing_sequence() -> Result<(), StoreError> {
        for mutation in [
            "UPDATE security_audit_chain SET details_json='{}' WHERE seq=2",
            "UPDATE security_audit_chain SET outcome='changed' WHERE seq=2",
            "DELETE FROM security_audit_chain WHERE seq=2",
            "UPDATE security_audit_chain SET checkpoint_sig=NULL WHERE seq=2",
        ] {
            let store = OrbitStore::open_in_memory()?;
            store.set_audit_identity("host".into(), SigningKey::from_bytes(&[7; 32]))?;
            for _ in 0..3 {
                store.append_audit_event(
                    "permission",
                    "resolve",
                    "human",
                    None,
                    "allow",
                    &json!({"n":1}),
                )?;
            }
            let key = store.audit_verifying_key();
            assert_eq!(
                store.verify_audit_chain(key.as_ref())?.signed_checkpoints,
                3
            );
            store
                .conn
                .lock()
                .map_err(|_| StoreError::LockPoisoned)?
                .execute(mutation, [])?;
            assert_eq!(store.verify_audit_chain(key.as_ref())?.broken_seq, Some(2));
        }
        Ok(())
    }

    #[test]
    fn audit_events_with_host_and_since_ms_preserve_chain_integrity() -> Result<(), StoreError> {
        let store = OrbitStore::open_in_memory()?;
        store.set_audit_identity("host_local".into(), SigningKey::from_bytes(&[9; 32]))?;
        store.append_audit_event_with_host(
            "personal_agent_decision",
            "allow",
            "personal_agent",
            Some("host_remote"),
            Some("ag_1"),
            "applied",
            &json!({"request_id": "r1", "reason": "ok"}),
        )?;
        let entries =
            store.list_audit_events_since(50, Some("personal_agent_decision"), Some(0))?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].host_id.as_deref(), Some("host_remote"));
        let key = store.audit_verifying_key();
        let verified = store.verify_audit_chain(key.as_ref())?;
        assert_eq!(verified.broken_seq, None);
        assert_eq!(verified.verified_entries, 1);
        assert_eq!(verified.signed_checkpoints, 1);

        let future = store.list_audit_events_since(
            50,
            Some("personal_agent_decision"),
            Some(entries[0].timestamp_ms + 60_000),
        )?;
        assert!(future.is_empty());
        Ok(())
    }
}
