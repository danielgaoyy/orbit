//! Pure-Rust fast workspace symbol indexer, fuzzy scorer, and canonical snippet reader.
//!
//! Powers instantaneous out-of-the-box `@symbol` fuzzy code citation across any repository
//! without external Node.js/Python/LSP dependencies, while exposing [`ExternalSymbolProvider`]
//! so live LSP `workspace/symbol` engines (Epic #16) can seamlessly merge and enrich results.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File};
use std::future::Future;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use orbit_protocol::WorkspaceSymbolEntry;

use crate::workspace_files::{canonicalize_workspace_root, is_path_inside_git};

/// Hard upper bound on source files indexed per workspace.
pub const MAX_INDEXED_FILES: usize = 4_000;
/// Hard upper bound on total symbols cached per workspace.
pub const MAX_INDEXED_SYMBOLS: usize = 40_000;
/// Skip individual files larger than 512 KiB.
pub const MAX_FILE_BYTES: u64 = 512 * 1024;
/// Maximum `.gitignore` file size read per directory (256 KiB).
const MAX_GITIGNORE_BYTES: u64 = 256 * 1024;
/// Default number of ranked symbol hits returned when `limit == 0`.
pub const DEFAULT_SEARCH_LIMIT: usize = 30;
/// Hard cap on returned symbols per query.
pub const HARD_MAX_SEARCH_LIMIT: usize = 100;
/// Maximum cached workspaces held in memory simultaneously.
const MAX_CACHED_WORKSPACES: usize = 16;
/// In-memory index TTL before re-checking directory / `.git/index` mtime or re-indexing.
const INDEX_CACHE_TTL: Duration = Duration::from_secs(15);
/// Maximum lines scanned forward to close a symbol's body span.
const MAX_SYMBOL_SPAN_LINES: usize = 250;
/// Default maximum snippet size returned by [`read_symbol_snippet`].
pub const DEFAULT_SNIPPET_MAX_BYTES: usize = 8 * 1024;
/// Bounded timeout for optional external (LSP) symbol enrichment so lexical autocomplete stays instant.
const EXTERNAL_PROVIDER_TIMEOUT: Duration = Duration::from_millis(800);

const SKIPPED_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".turbo",
    ".cache",
    ".parcel-cache",
    ".svelte-kit",
    "vendor",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".venv",
    "venv",
    "out",
    "coverage",
    ".idea",
    ".vscode",
    ".gradle",
    "Pods",
    "DerivedData",
    ".dart_tool",
];

/// Result of a workspace symbol search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolSearchResult {
    pub symbols: Vec<WorkspaceSymbolEntry>,
    pub truncated: bool,
}

/// Extensible hook for live symbol providers (e.g. LSP `workspace/symbol` in Epic #16).
///
/// Registered providers are queried alongside the pure-Rust lexical index and their hits
/// are merged and deduplicated with priority via [`merge_external_symbols`].
pub trait ExternalSymbolProvider: Send + Sync {
    fn search_symbols<'a>(
        &'a self,
        cwd: &'a Path,
        query: &'a str,
        limit: usize,
    ) -> Pin<Box<dyn Future<Output = Vec<WorkspaceSymbolEntry>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
struct IndexedSymbol {
    entry: WorkspaceSymbolEntry,
    name_lower: String,
    qualified_lower: String,
    path_lower: String,
}

#[derive(Debug)]
struct CachedWorkspaceIndex {
    symbols: Arc<Vec<IndexedSymbol>>,
    truncated: bool,
    built_at: Instant,
    last_used_ms: AtomicU64,
    root_mtime: Option<SystemTime>,
    git_index_mtime: Option<SystemTime>,
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Thread-safe, bounded, TTL + mtime-invalidated workspace symbol index.
#[derive(Clone, Default)]
pub struct WorkspaceSymbolIndex {
    cache: Arc<RwLock<HashMap<PathBuf, CachedWorkspaceIndex>>>,
    external_provider: Arc<RwLock<Option<Arc<dyn ExternalSymbolProvider>>>>,
}

impl fmt::Debug for WorkspaceSymbolIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let cached_workspaces = self.cache.read().map(|g| g.len()).unwrap_or(0);
        let has_external_provider = self
            .external_provider
            .read()
            .map(|g| g.is_some())
            .unwrap_or(false);
        f.debug_struct("WorkspaceSymbolIndex")
            .field("cached_workspaces", &cached_workspaces)
            .field("has_external_provider", &has_external_provider)
            .finish()
    }
}

impl WorkspaceSymbolIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an index pre-configured with an external symbol provider (e.g. LSP bridge).
    pub fn with_external_provider(provider: Arc<dyn ExternalSymbolProvider>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            external_provider: Arc::new(RwLock::new(Some(provider))),
        }
    }

    /// Attach or replace an external symbol provider at runtime (e.g. when LSP activates).
    pub fn set_external_provider(&self, provider: Option<Arc<dyn ExternalSymbolProvider>>) {
        if let Ok(mut slot) = self.external_provider.write() {
            *slot = provider;
        }
    }

    /// Invalidate any cached symbol index for `cwd` as well as parent/nested cached roots
    /// (called after workspace file mutations).
    pub fn invalidate(&self, cwd: &Path) {
        let key = crate::platform::canonicalize_clean(cwd).unwrap_or_else(|_| cwd.to_path_buf());
        if let Ok(mut guard) = self.cache.write() {
            guard.retain(|k, _| !(k == &key || key.starts_with(k) || k.starts_with(&key)));
        }
    }

    /// Search workspace symbols asynchronously without blocking Tokio worker threads.
    pub async fn search(
        &self,
        cwd: &Path,
        query: &str,
        limit: usize,
    ) -> Result<SymbolSearchResult, String> {
        let effective_limit = if limit == 0 {
            DEFAULT_SEARCH_LIMIT
        } else {
            limit.min(HARD_MAX_SEARCH_LIMIT)
        };
        let cwd_owned = cwd.to_path_buf();
        let query_owned = query.to_string();
        let cache = Arc::clone(&self.cache);

        let (local_result, canonical_cwd) = tokio::task::spawn_blocking(move || {
            search_local_index_sync(&cache, &cwd_owned, &query_owned, effective_limit)
        })
        .await
        .map_err(|_| "Workspace symbol search task cancelled".to_string())??;

        let external = self
            .external_provider
            .read()
            .ok()
            .and_then(|slot| slot.clone());

        if let Some(provider) = external
            && let Ok(extra) = tokio::time::timeout(
                EXTERNAL_PROVIDER_TIMEOUT,
                provider.search_symbols(&canonical_cwd, query, effective_limit),
            )
            .await
            && !extra.is_empty()
        {
            let merged =
                merge_external_symbols(extra, local_result.symbols, query, effective_limit);
            return Ok(SymbolSearchResult {
                symbols: merged,
                truncated: local_result.truncated,
            });
        }

        Ok(local_result)
    }
}

/// Merge external (e.g. live LSP) symbol hits with pure-Rust lexical index hits,
/// deduplicating by `(path, name, start_line)` and re-ranking cleanly for `query`.
pub fn merge_external_symbols(
    external: Vec<WorkspaceSymbolEntry>,
    lexical: Vec<WorkspaceSymbolEntry>,
    query: &str,
    limit: usize,
) -> Vec<WorkspaceSymbolEntry> {
    let max_items = if limit == 0 {
        DEFAULT_SEARCH_LIMIT
    } else {
        limit.min(HARD_MAX_SEARCH_LIMIT)
    };
    let mut seen: HashSet<(String, String, u32)> = HashSet::new();
    let mut scored: Vec<(i32, WorkspaceSymbolEntry)> = Vec::new();
    let q_clean = query.trim();

    // External (LSP) symbols receive a small tie-break boost when equally relevant.
    for entry in external {
        let key = (entry.path.clone(), entry.name.clone(), entry.start_line);
        if !seen.insert(key) {
            continue;
        }
        let indexed = IndexedSymbol::from_entry(entry);
        let score = score_symbol_match(&indexed, q_clean);
        if score >= 0 {
            scored.push((score + 25, indexed.entry));
        }
    }

    for entry in lexical {
        let key = (entry.path.clone(), entry.name.clone(), entry.start_line);
        if !seen.insert(key) {
            continue;
        }
        let indexed = IndexedSymbol::from_entry(entry);
        let score = score_symbol_match(&indexed, q_clean);
        if score >= 0 {
            scored.push((score, indexed.entry));
        }
    }

    scored.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| a.name.len().cmp(&b.name.len()))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.start_line.cmp(&b.start_line))
    });
    scored.truncate(max_items);
    scored.into_iter().map(|(_, e)| e).collect()
}

/// Resolve `.git/index` modification timestamp, supporting both normal repositories
/// (`.git` directory) and git worktrees (`.git` file containing `gitdir: <path>`).
fn resolve_git_index_mtime(canonical_cwd: &Path) -> Option<SystemTime> {
    let dot_git = canonical_cwd.join(".git");
    let meta = fs::symlink_metadata(&dot_git).ok()?;
    if meta.file_type().is_symlink() {
        return None;
    }
    if meta.is_dir() {
        return fs::metadata(dot_git.join("index"))
            .ok()
            .and_then(|m| m.modified().ok());
    }
    if meta.is_file() && meta.len() <= 4096 {
        let mut buf = Vec::new();
        let file = File::open(&dot_git).ok()?;
        file.take(4097).read_to_end(&mut buf).ok()?;
        if buf.len() > 4096 {
            return None;
        }
        let text = String::from_utf8(buf).ok()?;
        let raw_dir = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("gitdir:"))?
            .trim();
        if raw_dir.is_empty() {
            return None;
        }
        let git_dir = Path::new(raw_dir);
        let resolved_git_dir = if git_dir.is_absolute() {
            git_dir.to_path_buf()
        } else {
            canonical_cwd.join(git_dir)
        };
        return fs::metadata(resolved_git_dir.join("index"))
            .ok()
            .and_then(|m| m.modified().ok());
    }
    None
}

