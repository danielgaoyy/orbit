//! SQLite persistence for External Event Triggers (`event_triggers`, `event_trigger_deliveries`).
//!
//! Creates tables idempotently via `CREATE TABLE IF NOT EXISTS` without bumping
//! `PRAGMA user_version` (keeps `user_version = 6`).
//!
//! Secret material (`secret`, `github_token`) is stored server-side and NEVER returned in
//! sanitized [`EventTriggerRecord`] list snapshots (`masked_secret` is strictly `"••••"`).

use std::fmt;

use orbit_protocol::{
    EventTriggerDelivery, EventTriggerRecord, GitHubPollConfig, TriggerDispatchTarget,
    TriggerSourceKind,
};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params, types::Type};

use crate::{OrbitStore, StoreError};

/// Maximum number of legitimate delivery rows retained in SQLite (protects persisted `dedup_key`s).
const MAX_RETAINED_LEGIT_DELIVERIES: i64 = 450;
/// Maximum number of rejected/rate-limited audit rows retained in SQLite (isolated from legit rows).
const MAX_RETAINED_REJECTED_DELIVERIES: i64 = 50;
/// Maximum total rows returned by `list_recent_event_trigger_deliveries`.
const MAX_RETAINED_DELIVERIES: i64 =
    MAX_RETAINED_LEGIT_DELIVERIES + MAX_RETAINED_REJECTED_DELIVERIES;
/// Default number of recent deliveries returned in snapshots.
pub const DEFAULT_RECENT_DELIVERIES_LIMIT: usize = 50;

/// Full server-side trigger row including plaintext secrets for daemon-internal verification/polling.
#[derive(Clone)]
pub struct StoredEventTrigger {
    pub record: EventTriggerRecord,
    pub secret: Option<String>,
    pub github_token: Option<String>,
    pub github_poll_state_json: Option<String>,
}

impl fmt::Debug for StoredEventTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredEventTrigger")
            .field("record", &self.record)
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .field(
                "github_token",
                &self.github_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("github_poll_state_json", &self.github_poll_state_json)
            .finish()
    }
}

/// Parameters for creating or updating an external event trigger in SQLite.
#[derive(Clone)]
pub struct UpsertEventTriggerParams {
    pub trigger_id: String,
    pub name: String,
    pub source: TriggerSourceKind,
    pub event_filters: Vec<String>,
    pub prompt_template: String,
    pub target: TriggerDispatchTarget,
    pub webhook_path: String,
    pub secret: Option<String>,
    pub keep_existing_secret: bool,
    pub github_poll: Option<GitHubPollConfig>,
    pub github_token: Option<String>,
    pub keep_existing_token: bool,
    pub enabled: bool,
    pub now_ms: u64,
}

impl fmt::Debug for UpsertEventTriggerParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpsertEventTriggerParams")
            .field("trigger_id", &self.trigger_id)
            .field("name", &self.name)
            .field("source", &self.source)
            .field("event_filters", &self.event_filters)
            .field("prompt_template", &self.prompt_template)
            .field("target", &self.target)
            .field("webhook_path", &self.webhook_path)
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .field("keep_existing_secret", &self.keep_existing_secret)
            .field("github_poll", &self.github_poll)
            .field(
                "github_token",
                &self.github_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("keep_existing_token", &self.keep_existing_token)
            .field("enabled", &self.enabled)
            .field("now_ms", &self.now_ms)
            .finish()
    }
}

/// Mask a secret value for wire snapshots without leaking any plaintext characters.
pub fn mask_trigger_secret(secret: &str) -> String {
    if secret.is_empty() {
        String::new()
    } else {
        "••••".to_string()
    }
}

