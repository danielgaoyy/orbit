//! OS-native debounced filesystem watcher (`notify`) for active Orbit workspaces.
//!
//! Watches active workspace roots (via FSEvents on macOS, inotify on Linux, ReadDirectoryChangesW
//! on Windows), filters out noisy build/VCS internals (`.git/objects`, `.git/logs`, `target/`,
//! `node_modules/`, `dist/`, `.next/`, `build/`), coalesces bursts over a ~250ms debounce window,
//! and emits lightweight invalidation batches for symbol-index, skills/rules, and git/files UI sync.
//!
//! Hardening guarantees:
//! - Strictly bounded watched-root count (`MAX_WATCHED_ROOTS = 64`) and per-event path cap
//!   (`MAX_COALESCED_PATHS_PER_EVENT = 64`).
//! - Bounded non-blocking channel (`try_send`) from the OS watcher callback thread so bursts never
//!   stall FSEvents/inotify delivery.
//! - Graceful degradation when OS watch limits (`ENOSPC` / `EMFILE`) or permissions fail: logs a
//!   warning and continues without ever crashing the daemon.
//! - Automatic unwatching as soon as a workspace is archived or removed.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Default debounce window coalescing bursts of file writes / git index updates.
pub const DEFAULT_WATCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Maximum age a pending workspace batch may be delayed under continuous traffic before flushing.
pub const MAX_WATCH_BATCH_AGE: Duration = Duration::from_millis(1500);

/// Hard cap on distinct canonical workspace root directories watched simultaneously.
pub const MAX_WATCHED_ROOTS: usize = 64;

/// Hard cap on per-directory OS watches registered under a single workspace root on platforms
/// (like Linux inotify) that allocate one OS watch per subdirectory.
pub const MAX_WATCHED_DIRS_PER_ROOT: usize = 4096;

/// Cooldown after a workspace root fails OS watch registration (e.g. `ENOSPC` / `EMFILE`)
/// so subsequent sessions targeting the same root do not repeatedly re-scan disk.
pub const WATCH_FAILURE_COOLDOWN: Duration = Duration::from_secs(300);

/// Maximum relative paths included in a single coalesced `WorkspaceFilesChanged` event.
pub const MAX_COALESCED_PATHS_PER_EVENT: usize = 64;

/// Maximum relative paths retained per debounced batch for background memory revalidation.
pub const MAX_REVALIDATION_PATHS_PER_EVENT: usize = 4096;

/// Bounded event queue between the OS watcher callback thread and the Tokio debouncer task.
const RAW_EVENT_CHANNEL_CAPACITY: usize = 1024;

/// Bounded command queue for serialized background workspace watch registrations.
const WATCH_REGISTER_CHANNEL_CAPACITY: usize = 256;

/// Noisy directories ignored anywhere inside a watched workspace tree.
const IGNORED_DIR_COMPONENTS: &[&str] = &[
    "target",
    "node_modules",
    "dist",
    ".next",
    "build",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    ".nuxt",
    ".svelte-kit",
    ".parcel-cache",
    ".pytest_cache",
    ".mypy_cache",
];

/// Coalesced invalidation batch emitted after the debounce window settles for a workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceWatchBatch {
    pub workspace_id: String,
    pub cwd: String,
    pub canonical_root: PathBuf,
    pub changed_paths: Vec<String>,
    pub revalidation_paths: Vec<String>,
    pub is_primary_root_event: bool,
    pub git_changed: bool,
    pub skills_or_rules_changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClassifiedPathChange {
    pub rel_path: String,
    pub is_git_state: bool,
    pub is_skills_or_rules: bool,
}

