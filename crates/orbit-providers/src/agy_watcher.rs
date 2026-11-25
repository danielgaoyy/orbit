//! Process-wide event-driven filesystem watcher for Antigravity brain directories.
//!
//! All Antigravity sessions in an Orbit daemon share a single OS-native `notify::RecommendedWatcher`
//! (`inotify` on Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows) registered with
//! `RecursiveMode::NonRecursive` on leaf transcript/message directories only. This eliminates 200ms
//! idle polling while keeping `fs.inotify.max_user_watches` usage O(active sessions).

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc as std_mpsc};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, warn};

static GLOBAL_WATCHER: OnceLock<Arc<AgyBrainWatcher>> = OnceLock::new();

#[derive(Debug, Clone)]
struct HandleRoute {
    target_pairs: Vec<(PathBuf, PathBuf)>,
    effective_dirs: HashSet<PathBuf>,
    tx: mpsc::Sender<()>,
}

#[derive(Debug)]
struct ManagerHandleEntry {
    desired_leaves: Vec<PathBuf>,
    target_pairs: Vec<(PathBuf, PathBuf)>,
    effective_dirs: HashSet<PathBuf>,
    tx: mpsc::Sender<()>,
    reconcile_queued: Arc<AtomicBool>,
}

enum WatcherCmd {
    Subscribe {
        handle_id: u64,
        desired_leaves: Vec<PathBuf>,
        tx: mpsc::Sender<()>,
        reconcile_queued: Arc<AtomicBool>,
    },
    SetTargets {
        handle_id: u64,
        desired_leaves: Vec<PathBuf>,
    },
    Reconcile {
        handle_id: u64,
    },
    Remove {
        handle_id: u64,
    },
    #[cfg(test)]
    Inspect {
        reply: std_mpsc::Sender<(usize, HashMap<u64, Vec<PathBuf>>)>,
    },
}

struct ManagerState {
    watcher: Option<RecommendedWatcher>,
    handles: HashMap<u64, ManagerHandleEntry>,
    watched_dir_handles: HashMap<PathBuf, HashSet<u64>>,
    routes: Arc<Mutex<HashMap<u64, HandleRoute>>>,
    healthy: Arc<AtomicBool>,
}

impl ManagerState {
    fn new(
        watcher: Option<RecommendedWatcher>,
        routes: Arc<Mutex<HashMap<u64, HandleRoute>>>,
        healthy: Arc<AtomicBool>,
    ) -> Self {
        Self {
            watcher,
            handles: HashMap::new(),
            watched_dir_handles: HashMap::new(),
            routes,
            healthy,
        }
    }

    fn subscribe_handle(
        &mut self,
        handle_id: u64,
        desired_leaves: Vec<PathBuf>,
        tx: mpsc::Sender<()>,
        reconcile_queued: Arc<AtomicBool>,
    ) {
        let normalized_leaves = normalize_unique_leaves(desired_leaves);
        self.handles.insert(
            handle_id,
            ManagerHandleEntry {
                desired_leaves: normalized_leaves,
                target_pairs: Vec::new(),
                effective_dirs: HashSet::new(),
                tx,
                reconcile_queued,
            },
        );
        self.reconcile_handle(handle_id);
    }

    fn apply_handle_targets(&mut self, handle_id: u64, desired_leaves: Vec<PathBuf>) {
        let normalized_leaves = normalize_unique_leaves(desired_leaves);
        let Some(entry) = self.handles.get_mut(&handle_id) else {
            return;
        };
        if entry.desired_leaves != normalized_leaves {
            entry.desired_leaves = normalized_leaves;
        }
        self.reconcile_handle(handle_id);
    }

    fn clear_reconcile_queued(&self, handle_id: u64) {
        if let Some(entry) = self.handles.get(&handle_id) {
            entry.reconcile_queued.store(false, Ordering::Release);
        }
    }

