//! SQLite persistence for Native Mobile Background Push (`push_devices` and `push_preferences`).
//!
//! Tables are created idempotently with `CREATE TABLE IF NOT EXISTS` without bumping
//! `PRAGMA user_version` (strictly preserves `user_version = 6`).

use orbit_protocol::{PushDeviceRecord, PushPlatform, PushPreferences, mask_push_token};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{OrbitStore, StoreError};

/// Full daemon-internal registered mobile push device record (includes raw device token;
/// NEVER serialize or send directly over wire — call [`Self::to_public_record`]).
#[derive(Clone, PartialEq, Eq)]
pub struct StoredPushDevice {
    pub device_id: String,
    pub platform: PushPlatform,
    pub device_name: String,
    pub device_token: String,
    pub live_activity_token: Option<String>,
    pub sandbox: bool,
    pub enabled: bool,
    pub last_delivered_at_ms: Option<u64>,
    pub last_error: Option<String>,
    pub registered_at_ms: u64,
    pub updated_at_ms: u64,
}

impl std::fmt::Debug for StoredPushDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredPushDevice")
            .field("device_id", &self.device_id)
            .field("platform", &self.platform)
            .field("device_name", &self.device_name)
            .field("device_token", &"[REDACTED]")
            .field("sandbox", &self.sandbox)
            .field("enabled", &self.enabled)
            .field("last_delivered_at_ms", &self.last_delivered_at_ms)
            .field("last_error", &self.last_error)
            .field("registered_at_ms", &self.registered_at_ms)
            .field("updated_at_ms", &self.updated_at_ms)
            .finish()
    }
}

impl StoredPushDevice {
    /// Convert into a wire-safe snapshot record with `device_token` masked to `••••<last6>`.
    pub fn to_public_record(&self) -> PushDeviceRecord {
        PushDeviceRecord {
            device_id: self.device_id.clone(),
            platform: self.platform,
            device_name: self.device_name.clone(),
            masked_token: mask_push_token(&self.device_token),
            sandbox: self.sandbox,
            enabled: self.enabled,
            last_delivered_at_ms: self.last_delivered_at_ms,
            last_error: self.last_error.clone(),
            registered_at_ms: self.registered_at_ms,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

/// Full daemon-internal push preferences including optional gateway Bearer auth token.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct StoredPushPreferences {
    pub preferences: PushPreferences,
    pub gateway_auth_token: Option<String>,
}

impl std::fmt::Debug for StoredPushPreferences {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredPushPreferences")
            .field("preferences", &self.preferences)
            .field(
                "gateway_auth_token",
                &self.gateway_auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl StoredPushPreferences {
    /// Return wire-safe [`PushPreferences`] with `has_gateway_auth_token` accurately computed
    /// and zero secret material exposed.
    pub fn to_public_preferences(&self) -> PushPreferences {
        let mut prefs = self.preferences.clone();
        prefs.has_gateway_auth_token = self
            .gateway_auth_token
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty());
        prefs
    }
}

/// Idempotently create `push_devices` and `push_preferences` tables without modifying
/// `PRAGMA user_version` (remains `6`).
pub(crate) fn migrate_push_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS push_devices (
            device_id             TEXT PRIMARY KEY,
            platform              TEXT NOT NULL,
            device_name           TEXT NOT NULL,
            device_token          TEXT NOT NULL,
            sandbox               INTEGER NOT NULL DEFAULT 0,
            enabled               INTEGER NOT NULL DEFAULT 1,
            last_delivered_at_ms  INTEGER,
            last_error            TEXT,
            registered_at_ms      INTEGER NOT NULL,
            updated_at_ms         INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_push_devices_enabled
            ON push_devices(enabled, updated_at_ms DESC);

        CREATE TABLE IF NOT EXISTS push_preferences (
            id                          INTEGER PRIMARY KEY CHECK (id = 1),
            enabled                     INTEGER NOT NULL DEFAULT 1,
            notify_permission_required  INTEGER NOT NULL DEFAULT 1,
            notify_turn_completed       INTEGER NOT NULL DEFAULT 1,
            notify_agent_error          INTEGER NOT NULL DEFAULT 1,
            notify_goal_finished        INTEGER NOT NULL DEFAULT 1,
            notify_timer_fired          INTEGER NOT NULL DEFAULT 1,
            include_message_preview     INTEGER NOT NULL DEFAULT 0,
            quiet_hours_enabled         INTEGER NOT NULL DEFAULT 0,
            quiet_start_hour            INTEGER NOT NULL DEFAULT 23,
            quiet_end_hour              INTEGER NOT NULL DEFAULT 7,
            push_gateway_url            TEXT,
            gateway_auth_token          TEXT,
            updated_at_ms               INTEGER NOT NULL DEFAULT 0
        );
        ",
    )?;
    crate::ensure_table_column(conn, "push_devices", "live_activity_token", "TEXT")?;
    Ok(())
}

const PUSH_DEVICE_SELECT_COLS: &str = "device_id, platform, device_name, device_token, sandbox, \
    enabled, last_delivered_at_ms, last_error, registered_at_ms, updated_at_ms, live_activity_token";

fn map_push_device_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredPushDevice> {
    let device_id: String = row.get(0)?;
    let platform_raw: String = row.get(1)?;
    let device_name: String = row.get(2)?;
    let device_token: String = row.get(3)?;
    let sandbox_raw: i64 = row.get(4)?;
    let enabled_raw: i64 = row.get(5)?;
    let last_delivered_raw: Option<i64> = row.get(6)?;
    let last_error: Option<String> = row.get(7)?;
    let registered_raw: i64 = row.get(8)?;
    let updated_raw: i64 = row.get(9)?;

    let platform = PushPlatform::from_slug(&platform_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown push platform `{platform_raw}`"),
            )),
        )
    })?;
    Ok(StoredPushDevice {
        device_id,
        platform,
        device_name,
        device_token,
        live_activity_token: row.get(10)?,
        sandbox: sandbox_raw != 0,
        enabled: enabled_raw != 0,
        last_delivered_at_ms: last_delivered_raw.and_then(|v| u64::try_from(v).ok()),
        last_error,
        registered_at_ms: u64::try_from(registered_raw).unwrap_or(0),
        updated_at_ms: u64::try_from(updated_raw).unwrap_or(0),
    })
}

