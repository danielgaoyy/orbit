//! Compact storage for the nonce and ciphertext produced by Orbit's AES-GCM vault codec.
use crate::StoreError;
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use orbit_crypto::{
    CLOUD_VAULT_CIPHER, CLOUD_VAULT_ENVELOPE_FORMAT, CLOUD_VAULT_KDF, DerivedVaultKey,
    EncryptedVaultEnvelope, decrypt_vault_bytes, encrypt_vault_bytes,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const PREFIX: &str = "enc:v1:";
pub(super) const CLOUD_CONFIG: &str = "cloud_sync_config_v1";
const CHECK_KEY: &str = "client_master_key_check_v1";

#[derive(Default)]
pub(super) struct SecretEnvelope(pub Option<Zeroizing<[u8; 32]>>);

fn invalid() -> StoreError {
    StoreError::InvalidArgument("invalid client secret envelope or master key".into())
}

impl SecretEnvelope {
    fn key(&self, field: &str) -> Result<DerivedVaultKey, StoreError> {
        // The vault codec authenticates the salt as associated data. Bind each envelope
        // to its column and owner even when several rows share a hardware-supplied key.
        let context = Sha256::digest(format!("orbit-client-secret/v1/{field}"));
        DerivedVaultKey::from_raw(
            **self.0.as_ref().ok_or_else(invalid)?,
            URL_SAFE_NO_PAD.encode(&context[..16]),
            Default::default(),
        )
        .map_err(|_| invalid())
    }

    pub fn seal(&self, field: &str, value: &str) -> Result<String, StoreError> {
        if self.0.is_none() {
            return Ok(value.to_owned());
        }
        let envelope =
            encrypt_vault_bytes(&self.key(field)?, value.as_bytes(), 0).map_err(|_| invalid())?;
        let mut bytes = URL_SAFE_NO_PAD
            .decode(envelope.nonce_b64)
            .map_err(|_| invalid())?;
        bytes.extend(
            URL_SAFE_NO_PAD
                .decode(envelope.ciphertext_b64)
                .map_err(|_| invalid())?,
        );
        Ok(format!("{PREFIX}{}", STANDARD.encode(bytes)))
    }

    pub fn open(&self, field: &str, value: &str) -> Result<String, StoreError> {
        let Some(encoded) = value.strip_prefix(PREFIX) else {
            if self.0.is_some() {
                return Err(invalid());
            }
            return Ok(value.to_owned());
        };
        let bytes = STANDARD.decode(encoded).map_err(|_| invalid())?;
        if bytes.len() < 28 {
            return Err(invalid());
        }
        let key = self.key(field)?;
        let envelope = EncryptedVaultEnvelope {
            format: CLOUD_VAULT_ENVELOPE_FORMAT.into(),
            cipher: CLOUD_VAULT_CIPHER.into(),
            kdf: CLOUD_VAULT_KDF.into(),
            kdf_params: key.kdf_params,
            salt_b64: key.salt_b64.clone(),
            nonce_b64: URL_SAFE_NO_PAD.encode(&bytes[..12]),
            ciphertext_b64: URL_SAFE_NO_PAD.encode(&bytes[12..]),
            updated_at_ms: 0,
        };
        let plain = Zeroizing::new(decrypt_vault_bytes(&key, &envelope).map_err(|_| invalid())?);
        String::from_utf8(plain.to_vec()).map_err(|_| invalid())
    }

    pub fn cloud_fields(
        &self,
        raw: &str,
        transform: impl Fn(&Self, &str, &str) -> Result<String, StoreError>,
    ) -> Result<String, StoreError> {
        let mut json: serde_json::Value = serde_json::from_str(raw)?;
        for field in ["github_access_token", "cached_key_b64"] {
            if let Some(serde_json::Value::String(value)) = json.get_mut(field) {
                *value = transform(self, field, value)?;
            }
        }
        Ok(serde_json::to_string(&json)?)
    }

    fn migrate(&self, field: &str, raw: &str, enrolling: bool) -> Result<String, StoreError> {
        if !enrolling && raw.starts_with(PREFIX) {
            self.open(field, raw)?;
            Ok(raw.to_owned())
        } else {
            self.seal(field, raw)
        }
    }

    pub fn initialize(&self, conn: &mut Connection) -> Result<(), StoreError> {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let marker: Option<String> = tx
            .query_row(
                "SELECT value_json FROM client_kv WHERE key=?1",
                [CHECK_KEY],
                |r| r.get(0),
            )
            .optional()?;
        let enrolling = marker.is_none() && self.0.is_some();
        if let Some(marker) = marker {
            if !marker.starts_with(PREFIX) || self.open(CHECK_KEY, &marker)? != CHECK_KEY {
                return Err(invalid());
            }
        } else if self.0.is_some() {
            // Another process may still have the legacy database open. Enforce keyed
            // writes in SQLite itself so that handle cannot reintroduce plaintext.
            for event in ["INSERT", "UPDATE"] {
                for (table,condition) in [
                    ("client_hosts", "substr(NEW.pairing_secret,1,7) != 'enc:v1:'"),
                    ("client_pending_outbox","substr(NEW.payload_json,1,7) != 'enc:v1:'"),
                    ("client_kv", "NEW.key='cloud_sync_config_v1' AND (
                        COALESCE(substr(json_extract(NEW.value_json,'$.github_access_token'),1,7),'enc:v1:')!='enc:v1:' OR
                        COALESCE(substr(json_extract(NEW.value_json,'$.cached_key_b64'),1,7),'enc:v1:')!='enc:v1:')"),
                ] {
                    tx.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS require_client_key_{table}_{event}
                        BEFORE {event} ON {table} WHEN {condition}
                        BEGIN SELECT RAISE(ABORT,'client database requires encrypted secrets'); END;"))?;
                }
            }
            tx.execute(
                "INSERT INTO client_kv(key,value_json,updated_at_ms) VALUES(?1,?2,0)",
                params![CHECK_KEY, self.seal(CHECK_KEY, CHECK_KEY)?],
            )?;
        }
        if self.0.is_some() {
            // Preserve pre-existing `cloud_sync_dirty` state across secret re-encoding so
            // encrypting local rows at rest never triggers a spurious cloud sync upload.
            let prev_dirty: Option<(String, i64)> = tx
                .query_row(
                    "SELECT value_json, updated_at_ms FROM client_kv WHERE key='cloud_sync_dirty'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            {
                let query = "SELECT host_id,pairing_secret FROM client_hosts";
                let update = "UPDATE client_hosts SET pairing_secret=?2 WHERE host_id=?1";
                let mut stmt = tx.prepare(query)?;
                for row in
                    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                {
                    let (id, old) = row?;
                    let new = self.migrate(&format!("pairing_secret/{id}"), &old, enrolling)?;
                    if new != old {
                        tx.execute(update, params![id, new])?;
                    }
                }
            }
            let raw: Option<String> = tx
                .query_row(
                    "SELECT value_json FROM client_kv WHERE key=?1",
                    [CLOUD_CONFIG],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(raw) = raw {
                let updated =
                    self.cloud_fields(&raw, |s, field, value| s.migrate(field, value, enrolling))?;
                if updated != raw {
                    tx.execute(
                        "UPDATE client_kv SET value_json=?2 WHERE key=?1",
                        params![CLOUD_CONFIG, updated],
                    )?;
                }
            }
            let outbox_sql = if enrolling {
                "SELECT id,host_id,payload_json FROM client_pending_outbox"
            } else {
                "SELECT id,host_id,payload_json FROM client_pending_outbox WHERE substr(payload_json,1,7) != 'enc:v1:'"
            };
            let mut stmt = tx.prepare(outbox_sql)?;
            for row in stmt.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (id, host, raw) = row?;
                tx.execute(
                    "UPDATE client_pending_outbox SET payload_json=?2 WHERE id=?1",
                    params![id, self.seal(&format!("outbox/{host}"), &raw)?],
                )?;
            }
            match prev_dirty {
                Some((val, ts)) => {
                    tx.execute(
                        "INSERT INTO client_kv(key,value_json,updated_at_ms) VALUES('cloud_sync_dirty',?1,?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at_ms=excluded.updated_at_ms",
                        params![val, ts],
                    )?;
                }
                None => {
                    tx.execute("DELETE FROM client_kv WHERE key='cloud_sync_dirty'", [])?;
                }
            }
        }
        tx.commit()?;
        if enrolling {
            // Retire pre-migration WAL frames as soon as other readers release them.
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrbitStore, StoredHostProfile};

    #[test]
    fn concurrent_dirty_revisions_survive_a_completed_upload() -> Result<(), StoreError> {
        let store = OrbitStore::open_in_memory()?;
        assert_eq!(store.cloud_dirty_revision()?, None);
        store.set_client_kv("cloud_sync_prefs_v1", "{}", 123)?;
        let first = store.cloud_dirty_revision()?.ok_or_else(invalid)?;
        store.set_client_thread_pinned("host", "agent", true, 1, 124)?;
        let latest = store.cloud_dirty_revision()?.ok_or_else(invalid)?;
        assert!(latest > first);
        store.clear_cloud_dirty_revision(first)?;
        assert_eq!(store.cloud_dirty_revision()?, Some(latest));
        store.clear_cloud_dirty_revision(latest)?;
        assert_eq!(store.cloud_dirty_revision()?, None);
        Ok(())
    }
    #[test]
    fn legacy_secrets_migrate_together_and_reopen_with_the_same_key()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!("orbit-envelope-{}.db", uuid::Uuid::new_v4()));
        let host = StoredHostProfile {
            host_id: "host-a".into(),
            display_name: "mobile".into(),
            daemon_pubkey_b64: "public".into(),
            direct_endpoints: vec![],
            relay_url: None,
            pairing_secret: "pair-private".into(),
            meta: Default::default(),
        };
        let config = serde_json::json!({"github_access_token":"github-private","cached_key_b64":"vault-private","auto_sync_enabled":true});
        {
            let legacy = OrbitStore::open(&path)?;
            legacy.upsert_client_host(&host)?;
            legacy.set_client_kv(CLOUD_CONFIG, &config.to_string(), 123)?;
            legacy.enqueue_pending_outbox("host-a", "legacy-request")?;
        }
        let stale_handle = OrbitStore::open(&path)?;
        for _ in 0..2 {
            let store = OrbitStore::open_with_master_key(&path, Some([9; 32]))?;
            assert_eq!(
                store.list_client_hosts()?[0].pairing_secret,
                host.pairing_secret
            );
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    &store.get_client_kv(CLOUD_CONFIG)?.ok_or("missing config")?
                )?,
                config
            );
            assert_eq!(
                store.list_pending_outbox("host-a")?[0].payload_json,
                "legacy-request"
            );
            assert!(stale_handle.upsert_client_host(&host).is_err());
            assert!(
                stale_handle
                    .set_client_kv(CLOUD_CONFIG, &config.to_string(), 1)
                    .is_err()
            );
            store.upsert_client_host(&host)?;
            store.set_client_kv(CLOUD_CONFIG, &config.to_string(), 456)?;
            let raw = Connection::open(&path)?;
            let secret: String =
                raw.query_row("SELECT pairing_secret FROM client_hosts", [], |r| r.get(0))?;
            assert!(secret.starts_with(PREFIX));
            assert!(!secret.contains("pair-private"));
            let stored: String = raw.query_row(
                "SELECT value_json FROM client_kv WHERE key=?1",
                [CLOUD_CONFIG],
                |r| r.get(0),
            )?;
            let json: serde_json::Value = serde_json::from_str(&stored)?;
            for k in ["github_access_token", "cached_key_b64"] {
                assert!(json[k].as_str().is_some_and(|s| s.starts_with(PREFIX)));
            }
            assert!(!stored.contains("-private"));
            assert_eq!(
                raw.query_row("PRAGMA user_version", [], |r| r.get::<_, i32>(0))?,
                6
            );
        }
        assert!(OrbitStore::open_with_master_key(&path, Some([3; 32])).is_err());
        assert!(OrbitStore::open(&path).is_err());
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn durable_fifo_is_bounded_per_host_and_survives_restart()
    -> Result<(), Box<dyn std::error::Error>> {
        let path = std::env::temp_dir().join(format!("orbit-outbox-{}.db", uuid::Uuid::new_v4()));
        let store = OrbitStore::open(&path)?;
        for n in 0..100 {
            store.enqueue_pending_outbox("a", &n.to_string())?;
        }
        assert!(store.enqueue_pending_outbox("a", "100").is_err());
        let id = store.enqueue_pending_outbox("b", "pending")?;
        store.mark_outbox_attempt(id, "disconnected")?;
        drop(store);
        let store = OrbitStore::open(&path)?;
        let b = store.list_pending_outbox("b")?;
        assert_eq!(
            (b[0].id, b[0].attempts, b[0].last_error.as_deref()),
            (id, 1, Some("disconnected"))
        );
        let a = store.list_pending_outbox("a")?;
        assert_eq!(a.len(), 100);
        assert_eq!(a[0].payload_json, "0");
        assert_eq!(a[99].payload_json, "99");
        store.remove_pending_outbox(id)?;
        assert!(store.list_pending_outbox("b")?.is_empty());
        assert_eq!(store.prune_stale_outbox(i64::MAX)?, 0);
        drop(store);
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn keyed_open_rejects_plaintext_downgrade_and_migration_does_not_mark_cloud_dirty()
    -> Result<(), Box<dyn std::error::Error>> {
        let env = SecretEnvelope(Some(Zeroizing::new([9u8; 32])));
        assert!(
            env.open("pairing_secret/h1", "unencrypted-downgrade")
                .is_err()
        );

        // Legacy plaintext that happens to start with `enc:v1:` enrolls cleanly without aborting.
        let path =
            std::env::temp_dir().join(format!("orbit-enroll-edge-{}.db", uuid::Uuid::new_v4()));
        {
            let legacy = OrbitStore::open(&path)?;
            legacy.upsert_client_host(&StoredHostProfile {
                host_id: "h-edge".into(),
                display_name: "edge".into(),
                daemon_pubkey_b64: "pk".into(),
                direct_endpoints: vec![],
                relay_url: None,
                pairing_secret: "enc:v1:legacy-literal".into(),
                meta: Default::default(),
            })?;
            if let Some(rev) = legacy.cloud_dirty_revision()? {
                legacy.clear_cloud_dirty_revision(rev)?;
            }
            assert_eq!(legacy.cloud_dirty_revision()?, None);
        }
        let keyed = OrbitStore::open_with_master_key(&path, Some([7u8; 32]))?;
        // Migration ran before dirty triggers were installed, so cloud_sync_dirty stays clean.
        assert_eq!(keyed.cloud_dirty_revision()?, None);
        let hosts = keyed.list_client_hosts()?;
        assert_eq!(hosts[0].pairing_secret, "enc:v1:legacy-literal");
        drop(keyed);
        std::fs::remove_file(path)?;
        Ok(())
    }
}
