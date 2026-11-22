//! Cross-conversation SQLite FTS5 full-text search index, safe query parser,
//! incremental non-blocking backfill, BM25 ranking, and highlighted snippet extraction
//! for both the local client replica (`client_timeline_fts`) and daemon store (`timeline_events_fts`).

use chrono::Utc;
use orbit_protocol::{TimelinePayload, ToolCallDetail};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{OrbitStore, StoreError};

/// Maximum UTF-8 byte length indexed per timeline event to prevent oversized pastes or logs
/// from bloating the FTS5 inverted index.
const MAX_INDEXED_BODY_BYTES: usize = 16_384;

/// Maximum UTF-8 byte length indexed from a shell tool's output preview.
const MAX_TOOL_OUTPUT_INDEX_BYTES: usize = 2_048;

/// Maximum number of clauses emitted into a single FTS5 MATCH expression.
const MAX_FTS_QUERY_TERMS: usize = 16;

/// Bounded initial backfill performed synchronously during schema v6 upgrade so small/medium
/// databases are immediately searchable while huge databases never block app startup.
const INITIAL_MIGRATION_BACKFILL_BATCH: usize = 500;

/// Internal highlight sentinels passed to SQLite `snippet()` before normalizing to `<mark>...</mark>`.
const SNIPPET_MARK_START: char = '\u{0001}';
const SNIPPET_MARK_END: char = '\u{0002}';

const KV_REPLICA_LAST_ROWID: &str = "fts_backfill_replica_last_rowid";
const KV_REPLICA_DONE: &str = "fts_backfill_replica_done";
const KV_DAEMON_LAST_ROWID: &str = "fts_backfill_daemon_last_rowid";
const KV_DAEMON_DONE: &str = "fts_backfill_daemon_done";

/// Ranked full-text search match returned from `OrbitStore::search_replica_messages` or
/// `OrbitStore::search_daemon_messages`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MessageSearchHit {
    pub host_id: String,
    pub agent_id: String,
    pub seq: u64,
    pub role: String,
    pub snippet: String,
    pub rank: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_rfc3339: Option<String>,
}

/// Incremental progress returned by batched FTS5 backfill calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FtsBackfillProgress {
    pub scanned: usize,
    pub indexed: usize,
    pub done: bool,
}

/// Idempotent SQLite v6 schema migration creating `client_timeline_fts` and `timeline_events_fts`
/// plus bounded initial backfill for pre-existing databases.
pub(crate) fn migrate_fts5_schema(conn: &mut Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS client_timeline_fts USING fts5(
            host_id UNINDEXED,
            agent_id UNINDEXED,
            seq UNINDEXED,
            role UNINDEXED,
            body,
            cjk_tokens,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS timeline_events_fts USING fts5(
            agent_id UNINDEXED,
            seq UNINDEXED,
            role UNINDEXED,
            body,
            cjk_tokens,
            tokenize='unicode61 remove_diacritics 2'
        );

        CREATE INDEX IF NOT EXISTS idx_replica_agents_agent_id
            ON client_replica_agents(agent_id);
        ",
    )?;

    let (need_replica_initial, need_daemon_initial) = {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let user_ver: i64 = tx
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap_or(0);
        if user_ver < 6 {
            let has_replica_rows: bool = tx
                .query_row("SELECT 1 FROM client_replica_events LIMIT 1", [], |_| {
                    Ok(true)
                })
                .optional()?
                .unwrap_or(false);
            if has_replica_rows {
                write_kv_if_absent(&tx, KvTable::Client, KV_REPLICA_LAST_ROWID, "0")?;
                write_kv_if_absent(&tx, KvTable::Client, KV_REPLICA_DONE, "false")?;
            } else {
                write_kv_if_absent(&tx, KvTable::Client, KV_REPLICA_DONE, "true")?;
            }

            let has_daemon_rows: bool = tx
                .query_row("SELECT 1 FROM timeline_events LIMIT 1", [], |_| Ok(true))
                .optional()?
                .unwrap_or(false);
            if has_daemon_rows {
                write_kv_if_absent(&tx, KvTable::Host, KV_DAEMON_LAST_ROWID, "0")?;
                write_kv_if_absent(&tx, KvTable::Host, KV_DAEMON_DONE, "false")?;
            } else {
                write_kv_if_absent(&tx, KvTable::Host, KV_DAEMON_DONE, "true")?;
            }

            tx.execute_batch("PRAGMA user_version = 6;")?;
            tx.commit()?;
            (has_replica_rows, has_daemon_rows)
        } else {
            (false, false)
        }
    };

    // Bounded synchronous initial pass (<10ms for <=500 rows) so existing small/medium stores
    // are searchable right away without ever blocking startup on massive histories.
    if need_replica_initial
        && let Err(err) = backfill_replica_fts_batch_on_conn(conn, INITIAL_MIGRATION_BACKFILL_BATCH)
    {
        tracing::warn!(error = %err, "initial replica FTS5 backfill batch failed during v6 migration");
    }
    if need_daemon_initial
        && let Err(err) = backfill_daemon_fts_batch_on_conn(conn, INITIAL_MIGRATION_BACKFILL_BATCH)
    {
        tracing::warn!(error = %err, "initial daemon FTS5 backfill batch failed during v6 migration");
    }

    Ok(())
}

/// Returns true for CJK Unified Ideographs, Hiragana, Katakana, and Hangul characters that
/// SQLite's `unicode61` tokenizer does not split on word boundaries.
#[inline]
pub(crate) fn is_cjk_char(c: char) -> bool {
    matches!(
        c as u32,
        0x3040..=0x309F   // Hiragana
        | 0x30A0..=0x30FF // Katakana
        | 0x3400..=0x4DBF // CJK Unified Ideographs Extension A
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7AF // Hangul Syllables
        | 0xF900..=0xFAFF // CJK Compatibility Ideographs
        | 0x20000..=0x2FA1F // CJK Extensions B..F & Compatibility Supplement
    )
}

#[inline]
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() && !is_cjk_char(c)
}

/// Returns true for Unicode combining diacritical marks (macOS NFD sequences such as
/// `e` + `\u{0301}` for `é`) that attach to a preceding base word character.
#[inline]
fn is_combining_diacritic(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036F
        | 0x1AB0..=0x1AFF
        | 0x1DC0..=0x1DFF
        | 0x20D0..=0x20FF
        | 0xFE20..=0xFE2F
    )
}

fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn normalize_whitespace_limited(raw: &str, max_bytes: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max_bytes));
    let mut prev_space = true;
    for ch in raw.chars() {
        if ch.is_whitespace() || ch == SNIPPET_MARK_START || ch == SNIPPET_MARK_END {
            if !prev_space && !out.is_empty() {
                if out.len() + 1 > max_bytes {
                    break;
                }
                out.push(' ');
                prev_space = true;
            }
        } else {
            let ch_len = ch.len_utf8();
            if out.len() + ch_len > max_bytes {
                break;
            }
            out.push(ch);
            prev_space = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Build the auxiliary CJK character-segmented search column (`cjk_tokens`).
///
/// Returns an empty string for purely non-CJK documents so English/Latin histories pay zero extra
/// FTS5 index storage. For documents containing any CJK characters, inserts spaces between every
/// CJK character and across CJK/Latin boundaries so both exact CJK substring phrase queries
/// (`"连 接 超 时"`) and unspaced mixed queries (`"修复Rust超时"`) match in O(log N).
pub(crate) fn build_cjk_tokens(body: &str) -> String {
    if !body.chars().any(is_cjk_char) {
        return String::new();
    }
    let mut out = String::with_capacity(body.len() + body.len() / 2);
    let mut prev_kind: u8 = 0; // 0 = sep, 1 = cjk, 2 = word
    for ch in body.chars() {
        if is_cjk_char(ch) {
            if prev_kind != 0 {
                out.push(' ');
            }
            out.push(ch);
            prev_kind = 1;
        } else if is_word_char(ch) {
            if prev_kind == 1 {
                out.push(' ');
            }
            out.push(ch);
            prev_kind = 2;
        } else if is_combining_diacritic(ch) && prev_kind == 2 {
            out.push(ch);
        } else {
            if prev_kind != 0 {
                out.push(' ');
            }
            prev_kind = 0;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Extract the searchable role and normalized body text from a `TimelinePayload`.
///
/// Indexes user prompts, assistant replies, agent messages/summaries, notices, questions/answers,
/// and high-signal tool call details (shell commands/output previews, edited file paths, search
/// queries, and subagent prompts). Control/status events and raw thinking scratchpads are skipped.
pub(crate) fn extract_indexable_content(
    payload: &TimelinePayload,
) -> Option<(&'static str, String)> {
    match payload {
        TimelinePayload::UserPrompt { text, .. } => {
            let clean = normalize_whitespace_limited(text, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("user", clean))
        }
        TimelinePayload::AssistantMessage {
            text, streaming, ..
        } => {
            if *streaming {
                return None;
            }
            let clean = normalize_whitespace_limited(text, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("assistant", clean))
        }
        TimelinePayload::AgentMessage {
            source_title,
            event,
            text,
            ..
        } => {
            let mut combined = String::new();
            if let Some(title) = source_title
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                combined.push_str(title);
                combined.push(' ');
            }
            if let Some(ev) = event.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                combined.push_str(ev);
                combined.push(' ');
            }
            combined.push_str(text);
            let clean = normalize_whitespace_limited(&combined, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("agent", clean))
        }
        TimelinePayload::Notice { message, .. } => {
            let clean = normalize_whitespace_limited(message, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("notice", clean))
        }
        TimelinePayload::QuestionRequest { questions, .. } => {
            let mut combined = String::new();
            for q in questions {
                if let Some(h) = q.header.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    combined.push_str(h);
                    combined.push(' ');
                }
                combined.push_str(&q.question);
                combined.push(' ');
            }
            let clean = normalize_whitespace_limited(&combined, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("assistant", clean))
        }
        TimelinePayload::QuestionResolved { answers, .. } => {
            let mut combined = String::new();
            for group in answers {
                for ans in group {
                    combined.push_str(ans);
                    combined.push(' ');
                }
            }
            let clean = normalize_whitespace_limited(&combined, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("user", clean))
        }
        TimelinePayload::ToolCall { detail, .. } => {
            let mut combined = String::new();
            match detail {
                ToolCallDetail::Shell {
                    command,
                    output_preview,
                    ..
                } => {
                    combined.push_str(command);
                    if let Some(out) = output_preview.as_deref() {
                        let clipped = truncate_utf8_bytes(out, MAX_TOOL_OUTPUT_INDEX_BYTES);
                        if !clipped.trim().is_empty() {
                            combined.push('\n');
                            combined.push_str(clipped);
                        }
                    }
                }
                ToolCallDetail::FileEdit { path, .. } => {
                    combined.push_str(path);
                }
                ToolCallDetail::Search { query, scope } => {
                    combined.push_str(query);
                    if let Some(sc) = scope.as_deref().filter(|s| !s.trim().is_empty()) {
                        combined.push(' ');
                        combined.push_str(sc);
                    }
                }
                ToolCallDetail::Subagent {
                    role,
                    prompt_preview,
                    ..
                } => {
                    combined.push_str(role);
                    combined.push(' ');
                    combined.push_str(prompt_preview);
                }
                ToolCallDetail::FileRead { .. } | ToolCallDetail::Generic { .. } => return None,
            }
            let clean = normalize_whitespace_limited(&combined, MAX_INDEXED_BODY_BYTES);
            (!clean.is_empty()).then_some(("tool", clean))
        }
        TimelinePayload::AssistantThinking { .. }
        | TimelinePayload::PermissionRequest { .. }
        | TimelinePayload::PermissionResolved { .. }
        | TimelinePayload::StatusChanged { .. } => None,
    }
}

#[derive(Debug, Clone)]
enum QueryAtom {
    /// One or more contiguous non-CJK word tokens from the same whitespace segment
    /// (e.g. `foo-bar` -> `["foo", "bar"]` matched as `"foo bar"*`, `hello` -> `"hello"*`).
    WordPhrase(Vec<String>),
    /// Contiguous CJK substring (e.g. `"数据库连接"` -> `cjk_tokens : "数 据 库 连 接"`).
    Cjk(String),
}

fn parse_query_atoms(raw: &str) -> Vec<QueryAtom> {
    let mut atoms = Vec::new();
    for segment in raw.split_whitespace() {
        let mut word_group: Vec<String> = Vec::new();
        let mut cur_word = String::new();
        let mut cur_cjk = String::new();

        let flush_word = |cur_word: &mut String, word_group: &mut Vec<String>| {
            if !cur_word.is_empty() {
                word_group.push(std::mem::take(cur_word));
            }
        };
        let flush_word_group = |word_group: &mut Vec<String>, atoms: &mut Vec<QueryAtom>| {
            if !word_group.is_empty() {
                atoms.push(QueryAtom::WordPhrase(std::mem::take(word_group)));
            }
        };
        let flush_cjk = |cur_cjk: &mut String, atoms: &mut Vec<QueryAtom>| {
            if !cur_cjk.is_empty() {
                atoms.push(QueryAtom::Cjk(std::mem::take(cur_cjk)));
            }
        };

        for ch in segment.chars() {
            if is_cjk_char(ch) {
                flush_word(&mut cur_word, &mut word_group);
                flush_word_group(&mut word_group, &mut atoms);
                cur_cjk.push(ch);
            } else if is_word_char(ch) {
                flush_cjk(&mut cur_cjk, &mut atoms);
                cur_word.push(ch);
            } else if is_combining_diacritic(ch) && !cur_word.is_empty() {
                cur_word.push(ch);
            } else {
                // Punctuation like `-`, `/`, `:`, `"`, `*`, `(` acts as a token separator.
                flush_cjk(&mut cur_cjk, &mut atoms);
                flush_word(&mut cur_word, &mut word_group);
            }
        }
        flush_cjk(&mut cur_cjk, &mut atoms);
        flush_word(&mut cur_word, &mut word_group);
        flush_word_group(&mut word_group, &mut atoms);

        if atoms.len() >= MAX_FTS_QUERY_TERMS {
            atoms.truncate(MAX_FTS_QUERY_TERMS);
            break;
        }
    }
    atoms
}

fn escape_fts_quoted(s: &str) -> String {
    s.replace('"', "\"\"")
}

/// Convert an arbitrary user search string into a syntax-safe SQLite FTS5 `MATCH` expression.
///
/// Safely escapes quotes, hyphens (`foo-bar`), slashes (`a/b`), colons (`http://...`),
/// FTS boolean keywords (`AND`, `OR`, `NOT`, `NEAR`), wildcards, and CJK / mixed-script queries.
/// Returns `None` when the input contains no searchable tokens.
pub fn build_fts5_match_query(raw: &str) -> Option<String> {
    let atoms = parse_query_atoms(raw);
    if atoms.is_empty() {
        return None;
    }
    let mut clauses = Vec::with_capacity(atoms.len());
    for atom in atoms {
        match atom {
            QueryAtom::WordPhrase(words) => {
                let joined = words
                    .iter()
                    .map(|w| escape_fts_quoted(w))
                    .collect::<Vec<_>>()
                    .join(" ");
                if !joined.is_empty() {
                    clauses.push(format!("\"{joined}\"*"));
                }
            }
            QueryAtom::Cjk(run) => {
                let spaced = run
                    .chars()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                if !spaced.is_empty() {
                    clauses.push(format!("cjk_tokens : \"{}\"", escape_fts_quoted(&spaced)));
                }
            }
        }
    }
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" AND "))
    }
}

fn strip_literal_mark_tags(text: &str) -> String {
    let mut out = text.to_string();
    loop {
        let next = out.replace("<mark>", "").replace("</mark>", "");
        if next.len() == out.len() {
            return out;
        }
        out = next;
    }
}

/// Format a snippet returned by SQLite FTS5 `snippet()`, falling back to Rust-side centered
/// highlighting when the query contains CJK (matched via `cjk_tokens`) or when `body` has no FTS5 markers.
fn finalize_snippet(raw_sqlite_snippet: &str, full_body: &str, raw_query: &str) -> String {
    let has_cjk_query = raw_query.chars().any(is_cjk_char);
    if !has_cjk_query && raw_sqlite_snippet.contains(SNIPPET_MARK_START) {
        let cleaned = strip_literal_mark_tags(raw_sqlite_snippet);
        let mut out = String::with_capacity(cleaned.len() + 32);
        let mut prev_space = true;
        for ch in cleaned.chars() {
            if ch == SNIPPET_MARK_START {
                out.push_str("<mark>");
                prev_space = false;
            } else if ch == SNIPPET_MARK_END {
                out.push_str("</mark>");
                prev_space = false;
            } else if ch.is_whitespace() {
                if !prev_space && !out.is_empty() {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                out.push(ch);
                prev_space = false;
            }
        }
        return out.trim().to_string();
    }
    build_fallback_snippet(full_body, raw_query, 150)
}

fn build_fallback_snippet(full_body: &str, raw_query: &str, max_chars: usize) -> String {
    let clean_body = strip_literal_mark_tags(full_body);
    let atoms = parse_query_atoms(raw_query);

    // Collect searchable literal needles for highlighting in `clean_body`.
    // When longer words or CJK runs are present, skip 1-char ASCII words to avoid noisy highlights.
    let has_substantial_atom = atoms.iter().any(|atom| match atom {
        QueryAtom::Cjk(run) => !run.is_empty(),
        QueryAtom::WordPhrase(words) => words.iter().any(|w| w.len() > 1 || !w.is_ascii()),
    });
    let mut needles: Vec<String> = Vec::new();
    for atom in &atoms {
        match atom {
            QueryAtom::Cjk(run) => {
                if !run.is_empty() {
                    needles.push(run.clone());
                }
            }
            QueryAtom::WordPhrase(words) => {
                for w in words {
                    if w.is_empty() {
                        continue;
                    }
                    if has_substantial_atom && w.len() == 1 && w.is_ascii() {
                        continue;
                    }
                    needles.push(w.to_lowercase());
                }
            }
        }
    }

    // Find all match byte ranges in `clean_body`.
    let body_lower = clean_body.to_lowercase();
    let lower_matches_byte_len = body_lower.len() == clean_body.len();
    let mut ranges: Vec<(usize, usize)> = Vec::new();

    if lower_matches_byte_len {
        for needle in &needles {
            let needle_lower = needle.to_lowercase();
            if needle_lower.is_empty() {
                continue;
            }
            let mut offset = 0;
            while let Some(pos) = body_lower[offset..].find(&needle_lower) {
                let start = offset + pos;
                let end = start + needle_lower.len();
                if clean_body.is_char_boundary(start) && clean_body.is_char_boundary(end) {
                    ranges.push((start, end));
                }
                offset = end;
            }
        }
    } else {
        // Rare Unicode case-folding length change (e.g. Turkish dotted I): fall back to exact match.
        for needle in &needles {
            if needle.is_empty() {
                continue;
            }
            let mut offset = 0;
            while let Some(pos) = clean_body[offset..].find(needle.as_str()) {
                let start = offset + pos;
                let end = start + needle.len();
                ranges.push((start, end));
                offset = end;
            }
        }
    }

    // Sort and merge overlapping match ranges.
    ranges.sort_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        if let Some(last) = merged.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
            continue;
        }
        merged.push((s, e));
    }

    // Choose a clean char-aligned window centered on the first match.
    let char_indices: Vec<(usize, char)> = clean_body.char_indices().collect();
    let total_chars = char_indices.len();
    if total_chars == 0 {
        return String::new();
    }

    let first_match_char = merged
        .first()
        .map(|&(start_byte, _)| {
            char_indices
                .binary_search_by_key(&start_byte, |&(b, _)| b)
                .unwrap_or_else(|i| i.min(total_chars.saturating_sub(1)))
        })
        .unwrap_or(0);

    let half = max_chars / 3;
    let win_start_char = first_match_char.saturating_sub(half);
    let win_end_char = (win_start_char + max_chars).min(total_chars);
    let win_start_char = win_end_char.saturating_sub(max_chars);

    let win_start_byte = char_indices[win_start_char].0;
    let win_end_byte = if win_end_char < total_chars {
        char_indices[win_end_char].0
    } else {
        clean_body.len()
    };

    let mut out = String::with_capacity(max_chars * 2);
    if win_start_char > 0 {
        out.push('…');
    }

    let mut cursor = win_start_byte;
    for (s, e) in merged {
        if e <= win_start_byte {
            continue;
        }
        if s >= win_end_byte {
            break;
        }
        let clamped_s = s.max(win_start_byte);
        let clamped_e = e.min(win_end_byte);
        if clamped_s > cursor {
            out.push_str(&clean_body[cursor..clamped_s]);
        }
        if clamped_e > clamped_s {
            out.push_str("<mark>");
            out.push_str(&clean_body[clamped_s..clamped_e]);
            out.push_str("</mark>");
            cursor = clamped_e;
        }
    }
    if cursor < win_end_byte {
        out.push_str(&clean_body[cursor..win_end_byte]);
    }
    if win_end_char < total_chars {
        out.push('…');
    }
    out.trim().to_string()
}