/// Idempotent schema bootstrap for `event_triggers` and `event_trigger_deliveries`.
///
/// Intentionally does NOT alter `PRAGMA user_version`.
pub(crate) fn migrate_event_triggers_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS event_triggers (
            trigger_id             TEXT PRIMARY KEY,
            name                   TEXT NOT NULL,
            source                 TEXT NOT NULL,
            event_filters_json     TEXT NOT NULL DEFAULT '[]',
            prompt_template        TEXT NOT NULL,
            target_json            TEXT NOT NULL,
            webhook_path           TEXT NOT NULL,
            secret                 TEXT,
            github_poll_json       TEXT,
            github_token           TEXT,
            github_poll_state_json TEXT,
            enabled                INTEGER NOT NULL DEFAULT 1,
            trigger_count          INTEGER NOT NULL DEFAULT 0,
            last_triggered_at_ms   INTEGER,
            created_at_ms          INTEGER NOT NULL,
            updated_at_ms          INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_event_triggers_enabled_source
            ON event_triggers(enabled, source, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS event_trigger_deliveries (
            delivery_id         TEXT PRIMARY KEY,
            trigger_id          TEXT NOT NULL,
            source              TEXT NOT NULL,
            event_type          TEXT NOT NULL,
            status              TEXT NOT NULL,
            summary             TEXT NOT NULL,
            dispatched_agent_id TEXT,
            dedup_key           TEXT,
            received_at_ms      INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_event_trigger_deliveries_received
            ON event_trigger_deliveries(received_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_event_trigger_deliveries_trigger
            ON event_trigger_deliveries(trigger_id, received_at_ms DESC);

        CREATE INDEX IF NOT EXISTS idx_event_trigger_deliveries_dedup
            ON event_trigger_deliveries(trigger_id, dedup_key);
        ",
    )?;
    Ok(())
}

const TRIGGER_SELECT_COLS: &str = "trigger_id, name, source, event_filters_json, prompt_template, target_json, webhook_path, secret, github_poll_json, github_token, github_poll_state_json, enabled, trigger_count, last_triggered_at_ms, created_at_ms, updated_at_ms";

const DELIVERY_SELECT_COLS: &str = "delivery_id, trigger_id, source, event_type, status, summary, dispatched_agent_id, received_at_ms";

fn map_stored_trigger_row(row: &Row<'_>) -> rusqlite::Result<StoredEventTrigger> {
    let source_raw: String = row.get(2)?;
    let source = TriggerSourceKind::from_slug(&source_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown event trigger source `{source_raw}`"),
            )),
        )
    })?;
    let filters_json: String = row.get(3)?;
    let event_filters: Vec<String> = serde_json::from_str(&filters_json)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(err)))?;
    let target_json: String = row.get(5)?;
    let target: TriggerDispatchTarget = serde_json::from_str(&target_json)
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(5, Type::Text, Box::new(err)))?;
    let secret: Option<String> = row.get::<_, Option<String>>(7)?.filter(|s| !s.is_empty());
    let github_poll_json: Option<String> = row.get(8)?;
    let github_token: Option<String> = row.get::<_, Option<String>>(9)?.filter(|s| !s.is_empty());
    let github_poll_state_json: Option<String> = row.get(10)?;
    let github_poll = github_poll_json
        .map(|raw| {
            serde_json::from_str::<GitHubPollConfig>(&raw).map(|mut cfg| {
                cfg.has_token = github_token.is_some();
                cfg.clamped()
            })
        })
        .transpose()
        .map_err(|err| rusqlite::Error::FromSqlConversionFailure(8, Type::Text, Box::new(err)))?;
    let enabled_i32: i32 = row.get(11)?;
    let trigger_count_i64: i64 = row.get(12)?;
    let last_triggered_i64: Option<i64> = row.get(13)?;
    let created_i64: i64 = row.get(14)?;
    let updated_i64: i64 = row.get(15)?;

    let has_secret = secret.is_some();
    let masked_secret = secret.as_deref().map(mask_trigger_secret);

    let record = EventTriggerRecord {
        trigger_id: row.get(0)?,
        name: row.get(1)?,
        source,
        event_filters,
        prompt_template: row.get(4)?,
        target,
        webhook_path: row.get(6)?,
        has_secret,
        masked_secret,
        github_poll,
        enabled: enabled_i32 != 0,
        trigger_count: trigger_count_i64.max(0) as u64,
        last_triggered_at_ms: last_triggered_i64.map(|v| v.max(0) as u64),
        created_at_ms: created_i64.max(0) as u64,
        updated_at_ms: updated_i64.max(0) as u64,
    };

    Ok(StoredEventTrigger {
        record,
        secret,
        github_token,
        github_poll_state_json,
    })
}

