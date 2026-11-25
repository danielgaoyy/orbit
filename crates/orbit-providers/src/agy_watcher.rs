//! Process-wide event-driven filesystem watcher for Antigravity brain directories.
//!
//! All Antigravity sessions in an Orbit daemon share a single OS-native `notify::RecommendedWatcher`
//! (`inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows) registered with
//! `RecursiveMode::NonRecursive` on leaf transcript/message directories only. This eliminates 200ms
//! idle polling while keeping `fs.inotify.max_user_watches` usage O(active sessions).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, warn};

static GLOBAL_WATCHER: OnceLock<Arc<AgyBrainWatcher>> = OnceLock::new();

#[derive(Debug)]
struct HandleEntry {
    desired_leaves: Vec<PathBuf>,
    target_pairs: Vec<(PathBuf, PathBuf)>,
    effective_dirs: HashSet<PathBuf>,
    tx: mpsc::Sender<()>,
}

struct WatcherState {
    watcher: Option<RecommendedWatcher>,
    handles: HashMap<u64, HandleEntry>,
    watched_dir_handles: HashMap<PathBuf, HashSet<u64>>,
    next_handle_id: u64,
    healthy: Arc<AtomicBool>,
}

impl WatcherState {
    fn new(watcher: Option<RecommendedWatcher>, healthy: Arc<AtomicBool>) -> Self {
        Self {
            watcher,
            handles: HashMap::new(),
            watched_dir_handles: HashMap::new(),
            next_handle_id: 1,
            healthy,
        }
    }

    fn apply_handle_targets(&mut self, handle_id: u64, normalized_leaves: Vec<PathBuf>) {
        let Some(entry) = self.handles.get_mut(&handle_id) else {
            return;
        };
        entry.desired_leaves = normalized_leaves;
        self.reconcile_handle(handle_id);
    }

    fn reconcile_handle(&mut self, handle_id: u64) {
        let Some(desired_leaves) = self
            .handles
            .get(&handle_id)
            .map(|h| h.desired_leaves.clone())
        else {
            return;
        };

        // Reconcile up to a small bound so parent->child directory creation races resolve cleanly.
        for _ in 0..8 {
            let mut desired_pairs: Vec<(PathBuf, PathBuf)> =
                Vec::with_capacity(desired_leaves.len());
            let mut candidate_effective: HashSet<PathBuf> = HashSet::new();
            let mut has_ancestor_fallback = false;
            for leaf in &desired_leaves {
                if let Some(eff) = resolve_effective_watch_dir(leaf) {
                    if &eff != leaf {
                        has_ancestor_fallback = true;
                    }
                    candidate_effective.insert(eff.clone());
                    desired_pairs.push((leaf.clone(), eff));
                }
            }

            let old_effective = self
                .handles
                .get(&handle_id)
                .map(|h| h.effective_dirs.clone())
                .unwrap_or_default();
            let unchanged = candidate_effective == old_effective
                && self
                    .handles
                    .get(&handle_id)
                    .is_some_and(|h| h.target_pairs == desired_pairs);
            if unchanged {
                return;
            }

            let mut applied_effective = old_effective.clone();
            let mut watch_race_retry = false;

            // Add new watches before dropping old ones so parent->child upgrades have no blind gap.
            for dir in candidate_effective.difference(&old_effective) {
                let dir_handles = self.watched_dir_handles.entry(dir.clone()).or_default();
                let need_os_watch = dir_handles.is_empty();
                let mut ok = true;
                if need_os_watch {
                    if let Some(ref mut watcher) = self.watcher {
                        if let Err(err) = watcher.watch(dir, RecursiveMode::NonRecursive) {
                            if !dir.is_dir() {
                                watch_race_retry = true;
                                ok = false;
                            } else {
                                warn!(
                                    path = %dir.display(),
                                    error = %err,
                                    "Failed to register OS watch on Antigravity brain directory; enabling periodic safety-net polling"
                                );
                                self.healthy.store(false, Ordering::Release);
                            }
                        }
                    } else {
                        self.healthy.store(false, Ordering::Release);
                    }
                }
                if ok {
                    dir_handles.insert(handle_id);
                    applied_effective.insert(dir.clone());
                } else if need_os_watch {
                    self.watched_dir_handles.remove(dir);
                }
            }

            for dir in old_effective.difference(&candidate_effective) {
                if let Some(dir_handles) = self.watched_dir_handles.get_mut(dir) {
                    dir_handles.remove(&handle_id);
                    if dir_handles.is_empty() {
                        self.watched_dir_handles.remove(dir);
                        if let Some(ref mut watcher) = self.watcher {
                            let _ = watcher.unwatch(dir);
                        }
                    }
                }
                applied_effective.remove(dir);
            }

            if let Some(entry) = self.handles.get_mut(&handle_id) {
                entry.effective_dirs = applied_effective;
                entry.target_pairs = desired_pairs;
            }

            if watch_race_retry {
                continue;
            }
            if !has_ancestor_fallback {
                break;
            }
            let deeper_appeared = self.handles.get(&handle_id).is_some_and(|h| {
                h.target_pairs
                    .iter()
                    .any(|(leaf, eff)| resolve_effective_watch_dir(leaf).as_ref() != Some(eff))
            });
            if !deeper_appeared {
                break;
            }
        }
    }

