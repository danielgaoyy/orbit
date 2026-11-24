//! Rebuildable project-memory index. Portable Markdown IDs are scoped to a checkout,
//! including when two worktrees contain copies of the same document.

use std::collections::{BTreeMap, HashMap};
use std::sync::MutexGuard;

use chrono::Utc;
use orbit_protocol::{MemoryCategory, MemoryStatus, ProjectMemoryItem, ProjectMemorySearchHit};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{OrbitStore, StoreError};

const HASH_MODEL: &str = "orbit-feature-hash-v1";
const DIMS: usize = 256;
const BOUND_CONFIRMED: &str="workspace_root=?1 AND status='confirmed' AND EXISTS(
                SELECT 1 FROM json_each(file_paths) bound JOIN json_each(?2) changed ON bound.value=changed.value)";

pub(crate) fn migrate(conn: &Connection) -> Result<(), StoreError> {
    // Generated columns keep the portable DTO and its indexed fields in lockstep.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS project_memories (
            id TEXT PRIMARY KEY, document TEXT NOT NULL,
            workspace_root TEXT AS (json_extract(document,'$.workspace_root')),
            title TEXT AS (json_extract(document,'$.title')),
            content TEXT AS (json_extract(document,'$.content')),
            category TEXT AS (json_extract(document,'$.category')),
            status TEXT AS (json_extract(document,'$.status')),
            confidence REAL AS (json_extract(document,'$.confidence')),
            file_paths TEXT AS (json_extract(document,'$.file_paths')),
            tags TEXT AS (json_extract(document,'$.tags')),
            source_provider TEXT AS (json_extract(document,'$.source_provider')),
            source_agent_id TEXT AS (json_extract(document,'$.source_agent_id')),
            source_path TEXT AS (json_extract(document,'$.source_path')),
            created_at_ms INTEGER AS (json_extract(document,'$.created_at_ms')),
            updated_at_ms INTEGER AS (json_extract(document,'$.updated_at_ms')),
            access_count INTEGER NOT NULL, last_accessed_ms INTEGER NOT NULL,
            builtin_vector BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_project_memory_status ON project_memories(workspace_root,status,updated_at_ms DESC);
        CREATE INDEX IF NOT EXISTS idx_project_memory_recent ON project_memories(workspace_root,updated_at_ms DESC);
        CREATE VIRTUAL TABLE IF NOT EXISTS project_memories_fts USING fts5(
            id UNINDEXED, workspace_root UNINDEXED, title, content, tags, file_paths, tokenize='unicode61'
        );
        CREATE TABLE IF NOT EXISTS project_memory_vectors (
            memory_id TEXT PRIMARY KEY REFERENCES project_memories(id) ON DELETE CASCADE,
            model TEXT NOT NULL, dims INTEGER NOT NULL, vector BLOB NOT NULL, updated_at_ms INTEGER NOT NULL
        );
        CREATE TRIGGER IF NOT EXISTS project_memory_insert AFTER INSERT ON project_memories BEGIN
            INSERT INTO project_memories_fts(rowid,id,workspace_root,title,content,tags,file_paths)
            VALUES (new.rowid,new.id,new.workspace_root,new.title,new.content,new.tags,new.file_paths);
        END;
        CREATE TRIGGER IF NOT EXISTS project_memory_edit AFTER UPDATE OF document ON project_memories
        WHEN old.title != new.title OR old.content != new.content OR old.tags != new.tags OR old.file_paths != new.file_paths BEGIN
            DELETE FROM project_memories_fts WHERE rowid=old.rowid;
            INSERT INTO project_memories_fts(rowid,id,workspace_root,title,content,tags,file_paths)
            VALUES (new.rowid,new.id,new.workspace_root,new.title,new.content,new.tags,new.file_paths);
        END;
        CREATE TRIGGER IF NOT EXISTS project_memory_delete AFTER DELETE ON project_memories BEGIN
            DELETE FROM project_memories_fts WHERE rowid=old.rowid;
        END;
    ")?;
    Ok(())
}

pub fn embedding_text(item: &ProjectMemoryItem) -> String {
    format!("{}\n{}\n{}", item.title, item.content, item.tags.join(" "))
}