impl OrbitStore {
    pub fn list_push_devices(&self) -> Result<Vec<StoredPushDevice>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql = format!(
            "SELECT {PUSH_DEVICE_SELECT_COLS} FROM push_devices ORDER BY updated_at_ms DESC, device_id ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], map_push_device_row)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn get_push_device(&self, device_id: &str) -> Result<Option<StoredPushDevice>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let sql =
            format!("SELECT {PUSH_DEVICE_SELECT_COLS} FROM push_devices WHERE device_id = ?1");
        conn.query_row(&sql, params![device_id], map_push_device_row)
            .optional()
            .map_err(Into::into)
    }

    pub fn set_live_activity_token(
        &self,
        device_id: &str,
        token: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        if conn.execute(
            "UPDATE push_devices SET live_activity_token=?2 WHERE device_id=?1 AND platform='apns'",
            params![device_id, token],
        )? != 1
        {
            return Err(StoreError::InvalidArgument("unknown APNs device".into()));
        }
        Ok(())
    }

    pub fn count_push_devices(&self) -> Result<usize, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM push_devices", [], |row| row.get(0))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    pub fn upsert_push_device(&self, dev: &StoredPushDevice) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let last_delivered_i64 = dev
            .last_delivered_at_ms
            .map(|v| i64::try_from(v).unwrap_or(i64::MAX));
        let registered_i64 = i64::try_from(dev.registered_at_ms).unwrap_or(i64::MAX);
        let updated_i64 = i64::try_from(dev.updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            INSERT INTO push_devices (
                device_id, platform, device_name, device_token, sandbox, enabled,
                last_delivered_at_ms, last_error, registered_at_ms, updated_at_ms, live_activity_token
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(device_id) DO UPDATE SET
                platform = excluded.platform,
                device_name = excluded.device_name,
                device_token = excluded.device_token,
                live_activity_token = CASE WHEN push_devices.platform = excluded.platform THEN COALESCE(excluded.live_activity_token,push_devices.live_activity_token) ELSE excluded.live_activity_token END,
                sandbox = excluded.sandbox,
                enabled = excluded.enabled,
                last_delivered_at_ms = COALESCE(excluded.last_delivered_at_ms, push_devices.last_delivered_at_ms),
                last_error = excluded.last_error,
                updated_at_ms = excluded.updated_at_ms
            ",
            params![
                dev.device_id,
                dev.platform.slug(),
                dev.device_name,
                dev.device_token,
                i32::from(dev.sandbox),
                i32::from(dev.enabled),
                last_delivered_i64,
                dev.last_error,
                registered_i64,
                updated_i64,dev.live_activity_token,
            ],
        )?;
        Ok(())
    }