    fn remove_handle(&mut self, handle_id: u64) {
        let Some(removed) = self.handles.remove(&handle_id) else {
            return;
        };
        for dir in removed.effective_dirs {
            if let Some(dir_handles) = self.watched_dir_handles.get_mut(&dir) {
                dir_handles.remove(&handle_id);
                if dir_handles.is_empty() {
                    self.watched_dir_handles.remove(&dir);
                    if let Some(ref mut watcher) = self.watcher {
                        let _ = watcher.unwatch(&dir);
                    }
                }
            }
        }
    }
}

/// Shared OS-native file watcher for Antigravity session & subagent directories.
pub(crate) struct AgyBrainWatcher {
    state: Arc<Mutex<WatcherState>>,
    healthy: Arc<AtomicBool>,
}

impl AgyBrainWatcher {
    /// Returns the process-wide singleton watcher shared by all Antigravity sessions.
    pub(crate) fn global() -> Arc<Self> {
        GLOBAL_WATCHER.get_or_init(Self::new_internal).clone()
    }

    fn new_internal() -> Arc<Self> {
        let healthy = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(WatcherState::new(None, healthy.clone())));
        let cb_state = state.clone();

        match notify::recommended_watcher(move |res| {
            handle_notify_event(&cb_state, res);
        }) {
            Ok(watcher) => {
                healthy.store(true, Ordering::Release);
                let mut guard = match state.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                guard.watcher = Some(watcher);
            }
            Err(err) => {
                warn!(
                    error = %err,
                    "Failed to initialize OS file watcher for Antigravity transcripts; falling back to periodic safety-net polling"
                );
            }
        }

        Arc::new(Self { state, healthy })
    }

    /// Subscribe a session/monitor handle to a set of desired leaf directories.
    pub(crate) fn subscribe(self: &Arc<Self>, desired_leaves: Vec<PathBuf>) -> AgyWatchHandle {
        let (tx, rx) = mpsc::channel(1);
        let normalized_leaves = normalize_unique_leaves(desired_leaves);
        let handle_id = {
            let mut guard = match self.state.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            let id = guard.next_handle_id;
            guard.next_handle_id = guard.next_handle_id.saturating_add(1);
            guard.handles.insert(
                id,
                HandleEntry {
                    desired_leaves: normalized_leaves.clone(),
                    target_pairs: Vec::new(),
                    effective_dirs: HashSet::new(),
                    tx,
                },
            );
            guard.reconcile_handle(id);
            id
        };

        AgyWatchHandle {
            watcher: self.clone(),
            handle_id,
            desired_leaves: normalized_leaves,
            rx,
        }
    }

    /// Whether the underlying OS watch backend initialized and registered watches without error.
    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn active_watch_dir_count(&self) -> usize {
        let guard = match self.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.watched_dir_handles.len()
    }
}

/// Per-session watch subscription that automatically upgrades ancestor watches to leaf
/// directories and decrements reference counts on `Drop`.
pub(crate) struct AgyWatchHandle {
    watcher: Arc<AgyBrainWatcher>,
    handle_id: u64,
    desired_leaves: Vec<PathBuf>,
    rx: mpsc::Receiver<()>,
}

impl AgyWatchHandle {
    /// Update desired leaf directories (no-op if the normalized set is unchanged).
    pub(crate) fn set_targets(&mut self, desired_leaves: Vec<PathBuf>) {
        let normalized = normalize_unique_leaves(desired_leaves);
        if normalized == self.desired_leaves {
            self.reconcile_upgrades();
            return;
        }
        self.desired_leaves = normalized.clone();
        let mut guard = match self.watcher.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.apply_handle_targets(self.handle_id, normalized);
    }