pub(crate) fn resolve_replica_host_id(
    tx: &Transaction<'_>,
    explicit_host_id: &str,
    agent_id: &str,
) -> String {
    if !explicit_host_id.is_empty() {
        return explicit_host_id.to_string();
    }
    tx.query_row(
        "SELECT host_id FROM client_replica_agents WHERE agent_id = ?1 LIMIT 1",
        params![agent_id],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .unwrap_or_default()
}

pub(crate) fn sync_replica_fts_upsert(
    tx: &Transaction<'_>,
    rowid: i64,
    host_id: &str,
    agent_id: &str,
    seq: u64,
    payload: &TimelinePayload,
) -> Result<(), StoreError> {
    {
        let mut del_stmt = tx.prepare_cached("DELETE FROM client_timeline_fts WHERE rowid = ?1")?;
        del_stmt.execute(params![rowid])?;
    }
    let Some((role, body)) = extract_indexable_content(payload) else {
        return Ok(());
    };
    let seq_i64 = i64::try_from(seq)
        .map_err(|_| StoreError::InvalidArgument("sequence number exceeds i64".into()))?;
    let cjk_tokens = build_cjk_tokens(&body);
    let mut ins_stmt = tx.prepare_cached(
        "INSERT INTO client_timeline_fts (rowid, host_id, agent_id, seq, role, body, cjk_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    ins_stmt.execute(params![
        rowid, host_id, agent_id, seq_i64, role, body, cjk_tokens
    ])?;
    Ok(())
}

pub(crate) fn delete_replica_fts_by_rowids(
    tx: &Transaction<'_>,
    rowids: &[i64],
) -> Result<(), StoreError> {
    if rowids.is_empty() {
        return Ok(());
    }
    let mut del_stmt = tx.prepare_cached("DELETE FROM client_timeline_fts WHERE rowid = ?1")?;
    for &rid in rowids {
        del_stmt.execute(params![rid])?;
    }
    Ok(())
}