fn search_local_index_sync(
    cache: &Arc<RwLock<HashMap<PathBuf, CachedWorkspaceIndex>>>,
    cwd: &Path,
    query: &str,
    limit: usize,
) -> Result<(SymbolSearchResult, PathBuf), String> {
    let canonical_cwd = canonicalize_workspace_root(cwd)?;
    let root_meta = fs::metadata(&canonical_cwd)
        .map_err(|e| format!("Workspace directory unavailable: {e}"))?;

    let root_mtime = root_meta.modified().ok();
    let git_index_mtime = resolve_git_index_mtime(&canonical_cwd);
    let now = Instant::now();
    let now_ms = now_unix_ms();

    // Fast path: read-lock Arc clone without copying 40k IndexedSymbol entries
    let snapshot = {
        let guard = cache
            .read()
            .map_err(|_| "Symbol index cache lock poisoned".to_string())?;
        if let Some(cached) = guard.get(&canonical_cwd) {
            let fresh_ttl = now.duration_since(cached.built_at) < INDEX_CACHE_TTL;
            let same_mtimes =
                cached.root_mtime == root_mtime && cached.git_index_mtime == git_index_mtime;
            if fresh_ttl && same_mtimes {
                cached.last_used_ms.store(now_ms, Ordering::Relaxed);
                Some((Arc::clone(&cached.symbols), cached.truncated))
            } else {
                None
            }
        } else {
            None
        }
    };

    let (symbols, truncated) = match snapshot {
        Some(hit) => hit,
        None => {
            let (raw_symbols, truncated) = build_workspace_index_sync(&canonical_cwd);
            let shared_symbols = Arc::new(raw_symbols);
            let entry = CachedWorkspaceIndex {
                symbols: Arc::clone(&shared_symbols),
                truncated,
                built_at: now,
                last_used_ms: AtomicU64::new(now_ms),
                root_mtime,
                git_index_mtime,
            };
            if let Ok(mut guard) = cache.write() {
                if guard.len() >= MAX_CACHED_WORKSPACES
                    && !guard.contains_key(&canonical_cwd)
                    && let Some(oldest_key) = guard
                        .iter()
                        .min_by_key(|(_, v)| v.last_used_ms.load(Ordering::Relaxed))
                        .map(|(k, _)| k.clone())
                {
                    guard.remove(&oldest_key);
                }
                guard.insert(canonical_cwd.clone(), entry);
            }
            (shared_symbols, truncated)
        }
    };

    let ranked = rank_indexed_symbols(&symbols, query, limit);
    Ok((
        SymbolSearchResult {
            symbols: ranked,
            truncated,
        },
        canonical_cwd,
    ))
}

impl IndexedSymbol {
    fn from_entry(entry: WorkspaceSymbolEntry) -> Self {
        let name_lower = entry.name.to_ascii_lowercase();
        let qualified_lower = match &entry.container {
            Some(c) if !c.is_empty() => format!("{}::{}", c.to_ascii_lowercase(), name_lower),
            _ => name_lower.clone(),
        };
        let path_lower = entry.path.to_ascii_lowercase();
        Self {
            entry,
            name_lower,
            qualified_lower,
            path_lower,
        }
    }
}

fn rank_indexed_symbols(
    symbols: &[IndexedSymbol],
    query: &str,
    limit: usize,
) -> Vec<WorkspaceSymbolEntry> {
    let q = query.trim();
    let mut scored: Vec<(i32, &IndexedSymbol)> = Vec::new();
    for sym in symbols {
        let score = score_symbol_match(sym, q);
        if score >= 0 {
            scored.push((score, sym));
        }
    }
    scored.sort_by(|(sa, a), (sb, b)| {
        sb.cmp(sa)
            .then_with(|| a.entry.name.len().cmp(&b.entry.name.len()))
            .then_with(|| a.entry.path.cmp(&b.entry.path))
            .then_with(|| a.entry.start_line.cmp(&b.entry.start_line))
    });
    scored.truncate(limit);
    scored.into_iter().map(|(_, s)| s.entry.clone()).collect()
}

fn kind_priority_bonus(kind: &str) -> i32 {
    match kind {
        "struct" | "class" | "trait" | "interface" | "enum" => 12,
        "fn" | "method" => 10,
        "type" => 8,
        "module" => 6,
        "const" => 4,
        _ => 2,
    }
}

/// Score a single indexed symbol against `query`. Returns `-1` when non-matching.
fn score_symbol_match(sym: &IndexedSymbol, raw_query: &str) -> i32 {
    let kind_bonus = kind_priority_bonus(&sym.entry.kind);
    if raw_query.is_empty() {
        return 100 + kind_bonus;
    }

    let q_lower = raw_query.to_ascii_lowercase();
    let name_score = score_identifier(&sym.entry.name, &sym.name_lower, raw_query, &q_lower);
    if name_score >= 0 {
        return name_score + kind_bonus;
    }

    // Support qualified queries like `Foo::bar`, `Foo.bar`, or path-qualified queries
    if q_lower.contains("::") || q_lower.contains('.') || q_lower.contains('/') {
        let normalized_q = q_lower.replace('.', "::");
        if sym.qualified_lower == normalized_q {
            return 920 + kind_bonus;
        }
        if sym.qualified_lower.starts_with(&normalized_q) {
            return 760 + kind_bonus;
        }
        if sym.qualified_lower.contains(&normalized_q) {
            return 600 + kind_bonus;
        }
        if let Some(sub) = fuzzy_subsequence_score(&sym.qualified_lower, &normalized_q) {
            return 340 + sub.min(200) + kind_bonus;
        }
        let combined = format!("{}:{}", sym.path_lower, sym.name_lower);
        if let Some(sub) = fuzzy_subsequence_score(&combined, &q_lower) {
            return 220 + sub.min(180) + kind_bonus;
        }
        return -1;
    }

    // Fallback: match against container::name when container carries the query
    if sym.qualified_lower != sym.name_lower {
        if sym.qualified_lower.contains(&q_lower) {
            return 420 + kind_bonus;
        }
        if let Some(sub) = fuzzy_subsequence_score(&sym.qualified_lower, &q_lower) {
            return 240 + sub.min(160) + kind_bonus;
        }
    }

    -1
}

/// Score an identifier with exact, prefix, camelCase/snake_case boundary, substring, and fuzzy subsequence tiers.
fn score_identifier(orig: &str, lower: &str, raw_q: &str, q_lower: &str) -> i32 {
    if orig == raw_q {
        return 1_000;
    }
    if lower == q_lower {
        return 950;
    }
    if orig.starts_with(raw_q) {
        let brevity = 40i32.saturating_sub((orig.len().saturating_sub(raw_q.len())) as i32);
        return 840 + brevity.max(0);
    }
    if lower.starts_with(q_lower) {
        let brevity = 40i32.saturating_sub((lower.len().saturating_sub(q_lower.len())) as i32);
        return 800 + brevity.max(0);
    }

    // Word-boundary / camelCase / snake_case acronym prefix match (e.g. `sws` -> `search_workspace_symbols`, `OC` -> `OmniComposer`)
    let initials = extract_word_initials_lower(orig);
    if !initials.is_empty() && initials.starts_with(q_lower) {
        let exact_acronym = if initials == q_lower { 40 } else { 0 };
        return 720 + exact_acronym;
    }

    if let Some(pos) = lower.find(q_lower) {
        let boundary_bonus = if pos == 0 || is_word_boundary(orig, pos) {
            50
        } else {
            0
        };
        let pos_penalty = (pos as i32).min(40);
        return 620 + boundary_bonus - pos_penalty;
    }

    // Fuzzy subsequence on the identifier with camelCase/snake_case boundary & consecutive run bonuses
    if let Some(sub_score) = fuzzy_subsequence_with_boundaries(orig, lower, q_lower) {
        return 320 + sub_score.min(280);
    }

    -1
}

fn is_word_boundary(orig: &str, byte_idx: usize) -> bool {
    if byte_idx == 0 {
        return true;
    }
    let bytes = orig.as_bytes();
    if byte_idx >= bytes.len() {
        return false;
    }
    let prev = bytes[byte_idx - 1];
    let cur = bytes[byte_idx];
    prev == b'_'
        || prev == b'-'
        || prev == b':'
        || prev == b'/'
        || (prev.is_ascii_lowercase() && cur.is_ascii_uppercase())
}

fn extract_word_initials_lower(ident: &str) -> String {
    let bytes = ident.as_bytes();
    let mut out = String::new();
    for (i, &b) in bytes.iter().enumerate() {
        if !b.is_ascii_alphanumeric() {
            continue;
        }
        if i == 0 || is_word_boundary(ident, i) {
            out.push((b as char).to_ascii_lowercase());
        }
    }
    out
}

fn fuzzy_subsequence_score(haystack: &str, needle: &str) -> Option<i32> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    let mut ni = 0usize;
    let mut score = 0i32;
    let mut prev_match: Option<usize> = None;
    for (hi, &hb) in h.iter().enumerate() {
        if ni < n.len() && hb == n[ni] {
            if let Some(pm) = prev_match {
                if hi == pm + 1 {
                    score += 18;
                } else {
                    score -= ((hi - pm - 1) as i32).min(6);
                }
            } else {
                score += 20 - (hi as i32).min(20);
            }
            prev_match = Some(hi);
            ni += 1;
        }
    }
    if ni == n.len() {
        Some(score.max(0))
    } else {
        None
    }
}