/// Filter and classify a raw filesystem path relative to a watched canonical workspace root.
///
/// Returns `None` for ignored build/VCS noise (`.git/objects`, `.git/logs`, `target/`,
/// `node_modules/`, `dist/`, `.next/`, `build/`, editor swap/temp files).
pub(crate) fn classify_watched_path(root: &Path, raw_path: &Path) -> Option<ClassifiedPathChange> {
    let rel = raw_path.strip_prefix(root).ok()?;
    let mut comps: Vec<String> = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(os) => {
                let s = os.to_string_lossy();
                if !s.is_empty() {
                    comps.push(s.into_owned());
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if comps.is_empty() {
        return None;
    }

    let file_name = comps.last().map(String::as_str).unwrap_or("");
    if file_name.eq_ignore_ascii_case(".DS_Store")
        || file_name.eq_ignore_ascii_case("Thumbs.db")
        || file_name.starts_with(".orbit-edit-")
        || file_name.starts_with(".orbit-plan-edit-")
        || file_name.ends_with(".swp")
        || file_name.ends_with(".swo")
        || file_name.ends_with(".swx")
    {
        return None;
    }

    // Handle `.git` directory/file carefully: ignore `.git/objects`, `.git/logs`, and lockfiles,
    // while preserving high-signal git state triggers (`.git/HEAD`, `.git/index`, `.git/refs/heads`, etc.).
    if comps[0].eq_ignore_ascii_case(".git") {
        if comps.len() == 1 {
            // Worktree `.git` pointer file or `.git` root update
            return Some(ClassifiedPathChange {
                rel_path: ".git".to_string(),
                is_git_state: true,
                is_skills_or_rules: false,
            });
        }
        let sub = comps[1].to_ascii_lowercase();
        if sub == "objects" || sub == "logs" || sub == "hooks" || sub == "info" {
            return None;
        }
        if file_name.ends_with(".lock") {
            return None;
        }
        let is_git_state = matches!(
            sub.as_str(),
            "head"
                | "index"
                | "packed-refs"
                | "config"
                | "merge_head"
                | "rebase_head"
                | "cherry_pick_head"
                | "orig_head"
                | "fetch_head"
                | "refs"
                | "rebase-merge"
                | "rebase-apply"
                | "worktrees"
        );
        if !is_git_state {
            return None;
        }
        return Some(ClassifiedPathChange {
            rel_path: comps.join("/"),
            is_git_state: true,
            is_skills_or_rules: false,
        });
    }

    // Ignore noisy build/dependency/cache subtrees anywhere under the workspace root
    for seg in &comps {
        let lower = seg.to_ascii_lowercase();
        if lower == ".git" || IGNORED_DIR_COMPONENTS.iter().any(|ign| *ign == lower) {
            return None;
        }
    }

    let rel_path = comps.join("/");
    let is_skills_or_rules = is_skills_or_rules_rel_path(&comps);
    Some(ClassifiedPathChange {
        rel_path,
        is_git_state: false,
        is_skills_or_rules,
    })
}

fn is_skills_or_rules_rel_path(comps: &[String]) -> bool {
    if comps.is_empty() {
        return false;
    }
    let first = comps[0].to_ascii_lowercase();
    if matches!(first.as_str(), ".orbit" | ".claude" | ".cursor" | ".agents") {
        if comps.len() == 1 {
            return true;
        }
        let second = comps[1].to_ascii_lowercase();
        if matches!(second.as_str(), "rules" | "skills" | "hooks.json") {
            return true;
        }
    }
    let leaf = comps.last().map(String::as_str).unwrap_or("");
    leaf.eq_ignore_ascii_case("AGENTS.md")
        || leaf.eq_ignore_ascii_case("CLAUDE.md")
        || leaf.eq_ignore_ascii_case("GEMINI.md")
        || leaf.eq_ignore_ascii_case(".cursorrules")
        || leaf.eq_ignore_ascii_case(".mcp.json")
        || leaf.eq_ignore_ascii_case("orbit.json")
        || leaf.eq_ignore_ascii_case("paseo.json")
}

fn is_interesting_event_kind(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

#[cfg(any(test, not(any(target_os = "macos", target_os = "windows"))))]
fn should_skip_dir_on_watch(rel_components: &[String]) -> bool {
    if rel_components.is_empty() {
        return false;
    }
    if rel_components[0].eq_ignore_ascii_case(".git") {
        if rel_components.len() == 1 {
            return false;
        }
        let sub = rel_components[1].to_ascii_lowercase();
        return !matches!(
            sub.as_str(),
            "refs" | "worktrees" | "rebase-merge" | "rebase-apply"
        );
    }
    for seg in rel_components {
        let lower = seg.to_ascii_lowercase();
        if lower == ".git"
            || IGNORED_DIR_COMPONENTS.iter().any(|ign| *ign == lower)
            || matches!(
                lower.as_str(),
                "venv"
                    | "virtualenv"
                    | ".tox"
                    | ".ruff_cache"
                    | ".gradle"
                    | ".idea"
                    | ".vscode-server"
            )
        {
            return true;
        }
    }
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn collect_rel_components(root: &Path, path: &Path) -> Option<Vec<String>> {
    let rel = path.strip_prefix(root).ok()?;
    let mut comps = Vec::new();
    for comp in rel.components() {
        match comp {
            Component::Normal(os) => {
                let s = os.to_string_lossy();
                if !s.is_empty() {
                    comps.push(s.into_owned());
                }
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(comps)
}

struct WatcherRegistryState {
    raw_tx: mpsc::Sender<Event>,
    /// Dedicated OS watcher per canonical workspace root so failed registration or unwatch
    /// cleanly drops every partial inotify/FSEvents watch for that root without leaking.
    root_watchers: HashMap<PathBuf, RecommendedWatcher>,
    /// `workspace_id -> (canonical_root, display_cwd)`
    workspace_to_root: HashMap<String, (PathBuf, String)>,
    /// `canonical_root -> set of workspace_ids`
    root_to_workspaces: HashMap<PathBuf, HashSet<String>>,
    /// Number of OS directory watches registered for each root (used for Linux per-root budget).
    root_watched_dir_counts: HashMap<PathBuf, usize>,
    /// Roots that hit `MAX_WATCHED_DIRS_PER_ROOT` during initial or dynamic registration.
    root_truncated: HashSet<PathBuf>,
    /// Negative cache for roots that recently failed OS watch registration (e.g. `ENOSPC`).
    failed_roots: HashMap<PathBuf, Instant>,
    /// Monotonic generation token per `workspace_id` for cancelling stale async registrations.
    workspace_watch_gen: HashMap<String, u64>,
    next_gen: AtomicU64,
}

impl WatcherRegistryState {
    fn new(raw_tx: mpsc::Sender<Event>) -> Self {
        Self {
            raw_tx,
            root_watchers: HashMap::new(),
            workspace_to_root: HashMap::new(),
            root_to_workspaces: HashMap::new(),
            root_watched_dir_counts: HashMap::new(),
            root_truncated: HashSet::new(),
            failed_roots: HashMap::new(),
            workspace_watch_gen: HashMap::new(),
            next_gen: AtomicU64::new(1),
        }
    }

    fn alloc_watch_gen(&mut self, workspace_id: &str) -> u64 {
        let watch_gen = self.next_gen.fetch_add(1, Ordering::Relaxed);
        self.workspace_watch_gen
            .insert(workspace_id.to_string(), watch_gen);
        watch_gen
    }

    fn clear_watch_gen_if_matches(&mut self, workspace_id: &str, expected_gen: Option<u64>) {
        if let Some(exp) = expected_gen
            && self.workspace_watch_gen.get(workspace_id) == Some(&exp)
        {
            self.workspace_watch_gen.remove(workspace_id);
        }
    }
}

struct WatchRegisterCommand {
    workspace_id: String,
    cwd: PathBuf,
    expected_gen: u64,
}

#[derive(Default)]
struct PendingRootBatch {
    changed_paths: BTreeSet<String>,
    revalidation_paths: BTreeSet<String>,
    git_changed: bool,
    skills_or_rules_changed: bool,
    first_event_at: Option<Instant>,
    last_event_at: Option<Instant>,
}

type WatchCallback = Arc<dyn Fn(WorkspaceWatchBatch) + Send + Sync>;

/// Thread-safe, debounced, bounded OS-native file watcher managing all active Orbit workspaces.
pub struct WorkspaceWatcherManager {
    state: Arc<Mutex<WatcherRegistryState>>,
    watch_cmd_tx: mpsc::Sender<WatchRegisterCommand>,
    debouncer_abort: tokio::task::AbortHandle,
    worker_abort: tokio::task::AbortHandle,
}

impl Drop for WorkspaceWatcherManager {
    fn drop(&mut self) {
        self.debouncer_abort.abort();
        self.worker_abort.abort();
    }
}

impl WorkspaceWatcherManager {
    /// Start a new debounced `WorkspaceWatcherManager` using `DEFAULT_WATCH_DEBOUNCE` (250ms).
    pub fn new<F>(on_change: F) -> Arc<Self>
    where
        F: Fn(WorkspaceWatchBatch) + Send + Sync + 'static,
    {
        Self::with_debounce(DEFAULT_WATCH_DEBOUNCE, on_change)
    }

    /// Start a new `WorkspaceWatcherManager` with a custom debounce window.
    pub fn with_debounce<F>(debounce: Duration, on_change: F) -> Arc<Self>
    where
        F: Fn(WorkspaceWatchBatch) + Send + Sync + 'static,
    {
        Self::with_debounce_and_max_age(debounce, MAX_WATCH_BATCH_AGE, on_change)
    }

    fn with_debounce_and_max_age<F>(
        debounce: Duration,
        max_batch_age: Duration,
        on_change: F,
    ) -> Arc<Self>
    where
        F: Fn(WorkspaceWatchBatch) + Send + Sync + 'static,
    {
        let (raw_tx, raw_rx) = mpsc::channel::<Event>(RAW_EVENT_CHANNEL_CAPACITY);
        let (watch_cmd_tx, watch_cmd_rx) =
            mpsc::channel::<WatchRegisterCommand>(WATCH_REGISTER_CHANNEL_CAPACITY);
        let state = Arc::new(Mutex::new(WatcherRegistryState::new(raw_tx)));
        let cb: WatchCallback = Arc::new(on_change);
        let eff_debounce = debounce.max(Duration::from_millis(20));
        let eff_max_age = max_batch_age.max(eff_debounce);
        let debouncer_handle = tokio::spawn(run_debouncer_loop(
            Arc::clone(&state),
            raw_rx,
            eff_debounce,
            eff_max_age,
            cb,
        ));
        let worker_handle = tokio::spawn(run_watch_worker_loop(Arc::clone(&state), watch_cmd_rx));

        Arc::new(Self {
            state,
            watch_cmd_tx,
            debouncer_abort: debouncer_handle.abort_handle(),
            worker_abort: worker_handle.abort_handle(),
        })
    }

    /// Enqueue workspace watch registration without blocking the caller.
    pub fn watch_workspace_async(&self, workspace_id: &str, cwd: &Path) {
        let ws_id = workspace_id.trim();
        if ws_id.is_empty() {
            return;
        }
        let expected_gen = {
            let Ok(mut guard) = self.state.lock() else {
                return;
            };
            guard.alloc_watch_gen(ws_id)
        };
        if let Err(err) = self.watch_cmd_tx.try_send(WatchRegisterCommand {
            workspace_id: ws_id.to_string(),
            cwd: cwd.to_path_buf(),
            expected_gen,
        }) {
            debug!(
                workspace_id = %ws_id,
                cwd = %cwd.display(),
                error = %err,
                "Skipped async workspace watch registration: queue unavailable"
            );
            if let Ok(mut guard) = self.state.lock() {
                guard.clear_watch_gen_if_matches(ws_id, Some(expected_gen));
            }
        }
    }

    /// Start watching a workspace root directory synchronously.
    ///
    /// Fails gracefully (logs a warning, returns `false`) if the directory does not exist,
    /// `MAX_WATCHED_ROOTS` is reached, the root is in failure cooldown, or the OS watcher hits
    /// inotify/FSEvents watch limits.
    pub fn watch_workspace(&self, workspace_id: &str, cwd: &Path) -> bool {
        let ws_id = workspace_id.trim();
        if ws_id.is_empty() {
            return false;
        }
        let expected_gen = {
            let Ok(mut guard) = self.state.lock() else {
                return false;
            };
            guard.alloc_watch_gen(ws_id)
        };
        watch_workspace_inner(
            &self.state,
            ws_id,
            cwd,
            Some(expected_gen),
            MAX_WATCHED_DIRS_PER_ROOT,
        )
    }

    /// Stop watching `workspace_id`, releasing the underlying OS watch when no other active
    /// workspace references the same canonical directory and cancelling any pending async watch.
    pub fn unwatch_workspace(&self, workspace_id: &str) {
        let ws_id = workspace_id.trim();
        if ws_id.is_empty() {
            return;
        }
        let Ok(mut guard) = self.state.lock() else {
            return;
        };
        guard.workspace_watch_gen.remove(ws_id);
        if let Some((root, _)) = guard.workspace_to_root.remove(ws_id) {
            remove_workspace_from_root_locked(&mut guard, ws_id, &root);
        }
    }

    /// Enqueue watch registration for all active (non-archived) workspaces without blocking startup.
    pub fn watch_active_workspaces_async(&self, workspaces: &[(String, PathBuf)]) {
        for (ws_id, cwd) in workspaces {
            self.watch_workspace_async(ws_id, cwd);
        }
    }

    /// Register all active (non-archived) workspaces at daemon startup.
    pub fn watch_active_workspaces(&self, workspaces: &[(String, PathBuf)]) {
        for (ws_id, cwd) in workspaces {
            let _ = self.watch_workspace(ws_id, cwd);
        }
    }

    /// Return the count of currently watched distinct canonical workspace roots.
    pub fn watched_root_count(&self) -> usize {
        self.state
            .lock()
            .map(|g| g.root_to_workspaces.len())
            .unwrap_or(0)
    }

    /// Return the count of tracked active workspace registrations.
    pub fn watched_workspace_count(&self) -> usize {
        self.state
            .lock()
            .map(|g| g.workspace_to_root.len())
            .unwrap_or(0)
    }

    /// Return the count of registered OS directory watches for `cwd`.
    pub fn watched_dir_count(&self, cwd: &Path) -> usize {
        let Ok(canonical_root) = crate::platform::canonicalize_clean(cwd) else {
            return 0;
        };
        self.state
            .lock()
            .map(|g| {
                g.root_watched_dir_counts
                    .get(&canonical_root)
                    .copied()
                    .unwrap_or(0)
            })
            .unwrap_or(0)
    }

    /// Return whether `cwd` hit `MAX_WATCHED_DIRS_PER_ROOT` during watch registration.
    pub fn is_root_truncated(&self, cwd: &Path) -> bool {
        let Ok(canonical_root) = crate::platform::canonicalize_clean(cwd) else {
            return false;
        };
        self.state
            .lock()
            .map(|g| g.root_truncated.contains(&canonical_root))
            .unwrap_or(false)
    }
}

async fn run_watch_worker_loop(
    state: Arc<Mutex<WatcherRegistryState>>,
    mut cmd_rx: mpsc::Receiver<WatchRegisterCommand>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        let state_bg = Arc::clone(&state);
        let _ = tokio::task::spawn_blocking(move || {
            watch_workspace_inner(
                &state_bg,
                &cmd.workspace_id,
                &cmd.cwd,
                Some(cmd.expected_gen),
                MAX_WATCHED_DIRS_PER_ROOT,
            );
        })
        .await;
    }
}

fn watch_workspace_inner(
    state: &Arc<Mutex<WatcherRegistryState>>,
    workspace_id: &str,
    cwd: &Path,
    expected_gen: Option<u64>,
    max_dirs_per_root: usize,
) -> bool {
    let ws_id = workspace_id.trim();
    if ws_id.is_empty() {
        return false;
    }
    let Ok(canonical_root) = crate::platform::canonicalize_clean(cwd) else {
        debug!(
            workspace_id = %ws_id,
            cwd = %cwd.display(),
            "Skipping workspace watch: directory unavailable"
        );
        if let Ok(mut guard) = state.lock() {
            guard.clear_watch_gen_if_matches(ws_id, expected_gen);
        }
        return false;
    };
    if !canonical_root.is_dir() {
        if let Ok(mut guard) = state.lock() {
            guard.clear_watch_gen_if_matches(ws_id, expected_gen);
        }
        return false;
    }
    let display_cwd = cwd.to_string_lossy().to_string();

    let raw_tx = {
        let Ok(mut guard) = state.lock() else {
            return false;
        };
        if let Some(exp) = expected_gen
            && guard.workspace_watch_gen.get(ws_id) != Some(&exp)
        {
            return false;
        }

        // If this workspace was previously mapped to a different root, clean up the old mapping first.
        if let Some((prev_root, prev_cwd)) = guard.workspace_to_root.get(ws_id).cloned() {
            if prev_root == canonical_root {
                if prev_cwd != display_cwd {
                    guard
                        .workspace_to_root
                        .insert(ws_id.to_string(), (canonical_root, display_cwd));
                }
                guard.clear_watch_gen_if_matches(ws_id, expected_gen);
                return true;
            }
            remove_workspace_from_root_locked(&mut guard, ws_id, &prev_root);
        }

        if guard.root_to_workspaces.contains_key(&canonical_root) {
            guard
                .workspace_to_root
                .insert(ws_id.to_string(), (canonical_root.clone(), display_cwd));
            guard
                .root_to_workspaces
                .entry(canonical_root)
                .or_default()
                .insert(ws_id.to_string());
            guard.clear_watch_gen_if_matches(ws_id, expected_gen);
            return true;
        }

        let now = Instant::now();
        guard.failed_roots.retain(|_, until| now < *until);
        if guard.failed_roots.contains_key(&canonical_root) {
            guard.clear_watch_gen_if_matches(ws_id, expected_gen);
            return false;
        }

        if guard.root_to_workspaces.len() >= MAX_WATCHED_ROOTS {
            warn!(
                workspace_id = %ws_id,
                root = %canonical_root.display(),
                cap = MAX_WATCHED_ROOTS,
                "Reached maximum watched workspace root limit; skipping watch"
            );
            guard.clear_watch_gen_if_matches(ws_id, expected_gen);
            return false;
        }

        guard.raw_tx.clone()
    };

    let config = Config::default().with_follow_symlinks(false);
    let mut root_watcher = match RecommendedWatcher::new(
        move |res: notify::Result<Event>| match res {
            Ok(ev) => {
                if is_interesting_event_kind(&ev.kind) && !ev.paths.is_empty() {
                    let _ = raw_tx.try_send(ev);
                }
            }
            Err(err) => {
                warn!(error = %err, "OS workspace file watcher reported error");
            }
        },
        config,
    ) {
        Ok(w) => w,
        Err(err) => {
            warn!(
                workspace_id = %ws_id,
                root = %canonical_root.display(),
                error = %err,
                "Failed to create OS workspace file watcher; continuing without live FS events"
            );
            if let Ok(mut guard) = state.lock() {
                guard
                    .failed_roots
                    .insert(canonical_root, Instant::now() + WATCH_FAILURE_COOLDOWN);
                guard.clear_watch_gen_if_matches(ws_id, expected_gen);
            }
            return false;
        }
    };

    let (watched_dirs, truncated) =
        match register_os_watches(&mut root_watcher, &canonical_root, ws_id, max_dirs_per_root) {
            Ok(stats) => stats,
            Err(err) => {
                // Dropping `root_watcher` here immediately releases any partial OS watches
                // registered before hitting ENOSPC / EMFILE.
                drop(root_watcher);
                warn!(
                    workspace_id = %ws_id,
                    root = %canonical_root.display(),
                    error = %err,
                    "OS file watcher failed to watch workspace (possible inotify/FSEvents limit)"
                );
                if let Ok(mut guard) = state.lock() {
                    guard
                        .failed_roots
                        .insert(canonical_root, Instant::now() + WATCH_FAILURE_COOLDOWN);
                    guard.clear_watch_gen_if_matches(ws_id, expected_gen);
                }
                return false;
            }
        };

    let Ok(mut guard) = state.lock() else {
        drop(root_watcher);
        return false;
    };
    if let Some(exp) = expected_gen
        && guard.workspace_watch_gen.get(ws_id) != Some(&exp)
    {
        drop(root_watcher);
        return false;
    }
    guard.clear_watch_gen_if_matches(ws_id, expected_gen);

    if guard.root_to_workspaces.contains_key(&canonical_root) {
        drop(root_watcher);
        guard
            .workspace_to_root
            .insert(ws_id.to_string(), (canonical_root.clone(), display_cwd));
        guard
            .root_to_workspaces
            .entry(canonical_root)
            .or_default()
            .insert(ws_id.to_string());
        return true;
    }
    if guard.root_to_workspaces.len() >= MAX_WATCHED_ROOTS {
        drop(root_watcher);
        return false;
    }

    guard
        .root_watchers
        .insert(canonical_root.clone(), root_watcher);
    guard
        .root_watched_dir_counts
        .insert(canonical_root.clone(), watched_dirs);
    if truncated {
        guard.root_truncated.insert(canonical_root.clone());
    } else {
        guard.root_truncated.remove(&canonical_root);
    }
    guard
        .workspace_to_root
        .insert(ws_id.to_string(), (canonical_root.clone(), display_cwd));
    guard
        .root_to_workspaces
        .entry(canonical_root)
        .or_default()
        .insert(ws_id.to_string());
    true
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn register_os_watches(
    root_watcher: &mut RecommendedWatcher,
    canonical_root: &Path,
    _ws_id: &str,
    _max_dirs_per_root: usize,
) -> notify::Result<(usize, bool)> {
    root_watcher.watch(canonical_root, RecursiveMode::Recursive)?;
    Ok((1, false))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn register_os_watches(
    root_watcher: &mut RecommendedWatcher,
    canonical_root: &Path,
    ws_id: &str,
    max_dirs_per_root: usize,
) -> notify::Result<(usize, bool)> {
    register_pruned_bfs_watches(root_watcher, canonical_root, ws_id, max_dirs_per_root)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn register_pruned_bfs_watches(
    root_watcher: &mut RecommendedWatcher,
    canonical_root: &Path,
    ws_id: &str,
    max_dirs_per_root: usize,
) -> notify::Result<(usize, bool)> {
    use std::collections::VecDeque;

    let cap = max_dirs_per_root.max(1);
    let mut queue: VecDeque<(PathBuf, Vec<String>)> = VecDeque::new();
    queue.push_back((canonical_root.to_path_buf(), Vec::new()));

    let mut watched_dirs = 0usize;
    let mut truncated = false;

    while let Some((dir, rel_comps)) = queue.pop_front() {
        if watched_dirs >= cap {
            truncated = true;
            break;
        }
        if let Err(err) = root_watcher.watch(&dir, RecursiveMode::NonRecursive) {
            if dir == canonical_root {
                return Err(err);
            }
            if is_watch_limit_or_resource_error(&err) {
                return Err(err);
            }
            debug!(
                workspace_id = %ws_id,
                dir = %dir.display(),
                error = %err,
                "Skipping unreadable subdirectory while registering workspace watch"
            );
            continue;
        }
        watched_dirs += 1;

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if !ft.is_dir() || ft.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.is_empty() {
                continue;
            }
            let mut child_rel = rel_comps.clone();
            child_rel.push(name_str.into_owned());
            if should_skip_dir_on_watch(&child_rel) {
                continue;
            }
            queue.push_back((entry.path(), child_rel));
        }
    }

    if truncated {
        warn!(
            workspace_id = %ws_id,
            root = %canonical_root.display(),
            watched_dirs = watched_dirs,
            cap = cap,
            "Workspace exceeded per-root directory watch cap; shallow directories are watched and deeper subtrees use on-demand refresh"
        );
    }

    Ok((watched_dirs, truncated))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn is_watch_limit_or_resource_error(err: &notify::Error) -> bool {
    match &err.kind {
        notify::ErrorKind::MaxFilesWatch => true,
        notify::ErrorKind::Io(io_err) => matches!(io_err.raw_os_error(), Some(28 | 24 | 12)),
        _ => false,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn attach_dynamic_subdir_watches(
    state: &Arc<Mutex<WatcherRegistryState>>,
    root: &Path,
    dir_path: &Path,
) -> Vec<PathBuf> {
    use std::collections::VecDeque;

    let Ok(meta) = std::fs::symlink_metadata(dir_path) else {
        return Vec::new();
    };
    if !meta.file_type().is_dir() {
        return Vec::new();
    }
    let Some(rel_comps) = collect_rel_components(root, dir_path) else {
        return Vec::new();
    };
    if should_skip_dir_on_watch(&rel_comps) {
        return Vec::new();
    }

    const MAX_DYNAMIC_DIRS_PER_EVENT: usize = 64;
    let mut discovered_files = Vec::new();
    let Ok(mut guard) = state.lock() else {
        return discovered_files;
    };
    if guard.root_truncated.contains(root) {
        return discovered_files;
    }
    let mut watched_count = guard
        .root_watched_dir_counts
        .get(root)
        .copied()
        .unwrap_or(0);
    if watched_count >= MAX_WATCHED_DIRS_PER_ROOT {
        guard.root_truncated.insert(root.to_path_buf());
        return discovered_files;
    }
    let Some(watcher) = guard.root_watchers.get_mut(root) else {
        return discovered_files;
    };

    let mut queue: VecDeque<(PathBuf, Vec<String>)> = VecDeque::new();
    queue.push_back((dir_path.to_path_buf(), rel_comps));
    let mut added_here = 0usize;

    while let Some((dir, comps)) = queue.pop_front() {
        if watched_count >= MAX_WATCHED_DIRS_PER_ROOT || added_here >= MAX_DYNAMIC_DIRS_PER_EVENT {
            if watched_count >= MAX_WATCHED_DIRS_PER_ROOT {
                guard.root_truncated.insert(root.to_path_buf());
            }
            break;
        }
        if watcher.watch(&dir, RecursiveMode::NonRecursive).is_err() {
            guard.root_truncated.insert(root.to_path_buf());
            break;
        }
        watched_count += 1;
        added_here += 1;

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            let child_path = entry.path();
            if ft.is_file() {
                discovered_files.push(child_path);
                continue;
            }
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.is_empty() {
                continue;
            }
            let mut child_rel = comps.clone();
            child_rel.push(name_str.into_owned());
            if !should_skip_dir_on_watch(&child_rel) {
                queue.push_back((child_path, child_rel));
            }
        }
    }

    guard
        .root_watched_dir_counts
        .insert(root.to_path_buf(), watched_count);
    discovered_files
}

fn remove_workspace_from_root_locked(guard: &mut WatcherRegistryState, ws_id: &str, root: &Path) {
    let should_unwatch = if let Some(set) = guard.root_to_workspaces.get_mut(root) {
        set.remove(ws_id);
        set.is_empty()
    } else {
        false
    };
    if should_unwatch {
        guard.root_to_workspaces.remove(root);
        // Dropping the dedicated per-root watcher cleanly releases all OS watches for `root`.
        guard.root_watchers.remove(root);
        guard.root_watched_dir_counts.remove(root);
        guard.root_truncated.remove(root);
    }
}

fn earliest_flush_deadline(
    pending: &HashMap<PathBuf, PendingRootBatch>,
    debounce: Duration,
    max_batch_age: Duration,
) -> Option<Instant> {
    pending
        .values()
        .filter_map(|batch| {
            let last = batch.last_event_at?;
            let first = batch.first_event_at.unwrap_or(last);
            let by_debounce = last.checked_add(debounce).unwrap_or(last);
            let by_max_age = first.checked_add(max_batch_age).unwrap_or(first);
            Some(by_debounce.min(by_max_age))
        })
        .min()
}

async fn run_debouncer_loop(
    state: Arc<Mutex<WatcherRegistryState>>,
    mut raw_rx: mpsc::Receiver<Event>,
    debounce: Duration,
    max_batch_age: Duration,
    on_change: WatchCallback,
) {
    let mut pending: HashMap<PathBuf, PendingRootBatch> = HashMap::new();

    loop {
        let Some(deadline) = earliest_flush_deadline(&pending, debounce, max_batch_age) else {
            let Some(ev) = raw_rx.recv().await else {
                break;
            };
            ingest_raw_event(&state, &mut pending, ev);
            continue;
        };

        let now = Instant::now();
        if now >= deadline {
            flush_pending_roots(&state, &mut pending, debounce, max_batch_age, &on_change);
            continue;
        }

        let sleep_deadline = tokio::time::Instant::from_std(deadline);
        tokio::select! {
            biased;
            _ = tokio::time::sleep_until(sleep_deadline) => {
                flush_pending_roots(&state, &mut pending, debounce, max_batch_age, &on_change);
            }
            maybe_ev = raw_rx.recv() => {
                match maybe_ev {
                    Some(ev) => ingest_raw_event(&state, &mut pending, ev),
                    None => {
                        flush_pending_roots(
                            &state,
                            &mut pending,
                            Duration::ZERO,
                            Duration::ZERO,
                            &on_change,
                        );
                        break;
                    }
                }
            }
        }
    }
}

fn record_classified_path(
    pending: &mut HashMap<PathBuf, PendingRootBatch>,
    root: &Path,
    path: &Path,
    now: Instant,
) {
    let Some(classified) = classify_watched_path(root, path) else {
        return;
    };
    let entry = pending.entry(root.to_path_buf()).or_default();
    if entry.first_event_at.is_none() {
        entry.first_event_at = Some(now);
    }
    if entry.changed_paths.len() < MAX_COALESCED_PATHS_PER_EVENT {
        entry.changed_paths.insert(classified.rel_path.clone());
    }
    if entry.revalidation_paths.len() < MAX_REVALIDATION_PATHS_PER_EVENT {
        entry.revalidation_paths.insert(classified.rel_path);
    }
    if classified.is_git_state {
        entry.git_changed = true;
    }
    if classified.is_skills_or_rules {
        entry.skills_or_rules_changed = true;
    }
    entry.last_event_at = Some(now);
}

fn ingest_raw_event(
    state: &Arc<Mutex<WatcherRegistryState>>,
    pending: &mut HashMap<PathBuf, PendingRootBatch>,
    ev: Event,
) {
    let roots: Vec<PathBuf> = {
        let Ok(guard) = state.lock() else {
            return;
        };
        guard.root_to_workspaces.keys().cloned().collect()
    };
    if roots.is_empty() {
        return;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let is_dir_create = matches!(
        ev.kind,
        EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
    );

    let now = Instant::now();
    for raw_path in ev.paths {
        let clean_path = crate::platform::strip_windows_verbatim_prefix(&raw_path);
        for root in &roots {
            if !clean_path.starts_with(root) {
                continue;
            }
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            if is_dir_create {
                for discovered in attach_dynamic_subdir_watches(state, root, &clean_path) {
                    record_classified_path(pending, root, &discovered, now);
                }
            }
            record_classified_path(pending, root, &clean_path, now);
        }
    }
}

fn flush_pending_roots(
    state: &Arc<Mutex<WatcherRegistryState>>,
    pending: &mut HashMap<PathBuf, PendingRootBatch>,
    debounce: Duration,
    max_batch_age: Duration,
    on_change: &WatchCallback,
) {
    let now = Instant::now();
    let ready_roots: Vec<PathBuf> = pending
        .iter()
        .filter_map(|(root, batch)| {
            let last = batch.last_event_at?;
            let first = batch.first_event_at.unwrap_or(last);
            let settled = now.saturating_duration_since(last) >= debounce;
            let aged_out = now.saturating_duration_since(first) >= max_batch_age;
            (settled || aged_out).then(|| root.clone())
        })
        .collect();
    if ready_roots.is_empty() {
        return;
    }

    for root in ready_roots {
        let Some(batch) = pending.remove(&root) else {
            continue;
        };
        emit_root_batch(state, &root, batch, on_change);
    }
}

fn emit_root_batch(
    state: &Arc<Mutex<WatcherRegistryState>>,
    root: &Path,
    batch: PendingRootBatch,
    on_change: &WatchCallback,
) {
    if batch.changed_paths.is_empty() && !batch.git_changed && !batch.skills_or_rules_changed {
        return;
    }
    let targets: Vec<(String, String)> = {
        let Ok(guard) = state.lock() else {
            return;
        };
        guard
            .root_to_workspaces
            .get(root)
            .map(|ws_ids| {
                ws_ids
                    .iter()
                    .filter_map(|ws_id| {
                        let (_, cwd) = guard.workspace_to_root.get(ws_id)?;
                        Some((ws_id.clone(), cwd.clone()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let changed_paths: Vec<String> = batch.changed_paths.into_iter().collect();
    let revalidation_paths: Vec<String> = batch.revalidation_paths.into_iter().collect();
    for (idx, (workspace_id, cwd)) in targets.into_iter().enumerate() {
        on_change(WorkspaceWatchBatch {
            workspace_id,
            cwd,
            canonical_root: root.to_path_buf(),
            changed_paths: changed_paths.clone(),
            revalidation_paths: if idx == 0 {
                revalidation_paths.clone()
            } else {
                Vec::new()
            },
            is_primary_root_event: idx == 0,
            git_changed: batch.git_changed,
            skills_or_rules_changed: batch.skills_or_rules_changed,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc as std_mpsc;

    fn temp_watch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_watcher_{tag}_{}",
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        ));
        std::fs::create_dir_all(&dir).expect("create temp watch dir");
        crate::platform::canonicalize_clean(&dir).unwrap_or(dir)
    }

    #[tokio::test]
    async fn memory_revalidation_receives_every_path_across_a_full_batch() -> anyhow::Result<()> {
        let project = tempfile::tempdir()?;
        let root = crate::platform::canonicalize_clean(project.path())?;
        let cwd = root
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("workspace path"))?;
        let store = Arc::new(orbit_store::OrbitStore::open_in_memory()?);
        let engine = crate::memory::MemoryEngine::new(store.clone(), Default::default(), None);
        let draft = crate::memory::MemoryDraft {
            id: Some("watch".into()),
            title: "Storage".into(),
            content: "Keep the index local.".into(),
            category: orbit_protocol::MemoryCategory::Convention,
            status: Some(orbit_protocol::MemoryStatus::Confirmed),
            file_paths: vec!["src/index.rs".into()],
            tags: vec![],
        };
        engine.save(cwd, draft, None).await?;
        let mut test_state = WatcherRegistryState::new(mpsc::channel(1).0);
        test_state.workspace_to_root = HashMap::from([("ws".into(), (root.clone(), cwd.into()))]);
        test_state.root_to_workspaces =
            HashMap::from([(root.clone(), HashSet::from(["ws".into()]))]);
        let state = Arc::new(Mutex::new(test_state));
        let (tx, mut rx) = mpsc::unbounded_channel();
        let callback: WatchCallback = Arc::new(move |batch| {
            let _ = tx.send(batch);
        });
        let mut event = Event::new(EventKind::Any);
        event.paths.extend(
            (0..MAX_COALESCED_PATHS_PER_EVENT).map(|i| root.join(format!("src/file{i}.rs"))),
        );
        event.paths.push(root.join("src/index.rs"));
        let mut pending = HashMap::new();
        ingest_raw_event(&state, &mut pending, event);
        // Must stay debounced (not flushed synchronously before flush_pending_roots).
        assert!(rx.try_recv().is_err());
        flush_pending_roots(
            &state,
            &mut pending,
            Duration::ZERO,
            Duration::ZERO,
            &callback,
        );
        let batch = rx.try_recv()?;
        assert!(rx.try_recv().is_err());
        assert_eq!(batch.changed_paths.len(), MAX_COALESCED_PATHS_PER_EVENT);
        assert_eq!(
            batch.revalidation_paths.len(),
            MAX_COALESCED_PATHS_PER_EVENT + 1
        );
        assert!(batch.is_primary_root_event);
        engine.changed(&batch.cwd, batch.revalidation_paths).await?;
        assert_eq!(
            store.project_memory(cwd, "watch")?.map(|i| i.status),
            Some(orbit_protocol::MemoryStatus::NeedsRevalidation)
        );
        let path = root.join(".orbit/memory/watch.md");
        std::fs::write(
            &path,
            std::fs::read_to_string(&path)?.replace("Keep the index local.", "Rebuild locally."),
        )?;
        ingest_raw_event(
            &state,
            &mut pending,
            Event::new(EventKind::Any).add_path(path),
        );
        flush_pending_roots(
            &state,
            &mut pending,
            Duration::ZERO,
            Duration::ZERO,
            &callback,
        );
        let batch = rx.try_recv()?;
        engine.changed(&batch.cwd, batch.revalidation_paths).await?;
        assert_eq!(
            store.project_memory(cwd, "watch")?.map(|i| i.content),
            Some("Rebuild locally.".into())
        );
        Ok(())
    }

    #[test]
    fn classify_watched_path_ignores_build_and_git_object_noise_and_flags_git_and_rules() {
        let root = Path::new("/tmp/workspace_repo");

        // Ignored noisy directories
        assert!(classify_watched_path(root, &root.join(".git/objects/ab/1234")).is_none());
        assert!(classify_watched_path(root, &root.join(".git/logs/HEAD")).is_none());
        assert!(classify_watched_path(root, &root.join(".git/index.lock")).is_none());
        assert!(classify_watched_path(root, &root.join("target/debug/orbit")).is_none());
        assert!(classify_watched_path(root, &root.join("node_modules/pkg/index.js")).is_none());
        assert!(classify_watched_path(root, &root.join("apps/web/dist/bundle.js")).is_none());
        assert!(classify_watched_path(root, &root.join("apps/web/.next/cache/x")).is_none());
        assert!(classify_watched_path(root, &root.join("build/output.o")).is_none());
        assert!(classify_watched_path(root, &root.join(".DS_Store")).is_none());
        assert!(classify_watched_path(root, &root.join("src/.orbit-edit-123.tmp")).is_none());

        // High-signal git state files pass through with `is_git_state = true`
        let head = classify_watched_path(root, &root.join(".git/HEAD")).expect("git HEAD");
        assert_eq!(head.rel_path, ".git/HEAD");
        assert!(head.is_git_state);
        assert!(!head.is_skills_or_rules);

        let idx = classify_watched_path(root, &root.join(".git/index")).expect("git index");
        assert!(idx.is_git_state);

        // Rules & skills paths set `is_skills_or_rules = true`
        let rule = classify_watched_path(root, &root.join(".orbit/rules/rust.md")).expect("rule");
        assert!(rule.is_skills_or_rules);
        assert!(!rule.is_git_state);

        let agents_md = classify_watched_path(root, &root.join("AGENTS.md")).expect("AGENTS.md");
        assert!(agents_md.is_skills_or_rules);

        // Normal source file
        let src = classify_watched_path(root, &root.join("src/main.rs")).expect("src/main.rs");
        assert_eq!(src.rel_path, "src/main.rs");
        assert!(!src.is_git_state);
        assert!(!src.is_skills_or_rules);
    }

    #[tokio::test]
    async fn watcher_delivers_debounced_file_changes_ignores_git_objects_and_unwatches_cleanly() {
        let dir = temp_watch_dir("debounce");
        std::fs::create_dir_all(dir.join(".git/objects/aa")).expect("mkdir git objects");
        std::fs::create_dir_all(dir.join("src")).expect("mkdir src");

        let (tx, rx) = std_mpsc::channel::<WorkspaceWatchBatch>();
        let mgr = WorkspaceWatcherManager::with_debounce(Duration::from_millis(80), move |batch| {
            let _ = tx.send(batch);
        });

        assert!(mgr.watch_workspace("ws_test_1", &dir));
        assert_eq!(mgr.watched_root_count(), 1);
        assert_eq!(mgr.watched_workspace_count(), 1);

        // Give OS watcher a brief moment to settle initial watch registration (FSEvents/inotify)
        tokio::time::sleep(Duration::from_millis(120)).await;
        while rx.try_recv().is_ok() {}

        // 1. Writing under `.git/objects/` MUST be ignored and produce NO event
        std::fs::write(dir.join(".git/objects/aa/deadbeef"), "packdata").expect("write git obj");
        tokio::time::sleep(Duration::from_millis(220)).await;
        assert!(
            rx.try_recv().is_err(),
            ".git/objects writes must be ignored by workspace watcher"
        );

        // 2. Writing a normal file + `.orbit/rules/style.md` bursts should coalesce and deliver!
        std::fs::create_dir_all(dir.join(".orbit/rules")).expect("mkdir rules");
        std::fs::write(dir.join("src/lib.rs"), "pub fn hello() {}\n").expect("write src");
        std::fs::write(dir.join(".orbit/rules/style.md"), "# Style\n").expect("write rule");

        let mut saw_src = false;
        let mut saw_rules = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
        while tokio::time::Instant::now() < deadline && !(saw_src && saw_rules) {
            while let Ok(batch) = rx.try_recv() {
                assert_eq!(batch.workspace_id, "ws_test_1");
                if batch.changed_paths.iter().any(|p| p == "src/lib.rs") {
                    saw_src = true;
                }
                if batch.skills_or_rules_changed {
                    saw_rules = true;
                }
            }
            if !(saw_src && saw_rules) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        assert!(saw_src, "expected debounced change event for src/lib.rs");
        assert!(
            saw_rules,
            "expected skills_or_rules_changed flag for .orbit/rules/style.md"
        );

        // 3. Unwatching removes the workspace and OS watch cleanly
        mgr.unwatch_workspace("ws_test_1");
        assert_eq!(mgr.watched_workspace_count(), 0);
        assert_eq!(mgr.watched_root_count(), 0);

        // 4. Watching a non-existent directory fails gracefully (never panics)
        assert!(!mgr.watch_workspace("ws_missing", &dir.join("does_not_exist")));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn debouncer_never_starves_quiet_workspace_under_ignored_spam_or_continuous_traffic() {
        let root_a = temp_watch_dir("busy-a");
        let root_b = temp_watch_dir("quiet-b");
        let (tx, rx) = std_mpsc::channel::<WorkspaceWatchBatch>();
        let mgr = WorkspaceWatcherManager::with_debounce_and_max_age(
            Duration::from_millis(60),
            Duration::from_millis(180),
            move |batch| {
                let _ = tx.send(batch);
            },
        );

        assert!(mgr.watch_workspace("ws_a", &root_a));
        assert!(mgr.watch_workspace("ws_b", &root_b));
        let raw_tx = mgr.state.lock().expect("state").raw_tx.clone();

        // Emit one real change in quiet `ws_b`, then hammer the debouncer every 10ms with:
        // (1) ignored `target/` & `.git/objects` spam, AND
        // (2) continuous real edits in busy `ws_a`.
        raw_tx
            .send(Event {
                kind: EventKind::Modify(notify::event::ModifyKind::Any),
                paths: vec![root_b.join("src/quiet.rs")],
                attrs: Default::default(),
            })
            .await
            .expect("send ws_b");

        let start = Instant::now();
        let mut saw_b_at: Option<Duration> = None;
        let mut saw_a_at: Option<Duration> = None;
        for tick in 0..35u32 {
            raw_tx
                .send(Event {
                    kind: EventKind::Modify(notify::event::ModifyKind::Any),
                    paths: vec![
                        root_a.join("target/debug/out.o"),
                        root_a.join(".git/objects/ab/1234"),
                        root_a.join(format!("src/busy_{tick}.rs")),
                    ],
                    attrs: Default::default(),
                })
                .await
                .expect("send spam");
            tokio::time::sleep(Duration::from_millis(10)).await;
            while let Ok(batch) = rx.try_recv() {
                if batch.workspace_id == "ws_b" && saw_b_at.is_none() {
                    saw_b_at = Some(start.elapsed());
                }
                if batch.workspace_id == "ws_a" && saw_a_at.is_none() {
                    saw_a_at = Some(start.elapsed());
                }
            }
            if saw_b_at.is_some() && saw_a_at.is_some() {
                break;
            }
        }

        assert!(
            saw_b_at.is_some(),
            "quiet workspace ws_b must flush on debounce even under continuous ignored + busy-ws traffic"
        );
        assert!(
            saw_a_at.is_some(),
            "busy workspace ws_a must flush within max_batch_age even under continuous writes"
        );
        assert!(
            mgr.state.lock().expect("state").root_watchers.len() == 2,
            "each watched root must own an independent OS watcher"
        );
        mgr.unwatch_workspace("ws_a");
        assert_eq!(mgr.state.lock().expect("state").root_watchers.len(), 1);
        mgr.unwatch_workspace("ws_b");
        assert_eq!(mgr.state.lock().expect("state").root_watchers.len(), 0);

        let _ = std::fs::remove_dir_all(&root_a);
        let _ = std::fs::remove_dir_all(&root_b);
    }

    #[test]
    fn should_skip_dir_on_watch_prunes_noisy_dirs_and_preserves_git_state() {
        assert!(!should_skip_dir_on_watch(&[]));
        assert!(!should_skip_dir_on_watch(&[".git".into()]));
        assert!(!should_skip_dir_on_watch(&[".git".into(), "refs".into()]));
        assert!(!should_skip_dir_on_watch(&[
            ".git".into(),
            "refs".into(),
            "heads".into()
        ]));
        assert!(!should_skip_dir_on_watch(&[
            ".git".into(),
            "worktrees".into()
        ]));
        assert!(should_skip_dir_on_watch(&[".git".into(), "objects".into()]));
        assert!(should_skip_dir_on_watch(&[".git".into(), "logs".into()]));
        assert!(should_skip_dir_on_watch(&["node_modules".into()]));
        assert!(should_skip_dir_on_watch(&["apps".into(), ".venv".into()]));
        assert!(should_skip_dir_on_watch(&["subrepo".into(), ".git".into()]));
        assert!(!should_skip_dir_on_watch(&["src".into(), "daemon".into()]));
    }

    #[tokio::test]
    async fn per_root_dir_cap_cooldown_and_async_unwatch_race_work_cleanly() {
        let dir = temp_watch_dir("cap_and_async");
        std::fs::create_dir_all(dir.join("src")).expect("mkdir src");
        std::fs::create_dir_all(dir.join(".git/objects/00")).expect("mkdir git objects");
        std::fs::create_dir_all(dir.join(".git/refs/heads")).expect("mkdir git refs");
        std::fs::create_dir_all(dir.join("node_modules/pkg/sub")).expect("mkdir node_modules");
        for i in 0..10 {
            std::fs::create_dir_all(dir.join(format!("dir_{i}/nested"))).expect("mkdir sub");
        }

        let mgr = WorkspaceWatcherManager::with_debounce(Duration::from_millis(50), |_| {});

        // 1. Direct `watch_workspace_inner` with a tight dir cap stops promptly & marks truncated
        //    on platforms using per-directory BFS watches (Linux), and succeeds on macOS/Windows.
        let gen1 = {
            let mut guard = mgr.state.lock().expect("state");
            guard.alloc_watch_gen("ws_cap_1")
        };
        assert!(watch_workspace_inner(
            &mgr.state,
            "ws_cap_1",
            &dir,
            Some(gen1),
            4
        ));
        assert_eq!(mgr.watched_root_count(), 1);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert!(mgr.watched_dir_count(&dir) <= 4);
            assert!(mgr.is_root_truncated(&dir));
        }

        // Second workspace on same root joins in O(1) without duplicating root watchers
        assert!(mgr.watch_workspace("ws_cap_2", &dir));
        assert_eq!(mgr.watched_root_count(), 1);
        assert_eq!(mgr.watched_workspace_count(), 2);

        mgr.unwatch_workspace("ws_cap_1");
        mgr.unwatch_workspace("ws_cap_2");
        assert_eq!(mgr.watched_root_count(), 0);

        // 2. Cooldown negative cache prevents immediate re-scan of recently failed roots
        let canon = crate::platform::canonicalize_clean(&dir).unwrap_or_else(|_| dir.clone());
        {
            let mut guard = mgr.state.lock().expect("state");
            guard
                .failed_roots
                .insert(canon.clone(), Instant::now() + WATCH_FAILURE_COOLDOWN);
        }
        assert!(!mgr.watch_workspace("ws_cooldown", &dir));
        assert_eq!(mgr.watched_root_count(), 0);
        {
            let mut guard = mgr.state.lock().expect("state");
            guard.failed_roots.remove(&canon);
        }

        // 3. `watch_workspace_async` returns immediately and `unwatch_workspace` cancels cleanly
        mgr.watch_workspace_async("ws_async_cancel", &dir);
        mgr.unwatch_workspace("ws_async_cancel");
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(mgr.watched_workspace_count(), 0);
        assert_eq!(mgr.watched_root_count(), 0);

        // 4. `watch_workspace_async` succeeds in background
        mgr.watch_workspace_async("ws_async_ok", &dir);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline && mgr.watched_workspace_count() == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(mgr.watched_workspace_count(), 1);
        assert_eq!(mgr.watched_root_count(), 1);
        mgr.unwatch_workspace("ws_async_ok");
        assert_eq!(mgr.watched_root_count(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