/// Signed feature hashing with log-scaled term frequencies. Character features also
/// work on unsegmented Chinese text and on fragments of identifiers or error codes.
pub fn builtin_embedding(text: &str) -> Vec<f32> {
    let mut features = BTreeMap::<(u8, String), usize>::new();
    let mut add = |kind, term: String| *features.entry((kind, term)).or_default() += 1;
    for word in text
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
    {
        add(0, word.to_owned());
        let chars: Vec<_> = word.chars().collect();
        for gram in chars.windows(3) {
            add(1, gram.iter().collect());
        }
        for gram in chars
            .windows(2)
            .filter(|g| g.iter().all(|c| crate::search::is_cjk_char(*c)))
        {
            add(2, gram.iter().collect());
        }
    }
    let mut vector = vec![0.0; DIMS];
    for ((kind, term), count) in features {
        let mut hash = 2166136261u32 ^ u32::from(kind);
        for byte in term.bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(16777619);
        }
        hash ^= hash >> 16;
        let weight = (1.0 + (count as f32).ln()) * if kind == 0 { 2.0 } else { 1.0 };
        vector[hash as usize % DIMS] += if hash & 0x80000000 == 0 {
            weight
        } else {
            -weight
        };
    }
    normalize(&mut vector);
    vector
}

/// Reject invalid endpoint responses before persisting or comparing them.
pub fn normalize(vector: &mut [f32]) -> bool {
    let norm = vector
        .iter()
        .map(|&v| f64::from(v).powi(2))
        .sum::<f64>()
        .sqrt();
    if !norm.is_finite() || norm == 0.0 {
        return false;
    }
    for v in vector {
        *v = (f64::from(*v) / norm) as f32;
    }
    true
}

fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn similarity(blob: &[u8], query: &[f32]) -> f32 {
    if blob.len() != query.len() * 4 {
        return 0.0;
    }
    blob.as_chunks::<4>()
        .0
        .iter()
        .zip(query)
        .map(|(b, q)| f32::from_le_bytes(*b) * q)
        .sum()
}

fn key(root: &str, id: &str) -> String {
    format!("{root}/{id}")
}

fn item_row(row: &Row<'_>) -> rusqlite::Result<ProjectMemoryItem> {
    let mut item: ProjectMemoryItem =
        serde_json::from_str(&row.get::<_, String>(0)?).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
    item.access_count = row.get(1)?;
    item.last_accessed_ms = row.get(2)?;
    Ok(item)
}

fn get(conn: &Connection, key: &str) -> rusqlite::Result<Option<ProjectMemoryItem>> {
    conn.query_row(
        "SELECT document,access_count,last_accessed_ms FROM project_memories WHERE id=?1",
        [key],
        item_row,
    )
    .optional()
}

pub fn normalize_rel_path(path: &str) -> Option<String> {
    let portable = path.trim().replace('\\', "/");
    if portable.is_empty()
        || portable.starts_with('/')
        || portable.contains('\0')
        || portable.as_bytes().get(1) == Some(&b':')
    {
        return None;
    }
    let parts: Vec<&str> = portable
        .split('/')
        .filter(|p| !matches!(*p, "" | "."))
        .collect();
    if parts.is_empty() || parts.contains(&"..") {
        return None;
    }
    Some(parts.join("/"))
}