    fn reconcile_handle(&mut self, handle_id: u64) {
        let Some((desired_leaves, tx)) = self
            .handles
            .get(&handle_id)
            .map(|h| (h.desired_leaves.clone(), h.tx.clone()))
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

            // Publish candidate routes before registering OS watches so there is zero blind gap,
            // and release `routes` immediately before calling `watcher.watch`/`unwatch` so we can
            // never AB-BA deadlock with `notify`'s event callback thread.
            let pre_effective: HashSet<PathBuf> = old_effective
                .iter()
                .chain(candidate_effective.iter())
                .cloned()
                .collect();
            {
                let mut routes = match self.routes.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                routes.insert(
                    handle_id,
                    HandleRoute {
                        target_pairs: desired_pairs.clone(),
                        effective_dirs: pre_effective,
                        tx: tx.clone(),
                    },
                );
            }

            let mut applied_effective = old_effective.clone();
            let mut watch_race_retry = false;
            let mut mutated_os_watches = false;
            let mut added_effective_dir = false;

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
                        } else {
                            mutated_os_watches = true;
                        }
                    } else {
                        self.healthy.store(false, Ordering::Release);
                    }
                }
                if ok {
                    dir_handles.insert(handle_id);
                    applied_effective.insert(dir.clone());
                    added_effective_dir = true;
                } else if need_os_watch {
                    self.watched_dir_handles.remove(dir);
                }
            }

            for dir in old_effective.difference(&candidate_effective) {
                if let Some(dir_handles) = self.watched_dir_handles.get_mut(dir) {
                    dir_handles.remove(&handle_id);
                    if dir_handles.is_empty() {
                        self.watched_dir_handles.remove(dir);
                        if let Some(ref mut watcher) = self.watcher
                            && watcher.unwatch(dir).is_ok()
                        {
                            mutated_os_watches = true;
                        }
                    }
                }
                applied_effective.remove(dir);
            }

            if let Some(entry) = self.handles.get_mut(&handle_id) {
                entry.effective_dirs = applied_effective.clone();
                entry.target_pairs = desired_pairs.clone();
            }
            {
                let mut routes = match self.routes.lock() {
                    Ok(g) => g,
                    Err(poisoned) => poisoned.into_inner(),
                };
                if self.handles.contains_key(&handle_id) {
                    routes.insert(
                        handle_id,
                        HandleRoute {
                            target_pairs: desired_pairs,
                            effective_dirs: applied_effective,
                            tx: tx.clone(),
                        },
                    );
                }
            }

            // `subscribe()` and `set_targets()` enqueue asynchronously onto `orbit-agy-watcher`.
            // Send one coalesced wakeup once a new effective directory watch is active so any
            // file write that landed during the async registration window (before `inotify` /
            // `FSEvents` / `ReadDirectoryChangesW` attached) is caught immediately instead of
            // waiting for the 5-second safety-net poll.
            if added_effective_dir {
                let _ = tx.try_send(());
            }

            #[cfg(target_os = "macos")]
            if mutated_os_watches {
                self.nudge_handles_after_fsevents_restart(Some(handle_id));
            }
            #[cfg(not(target_os = "macos"))]
            let _ = mutated_os_watches;

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

    #[cfg(target_os = "macos")]
    fn nudge_handles_after_fsevents_restart(&self, exclude_handle_id: Option<u64>) {
        // `notify::FsEventWatcher` stops and recreates the process-wide `FSEventStream` with
        // `kFSEventStreamEventIdSinceNow` whenever watched paths change. Nudge other live
        // handles once so a write landing during that stream restart can never be silently lost.
        for (&id, entry) in &self.handles {
            if Some(id) != exclude_handle_id && !entry.effective_dirs.is_empty() {
                let _ = entry.tx.try_send(());
            }
        }
    }

    fn remove_handle(&mut self, handle_id: u64) {
        let Some(removed) = self.handles.remove(&handle_id) else {
            return;
        };
        {
            let mut routes = match self.routes.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            routes.remove(&handle_id);
        }
        let mut mutated_os_watches = false;
        for dir in removed.effective_dirs {
            if let Some(dir_handles) = self.watched_dir_handles.get_mut(&dir) {
                dir_handles.remove(&handle_id);
                if dir_handles.is_empty() {
                    self.watched_dir_handles.remove(&dir);
                    if let Some(ref mut watcher) = self.watcher
                        && watcher.unwatch(&dir).is_ok()
                    {
                        mutated_os_watches = true;
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        if mutated_os_watches {
            self.nudge_handles_after_fsevents_restart(None);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = mutated_os_watches;
    }
}

fn run_watcher_manager(mut state: ManagerState, cmd_rx: std_mpsc::Receiver<WatcherCmd>) {
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            WatcherCmd::Subscribe {
                handle_id,
                desired_leaves,
                tx,
                reconcile_queued,
            } => {
                state.subscribe_handle(handle_id, desired_leaves, tx, reconcile_queued);
            }
            WatcherCmd::SetTargets {
                handle_id,
                desired_leaves,
            } => {
                state.apply_handle_targets(handle_id, desired_leaves);
            }
            WatcherCmd::Reconcile { handle_id } => {
                state.clear_reconcile_queued(handle_id);
                state.reconcile_handle(handle_id);
            }
            WatcherCmd::Remove { handle_id } => {
                state.remove_handle(handle_id);
            }
            #[cfg(test)]
            WatcherCmd::Inspect { reply } => {
                let dir_count = state.watched_dir_handles.len();
                let handle_dirs = state
                    .handles
                    .iter()
                    .map(|(&id, entry)| {
                        let mut dirs: Vec<PathBuf> = entry.effective_dirs.iter().cloned().collect();
                        dirs.sort();
                        (id, dirs)
                    })
                    .collect();
                let _ = reply.send((dir_count, handle_dirs));
            }
        }
    }
}

/// Shared OS-native file watcher for Antigravity session & subagent directories.
pub(crate) struct AgyBrainWatcher {
    cmd_tx: std_mpsc::Sender<WatcherCmd>,
    next_handle_id: AtomicU64,
    healthy: Arc<AtomicBool>,
}

impl AgyBrainWatcher {
    /// Returns the process-wide singleton watcher shared by all Antigravity sessions.
    pub(crate) fn global() -> Arc<Self> {
        GLOBAL_WATCHER.get_or_init(Self::new_internal).clone()
    }

    fn new_internal() -> Arc<Self> {
        let healthy = Arc::new(AtomicBool::new(false));
        let routes = Arc::new(Mutex::new(HashMap::new()));
        let cb_routes = Arc::clone(&routes);

        let watcher = match notify::recommended_watcher(move |res| {
            handle_notify_event(&cb_routes, res);
        }) {
            Ok(watcher) => {
                healthy.store(true, Ordering::Release);
                Some(watcher)
            }
            Err(err) => {
                warn!(
                    error = %err,
                    "Failed to initialize OS file watcher for Antigravity transcripts; falling back to periodic safety-net polling"
                );
                None
            }
        };

        let (cmd_tx, cmd_rx) = std_mpsc::channel();
        let manager_state = ManagerState::new(watcher, routes, Arc::clone(&healthy));
        if let Err(err) = std::thread::Builder::new()
            .name("orbit-agy-watcher".to_string())
            .spawn(move || run_watcher_manager(manager_state, cmd_rx))
        {
            warn!(
                error = %err,
                "Failed to spawn Antigravity brain watcher thread; falling back to periodic safety-net polling"
            );
            healthy.store(false, Ordering::Release);
        }

        Arc::new(Self {
            cmd_tx,
            next_handle_id: AtomicU64::new(1),
            healthy,
        })
    }

    /// Subscribe a session/monitor handle to a set of desired leaf directories.
    pub(crate) fn subscribe(self: &Arc<Self>, desired_leaves: Vec<PathBuf>) -> AgyWatchHandle {
        let (tx, rx) = mpsc::channel(1);
        let handle_id = self.next_handle_id.fetch_add(1, Ordering::Relaxed);
        let reconcile_queued = Arc::new(AtomicBool::new(false));
        let last_raw_leaves = desired_leaves.clone();

        let _ = self.cmd_tx.send(WatcherCmd::Subscribe {
            handle_id,
            desired_leaves,
            tx,
            reconcile_queued: Arc::clone(&reconcile_queued),
        });

        AgyWatchHandle {
            watcher: self.clone(),
            handle_id,
            last_raw_leaves,
            reconcile_queued,
            rx,
        }
    }

    /// Whether the underlying OS watch backend initialized and registered watches without error.
    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn inspect(&self) -> (usize, HashMap<u64, Vec<PathBuf>>) {
        let (reply_tx, reply_rx) = std_mpsc::channel();
        if self
            .cmd_tx
            .send(WatcherCmd::Inspect { reply: reply_tx })
            .is_err()
        {
            return (0, HashMap::new());
        }
        reply_rx.recv().unwrap_or_default()
    }

    #[cfg(test)]
    fn active_watch_dir_count(&self) -> usize {
        self.inspect().0
    }
}

/// Per-session watch subscription that automatically upgrades ancestor watches to leaf
/// directories and decrements reference counts on `Drop`.
pub(crate) struct AgyWatchHandle {
    watcher: Arc<AgyBrainWatcher>,
    handle_id: u64,
    last_raw_leaves: Vec<PathBuf>,
    reconcile_queued: Arc<AtomicBool>,
    rx: mpsc::Receiver<()>,
}

impl AgyWatchHandle {
    /// Update desired leaf directories (no-op if unchanged, aside from upgrade reconciliation).
    pub(crate) fn set_targets(&mut self, desired_leaves: Vec<PathBuf>) {
        if desired_leaves == self.last_raw_leaves {
            self.reconcile_upgrades();
            return;
        }
        self.last_raw_leaves = desired_leaves.clone();
        let _ = self.watcher.cmd_tx.send(WatcherCmd::SetTargets {
            handle_id: self.handle_id,
            desired_leaves,
        });
    }

    /// Re-evaluate ancestor fallbacks or missing-dir downgrades without changing desired leaves.
    pub(crate) fn reconcile_upgrades(&self) {
        if !self.reconcile_queued.swap(true, Ordering::AcqRel) {
            let _ = self.watcher.cmd_tx.send(WatcherCmd::Reconcile {
                handle_id: self.handle_id,
            });
        }
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
        self.watcher
            .inspect()
            .1
            .remove(&self.handle_id)
            .unwrap_or_default()
    }
}

impl Drop for AgyWatchHandle {
    fn drop(&mut self) {
        let _ = self.watcher.cmd_tx.send(WatcherCmd::Remove {
            handle_id: self.handle_id,
        });
    }
}

fn handle_notify_event(
    routes_mutex: &Mutex<HashMap<u64, HandleRoute>>,
    res: notify::Result<notify::Event>,
) {
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
        let guard = match routes_mutex.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard
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

fn handle_matches_event_path(entry: &HandleRoute, event_path: &Path) -> bool {
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

        // Drain initial registration/dir-creation events until quiescent before testing a fresh append.
        while tokio::time::timeout(Duration::from_millis(120), handle.recv())
            .await
            .is_ok()
        {}

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

        // Drain initial settling events (both synthetic registration wakeup and any macOS FSEvents
        // latency from sibling directory creation above) until quiescent.
        while tokio::time::timeout(Duration::from_millis(120), handle.recv())
            .await
            .is_ok()
        {}

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

        // Drain any stale directory-upgrade notification (including synthetic watch-registration
        // wakeup) until quiescent before testing direct leaf append.
        while tokio::time::timeout(Duration::from_millis(100), handle.recv())
            .await
            .is_ok()
        {}

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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agy_watcher_concurrent_events_and_watch_mutations_never_deadlock() {
        let dir = TempTestDir::new("orbit_agy_watch_stress");
        let brain_root = dir.path().join("brain");
        std::fs::create_dir_all(&brain_root).expect("mkdir brain");

        let mut leaf_dirs = Vec::new();
        for i in 0..4 {
            let leaf = brain_root.join(format!("sess-{i}/.system_generated/logs"));
            std::fs::create_dir_all(&leaf).expect("mkdir leaf");
            std::fs::write(leaf.join("transcript_full.jsonl"), "{\"step\":0}\n")
                .expect("seed file");
            leaf_dirs.push(leaf);
        }

        let watcher = AgyBrainWatcher::new_internal();
        assert!(watcher.is_healthy(), "OS watcher should initialize cleanly");

        let stop_writers = Arc::new(AtomicBool::new(false));
        let mut writer_threads = Vec::new();
        for w_idx in 0..2 {
            let stop = Arc::clone(&stop_writers);
            let dirs = leaf_dirs.clone();
            let root = brain_root.clone();
            writer_threads.push(std::thread::spawn(move || {
                let mut seq = 0u64;
                while !stop.load(Ordering::Relaxed) {
                    let target_dir = &dirs[(seq as usize + w_idx) % dirs.len()];
                    let main_file = target_dir.join("transcript_full.jsonl");
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&main_file)
                    {
                        let _ = writeln!(f, "{{\"w\":{w_idx},\"seq\":{seq}}}");
                    }
                    let churn_dir = root.join(format!("ephemeral-{w_idx}-{}", seq % 3));
                    let _ = std::fs::create_dir_all(&churn_dir);
                    let churn_file = churn_dir.join("tmp.jsonl");
                    let _ = std::fs::write(&churn_file, "x\n");
                    let _ = std::fs::remove_file(&churn_file);
                    let _ = std::fs::remove_dir_all(&churn_dir);
                    seq = seq.saturating_add(1);
                }
            }));
        }

        let stop_watchdog = Arc::new(AtomicBool::new(false));
        let watchdog_stop = Arc::clone(&stop_watchdog);
        let watchdog = tokio::spawn(async move {
            while !watchdog_stop.load(Ordering::Relaxed) {
                let scheduled_at = std::time::Instant::now();
                tokio::time::sleep(Duration::from_millis(20)).await;
                let elapsed = scheduled_at.elapsed();
                assert!(
                    elapsed < Duration::from_millis(500),
                    "Tokio worker stalled for {elapsed:?} during concurrent watch mutations"
                );
            }
        });

        let stress_fut = async {
            let mut tasks = Vec::new();
            for task_idx in 0..8usize {
                let w = Arc::clone(&watcher);
                let dirs = leaf_dirs.clone();
                let root = brain_root.clone();
                tasks.push(tokio::spawn(async move {
                    for iter in 0..24usize {
                        let d0 = dirs[(task_idx + iter) % dirs.len()].clone();
                        let d1 = root.join(format!("ephemeral-{}-{}", task_idx % 2, iter % 3));
                        let mut handle = w.subscribe(vec![d0.clone()]);
                        handle.reconcile_upgrades();
                        handle.set_targets(vec![d0.clone(), d1.clone()]);
                        let _ = tokio::time::timeout(Duration::from_millis(8), handle.recv()).await;
                        handle.set_targets(vec![dirs[(task_idx + iter + 1) % dirs.len()].clone()]);
                        handle.reconcile_upgrades();
                        drop(handle);
                    }
                }));
            }
            for task in tasks {
                task.await.expect("worker task panicked");
            }
        };

        let result = tokio::time::timeout(Duration::from_secs(10), stress_fut).await;
        stop_writers.store(true, Ordering::Relaxed);
        stop_watchdog.store(true, Ordering::Relaxed);
        for t in writer_threads {
            let _ = t.join();
        }
        watchdog.await.expect("watchdog task failed");
        result.expect("concurrent AgyBrainWatcher operations deadlocked or exceeded 10s timeout");

        assert_eq!(
            watcher.active_watch_dir_count(),
            0,
            "all OS watches must clean up cleanly after concurrent stress test"
        );
    }

    #[tokio::test]
    async fn agy_watcher_wakes_when_write_races_async_subscribe() {
        let dir = TempTestDir::new("orbit_agy_watch_race");
        let leaf = dir.path().join("brain/sess-race/.system_generated/logs");
        std::fs::create_dir_all(&leaf).expect("mkdir");
        let file_path = leaf.join("transcript_full.jsonl");
        std::fs::write(&file_path, "{\"step\":0}\n").expect("seed");

        let watcher = AgyBrainWatcher::new_internal();
        assert!(watcher.is_healthy());

        // Write immediately after `subscribe()` returns without waiting for the async
        // `orbit-agy-watcher` thread to finish registering the OS watch.
        let mut handle = watcher.subscribe(vec![leaf]);
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&file_path)
                .expect("open append");
            writeln!(f, "{{\"step\":1}}").expect("append");
            f.sync_all().expect("sync");
        }

        tokio::time::timeout(Duration::from_millis(500), handle.recv())
            .await
            .expect("write racing async subscribe must wake immediately via post-register nudge");
    }
}
