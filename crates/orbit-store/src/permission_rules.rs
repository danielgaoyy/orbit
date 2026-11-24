//! Idempotent SQLite persistence for the fine-grained permission rules engine.
//!
//! Intentionally does NOT alter `PRAGMA user_version` (preserves `user_version = 6`).

use std::collections::VecDeque;
use std::path::{Component, Path, PathBuf};

use chrono::{DateTime, Utc};
use orbit_protocol::{
    PERMISSION_RULE_MAX_NOTE_LEN, PermissionRuleDecision, PermissionRulePatternKind,
    PermissionRuleRecord, PermissionRuleScopeKind, PermissionRuleSource, PermissionRuleToolKind,
    ProviderKind, UpsertPermissionRuleInput,
};
use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{OrbitStore, StoreError};

/// Idempotently create the `permission_rules` table and indexes.
/// Intentionally does NOT bump `PRAGMA user_version`.
pub(crate) fn migrate_permission_rules_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS permission_rules (
            id              TEXT PRIMARY KEY,
            scope_kind      TEXT NOT NULL,
            project_cwd     TEXT,
            provider_json   TEXT,
            tool_kind       TEXT NOT NULL,
            pattern_kind    TEXT NOT NULL,
            pattern         TEXT NOT NULL,
            decision        TEXT NOT NULL,
            source          TEXT NOT NULL DEFAULT 'manual',
            note            TEXT,
            created_at      TEXT NOT NULL,
            last_matched_at TEXT,
            match_count     INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_permission_rules_scope_project
            ON permission_rules(scope_kind, project_cwd);
        CREATE INDEX IF NOT EXISTS idx_permission_rules_decision
            ON permission_rules(decision);
        ",
    )?;
    Ok(())
}

/// Maximum symlink expansions permitted when resolving a single path (prevents symlink loops).
const MAX_SYMLINK_EXPANSION_DEPTH: usize = 16;

/// Error returned when a path cannot be safely resolved (e.g. cyclic symlink loop or unreadable link).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolutionError {
    SymlinkLoop,
    UnreadableSymlink(String),
}

/// Canonicalize a project directory path while resolving symlinks (including dangling targets)
/// and lexically normalizing `.` / `..` segments.
pub fn canonicalize_project_cwd(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match resolve_path_preserving_symlinks(Path::new(trimmed)) {
        Ok(resolved) => path_to_normalized_string(&resolved),
        Err(_) => String::new(),
    }
}