    /// Re-evaluate ancestor fallbacks or missing-dir downgrades without changing desired leaves.
    pub(crate) fn reconcile_upgrades(&self) {
        let mut guard = match self.watcher.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.reconcile_handle(self.handle_id);
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.watcher.is_healthy()
    }

    /// Cancel-safe wait for a relevant filesystem notification on any subscribed directory.
    pub(crate) async fn recv(&mut self) {
        if self.rx.recv().await.is_some() {
            self.reconcile_upgrades();
        } else {
            std::future::pending::<()>().await;
        }
    }

    #[cfg(test)]
    fn effective_watch_dirs(&self) -> Vec<PathBuf> {
        let guard = match self.watcher.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut dirs: Vec<PathBuf> = guard
            .handles
            .get(&self.handle_id)
            .map(|h| h.effective_dirs.iter().cloned().collect())
            .unwrap_or_default();
        dirs.sort();
        dirs
    }
}

impl Drop for AgyWatchHandle {
    fn drop(&mut self) {
        let mut guard = match self.watcher.state.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.remove_handle(self.handle_id);
    }
}

fn handle_notify_event(state_mutex: &Mutex<WatcherState>, res: notify::Result<notify::Event>) {
    let event = match res {
        Ok(ev) => ev,
        Err(err) => {
            warn!(error = %err, "Antigravity brain watcher reported backend error");
            return;
        }
    };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }

    let normalized_paths: Vec<PathBuf> = event
        .paths
        .iter()
        .map(|p| normalize_watch_path(p))
        .collect();

    let to_notify: Vec<mpsc::Sender<()>> = {
        let guard = match state_mutex.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
            .handles
            .values()
            .filter(|entry| {
                if normalized_paths.is_empty() {
                    return !entry.effective_dirs.is_empty();
                }
                normalized_paths
                    .iter()
                    .any(|ep| handle_matches_event_path(entry, ep))
            })
            .map(|entry| entry.tx.clone())
            .collect()
    };

    if !to_notify.is_empty() {
        debug!(
            handles = to_notify.len(),
            kind = ?event.kind,
            "Antigravity brain watcher firing event wakeup"
        );
    }
    for tx in to_notify {
        let _ = tx.try_send(());
    }
}

fn handle_matches_event_path(entry: &HandleEntry, event_path: &Path) -> bool {
    for (leaf, effective) in &entry.target_pairs {
        if effective == leaf {
            // Directly watching the leaf directory: wake on any change inside `leaf`, on `leaf`
            // itself, or if `leaf` was removed/renamed via a parent path.
            if event_path.starts_with(leaf) {
                return true;
            }
        } else {
            // Watching an ancestor while `leaf` does not yet exist: wake only for events along the
            // ancestor chain toward `leaf` (or bare `effective` directory events), ignoring sibling
            // session directories in the same parent folder.
            if event_path == effective
                || leaf.starts_with(event_path)
                || event_path.starts_with(leaf)
            {
                return true;
            }
        }
    }
    false
}