fn match_query(query: &str) -> String {
    query
        .split(|c: char| c == '\0' || c.is_whitespace() || c.is_control())
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .take(32)
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn vector_similarity(
    row: &Row<'_>,
    hash: &[f32],
    dense: Option<(&str, &[f32])>,
) -> rusqlite::Result<f32> {
    Ok(match dense {
        Some((model, vector))
            if row.get_ref(2)?.as_str()? == model && row.get::<_, usize>(3)? == vector.len() =>
        {
            similarity(row.get_ref(4)?.as_blob()?, vector)
        }
        _ => similarity(row.get_ref(1)?.as_blob()?, hash),
    })
}

impl OrbitStore {
    fn memory_conn(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.conn.lock().map_err(|_| StoreError::LockPoisoned)
    }

    pub fn index_project_memory(&self, item: &ProjectMemoryItem) -> Result<(), StoreError> {
        let id = key(&item.workspace_root, &item.id);
        let hash = encode(&builtin_embedding(&embedding_text(item)));
        let mut conn = self.memory_conn()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let same_text = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_memories WHERE id=?1 AND title=?2 AND content=?3 AND tags=?4)",
            params![id, item.title, item.content, serde_json::to_string(&item.tags)?],
            |row| row.get::<_,bool>(0),
        )?;
        tx.execute(
            "INSERT INTO project_memories(id, document, access_count, last_accessed_ms, builtin_vector)
            VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET document=excluded.document,builtin_vector=excluded.builtin_vector",
            params![id, serde_json::to_string(item)?, item.access_count, item.last_accessed_ms, hash],
        )?;
        if !same_text {
            tx.execute("INSERT INTO project_memory_vectors VALUES (?1,?2,?3,?4,?5)
            ON CONFLICT(memory_id) DO UPDATE SET model=excluded.model,dims=excluded.dims,vector=excluded.vector,updated_at_ms=excluded.updated_at_ms",
            params![id, HASH_MODEL, DIMS, hash, item.updated_at_ms],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn has_memory_vector_model(
        &self,
        item: &ProjectMemoryItem,
        model: &str,
    ) -> Result<bool, StoreError> {
        Ok(self.memory_conn()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM project_memory_vectors WHERE memory_id=?1 AND model=?2)",
            params![key(&item.workspace_root, &item.id), model],
            |r| r.get(0),
        )?)
    }

    pub fn set_dense_memory_vector(
        &self,
        item: &ProjectMemoryItem,
        model: &str,
        vector: &[f32],
    ) -> Result<(), StoreError> {
        let mut normalized = vector.to_vec();
        if normalized.is_empty()
            || !normalized.iter().all(|v| v.is_finite())
            || !normalize(&mut normalized)
        {
            return Ok(());
        }
        // A slow endpoint must never overwrite the vector for a newer revision.
        self.memory_conn()?.execute(
            "UPDATE project_memory_vectors SET model=?2,dims=?3,vector=?4,updated_at_ms=?5
            WHERE memory_id=?1 AND EXISTS(SELECT 1 FROM project_memories WHERE id=?1 AND title=?6 AND content=?7 AND tags=?8)",
            params![
                key(&item.workspace_root, &item.id), model, normalized.len(), encode(&normalized), item.updated_at_ms,
                item.title, item.content, serde_json::to_string(&item.tags)?,
            ],
        )?;
        Ok(())
    }

    pub fn project_memory(
        &self,
        root: &str,
        id: &str,
    ) -> Result<Option<ProjectMemoryItem>, StoreError> {
        let conn = self.memory_conn()?;
        Ok(get(&conn, &key(root, id))?)
    }

    pub fn remove_project_memory(&self, root: &str, id: &str) -> Result<bool, StoreError> {
        Ok(self
            .memory_conn()?
            .execute("DELETE FROM project_memories WHERE id=?1", [key(root, id)])?
            != 0)
    }

    pub fn project_memory_ids(&self, root: &str) -> Result<Vec<String>, StoreError> {
        let conn = self.memory_conn()?;
        let mut stmt = conn.prepare(
            "SELECT json_extract(document,'$.id') FROM project_memories WHERE workspace_root=?1",
        )?;
        Ok(stmt
            .query_map([root], |row| row.get(0))?
            .collect::<Result<_, _>>()?)
    }

    pub fn list_project_memories(
        &self,
        root: &str,
        status: Option<MemoryStatus>,
        category: Option<MemoryCategory>,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ProjectMemoryItem>, StoreError> {
        let fts_query = match query.map(str::trim).filter(|q| !q.is_empty()) {
            Some(q) => {
                let matched = match_query(q);
                if matched.is_empty() {
                    return Ok(Vec::new());
                }
                matched
            }
            None => String::new(),
        };
        let conn = self.memory_conn()?;
        let mut stmt = conn.prepare("SELECT document,access_count,last_accessed_ms FROM project_memories
            WHERE workspace_root=?1 AND (?2 IS NULL OR status=json_extract(?2,'$')) AND (?3 IS NULL OR category=json_extract(?3,'$'))
            AND (?4='' OR rowid IN (SELECT rowid FROM project_memories_fts WHERE workspace_root=?1 AND project_memories_fts MATCH ?4))
            ORDER BY updated_at_ms DESC,id LIMIT ?5")?;
        Ok(stmt
            .query_map(
                params![
                    root,
                    status.map(|s| serde_json::to_string(&s)).transpose()?,
                    category.map(|c| serde_json::to_string(&c)).transpose()?,
                    fts_query,
                    limit
                ],
                item_row,
            )?
            .collect::<Result<_, _>>()?)
    }

    pub fn preferred_confirmed_memories(
        &self,
        root: &str,
        limit: usize,
    ) -> Result<Vec<ProjectMemoryItem>, StoreError> {
        let conn = self.memory_conn()?;
        let mut stmt = conn.prepare("SELECT document,access_count,last_accessed_ms FROM project_memories
            WHERE workspace_root=?1 AND status='confirmed' ORDER BY (category IN ('architecture','convention')) DESC,
            confidence DESC,updated_at_ms DESC,id LIMIT ?2")?;
        Ok(stmt
            .query_map(params![root, limit], item_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn near_duplicate_memory(
        &self,
        root: &str,
        item: &ProjectMemoryItem,
        dense: Option<(&str, &[f32])>,
    ) -> Result<Option<ProjectMemoryItem>, StoreError> {
        let query = builtin_embedding(&embedding_text(item));
        let conn = self.memory_conn()?;
        let mut stmt = conn.prepare(
            "SELECT m.id,m.builtin_vector,v.model,v.dims,v.vector FROM project_memories m
             JOIN project_memory_vectors v ON m.id=v.memory_id
             WHERE workspace_root=?1 AND source_path LIKE '.orbit/memory/%' ORDER BY m.id",
        )?;
        let mut rows = stmt.query([root])?;
        let mut best = None;
        let mut highest = 0.92;
        while let Some(row) = rows.next()? {
            let score = vector_similarity(row, &query, dense)?;
            if score > highest {
                highest = score;
                best = Some(row.get::<_, String>(0)?);
            }
        }
        Ok(match best {
            Some(id) => get(&conn, &id)?,
            None => None,
        })
    }

    pub fn recall_project_memories(
        &self,
        root: &str,
        query: &str,
        limit: usize,
        include_unconfirmed: bool,
        dense: Option<(&str, &[f32])>,
    ) -> Result<Vec<ProjectMemorySearchHit>, StoreError> {
        if limit == 0 || query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let hash = builtin_embedding(query);
        let mut conn = self.memory_conn()?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut ranks = HashMap::<String, (Option<usize>, Option<usize>)>::new();
        let candidates = limit.saturating_mul(4).clamp(64, 1024);
        let terms = match_query(query);
        if !terms.is_empty() {
            let mut stmt = tx.prepare("SELECT m.id FROM project_memories_fts f JOIN project_memories m ON m.rowid=f.rowid
                WHERE project_memories_fts MATCH ?1 AND f.workspace_root=?2 AND (?3 OR m.status='confirmed')
                ORDER BY bm25(project_memories_fts,0,0,3,1,2,2),m.id LIMIT ?4")?;
            for (i, id) in stmt
                .query_map(params![terms, root, include_unconfirmed, candidates], |r| {
                    r.get(0)
                })?
                .enumerate()
            {
                ranks.entry(id?).or_default().0 = Some(i + 1);
            }
        }
        let mut scores = Vec::new();
        {
            let mut stmt = tx.prepare("SELECT m.id,m.builtin_vector,v.model,v.dims,v.vector FROM project_memories m
                JOIN project_memory_vectors v ON m.id=v.memory_id WHERE m.workspace_root=?1 AND (?2 OR m.status='confirmed')")?;
            let mut rows = stmt.query(params![root, include_unconfirmed])?;
            while let Some(row) = rows.next()? {
                let score = vector_similarity(row, &hash, dense)?;
                if score > 0.15 {
                    scores.push((row.get::<_, String>(0)?, score));
                }
            }
        }
        scores.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (i, (id, _)) in scores.into_iter().take(candidates).enumerate() {
            ranks.entry(id).or_default().1 = Some(i + 1);
        }

        let now = Utc::now().timestamp_millis();
        let rrf = |rank: Option<usize>| rank.map_or(0.0, |r| 1.0 / (60.0 + r as f64));
        let age_days = |t| now.saturating_sub(t).max(0) as f64 / 86_400_000.0;
        let mut ranked = Vec::with_capacity(ranks.len());
        {
            let mut metadata = tx.prepare("SELECT status='confirmed',updated_at_ms,access_count,last_accessed_ms FROM project_memories WHERE id=?1")?;
            for (id, (fts, vector)) in ranks {
                let (confirmed, updated, accesses, last) = metadata.query_row([&id], |row| {
                    Ok((
                        row.get::<_, bool>(0)?,
                        row.get(1)?,
                        row.get::<_, u64>(2)?,
                        row.get(3)?,
                    ))
                })?;
                let boost = (if confirmed { 1.04 } else { 1.0 })
                    + 0.015 / (1.0 + age_days(updated) / 30.0)
                    + 0.002 * (accesses as f64).ln_1p().min(5.0) / (1.0 + age_days(last) / 7.0);
                ranked.push((id, (rrf(fts) + rrf(vector)) * boost, fts, vector));
            }
        }
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut hits = Vec::with_capacity(limit.min(ranked.len()));
        // Only load Markdown bodies for the selected hits.
        {
            let mut touch = tx.prepare("UPDATE project_memories SET access_count=access_count+1,last_accessed_ms=?2 WHERE id=?1
                RETURNING document,access_count,last_accessed_ms")?;
            for (id, score, fts_rank, vector_rank) in ranked.into_iter().take(limit) {
                let item = touch.query_row(params![id, now], item_row)?;
                hits.push(ProjectMemorySearchHit {
                    item,
                    score,
                    fts_rank,
                    vector_rank,
                });
            }
        }
        tx.commit()?;
        Ok(hits)
    }

    pub fn confirmed_memories_for_paths(
        &self,
        root: &str,
        paths: &[String],
    ) -> Result<Vec<ProjectMemoryItem>, StoreError> {
        let normalized: Vec<String> = paths.iter().filter_map(|p| normalize_rel_path(p)).collect();
        if normalized.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.memory_conn()?;
        let mut stmt = conn.prepare(&format!("SELECT document,access_count,last_accessed_ms FROM project_memories WHERE {BOUND_CONFIRMED}"))?;
        Ok(stmt
            .query_map(params![root, serde_json::to_string(&normalized)?], item_row)?
            .collect::<Result<_, _>>()?)
    }

    pub fn mark_memories_needing_revalidation_for_paths(
        &self,
        root: &str,
        changed_rel_paths: &[String],
    ) -> Result<usize, StoreError> {
        self.mark_memories_needing_revalidation_for_paths_at(
            root,
            changed_rel_paths,
            Utc::now().timestamp_millis(),
        )
    }

    pub fn mark_memories_needing_revalidation_for_paths_at(
        &self,
        root: &str,
        changed_rel_paths: &[String],
        updated_at_ms: i64,
    ) -> Result<usize, StoreError> {
        let normalized: Vec<String> = changed_rel_paths
            .iter()
            .filter_map(|p| normalize_rel_path(p))
            .collect();
        if normalized.is_empty() {
            return Ok(0);
        }
        Ok(self.memory_conn()?.execute(&format!("UPDATE project_memories SET document=json_set(document,'$.status','needs_revalidation','$.updated_at_ms',?3)
            WHERE {BOUND_CONFIRMED}"),
            params![root, serde_json::to_string(&normalized)?, updated_at_ms])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(root: &str, id: &str, title: &str, content: &str) -> ProjectMemoryItem {
        ProjectMemoryItem {
            id: id.into(),
            workspace_root: root.into(),
            title: title.into(),
            content: content.into(),
            category: MemoryCategory::Debugging,
            status: MemoryStatus::Confirmed,
            confidence: 1.0,
            file_paths: vec!["src/auth.rs".into()],
            tags: vec!["storage".into()],
            source_provider: None,
            source_agent_id: None,
            source_path: Some(format!(".orbit/memory/{id}.md")),
            created_at_ms: 10,
            updated_at_ms: 10,
            last_accessed_ms: 0,
            access_count: 0,
        }
    }

    #[test]
    fn bilingual_features_and_hybrid_symbol_recall() -> Result<(), StoreError> {
        let store = OrbitStore::open_in_memory()?;
        for (query, related, unrelated) in [
            ("连接超时", "数据库连接超时需要重试", "橘子和蓝莓水果"),
            (
                "authentication",
                "authenticate authentication tokens",
                "oranges blueberries fruit",
            ),
        ] {
            let q = builtin_embedding(query);
            let score = |s| similarity(&encode(&builtin_embedding(s)), &q);
            assert!(score(related) > score(unrelated) + 0.25);
            assert!((similarity(&encode(&q), &q) - 1.0).abs() < 0.0001);
        }
        let mut hit = item(
            "/one",
            "a",
            "E_AUTH_401",
            "Call verify_pairing_secret before accepting a connection.",
        );
        store.index_project_memory(&hit)?;
        let mut second = item("/one", "b", "Database retry", "数据库连接超时需要重试");
        second.status = MemoryStatus::Hypothesis;
        store.index_project_memory(&second)?;
        let mut other = hit.clone();
        other.workspace_root = "/two".into();
        store.index_project_memory(&other)?;
        for query in ["E_AUTH_401", "verify_pairing_secret"] {
            let hits = store.recall_project_memories("/one", query, 5, false, None)?;
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].fts_rank, Some(1));
            assert_eq!(hits[0].vector_rank, Some(1));
            assert!((2.0 / 61.0..2.2 / 61.0).contains(&hits[0].score));
        }
        let zh = store.recall_project_memories("/one", "连接超时", 5, true, None)?;
        assert_eq!((&zh[0].item.id, zh[0].vector_rank), (&second.id, Some(1)));
        assert_eq!(
            store.project_memory("/two", "a")?.map(|i| i.access_count),
            Some(0)
        );
        assert_eq!(
            store
                .list_project_memories(
                    "/one",
                    Some(MemoryStatus::Confirmed),
                    Some(MemoryCategory::Debugging),
                    Some("E_AUTH_401"),
                    10
                )?
                .len(),
            1
        );
        let mut similar = hit.clone();
        similar.content.push_str(" Retry.");
        assert_eq!(
            store
                .near_duplicate_memory("/one", &similar, None)?
                .map(|i| i.id),
            Some(hit.id.clone())
        );
        assert!(
            store
                .near_duplicate_memory("/other", &similar, None)?
                .is_none()
        );
        hit.title = "New title".into();
        hit.content = "A different body".into();
        store.index_project_memory(&hit)?;
        assert!(
            store
                .list_project_memories("/one", None, None, Some("E_AUTH_401"), 10)?
                .is_empty()
        );
        assert!(store.remove_project_memory("/one", "a")?);
        assert!(
            store
                .list_project_memories("/one", None, None, Some("different"), 10)?
                .is_empty()
        );
        let conn = store.memory_conn()?;
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM project_memory_vectors WHERE memory_id='/one/a'",
                [],
                |r| r.get::<_, usize>(0)
            )?,
            0
        );
        migrate(&conn)?;
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?,
            6
        );
        Ok(())
    }

    #[test]
    fn dense_model_fallback_and_revalidation_are_scoped() -> Result<(), StoreError> {
        let store = OrbitStore::open_in_memory()?;
        let first = item("/one", "a", "socket", "Keep this network connection alive");
        let second = item("/two", "a", "socket", "Keep this network connection alive");
        store.index_project_memory(&first)?;
        store.index_project_memory(&second)?;
        store.set_dense_memory_vector(&first, "local:model", &[1., 0.])?;
        let query = "毫不相关";
        let hits = store.recall_project_memories(
            "/one",
            query,
            5,
            false,
            Some(("local:model", &[1., 0.])),
        )?;
        assert_eq!(hits[0].vector_rank, Some(1));
        assert_eq!(hits[0].fts_rank, None);
        for dense in [
            None,
            Some(("other", &[0., 1.][..])),
            Some(("local:model", &[1.][..])),
        ] {
            let h = store.recall_project_memories("/one", "network connection", 5, false, dense)?;
            assert_eq!(h[0].vector_rank, Some(1));
        }
        assert_eq!(
            store.mark_memories_needing_revalidation_for_paths("/one", &["auth.rs".into()])?,
            0
        );
        for n in [1, 0] {
            assert_eq!(
                store.mark_memories_needing_revalidation_for_paths(
                    "/one",
                    &["src/auth.rs".into()]
                )?,
                n
            );
        }
        assert!(
            store
                .recall_project_memories("/one", "socket", 5, false, None)?
                .is_empty()
        );
        assert_eq!(
            store
                .recall_project_memories("/two", "socket", 5, false, None)?
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn fts5_sanitizes_nul_and_special_chars_and_normalizes_windows_revalidation_paths()
    -> Result<(), StoreError> {
        let store = OrbitStore::open_in_memory()?;
        let mem = item("/win", "m1", "Socket retry", "Keep connection alive");
        store.index_project_memory(&mem)?;

        // Embedded NUL and FTS5 operators must never error.
        let _ = store.recall_project_memories(
            "/win",
            "foo\0bar \"* NEAR NOT :-，。！？",
            5,
            false,
            None,
        )?;
        let _ = store.list_project_memories(
            "/win",
            None,
            None,
            Some("foo\0bar \"* NEAR NOT :-"),
            10,
        )?;

        // Punctuation-only list filter must return empty, not all rows.
        assert!(
            store
                .list_project_memories("/win", None, None, Some("!!! ---"), 10)?
                .is_empty()
        );

        // Windows-style relative paths normalize cleanly for revalidation.
        assert_eq!(
            store
                .mark_memories_needing_revalidation_for_paths("/win", &[r".\src\auth.rs".into()])?,
            1
        );
        assert!(
            store
                .recall_project_memories("/win", "Socket", 5, false, None)?
                .is_empty()
        );
        Ok(())
    }
}