/// Resolve a path component-by-component, iteratively expanding both live AND dangling symlinks
/// via `read_link` (bounded to 16 hops to prevent cycles) before applying `..` or appending
/// non-existent tail components, then canonicalizing the nearest existing ancestor.
pub fn resolve_path_preserving_symlinks(path: &Path) -> Result<PathBuf, PathResolutionError> {
    let mut pending: VecDeque<std::ffi::OsString> = VecDeque::new();
    let mut acc = PathBuf::new();

    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => {
                acc.push(prefix.as_os_str());
            }
            Component::RootDir => {
                acc.push(Component::RootDir.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => pending.push_back(std::ffi::OsString::from("..")),
            Component::Normal(seg) => pending.push_back(seg.to_os_string()),
        }
    }

    let mut symlink_hops = 0usize;
    while let Some(seg) = pending.pop_front() {
        if seg == "." {
            continue;
        }
        if seg == ".." {
            if !acc.as_os_str().is_empty()
                && let Ok(canon) = std::fs::canonicalize(&acc)
            {
                acc = canon;
            }
            if !acc.pop() && acc.as_os_str().is_empty() {
                acc.push("..");
            }
            continue;
        }

        acc.push(&seg);
        if let Ok(meta) = std::fs::symlink_metadata(&acc) {
            if meta.file_type().is_symlink() {
                symlink_hops += 1;
                if symlink_hops > MAX_SYMLINK_EXPANSION_DEPTH {
                    return Err(PathResolutionError::SymlinkLoop);
                }
                let link_target = std::fs::read_link(&acc)
                    .map_err(|e| PathResolutionError::UnreadableSymlink(e.to_string()))?;

                if link_target.is_absolute() {
                    acc.clear();
                    let mut target_segs = Vec::new();
                    for comp in link_target.components() {
                        match comp {
                            Component::Prefix(prefix) => acc.push(prefix.as_os_str()),
                            Component::RootDir => acc.push(Component::RootDir.as_os_str()),
                            Component::CurDir => {}
                            Component::ParentDir => {
                                target_segs.push(std::ffi::OsString::from(".."));
                            }
                            Component::Normal(s) => target_segs.push(s.to_os_string()),
                        }
                    }
                    for s in target_segs.into_iter().rev() {
                        pending.push_front(s);
                    }
                } else {
                    // Relative symlink target is resolved against the symlink's parent directory.
                    acc.pop();
                    let mut target_segs = Vec::new();
                    for comp in link_target.components() {
                        match comp {
                            Component::CurDir => {}
                            Component::ParentDir => {
                                target_segs.push(std::ffi::OsString::from(".."));
                            }
                            Component::Normal(s) => target_segs.push(s.to_os_string()),
                            Component::Prefix(_) | Component::RootDir => {}
                        }
                    }
                    for s in target_segs.into_iter().rev() {
                        pending.push_front(s);
                    }
                }
            } else if let Ok(canon) = std::fs::canonicalize(&acc) {
                acc = canon;
            }
        }
    }

    // Canonicalize the deepest existing ancestor (e.g. macOS `/var` -> `/private/var`) while
    // keeping non-existent tail segments intact.
    if !acc.as_os_str().is_empty() {
        acc = canonicalize_deepest_existing_ancestor(&acc);
    }
    Ok(acc)
}

fn canonicalize_deepest_existing_ancestor(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return canon;
    }
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    while let Some(name) = cur.file_name().map(ToOwned::to_owned) {
        if !cur.pop() {
            break;
        }
        tail.push(name);
        if cur.as_os_str().is_empty() {
            break;
        }
        if let Ok(canon) = std::fs::canonicalize(&cur) {
            let mut rebuilt = canon;
            for seg in tail.into_iter().rev() {
                rebuilt.push(seg);
            }
            return rebuilt;
        }
    }
    path.to_path_buf()
}

fn path_to_normalized_string(path: &Path) -> String {
    #[cfg(windows)]
    let raw = path.to_string_lossy().replace('\\', "/");
    #[cfg(not(windows))]
    let raw = path.to_string_lossy().into_owned();
    if raw.len() > 1 {
        raw.trim_end_matches('/').to_string()
    } else {
        raw
    }
}

fn clean_optional_note(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().count() <= PERMISSION_RULE_MAX_NOTE_LEN {
        Some(trimmed.to_string())
    } else {
        Some(trimmed.chars().take(PERMISSION_RULE_MAX_NOTE_LEN).collect())
    }
}