fn fuzzy_subsequence_with_boundaries(orig: &str, lower: &str, q_lower: &str) -> Option<i32> {
    let h = lower.as_bytes();
    let n = q_lower.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return None;
    }
    let mut ni = 0usize;
    let mut score = 0i32;
    let mut prev_match: Option<usize> = None;
    for (hi, &hb) in h.iter().enumerate() {
        if ni < n.len() && hb == n[ni] {
            if is_word_boundary(orig, hi) {
                score += 28;
            }
            if let Some(pm) = prev_match {
                if hi == pm + 1 {
                    score += 22;
                } else {
                    score -= ((hi - pm - 1) as i32).min(5);
                }
            } else {
                score += 24 - (hi as i32).min(24);
            }
            prev_match = Some(hi);
            ni += 1;
        }
    }
    if ni == n.len() {
        Some(score.max(0))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Pure-Rust `.gitignore`-aware bounded workspace walker
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct GitIgnoreRule {
    base_rel: String,
    pattern: String,
    negated: bool,
    dir_only: bool,
    anchored: bool,
}

#[derive(Debug, Clone, Default)]
struct GitIgnoreStack {
    rules: Vec<GitIgnoreRule>,
}

impl GitIgnoreStack {
    fn push_from_dir(&mut self, dir: &Path, base_rel: &str) -> usize {
        let gi_path = dir.join(".gitignore");
        let Ok(meta) = fs::symlink_metadata(&gi_path) else {
            return 0;
        };
        if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_GITIGNORE_BYTES {
            return 0;
        }
        let Ok(file) = File::open(&gi_path) else {
            return 0;
        };
        let mut raw_bytes = Vec::with_capacity((meta.len() as usize).min(4096));
        if file
            .take(MAX_GITIGNORE_BYTES + 1)
            .read_to_end(&mut raw_bytes)
            .is_err()
            || raw_bytes.len() as u64 > MAX_GITIGNORE_BYTES
        {
            return 0;
        }
        let Ok(raw) = String::from_utf8(raw_bytes) else {
            return 0;
        };

        let mut added = 0usize;
        for line in raw.lines() {
            let trimmed = line.trim_end_matches('\r');
            if trimmed.trim().is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (negated, rest) = if let Some(stripped) = trimmed.strip_prefix('!') {
                (true, stripped)
            } else {
                (false, trimmed)
            };
            let rest = rest.trim();
            if rest.is_empty() {
                continue;
            }
            let dir_only = rest.ends_with('/');
            let rest = rest.trim_end_matches('/');
            let anchored = rest.starts_with('/') || rest.contains('/');
            let pattern = normalize_glob_pattern(rest.trim_start_matches('/'));
            if pattern.is_empty() {
                continue;
            }
            self.rules.push(GitIgnoreRule {
                base_rel: base_rel.to_string(),
                pattern,
                negated,
                dir_only,
                anchored,
            });
            added += 1;
        }
        added
    }

    fn pop(&mut self, count: usize) {
        let new_len = self.rules.len().saturating_sub(count);
        self.rules.truncate(new_len);
    }

    fn is_ignored(&self, rel_path: &str, name: &str, is_dir: bool) -> bool {
        let mut ignored = false;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let sub_rel = if rule.base_rel.is_empty() {
                rel_path
            } else if let Some(s) = rel_path
                .strip_prefix(&rule.base_rel)
                .and_then(|r| r.strip_prefix('/'))
            {
                s
            } else {
                continue;
            };

            let matched = if rule.anchored {
                glob_match(&rule.pattern, sub_rel)
            } else {
                glob_match(&rule.pattern, name) || glob_match(&rule.pattern, sub_rel)
            };

            if matched {
                ignored = !rule.negated;
            }
        }
        ignored
    }
}

/// Collapse redundant `**` / `/**/` segments so pathological patterns cannot blow up pattern length.
fn normalize_glob_pattern(raw: &str) -> String {
    let mut segments: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        if seg.is_empty() {
            continue;
        }
        let is_double_star = seg.len() >= 2 && seg.bytes().all(|b| b == b'*');
        if is_double_star {
            if segments.last() != Some(&"**") {
                segments.push("**");
            }
        } else {
            segments.push(seg);
        }
    }
    segments.join("/")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GlobToken {
    /// Matches zero or more characters including `/` (handles both `**` and `**/`).
    DoubleStar,
    /// Matches zero or more characters excluding `/`.
    Star,
    /// Matches any single character excluding `/`.
    Question,
    /// Matches an exact literal character.
    Literal(char),
}

fn tokenize_glob(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '*' {
            let mut run = 1usize;
            while i + run < chars.len() && chars[i + run] == '*' {
                run += 1;
            }
            if run >= 2 {
                // Consume optional immediately following `/` so `**/foo` also matches `foo` at root
                let mut next = i + run;
                while next < chars.len() && chars[next] == '/' {
                    next += 1;
                }
                if tokens.last() != Some(&GlobToken::DoubleStar) {
                    tokens.push(GlobToken::DoubleStar);
                }
                i = next;
                continue;
            }
            if tokens.last() != Some(&GlobToken::Star)
                && tokens.last() != Some(&GlobToken::DoubleStar)
            {
                tokens.push(GlobToken::Star);
            }
            i += 1;
            continue;
        }
        if chars[i] == '?' {
            tokens.push(GlobToken::Question);
        } else {
            tokens.push(GlobToken::Literal(chars[i]));
        }
        i += 1;
    }
    tokens
}

/// Polynomial-time (`O(P * T)`) iterative DP glob matcher supporting `*`, `?`, and `**`
/// with zero exponential backtracking.
fn glob_match(pattern: &str, text: &str) -> bool {
    let normalized = normalize_glob_pattern(pattern);
    let tokens = tokenize_glob(&normalized);
    let t: Vec<char> = text.chars().collect();
    let m = t.len();

    let mut dp = vec![false; m + 1];
    let mut next = vec![false; m + 1];
    dp[0] = true;

    for tok in tokens {
        next.fill(false);
        match tok {
            GlobToken::DoubleStar => {
                // Matches 0 or more of ANY character (including `/`)
                let mut seen_true = false;
                for j in 0..=m {
                    if dp[j] {
                        seen_true = true;
                    }
                    if seen_true {
                        next[j] = true;
                    }
                }
            }
            GlobToken::Star => {
                // Matches 0 or more non-`/` characters
                for j in 0..=m {
                    if dp[j] || (j > 0 && next[j - 1] && t[j - 1] != '/') {
                        next[j] = true;
                    }
                }
            }
            GlobToken::Question => {
                for j in 1..=m {
                    if dp[j - 1] && t[j - 1] != '/' {
                        next[j] = true;
                    }
                }
            }
            GlobToken::Literal(ch) => {
                for j in 1..=m {
                    if dp[j - 1] && t[j - 1] == ch {
                        next[j] = true;
                    }
                }
            }
        }
        std::mem::swap(&mut dp, &mut next);
    }

    dp[m]
}

fn build_workspace_index_sync(canonical_root: &Path) -> (Vec<IndexedSymbol>, bool) {
    let mut symbols: Vec<IndexedSymbol> = Vec::new();
    let mut files_visited = 0usize;
    let mut truncated = false;
    let mut gi = GitIgnoreStack::default();

    walk_dir_for_symbols(
        canonical_root,
        "",
        0,
        &mut gi,
        &mut files_visited,
        &mut symbols,
        &mut truncated,
    );

    (symbols, truncated)
}

#[allow(clippy::too_many_arguments)]
fn walk_dir_for_symbols(
    dir: &Path,
    rel_dir: &str,
    depth: usize,
    gi: &mut GitIgnoreStack,
    files_visited: &mut usize,
    symbols: &mut Vec<IndexedSymbol>,
    truncated: &mut bool,
) {
    if depth > 16 || *files_visited >= MAX_INDEXED_FILES || symbols.len() >= MAX_INDEXED_SYMBOLS {
        *truncated = true;
        return;
    }

    let added_rules = gi.push_from_dir(dir, rel_dir);

    let Ok(read_dir) = fs::read_dir(dir) else {
        gi.pop(added_rules);
        return;
    };

    let mut entries: Vec<_> = read_dir.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if *files_visited >= MAX_INDEXED_FILES || symbols.len() >= MAX_INDEXED_SYMBOLS {
            *truncated = true;
            break;
        }

        let name_os = entry.file_name();
        let Some(name) = name_os.to_str() else {
            continue;
        };

        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };

        // Never traverse symlinks during indexing to guarantee confinement & avoid loops
        if meta.file_type().is_symlink() {
            continue;
        }

        let is_dir = meta.is_dir();
        let rel_path = if rel_dir.is_empty() {
            name.to_string()
        } else {
            format!("{rel_dir}/{name}")
        };

        if is_dir {
            if is_skipped_dir_name(name) || gi.is_ignored(&rel_path, name, true) {
                continue;
            }
            walk_dir_for_symbols(
                &path,
                &rel_path,
                depth + 1,
                gi,
                files_visited,
                symbols,
                truncated,
            );
            continue;
        }

        if !meta.is_file() {
            continue;
        }
        if meta.len() == 0 || meta.len() > MAX_FILE_BYTES {
            continue;
        }
        let Some(lang) = detect_language(&path) else {
            continue;
        };
        if gi.is_ignored(&rel_path, name, false) {
            continue;
        }

        *files_visited += 1;

        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        // Skip binary files (NUL byte in first 4 KiB)
        let probe_len = bytes.len().min(4096);
        if bytes[..probe_len].contains(&0) {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };

        let remaining = MAX_INDEXED_SYMBOLS.saturating_sub(symbols.len());
        if remaining == 0 {
            *truncated = true;
            break;
        }

        let extracted = extract_symbols_for_lang(&rel_path, lang, &content);
        for sym in extracted.into_iter().take(remaining) {
            symbols.push(IndexedSymbol::from_entry(sym));
        }
    }

    gi.pop(added_rules);
}

