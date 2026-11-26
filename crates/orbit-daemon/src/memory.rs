//! Markdown owns memory content and review state. SQLite is a disposable search index.
//! Each mutation is serialized with reconciliation and commits its file before its index.

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use chrono::Utc;
use orbit_protocol::{
    MemoryCategory, MemoryEmbeddingConfig, MemoryEmbeddingProvider, MemoryStatus,
    ProjectMemoryItem, ProjectMemorySearchHit,
};
use orbit_store::{
    OrbitStore,
    memory::{embedding_text, normalize},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const PREFIX: &str = ".orbit/memory/";
const MAX_BYTES: usize = 256 * 1024;
const MAX_MEMORY_FILES: usize = 2048;
const MAX_SHARD_WORKSPACES: usize = 64;
const TOP_K: usize = 8;

const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

const BLOCKED_METADATA_HOSTS: &[&str] = &[
    "metadata.google.internal",
    "metadata.goog",
    "metadata",
    "instance-data",
];

#[derive(Deserialize)]
pub(crate) struct MemoryDraft {
    pub id: Option<String>,
    pub title: String,
    pub content: String,
    pub category: MemoryCategory,
    pub status: Option<MemoryStatus>,
    #[serde(default)]
    pub file_paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Frontmatter {
    id: String,
    title: String,
    category: MemoryCategory,
    status: MemoryStatus,
    confidence: f32,
    #[serde(default)]
    file_paths: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
    source_provider: Option<String>,
    source_agent_id: Option<String>,
    created_at_ms: i64,
    updated_at_ms: i64,
}

fn valid_id(id: &str) -> bool {
    if id.is_empty()
        || id.len() > 160
        || id.starts_with('.')
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return false;
    }
    let stem = id.split('.').next().unwrap_or(id).to_ascii_lowercase();
    !WINDOWS_RESERVED_NAMES.contains(&stem.as_str())
}

fn markdown_name(name: &str) -> bool {
    !name.starts_with('.')
        && name.ends_with(".md")
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(char::is_control)
}

fn bound_paths(paths: Vec<String>) -> Result<Vec<String>> {
    let mut bound = Vec::with_capacity(paths.len());
    for path in paths {
        let portable = path.replace('\\', "/");
        ensure!(
            !portable.starts_with('/')
                && !portable.contains('\0')
                && portable.as_bytes().get(1) != Some(&b':'),
            "expected a workspace-relative source path"
        );
        let parts: Vec<_> = portable
            .split('/')
            .filter(|p| !matches!(*p, "" | "."))
            .collect();
        ensure!(
            !parts.is_empty() && !parts.contains(&".."),
            "expected a workspace-relative source path"
        );
        let normalized = parts.join("/");
        if !bound.contains(&normalized) {
            bound.push(normalized);
        }
    }
    Ok(bound)
}

fn validate(item: &ProjectMemoryItem) -> Result<()> {
    let extra_bytes: usize = item
        .tags
        .iter()
        .chain(item.file_paths.iter())
        .map(String::len)
        .sum();
    ensure!(
        item.title
            .len()
            .saturating_add(item.content.len())
            .saturating_add(extra_bytes)
            <= MAX_BYTES,
        "memory exceeds {MAX_BYTES} bytes"
    );
    ensure!(
        item.tags.len() <= 64 && item.file_paths.len() <= 128,
        "memory has too many tags or bound file paths"
    );
    ensure!(
        valid_id(&item.id) && !item.id.starts_with("hermes-"),
        "invalid project memory id"
    );
    ensure!(
        !item.title.trim().is_empty() && !item.content.trim().is_empty(),
        "memory title and content are required"
    );
    ensure!(
        (0.0..=1.0).contains(&item.confidence),
        "confidence must be between zero and one"
    );
    Ok(())
}

fn markdown(item: &ProjectMemoryItem) -> Result<String> {
    validate(item)?;
    let header = Frontmatter {
        id: item.id.clone(),
        title: item.title.clone(),
        category: item.category,
        status: item.status,
        confidence: item.confidence,
        file_paths: item.file_paths.clone(),
        tags: item.tags.clone(),
        source_provider: item.source_provider.clone(),
        source_agent_id: item.source_agent_id.clone(),
        created_at_ms: item.created_at_ms,
        updated_at_ms: item.updated_at_ms,
    };
    let out = format!(
        "---\n{}---\n\n{}",
        serde_saphyr::to_string(&header)?,
        item.content
    );
    ensure!(out.len() <= MAX_BYTES, "memory exceeds {MAX_BYTES} bytes");
    Ok(out)
}

fn parse(root: &str, name: &str, text: &str) -> Result<ProjectMemoryItem> {
    let (header, content) = crate::rules::split_frontmatter(text);
    let h: Frontmatter = serde_saphyr::from_str(header.context("missing YAML frontmatter")?)?;
    let item = ProjectMemoryItem {
        id: h.id,
        workspace_root: root.into(),
        title: h.title,
        content,
        category: h.category,
        status: h.status,
        confidence: h.confidence,
        file_paths: bound_paths(h.file_paths)?,
        tags: h.tags,
        source_provider: h.source_provider,
        source_agent_id: h.source_agent_id,
        source_path: Some(format!("{PREFIX}{name}")),
        created_at_ms: h.created_at_ms,
        updated_at_ms: h.updated_at_ms,
        access_count: 0,
        last_accessed_ms: 0,
    };
    validate(&item)?;
    Ok(item)
}

fn memory_dir(root: &str, create: bool) -> Result<Option<Dir>> {
    let mut dir = Dir::open_ambient_dir(root, cap_std::ambient_authority())?;
    for component in [".orbit", "memory"] {
        if create
            && let Err(e) = dir.create_dir(component)
            && e.kind() != ErrorKind::AlreadyExists
        {
            return Err(e.into());
        }
        dir = match dir.open_dir_nofollow(component) {
            Ok(d) => d,
            Err(e) if e.kind() == ErrorKind::NotFound && !create => return Ok(None),
            Err(e) => return Err(e.into()),
        };
    }
    Ok(Some(dir))
}

fn read_md(dir: &Dir, name: &str) -> Result<String> {
    ensure!(
        dir.symlink_metadata(name)?.is_file(),
        "expected a regular Markdown file"
    );
    let file = dir.open_with(
        name,
        OpenOptions::new().read(true).follow(FollowSymlinks::No),
    )?;
    let mut text = String::new();
    file.take((MAX_BYTES + 1) as u64)
        .read_to_string(&mut text)?;
    ensure!(text.len() <= MAX_BYTES, "memory exceeds {MAX_BYTES} bytes");
    Ok(text)
}

fn local_name(item: &ProjectMemoryItem) -> Result<&str> {
    let name = item
        .source_path
        .as_deref()
        .and_then(|p| p.strip_prefix(PREFIX))
        .context("legacy memory is read-only")?;
    ensure!(markdown_name(name), "invalid memory filename");
    Ok(name)
}

fn write_md(root: &str, item: &ProjectMemoryItem, create: bool) -> Result<()> {
    let name = local_name(item)?;
    let text = markdown(item)?;
    let dir = memory_dir(root, true)?.context("missing memory directory")?;
    let tmp = format!(".orbit-edit-{}.tmp", Uuid::new_v4().simple());
    let mut file = dir.open_with(&tmp, OpenOptions::new().write(true).create_new(true))?;
    let result = (|| -> Result<()> {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        if create {
            // Publish atomically without overwriting a human-authored file at this slug.
            dir.hard_link(&tmp, &dir, name)?;
        } else {
            dir.rename(&tmp, &dir, name)?;
        }
        Ok(())
    })();
    let _ = dir.remove_file(&tmp);
    result
}

#[derive(Deserialize, Default)]
struct LegacyHeader {
    title: Option<String>,
    workspace_root: Option<String>,
}

/// Unscoped Hermes notes belong to the Hermes workspace. Notes with a workspace_root
/// in frontmatter can opt into that project's index. Never write to these source files.
fn legacy_item(
    root: &str,
    source: &Path,
    raw: &str,
    modified: std::time::SystemTime,
) -> Result<Option<ProjectMemoryItem>> {
    let (header, content) = crate::rules::split_frontmatter(raw);
    let meta = header
        .map(serde_saphyr::from_str::<LegacyHeader>)
        .transpose()?
        .unwrap_or_default();
    let owner = meta
        .workspace_root
        .as_deref()
        .map(Path::new)
        .or_else(|| source.parent()?.parent());
    if owner
        .and_then(|p| crate::platform::canonicalize_clean(p).ok())
        .as_deref()
        != Some(Path::new(root))
        || content.trim().is_empty()
    {
        return Ok(None);
    }
    let name = source
        .file_name()
        .and_then(|n| n.to_str())
        .context("invalid legacy filename")?;
    let stamp = chrono::DateTime::<Utc>::from(modified).timestamp_millis();
    Ok(Some(ProjectMemoryItem {
        id: format!("hermes-{:x}", Sha256::digest(name.as_bytes())),
        workspace_root: root.into(),
        title: meta
            .title
            .unwrap_or_else(|| name.trim_end_matches(".md").into()),
        content,
        category: MemoryCategory::Convention,
        status: MemoryStatus::Hypothesis,
        confidence: 0.5,
        file_paths: vec![],
        tags: vec![],
        source_provider: Some("hermes".into()),
        source_agent_id: None,
        source_path: Some(source.display().to_string()),
        created_at_ms: stamp,
        updated_at_ms: stamp,
        last_accessed_ms: 0,
        access_count: 0,
    }))
}

#[derive(Clone, PartialEq, Eq)]
struct IndexedFile {
    modified: std::time::SystemTime,
    size: u64,
    id: Option<String>,
}

#[derive(Default)]
struct ShardCache {
    workspaces: HashMap<String, HashMap<String, IndexedFile>>,
}

#[derive(Clone)]
pub(crate) struct MemoryEngine {
    store: Arc<OrbitStore>,
    pub config: MemoryEmbeddingConfig,
    legacy: Option<PathBuf>,
    dense_enabled: bool,
    locks: Arc<[Mutex<ShardCache>; 32]>,
    watcher_gates: Arc<[tokio::sync::Mutex<()>; 32]>,
}

fn validate_embedding_endpoint(raw: &str) -> Result<reqwest::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("embedding endpoint URL cannot be empty or contain control/whitespace".into());
    }
    let mut parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("invalid embedding endpoint URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported embedding endpoint scheme `{}` (expected http or https)",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("embedding endpoint URL must not contain embedded credentials".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "embedding endpoint URL is missing a host".to_string())?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if BLOCKED_METADATA_HOSTS.contains(&host.as_str()) {
        return Err(format!(
            "refusing to connect embedding endpoint to metadata host `{host}`"
        ));
    }
    let current_path = parsed.path().trim_end_matches('/');
    if !current_path.ends_with("/embeddings") {
        let next_path = if current_path.is_empty() {
            "/embeddings".to_string()
        } else {
            format!("{current_path}/embeddings")
        };
        parsed.set_path(&next_path);
    }
    Ok(parsed)
}

