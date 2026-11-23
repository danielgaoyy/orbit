//! SQLite persistence for First-Class Artifacts & versioned revisions.
//!
//! Creates `artifacts` and `artifact_revisions` idempotently via `CREATE TABLE IF NOT EXISTS`
//! without bumping `PRAGMA user_version` (keeps `user_version = 6`).

use chrono::{DateTime, Utc};
use orbit_protocol::{
    ArtifactDetail, ArtifactKind, ArtifactRevisionContent, ArtifactRevisionMeta, ArtifactSummary,
};
use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use uuid::Uuid;

use crate::{OrbitStore, StoreError};

/// Maximum UTF-8 byte length permitted for a single Artifact content revision (2 MiB).
pub const MAX_ARTIFACT_CONTENT_BYTES: usize = 2 * 1024 * 1024;

/// Maximum number of historical revisions retained per Artifact (older revisions are pruned).
pub const MAX_ARTIFACT_REVISIONS: usize = 25;

/// Input payload for creating or updating a versioned Artifact.
#[derive(Debug, Clone)]
pub struct UpsertArtifactInput {
    pub artifact_id: Option<String>,
    pub workspace_id: String,
    pub agent_id: Option<String>,
    pub title: String,
    pub kind: ArtifactKind,
    pub language: Option<String>,
    pub mime_type: Option<String>,
    pub resource_uri: Option<String>,
    pub content: String,
    pub summary_note: Option<String>,
}

/// Idempotent schema bootstrap for `artifacts` and `artifact_revisions`.
///
/// Intentionally does NOT alter `PRAGMA user_version`.
pub(crate) fn migrate_artifacts_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS artifacts (
            artifact_id  TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL,
            agent_id     TEXT,
            title        TEXT NOT NULL,
            kind         TEXT NOT NULL,
            language     TEXT,
            mime_type    TEXT,
            resource_uri TEXT,
            version      INTEGER NOT NULL DEFAULT 1,
            byte_len     INTEGER NOT NULL DEFAULT 0,
            created_at   TEXT NOT NULL,
            updated_at   TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_artifacts_workspace_updated
            ON artifacts(workspace_id, updated_at DESC);

        CREATE INDEX IF NOT EXISTS idx_artifacts_agent_updated
            ON artifacts(agent_id, updated_at DESC);

        CREATE TABLE IF NOT EXISTS artifact_revisions (
            artifact_id  TEXT NOT NULL REFERENCES artifacts(artifact_id) ON DELETE CASCADE,
            version      INTEGER NOT NULL,
            content      TEXT NOT NULL,
            summary_note TEXT,
            byte_len     INTEGER NOT NULL DEFAULT 0,
            created_at   TEXT NOT NULL,
            PRIMARY KEY (artifact_id, version)
        );
        ",
    )?;
    Ok(())
}