pub(crate) fn delete_replica_fts_for_agent(
    tx: &Transaction<'_>,
    agent_id: &str,
) -> Result<(), StoreError> {
    let rowids: Vec<i64> = {
        let mut stmt =
            tx.prepare_cached("SELECT rowid FROM client_replica_events WHERE agent_id = ?1")?;
        let rows = stmt.query_map(params![agent_id], |r| r.get::<_, i64>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    delete_replica_fts_by_rowids(tx, &rowids)
}

pub(crate) fn delete_replica_fts_and_events_for_host(
    tx: &Transaction<'_>,
    host_id: &str,
) -> Result<(), StoreError> {
    let agent_ids: Vec<String> = {
        let mut stmt =
            tx.prepare_cached("SELECT agent_id FROM client_replica_agents WHERE host_id = ?1")?;
        let rows = stmt.query_map(params![host_id], |r| r.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for ag_id in &agent_ids {
        delete_replica_fts_for_agent(tx, ag_id)?;
        tx.execute(
            "DELETE FROM client_replica_events WHERE agent_id = ?1",
            params![ag_id],
        )?;
        tx.execute(
            "DELETE FROM client_replica_cursors WHERE agent_id = ?1",
            params![ag_id],
        )?;
    }
    tx.execute(
        "DELETE FROM client_timeline_fts WHERE host_id = ?1",
        params![host_id],
    )?;
    Ok(())
}

pub(crate) fn sync_daemon_fts_upsert(
    tx: &Transaction<'_>,
    seq: i64,
    agent_id: &str,
    payload: &TimelinePayload,
) -> Result<(), StoreError> {
    {
        let mut del_stmt = tx.prepare_cached("DELETE FROM timeline_events_fts WHERE rowid = ?1")?;
        del_stmt.execute(params![seq])?;
    }
    let Some((role, body)) = extract_indexable_content(payload) else {
        return Ok(());
    };
    let cjk_tokens = build_cjk_tokens(&body);
    let mut ins_stmt = tx.prepare_cached(
        "INSERT INTO timeline_events_fts (rowid, agent_id, seq, role, body, cjk_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
    )?;
    ins_stmt.execute(params![seq, agent_id, seq, role, body, cjk_tokens])?;
    Ok(())
}

pub(crate) fn delete_daemon_fts_by_seqs(
    tx: &Transaction<'_>,
    seqs: &[i64],
) -> Result<(), StoreError> {
    if seqs.is_empty() {
        return Ok(());
    }
    let mut del_stmt = tx.prepare_cached("DELETE FROM timeline_events_fts WHERE rowid = ?1")?;
    for &seq in seqs {
        del_stmt.execute(params![seq])?;
    }
    Ok(())
}

pub(crate) fn delete_daemon_fts_for_agent(
    tx: &Transaction<'_>,
    agent_id: &str,
) -> Result<(), StoreError> {
    let seqs: Vec<i64> = {
        let mut stmt = tx.prepare_cached("SELECT seq FROM timeline_events WHERE agent_id = ?1")?;
        let rows = stmt.query_map(params![agent_id], |r| r.get::<_, i64>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    delete_daemon_fts_by_seqs(tx, &seqs)
}

pub(crate) fn delete_daemon_fts_from_seq(
    tx: &Transaction<'_>,
    agent_id: &str,
    from_seq: i64,
) -> Result<(), StoreError> {
    let seqs: Vec<i64> = {
        let mut stmt =
            tx.prepare_cached("SELECT seq FROM timeline_events WHERE agent_id = ?1 AND seq >= ?2")?;
        let rows = stmt.query_map(params![agent_id, from_seq], |r| r.get::<_, i64>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    delete_daemon_fts_by_seqs(tx, &seqs)
}

pub(crate) fn clear_daemon_fts(conn: &Connection) -> Result<(), StoreError> {
    conn.execute("DELETE FROM timeline_events_fts", [])?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KvTable {
    Client,
    Host,
}

fn read_kv(conn: &Connection, table: KvTable, key: &str) -> Result<Option<String>, StoreError> {
    let sql = match table {
        KvTable::Client => "SELECT value_json FROM client_kv WHERE key = ?1",
        KvTable::Host => "SELECT value_json FROM host_kv WHERE key = ?1",
    };
    conn.query_row(sql, params![key], |r| r.get::<_, String>(0))
        .optional()
        .map_err(Into::into)
}

fn write_kv(conn: &Connection, table: KvTable, key: &str, value: &str) -> Result<(), StoreError> {
    let sql = match table {
        KvTable::Client => {
            "INSERT INTO client_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at_ms = excluded.updated_at_ms"
        }
        KvTable::Host => {
            "INSERT INTO host_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET
                value_json = excluded.value_json,
                updated_at_ms = excluded.updated_at_ms"
        }
    };
    conn.execute(sql, params![key, value, Utc::now().timestamp_millis()])?;
    Ok(())
}

fn write_kv_if_absent(
    conn: &Connection,
    table: KvTable,
    key: &str,
    value: &str,
) -> Result<(), StoreError> {
    let sql = match table {
        KvTable::Client => {
            "INSERT OR IGNORE INTO client_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)"
        }
        KvTable::Host => {
            "INSERT OR IGNORE INTO host_kv (key, value_json, updated_at_ms)
             VALUES (?1, ?2, ?3)"
        }
    };
    conn.execute(sql, params![key, value, Utc::now().timestamp_millis()])?;
    Ok(())
}

struct RawReplicaBackfillRow {
    rowid: i64,
    agent_id: String,
    seq: i64,
    payload_json: String,
    host_id: String,
}

struct PreparedReplicaFtsRow {
    rowid: i64,
    host_id: String,
    agent_id: String,
    seq: i64,
    role: &'static str,
    body: String,
    cjk_tokens: String,
}

fn fetch_replica_backfill_batch(
    conn: &Connection,
    batch: i64,
) -> Result<(i64, Vec<RawReplicaBackfillRow>), StoreError> {
    let last_rowid: i64 = read_kv(conn, KvTable::Client, KV_REPLICA_LAST_ROWID)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut stmt = conn.prepare(
        "SELECT e.rowid,
                e.agent_id,
                e.seq,
                e.payload_json,
                COALESCE((SELECT cra.host_id FROM client_replica_agents cra WHERE cra.agent_id = e.agent_id LIMIT 1), '')
         FROM client_replica_events e
         WHERE e.rowid > ?1
           AND NOT EXISTS (SELECT 1 FROM client_timeline_fts f WHERE f.rowid = e.rowid)
         ORDER BY e.rowid ASC
         LIMIT ?2",
    )?;
    let mapped = stmt.query_map(params![last_rowid, batch], |r| {
        Ok(RawReplicaBackfillRow {
            rowid: r.get::<_, i64>(0)?,
            agent_id: r.get::<_, String>(1)?,
            seq: r.get::<_, i64>(2)?.max(0),
            payload_json: r.get::<_, String>(3)?,
            host_id: r.get::<_, String>(4)?,
        })
    })?;
    let rows = mapped.collect::<Result<Vec<_>, _>>()?;
    Ok((last_rowid, rows))
}

fn prepare_replica_backfill_entries(
    rows: Vec<RawReplicaBackfillRow>,
) -> Vec<PreparedReplicaFtsRow> {
    let mut prepared = Vec::with_capacity(rows.len());
    for row in rows {
        if let Ok(payload) = serde_json::from_str::<TimelinePayload>(&row.payload_json)
            && let Some((role, body)) = extract_indexable_content(&payload)
        {
            let cjk_tokens = build_cjk_tokens(&body);
            prepared.push(PreparedReplicaFtsRow {
                rowid: row.rowid,
                host_id: row.host_id,
                agent_id: row.agent_id,
                seq: row.seq,
                role,
                body,
                cjk_tokens,
            });
        }
    }
    prepared
}

fn commit_replica_backfill_batch(
    conn: &mut Connection,
    scanned: usize,
    max_rowid: i64,
    done: bool,
    prepared: Vec<PreparedReplicaFtsRow>,
) -> Result<FtsBackfillProgress, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut indexed = 0usize;
    {
        let mut exists_stmt =
            tx.prepare_cached("SELECT 1 FROM client_timeline_fts WHERE rowid = ?1 LIMIT 1")?;
        let mut ins_stmt = tx.prepare_cached(
            "INSERT INTO client_timeline_fts (rowid, host_id, agent_id, seq, role, body, cjk_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for entry in prepared {
            // Skip if a concurrent live write already indexed this rowid with an authoritative host_id.
            let already_indexed = exists_stmt
                .query_row(params![entry.rowid], |_| Ok(true))
                .optional()?
                .unwrap_or(false);
            if already_indexed {
                continue;
            }
            ins_stmt.execute(params![
                entry.rowid,
                entry.host_id,
                entry.agent_id,
                entry.seq,
                entry.role,
                entry.body,
                entry.cjk_tokens
            ])?;
            indexed += 1;
        }
    }
    let cur_last: i64 = read_kv(&tx, KvTable::Client, KV_REPLICA_LAST_ROWID)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let next_last = cur_last.max(max_rowid);
    write_kv(
        &tx,
        KvTable::Client,
        KV_REPLICA_LAST_ROWID,
        &next_last.to_string(),
    )?;
    write_kv(
        &tx,
        KvTable::Client,
        KV_REPLICA_DONE,
        if done { "true" } else { "false" },
    )?;
    tx.commit()?;
    Ok(FtsBackfillProgress {
        scanned,
        indexed,
        done,
    })
}

fn backfill_replica_fts_batch_on_conn(
    conn: &mut Connection,
    batch_size: usize,
) -> Result<FtsBackfillProgress, StoreError> {
    let batch = batch_size.clamp(1, 5_000) as i64;
    let (last_rowid, rows) = fetch_replica_backfill_batch(conn, batch)?;
    if rows.is_empty() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_kv(&tx, KvTable::Client, KV_REPLICA_DONE, "true")?;
        tx.commit()?;
        return Ok(FtsBackfillProgress {
            scanned: 0,
            indexed: 0,
            done: true,
        });
    }
    let scanned = rows.len();
    let max_rowid = rows.last().map(|r| r.rowid).unwrap_or(last_rowid);
    let done = (scanned as i64) < batch;
    let prepared = prepare_replica_backfill_entries(rows);
    commit_replica_backfill_batch(conn, scanned, max_rowid, done, prepared)
}

struct RawDaemonBackfillRow {
    seq: i64,
    agent_id: String,
    payload_json: String,
}

struct PreparedDaemonFtsRow {
    seq: i64,
    agent_id: String,
    role: &'static str,
    body: String,
    cjk_tokens: String,
}

fn fetch_daemon_backfill_batch(
    conn: &Connection,
    batch: i64,
) -> Result<(i64, Vec<RawDaemonBackfillRow>), StoreError> {
    let last_rowid: i64 = read_kv(conn, KvTable::Host, KV_DAEMON_LAST_ROWID)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let mut stmt = conn.prepare(
        "SELECT e.seq, e.agent_id, e.payload_json
         FROM timeline_events e
         WHERE e.seq > ?1
           AND NOT EXISTS (SELECT 1 FROM timeline_events_fts f WHERE f.rowid = e.seq)
         ORDER BY e.seq ASC
         LIMIT ?2",
    )?;
    let mapped = stmt.query_map(params![last_rowid, batch], |r| {
        Ok(RawDaemonBackfillRow {
            seq: r.get::<_, i64>(0)?,
            agent_id: r.get::<_, String>(1)?,
            payload_json: r.get::<_, String>(2)?,
        })
    })?;
    let rows = mapped.collect::<Result<Vec<_>, _>>()?;
    Ok((last_rowid, rows))
}

fn prepare_daemon_backfill_entries(rows: Vec<RawDaemonBackfillRow>) -> Vec<PreparedDaemonFtsRow> {
    let mut prepared = Vec::with_capacity(rows.len());
    for row in rows {
        if let Ok(payload) = serde_json::from_str::<TimelinePayload>(&row.payload_json)
            && let Some((role, body)) = extract_indexable_content(&payload)
        {
            let cjk_tokens = build_cjk_tokens(&body);
            prepared.push(PreparedDaemonFtsRow {
                seq: row.seq,
                agent_id: row.agent_id,
                role,
                body,
                cjk_tokens,
            });
        }
    }
    prepared
}

fn commit_daemon_backfill_batch(
    conn: &mut Connection,
    scanned: usize,
    max_seq: i64,
    done: bool,
    prepared: Vec<PreparedDaemonFtsRow>,
) -> Result<FtsBackfillProgress, StoreError> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut indexed = 0usize;
    {
        let mut exists_stmt =
            tx.prepare_cached("SELECT 1 FROM timeline_events_fts WHERE rowid = ?1 LIMIT 1")?;
        let mut ins_stmt = tx.prepare_cached(
            "INSERT INTO timeline_events_fts (rowid, agent_id, seq, role, body, cjk_tokens)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for entry in prepared {
            let already_indexed = exists_stmt
                .query_row(params![entry.seq], |_| Ok(true))
                .optional()?
                .unwrap_or(false);
            if already_indexed {
                continue;
            }
            ins_stmt.execute(params![
                entry.seq,
                entry.agent_id,
                entry.seq,
                entry.role,
                entry.body,
                entry.cjk_tokens
            ])?;
            indexed += 1;
        }
    }
    let cur_last: i64 = read_kv(&tx, KvTable::Host, KV_DAEMON_LAST_ROWID)?
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    let next_last = cur_last.max(max_seq);
    write_kv(
        &tx,
        KvTable::Host,
        KV_DAEMON_LAST_ROWID,
        &next_last.to_string(),
    )?;
    write_kv(
        &tx,
        KvTable::Host,
        KV_DAEMON_DONE,
        if done { "true" } else { "false" },
    )?;
    tx.commit()?;
    Ok(FtsBackfillProgress {
        scanned,
        indexed,
        done,
    })
}

fn backfill_daemon_fts_batch_on_conn(
    conn: &mut Connection,
    batch_size: usize,
) -> Result<FtsBackfillProgress, StoreError> {
    let batch = batch_size.clamp(1, 5_000) as i64;
    let (last_rowid, rows) = fetch_daemon_backfill_batch(conn, batch)?;
    if rows.is_empty() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_kv(&tx, KvTable::Host, KV_DAEMON_DONE, "true")?;
        tx.commit()?;
        return Ok(FtsBackfillProgress {
            scanned: 0,
            indexed: 0,
            done: true,
        });
    }
    let scanned = rows.len();
    let max_seq = rows.last().map(|r| r.seq).unwrap_or(last_rowid);
    let done = (scanned as i64) < batch;
    let prepared = prepare_daemon_backfill_entries(rows);
    commit_daemon_backfill_batch(conn, scanned, max_seq, done, prepared)
}

impl OrbitStore {
    /// Returns `true` when incremental backfill of legacy `client_replica_events` into
    /// `client_timeline_fts` has completed.
    pub fn is_replica_fts_backfill_done(&self) -> bool {
        let Ok(conn) = self.conn.lock() else {
            return true;
        };
        read_kv(&conn, KvTable::Client, KV_REPLICA_DONE)
            .ok()
            .flatten()
            .is_some_and(|v| v == "true")
    }

    /// Returns `true` when incremental backfill of legacy `timeline_events` into
    /// `timeline_events_fts` has completed.
    pub fn is_daemon_fts_backfill_done(&self) -> bool {
        let Ok(conn) = self.conn.lock() else {
            return true;
        };
        read_kv(&conn, KvTable::Host, KV_DAEMON_DONE)
            .ok()
            .flatten()
            .is_some_and(|v| v == "true")
    }

    /// Backfill up to `batch_size` unindexed `client_replica_events` rows into `client_timeline_fts`,
    /// releasing the store mutex during CPU-bound JSON parsing and CJK tokenization.
    pub fn backfill_replica_fts_batch(
        &self,
        batch_size: usize,
    ) -> Result<FtsBackfillProgress, StoreError> {
        let batch = batch_size.clamp(1, 5_000) as i64;
        let (last_rowid, rows) = {
            let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            fetch_replica_backfill_batch(&conn, batch)?
        };
        if rows.is_empty() {
            let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            write_kv(&tx, KvTable::Client, KV_REPLICA_DONE, "true")?;
            tx.commit()?;
            return Ok(FtsBackfillProgress {
                scanned: 0,
                indexed: 0,
                done: true,
            });
        }
        let scanned = rows.len();
        let max_rowid = rows.last().map(|r| r.rowid).unwrap_or(last_rowid);
        let done = (scanned as i64) < batch;
        let prepared = prepare_replica_backfill_entries(rows);
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        commit_replica_backfill_batch(&mut conn, scanned, max_rowid, done, prepared)
    }

    /// Backfill up to `batch_size` unindexed daemon `timeline_events` rows into `timeline_events_fts`,
    /// releasing the store mutex during CPU-bound JSON parsing and CJK tokenization.
    pub fn backfill_daemon_fts_batch(
        &self,
        batch_size: usize,
    ) -> Result<FtsBackfillProgress, StoreError> {
        let batch = batch_size.clamp(1, 5_000) as i64;
        let (last_rowid, rows) = {
            let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            fetch_daemon_backfill_batch(&conn, batch)?
        };
        if rows.is_empty() {
            let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            write_kv(&tx, KvTable::Host, KV_DAEMON_DONE, "true")?;
            tx.commit()?;
            return Ok(FtsBackfillProgress {
                scanned: 0,
                indexed: 0,
                done: true,
            });
        }
        let scanned = rows.len();
        let max_seq = rows.last().map(|r| r.seq).unwrap_or(last_rowid);
        let done = (scanned as i64) < batch;
        let prepared = prepare_daemon_backfill_entries(rows);
        let mut conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        commit_daemon_backfill_batch(&mut conn, scanned, max_seq, done, prepared)
    }

    /// Full-text search across all locally replicated conversations using SQLite FTS5 + BM25,
    /// returning at most `limit` unique conversations (best-ranked match per conversation) with
    /// highlighted `<mark>...</mark>` snippets.
    pub fn search_replica_messages(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MessageSearchHit>, StoreError> {
        self.search_replica_messages_for_host(query, None, limit)
    }

    /// Host-filterable full-text search across locally replicated conversations using SQLite FTS5
    /// and BM25 ranking, returning at most `limit` unique conversations (best-ranked match per
    /// conversation) with highlighted `<mark>...</mark>` snippets. When `host_id` is
    /// `Some(non_empty)`, restricts results to that host before deduplicating and applying `limit`.
    pub fn search_replica_messages_for_host(
        &self,
        query: &str,
        host_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MessageSearchHit>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(fts_query) = build_fts5_match_query(query) else {
            return Ok(Vec::new());
        };
        let host_filter = host_id.map(str::trim).filter(|s| !s.is_empty());
        let effective_limit = limit.min(200);
        let mark_start = SNIPPET_MARK_START.to_string();
        let mark_end = SNIPPET_MARK_END.to_string();

        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare_cached(
            "WITH matched AS (
                SELECT
                    COALESCE(
                        NULLIF(client_timeline_fts.host_id, ''),
                        (SELECT cra.host_id FROM client_replica_agents cra WHERE cra.agent_id = client_timeline_fts.agent_id LIMIT 1),
                        ''
                    ) AS resolved_host_id,
                    client_timeline_fts.agent_id AS agent_id,
                    client_timeline_fts.seq AS seq,
                    client_timeline_fts.role AS role,
                    snippet(client_timeline_fts, 4, ?2, ?3, '…', 20) AS raw_snippet,
                    client_timeline_fts.body AS body,
                    bm25(client_timeline_fts, 0.0, 0.0, 0.0, 0.0, 1.0, 0.5) AS rank,
                    e.timestamp AS timestamp
                FROM client_timeline_fts
                LEFT JOIN client_replica_events e ON e.rowid = client_timeline_fts.rowid
                WHERE client_timeline_fts MATCH ?1
                  AND (?5 IS NULL OR COALESCE(
                        NULLIF(client_timeline_fts.host_id, ''),
                        (SELECT cra.host_id FROM client_replica_agents cra WHERE cra.agent_id = client_timeline_fts.agent_id LIMIT 1),
                        ''
                  ) = ?5)
             ),
             ranked AS (
                SELECT
                    resolved_host_id,
                    agent_id,
                    seq,
                    role,
                    raw_snippet,
                    body,
                    rank,
                    timestamp,
                    ROW_NUMBER() OVER (
                        PARTITION BY resolved_host_id, agent_id
                        ORDER BY rank ASC, seq DESC
                    ) AS rn
                FROM matched
             )
             SELECT resolved_host_id, agent_id, seq, role, raw_snippet, body, rank, timestamp
             FROM ranked
             WHERE rn = 1
             ORDER BY rank ASC, seq DESC
             LIMIT ?4",
        )?;

        let rows = stmt.query_map(
            params![
                fts_query,
                mark_start,
                mark_end,
                effective_limit as i64,
                host_filter
            ],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?.max(0) as u64,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            },
        )?;

        let mut hits = Vec::with_capacity(effective_limit);
        for row in rows {
            let (host_id, agent_id, seq, role, raw_snippet, body, rank, timestamp_rfc3339) = row?;
            let snippet = finalize_snippet(&raw_snippet, &body, query);
            hits.push(MessageSearchHit {
                host_id,
                agent_id,
                seq,
                role,
                snippet,
                rank,
                timestamp_rfc3339,
            });
        }
        Ok(hits)
    }

    /// Full-text search across daemon-side `timeline_events_fts` using SQLite FTS5 + BM25,
    /// deduplicated to the top match per `agent_id`.
    pub fn search_daemon_messages(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MessageSearchHit>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let Some(fts_query) = build_fts5_match_query(query) else {
            return Ok(Vec::new());
        };
        let effective_limit = limit.min(200);
        let mark_start = SNIPPET_MARK_START.to_string();
        let mark_end = SNIPPET_MARK_END.to_string();

        let conn = self.conn.lock().map_err(|_| StoreError::LockPoisoned)?;
        let mut stmt = conn.prepare_cached(
            "WITH matched AS (
                SELECT
                    timeline_events_fts.agent_id AS agent_id,
                    timeline_events_fts.seq AS seq,
                    timeline_events_fts.role AS role,
                    snippet(timeline_events_fts, 3, ?2, ?3, '…', 20) AS raw_snippet,
                    timeline_events_fts.body AS body,
                    bm25(timeline_events_fts, 0.0, 0.0, 0.0, 1.0, 0.5) AS rank,
                    e.timestamp AS timestamp
                FROM timeline_events_fts
                LEFT JOIN timeline_events e ON e.seq = timeline_events_fts.rowid
                WHERE timeline_events_fts MATCH ?1
             ),
             ranked AS (
                SELECT
                    agent_id,
                    seq,
                    role,
                    raw_snippet,
                    body,
                    rank,
                    timestamp,
                    ROW_NUMBER() OVER (
                        PARTITION BY agent_id
                        ORDER BY rank ASC, seq DESC
                    ) AS rn
                FROM matched
             )
             SELECT agent_id, seq, role, raw_snippet, body, rank, timestamp
             FROM ranked
             WHERE rn = 1
             ORDER BY rank ASC, seq DESC
             LIMIT ?4",
        )?;

        let rows = stmt.query_map(
            params![fts_query, mark_start, mark_end, effective_limit as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?.max(0) as u64,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, f64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )?;

        let mut hits = Vec::with_capacity(effective_limit);
        for row in rows {
            let (agent_id, seq, role, raw_snippet, body, rank, timestamp_rfc3339) = row?;
            let snippet = finalize_snippet(&raw_snippet, &body, query);
            hits.push(MessageSearchHit {
                host_id: String::new(),
                agent_id,
                seq,
                role,
                snippet,
                rank,
                timestamp_rfc3339,
            });
        }
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{Duration, Utc};
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, ProviderKind, SessionMode, TimelineEvent,
        TimelinePayload, ToolCallDetail, ToolCallStatus, WorkspaceSetupStatus, WorkspaceSummary,
    };

    use super::*;

    fn mk_agent(agent_id: &str, workspace_id: &str, title: &str) -> AgentSummary {
        let now = Utc::now();
        AgentSummary {
            agent_id: agent_id.into(),
            workspace_id: workspace_id.into(),
            cwd: "/workspace/orbit".into(),
            title: title.into(),
            provider: ProviderKind::Antigravity,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::Default,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: None,
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::None,
            last_activity_at: Some(now),
            queued_prompts: Vec::new(),
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        }
    }

    fn mk_replica_event(
        agent_id: &str,
        seq: u64,
        payload: TimelinePayload,
        offset_secs: i64,
    ) -> TimelineEvent {
        TimelineEvent {
            seq,
            epoch: 1,
            agent_id: agent_id.into(),
            turn_id: Some(format!("turn_{seq}")),
            timestamp: Utc::now() + Duration::seconds(offset_secs),
            payload,
        }
    }

    #[test]
    fn fts5_query_escaping_handles_punctuation_quotes_and_keywords_safely() {
        assert_eq!(build_fts5_match_query(""), None);
        assert_eq!(build_fts5_match_query("   --- /// \"\"\" () * ^ :  "), None);
        assert_eq!(
            build_fts5_match_query("foo-bar"),
            Some("\"foo bar\"*".into())
        );
        assert_eq!(build_fts5_match_query("a/b"), Some("\"a b\"*".into()));
        assert_eq!(
            build_fts5_match_query("\"unclosed"),
            Some("\"unclosed\"*".into())
        );
        assert_eq!(
            build_fts5_match_query("AND OR NOT NEAR"),
            Some("\"AND\"* AND \"OR\"* AND \"NOT\"* AND \"NEAR\"*".into())
        );
        assert_eq!(
            build_fts5_match_query("数据库连接"),
            Some("cjk_tokens : \"数 据 库 连 接\"".into())
        );
        assert_eq!(
            build_fts5_match_query("修复SQLite超时"),
            Some("cjk_tokens : \"修 复\" AND \"SQLite\"* AND cjk_tokens : \"超 时\"".into())
        );
    }

    #[test]
    fn fts5_english_cjk_punctuation_and_snippet_extraction() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag1 = mk_agent("ag_en", "ws_1", "Postgres Pool");
        let ag2 = mk_agent("ag_cjk", "ws_1", "数据库调优");
        let ag3 = mk_agent("ag_path", "ws_1", "Route Parser");
        store
            .upsert_replica_agents("host_a", &[ag1, ag2])
            .expect("upsert agents a");
        store
            .upsert_replica_agents("host_b", &[ag3])
            .expect("upsert agents b");

        store
            .upsert_replica_timeline_events_batch_for_host(
                "host_a",
                &[
                    mk_replica_event(
                        "ag_en",
                        1,
                        TimelinePayload::UserPrompt {
                            message_id: "u1".into(),
                            text: "Fix PostgreSQL connection timeout in café deployment".into(),
                            badges: Vec::new(),
                        },
                        0,
                    ),
                    mk_replica_event(
                        "ag_cjk",
                        1,
                        TimelinePayload::UserPrompt {
                            message_id: "u2".into(),
                            text: "请帮我修复数据库连接超时的问题，顺便检查SQLite索引性能".into(),
                            badges: Vec::new(),
                        },
                        1,
                    ),
                    mk_replica_event(
                        "ag_cjk",
                        2,
                        TimelinePayload::AssistantMessage {
                            item_id: "a2".into(),
                            text: "已完成SQLite连接池优化与日志排查".into(),
                            streaming: false,
                        },
                        2,
                    ),
                ],
            )
            .expect("batch host a");

        store
            .upsert_replica_timeline_events_batch_for_host(
                "host_b",
                &[
                    mk_replica_event(
                        "ag_path",
                        1,
                        TimelinePayload::AssistantMessage {
                            item_id: "a3".into(),
                            text: "Updated foo-bar parser and moved module to a/b/router.rs with \"unclosed string handling".into(),
                            streaming: false,
                        },
                        3,
                    ),
                    mk_replica_event(
                        "ag_path",
                        2,
                        TimelinePayload::ToolCall {
                            call_id: "c1".into(),
                            status: ToolCallStatus::Completed,
                            detail: ToolCallDetail::Shell {
                                command: "cargo test --package orbit-store".into(),
                                cwd: Some("/workspace/orbit".into()),
                                exit_code: Some(0),
                                output_preview: Some("test result: ok. 42 passed; zephyr_token".into()),
                            },
                            background_task: None,
                        },
                        4,
                    ),
                ],
            )
            .expect("batch host b");

        // English prefix + diacritics removal
        let en_hits = store
            .search_replica_messages("postgres cafe", 10)
            .expect("en search");
        assert_eq!(en_hits.len(), 1);
        assert_eq!(en_hits[0].host_id, "host_a");
        assert_eq!(en_hits[0].agent_id, "ag_en");
        assert_eq!(en_hits[0].role, "user");
        assert!(
            en_hits[0].snippet.contains("<mark>PostgreSQL</mark>"),
            "expected highlighted PostgreSQL in snippet: {}",
            en_hits[0].snippet
        );
        assert!(
            en_hits[0].snippet.contains("<mark>café</mark>"),
            "expected diacritic-matched café in snippet: {}",
            en_hits[0].snippet
        );

        // CJK mid-sentence substring query
        let cjk_hits = store
            .search_replica_messages("数据库连接超时", 10)
            .expect("cjk search");
        assert_eq!(cjk_hits.len(), 1);
        assert_eq!(cjk_hits[0].agent_id, "ag_cjk");
        assert!(
            cjk_hits[0].snippet.contains("<mark>数据库连接超时</mark>"),
            "expected highlighted CJK substring in snippet: {}",
            cjk_hits[0].snippet
        );

        // Mixed CJK + unspaced English query (adjacent matches coalesce into one clean <mark>)
        let mixed_hits = store
            .search_replica_messages("检查SQLite索引", 10)
            .expect("mixed search");
        assert_eq!(mixed_hits.len(), 1);
        assert_eq!(mixed_hits[0].agent_id, "ag_cjk");
        assert!(
            mixed_hits[0]
                .snippet
                .contains("<mark>检查SQLite索引</mark>"),
            "expected contiguous highlighted mixed span in snippet: {}",
            mixed_hits[0].snippet
        );

        // Mixed CJK + space-separated English query highlights both separate spans
        let mixed_sep_hits = store
            .search_replica_messages("数据库 SQLite", 10)
            .expect("mixed separated search");
        assert_eq!(mixed_sep_hits.len(), 1);
        assert!(mixed_sep_hits[0].snippet.contains("<mark>数据库</mark>"));
        assert!(mixed_sep_hits[0].snippet.contains("<mark>SQLite</mark>"));

        // Special punctuation: foo-bar, a/b, "unclosed, bare symbols
        let hyphen_hits = store
            .search_replica_messages("foo-bar", 10)
            .expect("foo-bar");
        assert_eq!(hyphen_hits.len(), 1);
        assert_eq!(hyphen_hits[0].host_id, "host_b");
        assert_eq!(hyphen_hits[0].agent_id, "ag_path");

        let slash_hits = store.search_replica_messages("a/b", 10).expect("a/b");
        assert_eq!(slash_hits.len(), 1);
        assert_eq!(slash_hits[0].agent_id, "ag_path");

        let quote_hits = store
            .search_replica_messages("\"unclosed", 10)
            .expect("unclosed quote");
        assert_eq!(quote_hits.len(), 1);
        assert_eq!(quote_hits[0].agent_id, "ag_path");

        // Punctuation-only & FTS operator queries never error
        assert!(
            store
                .search_replica_messages("--- /// \"\"\"", 10)
                .expect("punct")
                .is_empty()
        );
        assert!(
            store
                .search_replica_messages("AND OR NOT NEAR * ^", 10)
                .expect("ops")
                .is_empty()
        );

        // Tool output search
        let tool_hits = store
            .search_replica_messages("zephyr_token", 10)
            .expect("tool output");
        assert_eq!(tool_hits.len(), 1);
        assert_eq!(tool_hits[0].role, "tool");
        assert_eq!(tool_hits[0].seq, 2);
    }

    #[test]
    fn fts5_bm25_ordering_and_per_agent_deduplication() {
        let store = OrbitStore::open_in_memory().expect("open store");

        // ag_low mentions "quasar" once in a long unrelated paragraph
        store
            .upsert_replica_timeline_event_for_host(
                "host_1",
                &mk_replica_event(
                    "ag_low",
                    1,
                    TimelinePayload::AssistantMessage {
                        item_id: "m_low".into(),
                        text: "This is a lengthy architecture discussion about UI layout, caching, networking, terminal rendering, keyboard shortcuts, and only briefly mentions quasar once at the end.".into(),
                        streaming: false,
                    },
                    0,
                ),
            )
            .expect("low");

        // ag_high mentions "quasar" densely across multiple messages (should dedupe to 1 hit & rank #1)
        store
            .upsert_replica_timeline_events_batch_for_host(
                "host_1",
                &[
                    mk_replica_event(
                        "ag_high",
                        1,
                        TimelinePayload::UserPrompt {
                            message_id: "u_hi".into(),
                            text: "Debug quasar engine quasar scheduler quasar".into(),
                            badges: Vec::new(),
                        },
                        1,
                    ),
                    mk_replica_event(
                        "ag_high",
                        2,
                        TimelinePayload::AssistantMessage {
                            item_id: "a_hi".into(),
                            text: "The quasar scheduler is fixed now.".into(),
                            streaming: false,
                        },
                        2,
                    ),
                ],
            )
            .expect("high");

        let hits = store.search_replica_messages("quasar", 10).expect("search");
        assert_eq!(hits.len(), 2, "must deduplicate to 1 hit per agent");
        assert_eq!(
            hits[0].agent_id, "ag_high",
            "higher BM25 density conversation must rank first"
        );
        assert_eq!(hits[0].seq, 1);
        assert_eq!(hits[1].agent_id, "ag_low");
        assert!(hits[0].rank <= hits[1].rank);
    }

    #[test]
    fn fts5_syncs_coalescing_rewind_delete_and_daemon_purge() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_fts".into(),
                title: "fts".into(),
                cwd: "/tmp/fts".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        let ag = mk_agent("ag_rw", "ws_fts", "Rewindable");
        store.upsert_agent(&ag, None).expect("agent");
        store
            .upsert_single_replica_agent("host_1", &ag)
            .expect("rep agent");

        // Streaming chunk coalescing on daemon & replica: superseded phrase disappears from FTS
        let p1 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t1"),
                TimelinePayload::UserPrompt {
                    message_id: "u1".into(),
                    text: "Keep alpha_keep_token in turn one".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p1");
        let chunk1 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "stream_1".into(),
                    text: "draft_obsolete_phrase".into(),
                    streaming: true,
                },
            )
            .expect("chunk1");
        let chunk2 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t1"),
                TimelinePayload::AssistantMessage {
                    item_id: "stream_1".into(),
                    text: "final_confirmed_phrase".into(),
                    streaming: false,
                },
            )
            .expect("chunk2");
        let p2 = store
            .append_timeline_event(
                "ag_rw",
                1,
                Some("t2"),
                TimelinePayload::UserPrompt {
                    message_id: "u2".into(),
                    text: "Turn two beta_rewound_token".into(),
                    badges: Vec::new(),
                },
            )
            .expect("p2");

        // Mirror to replica too
        store
            .upsert_replica_timeline_event_for_host("host_1", &p1)
            .expect("rep p1");
        store
            .upsert_replica_timeline_event_for_host("host_1", &chunk1)
            .expect("rep chunk1");
        store
            .upsert_replica_timeline_event_for_host("host_1", &chunk2)
            .expect("rep chunk2");
        store
            .upsert_replica_timeline_event_for_host("host_1", &p2)
            .expect("rep p2");

        // Superseded streaming chunk is gone from both daemon and replica FTS
        assert!(
            store
                .search_daemon_messages("draft_obsolete_phrase", 5)
                .expect("daemon obsolete")
                .is_empty()
        );
        assert!(
            store
                .search_replica_messages("draft_obsolete_phrase", 5)
                .expect("replica obsolete")
                .is_empty()
        );
        assert_eq!(
            store
                .search_daemon_messages("final_confirmed_phrase", 5)
                .expect("daemon final")
                .len(),
            1
        );
        assert_eq!(
            store
                .search_replica_messages("final_confirmed_phrase", 5)
                .expect("replica final")
                .len(),
            1
        );

        // Daemon rewind removes turn 2 from daemon FTS while preserving turn 1
        let rewound = store
            .rewind_agent_timeline("ag_rw", p2.seq)
            .expect("rewind")
            .expect("some");
        assert!(
            store
                .search_daemon_messages("beta_rewound_token", 5)
                .expect("daemon after rewind")
                .is_empty()
        );
        assert_eq!(
            store
                .search_daemon_messages("alpha_keep_token", 5)
                .expect("daemon keep")
                .len(),
            1
        );

        // Replica rewind (`replace_replica_timeline_for_host`) removes turn 2 from replica FTS
        store
            .replace_replica_timeline_for_host("host_1", "ag_rw", &rewound.events)
            .expect("replace replica");
        assert!(
            store
                .search_replica_messages("beta_rewound_token", 5)
                .expect("replica after rewind")
                .is_empty()
        );
        assert_eq!(
            store
                .search_replica_messages("alpha_keep_token", 5)
                .expect("replica keep")
                .len(),
            1
        );

        // Deleting replica agent purges replica FTS
        store
            .delete_replica_agent("host_1", "ag_rw")
            .expect("delete replica agent");
        assert!(
            store
                .search_replica_messages("alpha_keep_token", 5)
                .expect("replica after delete")
                .is_empty()
        );

        // Deleting daemon agent purges daemon FTS
        store.delete_agent("ag_rw").expect("delete daemon agent");
        assert!(
            store
                .search_daemon_messages("alpha_keep_token", 5)
                .expect("daemon after delete")
                .is_empty()
        );
    }

    #[test]
    fn fts5_incremental_batched_backfill_indexes_legacy_rows() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag = mk_agent("ag_legacy", "ws_1", "Legacy");
        store
            .upsert_single_replica_agent("host_legacy", &ag)
            .expect("rep agent");

        // Simulate pre-v6 rows inserted directly into client_replica_events without FTS rows
        {
            let conn = store.conn.lock().expect("lock");
            for i in 1..=5u64 {
                let payload = serde_json::to_string(&TimelinePayload::UserPrompt {
                    message_id: format!("m_{i}"),
                    text: format!("legacy_backfill_item_{i} 历史消息回填"),
                    badges: Vec::new(),
                })
                .expect("json");
                conn.execute(
                    "INSERT INTO client_replica_events (agent_id, seq, epoch, turn_id, timestamp, payload_json, event_type)
                     VALUES (?1, ?2, 1, NULL, ?3, ?4, 'user_prompt')",
                    params!["ag_legacy", i as i64, Utc::now().to_rfc3339(), payload],
                )
                .expect("raw insert");
            }
            write_kv(&conn, KvTable::Client, KV_REPLICA_LAST_ROWID, "0").expect("kv reset");
            write_kv(&conn, KvTable::Client, KV_REPLICA_DONE, "false").expect("kv flag");
        }

        assert!(!store.is_replica_fts_backfill_done());
        assert!(
            store
                .search_replica_messages("legacy_backfill_item_5", 5)
                .expect("before backfill")
                .is_empty()
        );

        // Batch 1 (size 3)
        let p1 = store.backfill_replica_fts_batch(3).expect("batch 1");
        assert_eq!(p1.scanned, 3);
        assert_eq!(p1.indexed, 3);
        assert!(!p1.done);
        assert_eq!(
            store
                .search_replica_messages("legacy_backfill_item_2", 5)
                .expect("after batch 1")
                .len(),
            1
        );

        // Batch 2 (remaining 2 rows -> marks done)
        let p2 = store.backfill_replica_fts_batch(3).expect("batch 2");
        assert_eq!(p2.scanned, 2);
        assert_eq!(p2.indexed, 2);
        assert!(p2.done);
        assert!(store.is_replica_fts_backfill_done());

        let hits = store
            .search_replica_messages("legacy_backfill_item_5", 5)
            .expect("after batch 2");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].host_id, "host_legacy");
        assert_eq!(hits[0].seq, 5);
    }

    #[test]
    fn fts5_special_chars_underscores_japanese_korean_and_edge_cases() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag_jp = mk_agent("ag_jp", "ws_1", "日本語テスト");
        let ag_kr = mk_agent("ag_kr", "ws_1", "한국어 테스트");
        let ag_edge = mk_agent("ag_edge", "ws_1", "Edge Cases");
        store
            .upsert_replica_agents("host_intl", &[ag_jp, ag_kr, ag_edge])
            .expect("upsert intl agents");

        store
            .upsert_replica_timeline_events_batch_for_host(
                "host_intl",
                &[
                    mk_replica_event(
                        "ag_jp",
                        1,
                        TimelinePayload::UserPrompt {
                            message_id: "jp1".into(),
                            text: "サーバー接続エラーの原因を調査してください".into(),
                            badges: Vec::new(),
                        },
                        0,
                    ),
                    mk_replica_event(
                        "ag_kr",
                        1,
                        TimelinePayload::AssistantMessage {
                            item_id: "kr1".into(),
                            text: "데이터베이스 연결 시간 초과 문제를 수정했습니다".into(),
                            streaming: false,
                        },
                        1,
                    ),
                    mk_replica_event(
                        "ag_edge",
                        1,
                        TimelinePayload::AssistantMessage {
                            item_id: "e1".into(),
                            text: "Guard against <mark>injected</mark> and <script>alert(1)</script> in snake_case_identifier.".into(),
                            streaming: false,
                        },
                        2,
                    ),
                ],
            )
            .expect("batch intl");

        // Underscore-only and FTS5 operator / column-filter / punctuation queries must NEVER error
        for tricky in [
            "",
            "   ",
            "_",
            "___",
            "foo __ bar",
            "body:injected",
            "nonexistent_col:foo",
            "(foo OR bar) AND NOT baz",
            "-excluded +required ^start",
            "NEAR/3(foo bar)",
            "\"\"\"***((())):::",
            "a*b?c~d",
            "①② ² ½",
            "test's \"quoted\" \\backslash\\",
        ] {
            store
                .search_replica_messages(tricky, 10)
                .unwrap_or_else(|e| panic!("FTS5 query {tricky:?} must never fail: {e}"));
        }

        // Snake-case identifiers match both full identifier and sub-tokens cleanly
        assert_eq!(
            build_fts5_match_query("___"),
            None,
            "underscore-only query must produce None"
        );
        assert_eq!(
            build_fts5_match_query("snake_case_identifier"),
            Some("\"snake case identifier\"*".into())
        );
        let snake_hits = store
            .search_replica_messages("snake_case_identifier", 10)
            .expect("snake full");
        assert_eq!(snake_hits.len(), 1);
        assert_eq!(snake_hits[0].agent_id, "ag_edge");
        assert!(
            snake_hits[0]
                .snippet
                .contains("<mark>snake_case_identifier</mark>"),
            "expected full highlighted snake_case_identifier: {}",
            snake_hits[0].snippet
        );
        // Literal <mark> tags from message body are stripped before highlighting
        assert!(
            !snake_hits[0].snippet.contains("<mark>injected</mark>"),
            "literal <mark> tags from body must not leak as highlights: {}",
            snake_hits[0].snippet
        );
        let sub_token_hits = store
            .search_replica_messages("identifier", 10)
            .expect("snake subtoken");
        assert_eq!(sub_token_hits.len(), 1);
        assert_eq!(sub_token_hits[0].agent_id, "ag_edge");

        // Japanese (Katakana + Kanji + Hiragana) search + snippet highlight
        let jp_hits = store
            .search_replica_messages("サーバー接続エラー", 10)
            .expect("jp search");
        assert_eq!(jp_hits.len(), 1);
        assert_eq!(jp_hits[0].agent_id, "ag_jp");
        assert!(
            jp_hits[0]
                .snippet
                .contains("<mark>サーバー接続エラー</mark>"),
            "expected highlighted Japanese snippet: {}",
            jp_hits[0].snippet
        );

        // Korean (Hangul) search + snippet highlight
        let kr_hits = store
            .search_replica_messages("데이터베이스 연결", 10)
            .expect("kr search");
        assert_eq!(kr_hits.len(), 1);
        assert_eq!(kr_hits[0].agent_id, "ag_kr");
        assert!(
            kr_hits[0].snippet.contains("<mark>데이터베이스</mark>")
                && kr_hits[0].snippet.contains("<mark>연결</mark>"),
            "expected highlighted Korean snippet: {}",
            kr_hits[0].snippet
        );

        // limit == 0 always returns empty
        assert!(
            store
                .search_replica_messages("snake_case_identifier", 0)
                .expect("limit zero")
                .is_empty()
        );
    }

    #[test]
    fn fts5_multi_host_filtering_and_host_purge_cleanup() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag_a = mk_agent("ag_a", "ws_a", "Host A Session");
        let ag_b = mk_agent("ag_b", "ws_b", "Host B Session");
        store
            .upsert_replica_agents("host_alpha", &[ag_a])
            .expect("agents alpha");
        store
            .upsert_replica_agents("host_beta", &[ag_b])
            .expect("agents beta");

        store
            .upsert_replica_timeline_event_for_host(
                "host_alpha",
                &mk_replica_event(
                    "ag_a",
                    1,
                    TimelinePayload::UserPrompt {
                        message_id: "m_a".into(),
                        text: "Deploy orbit_multihost_beacon to alpha cluster".into(),
                        badges: Vec::new(),
                    },
                    0,
                ),
            )
            .expect("ev a");
        store
            .upsert_replica_timeline_event_for_host(
                "host_beta",
                &mk_replica_event(
                    "ag_b",
                    1,
                    TimelinePayload::UserPrompt {
                        message_id: "m_b".into(),
                        text: "Deploy orbit_multihost_beacon to beta cluster".into(),
                        badges: Vec::new(),
                    },
                    1,
                ),
            )
            .expect("ev b");
        // Also insert an event for an agent not in client_replica_agents to verify orphan cleanup
        store
            .upsert_replica_timeline_event_for_host(
                "host_beta",
                &mk_replica_event(
                    "ag_beta_orphan",
                    1,
                    TimelinePayload::AssistantMessage {
                        item_id: "o1".into(),
                        text: "Orphaned orbit_multihost_beacon event".into(),
                        streaming: false,
                    },
                    2,
                ),
            )
            .expect("ev orphan");

        // Unfiltered search spans both hosts
        let all_hits = store
            .search_replica_messages("orbit_multihost_beacon", 10)
            .expect("all hosts");
        assert_eq!(all_hits.len(), 3);

        // Host-filtered search returns only matching host
        let alpha_hits = store
            .search_replica_messages_for_host("orbit_multihost_beacon", Some("host_alpha"), 10)
            .expect("alpha only");
        assert_eq!(alpha_hits.len(), 1);
        assert_eq!(alpha_hits[0].host_id, "host_alpha");
        assert_eq!(alpha_hits[0].agent_id, "ag_a");

        let beta_hits = store
            .search_replica_messages_for_host("orbit_multihost_beacon", Some("host_beta"), 10)
            .expect("beta only");
        assert_eq!(beta_hits.len(), 2);
        assert!(beta_hits.iter().all(|h| h.host_id == "host_beta"));

        assert!(
            store
                .search_replica_messages_for_host(
                    "orbit_multihost_beacon",
                    Some("host_unknown"),
                    10
                )
                .expect("unknown host")
                .is_empty()
        );

        // Clearing host_beta removes both tracked and orphaned FTS rows without affecting host_alpha
        store
            .clear_host_replica("host_beta")
            .expect("clear host_beta");
        assert!(
            store
                .search_replica_messages_for_host("orbit_multihost_beacon", Some("host_beta"), 10)
                .expect("after clear beta")
                .is_empty()
        );
        let remaining = store
            .search_replica_messages("orbit_multihost_beacon", 10)
            .expect("remaining");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].host_id, "host_alpha");

        // Deleting host_alpha removes remaining FTS rows
        store
            .delete_client_host("host_alpha")
            .expect("delete host_alpha");
        assert!(
            store
                .search_replica_messages("orbit_multihost_beacon", 10)
                .expect("after delete alpha")
                .is_empty()
        );
    }

    #[test]
    fn fts5_daemon_batched_backfill_indexes_legacy_rows() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_d".into(),
                title: "daemon".into(),
                cwd: "/tmp/daemon".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        store
            .upsert_agent(&mk_agent("ag_d", "ws_d", "Daemon Legacy"), None)
            .expect("agent");

        {
            let conn = store.conn.lock().expect("lock");
            for i in 1..=4u64 {
                let payload = serde_json::to_string(&TimelinePayload::AssistantMessage {
                    item_id: format!("d_{i}"),
                    text: format!("daemon_legacy_record_{i} 守护进程回填"),
                    streaming: false,
                })
                .expect("json");
                conn.execute(
                    "INSERT INTO timeline_events (epoch, agent_id, turn_id, timestamp, payload_json, event_type)
                     VALUES (1, 'ag_d', NULL, ?1, ?2, 'assistant_message')",
                    params![now.to_rfc3339(), payload],
                )
                .expect("raw daemon insert");
            }
            write_kv(&conn, KvTable::Host, KV_DAEMON_LAST_ROWID, "0").expect("kv reset");
            write_kv(&conn, KvTable::Host, KV_DAEMON_DONE, "false").expect("kv flag");
        }

        assert!(!store.is_daemon_fts_backfill_done());
        let p1 = store.backfill_daemon_fts_batch(2).expect("d batch 1");
        assert_eq!(p1.scanned, 2);
        assert_eq!(p1.indexed, 2);
        assert!(!p1.done);

        let p2 = store.backfill_daemon_fts_batch(3).expect("d batch 2");
        assert_eq!(p2.scanned, 2);
        assert_eq!(p2.indexed, 2);
        assert!(p2.done);
        assert!(store.is_daemon_fts_backfill_done());

        let hits = store
            .search_daemon_messages("守护进程回填", 5)
            .expect("daemon cjk");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].agent_id, "ag_d");
    }

    #[test]
    fn fts5_schema_v6_reopen_preserves_user_version_and_backfill_cursors() {
        let unique = format!(
            "orbit_fts5_reopen_{}_{}.sqlite",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        );
        let db_path = std::env::temp_dir().join(unique);
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
                let _ = std::fs::remove_file(format!("{}-wal", self.0.display()));
                let _ = std::fs::remove_file(format!("{}-shm", self.0.display()));
            }
        }
        let _guard = Cleanup(db_path.clone());

        {
            let store = OrbitStore::open(&db_path).expect("initial open");
            let ver: i64 = store
                .conn
                .lock()
                .expect("lock")
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("user_version");
            assert_eq!(ver, 6, "fresh DB must be migrated to schema v6");
            assert!(store.is_replica_fts_backfill_done());
            assert!(store.is_daemon_fts_backfill_done());

            let ag = mk_agent("ag_reopen", "ws_1", "Reopen Session");
            store
                .upsert_replica_agents("host_persist", &[ag])
                .expect("upsert agent");
            store
                .upsert_replica_timeline_event_for_host(
                    "host_persist",
                    &mk_replica_event(
                        "ag_reopen",
                        1,
                        TimelinePayload::UserPrompt {
                            message_id: "m1".into(),
                            text: "persistent_reopen_fts_token".into(),
                            badges: Vec::new(),
                        },
                        0,
                    ),
                )
                .expect("upsert event");
        }

        // Reopen file-backed DB: orchestration migration must NOT downgrade user_version below 6
        {
            let store = OrbitStore::open(&db_path).expect("reopen 1");
            let ver: i64 = store
                .conn
                .lock()
                .expect("lock")
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("user_version");
            assert_eq!(ver, 6, "reopened DB must remain at schema v6");
            assert!(
                store.is_replica_fts_backfill_done(),
                "replica backfill done flag must persist across reopen"
            );
            assert!(store.is_daemon_fts_backfill_done());
            let hits = store
                .search_replica_messages("persistent_reopen_fts_token", 5)
                .expect("search after reopen");
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].host_id, "host_persist");

            // Simulate an in-progress partial backfill cursor and reopen again: must NOT reset to 0
            {
                let conn = store.conn.lock().expect("lock");
                write_kv(&conn, KvTable::Client, KV_REPLICA_LAST_ROWID, "42").expect("cursor");
                write_kv(&conn, KvTable::Client, KV_REPLICA_DONE, "false").expect("done false");
            }
        }

        {
            let store = OrbitStore::open(&db_path).expect("reopen 2");
            let conn = store.conn.lock().expect("lock");
            let ver: i64 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .expect("user_version");
            assert_eq!(ver, 6);
            assert_eq!(
                read_kv(&conn, KvTable::Client, KV_REPLICA_LAST_ROWID)
                    .expect("read cursor")
                    .as_deref(),
                Some("42"),
                "existing backfill cursor must never be overwritten on reopen"
            );
        }
    }

    #[test]
    fn fts5_nfd_combining_diacritics_and_nested_mark_spoofing() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag = mk_agent("ag_nfd", "ws_1", "NFD Diacritics");
        store
            .upsert_replica_agents("host_nfd", &[ag])
            .expect("agents");
        store
            .upsert_replica_timeline_event_for_host(
                "host_nfd",
                &mk_replica_event(
                    "ag_nfd",
                    1,
                    TimelinePayload::AssistantMessage {
                        item_id: "a_nfd".into(),
                        text: "Review résumé and <ma<mark>rk>spoofed</ma</mark>rk> alongside a 数据库 query".into(),
                        streaming: false,
                    },
                    0,
                ),
            )
            .expect("ev");

        // macOS NFD decomposed "re\u{0301}sume\u{0301}" matches NFC "résumé" via unicode61 remove_diacritics 2
        let nfd_query = "re\u{0301}sume\u{0301}";
        assert_eq!(
            build_fts5_match_query(nfd_query),
            Some(format!("\"{nfd_query}\"*"))
        );
        let hits = store
            .search_replica_messages(nfd_query, 10)
            .expect("nfd search");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].agent_id, "ag_nfd");

        // Standalone combining marks safely produce None
        assert_eq!(build_fts5_match_query("\u{0301}\u{0302}"), None);

        // Nested <ma<mark>rk> spoofing is stripped to convergence
        assert_eq!(
            strip_literal_mark_tags("<ma<mark>rk>spoofed</ma</mark>rk>"),
            "spoofed"
        );
        // Searching for another word in the same message must NOT leave spoofed <mark> around "spoofed"
        assert!(
            !hits[0].snippet.contains("<mark>spoofed</mark>"),
            "nested mark spoof must not leak into snippet: {}",
            hits[0].snippet
        );
        let spoof_hits = store
            .search_replica_messages("spoofed", 10)
            .expect("spoof search");
        assert_eq!(spoof_hits.len(), 1);
        assert!(
            spoof_hits[0]
                .snippet
                .contains("and <mark>spoofed</mark> alongside"),
            "expected clean stripped snippet around matched word: {}",
            spoof_hits[0].snippet
        );

        // Single-char ASCII word paired with CJK does not spam single-letter highlights
        let cjk_hits = store
            .search_replica_messages("a 数据库", 10)
            .expect("cjk + single ascii");
        assert_eq!(cjk_hits.len(), 1);
        assert!(cjk_hits[0].snippet.contains("<mark>数据库</mark>"));
        assert!(
            !cjk_hits[0].snippet.contains("<mark>a</mark>"),
            "single-char ASCII should not spam highlights when substantial terms exist: {}",
            cjk_hits[0].snippet
        );
    }

    #[test]
    fn fts5_window_cte_prevents_chatty_thread_candidate_starvation() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_starve".into(),
                title: "starve".into(),
                cwd: "/tmp/starve".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");
        let ag_chatty = mk_agent("ag_chatty", "ws_starve", "Chatty Thread");
        let ag_quiet = mk_agent("ag_quiet", "ws_starve", "Quiet Thread");
        store.upsert_agent(&ag_chatty, None).expect("d chatty");
        store.upsert_agent(&ag_quiet, None).expect("d quiet");
        store
            .upsert_replica_agents("host_starve", &[ag_chatty, ag_quiet])
            .expect("rep agents");

        // 60 dense matches in ag_chatty + 1 sparse match in ag_quiet
        let mut replica_events = Vec::with_capacity(61);
        for i in 1..=60u64 {
            let payload = TimelinePayload::UserPrompt {
                message_id: format!("c_{i}"),
                text: format!("starvation_beacon starvation_beacon turn {i}"),
                badges: Vec::new(),
            };
            store
                .append_timeline_event("ag_chatty", 1, None, payload.clone())
                .expect("daemon chatty");
            replica_events.push(mk_replica_event("ag_chatty", i, payload, i as i64));
        }
        let quiet_payload = TimelinePayload::AssistantMessage {
            item_id: "q_1".into(),
            text: "This long unrelated message mentions starvation_beacon only once among many other unrelated words about layout, themes, networking, terminals, diffs, and worktrees.".into(),
            streaming: false,
        };
        store
            .append_timeline_event("ag_quiet", 1, None, quiet_payload.clone())
            .expect("daemon quiet");
        replica_events.push(mk_replica_event("ag_quiet", 1, quiet_payload, 100));

        store
            .upsert_replica_timeline_events_batch_for_host("host_starve", &replica_events)
            .expect("rep batch");

        let rep_hits = store
            .search_replica_messages("starvation_beacon", 5)
            .expect("replica search");
        assert_eq!(
            rep_hits.len(),
            2,
            "chatty thread with 60 matches must not starve quiet thread"
        );
        assert!(rep_hits.iter().any(|h| h.agent_id == "ag_chatty"));
        assert!(rep_hits.iter().any(|h| h.agent_id == "ag_quiet"));

        let daemon_hits = store
            .search_daemon_messages("starvation_beacon", 5)
            .expect("daemon search");
        assert_eq!(daemon_hits.len(), 2);
        assert!(daemon_hits.iter().any(|h| h.agent_id == "ag_chatty"));
        assert!(daemon_hits.iter().any(|h| h.agent_id == "ag_quiet"));
    }

    #[test]
    fn fts5_streaming_deferred_and_snapshot_pruning_removes_deleted_agents() {
        let store = OrbitStore::open_in_memory().expect("open store");
        let ag_keep = mk_agent("ag_keep", "ws_1", "Keep Me");
        let ag_drop = mk_agent("ag_drop", "ws_1", "Drop Me");
        store
            .upsert_replica_agents("host_snap", &[ag_keep.clone(), ag_drop])
            .expect("seed 2 agents");

        // Streaming assistant chunk is deferred (not indexed until finalized)
        store
            .upsert_replica_timeline_event_for_host(
                "host_snap",
                &mk_replica_event(
                    "ag_keep",
                    1,
                    TimelinePayload::AssistantMessage {
                        item_id: "s1".into(),
                        text: "ephemeral_streaming_token".into(),
                        streaming: true,
                    },
                    0,
                ),
            )
            .expect("streaming ev");
        assert!(
            store
                .search_replica_messages("ephemeral_streaming_token", 5)
                .expect("streaming skipped")
                .is_empty()
        );

        // Finalized chunk with same item_id IS indexed
        store
            .upsert_replica_timeline_event_for_host(
                "host_snap",
                &mk_replica_event(
                    "ag_keep",
                    2,
                    TimelinePayload::AssistantMessage {
                        item_id: "s1".into(),
                        text: "settled_final_token".into(),
                        streaming: false,
                    },
                    1,
                ),
            )
            .expect("final ev");
        assert_eq!(
            store
                .search_replica_messages("settled_final_token", 5)
                .expect("final indexed")
                .len(),
            1
        );

        // Seed event for ag_drop, then replace snapshot without ag_drop -> pruned from FTS & events
        store
            .upsert_replica_timeline_event_for_host(
                "host_snap",
                &mk_replica_event(
                    "ag_drop",
                    1,
                    TimelinePayload::UserPrompt {
                        message_id: "d1".into(),
                        text: "snapshot_pruned_agent_token".into(),
                        badges: Vec::new(),
                    },
                    2,
                ),
            )
            .expect("drop ev");
        assert_eq!(
            store
                .search_replica_messages("snapshot_pruned_agent_token", 5)
                .expect("before prune")
                .len(),
            1
        );

        store
            .upsert_replica_agents("host_snap", &[ag_keep])
            .expect("prune snapshot");
        assert!(
            store
                .search_replica_messages("snapshot_pruned_agent_token", 5)
                .expect("after prune")
                .is_empty(),
            "agents dropped from AgentsSnapshot must be pruned from FTS"
        );
        assert!(
            store
                .fetch_replica_timeline("ag_drop", 10)
                .expect("events pruned")
                .is_empty()
        );
        assert_eq!(
            store
                .search_replica_messages("settled_final_token", 5)
                .expect("kept agent intact")
                .len(),
            1
        );
    }
}