fn normalize_unique_leaves(leaves: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(leaves.len());
    for leaf in leaves {
        let norm = normalize_watch_path(&leaf);
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out.sort();
    out
}

fn resolve_effective_watch_dir(normalized_leaf: &Path) -> Option<PathBuf> {
    for ancestor in normalized_leaf.ancestors() {
        if ancestor.is_dir() {
            return Some(normalize_watch_path(ancestor));
        }
    }
    None
}

pub(crate) fn normalize_watch_path(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    let clean = clean_path_lexically(&abs);

    let mut missing_suffix: Vec<OsString> = Vec::new();
    let mut cursor: Option<&Path> = Some(clean.as_path());
    while let Some(cur) = cursor {
        if let Ok(canon) = std::fs::canonicalize(cur) {
            let mut resolved = strip_verbatim_prefix(canon);
            for seg in missing_suffix.into_iter().rev() {
                resolved.push(seg);
            }
            return resolved;
        }
        if let Some(name) = cur.file_name() {
            missing_suffix.push(name.to_os_string());
            cursor = cur.parent();
        } else {
            break;
        }
    }
    strip_verbatim_prefix(clean)
}

fn clean_path_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        let raw = path.to_string_lossy();
        if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(stripped) = raw.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    struct TempTestDir(PathBuf);

    impl TempTestDir {
        fn new(prefix: &str) -> Self {
            let path = std::env::temp_dir().join(format!("{prefix}_{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).expect("create temp test dir");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn agy_watcher_wakes_on_file_append_and_unwatches_on_drop() {
        let dir = TempTestDir::new("orbit_agy_watch_append");
        let leaf = dir.path().join("brain/sess-a/.system_generated/logs");
        std::fs::create_dir_all(&leaf).expect("mkdir");
        let file_path = leaf.join("transcript_full.jsonl");
        std::fs::write(&file_path, "{\"step\":1}\n").expect("seed file");

        let watcher = AgyBrainWatcher::new_internal();
        assert!(watcher.is_healthy(), "OS watcher should initialize cleanly");

        let mut handle = watcher.subscribe(vec![leaf.clone()]);
        assert_eq!(watcher.active_watch_dir_count(), 1);
        assert_eq!(
            handle.effective_watch_dirs(),
            vec![normalize_watch_path(&leaf)]
        );

        // Drain any initial dir-creation event before testing a fresh append.
        let _ = tokio::time::timeout(Duration::from_millis(120), handle.recv()).await;

        {
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .expect("open append");
            writeln!(file, "{{\"step\":2}}").expect("append line");
            // On Windows NTFS, `File::flush()` is a no-op and `ReadDirectoryChangesW`
            // waits for buffers/metadata flush or handle close before emitting `Modified`.
            file.sync_all().expect("sync append");
        }

        tokio::time::timeout(Duration::from_secs(2), handle.recv())
            .await
            .expect("expected immediate watcher wakeup on file append");

        drop(handle);
        assert_eq!(
            watcher.active_watch_dir_count(),
            0,
            "dropping AgyWatchHandle must unwatch leaf dir when refcount reaches 0"
        );
    }

    #[tokio::test]
    async fn agy_watcher_upgrades_lazy_created_directories_to_leaf_watch() {
        let dir = TempTestDir::new("orbit_agy_watch_lazy");
        let brain_root = dir.path().join("brain");
        std::fs::create_dir_all(&brain_root).expect("mkdir brain");

        let desired_leaf = brain_root.join("sess-lazy/.system_generated/logs");
        let sibling_leaf = brain_root.join("sess-other/.system_generated/logs");
        std::fs::create_dir_all(&sibling_leaf).expect("mkdir sibling");

        let watcher = AgyBrainWatcher::new_internal();
        assert!(watcher.is_healthy());

        let mut handle = watcher.subscribe(vec![desired_leaf.clone()]);
        assert_eq!(
            handle.effective_watch_dirs(),
            vec![normalize_watch_path(&brain_root)],
            "missing desired leaf should watch deepest existing ancestor"
        );

        // Drain initial settling events if any.
        let _ = tokio::time::timeout(Duration::from_millis(100), handle.recv()).await;

        // Writing inside a sibling session's leaf should not wake this ancestor-watched handle.
        std::fs::write(sibling_leaf.join("transcript_full.jsonl"), "ignored\n")
            .expect("write sibling");
        assert!(
            tokio::time::timeout(Duration::from_millis(150), handle.recv())
                .await
                .is_err(),
            "unrelated sibling session write must not wake handle"
        );

        // Create desired session's leaf directory tree lazily and write transcript.
        std::fs::create_dir_all(&desired_leaf).expect("create lazy desired leaf");
        let log_file = desired_leaf.join("transcript_full.jsonl");
        std::fs::write(&log_file, "{\"step\":1}\n").expect("write target");

        // Wait up to 2s and trigger upgrade check; directory creation on `brain_root` wakes recv().
        tokio::time::timeout(Duration::from_secs(2), handle.recv())
            .await
            .expect("expected wakeup when lazy session directory was created");
        // Call `set_targets` / `reconcile_upgrades` until upgraded to the leaf dir.
        handle.reconcile_upgrades();
        assert_eq!(
            handle.effective_watch_dirs(),
            vec![normalize_watch_path(&desired_leaf)],
            "handle should automatically upgrade watch from ancestor to new leaf directory"
        );
        assert_eq!(
            watcher.active_watch_dir_count(),
            1,
            "old ancestor watch should be dropped after upgrade"
        );

        // Drain any stale directory-upgrade notification before testing direct leaf append.
        let _ = tokio::time::timeout(Duration::from_millis(100), handle.recv()).await;

        // Subsequent append directly inside the newly watched leaf wakes immediately.
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_file)
                .expect("open log_file");
            writeln!(f, "{{\"step\":2}}").expect("append step 2");
            f.sync_all().expect("sync step 2");
        }

        tokio::time::timeout(Duration::from_secs(2), handle.recv())
            .await
            .expect("upgraded leaf watch should wake on direct file append");
    }
}
