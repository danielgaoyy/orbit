//! SQLite persistence for Bidirectional IM Channels (`im_channels` and `im_channel_bindings`).
//!
//! Tables and indexes are created idempotently with `CREATE TABLE IF NOT EXISTS` /
//! `CREATE INDEX IF NOT EXISTS` without bumping `PRAGMA user_version` (keeps `user_version = 6`).

use orbit_protocol::{
    ImChannelRecord, ImConnectionStatus, ImPlatform, ImRoutingMode, ImThreadBinding,
    ImTransportMode, ProviderKind, RouterImMirrorPolicy, SessionMode, mask_im_secret,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{OrbitStore, StoreError};

/// Full daemon-internal IM channel record (includes stored secrets; NEVER send over wire directly).
#[derive(Clone, PartialEq, Eq)]
pub struct StoredImChannel {
    pub channel_id: String,
    pub name: String,
    pub platform: ImPlatform,
    pub enabled: bool,
    pub transport_mode: ImTransportMode,
    pub status: ImConnectionStatus,
    pub status_message: Option<String>,
    pub webhook_path: String,
    pub app_id: Option<String>,
    pub primary_secret: Option<String>,
    pub signing_secret: Option<String>,
    pub app_token: Option<String>,
    pub public_key: Option<String>,
    pub allowed_chat_ids: Vec<String>,
    pub allowed_user_ids: Vec<String>,
    pub default_workspace_cwd: String,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    pub forward_approvals: bool,
    pub routing_mode: ImRoutingMode,
    pub router_mirror_policy: RouterImMirrorPolicy,
    pub message_count: u64,
    pub last_activity_at_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl std::fmt::Debug for StoredImChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredImChannel")
            .field("channel_id", &self.channel_id)
            .field("name", &self.name)
            .field("platform", &self.platform)
            .field("enabled", &self.enabled)
            .field("transport_mode", &self.transport_mode)
            .field("status", &self.status)
            .field("status_message", &self.status_message)
            .field("webhook_path", &self.webhook_path)
            .field("app_id", &self.app_id)
            .field(
                "primary_secret",
                &self.primary_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "signing_secret",
                &self.signing_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("app_token", &self.app_token.as_ref().map(|_| "[REDACTED]"))
            .field("public_key", &self.public_key)
            .field("allowed_chat_ids", &self.allowed_chat_ids)
            .field("allowed_user_ids", &self.allowed_user_ids)
            .field("default_workspace_cwd", &self.default_workspace_cwd)
            .field("default_provider", &self.default_provider)
            .field("default_mode", &self.default_mode)
            .field("forward_approvals", &self.forward_approvals)
            .field("routing_mode", &self.routing_mode)
            .field("router_mirror_policy", &self.router_mirror_policy)
            .field("message_count", &self.message_count)
            .field("last_activity_at_ms", &self.last_activity_at_ms)
            .field("created_at_ms", &self.created_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

impl StoredImChannel {
    /// Convert into a sanitized wire-safe record with all secrets masked (`••••`).
    pub fn to_public_record(&self) -> ImChannelRecord {
        let masked_primary = mask_im_secret(self.primary_secret.as_deref());
        let masked_signing = mask_im_secret(self.signing_secret.as_deref());
        let masked_app_token = mask_im_secret(self.app_token.as_deref());
        ImChannelRecord {
            channel_id: self.channel_id.clone(),
            name: self.name.clone(),
            platform: self.platform,
            enabled: self.enabled,
            transport_mode: self.transport_mode,
            status: self.status,
            status_message: self.status_message.clone(),
            webhook_path: self.webhook_path.clone(),
            app_id: self.app_id.clone(),
            has_primary_secret: masked_primary.is_some(),
            has_signing_secret: masked_signing.is_some(),
            has_app_token: masked_app_token.is_some(),
            masked_primary_secret: masked_primary,
            masked_signing_secret: masked_signing,
            masked_app_token,
            public_key: self.public_key.clone(),
            allowed_chat_ids: self.allowed_chat_ids.clone(),
            allowed_user_ids: self.allowed_user_ids.clone(),
            default_workspace_cwd: self.default_workspace_cwd.clone(),
            default_provider: self.default_provider,
            default_mode: self.default_mode.clone(),
            forward_approvals: self.forward_approvals,
            routing_mode: self.routing_mode,
            router_mirror_policy: self.router_mirror_policy,
            message_count: self.message_count,
            last_activity_at_ms: self.last_activity_at_ms,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Idempotently create `im_channels` and `im_channel_bindings` tables and indexes.
/// Intentionally does NOT alter `PRAGMA user_version` (remains `6`).
pub(crate) fn migrate_im_channels_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS im_channels (
            channel_id             TEXT PRIMARY KEY,
            name                   TEXT NOT NULL,
            platform               TEXT NOT NULL,
            enabled                INTEGER NOT NULL DEFAULT 1,
            transport_mode         TEXT NOT NULL DEFAULT 'long_connection',
            status                 TEXT NOT NULL DEFAULT 'disconnected',
            status_message         TEXT,
            webhook_path           TEXT NOT NULL,
            app_id                 TEXT,
            primary_secret         TEXT,
            signing_secret         TEXT,
            app_token              TEXT,
            public_key             TEXT,
            allowed_chat_ids_json  TEXT NOT NULL DEFAULT '[]',
            allowed_user_ids_json  TEXT NOT NULL DEFAULT '[]',
            default_workspace_cwd  TEXT NOT NULL,
            default_provider_json  TEXT NOT NULL,
            default_mode_json      TEXT NOT NULL,
            forward_approvals      INTEGER NOT NULL DEFAULT 1,
            routing_mode           TEXT NOT NULL DEFAULT 'per_thread_agent',
            router_mirror_policy   TEXT NOT NULL DEFAULT 'milestones_and_approvals',
            message_count          INTEGER NOT NULL DEFAULT 0,
            last_activity_at_ms    INTEGER,
            created_at_ms          INTEGER NOT NULL,
            updated_at_ms          INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS im_channel_bindings (
            channel_id          TEXT NOT NULL REFERENCES im_channels(channel_id) ON DELETE CASCADE,
            external_chat_id    TEXT NOT NULL,
            external_thread_id  TEXT NOT NULL,
            agent_id            TEXT NOT NULL,
            updated_at_ms       INTEGER NOT NULL,
            PRIMARY KEY (channel_id, external_chat_id, external_thread_id)
        );

        CREATE INDEX IF NOT EXISTS idx_im_bindings_agent
            ON im_channel_bindings(agent_id);
        CREATE INDEX IF NOT EXISTS idx_im_bindings_channel
            ON im_channel_bindings(channel_id, updated_at_ms DESC);
        ",
    )?;
    for (col, col_type) in [
        ("routing_mode", "TEXT NOT NULL DEFAULT 'per_thread_agent'"),
        (
            "router_mirror_policy",
            "TEXT NOT NULL DEFAULT 'milestones_and_approvals'",
        ),
        ("transport_mode", "TEXT NOT NULL DEFAULT 'long_connection'"),
        ("app_token", "TEXT"),
        ("public_key", "TEXT"),
    ] {
        crate::ensure_table_column(conn, "im_channels", col, col_type)?;
    }
    Ok(())
}

const IM_CHANNEL_SELECT_COLS: &str = "channel_id, name, platform, enabled, status, status_message, \
    webhook_path, app_id, primary_secret, signing_secret, allowed_chat_ids_json, \
    allowed_user_ids_json, default_workspace_cwd, default_provider_json, default_mode_json, \
    forward_approvals, message_count, last_activity_at_ms, created_at_ms, updated_at_ms, \
    routing_mode, router_mirror_policy, transport_mode, app_token, public_key";

fn map_im_channel_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredImChannel> {
    let channel_id: String = row.get(0)?;
    let name: String = row.get(1)?;
    let platform_raw: String = row.get(2)?;
    let enabled_raw: i64 = row.get(3)?;
    let status_raw: String = row.get(4)?;
    let status_message: Option<String> = row.get(5)?;
    let webhook_path: String = row.get(6)?;
    let app_id: Option<String> = row.get(7)?;
    let primary_secret: Option<String> = row.get(8)?;
    let signing_secret: Option<String> = row.get(9)?;
    let allowed_chat_ids_json: String = row.get(10)?;
    let allowed_user_ids_json: String = row.get(11)?;
    let default_workspace_cwd: String = row.get(12)?;
    let default_provider_json: String = row.get(13)?;
    let default_mode_json: String = row.get(14)?;
    let forward_approvals_raw: i64 = row.get(15)?;
    let message_count_raw: i64 = row.get(16)?;
    let last_activity_raw: Option<i64> = row.get(17)?;
    let created_at_raw: i64 = row.get(18)?;
    let updated_at_raw: i64 = row.get(19)?;
    let routing_mode_raw: String = row
        .get(20)
        .unwrap_or_else(|_| "per_thread_agent".to_string());
    let mirror_policy_raw: String = row
        .get(21)
        .unwrap_or_else(|_| "milestones_and_approvals".to_string());
    let transport_mode_raw: String = row
        .get(22)
        .unwrap_or_else(|_| "long_connection".to_string());
    let app_token: Option<String> = row.get(23).unwrap_or(None);
    let public_key: Option<String> = row.get(24).unwrap_or(None);

    let platform = ImPlatform::from_slug(&platform_raw).unwrap_or(ImPlatform::Lark);
    let status = ImConnectionStatus::from_slug(&status_raw);
    let transport_mode = ImTransportMode::from_slug(&transport_mode_raw);
    let routing_mode: ImRoutingMode =
        serde_json::from_str(&format!("\"{routing_mode_raw}\"")).unwrap_or_default();
    let router_mirror_policy: RouterImMirrorPolicy =
        serde_json::from_str(&format!("\"{mirror_policy_raw}\"")).unwrap_or_default();
    let allowed_chat_ids: Vec<String> =
        serde_json::from_str(&allowed_chat_ids_json).unwrap_or_default();
    let allowed_user_ids: Vec<String> =
        serde_json::from_str(&allowed_user_ids_json).unwrap_or_default();
    let default_provider: ProviderKind =
        serde_json::from_str(&default_provider_json).unwrap_or(ProviderKind::ClaudeCode);
    let default_mode: SessionMode =
        serde_json::from_str(&default_mode_json).unwrap_or(SessionMode::Default);

    Ok(StoredImChannel {
        channel_id,
        name,
        platform,
        enabled: enabled_raw != 0,
        transport_mode,
        status,
        status_message,
        webhook_path,
        app_id,
        primary_secret,
        signing_secret,
        app_token,
        public_key,
        allowed_chat_ids,
        allowed_user_ids,
        default_workspace_cwd,
        default_provider,
        default_mode,
        forward_approvals: forward_approvals_raw != 0,
        routing_mode,
        router_mirror_policy,
        message_count: u64::try_from(message_count_raw).unwrap_or(0),
        last_activity_at_ms: last_activity_raw.and_then(|v| u64::try_from(v).ok()),
        created_at_ms: u64::try_from(created_at_raw).unwrap_or(0),
        updated_at_ms: u64::try_from(updated_at_raw).unwrap_or(0),
    })
}

fn map_im_binding_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ImThreadBinding> {
    let channel_id: String = row.get(0)?;
    let external_chat_id: String = row.get(1)?;
    let external_thread_id: String = row.get(2)?;
    let agent_id: String = row.get(3)?;
    let updated_at_raw: i64 = row.get(4)?;
    Ok(ImThreadBinding {
        channel_id,
        external_chat_id,
        external_thread_id,
        agent_id,
        updated_at_ms: u64::try_from(updated_at_raw).unwrap_or(0),
    })
}

impl OrbitStore {
    pub fn list_im_channels(&self) -> Result<Vec<StoredImChannel>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {IM_CHANNEL_SELECT_COLS} FROM im_channels ORDER BY created_at_ms ASC, channel_id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_im_channel_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_im_channel(&self, channel_id: &str) -> Result<Option<StoredImChannel>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!("SELECT {IM_CHANNEL_SELECT_COLS} FROM im_channels WHERE channel_id = ?1");
        conn.query_row(&sql, params![channel_id], map_im_channel_row)
            .optional()
            .map_err(Into::into)
    }

    pub fn upsert_im_channel(&self, ch: &StoredImChannel) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let allowed_chats_json = serde_json::to_string(&ch.allowed_chat_ids)?;
        let allowed_users_json = serde_json::to_string(&ch.allowed_user_ids)?;
        let provider_json = serde_json::to_string(&ch.default_provider)?;
        let mode_json = serde_json::to_string(&ch.default_mode)?;
        let message_count_i64 = i64::try_from(ch.message_count).unwrap_or(i64::MAX);
        let last_activity_i64 = ch
            .last_activity_at_ms
            .map(|v| i64::try_from(v).unwrap_or(i64::MAX));
        let created_at_i64 = i64::try_from(ch.created_at_ms).unwrap_or(i64::MAX);
        let updated_at_i64 = i64::try_from(ch.updated_at_ms).unwrap_or(i64::MAX);

        let routing_mode_str = match ch.routing_mode {
            ImRoutingMode::PerThreadAgent => "per_thread_agent",
            ImRoutingMode::PersonalRouter => "personal_router",
        };
        let mirror_policy_str = match ch.router_mirror_policy {
            RouterImMirrorPolicy::ReplyOriginOnly => "reply_origin_only",
            RouterImMirrorPolicy::MilestonesAndApprovals => "milestones_and_approvals",
            RouterImMirrorPolicy::MirrorAll => "mirror_all",
        };

        conn.execute(
            "
            INSERT INTO im_channels (
                channel_id, name, platform, enabled, transport_mode, status, status_message,
                webhook_path, app_id, primary_secret, signing_secret, app_token, public_key,
                allowed_chat_ids_json, allowed_user_ids_json,
                default_workspace_cwd, default_provider_json, default_mode_json,
                forward_approvals, routing_mode, router_mirror_policy,
                message_count, last_activity_at_ms,
                created_at_ms, updated_at_ms
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7,
                ?8, ?9, ?10, ?11, ?12, ?13,
                ?14, ?15,
                ?16, ?17, ?18,
                ?19, ?20, ?21,
                ?22, ?23,
                ?24, ?25
            )
            ON CONFLICT(channel_id) DO UPDATE SET
                name = excluded.name,
                platform = excluded.platform,
                enabled = excluded.enabled,
                transport_mode = excluded.transport_mode,
                status = excluded.status,
                status_message = excluded.status_message,
                webhook_path = excluded.webhook_path,
                app_id = excluded.app_id,
                primary_secret = excluded.primary_secret,
                signing_secret = excluded.signing_secret,
                app_token = excluded.app_token,
                public_key = excluded.public_key,
                allowed_chat_ids_json = excluded.allowed_chat_ids_json,
                allowed_user_ids_json = excluded.allowed_user_ids_json,
                default_workspace_cwd = excluded.default_workspace_cwd,
                default_provider_json = excluded.default_provider_json,
                default_mode_json = excluded.default_mode_json,
                forward_approvals = excluded.forward_approvals,
                routing_mode = excluded.routing_mode,
                router_mirror_policy = excluded.router_mirror_policy,
                message_count = MAX(im_channels.message_count, excluded.message_count),
                last_activity_at_ms = COALESCE(excluded.last_activity_at_ms, im_channels.last_activity_at_ms),
                updated_at_ms = excluded.updated_at_ms
            ",
            params![
                ch.channel_id,
                ch.name,
                ch.platform.slug(),
                i32::from(ch.enabled),
                ch.transport_mode.slug(),
                ch.status.slug(),
                ch.status_message,
                ch.webhook_path,
                ch.app_id,
                ch.primary_secret,
                ch.signing_secret,
                ch.app_token,
                ch.public_key,
                allowed_chats_json,
                allowed_users_json,
                ch.default_workspace_cwd,
                provider_json,
                mode_json,
                i32::from(ch.forward_approvals),
                routing_mode_str,
                mirror_policy_str,
                message_count_i64,
                last_activity_i64,
                created_at_i64,
                updated_at_i64,
            ],
        )?;
        Ok(())
    }

    pub fn delete_im_channel(&self, channel_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM im_channels WHERE channel_id = ?1",
            params![channel_id],
        )?;
        Ok(affected > 0)
    }

    pub fn update_im_channel_status(
        &self,
        channel_id: &str,
        status: ImConnectionStatus,
        status_message: Option<&str>,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let updated_i64 = i64::try_from(updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            UPDATE im_channels
            SET status = ?2, status_message = ?3, updated_at_ms = ?4
            WHERE channel_id = ?1
            ",
            params![channel_id, status.slug(), status_message, updated_i64],
        )?;
        Ok(())
    }

    pub fn increment_im_channel_activity(
        &self,
        channel_id: &str,
        now_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let now_i64 = i64::try_from(now_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            UPDATE im_channels
            SET message_count = message_count + 1,
                last_activity_at_ms = ?2,
                updated_at_ms = ?2
            WHERE channel_id = ?1
            ",
            params![channel_id, now_i64],
        )?;
        Ok(())
    }

    pub fn list_im_bindings(
        &self,
        channel_id: Option<&str>,
    ) -> Result<Vec<ImThreadBinding>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut out = Vec::new();
        if let Some(cid) = channel_id {
            let mut stmt = conn.prepare(
                "
                SELECT channel_id, external_chat_id, external_thread_id, agent_id, updated_at_ms
                FROM im_channel_bindings
                WHERE channel_id = ?1
                ORDER BY updated_at_ms DESC
                ",
            )?;
            let rows = stmt.query_map(params![cid], map_im_binding_row)?;
            for row in rows {
                out.push(row?);
            }
        } else {
            let mut stmt = conn.prepare(
                "
                SELECT channel_id, external_chat_id, external_thread_id, agent_id, updated_at_ms
                FROM im_channel_bindings
                ORDER BY updated_at_ms DESC
                ",
            )?;
            let rows = stmt.query_map([], map_im_binding_row)?;
            for row in rows {
                out.push(row?);
            }
        }
        Ok(out)
    }

    pub fn find_im_binding(
        &self,
        channel_id: &str,
        external_chat_id: &str,
        external_thread_id: &str,
    ) -> Result<Option<ImThreadBinding>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "
            SELECT channel_id, external_chat_id, external_thread_id, agent_id, updated_at_ms
            FROM im_channel_bindings
            WHERE channel_id = ?1 AND external_chat_id = ?2 AND external_thread_id = ?3
            ",
            params![channel_id, external_chat_id, external_thread_id],
            map_im_binding_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn find_im_bindings_by_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<ImThreadBinding>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "
            SELECT channel_id, external_chat_id, external_thread_id, agent_id, updated_at_ms
            FROM im_channel_bindings
            WHERE agent_id = ?1
            ORDER BY updated_at_ms DESC
            ",
        )?;
        let rows = stmt.query_map(params![agent_id], map_im_binding_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn upsert_im_binding(&self, binding: &ImThreadBinding) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let updated_i64 = i64::try_from(binding.updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            INSERT INTO im_channel_bindings (
                channel_id, external_chat_id, external_thread_id, agent_id, updated_at_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(channel_id, external_chat_id, external_thread_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                updated_at_ms = excluded.updated_at_ms
            ",
            params![
                binding.channel_id,
                binding.external_chat_id,
                binding.external_thread_id,
                binding.agent_id,
                updated_i64,
            ],
        )?;
        Ok(())
    }

    pub fn delete_im_bindings_for_agent(&self, agent_id: &str) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM im_channel_bindings WHERE agent_id = ?1",
            params![agent_id],
        )?;
        Ok(affected)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn im_channels_schema_preserves_user_version_6_and_roundtrips() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let uv: i64 = store
            .conn
            .lock()
            .expect("lock")
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(uv, 6, "PRAGMA user_version must stay 6");

        let ch = StoredImChannel {
            channel_id: "im_lark_1".into(),
            name: "Engineering Feishu".into(),
            platform: ImPlatform::Lark,
            enabled: true,
            transport_mode: ImTransportMode::LongConnection,
            status: ImConnectionStatus::Connected,
            status_message: Some("Connected (WS)".into()),
            webhook_path: ImPlatform::Lark.webhook_path_for("im_lark_1"),
            app_id: Some("cli_a1b2c3d4".into()),
            primary_secret: Some("sec_super_secret_9988".into()),
            signing_secret: Some("vtok_1234567890".into()),
            app_token: Some("xapp-1-A12345-secret-4321".into()),
            public_key: Some("a".repeat(64)),
            allowed_chat_ids: vec!["oc_chat_1".into()],
            allowed_user_ids: vec!["ou_user_1".into()],
            default_workspace_cwd: "/workspace/orbit".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PersonalRouter,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            message_count: 0,
            last_activity_at_ms: None,
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_000_000,
        };
        store.upsert_im_channel(&ch).expect("upsert channel");
        let loaded = store
            .get_im_channel("im_lark_1")
            .expect("get channel")
            .expect("present");
        assert_eq!(loaded, ch);

        let public = loaded.to_public_record();
        assert_eq!(public.transport_mode, ImTransportMode::LongConnection);
        assert!(public.has_primary_secret);
        assert!(public.has_signing_secret);
        assert!(public.has_app_token);
        assert_eq!(public.masked_primary_secret.as_deref(), Some("••••9988"));
        assert_eq!(public.masked_signing_secret.as_deref(), Some("••••7890"));
        assert_eq!(public.masked_app_token.as_deref(), Some("••••4321"));
        assert_eq!(public.public_key.as_deref(), Some("a".repeat(64).as_str()));
        let serialized = serde_json::to_string(&public).expect("serialize public record");
        assert!(!serialized.contains("sec_super_secret_9988"));
        assert!(!serialized.contains("vtok_1234567890"));
        assert!(!serialized.contains("xapp-1-A12345-secret-4321"));

        let debug_str = format!("{loaded:?}");
        assert!(!debug_str.contains("sec_super_secret_9988"));
        assert!(!debug_str.contains("vtok_1234567890"));
        assert!(!debug_str.contains("xapp-1-A12345-secret-4321"));
        assert!(debug_str.contains("[REDACTED]"));

        let binding = ImThreadBinding {
            channel_id: "im_lark_1".into(),
            external_chat_id: "oc_chat_1".into(),
            external_thread_id: "om_root_1".into(),
            agent_id: "ag_123".into(),
            updated_at_ms: 1_760_000_001_000,
        };
        store.upsert_im_binding(&binding).expect("upsert binding");
        let found = store
            .find_im_binding("im_lark_1", "oc_chat_1", "om_root_1")
            .expect("find binding")
            .expect("present");
        assert_eq!(found, binding);
        assert_eq!(
            store
                .find_im_bindings_by_agent("ag_123")
                .expect("by agent")
                .len(),
            1
        );

        store
            .increment_im_channel_activity("im_lark_1", 1_760_000_002_000)
            .expect("inc activity");
        let after_inc = store
            .get_im_channel("im_lark_1")
            .expect("get")
            .expect("present");
        assert_eq!(after_inc.message_count, 1);
        assert_eq!(after_inc.last_activity_at_ms, Some(1_760_000_002_000));

        // Cascade delete channel -> bindings removed too
        assert!(store.delete_im_channel("im_lark_1").expect("delete"));
        assert!(store.list_im_bindings(None).expect("list").is_empty());
    }
}