    pub fn delete_push_device(&self, device_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM push_devices WHERE device_id = ?1",
            params![device_id],
        )?;
        Ok(affected > 0)
    }

    pub fn record_push_delivery_result(
        &self,
        device_id: &str,
        delivered_at_ms: Option<u64>,
        error: Option<&str>,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let delivered_i64 = delivered_at_ms.map(|v| i64::try_from(v).unwrap_or(i64::MAX));
        let updated_i64 = i64::try_from(updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            UPDATE push_devices
            SET last_delivered_at_ms = COALESCE(?2, last_delivered_at_ms),
                last_error = ?3,
                updated_at_ms = ?4
            WHERE device_id = ?1
            ",
            params![device_id, delivered_i64, error, updated_i64],
        )?;
        Ok(())
    }

    pub fn disable_push_device_with_error(
        &self,
        device_id: &str,
        error: &str,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let updated_i64 = i64::try_from(updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            UPDATE push_devices
            SET enabled = 0,
                last_error = ?2,
                updated_at_ms = ?3
            WHERE device_id = ?1
            ",
            params![device_id, error, updated_i64],
        )?;
        Ok(())
    }
    pub fn disable_push_token_with_error(
        &self,
        device_id: &str,
        device_token: &str,
        error: &str,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let updated_i64 = i64::try_from(updated_at_ms).unwrap_or(i64::MAX);
        conn.execute(
            "
            UPDATE push_devices
            SET enabled = 0,
                last_error = ?2,
                updated_at_ms = ?3
            WHERE device_id = ?1 AND device_token=?4
            ",
            params![device_id, error, updated_i64, device_token],
        )?;
        Ok(())
    }
    pub fn clear_stale_activity_token(
        &self,
        device_id: &str,
        token: &str,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.execute("UPDATE push_devices SET live_activity_token=NULL WHERE device_id=?1 AND live_activity_token=?2",params![device_id,token])?;
        Ok(())
    }

    pub fn get_push_preferences(&self) -> Result<StoredPushPreferences, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let row = conn
            .query_row(
                "
                SELECT enabled, notify_permission_required, notify_turn_completed,
                       notify_agent_error, notify_goal_finished, notify_timer_fired,
                       include_message_preview, quiet_hours_enabled, quiet_start_hour,
                       quiet_end_hour, push_gateway_url, gateway_auth_token
                FROM push_preferences
                WHERE id = 1
                ",
                [],
                |row| {
                    let enabled: i64 = row.get(0)?;
                    let notify_permission_required: i64 = row.get(1)?;
                    let notify_turn_completed: i64 = row.get(2)?;
                    let notify_agent_error: i64 = row.get(3)?;
                    let notify_goal_finished: i64 = row.get(4)?;
                    let notify_timer_fired: i64 = row.get(5)?;
                    let include_message_preview: i64 = row.get(6)?;
                    let quiet_hours_enabled: i64 = row.get(7)?;
                    let quiet_start_hour: i64 = row.get(8)?;
                    let quiet_end_hour: i64 = row.get(9)?;
                    let push_gateway_url: Option<String> = row.get(10)?;
                    let gateway_auth_token: Option<String> = row.get(11)?;

                    let has_token = gateway_auth_token
                        .as_deref()
                        .is_some_and(|s| !s.trim().is_empty());

                    Ok(StoredPushPreferences {
                        preferences: PushPreferences {
                            enabled: enabled != 0,
                            notify_permission_required: notify_permission_required != 0,
                            notify_turn_completed: notify_turn_completed != 0,
                            notify_agent_error: notify_agent_error != 0,
                            notify_goal_finished: notify_goal_finished != 0,
                            notify_timer_fired: notify_timer_fired != 0,
                            include_message_preview: include_message_preview != 0,
                            quiet_hours_enabled: quiet_hours_enabled != 0,
                            quiet_start_hour: u8::try_from(quiet_start_hour.clamp(0, 23))
                                .unwrap_or(23),
                            quiet_end_hour: u8::try_from(quiet_end_hour.clamp(0, 23)).unwrap_or(7),
                            push_gateway_url,
                            has_gateway_auth_token: has_token,
                        },
                        gateway_auth_token,
                    })
                },
            )
            .optional()?;

        Ok(row.unwrap_or_default())
    }

    pub fn upsert_push_preferences(
        &self,
        stored: &StoredPushPreferences,
        updated_at_ms: u64,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let prefs = &stored.preferences;
        let updated_i64 = i64::try_from(updated_at_ms).unwrap_or(i64::MAX);
        let start_hour = i64::from(prefs.quiet_start_hour.min(23));
        let end_hour = i64::from(prefs.quiet_end_hour.min(23));
        conn.execute(
            "
            INSERT INTO push_preferences (
                id, enabled, notify_permission_required, notify_turn_completed,
                notify_agent_error, notify_goal_finished, notify_timer_fired,
                include_message_preview, quiet_hours_enabled, quiet_start_hour,
                quiet_end_hour, push_gateway_url, gateway_auth_token, updated_at_ms
            ) VALUES (
                1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13
            )
            ON CONFLICT(id) DO UPDATE SET
                enabled = excluded.enabled,
                notify_permission_required = excluded.notify_permission_required,
                notify_turn_completed = excluded.notify_turn_completed,
                notify_agent_error = excluded.notify_agent_error,
                notify_goal_finished = excluded.notify_goal_finished,
                notify_timer_fired = excluded.notify_timer_fired,
                include_message_preview = excluded.include_message_preview,
                quiet_hours_enabled = excluded.quiet_hours_enabled,
                quiet_start_hour = excluded.quiet_start_hour,
                quiet_end_hour = excluded.quiet_end_hour,
                push_gateway_url = excluded.push_gateway_url,
                gateway_auth_token = excluded.gateway_auth_token,
                updated_at_ms = excluded.updated_at_ms
            ",
            params![
                i32::from(prefs.enabled),
                i32::from(prefs.notify_permission_required),
                i32::from(prefs.notify_turn_completed),
                i32::from(prefs.notify_agent_error),
                i32::from(prefs.notify_goal_finished),
                i32::from(prefs.notify_timer_fired),
                i32::from(prefs.include_message_preview),
                i32::from(prefs.quiet_hours_enabled),
                start_hour,
                end_hour,
                prefs.push_gateway_url,
                stored.gateway_auth_token,
                updated_i64,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_schema_preserves_user_version_6_and_roundtrips_devices_and_prefs() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let uv: i64 = store
            .conn
            .lock()
            .expect("lock")
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(uv, 6, "PRAGMA user_version must stay 6");

        // Default preferences preserve zero-knowledge boundary.
        let default_prefs = store.get_push_preferences().expect("default prefs");
        assert!(default_prefs.preferences.enabled);
        assert!(!default_prefs.preferences.include_message_preview);
        assert!(!default_prefs.preferences.has_gateway_auth_token);

        // Register & mask device token.
        let dev = StoredPushDevice {
            live_activity_token: None,
            device_id: "ios_iphone_16".into(),
            platform: PushPlatform::Apns,
            device_name: "Jess iPhone 16 Pro".into(),
            device_token: "apns_secret_token_987654".into(),
            sandbox: true,
            enabled: true,
            last_delivered_at_ms: None,
            last_error: None,
            registered_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_000_000,
        };
        store.upsert_push_device(&dev).expect("upsert device");

        let loaded = store
            .get_push_device("ios_iphone_16")
            .expect("get device")
            .expect("present");
        assert_eq!(loaded, dev);

        // Debug redacts raw device_token.
        let dev_debug = format!("{loaded:?}");
        assert!(!dev_debug.contains("apns_secret_token_987654"));
        assert!(dev_debug.contains("[REDACTED]"));

        // Public record masks token to ••••<last6> and never leaks raw token.
        let pub_rec = loaded.to_public_record();
        assert_eq!(pub_rec.masked_token, "••••987654");
        let serialized = serde_json::to_string(&pub_rec).expect("serialize");
        assert!(!serialized.contains("apns_secret_token_987654"));

        // Delivery result updates cleanly.
        store
            .record_push_delivery_result(
                "ios_iphone_16",
                Some(1_760_000_005_000),
                None,
                1_760_000_005_000,
            )
            .expect("record delivery");
        let after_deliv = store
            .get_push_device("ios_iphone_16")
            .expect("get")
            .expect("present");
        assert_eq!(after_deliv.last_delivered_at_ms, Some(1_760_000_005_000));
        assert_eq!(after_deliv.last_error, None);

        // Persist push preferences with redacted gateway auth token.
        let prefs_to_save = StoredPushPreferences {
            preferences: PushPreferences {
                enabled: true,
                notify_permission_required: true,
                notify_turn_completed: false,
                notify_agent_error: true,
                notify_goal_finished: true,
                notify_timer_fired: false,
                include_message_preview: false,
                quiet_hours_enabled: true,
                quiet_start_hour: 23,
                quiet_end_hour: 7,
                push_gateway_url: Some("https://push.orbit.example/v1/dispatch".into()),
                has_gateway_auth_token: true,
            },
            gateway_auth_token: Some("orbit_push_bearer_secret_xyz".into()),
        };
        store
            .upsert_push_preferences(&prefs_to_save, 1_760_000_006_000)
            .expect("upsert prefs");

        let loaded_prefs = store.get_push_preferences().expect("get prefs");
        assert_eq!(loaded_prefs, prefs_to_save);
        let prefs_debug = format!("{loaded_prefs:?}");
        assert!(!prefs_debug.contains("orbit_push_bearer_secret_xyz"));
        assert!(prefs_debug.contains("[REDACTED]"));

        let public_prefs = loaded_prefs.to_public_preferences();
        assert!(public_prefs.has_gateway_auth_token);
        assert!(!public_prefs.include_message_preview);

        // Disable stale device persists enabled = 0 + last_error.
        store
            .disable_push_device_with_error(
                "ios_iphone_16",
                "Unregistered token",
                1_760_000_007_000,
            )
            .expect("disable device");
        let disabled = store
            .get_push_device("ios_iphone_16")
            .expect("get")
            .expect("present");
        assert!(!disabled.enabled);
        assert_eq!(disabled.last_error.as_deref(), Some("Unregistered token"));

        // Corrupt platform slug returns explicit FromSqlConversionFailure instead of silently defaulting.
        store
            .conn
            .lock()
            .expect("lock")
            .execute(
                "UPDATE push_devices SET platform = 'corrupt_platform' WHERE device_id = ?1",
                params!["ios_iphone_16"],
            )
            .expect("corrupt platform");
        assert!(
            store.get_push_device("ios_iphone_16").is_err(),
            "corrupt platform slug must return an error"
        );

        // Unregister removes device.
        assert!(store.delete_push_device("ios_iphone_16").expect("delete"));
        assert!(store.list_push_devices().expect("list").is_empty());
        assert_eq!(store.count_push_devices().expect("count"), 0);
    }
}