fn map_delivery_row(row: &Row<'_>) -> rusqlite::Result<EventTriggerDelivery> {
    let source_raw: String = row.get(2)?;
    let source = TriggerSourceKind::from_slug(&source_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown event trigger delivery source `{source_raw}`"),
            )),
        )
    })?;
    let received_i64: i64 = row.get(7)?;
    Ok(EventTriggerDelivery {
        delivery_id: row.get(0)?,
        trigger_id: row.get(1)?,
        source,
        event_type: row.get(3)?,
        status: row.get(4)?,
        summary: row.get(5)?,
        dispatched_agent_id: row.get(6)?,
        received_at_ms: received_i64.max(0) as u64,
    })
}

impl OrbitStore {
    /// Insert or update an event trigger while safely preserving existing secrets when requested.
    pub fn upsert_event_trigger(
        &self,
        input: &UpsertEventTriggerParams,
    ) -> Result<StoredEventTrigger, StoreError> {
        let now_i64 = i64::try_from(input.now_ms)
            .map_err(|_| StoreError::InvalidArgument("now_ms exceeds i64".into()))?;
        let filters_json = serde_json::to_string(&input.event_filters)?;
        let target_json = serde_json::to_string(&input.target)?;

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing_sql =
            format!("SELECT {TRIGGER_SELECT_COLS} FROM event_triggers WHERE trigger_id = ?1");
        let existing: Option<StoredEventTrigger> = tx
            .query_row(
                &existing_sql,
                params![input.trigger_id],
                map_stored_trigger_row,
            )
            .optional()?;

        let incoming_secret = input
            .secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let final_secret = if incoming_secret.is_some() {
            incoming_secret
        } else if input.keep_existing_secret {
            existing.as_ref().and_then(|ex| ex.secret.clone())
        } else {
            None
        };

        // Gate stored GitHub PAT on GitHub source AND active polling configuration so disabling
        // polling always clears any previously stored PAT instead of leaving it orphaned.
        let poll_active = input.source == TriggerSourceKind::GitHub && input.github_poll.is_some();
        let incoming_token = input
            .github_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let final_token = if poll_active {
            if incoming_token.is_some() {
                incoming_token
            } else if input.keep_existing_token {
                existing.as_ref().and_then(|ex| ex.github_token.clone())
            } else {
                None
            }
        } else {
            None
        };

        let sanitized_poll = if poll_active {
            input.github_poll.clone().map(|mut cfg| {
                cfg.has_token = final_token.is_some();
                cfg.clamped()
            })
        } else {
            None
        };
        let github_poll_json = sanitized_poll
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;

        // Reset poll cursor if repo changed; preserve cursor if same repo.
        let poll_state_json = match (&existing, &sanitized_poll) {
            (Some(ex), Some(new_poll))
                if ex
                    .record
                    .github_poll
                    .as_ref()
                    .is_some_and(|old| old.repo.eq_ignore_ascii_case(&new_poll.repo)) =>
            {
                ex.github_poll_state_json.clone()
            }
            _ => None,
        };

        let created_i64 = existing
            .as_ref()
            .map(|ex| ex.record.created_at_ms as i64)
            .unwrap_or(now_i64);
        let trigger_count_i64 = existing
            .as_ref()
            .map(|ex| ex.record.trigger_count as i64)
            .unwrap_or(0);
        let last_triggered_i64 = existing
            .as_ref()
            .and_then(|ex| ex.record.last_triggered_at_ms.map(|v| v as i64));

        tx.execute(
            "INSERT INTO event_triggers (
                trigger_id, name, source, event_filters_json, prompt_template, target_json,
                webhook_path, secret, github_poll_json, github_token, github_poll_state_json,
                enabled, trigger_count, last_triggered_at_ms, created_at_ms, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            ON CONFLICT(trigger_id) DO UPDATE SET
                name = excluded.name,
                source = excluded.source,
                event_filters_json = excluded.event_filters_json,
                prompt_template = excluded.prompt_template,
                target_json = excluded.target_json,
                webhook_path = excluded.webhook_path,
                secret = excluded.secret,
                github_poll_json = excluded.github_poll_json,
                github_token = excluded.github_token,
                github_poll_state_json = excluded.github_poll_state_json,
                enabled = excluded.enabled,
                updated_at_ms = excluded.updated_at_ms",
            params![
                input.trigger_id,
                input.name,
                input.source.slug(),
                filters_json,
                input.prompt_template,
                target_json,
                input.webhook_path,
                final_secret,
                github_poll_json,
                final_token,
                poll_state_json,
                i32::from(input.enabled),
                trigger_count_i64,
                last_triggered_i64,
                created_i64,
                now_i64,
            ],
        )?;

        let saved = tx.query_row(
            &existing_sql,
            params![input.trigger_id],
            map_stored_trigger_row,
        )?;
        tx.commit()?;
        Ok(saved)
    }