fn parse_rfc3339_utc(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn map_artifact_summary_row(row: &Row<'_>) -> rusqlite::Result<ArtifactSummary> {
    let kind_raw: String = row.get(4)?;
    let kind = ArtifactKind::from_slug(&kind_raw).unwrap_or(ArtifactKind::Markdown);
    let version_i64: i64 = row.get(8)?;
    let byte_len_i64: i64 = row.get(9)?;
    let created_raw: String = row.get(10)?;
    let updated_raw: String = row.get(11)?;
    Ok(ArtifactSummary {
        artifact_id: row.get(0)?,
        workspace_id: row.get(1)?,
        agent_id: row.get(2)?,
        title: row.get(3)?,
        kind,
        language: row.get(5)?,
        mime_type: row.get(6)?,
        resource_uri: row.get(7)?,
        version: version_i64.max(1) as u32,
        byte_len: byte_len_i64.max(0) as usize,
        created_at: parse_rfc3339_utc(&created_raw),
        updated_at: parse_rfc3339_utc(&updated_raw),
    })
}

/// Maximum length permitted for caller-supplied Artifact IDs.
pub const MAX_ARTIFACT_ID_LEN: usize = 128;

fn normalize_opt_str(raw: Option<String>) -> Option<String> {
    raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// Validate a caller-supplied `artifact_id`: non-empty, `<= 128` ASCII chars, `[A-Za-z0-9_.-]` only.
pub fn validate_artifact_id(id: &str) -> Result<(), StoreError> {
    if id.is_empty() || id.len() > MAX_ARTIFACT_ID_LEN {
        return Err(StoreError::InvalidArgument(format!(
            "artifact_id must be 1..={MAX_ARTIFACT_ID_LEN} characters"
        )));
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
    {
        return Err(StoreError::InvalidArgument(
            "artifact_id may only contain ASCII letters, digits, '_', '.', or '-'".into(),
        ));
    }
    Ok(())
}

impl OrbitStore {
    /// Atomically create a new Artifact (v1) or append a new revision (`version + 1`),
    /// enforcing the 2 MiB size cap and pruning revisions beyond `MAX_ARTIFACT_REVISIONS`.
    pub fn upsert_artifact(
        &self,
        input: UpsertArtifactInput,
    ) -> Result<ArtifactSummary, StoreError> {
        let workspace_id = input.workspace_id.trim().to_string();
        if workspace_id.is_empty() {
            return Err(StoreError::InvalidArgument(
                "artifact workspace_id must not be empty".into(),
            ));
        }
        let title = input.title.trim().to_string();
        if title.is_empty() {
            return Err(StoreError::InvalidArgument(
                "artifact title must not be empty".into(),
            ));
        }
        let byte_len = input.content.len();
        if byte_len > MAX_ARTIFACT_CONTENT_BYTES {
            return Err(StoreError::InvalidArgument(format!(
                "artifact content ({byte_len} bytes) exceeds maximum limit of {} bytes (2 MiB)",
                MAX_ARTIFACT_CONTENT_BYTES
            )));
        }

        let requested_id = normalize_opt_str(input.artifact_id);
        if let Some(ref aid) = requested_id {
            validate_artifact_id(aid)?;
        }
        let agent_id = normalize_opt_str(input.agent_id);
        let language = normalize_opt_str(input.language);
        let mime_type = normalize_opt_str(input.mime_type)
            .or_else(|| Some(input.kind.default_mime_type().to_string()));
        let resource_uri = normalize_opt_str(input.resource_uri);
        let summary_note = normalize_opt_str(input.summary_note);
        let now = Utc::now();
        let now_str = now.to_rfc3339();

        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing: Option<(String, String, Option<String>, i64, String)> =
            if let Some(ref aid) = requested_id {
                tx.query_row(
                    "SELECT artifact_id, workspace_id, agent_id, version, created_at
                     FROM artifacts
                     WHERE artifact_id = ?1",
                    params![aid],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?
            } else {
                None
            };

        let (artifact_id, effective_agent_id, next_version, created_at) = match existing {
            Some((existing_id, existing_workspace_id, existing_agent, prev_ver, created_raw)) => {
                if existing_workspace_id != workspace_id {
                    return Err(StoreError::InvalidArgument(format!(
                        "artifact `{existing_id}` belongs to a different workspace"
                    )));
                }
                let prev_u32 = u32::try_from(prev_ver).map_err(|_| {
                    StoreError::InvalidArgument("stored artifact version out of range".into())
                })?;
                let next_ver = prev_u32.checked_add(1).ok_or_else(|| {
                    StoreError::InvalidArgument("artifact version counter overflow".into())
                })?;
                let merged_agent = agent_id.or(existing_agent);
                tx.execute(
                    "UPDATE artifacts
                     SET agent_id = ?1,
                         title = ?2,
                         kind = ?3,
                         language = ?4,
                         mime_type = ?5,
                         resource_uri = ?6,
                         version = ?7,
                         byte_len = ?8,
                         updated_at = ?9
                     WHERE artifact_id = ?10",
                    params![
                        merged_agent,
                        title,
                        input.kind.slug(),
                        language,
                        mime_type,
                        resource_uri,
                        i64::from(next_ver),
                        byte_len as i64,
                        now_str,
                        existing_id,
                    ],
                )?;
                (
                    existing_id,
                    merged_agent,
                    next_ver,
                    parse_rfc3339_utc(&created_raw),
                )
            }
            None => {
                let new_id = requested_id.unwrap_or_else(|| {
                    let simple = Uuid::new_v4().simple().to_string();
                    format!("art_{}", &simple[..12])
                });
                tx.execute(
                    "INSERT INTO artifacts (
                        artifact_id, workspace_id, agent_id, title, kind, language,
                        mime_type, resource_uri, version, byte_len, created_at, updated_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11)",
                    params![
                        new_id,
                        workspace_id,
                        agent_id,
                        title,
                        input.kind.slug(),
                        language,
                        mime_type,
                        resource_uri,
                        byte_len as i64,
                        now_str,
                        now_str,
                    ],
                )?;
                (new_id, agent_id, 1u32, now)
            }
        };

        tx.execute(
            "INSERT OR REPLACE INTO artifact_revisions (
                artifact_id, version, content, summary_note, byte_len, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                artifact_id,
                i64::from(next_version),
                input.content,
                summary_note,
                byte_len as i64,
                now_str,
            ],
        )?;

        // Automatically prune oldest revisions beyond MAX_ARTIFACT_REVISIONS (25)
        tx.execute(
            "DELETE FROM artifact_revisions
             WHERE artifact_id = ?1
               AND version NOT IN (
                   SELECT version FROM artifact_revisions
                   WHERE artifact_id = ?1
                   ORDER BY version DESC
                   LIMIT ?2
               )",
            params![artifact_id, MAX_ARTIFACT_REVISIONS as i64],
        )?;

        tx.commit()?;

        Ok(ArtifactSummary {
            artifact_id,
            workspace_id,
            agent_id: effective_agent_id,
            title,
            kind: input.kind,
            language,
            mime_type,
            resource_uri,
            version: next_version,
            byte_len,
            created_at,
            updated_at: now,
        })
    }

    /// List Artifact summaries optionally filtered by `workspace_id` and/or `agent_id`.
    pub fn list_artifacts(
        &self,
        workspace_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ArtifactSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let ws = workspace_id.map(str::trim).filter(|s| !s.is_empty());
        let ag = agent_id.map(str::trim).filter(|s| !s.is_empty());

        match (ws, ag) {
            (Some(w), Some(a)) => {
                let mut stmt = conn.prepare(
                    "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                            mime_type, resource_uri, version, byte_len, created_at, updated_at
                     FROM artifacts
                     WHERE workspace_id = ?1 AND agent_id = ?2
                     ORDER BY updated_at DESC, artifact_id ASC",
                )?;
                let rows = stmt.query_map(params![w, a], map_artifact_summary_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
            (Some(w), None) => {
                let mut stmt = conn.prepare(
                    "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                            mime_type, resource_uri, version, byte_len, created_at, updated_at
                     FROM artifacts
                     WHERE workspace_id = ?1
                     ORDER BY updated_at DESC, artifact_id ASC",
                )?;
                let rows = stmt.query_map(params![w], map_artifact_summary_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
            (None, Some(a)) => {
                let mut stmt = conn.prepare(
                    "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                            mime_type, resource_uri, version, byte_len, created_at, updated_at
                     FROM artifacts
                     WHERE agent_id = ?1
                     ORDER BY updated_at DESC, artifact_id ASC",
                )?;
                let rows = stmt.query_map(params![a], map_artifact_summary_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
            (None, None) => {
                let mut stmt = conn.prepare(
                    "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                            mime_type, resource_uri, version, byte_len, created_at, updated_at
                     FROM artifacts
                     ORDER BY updated_at DESC, artifact_id ASC",
                )?;
                let rows = stmt.query_map([], map_artifact_summary_row)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
            }
        }
    }

    /// Fetch full Artifact content (latest or a specific historical `version`) plus revision metadata.
    pub fn get_artifact(
        &self,
        artifact_id: &str,
        version: Option<u32>,
    ) -> Result<Option<ArtifactDetail>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let Some(mut summary) = conn
            .query_row(
                "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                        mime_type, resource_uri, version, byte_len, created_at, updated_at
                 FROM artifacts
                 WHERE artifact_id = ?1",
                params![artifact_id],
                map_artifact_summary_row,
            )
            .optional()?
        else {
            return Ok(None);
        };

        let revisions: Vec<ArtifactRevisionMeta> = {
            let mut stmt = conn.prepare(
                "SELECT version, created_at, summary_note, byte_len
                 FROM artifact_revisions
                 WHERE artifact_id = ?1
                 ORDER BY version DESC",
            )?;
            let rows = stmt.query_map(params![artifact_id], |r| {
                let ver: i64 = r.get(0)?;
                let created_raw: String = r.get(1)?;
                let summary_note: Option<String> = r.get(2)?;
                let byte_len: i64 = r.get(3)?;
                Ok(ArtifactRevisionMeta {
                    version: ver.max(1) as u32,
                    created_at: parse_rfc3339_utc(&created_raw),
                    summary_note,
                    byte_len: byte_len.max(0) as usize,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let target_ver = version.unwrap_or(summary.version);
        let content_row: Option<(String, i64)> = conn
            .query_row(
                "SELECT content, byte_len
                 FROM artifact_revisions
                 WHERE artifact_id = ?1 AND version = ?2",
                params![artifact_id, i64::from(target_ver)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;

        let Some((content, rev_byte_len)) = content_row else {
            return Err(StoreError::InvalidArgument(format!(
                "artifact `{artifact_id}` version {target_ver} not found"
            )));
        };

        summary.version = target_ver;
        summary.byte_len = rev_byte_len.max(0) as usize;

        Ok(Some(ArtifactDetail {
            summary,
            content,
            revisions,
        }))
    }

    /// Fetch raw content for a single historical revision.
    pub fn get_artifact_revision(
        &self,
        artifact_id: &str,
        version: u32,
    ) -> Result<Option<ArtifactRevisionContent>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT artifact_id, version, content, created_at
             FROM artifact_revisions
             WHERE artifact_id = ?1 AND version = ?2",
            params![artifact_id, i64::from(version)],
            |r| {
                let ver: i64 = r.get(1)?;
                let created_raw: String = r.get(3)?;
                Ok(ArtifactRevisionContent {
                    artifact_id: r.get(0)?,
                    version: ver.max(1) as u32,
                    content: r.get(2)?,
                    created_at: parse_rfc3339_utc(&created_raw),
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// Find the most recently updated Artifact matching a workspace and `resource_uri`.
    pub fn find_artifact_by_workspace_and_resource_uri(
        &self,
        workspace_id: &str,
        resource_uri: &str,
    ) -> Result<Option<ArtifactSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                    mime_type, resource_uri, version, byte_len, created_at, updated_at
             FROM artifacts
             WHERE workspace_id = ?1 AND resource_uri = ?2
             ORDER BY updated_at DESC
             LIMIT 1",
            params![workspace_id, resource_uri],
            map_artifact_summary_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Find the most recently updated Artifact matching a workspace, title, and kind.
    pub fn find_artifact_by_workspace_title_kind(
        &self,
        workspace_id: &str,
        title: &str,
        kind: ArtifactKind,
    ) -> Result<Option<ArtifactSummary>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        conn.query_row(
            "SELECT artifact_id, workspace_id, agent_id, title, kind, language,
                    mime_type, resource_uri, version, byte_len, created_at, updated_at
             FROM artifacts
             WHERE workspace_id = ?1 AND LOWER(title) = LOWER(?2) AND kind = ?3
             ORDER BY updated_at DESC
             LIMIT 1",
            params![workspace_id, title.trim(), kind.slug()],
            map_artifact_summary_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Delete an Artifact and cascade-delete all of its `artifact_revisions`.
    pub fn delete_artifact(&self, artifact_id: &str) -> Result<bool, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM artifacts WHERE artifact_id = ?1",
            params![artifact_id],
        )?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifacts_migration_preserves_user_version_6() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let conn = store.conn.lock().expect("lock");
        let user_ver: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(user_ver, 6);
    }

    #[test]
    fn artifacts_crud_filtering_and_cascade_delete() {
        let store = OrbitStore::open_in_memory().expect("open store");

        let created = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: None,
                workspace_id: "ws_alpha".into(),
                agent_id: Some("ag_1".into()),
                title: "Architecture Spec".into(),
                kind: ArtifactKind::Markdown,
                language: None,
                mime_type: None,
                resource_uri: None,
                content: "# Spec\nInitial draft".into(),
                summary_note: Some("Initial".into()),
            })
            .expect("create artifact");

        assert!(created.artifact_id.starts_with("art_"));
        assert_eq!(created.version, 1);
        assert_eq!(created.mime_type.as_deref(), Some("text/markdown"));

        let created_b = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: Some("art_custom_app".into()),
                workspace_id: "ws_beta".into(),
                agent_id: Some("ag_2".into()),
                title: "Metrics Widget".into(),
                kind: ArtifactKind::McpApp,
                language: Some("html".into()),
                mime_type: None,
                resource_uri: Some("ui://orbit/metrics".into()),
                content: "<div>chart</div>".into(),
                summary_note: None,
            })
            .expect("create mcp app");

        assert_eq!(created_b.artifact_id, "art_custom_app");
        assert_eq!(
            created_b.mime_type.as_deref(),
            Some("text/html;profile=mcp-app")
        );

        let all = store.list_artifacts(None, None).expect("list all");
        assert_eq!(all.len(), 2);

        let ws_alpha = store
            .list_artifacts(Some("ws_alpha"), None)
            .expect("list ws_alpha");
        assert_eq!(ws_alpha.len(), 1);
        assert_eq!(ws_alpha[0].artifact_id, created.artifact_id);

        let ag_2 = store.list_artifacts(None, Some("ag_2")).expect("list ag_2");
        assert_eq!(ag_2.len(), 1);
        assert_eq!(ag_2[0].artifact_id, "art_custom_app");

        // Update version 1 -> 2
        let updated = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: Some(created.artifact_id.clone()),
                workspace_id: "ws_alpha".into(),
                agent_id: None,
                title: "Architecture Spec v2".into(),
                kind: ArtifactKind::Markdown,
                language: None,
                mime_type: None,
                resource_uri: None,
                content: "# Spec\nUpdated body".into(),
                summary_note: Some("Add details".into()),
            })
            .expect("update artifact");
        assert_eq!(updated.version, 2);
        assert_eq!(updated.agent_id.as_deref(), Some("ag_1"));

        let detail_latest = store
            .get_artifact(&created.artifact_id, None)
            .expect("get latest")
            .expect("exists");
        assert_eq!(detail_latest.summary.version, 2);
        assert_eq!(detail_latest.content, "# Spec\nUpdated body");
        assert_eq!(detail_latest.revisions.len(), 2);
        assert_eq!(detail_latest.revisions[0].version, 2);
        assert_eq!(detail_latest.revisions[1].version, 1);

        let detail_v1 = store
            .get_artifact(&created.artifact_id, Some(1))
            .expect("get v1")
            .expect("exists");
        assert_eq!(detail_v1.summary.version, 1);
        assert_eq!(detail_v1.content, "# Spec\nInitial draft");

        let rev1 = store
            .get_artifact_revision(&created.artifact_id, 1)
            .expect("get rev1")
            .expect("rev1 exists");
        assert_eq!(rev1.content, "# Spec\nInitial draft");

        // Delete and confirm CASCADE wipes artifact_revisions
        assert!(store.delete_artifact(&created.artifact_id).expect("delete"));
        assert!(
            !store
                .delete_artifact(&created.artifact_id)
                .expect("delete again")
        );
        assert!(
            store
                .get_artifact(&created.artifact_id, None)
                .expect("get after delete")
                .is_none()
        );
        let conn = store.conn.lock().expect("lock");
        let remaining_revs: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM artifact_revisions WHERE artifact_id = ?1",
                params![created.artifact_id],
                |r| r.get(0),
            )
            .expect("count revs");
        assert_eq!(remaining_revs, 0);
    }

    #[test]
    fn artifacts_prune_revisions_beyond_25_and_enforce_2mib_cap() {
        let store = OrbitStore::open_in_memory().expect("open store");

        let mut id = None;
        for i in 1..=30u32 {
            let summary = store
                .upsert_artifact(UpsertArtifactInput {
                    artifact_id: id.clone(),
                    workspace_id: "ws_1".into(),
                    agent_id: Some("ag_1".into()),
                    title: "Iterative Prototype".into(),
                    kind: ArtifactKind::Html,
                    language: Some("html".into()),
                    mime_type: None,
                    resource_uri: None,
                    content: format!("<div>Revision {i}</div>"),
                    summary_note: Some(format!("rev {i}")),
                })
                .expect("upsert rev");
            assert_eq!(summary.version, i);
            id = Some(summary.artifact_id);
        }

        let aid = id.expect("artifact id");
        let detail = store
            .get_artifact(&aid, None)
            .expect("get latest")
            .expect("exists");
        assert_eq!(detail.summary.version, 30);
        assert_eq!(detail.revisions.len(), MAX_ARTIFACT_REVISIONS);
        assert_eq!(detail.revisions.first().map(|r| r.version), Some(30));
        assert_eq!(detail.revisions.last().map(|r| r.version), Some(6));

        // Pruned revision 5 returns an invalid argument error
        assert!(store.get_artifact(&aid, Some(5)).is_err());
        assert!(store.get_artifact(&aid, Some(6)).expect("v6").is_some());

        // Exact 2 MiB succeeds
        let at_cap = "a".repeat(MAX_ARTIFACT_CONTENT_BYTES);
        let ok_cap = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: None,
                workspace_id: "ws_1".into(),
                agent_id: None,
                title: "Exact 2MiB".into(),
                kind: ArtifactKind::Code,
                language: Some("text".into()),
                mime_type: None,
                resource_uri: None,
                content: at_cap,
                summary_note: None,
            })
            .expect("2 MiB payload allowed");
        assert_eq!(ok_cap.byte_len, MAX_ARTIFACT_CONTENT_BYTES);

        // 2 MiB + 1 byte is rejected
        let over_cap = "a".repeat(MAX_ARTIFACT_CONTENT_BYTES + 1);
        let err = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: None,
                workspace_id: "ws_1".into(),
                agent_id: None,
                title: "Over 2MiB".into(),
                kind: ArtifactKind::Code,
                language: None,
                mime_type: None,
                resource_uri: None,
                content: over_cap,
                summary_note: None,
            })
            .expect_err("oversized artifact must fail");
        assert!(matches!(err, StoreError::InvalidArgument(_)));
    }

    #[test]
    fn artifacts_reject_cross_workspace_overwrite_and_invalid_ids() {
        let store = OrbitStore::open_in_memory().expect("open store");

        let created = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: Some("art_shared_id".into()),
                workspace_id: "ws_owner".into(),
                agent_id: Some("ag_owner".into()),
                title: "Owner Artifact".into(),
                kind: ArtifactKind::Markdown,
                language: None,
                mime_type: None,
                resource_uri: None,
                content: "original content".into(),
                summary_note: None,
            })
            .expect("create owner artifact");
        assert_eq!(created.workspace_id, "ws_owner");

        // Another workspace attempting to overwrite `art_shared_id` must be rejected
        let hijack_err = store
            .upsert_artifact(UpsertArtifactInput {
                artifact_id: Some("art_shared_id".into()),
                workspace_id: "ws_attacker".into(),
                agent_id: Some("ag_attacker".into()),
                title: "Hijacked".into(),
                kind: ArtifactKind::Html,
                language: None,
                mime_type: None,
                resource_uri: None,
                content: "<script>evil()</script>".into(),
                summary_note: None,
            })
            .expect_err("cross-workspace hijack must fail");
        assert!(matches!(hijack_err, StoreError::InvalidArgument(_)));

        // Original artifact remains completely intact
        let intact = store
            .get_artifact("art_shared_id", None)
            .expect("get intact")
            .expect("exists");
        assert_eq!(intact.summary.workspace_id, "ws_owner");
        assert_eq!(intact.summary.version, 1);
        assert_eq!(intact.content, "original content");

        // Invalid artifact_id charset and excessive length are rejected
        for bad_id in [
            "../etc/passwd",
            "art with space",
            "art/slash",
            &"a".repeat(129),
        ] {
            let err = store
                .upsert_artifact(UpsertArtifactInput {
                    artifact_id: Some(bad_id.to_string()),
                    workspace_id: "ws_owner".into(),
                    agent_id: None,
                    title: "Bad ID".into(),
                    kind: ArtifactKind::Code,
                    language: None,
                    mime_type: None,
                    resource_uri: None,
                    content: "x".into(),
                    summary_note: None,
                })
                .expect_err("invalid artifact_id must fail");
            assert!(matches!(err, StoreError::InvalidArgument(_)));
        }
    }
}