fn is_skipped_dir_name(name: &str) -> bool {
    SKIPPED_DIRS
        .iter()
        .any(|skip| skip.eq_ignore_ascii_case(name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLang {
    Rust,
    TypeScript,
    ArkTs,
    JavaScript,
    Python,
    Go,
    Kotlin,
    Swift,
    Java,
    C,
    Cpp,
}

impl SourceLang {
    fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "typescript",
            Self::ArkTs => "arkts",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
        }
    }
}

fn detect_language(path: &Path) -> Option<SourceLang> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "rs" => Some(SourceLang::Rust),
        "ts" | "tsx" | "mts" | "cts" => Some(SourceLang::TypeScript),
        "ets" => Some(SourceLang::ArkTs),
        "js" | "jsx" | "mjs" | "cjs" => Some(SourceLang::JavaScript),
        "py" | "pyi" => Some(SourceLang::Python),
        "go" => Some(SourceLang::Go),
        "kt" | "kts" => Some(SourceLang::Kotlin),
        "swift" => Some(SourceLang::Swift),
        "java" => Some(SourceLang::Java),
        "c" | "h" => Some(SourceLang::C),
        "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Some(SourceLang::Cpp),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Multi-language symbol extraction
// ---------------------------------------------------------------------------

/// Extract workspace symbols from a single source file's UTF-8 text.
pub fn extract_symbols_from_source(
    rel_path: &str,
    language_name: &str,
    content: &str,
) -> Vec<WorkspaceSymbolEntry> {
    let lang = match language_name {
        "rust" => SourceLang::Rust,
        "typescript" => SourceLang::TypeScript,
        "arkts" => SourceLang::ArkTs,
        "javascript" => SourceLang::JavaScript,
        "python" => SourceLang::Python,
        "go" => SourceLang::Go,
        "kotlin" => SourceLang::Kotlin,
        "swift" => SourceLang::Swift,
        "java" => SourceLang::Java,
        "c" => SourceLang::C,
        "cpp" => SourceLang::Cpp,
        _ => return Vec::new(),
    };
    extract_symbols_for_lang(rel_path, lang, content)
}

fn extract_symbols_for_lang(
    rel_path: &str,
    lang: SourceLang,
    content: &str,
) -> Vec<WorkspaceSymbolEntry> {
    let lines: Vec<&str> = content.lines().collect();
    if lang == SourceLang::Python {
        return extract_python_symbols(rel_path, &lines);
    }
    extract_brace_lang_symbols(rel_path, lang, &lines)
}

#[derive(Debug, Clone)]
struct ScopeEntry {
    name: String,
    depth: i32,
}

fn extract_brace_lang_symbols(
    rel_path: &str,
    lang: SourceLang,
    lines: &[&str],
) -> Vec<WorkspaceSymbolEntry> {
    let mut out = Vec::new();
    let mut in_block_comment = false;
    let mut brace_depth: i32 = 0;
    let mut scopes: Vec<ScopeEntry> = Vec::new();

    for (idx, raw_line) in lines.iter().enumerate() {
        let code = strip_c_style_comments_line(raw_line, &mut in_block_comment);
        let trimmed = code.trim();
        let line_num = (idx + 1) as u32;

        // Pop scopes that have closed before this line
        while let Some(top) = scopes.last() {
            if brace_depth <= top.depth {
                scopes.pop();
            } else {
                break;
            }
        }

        if !trimmed.is_empty() {
            let current_container = scopes.last().map(|s| s.name.clone());
            let parsed = match lang {
                SourceLang::Rust => parse_rust_decl(trimmed, current_container.as_deref()),
                SourceLang::TypeScript | SourceLang::ArkTs | SourceLang::JavaScript => {
                    parse_ts_js_decl(trimmed, current_container.as_deref(), brace_depth)
                }
                SourceLang::Go => parse_go_decl(trimmed),
                SourceLang::Kotlin | SourceLang::Swift | SourceLang::Java => {
                    parse_jvm_swift_decl(lang, trimmed, current_container.as_deref(), brace_depth)
                }
                SourceLang::C | SourceLang::Cpp => {
                    parse_c_cpp_decl(trimmed, current_container.as_deref(), brace_depth)
                }
                SourceLang::Python => None,
            };

            if let Some(decl) = parsed {
                let end_line = find_brace_span_end(lines, idx);
                let signature = Some(clean_signature(raw_line));
                if decl.pushes_scope {
                    scopes.push(ScopeEntry {
                        name: decl.scope_name.unwrap_or_else(|| decl.name.clone()),
                        depth: brace_depth,
                    });
                }
                if decl.emit {
                    out.push(WorkspaceSymbolEntry {
                        name: decl.name,
                        kind: decl.kind.to_string(),
                        path: rel_path.to_string(),
                        start_line: line_num,
                        end_line,
                        signature,
                        container: decl.container.or(current_container),
                        language: lang.as_str().to_string(),
                    });
                }
            }
        }

        brace_depth = (brace_depth + count_brace_delta(&code)).max(0);
        while let Some(top) = scopes.last() {
            if brace_depth <= top.depth {
                scopes.pop();
            } else {
                break;
            }
        }
    }

    out
}

#[derive(Debug)]
struct ParsedDecl {
    name: String,
    kind: &'static str,
    container: Option<String>,
    pushes_scope: bool,
    scope_name: Option<String>,
    emit: bool,
}

fn parse_rust_decl(trimmed: &str, container: Option<&str>) -> Option<ParsedDecl> {
    if trimmed.starts_with("#[") || trimmed.starts_with("use ") || trimmed.starts_with("//") {
        return None;
    }

    // Strip leading visibility (`pub`, `pub(crate)`, `pub(super)`, etc.) and qualifiers
    let rest = strip_rust_prefixes(trimmed);

    if let Some(after) = strip_keyword(rest, "fn") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: if container.is_some() { "method" } else { "fn" },
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "struct") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "struct",
            container: None,
            pushes_scope: trimmed.contains('{'),
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "enum") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "enum",
            container: None,
            pushes_scope: trimmed.contains('{'),
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "trait") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "trait",
            container: None,
            pushes_scope: true,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "impl") {
        let target = extract_rust_impl_target(after)?;
        return Some(ParsedDecl {
            name: target.clone(),
            kind: "impl",
            container: None,
            pushes_scope: true,
            scope_name: Some(target),
            emit: false, // Track impl as scope container for methods without polluting symbol list with duplicate impl names
        });
    }
    if let Some(after) = strip_keyword(rest, "type") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "type",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "const")
        && let Some(name) = take_ident(after)
    {
        return Some(ParsedDecl {
            name,
            kind: "const",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "static") {
        let after = strip_keyword(after, "mut").unwrap_or(after);
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "const",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "mod") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "module",
            container: None,
            pushes_scope: trimmed.contains('{'),
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = rest.strip_prefix("macro_rules!") {
        let name = take_ident(after.trim_start())?;
        return Some(ParsedDecl {
            name,
            kind: "fn",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }

    None
}

fn strip_rust_prefixes(mut s: &str) -> &str {
    loop {
        s = s.trim_start();
        if let Some(after_pub) = strip_keyword(s, "pub") {
            let after_pub = after_pub.trim_start();
            if let Some(after_paren) = after_pub.strip_prefix('(')
                && let Some(close) = after_paren.find(')')
            {
                s = &after_paren[close + 1..];
                continue;
            }
            s = after_pub;
            continue;
        }
        let mut matched = false;
        for kw in ["default", "async", "unsafe", "const"] {
            // Do not consume `const` if it's a `const NAME:` declaration (only consume `const fn` / `const unsafe`)
            if kw == "const" {
                if let Some(after) = strip_keyword(s, "const") {
                    let next_word = take_ident(after).unwrap_or_default();
                    if matches!(next_word.as_str(), "fn" | "unsafe" | "extern") {
                        s = after;
                        matched = true;
                        break;
                    }
                }
                continue;
            }
            if let Some(after) = strip_keyword(s, kw) {
                s = after;
                matched = true;
                break;
            }
        }
        if matched {
            continue;
        }
        if let Some(after_extern) = strip_keyword(s, "extern") {
            let after_extern = after_extern.trim_start();
            if let Some(rest_quote) = after_extern.strip_prefix('"')
                && let Some(end_q) = rest_quote.find('"')
            {
                s = &rest_quote[end_q + 1..];
                continue;
            }
            s = after_extern;
            continue;
        }
        break;
    }
    s
}

fn extract_rust_impl_target(after_impl: &str) -> Option<String> {
    let mut s = after_impl.trim_start();
    // Skip leading generic params `impl<T: Foo>`
    if s.starts_with('<') {
        s = skip_balanced_angles(s).trim_start();
    }
    // Trim `{` or `where` suffix
    if let Some(pos) = s.find('{') {
        s = &s[..pos];
    }
    if let Some(pos) = s.find(" where ") {
        s = &s[..pos];
    }
    // If `impl Trait for Type`, take `Type`
    if let Some((_, rhs)) = s.rsplit_once(" for ") {
        s = rhs.trim();
    }
    // Take final path segment before `<`
    let before_generics = s.split('<').next().unwrap_or(s).trim();
    let last_seg = before_generics
        .rsplit("::")
        .next()
        .unwrap_or(before_generics)
        .trim();
    take_ident(last_seg)
}

fn parse_ts_js_decl(
    trimmed: &str,
    container: Option<&str>,
    brace_depth: i32,
) -> Option<ParsedDecl> {
    if trimmed.starts_with("import ") || trimmed.starts_with("return ") {
        return None;
    }

    let mut rest = trimmed;
    // Strip ArkTS/TS decorators on same line (e.g. `@Component struct Foo`)
    while rest.starts_with('@') {
        let mut chars = rest.char_indices();
        let _ = chars.next();
        let mut end = rest.len();
        let mut paren_depth = 0i32;
        for (i, ch) in chars {
            match ch {
                '(' => paren_depth += 1,
                ')' => paren_depth = (paren_depth - 1).max(0),
                c if c.is_whitespace() && paren_depth == 0 => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        rest = rest[end..].trim_start();
    }

    loop {
        let mut stripped = false;
        for kw in [
            "export",
            "default",
            "declare",
            "abstract",
            "async",
            "public",
            "private",
            "protected",
            "readonly",
            "static",
            "override",
        ] {
            if let Some(after) = strip_keyword(rest, kw) {
                rest = after;
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    if let Some(after) = strip_keyword(rest, "function") {
        let after = after.trim_start().trim_start_matches('*').trim_start();
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "fn",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "class") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "class",
            container: None,
            pushes_scope: true,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "struct") {
        // ArkTS `@Component struct MyPage`
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "struct",
            container: None,
            pushes_scope: true,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "interface") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "interface",
            container: None,
            pushes_scope: true,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "type") {
        let name = take_ident(after)?;
        let after_name = after.trim_start()[name.len()..].trim_start();
        if after_name.starts_with('=') || after_name.starts_with('<') {
            return Some(ParsedDecl {
                name,
                kind: "type",
                container: None,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
    }
    if let Some(after) = strip_keyword(rest, "enum") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "enum",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(rest, "namespace") {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "module",
            container: None,
            pushes_scope: true,
            scope_name: None,
            emit: true,
        });
    }

    // Top-level or exported `const/let/var name = (...) =>` or `const name = memo(function ...)` or `const UPPER = ...`
    for kw in ["const", "let", "var"] {
        if let Some(after) = strip_keyword(rest, kw) {
            if brace_depth > 0 && !trimmed.starts_with("export ") {
                return None;
            }
            let name = take_ident(after)?;
            let rhs = after.trim_start()[name.len()..].trim_start();
            let is_fn_expr = rhs.contains("=>")
                || rhs.contains("function(")
                || rhs.contains("function ")
                || rhs.contains("memo(")
                || rhs.contains("forwardRef(");
            if is_fn_expr {
                return Some(ParsedDecl {
                    name,
                    kind: "fn",
                    container: None,
                    pushes_scope: false,
                    scope_name: None,
                    emit: true,
                });
            }
            if kw == "const" && (trimmed.starts_with("export ") || is_upper_snake(&name)) {
                return Some(ParsedDecl {
                    name,
                    kind: "const",
                    container: None,
                    pushes_scope: false,
                    scope_name: None,
                    emit: true,
                });
            }
            return None;
        }
    }

    // Class / ArkTS struct method: `foo(a: number): void {`
    if container.is_some() && brace_depth >= 1 {
        let method_rest = strip_keyword(rest, "get")
            .or_else(|| strip_keyword(rest, "set"))
            .unwrap_or(rest);
        if let Some(name) = take_ident(method_rest)
            && !is_control_keyword(&name)
            && name != "constructor"
        {
            let after_name = method_rest.trim_start()[name.len()..].trim_start();
            let after_gen = if after_name.starts_with('<') {
                skip_balanced_angles(after_name).trim_start()
            } else {
                after_name
            };
            if after_gen.starts_with('(') && trimmed.contains('{') {
                return Some(ParsedDecl {
                    name,
                    kind: "method",
                    container: None,
                    pushes_scope: false,
                    scope_name: None,
                    emit: true,
                });
            }
        }
    }

    None
}

fn parse_go_decl(trimmed: &str) -> Option<ParsedDecl> {
    if let Some(after) = strip_keyword(trimmed, "func") {
        let after = after.trim_start();
        if let Some(after_lparen) = after.strip_prefix('(') {
            // Receiver method: `func (s *Server) Handle(...)`
            let close = after_lparen.find(')')?;
            let recv_section = after_lparen[..close].trim();
            let container = recv_section
                .split_whitespace()
                .last()
                .map(|t| t.trim_start_matches('*'))
                .and_then(|t| t.split('[').next())
                .and_then(take_ident);
            let name = take_ident(after_lparen[close + 1..].trim_start())?;
            return Some(ParsedDecl {
                name,
                kind: "method",
                container,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "fn",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(trimmed, "type") {
        let name = take_ident(after)?;
        let rest = after.trim_start()[name.len()..].trim_start();
        let kind = if strip_keyword(rest, "struct").is_some() {
            "struct"
        } else if strip_keyword(rest, "interface").is_some() {
            "interface"
        } else {
            "type"
        };
        return Some(ParsedDecl {
            name,
            kind,
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    if let Some(after) = strip_keyword(trimmed, "const")
        && !after.trim_start().starts_with('(')
    {
        let name = take_ident(after)?;
        return Some(ParsedDecl {
            name,
            kind: "const",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }
    None
}

fn parse_jvm_swift_decl(
    lang: SourceLang,
    trimmed: &str,
    container: Option<&str>,
    brace_depth: i32,
) -> Option<ParsedDecl> {
    if trimmed.starts_with("import ") || trimmed.starts_with("package ") || trimmed.starts_with('@')
    {
        return None;
    }

    let mut rest = trimmed;
    loop {
        let mut stripped = false;
        for kw in [
            "public",
            "private",
            "protected",
            "internal",
            "fileprivate",
            "open",
            "final",
            "abstract",
            "sealed",
            "data",
            "inner",
            "static",
            "override",
            "suspend",
            "inline",
            "mutating",
            "nonmutating",
            "synchronized",
            "async",
            "indirect",
        ] {
            if let Some(after) = strip_keyword(rest, kw) {
                rest = after;
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }

    for (kw, kind, pushes) in [
        ("class", "class", true),
        ("struct", "struct", true),
        ("interface", "interface", true),
        ("protocol", "trait", true),
        ("object", "class", true),
        ("actor", "class", true),
        ("enum", "enum", true),
        ("record", "struct", true),
    ] {
        if let Some(after) = strip_keyword(rest, kw) {
            // Handle Kotlin `enum class Foo`
            let after = if kw == "enum" {
                strip_keyword(after, "class").unwrap_or(after)
            } else {
                after
            };
            let name = take_ident(after)?;
            return Some(ParsedDecl {
                name,
                kind,
                container: None,
                pushes_scope: pushes,
                scope_name: None,
                emit: true,
            });
        }
    }

    if lang == SourceLang::Swift {
        if let Some(after) = strip_keyword(rest, "extension") {
            let name = take_ident(after)?;
            return Some(ParsedDecl {
                name: name.clone(),
                kind: "impl",
                container: None,
                pushes_scope: true,
                scope_name: Some(name),
                emit: false,
            });
        }
        if let Some(after) = strip_keyword(rest, "func") {
            let name = take_ident(after)?;
            return Some(ParsedDecl {
                name,
                kind: if container.is_some() { "method" } else { "fn" },
                container: None,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
        if let Some(after) = strip_keyword(rest, "typealias") {
            let name = take_ident(after)?;
            return Some(ParsedDecl {
                name,
                kind: "type",
                container: None,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
    }

    if lang == SourceLang::Kotlin {
        if let Some(after) = strip_keyword(rest, "fun") {
            let mut s = after.trim_start();
            if s.starts_with('<') {
                s = skip_balanced_angles(s).trim_start();
            }
            // Handle extension functions `fun Foo.bar(...)`
            let first = take_ident(s)?;
            let tail = s[first.len()..].trim_start();
            let (name, ext_container) = if let Some(after_dot) = tail.strip_prefix('.') {
                (take_ident(after_dot)?, Some(first))
            } else {
                (first, None)
            };
            return Some(ParsedDecl {
                name,
                kind: if container.is_some() || ext_container.is_some() {
                    "method"
                } else {
                    "fn"
                },
                container: ext_container,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
        if let Some(after) = strip_keyword(rest, "typealias") {
            let name = take_ident(after)?;
            return Some(ParsedDecl {
                name,
                kind: "type",
                container: None,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
    }

    // Java method definition inside a class/interface: `ReturnType methodName(...) {`
    if lang == SourceLang::Java
        && container.is_some()
        && brace_depth >= 1
        && trimmed.contains('{')
        && let Some(name) = extract_c_like_fn_name(rest)
    {
        return Some(ParsedDecl {
            name,
            kind: "method",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }

    None
}

fn parse_c_cpp_decl(
    trimmed: &str,
    container: Option<&str>,
    brace_depth: i32,
) -> Option<ParsedDecl> {
    if trimmed.starts_with('#') {
        return None;
    }

    for (kw, kind, pushes) in [
        ("class", "class", true),
        ("struct", "struct", true),
        ("enum", "enum", false),
        ("union", "struct", true),
        ("namespace", "module", true),
    ] {
        if let Some(after) = strip_keyword(trimmed, kw) {
            let after = if kw == "enum" {
                strip_keyword(after, "class").unwrap_or(after)
            } else {
                after
            };
            if trimmed.contains('{')
                && let Some(name) = take_ident(after)
            {
                return Some(ParsedDecl {
                    name,
                    kind,
                    container: None,
                    pushes_scope: pushes,
                    scope_name: None,
                    emit: true,
                });
            }
        }
    }

    if let Some(after) = strip_keyword(trimmed, "using")
        && let Some(name) = take_ident(after)
        && after[name.len()..].trim_start().starts_with('=')
    {
        return Some(ParsedDecl {
            name,
            kind: "type",
            container: None,
            pushes_scope: false,
            scope_name: None,
            emit: true,
        });
    }

    // C/C++ function definition at top level or inside class/namespace
    if brace_depth <= 2
        && trimmed.contains('(')
        && trimmed.contains('{')
        && !trimmed.contains(';')
        && let Some(raw_name) = extract_c_like_fn_name(trimmed)
    {
        let (name, qual_container) = if let Some((cls, method)) = raw_name.rsplit_once("::") {
            (method.to_string(), Some(cls.to_string()))
        } else {
            (raw_name, None)
        };
        if !name.is_empty() && !is_control_keyword(&name) {
            return Some(ParsedDecl {
                name,
                kind: if container.is_some() || qual_container.is_some() {
                    "method"
                } else {
                    "fn"
                },
                container: qual_container,
                pushes_scope: false,
                scope_name: None,
                emit: true,
            });
        }
    }

    None
}

fn extract_c_like_fn_name(line: &str) -> Option<String> {
    let paren = line.find('(')?;
    let before = line[..paren].trim_end();
    if before.is_empty() || before.ends_with('=') {
        return None;
    }
    // Extract trailing identifier (optionally `Class::method`)
    let mut start = before.len();
    for (i, ch) in before.char_indices().rev() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == ':' {
            start = i;
        } else {
            break;
        }
    }
    let candidate = before[start..].trim_matches(':');
    if candidate.is_empty() {
        return None;
    }
    let last = candidate.rsplit("::").next().unwrap_or(candidate);
    if is_control_keyword(last)
        || !last
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        return None;
    }
    // Require a return type or qualifier before the identifier for C/C++/Java
    let prefix = before[..start].trim();
    if prefix.is_empty() {
        return None;
    }
    Some(candidate.to_string())
}

fn extract_python_symbols(rel_path: &str, lines: &[&str]) -> Vec<WorkspaceSymbolEntry> {
    let mut out = Vec::new();
    let mut scopes: Vec<(String, usize)> = Vec::new();

    for (idx, raw_line) in lines.iter().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('@') {
            continue;
        }
        let indent = leading_indent_width(raw_line);
        while let Some((_, scope_indent)) = scopes.last() {
            if indent <= *scope_indent {
                scopes.pop();
            } else {
                break;
            }
        }

        let line_num = (idx + 1) as u32;
        if let Some(after) = strip_keyword(trimmed, "class")
            && let Some(name) = take_ident(after)
        {
            let end_line = find_python_span_end(lines, idx, indent);
            let container = scopes.last().map(|(s, _)| s.clone());
            out.push(WorkspaceSymbolEntry {
                name: name.clone(),
                kind: "class".to_string(),
                path: rel_path.to_string(),
                start_line: line_num,
                end_line,
                signature: Some(clean_signature(raw_line)),
                container,
                language: "python".to_string(),
            });
            scopes.push((name, indent));
            continue;
        }

        let def_rest = strip_keyword(trimmed, "async")
            .and_then(|s| strip_keyword(s, "def"))
            .or_else(|| strip_keyword(trimmed, "def"));
        if let Some(after) = def_rest
            && let Some(name) = take_ident(after)
        {
            let end_line = find_python_span_end(lines, idx, indent);
            let container = scopes.last().map(|(s, _)| s.clone());
            let kind = if container.is_some() { "method" } else { "fn" };
            out.push(WorkspaceSymbolEntry {
                name,
                kind: kind.to_string(),
                path: rel_path.to_string(),
                start_line: line_num,
                end_line,
                signature: Some(clean_signature(raw_line)),
                container,
                language: "python".to_string(),
            });
            continue;
        }

        if indent == 0
            && let Some(name) = take_ident(trimmed)
            && is_upper_snake(&name)
        {
            let rest = trimmed[name.len()..].trim_start();
            if rest.starts_with('=') && !rest.starts_with("==") {
                out.push(WorkspaceSymbolEntry {
                    name,
                    kind: "const".to_string(),
                    path: rel_path.to_string(),
                    start_line: line_num,
                    end_line: line_num,
                    signature: Some(clean_signature(raw_line)),
                    container: None,
                    language: "python".to_string(),
                });
            }
        }
    }

    out
}

fn leading_indent_width(line: &str) -> usize {
    let mut width = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => width += 1,
            '\t' => width += 4,
            _ => break,
        }
    }
    width
}

fn find_python_span_end(lines: &[&str], start_idx: usize, def_indent: usize) -> u32 {
    let mut last_body_idx = start_idx;
    let max_idx = (start_idx + MAX_SYMBOL_SPAN_LINES).min(lines.len());
    for (idx, raw) in lines.iter().enumerate().take(max_idx).skip(start_idx + 1) {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let ind = leading_indent_width(raw);
        if ind <= def_indent {
            break;
        }
        last_body_idx = idx;
    }
    (last_body_idx + 1) as u32
}

fn find_brace_span_end(lines: &[&str], start_idx: usize) -> u32 {
    let max_idx = (start_idx + MAX_SYMBOL_SPAN_LINES).min(lines.len());
    let mut in_block_comment = false;
    let mut saw_open = false;
    let mut depth: i32 = 0;

    for (idx, raw) in lines.iter().enumerate().take(max_idx).skip(start_idx) {
        let code = strip_c_style_comments_line(raw, &mut in_block_comment);
        for ch in code.chars() {
            match ch {
                '{' => {
                    saw_open = true;
                    depth += 1;
                }
                '}' => {
                    if saw_open {
                        depth -= 1;
                        if depth <= 0 {
                            return (idx + 1) as u32;
                        }
                    }
                }
                ';' if !saw_open => {
                    return (idx + 1) as u32;
                }
                _ => {}
            }
        }
        // Single-line arrow expression or type alias without braces
        if idx == start_idx && !saw_open && code.trim_end().ends_with(';') {
            return (idx + 1) as u32;
        }
    }

    (start_idx + 1) as u32
}

fn strip_c_style_comments_line(line: &str, in_block_comment: &mut bool) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0usize;
    let mut in_str: Option<u8> = None;

    while i < bytes.len() {
        if *in_block_comment {
            if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'/' {
                *in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }

        let b = bytes[i];
        if let Some(q) = in_str {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == q {
                in_str = None;
            }
            // Replace string contents with space so braces inside strings don't skew brace depth
            out.push(' ');
            i += 1;
            continue;
        }

        if i + 1 < bytes.len() && b == b'/' && bytes[i + 1] == b'/' {
            break;
        }
        if i + 1 < bytes.len() && b == b'/' && bytes[i + 1] == b'*' {
            *in_block_comment = true;
            i += 2;
            continue;
        }
        if b == b'"' || b == b'`' {
            in_str = Some(b);
            out.push(' ');
            i += 1;
            continue;
        }
        // Only treat single quotes as char literals when followed by escape or single char + quote
        // (avoids mistaking Rust lifetimes `'a` for unclosed string literals!)
        if b == b'\'' {
            if i + 2 < bytes.len() && bytes[i + 1] != b'\\' && bytes[i + 2] == b'\'' {
                out.push(' ');
                i += 3;
                continue;
            }
            if i + 3 < bytes.len() && bytes[i + 1] == b'\\' && bytes[i + 3] == b'\'' {
                out.push(' ');
                i += 4;
                continue;
            }
        }

        if let Some(ch) = line[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    out
}

fn count_brace_delta(code: &str) -> i32 {
    let mut delta = 0i32;
    for ch in code.chars() {
        if ch == '{' {
            delta += 1;
        } else if ch == '}' {
            delta -= 1;
        }
    }
    delta
}

fn strip_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(kw)?;
    match rest.chars().next() {
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => None,
        _ => Some(rest.trim_start()),
    }
}

fn take_ident(s: &str) -> Option<String> {
    let s = s.trim_start();
    let mut end = 0usize;
    for (i, ch) in s.char_indices() {
        if i == 0 {
            if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' {
                end = i + ch.len_utf8();
            } else {
                return None;
            }
        } else if ch.is_ascii_alphanumeric() || ch == '_' || ch == '$' {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some(s[..end].to_string())
    }
}

fn skip_balanced_angles(s: &str) -> &str {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                depth -= 1;
                if depth <= 0 {
                    return &s[i + 1..];
                }
            }
            _ => {}
        }
    }
    s
}

fn is_upper_snake(ident: &str) -> bool {
    ident.len() >= 2
        && ident.chars().any(|c| c.is_ascii_uppercase())
        && ident
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

fn is_control_keyword(ident: &str) -> bool {
    matches!(
        ident,
        "if" | "else"
            | "for"
            | "while"
            | "switch"
            | "catch"
            | "return"
            | "throw"
            | "match"
            | "loop"
            | "do"
            | "try"
            | "finally"
            | "new"
            | "delete"
            | "sizeof"
            | "typeof"
    )
}

fn clean_signature(raw_line: &str) -> String {
    let trimmed = raw_line.trim();
    let without_brace = trimmed.trim_end_matches('{').trim_end();
    let collapsed: String = without_brace
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    clip_utf8_chars(&collapsed, 140)
}

fn clip_utf8_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

// ---------------------------------------------------------------------------
// Canonical workspace-confined symbol snippet reader
// ---------------------------------------------------------------------------

/// Read a line-numbered code snippet for a cited symbol strictly confined inside `cwd`.
///
/// Rejects path traversal (`..`), symlinks escaping `cwd`, `.git` internals, NUL/control
/// characters, and binary files. Truncates cleanly on a UTF-8 character boundary.
pub fn read_symbol_snippet(
    cwd: &Path,
    raw_path: &str,
    start_line: u32,
    end_line: u32,
    max_bytes: usize,
) -> Result<String, String> {
    if raw_path.trim().is_empty() {
        return Err("Symbol path cannot be empty".to_string());
    }
    if raw_path.chars().any(|c| c == '\0' || c.is_control()) {
        return Err("Symbol path contains invalid control characters".to_string());
    }
    if raw_path.contains('\\') {
        return Err("Symbol path must use forward slashes".to_string());
    }

    let canonical_cwd = canonicalize_workspace_root(cwd)?;

    let rel = Path::new(raw_path.trim());
    if rel.is_absolute() {
        return Err("Absolute symbol paths are not allowed".to_string());
    }
    for comp in rel.components() {
        match comp {
            Component::Normal(seg) => {
                if seg.to_string_lossy().eq_ignore_ascii_case(".git") {
                    return Err("Reading `.git` internals is forbidden".to_string());
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(format!("Symbol path `{raw_path}` escapes workspace root"));
            }
        }
    }

    let candidate = canonical_cwd.join(rel);
    let canonical_file = crate::platform::canonicalize_clean(&candidate)
        .map_err(|e| format!("Symbol file unavailable: {e}"))?;
    if !canonical_file.starts_with(&canonical_cwd) {
        return Err(format!("Symbol path `{raw_path}` escapes workspace root"));
    }
    if is_path_inside_git(&canonical_cwd, &canonical_file) {
        return Err("Reading `.git` internals is forbidden".to_string());
    }

    let meta =
        fs::metadata(&canonical_file).map_err(|e| format!("Failed to inspect symbol file: {e}"))?;
    if !meta.is_file() {
        return Err("Symbol target is not a regular file".to_string());
    }
    if meta.len() > MAX_FILE_BYTES * 4 {
        return Err("Symbol file exceeds maximum readable size".to_string());
    }

    let bytes =
        fs::read(&canonical_file).map_err(|e| format!("Failed reading symbol file: {e}"))?;
    let probe = bytes.len().min(4096);
    if bytes[..probe].contains(&0) {
        return Err("Refusing to read binary file as code snippet".to_string());
    }
    let text =
        String::from_utf8(bytes).map_err(|_| "Symbol file is not valid UTF-8 text".to_string())?;

    let all_lines: Vec<&str> = text.lines().collect();
    if all_lines.is_empty() {
        return Ok(String::new());
    }

    let safe_start = start_line.max(1);
    let start_idx = (safe_start - 1) as usize;
    if start_idx >= all_lines.len() {
        return Err(format!(
            "Start line {safe_start} exceeds file length ({})",
            all_lines.len()
        ));
    }
    let clamped_end = end_line
        .max(safe_start)
        .min(all_lines.len() as u32)
        .min(safe_start.saturating_add(MAX_SYMBOL_SPAN_LINES as u32));
    let end_idx = (clamped_end as usize).min(all_lines.len());

    let budget = if max_bytes == 0 {
        DEFAULT_SNIPPET_MAX_BYTES
    } else {
        max_bytes
    };

    let mut snippet = String::new();
    for (offset, line) in all_lines[start_idx..end_idx].iter().enumerate() {
        let line_no = start_idx + offset + 1;
        let formatted = format!("{line_no} | {line}\n");
        if snippet.len() + formatted.len() > budget {
            if snippet.is_empty() {
                // Clip single oversized line safely at UTF-8 boundary
                let mut end = budget.min(formatted.len());
                while end > 0 && !formatted.is_char_boundary(end) {
                    end -= 1;
                }
                snippet.push_str(&formatted[..end]);
            }
            snippet.push_str("… (truncated)");
            break;
        }
        snippet.push_str(&formatted);
    }

    Ok(snippet.trim_end_matches('\n').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn temp_workspace(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orbit-symbols-{prefix}-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create temp workspace");
        dir
    }

    #[test]
    fn extracts_rust_symbols_with_containers_and_line_spans() {
        let src = r#"
pub struct OrbitEngine {
    pub id: String,
}

pub enum SessionState {
    Idle,
    Running,
}

pub trait Executor {
    fn execute(&self);
}

impl OrbitEngine {
    pub async fn run_turn(&self, input: &str) -> bool {
        let ok = !input.is_empty();
        ok
    }
}

pub type EngineId = String;
pub const MAX_RETRIES: usize = 3;
"#;
        let syms = extract_symbols_from_source("src/engine.rs", "rust", src);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"OrbitEngine"));
        assert!(names.contains(&"SessionState"));
        assert!(names.contains(&"Executor"));
        assert!(names.contains(&"execute"));
        assert!(names.contains(&"run_turn"));
        assert!(names.contains(&"EngineId"));
        assert!(names.contains(&"MAX_RETRIES"));

        let run_turn = syms
            .iter()
            .find(|s| s.name == "run_turn")
            .expect("run_turn");
        assert_eq!(run_turn.kind, "method");
        assert_eq!(run_turn.container.as_deref(), Some("OrbitEngine"));
        assert_eq!(run_turn.start_line, 16);
        assert_eq!(run_turn.end_line, 19);
        assert_eq!(run_turn.language, "rust");
    }

    #[test]
    fn extracts_typescript_arkts_python_go_kotlin_swift_and_cpp_symbols() {
        let ts_src = r#"
export interface AuthSession {
  token: string;
}

export type UserId = string;

export class SessionStore {
  async refreshToken(id: UserId): Promise<void> {
    return;
  }
}

export const buildSessionKey = (hostId: string) => `${hostId}:session`;
export function resolveHostRoute(mode: string) {
  return mode;
}
"#;
        let ts_syms = extract_symbols_from_source("src/session.ts", "typescript", ts_src);
        assert!(
            ts_syms
                .iter()
                .any(|s| s.name == "AuthSession" && s.kind == "interface")
        );
        assert!(
            ts_syms
                .iter()
                .any(|s| s.name == "UserId" && s.kind == "type")
        );
        assert!(
            ts_syms
                .iter()
                .any(|s| s.name == "SessionStore" && s.kind == "class")
        );
        assert!(ts_syms.iter().any(|s| {
            s.name == "refreshToken"
                && s.kind == "method"
                && s.container.as_deref() == Some("SessionStore")
        }));
        assert!(
            ts_syms
                .iter()
                .any(|s| s.name == "buildSessionKey" && s.kind == "fn")
        );
        assert!(
            ts_syms
                .iter()
                .any(|s| s.name == "resolveHostRoute" && s.kind == "fn")
        );

        let arkts_src = r#"
@Component
struct OrbitStagePage {
  build() {
  }
}
"#;
        let ark_syms = extract_symbols_from_source("entry/OrbitStagePage.ets", "arkts", arkts_src);
        assert!(
            ark_syms
                .iter()
                .any(|s| s.name == "OrbitStagePage" && s.kind == "struct")
        );
        assert!(
            ark_syms
                .iter()
                .any(|s| { s.name == "build" && s.container.as_deref() == Some("OrbitStagePage") })
        );

        let py_src = r#"
MAX_BATCH = 64

class SymbolIndexer:
    async def scan_workspace(self, root: str) -> list[str]:
        items = []
        return items

def format_citation(path: str) -> str:
    return path
"#;
        let py_syms = extract_symbols_from_source("indexer.py", "python", py_src);
        let scan = py_syms
            .iter()
            .find(|s| s.name == "scan_workspace")
            .expect("scan_workspace");
        assert_eq!(scan.kind, "method");
        assert_eq!(scan.container.as_deref(), Some("SymbolIndexer"));
        assert_eq!(scan.start_line, 5);
        assert_eq!(scan.end_line, 7);
        assert!(
            py_syms
                .iter()
                .any(|s| s.name == "MAX_BATCH" && s.kind == "const")
        );
        assert!(
            py_syms
                .iter()
                .any(|s| s.name == "format_citation" && s.kind == "fn")
        );

        let go_src = r#"
type RelayHub struct {
    Addr string
}

func (h *RelayHub) BroadcastFrame(data []byte) error {
    return nil
}

func DialRelay(url string) error {
    return nil
}
"#;
        let go_syms = extract_symbols_from_source("relay.go", "go", go_src);
        assert!(
            go_syms
                .iter()
                .any(|s| s.name == "RelayHub" && s.kind == "struct")
        );
        assert!(go_syms.iter().any(|s| {
            s.name == "BroadcastFrame"
                && s.kind == "method"
                && s.container.as_deref() == Some("RelayHub")
        }));
        assert!(
            go_syms
                .iter()
                .any(|s| s.name == "DialRelay" && s.kind == "fn")
        );

        let kt_src = r#"
interface TransportChannel {
    fun sendFrame(payload: ByteArray)
}

class SocketManager {
    suspend fun connectPeer(hostId: String) {
        return
    }
}
"#;
        let kt_syms = extract_symbols_from_source("Socket.kt", "kotlin", kt_src);
        assert!(
            kt_syms
                .iter()
                .any(|s| s.name == "TransportChannel" && s.kind == "interface")
        );
        assert!(kt_syms.iter().any(|s| {
            s.name == "connectPeer"
                && s.kind == "method"
                && s.container.as_deref() == Some("SocketManager")
        }));
    }

    #[test]
    fn fuzzy_ranking_prefers_exact_prefix_and_camel_snake_initials() {
        let entries = vec![
            WorkspaceSymbolEntry {
                name: "x_search_w_sym".into(),
                kind: "fn".into(),
                path: "c.rs".into(),
                start_line: 1,
                end_line: 3,
                signature: None,
                container: None,
                language: "rust".into(),
            },
            WorkspaceSymbolEntry {
                name: "search_workspace_symbols".into(),
                kind: "fn".into(),
                path: "a.rs".into(),
                start_line: 10,
                end_line: 20,
                signature: None,
                container: None,
                language: "rust".into(),
            },
            WorkspaceSymbolEntry {
                name: "sws".into(),
                kind: "fn".into(),
                path: "b.rs".into(),
                start_line: 1,
                end_line: 2,
                signature: None,
                container: None,
                language: "rust".into(),
            },
            WorkspaceSymbolEntry {
                name: "sws_helper".into(),
                kind: "fn".into(),
                path: "d.rs".into(),
                start_line: 1,
                end_line: 2,
                signature: None,
                container: None,
                language: "rust".into(),
            },
        ];
        let indexed: Vec<IndexedSymbol> =
            entries.into_iter().map(IndexedSymbol::from_entry).collect();

        let ranked = rank_indexed_symbols(&indexed, "sws", 10);
        assert_eq!(ranked[0].name, "sws");
        assert_eq!(ranked[1].name, "sws_helper");
        assert_eq!(ranked[2].name, "search_workspace_symbols");
    }

    #[tokio::test]
    async fn indexer_respects_gitignore_and_skips_git_and_target_dirs() {
        let ws = temp_workspace("gitignore");
        fs::write(ws.join(".gitignore"), "ignored_dir/\n*.gen.rs\n").expect("write gitignore");
        fs::create_dir_all(ws.join("src")).expect("mkdir src");
        fs::create_dir_all(ws.join("ignored_dir")).expect("mkdir ignored");
        fs::create_dir_all(ws.join(".git")).expect("mkdir .git");
        fs::create_dir_all(ws.join("target")).expect("mkdir target");

        fs::write(
            ws.join("src/lib.rs"),
            "pub fn visible_symbol() {\n    println!(\"ok\");\n}\n",
        )
        .expect("write visible");
        fs::write(
            ws.join("src/schema.gen.rs"),
            "pub fn generated_hidden() {}\n",
        )
        .expect("write gen");
        fs::write(
            ws.join("ignored_dir/secret.rs"),
            "pub fn ignored_hidden() {}\n",
        )
        .expect("write ignored");
        fs::write(ws.join(".git/hook.rs"), "pub fn git_hidden() {}\n").expect("write git");
        fs::write(ws.join("target/out.rs"), "pub fn target_hidden() {}\n").expect("write target");

        let index = WorkspaceSymbolIndex::new();
        let res = index.search(&ws, "", 30).await.expect("search symbols");
        let names: Vec<&str> = res.symbols.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["visible_symbol"]);

        // Mutate file and invalidate cache -> picks up new symbol immediately
        fs::write(
            ws.join("src/lib.rs"),
            "pub fn visible_symbol() {}\npub fn freshly_added_symbol() {}\n",
        )
        .expect("update visible");
        index.invalidate(&ws);
        let res2 = index
            .search(&ws, "freshly", 30)
            .await
            .expect("search after invalidate");
        assert_eq!(res2.symbols.len(), 1);
        assert_eq!(res2.symbols[0].name, "freshly_added_symbol");

        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn read_symbol_snippet_enforces_confinement_and_utf8_safety() {
        let ws = temp_workspace("snippet");
        let outside = std::env::temp_dir().join(format!("orbit-outside-{}.rs", Uuid::new_v4()));
        fs::write(&outside, "pub fn secret() {}\n").expect("write outside");
        fs::create_dir_all(ws.join("src")).expect("mkdir src");
        fs::write(
            ws.join("src/demo.rs"),
            "/// 你好世界注释\npub fn greet() {\n    let msg = \"你好，Orbit 🚀\";\n}\n",
        )
        .expect("write demo");

        // Valid snippet
        let snippet = read_symbol_snippet(&ws, "src/demo.rs", 2, 4, 4096).expect("read snippet");
        assert!(snippet.contains("2 | pub fn greet() {"));
        assert!(snippet.contains("3 |     let msg = \"你好，Orbit 🚀\";"));

        // UTF-8 safe tight truncation (must never panic on multibyte boundaries)
        let clipped = read_symbol_snippet(&ws, "src/demo.rs", 1, 4, 15).expect("tight clip");
        assert!(clipped.contains("truncated"));

        // Path traversal rejected
        assert!(read_symbol_snippet(&ws, "../outside.rs", 1, 2, 1024).is_err());
        assert!(read_symbol_snippet(&ws, outside.to_str().unwrap_or(""), 1, 2, 1024).is_err());
        assert!(read_symbol_snippet(&ws, ".git/config", 1, 2, 1024).is_err());

        #[cfg(unix)]
        {
            let link = ws.join("src/escape.rs");
            std::os::unix::fs::symlink(&outside, &link).expect("create symlink");
            assert!(read_symbol_snippet(&ws, "src/escape.rs", 1, 2, 1024).is_err());
        }

        let _ = fs::remove_file(&outside);
        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn merge_external_symbols_dedups_and_prioritizes_external_lsp_hits() {
        let lsp_hit = WorkspaceSymbolEntry {
            name: "resolve_route".into(),
            kind: "fn".into(),
            path: "src/net.rs".into(),
            start_line: 10,
            end_line: 25,
            signature: Some("fn resolve_route(host: &Host) -> Route".into()),
            container: Some("Router".into()),
            language: "rust".into(),
        };
        let lexical_dup = WorkspaceSymbolEntry {
            name: "resolve_route".into(),
            kind: "fn".into(),
            path: "src/net.rs".into(),
            start_line: 10,
            end_line: 25,
            signature: Some("fn resolve_route".into()),
            container: None,
            language: "rust".into(),
        };
        let lexical_other = WorkspaceSymbolEntry {
            name: "resolve_route_fallback".into(),
            kind: "fn".into(),
            path: "src/net.rs".into(),
            start_line: 40,
            end_line: 55,
            signature: None,
            container: None,
            language: "rust".into(),
        };

        let merged = merge_external_symbols(
            vec![lsp_hit],
            vec![lexical_dup, lexical_other],
            "resolve_route",
            10,
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "resolve_route");
        assert_eq!(merged[0].container.as_deref(), Some("Router"));
        assert_eq!(merged[1].name, "resolve_route_fallback");
    }

    #[tokio::test]
    async fn rejects_filesystem_root_and_home_in_search_and_snippet() {
        let index = WorkspaceSymbolIndex::new();
        assert!(index.search(Path::new("/"), "foo", 10).await.is_err());
        assert!(read_symbol_snippet(Path::new("/"), "etc/hosts", 1, 5, 1024).is_err());
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
            && home.is_dir()
        {
            assert!(index.search(&home, "foo", 10).await.is_err());
            assert!(read_symbol_snippet(&home, ".bashrc", 1, 5, 1024).is_err());
        }
        // Verify Debug impl works cleanly
        let dbg = format!("{index:?}");
        assert!(dbg.contains("WorkspaceSymbolIndex"));
    }

    #[tokio::test]
    async fn gitignore_hardening_rejects_symlinks_oversized_and_quadratic_backtracking() {
        // 1. Stress test polynomial DP glob matcher on pathological repeated `**`
        let pathological = "**/**/**/**/**/**/**/**/**/**/**/**/**/never_matches.rs";
        let deep_path = "a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p/q/r/s/t/u/v/w/x/y/z/".repeat(4);
        let started = Instant::now();
        assert!(!glob_match(pathological, &deep_path));
        assert!(started.elapsed() < Duration::from_millis(100));
        assert!(glob_match("**/foo/*.rs", "a/b/foo/bar.rs"));
        assert!(glob_match("**/foo/*.rs", "foo/bar.rs"));
        assert!(!glob_match("foo/*.rs", "foo/sub/bar.rs"));

        // 2. Oversized > 256 KiB .gitignore and symlinked .gitignore are ignored safely
        let ws = temp_workspace("gi-harden");
        fs::create_dir_all(ws.join("big")).expect("mkdir big");
        fs::create_dir_all(ws.join("sym")).expect("mkdir sym");
        let huge_gi = "ignored_big.rs\n".repeat(25_000); // > 350 KiB
        fs::write(ws.join("big/.gitignore"), huge_gi).expect("write big gitignore");
        fs::write(ws.join("big/ignored_big.rs"), "pub fn big_visible() {}\n").expect("write rs");

        let outside_gi = std::env::temp_dir().join(format!("orbit-gi-{}.txt", Uuid::new_v4()));
        fs::write(&outside_gi, "sym_file.rs\n").expect("write outside gi");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside_gi, ws.join("sym/.gitignore"))
                .expect("symlink gitignore");
        }
        fs::write(ws.join("sym/sym_file.rs"), "pub fn sym_visible() {}\n").expect("write sym rs");

        let index = WorkspaceSymbolIndex::new();
        let res = index.search(&ws, "", 30).await.expect("search");
        let names: Vec<&str> = res.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"big_visible"));
        assert!(names.contains(&"sym_visible"));

        let _ = fs::remove_file(&outside_gi);
        let _ = fs::remove_dir_all(&ws);
    }

    #[tokio::test]
    async fn cache_hit_shares_arc_and_parent_child_invalidate_clears_nested() {
        let ws = temp_workspace("arc-cache");
        let nested = ws.join("crates/sub");
        fs::create_dir_all(&nested).expect("mkdir nested");
        fs::write(nested.join("mod.rs"), "pub fn alpha_fn() {}\n").expect("write mod.rs");

        let index = WorkspaceSymbolIndex::new();
        let _ = index.search(&ws, "alpha", 10).await.expect("search 1");
        let ptr1 = {
            let guard = index.cache.read().expect("read cache");
            let canon = crate::platform::canonicalize_clean(&ws).expect("canon");
            Arc::as_ptr(&guard.get(&canon).expect("cached").symbols)
        };
        let _ = index.search(&ws, "alpha", 10).await.expect("search 2");
        let ptr2 = {
            let guard = index.cache.read().expect("read cache");
            let canon = crate::platform::canonicalize_clean(&ws).expect("canon");
            Arc::as_ptr(&guard.get(&canon).expect("cached").symbols)
        };
        assert_eq!(ptr1, ptr2, "cache hit must reuse Arc without deep cloning");

        // Also cache nested workspace, then invalidate parent -> clears BOTH parent and nested
        let _ = index
            .search(&nested, "alpha", 10)
            .await
            .expect("search nested");
        assert_eq!(index.cache.read().expect("read").len(), 2);
        index.invalidate(&ws);
        assert_eq!(index.cache.read().expect("read").len(), 0);

        let _ = fs::remove_dir_all(&ws);
    }

    struct SlowExternalProvider {
        delay: Duration,
        observed_cwd: Arc<RwLock<Option<PathBuf>>>,
    }

    impl ExternalSymbolProvider for SlowExternalProvider {
        fn search_symbols<'a>(
            &'a self,
            cwd: &'a Path,
            _query: &'a str,
            _limit: usize,
        ) -> Pin<Box<dyn Future<Output = Vec<WorkspaceSymbolEntry>> + Send + 'a>> {
            let delay = self.delay;
            let observed = Arc::clone(&self.observed_cwd);
            let cwd_owned = cwd.to_path_buf();
            Box::pin(async move {
                if let Ok(mut slot) = observed.write() {
                    *slot = Some(cwd_owned);
                }
                tokio::time::sleep(delay).await;
                vec![WorkspaceSymbolEntry {
                    name: "from_external_lsp".into(),
                    kind: "fn".into(),
                    path: "src/lib.rs".into(),
                    start_line: 1,
                    end_line: 2,
                    signature: None,
                    container: None,
                    language: "rust".into(),
                }]
            })
        }
    }

    #[tokio::test]
    async fn external_provider_receives_canonical_cwd_and_is_timeout_bounded() {
        let ws = temp_workspace("ext-timeout");
        fs::create_dir_all(ws.join("src")).expect("mkdir src");
        fs::write(ws.join("src/lib.rs"), "pub fn local_fast_fn() {}\n").expect("write lib.rs");

        let observed = Arc::new(RwLock::new(None));
        let slow = Arc::new(SlowExternalProvider {
            delay: Duration::from_secs(5),
            observed_cwd: Arc::clone(&observed),
        });
        let index = WorkspaceSymbolIndex::with_external_provider(slow);

        let started = Instant::now();
        let res = index.search(&ws, "local_fast", 10).await.expect("search");
        assert!(
            started.elapsed() < Duration::from_millis(1800),
            "slow external provider must time out promptly"
        );
        assert_eq!(res.symbols.len(), 1);
        assert_eq!(res.symbols[0].name, "local_fast_fn");
        let canon = crate::platform::canonicalize_clean(&ws).expect("canon");
        assert_eq!(observed.read().expect("read").as_ref(), Some(&canon));

        let _ = fs::remove_dir_all(&ws);
    }

    #[test]
    fn read_symbol_snippet_clamps_zero_lines_and_resolves_worktree_gitdir_mtime() {
        let ws = temp_workspace("p2-checks");
        fs::write(
            ws.join("sample.rs"),
            "pub fn line_one() {}\npub fn line_two() {}\n",
        )
        .expect("write sample");

        // Zero start_line / end_line clamps safely to line 1
        let snippet = read_symbol_snippet(&ws, "sample.rs", 0, 0, 1024).expect("clamped snippet");
        assert_eq!(snippet, "1 | pub fn line_one() {}");

        // Worktree `.git` file pointing to external gitdir with `index`
        let git_common = temp_workspace("gitdir");
        fs::write(git_common.join("index"), b"idx").expect("write index");
        fs::write(
            ws.join(".git"),
            format!("gitdir: {}\n", git_common.display()),
        )
        .expect("write .git file");
        let canon_ws = fs::canonicalize(&ws).expect("canon ws");
        assert!(resolve_git_index_mtime(&canon_ws).is_some());

        let _ = fs::remove_dir_all(&git_common);
        let _ = fs::remove_dir_all(&ws);
    }
}