    /// Fetch a single trigger including server-side secret/token for verification & polling.
    pub fn get_event_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<Option<StoredEventTrigger>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {TRIGGER_SELECT_COLS} FROM event_triggers WHERE trigger_id = ?1");
        conn.query_row(&sql, params![trigger_id], map_stored_trigger_row)
            .optional()
            .map_err(Into::into)
    }

    /// List all triggers as sanitized wire records (secrets strictly masked).
    pub fn list_event_triggers(&self) -> Result<Vec<EventTriggerRecord>, StoreError> {
        Ok(self
            .list_stored_event_triggers()?
            .into_iter()
            .map(|row| row.record)
            .collect())
    }

    /// List all stored triggers including server-side secrets (daemon-internal use only).
    pub fn list_stored_event_triggers(&self) -> Result<Vec<StoredEventTrigger>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {TRIGGER_SELECT_COLS} FROM event_triggers ORDER BY created_at_ms DESC, trigger_id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_stored_trigger_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Delete a trigger by ID. Returns `true` if a row existed.
    pub fn delete_event_trigger(&self, trigger_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM event_triggers WHERE trigger_id = ?1",
            params![trigger_id],
        )?;
        Ok(affected > 0)
    }

    /// Update persisted GitHub polling cursor/ETag state for a trigger.
    pub fn update_event_trigger_poll_state(
        &self,
        trigger_id: &str,
        state_json: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute(
            "UPDATE event_triggers SET github_poll_state_json = ?1 WHERE trigger_id = ?2",
            params![state_json, trigger_id],
        )?;
        Ok(())
    }

    /// Check whether a non-empty external delivery deduplication key has already been processed
    /// for `trigger_id` (excluding `"duplicate"` audit rows).
    pub fn has_event_trigger_dedup_key(
        &self,
        trigger_id: &str,
        dedup_key: &str,
    ) -> Result<bool, StoreError> {
        let key = dedup_key.trim();
        if key.is_empty() {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let found: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM event_trigger_deliveries
                 WHERE trigger_id = ?1 AND dedup_key = ?2 AND status != 'duplicate'
                 LIMIT 1",
                params![trigger_id, key],
                |r| r.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Record a trigger delivery audit entry, optionally incrementing `trigger_count` and
    /// `last_triggered_at_ms`.
    ///
    /// Retention is partitioned so rejected/rate-limited audit entries can NEVER evict
    /// legitimate persisted deliveries or their `dedup_key`s.
    pub fn record_event_trigger_delivery(
        &self,
        delivery: &EventTriggerDelivery,
        dedup_key: Option<&str>,
        increment_trigger_count: bool,
    ) -> Result<(), StoreError> {
        let received_i64 = i64::try_from(delivery.received_at_ms)
            .map_err(|_| StoreError::InvalidArgument("received_at_ms exceeds i64".into()))?;
        let clean_dedup = dedup_key.map(str::trim).filter(|s| !s.is_empty());

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO event_trigger_deliveries (
                delivery_id, trigger_id, source, event_type, status, summary,
                dispatched_agent_id, dedup_key, received_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                delivery.delivery_id,
                delivery.trigger_id,
                delivery.source.slug(),
                delivery.event_type,
                delivery.status,
                delivery.summary,
                delivery.dispatched_agent_id,
                clean_dedup,
                received_i64,
            ],
        )?;
        if increment_trigger_count {
            tx.execute(
                "UPDATE event_triggers
                 SET trigger_count = trigger_count + 1,
                     last_triggered_at_ms = ?1
                 WHERE trigger_id = ?2",
                params![received_i64, delivery.trigger_id],
            )?;
        }
        // Partitioned pruning: unauthenticated 401/429 rows are capped separately from
        // legitimate deliveries so an external flood can never wipe persisted dedup keys.
        let is_rejected = matches!(
            delivery.status.as_str(),
            "rejected_signature" | "rate_limited"
        );
        if is_rejected {
            tx.execute(
                "DELETE FROM event_trigger_deliveries
                 WHERE status IN ('rejected_signature', 'rate_limited')
                   AND delivery_id NOT IN (
                       SELECT delivery_id FROM event_trigger_deliveries
                       WHERE status IN ('rejected_signature', 'rate_limited')
                       ORDER BY received_at_ms DESC, rowid DESC
                       LIMIT ?1
                   )",
                params![MAX_RETAINED_REJECTED_DELIVERIES],
            )?;
        } else {
            tx.execute(
                "DELETE FROM event_trigger_deliveries
                 WHERE status NOT IN ('rejected_signature', 'rate_limited')
                   AND delivery_id NOT IN (
                       SELECT delivery_id FROM event_trigger_deliveries
                       WHERE status NOT IN ('rejected_signature', 'rate_limited')
                       ORDER BY received_at_ms DESC, rowid DESC
                       LIMIT ?1
                   )",
                params![MAX_RETAINED_LEGIT_DELIVERIES],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// List recent trigger deliveries newest-first (capped to `limit`).
    pub fn list_recent_event_trigger_deliveries(
        &self,
        limit: usize,
    ) -> Result<Vec<EventTriggerDelivery>, StoreError> {
        let capped = limit.clamp(1, MAX_RETAINED_DELIVERIES as usize) as i64;
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {DELIVERY_SELECT_COLS} FROM event_trigger_deliveries
             ORDER BY received_at_ms DESC, rowid DESC
             LIMIT ?1"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![capped], map_delivery_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbit_protocol::{ProviderKind, SessionMode};

    fn sample_upsert(trigger_id: &str, secret: Option<&str>) -> UpsertEventTriggerParams {
        UpsertEventTriggerParams {
            trigger_id: trigger_id.to_string(),
            name: "PR Reviewer".to_string(),
            source: TriggerSourceKind::GitHub,
            event_filters: vec!["pull_request.opened".into()],
            prompt_template: "Review PR {{title}} ({{url}})".into(),
            target: TriggerDispatchTarget::SpawnWorkspaceAgent {
                cwd: "/repo/orbit".into(),
                provider: ProviderKind::ClaudeCode,
                model: Some("claude-sonnet-4-5".into()),
                mode: SessionMode::AcceptEdits,
            },
            webhook_path: format!("/webhooks/{trigger_id}"),
            secret: secret.map(str::to_string),
            keep_existing_secret: false,
            github_poll: Some(GitHubPollConfig {
                repo: "octocat/orbit".into(),
                poll_interval_secs: 15, // should clamp to 30
                watch_prs: true,
                watch_issues: false,
                watch_ci_failures: true,
                has_token: false,
            }),
            github_token: Some("ghp_super_secret_token_123".into()),
            keep_existing_token: false,
            enabled: true,
            now_ms: 1_760_000_000_000,
        }
    }

    #[test]
    fn event_triggers_keep_user_version_6_and_mask_secrets() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let user_version: i64 = {
            let conn = store.conn.lock().expect("lock");
            conn.query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("read user_version")
        };
        assert_eq!(
            user_version, 6,
            "event_triggers tables must keep PRAGMA user_version = 6"
        );

        let params = sample_upsert("trg_1", Some("whsec_top_secret"));
        // Debug impl must redact secret & token
        let params_dbg = format!("{params:?}");
        assert!(!params_dbg.contains("whsec_top_secret"));
        assert!(!params_dbg.contains("ghp_super_secret_token_123"));
        assert!(params_dbg.contains("[REDACTED]"));

        let saved = store.upsert_event_trigger(&params).expect("insert trigger");
        let saved_dbg = format!("{saved:?}");
        assert!(!saved_dbg.contains("whsec_top_secret"));
        assert!(!saved_dbg.contains("ghp_super_secret_token_123"));
        assert!(saved_dbg.contains("[REDACTED]"));

        assert_eq!(saved.secret.as_deref(), Some("whsec_top_secret"));
        assert_eq!(
            saved.github_token.as_deref(),
            Some("ghp_super_secret_token_123")
        );
        assert!(saved.record.has_secret);
        assert_eq!(saved.record.masked_secret.as_deref(), Some("••••"));
        assert_eq!(
            saved
                .record
                .github_poll
                .as_ref()
                .map(|p| p.poll_interval_secs),
            Some(30)
        );
        assert_eq!(
            saved.record.github_poll.as_ref().map(|p| p.has_token),
            Some(true)
        );

        // Serialized list snapshot must never leak plaintext secret or GitHub token
        let listed = store.list_event_triggers().expect("list triggers");
        assert_eq!(listed.len(), 1);
        let serialized = serde_json::to_string(&listed).expect("serialize");
        assert!(!serialized.contains("whsec_top_secret"));
        assert!(!serialized.contains("ghp_super_secret_token_123"));
        assert_eq!(listed[0].masked_secret.as_deref(), Some("••••"));

        // Updating with keep_existing_secret + keep_existing_token preserves secrets
        let mut update = sample_upsert("trg_1", None);
        update.name = "PR Reviewer Updated".into();
        update.keep_existing_secret = true;
        update.github_token = None;
        update.keep_existing_token = true;
        update.now_ms = 1_760_000_050_000;
        let updated = store.upsert_event_trigger(&update).expect("update trigger");
        assert_eq!(updated.record.name, "PR Reviewer Updated");
        assert_eq!(updated.secret.as_deref(), Some("whsec_top_secret"));
        assert_eq!(
            updated.github_token.as_deref(),
            Some("ghp_super_secret_token_123")
        );
        assert_eq!(updated.record.created_at_ms, 1_760_000_000_000);
        assert_eq!(updated.record.updated_at_ms, 1_760_000_050_000);

        // Disabling github_poll MUST clear any previously stored github_token even if keep_existing_token=true
        let mut disable_poll = sample_upsert("trg_1", None);
        disable_poll.keep_existing_secret = true;
        disable_poll.github_poll = None;
        disable_poll.github_token = None;
        disable_poll.keep_existing_token = true;
        disable_poll.now_ms = 1_760_000_090_000;
        let cleared = store
            .upsert_event_trigger(&disable_poll)
            .expect("disable poll");
        assert!(cleared.record.github_poll.is_none());
        assert!(
            cleared.github_token.is_none(),
            "disabling polling must clear orphaned GitHub PAT"
        );
        assert_eq!(cleared.secret.as_deref(), Some("whsec_top_secret"));
    }

    #[test]
    fn event_trigger_deliveries_dedup_and_count_and_rejected_flood_isolation() {
        let store = OrbitStore::open_in_memory().expect("open store");
        store
            .upsert_event_trigger(&sample_upsert("trg_del", Some("sec")))
            .expect("insert");

        let delivery = EventTriggerDelivery {
            delivery_id: "del_1".into(),
            trigger_id: "trg_del".into(),
            source: TriggerSourceKind::GitHub,
            event_type: "pull_request.opened".into(),
            status: "dispatched".into(),
            summary: "Dispatched to agent ag_1".into(),
            dispatched_agent_id: Some("ag_1".into()),
            received_at_ms: 1_760_000_100_000,
        };
        store
            .record_event_trigger_delivery(&delivery, Some("gh-delivery-abc"), true)
            .expect("record delivery");

        // Flood 80 rejected_signature deliveries — must NOT evict the legitimate delivery or dedup_key
        for i in 0..80u64 {
            let bad = EventTriggerDelivery {
                delivery_id: format!("del_bad_{i}"),
                trigger_id: "trg_del".into(),
                source: TriggerSourceKind::GitHub,
                event_type: "webhook".into(),
                status: "rejected_signature".into(),
                summary: "bad sig".into(),
                dispatched_agent_id: None,
                received_at_ms: 1_760_000_200_000 + i,
            };
            store
                .record_event_trigger_delivery(&bad, None, false)
                .expect("record rejected");
        }

        assert!(
            store
                .has_event_trigger_dedup_key("trg_del", "gh-delivery-abc")
                .expect("dedup check must survive rejected flood")
        );
        assert!(
            !store
                .has_event_trigger_dedup_key("trg_del", "gh-delivery-other")
                .expect("dedup miss")
        );

        let reloaded = store
            .get_event_trigger("trg_del")
            .expect("get")
            .expect("exists");
        assert_eq!(reloaded.record.trigger_count, 1);
        assert_eq!(
            reloaded.record.last_triggered_at_ms,
            Some(1_760_000_100_000)
        );

        let recent = store
            .list_recent_event_trigger_deliveries(100)
            .expect("list deliveries");
        // 50 capped rejected + 1 legit = 51 total
        assert_eq!(recent.len(), 51);
        assert!(recent.iter().any(|d| d.delivery_id == "del_1"));

        assert!(store.delete_event_trigger("trg_del").expect("delete"));
        assert!(!store.delete_event_trigger("trg_del").expect("delete twice"));
    }
}