fn parse_rfc3339_utc(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn decode_rule_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<PermissionRuleRecord>> {
    let id: String = row.get(0)?;
    let scope_raw: String = row.get(1)?;
    let project_cwd: Option<String> = row.get(2)?;
    let provider_json: Option<String> = row.get(3)?;
    let tool_raw: String = row.get(4)?;
    let pattern_kind_raw: String = row.get(5)?;
    let pattern: String = row.get(6)?;
    let decision_raw: String = row.get(7)?;
    let source_raw: String = row.get(8)?;
    let note: Option<String> = row.get(9)?;
    let created_at_raw: String = row.get(10)?;
    let last_matched_raw: Option<String> = row.get(11)?;
    let match_count_raw: i64 = row.get(12)?;

    let Some(scope_kind) = PermissionRuleScopeKind::from_db_str(&scope_raw) else {
        return Ok(None);
    };
    let Some(tool_kind) = PermissionRuleToolKind::from_db_str(&tool_raw) else {
        return Ok(None);
    };
    let Some(pattern_kind) = PermissionRulePatternKind::from_db_str(&pattern_kind_raw) else {
        return Ok(None);
    };
    let Some(decision) = PermissionRuleDecision::from_db_str(&decision_raw) else {
        return Ok(None);
    };
    let source = PermissionRuleSource::from_db_str(&source_raw).unwrap_or_default();
    let provider = match provider_json {
        Some(ref raw) if !raw.trim().is_empty() => serde_json::from_str::<ProviderKind>(raw)
            .ok()
            .or_else(|| ProviderKind::from_slug(raw.trim().trim_matches('"'))),
        _ => None,
    };
    let created_at = parse_rfc3339_utc(&created_at_raw).unwrap_or_else(Utc::now);
    let last_matched_at = last_matched_raw.as_deref().and_then(parse_rfc3339_utc);

    Ok(Some(PermissionRuleRecord {
        id,
        scope_kind,
        project_cwd,
        provider,
        tool_kind,
        pattern_kind,
        pattern,
        decision,
        source,
        note,
        created_at,
        last_matched_at,
        match_count: u64::try_from(match_count_raw).unwrap_or(0),
    }))
}

impl OrbitStore {
    /// List persisted permission rules.
    ///
    /// When `project_cwd` is `Some`, returns all `Host`-scoped rules plus `Project`-scoped rules
    /// matching the canonicalized `project_cwd`. When `None`, returns all rules on this host.
    pub fn list_permission_rules(
        &self,
        project_cwd: Option<&str>,
    ) -> Result<Vec<PermissionRuleRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let filter_cwd = project_cwd
            .map(canonicalize_project_cwd)
            .filter(|s| !s.is_empty());

        let mut out = Vec::new();
        if let Some(canon_cwd) = filter_cwd {
            let mut stmt = conn.prepare(
                "
                SELECT
                    id, scope_kind, project_cwd, provider_json, tool_kind, pattern_kind,
                    pattern, decision, source, note, created_at, last_matched_at, match_count
                FROM permission_rules
                WHERE scope_kind = 'host' OR (scope_kind = 'project' AND project_cwd = ?1)
                ORDER BY
                    CASE decision WHEN 'deny' THEN 0 ELSE 1 END ASC,
                    CASE scope_kind WHEN 'project' THEN 0 ELSE 1 END ASC,
                    created_at DESC,
                    id ASC
                ",
            )?;
            let rows = stmt.query_map(params![canon_cwd], decode_rule_row)?;
            for row in rows {
                if let Some(rec) = row? {
                    out.push(rec);
                }
            }
        } else {
            let mut stmt = conn.prepare(
                "
                SELECT
                    id, scope_kind, project_cwd, provider_json, tool_kind, pattern_kind,
                    pattern, decision, source, note, created_at, last_matched_at, match_count
                FROM permission_rules
                ORDER BY
                    CASE decision WHEN 'deny' THEN 0 ELSE 1 END ASC,
                    CASE scope_kind WHEN 'project' THEN 0 ELSE 1 END ASC,
                    created_at DESC,
                    id ASC
                ",
            )?;
            let rows = stmt.query_map([], decode_rule_row)?;
            for row in rows {
                if let Some(rec) = row? {
                    out.push(rec);
                }
            }
        }
        Ok(out)
    }

    /// Fetch a single permission rule by its `id`.
    pub fn get_permission_rule(
        &self,
        rule_id: &str,
    ) -> Result<Option<PermissionRuleRecord>, StoreError> {
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare(
            "
            SELECT
                id, scope_kind, project_cwd, provider_json, tool_kind, pattern_kind,
                pattern, decision, source, note, created_at, last_matched_at, match_count
            FROM permission_rules
            WHERE id = ?1
            ",
        )?;
        let row = stmt
            .query_row(params![rule_id.trim()], decode_rule_row)
            .optional()?;
        Ok(row.flatten())
    }

    /// Create or update a permission rule from a validated [`UpsertPermissionRuleInput`].
    ///
    /// - Canonicalizes `project_cwd` when `scope_kind == Project` (and clears it for `Host`).
    /// - When `input.id` is `None`, deduplicates against an existing rule with identical
    ///   `(scope_kind, project_cwd, provider, tool_kind, pattern_kind, pattern, decision)` so
    ///   repeated `allow_always` grants update cleanly in place without creating duplicates.
    pub fn upsert_permission_rule(
        &self,
        input: &UpsertPermissionRuleInput,
    ) -> Result<PermissionRuleRecord, StoreError> {
        input
            .validate_basic()
            .map_err(StoreError::InvalidArgument)?;

        let normalized_cwd = match input.scope_kind {
            PermissionRuleScopeKind::Host => None,
            PermissionRuleScopeKind::Project => {
                let raw = input.project_cwd.as_deref().unwrap_or("");
                let canon = canonicalize_project_cwd(raw);
                if canon.is_empty() {
                    return Err(StoreError::InvalidArgument(
                        "project-scoped permission rule requires a valid project_cwd".into(),
                    ));
                }
                Some(canon)
            }
        };

        let pattern = input.pattern.trim().to_string();
        let note = clean_optional_note(input.note.as_deref());
        let provider_json = match input.provider {
            Some(p) => Some(serde_json::to_string(&p)?),
            None => None,
        };

        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;

        let explicit_id = input
            .id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        struct ExistingRuleMeta {
            id: String,
            created_at: String,
            last_matched_at: Option<String>,
            match_count: i64,
            note: Option<String>,
        }

        let existing: Option<ExistingRuleMeta> = if let Some(ref target_id) = explicit_id {
            conn.query_row(
                "
                SELECT id, created_at, last_matched_at, match_count, note
                FROM permission_rules
                WHERE id = ?1
                ",
                params![target_id],
                |r| {
                    Ok(ExistingRuleMeta {
                        id: r.get(0)?,
                        created_at: r.get(1)?,
                        last_matched_at: r.get(2)?,
                        match_count: r.get(3)?,
                        note: r.get(4)?,
                    })
                },
            )
            .optional()?
        } else {
            conn.query_row(
                "
                SELECT id, created_at, last_matched_at, match_count, note
                FROM permission_rules
                WHERE scope_kind = ?1
                  AND COALESCE(project_cwd, '') = COALESCE(?2, '')
                  AND COALESCE(provider_json, '') = COALESCE(?3, '')
                  AND tool_kind = ?4
                  AND pattern_kind = ?5
                  AND pattern = ?6
                  AND decision = ?7
                LIMIT 1
                ",
                params![
                    input.scope_kind.as_str(),
                    normalized_cwd,
                    provider_json,
                    input.tool_kind.as_str(),
                    input.pattern_kind.as_str(),
                    pattern,
                    input.decision.as_str(),
                ],
                |r| {
                    Ok(ExistingRuleMeta {
                        id: r.get(0)?,
                        created_at: r.get(1)?,
                        last_matched_at: r.get(2)?,
                        match_count: r.get(3)?,
                        note: r.get(4)?,
                    })
                },
            )
            .optional()?
        };

        let now = Utc::now();
        let (id, created_at, last_matched_at, match_count, merged_note) = match existing {
            Some(prev) => (
                prev.id,
                parse_rfc3339_utc(&prev.created_at).unwrap_or(now),
                prev.last_matched_at.as_deref().and_then(parse_rfc3339_utc),
                u64::try_from(prev.match_count).unwrap_or(0),
                if explicit_id.is_some() {
                    note
                } else {
                    note.or(prev.note)
                },
            ),
            None => (
                explicit_id.unwrap_or_else(|| {
                    format!("pr_{}", &Uuid::new_v4().simple().to_string()[..12])
                }),
                now,
                None,
                0,
                note,
            ),
        };

        let match_count_i64 = i64::try_from(match_count).unwrap_or(i64::MAX);
        conn.execute(
            "
            INSERT INTO permission_rules (
                id, scope_kind, project_cwd, provider_json, tool_kind, pattern_kind,
                pattern, decision, source, note, created_at, last_matched_at, match_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                scope_kind = excluded.scope_kind,
                project_cwd = excluded.project_cwd,
                provider_json = excluded.provider_json,
                tool_kind = excluded.tool_kind,
                pattern_kind = excluded.pattern_kind,
                pattern = excluded.pattern,
                decision = excluded.decision,
                source = excluded.source,
                note = excluded.note
            ",
            params![
                id,
                input.scope_kind.as_str(),
                normalized_cwd,
                provider_json,
                input.tool_kind.as_str(),
                input.pattern_kind.as_str(),
                pattern,
                input.decision.as_str(),
                input.source.as_str(),
                merged_note,
                created_at.to_rfc3339(),
                last_matched_at.map(|dt| dt.to_rfc3339()),
                match_count_i64,
            ],
        )?;

        Ok(PermissionRuleRecord {
            id,
            scope_kind: input.scope_kind,
            project_cwd: normalized_cwd,
            provider: input.provider,
            tool_kind: input.tool_kind,
            pattern_kind: input.pattern_kind,
            pattern,
            decision: input.decision,
            source: input.source,
            note: merged_note,
            created_at,
            last_matched_at,
            match_count,
        })
    }

    /// Delete a permission rule by `id`. Returns `true` if a row was removed.
    pub fn delete_permission_rule(&self, rule_id: &str) -> Result<bool, StoreError> {
        let trimmed = rule_id.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "DELETE FROM permission_rules WHERE id = ?1",
            params![trimmed],
        )?;
        Ok(affected > 0)
    }

    /// Increment `match_count` and update `last_matched_at` when a rule triggers.
    pub fn record_permission_rule_match(
        &self,
        rule_id: &str,
        matched_at: DateTime<Utc>,
    ) -> Result<bool, StoreError> {
        let trimmed = rule_id.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let affected = conn.execute(
            "
            UPDATE permission_rules
            SET match_count = match_count + 1,
                last_matched_at = ?2
            WHERE id = ?1
            ",
            params![trimmed, matched_at.to_rfc3339()],
        )?;
        Ok(affected > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_rules_preserve_user_version_6_and_crud_roundtrip() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let uv: i64 = store
            .conn
            .lock()
            .expect("lock")
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("query user_version");
        assert_eq!(uv, 6);

        let created = store
            .upsert_permission_rule(&UpsertPermissionRuleInput {
                id: None,
                scope_kind: PermissionRuleScopeKind::Project,
                project_cwd: Some("/workspace/repo/sub/..".into()),
                provider: None,
                tool_kind: PermissionRuleToolKind::Shell,
                pattern_kind: PermissionRulePatternKind::CommandPrefix,
                pattern: "cargo test".into(),
                decision: PermissionRuleDecision::Allow,
                source: PermissionRuleSource::AlwaysAllowGrant,
                note: Some("From Codex".into()),
            })
            .expect("upsert project rule");
        assert_eq!(created.project_cwd.as_deref(), Some("/workspace/repo"));
        assert_eq!(created.match_count, 0);
        assert!(created.last_matched_at.is_none());

        // Deduplicates repeated AlwaysAllowGrant with identical signature
        let deduped = store
            .upsert_permission_rule(&UpsertPermissionRuleInput {
                id: None,
                scope_kind: PermissionRuleScopeKind::Project,
                project_cwd: Some("/workspace/repo/".into()),
                provider: None,
                tool_kind: PermissionRuleToolKind::Shell,
                pattern_kind: PermissionRulePatternKind::CommandPrefix,
                pattern: "cargo test".into(),
                decision: PermissionRuleDecision::Allow,
                source: PermissionRuleSource::AlwaysAllowGrant,
                note: None,
            })
            .expect("dedup upsert");
        assert_eq!(deduped.id, created.id);

        // Host-scoped deny rule
        let host_deny = store
            .upsert_permission_rule(&UpsertPermissionRuleInput {
                id: None,
                scope_kind: PermissionRuleScopeKind::Host,
                project_cwd: Some("/ignored/for/host".into()),
                provider: Some(ProviderKind::ClaudeCode),
                tool_kind: PermissionRuleToolKind::Shell,
                pattern_kind: PermissionRulePatternKind::CommandPrefix,
                pattern: "rm -rf".into(),
                decision: PermissionRuleDecision::Deny,
                source: PermissionRuleSource::Manual,
                note: Some("Block destructive rm".into()),
            })
            .expect("upsert host deny");
        assert!(host_deny.project_cwd.is_none());

        // Record match count & timestamp
        let matched_at = Utc::now();
        assert!(
            store
                .record_permission_rule_match(&created.id, matched_at)
                .expect("record match")
        );
        let fetched = store
            .get_permission_rule(&created.id)
            .expect("get rule")
            .expect("rule present");
        assert_eq!(fetched.match_count, 1);
        assert!(fetched.last_matched_at.is_some());

        // Project-filtered listing includes Host rules + matching Project rules only
        let matched_list = store
            .list_permission_rules(Some("/workspace/repo"))
            .expect("list matching project");
        assert_eq!(matched_list.len(), 2);
        assert_eq!(matched_list[0].decision, PermissionRuleDecision::Deny);

        let other_list = store
            .list_permission_rules(Some("/workspace/other"))
            .expect("list other project");
        assert_eq!(other_list.len(), 1);
        assert_eq!(other_list[0].id, host_deny.id);

        // Delete
        assert!(store.delete_permission_rule(&created.id).expect("delete"));
        assert!(
            !store
                .delete_permission_rule(&created.id)
                .expect("delete again")
        );
        assert_eq!(
            store.list_permission_rules(None).expect("list all").len(),
            1
        );
    }

    #[cfg(unix)]
    #[test]
    fn resolve_path_preserving_symlinks_resolves_dangling_escape_and_rejects_cycles() {
        use std::os::unix::fs::symlink;

        let base =
            std::env::temp_dir().join(format!("orbit-store-symlink-{}", Uuid::new_v4().simple()));
        let repo = base.join("repo");
        let outside = base.join("outside");
        std::fs::create_dir_all(&repo).expect("mkdir repo");
        std::fs::create_dir_all(&outside).expect("mkdir outside");

        let canon_outside = std::fs::canonicalize(&outside).expect("canon outside");

        // Dangling symlink inside repo pointing to a non-existent file outside repo
        let dangling = repo.join("escape_link");
        symlink(outside.join("new_secret.env"), &dangling).expect("create dangling symlink");
        let resolved =
            resolve_path_preserving_symlinks(&dangling).expect("dangling link should resolve");
        assert_eq!(resolved, canon_outside.join("new_secret.env"));

        // Cyclic symlink pair must fail-closed with SymlinkLoop
        let cyc_a = repo.join("cyc_a");
        let cyc_b = repo.join("cyc_b");
        symlink(&cyc_b, &cyc_a).expect("symlink a->b");
        symlink(&cyc_a, &cyc_b).expect("symlink b->a");
        assert_eq!(
            resolve_path_preserving_symlinks(&cyc_a),
            Err(PathResolutionError::SymlinkLoop)
        );

        let _ = std::fs::remove_dir_all(&base);
    }
}