impl MemoryEngine {
    pub fn new(
        store: Arc<OrbitStore>,
        config: MemoryEmbeddingConfig,
        legacy: Option<PathBuf>,
    ) -> Self {
        let dense_enabled = config.provider == MemoryEmbeddingProvider::OpenAiCompatible;
        Self {
            store,
            config,
            legacy,
            dense_enabled,
            locks: Arc::new(std::array::from_fn(|_| Mutex::default())),
            watcher_gates: Arc::new(std::array::from_fn(|_| tokio::sync::Mutex::new(()))),
        }
    }

    fn embedding_model(&self) -> Option<(reqwest::Url, &str, String)> {
        let raw_url = self.config.endpoint_url.as_deref()?;
        let parsed = match validate_embedding_endpoint(raw_url) {
            Ok(url) => url,
            Err(reason) => {
                tracing::warn!(%reason, "invalid memory dense embedding endpoint");
                return None;
            }
        };
        let model = self.config.model.as_deref()?.trim();
        if model.is_empty() {
            tracing::warn!("memory dense embedding model name is empty");
            return None;
        }
        let index_key = format!("{model}@{:x}", Sha256::digest(parsed.as_str().as_bytes()));
        Some((parsed, model, index_key))
    }

    async fn dense(&self, text: &str) -> Option<(String, Vec<f32>)> {
        #[derive(Deserialize)]
        struct Embedding {
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Response {
            data: Vec<Embedding>,
        }
        if !self.dense_enabled {
            return None;
        }
        let (endpoint, model, index_key) = self.embedding_model()?;
        let (host_norm, pinned_addrs) =
            match crate::mcp_servers::resolve_and_validate_http_addrs(&endpoint).await {
                Ok(v) => v,
                Err(reason) => {
                    tracing::warn!(%reason, "blocked memory dense embedding endpoint");
                    return None;
                }
            };
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .resolve_to_addrs(&host_norm, &pinned_addrs)
            .build()
        {
            Ok(c) => c,
            Err(error) => {
                tracing::warn!(%error, "failed to build pinned dense embedding client");
                return None;
            }
        };
        let mut req = client
            .post(endpoint)
            .json(&serde_json::json!({"model":model,"input":text}));
        if let Some(var) = &self.config.api_key_env {
            let Ok(secret) = std::env::var(var) else {
                tracing::warn!(env_var = %var, "memory embedding API key env var is unset");
                return None;
            };
            req = req.bearer_auth(secret);
        }
        let mut response = match req.send().await.and_then(|r| r.error_for_status()) {
            Ok(r) => r,
            Err(error) => {
                tracing::warn!(%error, "memory dense embedding request failed");
                return None;
            }
        };
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                tracing::warn!("memory dense embedding response exceeded 1 MiB limit");
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        let res: Response = match serde_json::from_slice(&bytes) {
            Ok(r) => r,
            Err(error) => {
                tracing::warn!(%error, "memory dense embedding response JSON decode failed");
                return None;
            }
        };
        let mut v = res.data.into_iter().next()?.embedding;
        (v.len() <= 16384 && normalize(&mut v)).then_some((index_key, v))
    }

    async fn embed_item(&self, item: ProjectMemoryItem) {
        if self.dense_enabled
            && let Some((model, v)) = self.dense(&embedding_text(&item)).await
        {
            let store = self.store.clone();
            let _ = tokio::task::spawn_blocking(move || {
                store.set_dense_memory_vector(&item, &model, &v)
            })
            .await;
        }
    }

    /// Compare directory entries on access, including checkouts without a live OS watcher.
    /// Only changed files are opened. An explicit watcher event also invalidates an
    /// otherwise indistinguishable same-size, same-mtime edit.
    fn reconcile(
        &self,
        root: &str,
        shard: &mut ShardCache,
        events: &[String],
    ) -> Result<Vec<ProjectMemoryItem>> {
        let mut dirs = Vec::new();
        if let Some(dir) = memory_dir(root, false)? {
            dirs.push((dir, Path::new(PREFIX), false));
        }
        if let Some(path) = &self.legacy {
            match Dir::open_ambient_dir(path, cap_std::ambient_authority()) {
                Ok(dir) => dirs.push((dir, path, true)),
                Err(e) if e.kind() == ErrorKind::NotFound => {}
                Err(e) => tracing::warn!(%e,"cannot scan legacy Hermes memories"),
            }
        }
        let prev_files = shard.workspaces.get(root);
        let cold = prev_files.is_none();
        let mut next = HashMap::new();
        let mut changed = Vec::new();
        let mut ids = HashSet::new();
        let mut scanned = 0usize;
        for (dir, prefix, legacy) in dirs {
            let mut entries = Vec::new();
            for entry in dir.entries()? {
                let entry = match entry {
                    Ok(e) => e,
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                let filename = entry.file_name();
                let Some(name) = filename.to_str().filter(|n| markdown_name(n)) else {
                    continue;
                };
                entries.push((name.to_string(), entry));
            }
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, entry) in entries {
                if scanned >= MAX_MEMORY_FILES {
                    tracing::warn!(%root, limit = MAX_MEMORY_FILES, "memory directory exceeded scan limit; skipping remaining files");
                    break;
                }
                let is_file = match entry.file_type() {
                    Ok(ft) => ft.is_file(),
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                if !is_file {
                    continue;
                }
                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                let modified = match meta.modified() {
                    Ok(m) => m.into_std(),
                    Err(e) if e.kind() == ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                scanned += 1;
                let source = prefix.join(&name);
                let key = if legacy {
                    source.display().to_string()
                } else {
                    format!("{PREFIX}{name}")
                };
                let prev = prev_files.and_then(|files| files.get(&key));
                let mut id = if !cold
                    && let Some(previous) = prev
                    && previous.modified == modified
                    && previous.size == meta.len()
                    && !events.contains(&key)
                {
                    previous.id.clone()
                } else {
                    let parsed = match read_md(&dir, &name) {
                        Ok(text) => {
                            if legacy {
                                legacy_item(root, &source, &text, modified)
                            } else {
                                parse(root, &name, &text).map(Some)
                            }
                        }
                        Err(err)
                            if err
                                .downcast_ref::<std::io::Error>()
                                .is_some_and(|io| io.kind() == ErrorKind::NotFound) =>
                        {
                            continue;
                        }
                        Err(err) => Err(err),
                    };
                    match parsed {
                        Ok(Some(item)) => {
                            let id = item.id.clone();
                            changed.push(item);
                            Some(id)
                        }
                        Ok(None) => None,
                        Err(error) => {
                            tracing::warn!(%name,%error,"skipping unreadable memory");
                            None
                        }
                    }
                };
                if let Some(candidate) = &id
                    && !ids.insert(candidate.clone())
                {
                    tracing::warn!(%name, id = %candidate, "skipping duplicate Markdown memory id");
                    changed.retain(|item| {
                        item.id != *candidate || item.source_path.as_deref() != Some(key.as_str())
                    });
                    id = None;
                }
                next.insert(
                    key,
                    IndexedFile {
                        modified,
                        size: meta.len(),
                        id,
                    },
                );
            }
        }
        if !cold && changed.is_empty() && prev_files.is_some_and(|files| files == &next) {
            return Ok(Vec::new());
        }
        let model = self
            .dense_enabled
            .then(|| self.embedding_model())
            .flatten()
            .map(|(_, _, key)| key);
        let mut embed = Vec::new();
        for mut item in changed {
            if let Some(old) = self.store.project_memory(root, &item.id)? {
                item.access_count = old.access_count;
                item.last_accessed_ms = old.last_accessed_ms;
                if item == old {
                    if cold
                        && local_name(&item).is_ok()
                        && let Some(model) = &model
                        && !self.store.has_memory_vector_model(&item, model)?
                    {
                        embed.push(item);
                    }
                    continue;
                }
            }
            self.store.index_project_memory(&item)?;
            if self.dense_enabled && local_name(&item).is_ok() {
                embed.push(item);
            }
        }
        for old in self.store.project_memory_ids(root)? {
            if !ids.contains(&old) {
                self.store.remove_project_memory(root, &old)?;
            }
        }
        if shard.workspaces.len() >= MAX_SHARD_WORKSPACES && !shard.workspaces.contains_key(root) {
            shard.workspaces.clear();
        }
        shard.workspaces.insert(root.into(), next);
        Ok(embed)
    }

    async fn workspace<T: Send + 'static>(
        &self,
        cwd: &str,
        op: impl FnOnce(&Self, &str) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.workspace_after_events(cwd, Vec::new(), op).await
    }

    async fn workspace_after_events<T: Send + 'static>(
        &self,
        cwd: &str,
        events: Vec<String>,
        op: impl FnOnce(&Self, &str) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let engine = self.clone();
        let cwd = cwd.to_owned();
        let (result, changed) = tokio::task::spawn_blocking(move || -> Result<_> {
            let path = crate::platform::canonicalize_clean(Path::new(&cwd))?;
            let root = path.to_str().context("workspace path must be UTF-8")?;
            let mut hasher = DefaultHasher::new();
            root.hash(&mut hasher);
            let mut cache = engine.locks[hasher.finish() as usize % engine.locks.len()]
                .lock()
                .map_err(|_| anyhow!("memory lock poisoned"))?;
            let changed = engine.reconcile(root, &mut cache, &events)?;
            Ok((op(&engine, root), changed))
        })
        .await??;
        if !changed.is_empty() {
            let engine = self.clone();
            tokio::spawn(async move {
                for item in changed {
                    engine.embed_item(item).await;
                }
            });
        }
        result
    }

    pub async fn save(
        &self,
        cwd: &str,
        draft: MemoryDraft,
        source: Option<(String, String)>,
    ) -> Result<ProjectMemoryItem> {
        let now = Utc::now().timestamp_millis();
        let explicit = draft.id.is_some();
        let id = draft
            .id
            .unwrap_or_else(|| Uuid::new_v4().simple().to_string());
        let (provider, agent) = source.unzip();
        let mut item = ProjectMemoryItem {
            source_path: Some(format!("{PREFIX}{id}.md")),
            id,
            workspace_root: cwd.into(),
            title: draft.title,
            content: draft.content,
            category: draft.category,
            status: draft.status.unwrap_or(MemoryStatus::Hypothesis),
            confidence: 0.5,
            file_paths: bound_paths(draft.file_paths)?,
            tags: draft.tags,
            source_provider: provider,
            source_agent_id: agent,
            created_at_ms: now,
            updated_at_ms: now,
            last_accessed_ms: 0,
            access_count: 0,
        };
        validate(&item)?;
        let input = self.dense_enabled.then(|| embedding_text(&item));
        let vector = match &input {
            Some(text) => self.dense(text).await,
            None => None,
        };
        let (item, reembed) = self
            .workspace(cwd, move |engine, root| {
                item.workspace_root = root.into();
                let dense = vector
                    .as_ref()
                    .map(|(model, v)| (model.as_str(), v.as_slice()));
                let old = if explicit {
                    engine.store.project_memory(root, &item.id)?
                } else {
                    engine.store.near_duplicate_memory(root, &item, dense)?
                };
                let create = old.is_none();
                if let Some(old) = old {
                    local_name(&old)?;
                    if !explicit {
                        let same_evidence = old.category == item.category
                            && item.tags.iter().all(|tag| old.tags.contains(tag))
                            && item
                                .file_paths
                                .iter()
                                .all(|path| old.file_paths.contains(path));
                        for tag in old.tags {
                            if !item.tags.contains(&tag) {
                                item.tags.push(tag);
                            }
                        }
                        for path in old.file_paths {
                            if !item.file_paths.contains(&path) {
                                item.file_paths.push(path);
                            }
                        }
                        if draft.status.is_none()
                            && same_evidence
                            && old.title == item.title
                            && old.content == item.content
                        {
                            item.status = old.status;
                        }
                    }
                    item.id = old.id;
                    item.source_path = old.source_path;
                    item.created_at_ms = old.created_at_ms;
                    item.access_count = old.access_count;
                    item.last_accessed_ms = old.last_accessed_ms;
                    item.source_provider = item.source_provider.or(old.source_provider);
                    item.source_agent_id = item.source_agent_id.or(old.source_agent_id);
                }
                if item.status == MemoryStatus::Confirmed {
                    item.confidence = 1.0;
                }
                write_md(root, &item, create)?;
                engine.store.index_project_memory(&item)?;
                let needs_embedding = if let Some((model, v)) = dense {
                    if input.as_deref() == Some(&embedding_text(&item)) {
                        if let Err(error) = engine.store.set_dense_memory_vector(&item, model, v) {
                            tracing::warn!(%error,"could not cache optional dense memory vector");
                        }
                        false
                    } else {
                        true
                    }
                } else {
                    false
                };
                Ok((item, needs_embedding))
            })
            .await?;
        if reembed {
            self.embed_item(item.clone()).await;
        }
        Ok(item)
    }

    pub async fn confirm(&self, cwd: &str, id: String) -> Result<ProjectMemoryItem> {
        self.workspace(cwd, move |e, root| {
            ensure!(valid_id(&id), "invalid memory id");
            let mut item = e
                .store
                .project_memory(root, &id)?
                .context("unknown memory id")?;
            item.status = MemoryStatus::Confirmed;
            item.confidence = 1.0;
            item.updated_at_ms = Utc::now().timestamp_millis();
            write_md(root, &item, false)?;
            e.store.index_project_memory(&item)?;
            Ok(item)
        })
        .await
    }

    pub async fn forget(&self, cwd: &str, id: String) -> Result<bool> {
        self.workspace(cwd, move |e, root| {
            ensure!(valid_id(&id), "invalid memory id");
            let Some(item) = e.store.project_memory(root, &id)? else {
                return Ok(false);
            };
            let name = local_name(&item)?;
            memory_dir(root, false)?
                .context("missing memory directory")?
                .remove_file(name)?;
            Ok(e.store.remove_project_memory(root, &id)?)
        })
        .await
    }

    pub async fn list(
        &self,
        cwd: &str,
        status: Option<MemoryStatus>,
        category: Option<MemoryCategory>,
        query: Option<String>,
        limit: Option<u32>,
    ) -> Result<Vec<ProjectMemoryItem>> {
        self.workspace(cwd, move |e, root| {
            Ok(e.store.list_project_memories(
                root,
                status,
                category,
                query.as_deref(),
                limit.unwrap_or(50).min(250) as usize,
            )?)
        })
        .await
    }

    pub async fn recall(
        &self,
        cwd: &str,
        query: String,
        limit: Option<u32>,
        include_unconfirmed: bool,
    ) -> Result<Vec<ProjectMemorySearchHit>> {
        let dense = self.dense(&query).await;
        self.workspace(cwd, move |e, root| {
            Ok(e.store.recall_project_memories(
                root,
                &query,
                limit.unwrap_or(8).min(100) as usize,
                include_unconfirmed,
                dense.as_ref().map(|(m, v)| (m.as_str(), v.as_slice())),
            )?)
        })
        .await
    }

    pub async fn changed(&self, cwd: &str, paths: Vec<String>) -> Result<()> {
        let mut hasher = DefaultHasher::new();
        cwd.hash(&mut hasher);
        let _gate = self.watcher_gates[hasher.finish() as usize % self.watcher_gates.len()]
            .lock()
            .await;
        let normalized: Vec<String> = paths
            .into_iter()
            .filter_map(|p| orbit_store::memory::normalize_rel_path(&p))
            .collect();
        let memory_events: Vec<String> = normalized
            .iter()
            .filter(|p| p.starts_with(PREFIX))
            .cloned()
            .collect();
        self.workspace_after_events(cwd, memory_events, move |e, root| {
            let changed: Vec<_> = normalized
                .into_iter()
                .filter(|p| !p.starts_with(PREFIX))
                .collect();
            if changed.is_empty() {
                return Ok(());
            }
            let confirmed = e.store.confirmed_memories_for_paths(root, &changed)?;
            if confirmed.is_empty() {
                return Ok(());
            }
            let now = Utc::now().timestamp_millis();
            for mut item in confirmed {
                item.status = MemoryStatus::NeedsRevalidation;
                item.updated_at_ms = now;
                if let Err(error) = write_md(root, &item, false) {
                    tracing::warn!(id = %item.id, %error, "failed to update memory file status to needs_revalidation");
                    continue;
                }
                if let Err(error) = e.store.index_project_memory(&item) {
                    tracing::warn!(id = %item.id, %error, "failed to reindex memory after revalidation");
                }
            }
            Ok(())
        })
        .await
    }

    pub async fn build_confirmed_memory_context(
        &self,
        workspace_root: &str,
        prompt: &str,
        budget_tokens: u32,
    ) -> Result<String> {
        if budget_tokens == 0
            || !tokio::fs::try_exists(Path::new(workspace_root).join(".orbit/memory")).await?
        {
            return Ok(String::new());
        }
        let query = prompt
            .trim()
            .trim_end_matches(['.', '!', '?', '。', '！', '？']);
        let general = matches!(
            query.to_lowercase().as_str(),
            "" | "hi" | "hello" | "continue" | "help" | "你好" | "继续"
        );
        let dense = if !general && self.dense_enabled {
            let present = self
                .workspace(workspace_root, |e, root| {
                    Ok(!e.store.preferred_confirmed_memories(root, 1)?.is_empty())
                })
                .await?;
            if !present {
                return Ok(String::new());
            }
            self.dense(query).await
        } else {
            None
        };
        let query = query.to_owned();
        self.workspace(workspace_root, move |e, root| {
            let preferred = e.store.preferred_confirmed_memories(root, TOP_K)?;
            if preferred.is_empty() || general {
                return Ok(format_context(&preferred, budget_tokens));
            }
            let hits = e.store.recall_project_memories(
                root,
                &query,
                TOP_K,
                false,
                dense.as_ref().map(|(m, v)| (m.as_str(), v.as_slice())),
            )?;
            if hits.is_empty() {
                Ok(format_context(&preferred, budget_tokens))
            } else {
                let items = hits.into_iter().map(|h| h.item).collect::<Vec<_>>();
                Ok(format_context(&items, budget_tokens))
            }
        })
        .await
    }
}

fn format_context(items: &[ProjectMemoryItem], tokens: u32) -> String {
    const OPEN: &str = "<orbit_project_memory untrusted=\"true\">\n";
    const CLOSE: &str = "\n</orbit_project_memory>";
    let mut remaining = (tokens as usize)
        .saturating_mul(4)
        .saturating_sub(OPEN.len() + CLOSE.len());
    let mut body = String::new();
    for item in items {
        let prefix = if body.is_empty() { "- " } else { "\n- " };
        if remaining <= prefix.len() {
            break;
        }
        let mut line_budget = remaining - prefix.len();
        let raw_content = format!("{} — {}", item.title, item.content);
        let mut escaped_content = String::new();
        for ch in raw_content.chars() {
            let escaped = match ch {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                _ => "",
            };
            let cost = if escaped.is_empty() {
                ch.len_utf8()
            } else {
                escaped.len()
            };
            if cost > line_budget {
                line_budget = 0;
                break;
            }
            if escaped.is_empty() {
                escaped_content.push(ch);
            } else {
                escaped_content.push_str(escaped);
            }
            line_budget -= cost;
        }
        if escaped_content.is_empty() {
            break;
        }
        body.push_str(prefix);
        body.push_str(&escaped_content);
        remaining = line_budget;
    }
    if body.is_empty() {
        String::new()
    } else {
        format!("{OPEN}{body}{CLOSE}")
    }
}

pub(crate) async fn execute_tool(
    engine: &MemoryEngine,
    caller: &orbit_protocol::AgentSummary,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    execute_tool_for_root(engine, &caller.cwd, caller, args).await
}

pub(crate) async fn execute_tool_for_root(
    engine: &MemoryEngine,
    root: &str,
    caller: &orbit_protocol::AgentSummary,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    #[derive(Deserialize)]
    #[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
    enum Action {
        Recall {
            query: String,
            limit: Option<u32>,
            include_unconfirmed: Option<bool>,
        },
        Remember {
            id: Option<String>,
            title: String,
            content: String,
            category: Option<MemoryCategory>,
            #[serde(default)]
            file_paths: Vec<String>,
            #[serde(default)]
            tags: Vec<String>,
        },
        Confirm {
            id: String,
        },
        Forget {
            id: String,
        },
    }
    use serde_json::json;
    Ok(match serde_json::from_value(args.clone())? {
        Action::Recall {
            query,
            limit,
            include_unconfirmed,
        } => {
            json!({"hits":engine.recall(root,query,limit,include_unconfirmed.unwrap_or(false)).await?})
        }
        Action::Remember {
            id,
            title,
            content,
            category,
            file_paths,
            tags,
        } => {
            let draft = MemoryDraft {
                id,
                title,
                content,
                category: category.unwrap_or(MemoryCategory::Convention),
                status: None,
                file_paths,
                tags,
            };
            json!({"item":engine.save(root,draft,Some((caller.provider.slug().into(),caller.agent_id.clone()))).await?})
        }
        Action::Confirm { id } => json!({"item":engine.confirm(root,id).await?}),
        Action::Forget { id } => json!({"deleted":engine.forget(root,id.clone()).await?,"id":id}),
    })
}

#[cfg(test)]
pub(crate) mod tests;
