//! Automatic OS idle-sleep prevention (`KeepAwakeManager` & `KeepAwakeGuard`).
//!
//! Prevents the host OS from entering idle sleep while any Orbit agent turn, autonomous wakeup,
//! background workflow, or active scheduled run is executing, and cleanly releases the assertion
//! after a short grace debounce once all work returns to idle (or immediately when disabled or
//! on daemon shutdown).
//!
//! Pure safe Rust (`unsafe_code = "deny"`):
//! - **macOS**: spawns `caffeinate -i -w <daemon_pid>` so macOS automatically releases the power
//!   assertion if the daemon ever crashes or is killed with `SIGKILL`.
//! - **Linux**: spawns `systemd-inhibit --what=idle --who=Orbit --why="Active AI coding agents running" --mode=block sleep infinity`
//!   when `systemd-inhibit` is available, with graceful no-op fallback on headless/non-systemd hosts.
//! - **Windows / other**: clean no-op stub behind the `SleepInhibitorBackend` trait, ready for a
//!   future safe process/power backend without changing callers.

use std::collections::HashSet;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Duration;

use orbit_protocol::AgentStatus;
pub use orbit_protocol::KeepAwakeStatus;
use tracing::{debug, info, warn};

/// Default grace period before releasing the OS sleep inhibitor after the last busy task settles,
/// preventing rapid process thrashing across consecutive agent turns.
pub const DEFAULT_KEEP_AWAKE_GRACE_PERIOD: Duration = Duration::from_secs(15);

/// Returns `true` if the given agent lifecycle state requires keeping the host OS awake.
pub fn status_requires_keep_awake(status: &AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Initializing
            | AgentStatus::Running { .. }
            | AgentStatus::AutonomousWakeup { .. }
    )
}

/// Low-level handle to a held OS sleep-prevention assertion.
/// Implementations must release/reap any underlying OS resource in `Drop`.
pub trait InhibitorHandle: Send + 'static {
    /// Returns `true` while the underlying inhibitor process/assertion is still live.
    fn is_alive(&mut self) -> bool;
}

/// Child-process-backed sleep inhibitor handle (`caffeinate`, `systemd-inhibit`, etc.).
/// Guarantees the child process is killed and reaped on drop with zero zombie leaks.
pub struct ProcessInhibitorHandle {
    child: Child,
}

impl ProcessInhibitorHandle {
    pub fn new(child: Child) -> Self {
        Self { child }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    fn terminate_and_reap(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
}

impl InhibitorHandle for ProcessInhibitorHandle {
    fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for ProcessInhibitorHandle {
    fn drop(&mut self) {
        self.terminate_and_reap();
    }
}

/// RAII guard owning a single active OS sleep-prevention handle.
/// Dropping the guard immediately releases the OS sleep assertion and reaps any helper process.
pub struct KeepAwakeGuard {
    handle: Box<dyn InhibitorHandle>,
}

impl KeepAwakeGuard {
    pub fn new(handle: Box<dyn InhibitorHandle>) -> Self {
        Self { handle }
    }

    pub fn is_alive(&mut self) -> bool {
        self.handle.is_alive()
    }
}

/// Strategy trait for platform-specific OS sleep-prevention backends.
pub trait SleepInhibitorBackend: Send + Sync + 'static {
    /// Human-readable backend name reported in `KeepAwakeStatus.backend`.
    fn name(&self) -> &'static str;

    /// Acquire a sleep-prevention guard. Returning `Ok(None)` signals a graceful no-op backend.
    fn acquire(&self) -> std::io::Result<Option<KeepAwakeGuard>>;
}

/// Default cross-platform OS sleep-inhibitor backend using safe native OS helpers.
pub struct OsSleepInhibitorBackend {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    binary: Option<PathBuf>,
}

impl OsSleepInhibitorBackend {
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            let binary = find_in_path("caffeinate").or_else(|| {
                let fallback = PathBuf::from("/usr/bin/caffeinate");
                fallback.is_file().then_some(fallback)
            });
            Self { binary }
        }
        #[cfg(target_os = "linux")]
        {
            let binary = find_in_path("systemd-inhibit").or_else(|| {
                ["/usr/bin/systemd-inhibit", "/bin/systemd-inhibit"]
                    .into_iter()
                    .map(PathBuf::from)
                    .find(|p| p.is_file())
            });
            Self { binary }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Self {}
        }
    }
}

impl Default for OsSleepInhibitorBackend {
    fn default() -> Self {
        Self::detect()
    }
}

impl SleepInhibitorBackend for OsSleepInhibitorBackend {
    fn name(&self) -> &'static str {
        #[cfg(target_os = "macos")]
        {
            if self.binary.is_some() {
                "caffeinate"
            } else {
                "noop"
            }
        }
        #[cfg(target_os = "linux")]
        {
            if self.binary.is_some() {
                "systemd-inhibit"
            } else {
                "noop"
            }
        }
        #[cfg(target_os = "windows")]
        {
            "windows-stub"
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            "noop"
        }
    }

    fn acquire(&self) -> std::io::Result<Option<KeepAwakeGuard>> {
        #[cfg(target_os = "macos")]
        {
            let Some(ref bin) = self.binary else {
                return Ok(None);
            };
            let pid = std::process::id().to_string();
            let child = Command::new(bin)
                .args(["-i", "-w", &pid])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(Some(KeepAwakeGuard::new(Box::new(
                ProcessInhibitorHandle::new(child),
            ))))
        }
        #[cfg(target_os = "linux")]
        {
            let Some(ref bin) = self.binary else {
                return Ok(None);
            };
            let child = Command::new(bin)
                .args([
                    "--what=idle",
                    "--who=Orbit",
                    "--why=Active AI coding agents running",
                    "--mode=block",
                    "sleep",
                    "infinity",
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(Some(KeepAwakeGuard::new(Box::new(
                ProcessInhibitorHandle::new(child),
            ))))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            // Windows and unsupported platforms degrade gracefully to a no-op stub while
            // preserving the exact process/backend abstraction for future native support.
            Ok(None)
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn find_in_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

type StatusCallback = Arc<dyn Fn(KeepAwakeStatus) + Send + Sync>;

struct KeepAwakeInner {
    busy_keys: HashSet<String>,
    guard: Option<KeepAwakeGuard>,
    release_gen: u64,
    pending_release: Option<tokio::task::AbortHandle>,
    effective_backend: String,
    logged_backend_error: bool,
    last_emitted: Option<KeepAwakeStatus>,
}

struct KeepAwakeState {
    enabled: AtomicBool,
    grace_period: Duration,
    backend: Arc<dyn SleepInhibitorBackend>,
    inner: Mutex<KeepAwakeInner>,
    on_status_change: Mutex<Option<StatusCallback>>,
}

impl KeepAwakeState {
    fn lock_inner(&self) -> MutexGuard<'_, KeepAwakeInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn compute_status_locked(&self, inner: &mut KeepAwakeInner) -> KeepAwakeStatus {
        // Reconcile if the child process exited prematurely on its own.
        if let Some(ref mut guard) = inner.guard
            && !guard.is_alive()
        {
            inner.guard = None;
        }
        KeepAwakeStatus {
            enabled: self.enabled.load(Ordering::Acquire),
            active: inner.guard.is_some(),
            active_agents: inner.busy_keys.len(),
            backend: inner.effective_backend.clone(),
        }
    }

    fn emit_status(&self, status: KeepAwakeStatus) {
        let cb = self
            .on_status_change
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(cb) = cb {
            cb(status);
        }
    }

    fn ensure_acquired_locked(&self, inner: &mut KeepAwakeInner) {
        if !self.enabled.load(Ordering::Acquire) || inner.busy_keys.is_empty() {
            return;
        }
        if let Some(ref mut guard) = inner.guard {
            if guard.is_alive() {
                return;
            }
            inner.guard = None;
        }

        match self.backend.acquire() {
            Ok(Some(guard)) => {
                inner.guard = Some(guard);
                inner.effective_backend = self.backend.name().to_string();
                inner.logged_backend_error = false;
                info!(
                    backend = %inner.effective_backend,
                    active_agents = inner.busy_keys.len(),
                    "Acquired OS keep-awake sleep inhibitor"
                );
            }
            Ok(None) => {
                inner.effective_backend = self.backend.name().to_string();
                debug!(
                    backend = %inner.effective_backend,
                    "Keep-awake backend is a no-op on this host"
                );
            }
            Err(err) => {
                inner.effective_backend = "noop".to_string();
                if !inner.logged_backend_error {
                    inner.logged_backend_error = true;
                    warn!(
                        backend = %self.backend.name(),
                        error = %err,
                        "Failed to start OS keep-awake helper; falling back to no-op"
                    );
                }
            }
        }
    }

    fn mark_busy(self: &Arc<Self>, key: String) -> KeepAwakeStatus {
        let (status, maybe_emit) = {
            let mut inner = self.lock_inner();
            inner.busy_keys.insert(key);
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            self.ensure_acquired_locked(&mut inner);
            let status = self.compute_status_locked(&mut inner);
            let maybe_emit = record_if_changed(&mut inner.last_emitted, &status);
            (status, maybe_emit)
        };
        if let Some(changed) = maybe_emit {
            self.emit_status(changed);
        }
        status
    }

    fn mark_idle(self: &Arc<Self>, key: &str) -> KeepAwakeStatus {
        let (dropped_guard, status, maybe_emit) = {
            let mut inner = self.lock_inner();
            inner.busy_keys.remove(key);

            let mut dropped_guard = None;
            if inner.busy_keys.is_empty() && inner.guard.is_some() {
                let enabled = self.enabled.load(Ordering::Acquire);
                if !enabled || self.grace_period.is_zero() {
                    inner.release_gen = inner.release_gen.wrapping_add(1);
                    if let Some(handle) = inner.pending_release.take() {
                        handle.abort();
                    }
                    dropped_guard = inner.guard.take();
                } else if inner.pending_release.is_none() {
                    inner.release_gen = inner.release_gen.wrapping_add(1);
                    let expected_gen = inner.release_gen;
                    if let Ok(rt) = tokio::runtime::Handle::try_current() {
                        let weak: Weak<KeepAwakeState> = Arc::downgrade(self);
                        let grace = self.grace_period;
                        let task = rt.spawn(async move {
                            tokio::time::sleep(grace).await;
                            if let Some(state) = weak.upgrade() {
                                state.release_after_grace(expected_gen);
                            }
                        });
                        inner.pending_release = Some(task.abort_handle());
                    } else {
                        // Outside an async runtime, release synchronously so we never leak.
                        dropped_guard = inner.guard.take();
                    }
                }
            }

            let status = self.compute_status_locked(&mut inner);
            let maybe_emit = record_if_changed(&mut inner.last_emitted, &status);
            (dropped_guard, status, maybe_emit)
        };

        if dropped_guard.is_some() {
            drop(dropped_guard);
            info!("Released OS keep-awake sleep inhibitor");
        }
        if let Some(changed) = maybe_emit {
            self.emit_status(changed);
        }
        status
    }

    fn release_after_grace(&self, expected_gen: u64) {
        let (dropped_guard, maybe_emit) = {
            let mut inner = self.lock_inner();
            if inner.release_gen != expected_gen || !inner.busy_keys.is_empty() {
                return;
            }
            inner.pending_release = None;
            let dropped_guard = inner.guard.take();
            let status = self.compute_status_locked(&mut inner);
            let maybe_emit = record_if_changed(&mut inner.last_emitted, &status);
            (dropped_guard, maybe_emit)
        };

        if dropped_guard.is_some() {
            drop(dropped_guard);
            info!("Released OS keep-awake sleep inhibitor after idle grace window");
        }
        if let Some(changed) = maybe_emit {
            self.emit_status(changed);
        }
    }

    fn cleanup_sync(&self) {
        let dropped_guard = {
            let mut inner = self.lock_inner();
            inner.busy_keys.clear();
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            inner.guard.take()
        };
        drop(dropped_guard);
    }
}

impl Drop for KeepAwakeState {
    fn drop(&mut self) {
        self.cleanup_sync();
    }
}

fn record_if_changed(
    last_emitted: &mut Option<KeepAwakeStatus>,
    current: &KeepAwakeStatus,
) -> Option<KeepAwakeStatus> {
    if last_emitted.as_ref() == Some(current) {
        return None;
    }
    *last_emitted = Some(current.clone());
    Some(current.clone())
}

/// Scoped RAII lease keeping the host awake for a background workflow, scheduled run, or task.
/// Dropping the lease automatically marks the key idle (subject to normal grace debounce).
pub struct KeepAwakeLease {
    state: Weak<KeepAwakeState>,
    key: String,
}

impl Drop for KeepAwakeLease {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let _ = state.mark_idle(&self.key);
        }
    }
}

/// Coordinates host OS sleep prevention across all running agents and background workflows.
pub struct KeepAwakeManager {
    state: Arc<KeepAwakeState>,
}

impl KeepAwakeManager {
    /// Create a manager backed by the current host OS sleep inhibitor and a 15s idle grace window.
    pub fn new(enabled: bool) -> Self {
        Self::with_backend(
            enabled,
            DEFAULT_KEEP_AWAKE_GRACE_PERIOD,
            Arc::new(OsSleepInhibitorBackend::detect()),
        )
    }

    /// Create a manager with an explicit grace period and injectable backend (ideal for tests).
    pub fn with_backend(
        enabled: bool,
        grace_period: Duration,
        backend: Arc<dyn SleepInhibitorBackend>,
    ) -> Self {
        let initial_backend = backend.name().to_string();
        Self {
            state: Arc::new(KeepAwakeState {
                enabled: AtomicBool::new(enabled),
                grace_period,
                backend,
                inner: Mutex::new(KeepAwakeInner {
                    busy_keys: HashSet::new(),
                    guard: None,
                    release_gen: 0,
                    pending_release: None,
                    effective_backend: initial_backend,
                    logged_backend_error: false,
                    last_emitted: None,
                }),
                on_status_change: Mutex::new(None),
            }),
        }
    }

    /// Register a callback invoked whenever `KeepAwakeStatus` changes.
    pub fn set_on_status_change(&self, callback: impl Fn(KeepAwakeStatus) + Send + Sync + 'static) {
        let mut slot = self
            .state
            .on_status_change
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *slot = Some(Arc::new(callback));
    }

    pub fn is_enabled(&self) -> bool {
        self.state.enabled.load(Ordering::Acquire)
    }

    /// Enable or disable keep-awake. Disabling immediately releases any held OS inhibitor
    /// without waiting for the idle grace window; enabling immediately acquires if work is busy.
    pub fn set_enabled(&self, enabled: bool) -> KeepAwakeStatus {
        self.state.enabled.store(enabled, Ordering::Release);
        let (dropped_guard, status, maybe_emit) = {
            let mut inner = self.state.lock_inner();
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            let dropped_guard = if enabled {
                self.state.ensure_acquired_locked(&mut inner);
                None
            } else {
                inner.guard.take()
            };
            let status = self.state.compute_status_locked(&mut inner);
            let maybe_emit = record_if_changed(&mut inner.last_emitted, &status);
            (dropped_guard, status, maybe_emit)
        };

        if dropped_guard.is_some() {
            drop(dropped_guard);
            info!("Released OS keep-awake sleep inhibitor (disabled by config)");
        }
        if let Some(changed) = maybe_emit {
            self.state.emit_status(changed);
        }
        status
    }

    /// Synchronize an agent's lifecycle state into the keep-awake busy set (idempotent).
    pub fn notify_agent_status(&self, agent_id: &str, status: &AgentStatus) -> KeepAwakeStatus {
        let key = format!("agent:{agent_id}");
        if status_requires_keep_awake(status) {
            self.state.mark_busy(key)
        } else {
            self.state.mark_idle(&key)
        }
    }

    /// Explicitly mark an agent idle/removed (e.g. archived, killed, deleted, or driver closed).
    pub fn mark_agent_idle(&self, agent_id: &str) -> KeepAwakeStatus {
        self.state.mark_idle(&format!("agent:{agent_id}"))
    }

    /// Mark an arbitrary busy workload key active (e.g. background workflow or scheduled task).
    pub fn mark_busy(&self, key: impl Into<String>) -> KeepAwakeStatus {
        self.state.mark_busy(key.into())
    }

    /// Mark an arbitrary busy workload key idle.
    pub fn mark_idle(&self, key: &str) -> KeepAwakeStatus {
        self.state.mark_idle(key)
    }

    /// Acquire an RAII lease for a scoped background run or workflow.
    pub fn acquire_lease(&self, key: impl Into<String>) -> KeepAwakeLease {
        let key = key.into();
        let _ = self.state.mark_busy(key.clone());
        KeepAwakeLease {
            state: Arc::downgrade(&self.state),
            key,
        }
    }

    /// Return a live snapshot of keep-awake state.
    pub fn status(&self) -> KeepAwakeStatus {
        let mut inner = self.state.lock_inner();
        self.state.compute_status_locked(&mut inner)
    }

    /// Immediately release any held OS sleep inhibitor and cancel pending grace timers.
    pub fn shutdown(&self) {
        let (dropped_guard, maybe_emit) = {
            let mut inner = self.state.lock_inner();
            inner.busy_keys.clear();
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            let dropped_guard = inner.guard.take();
            let status = self.state.compute_status_locked(&mut inner);
            let maybe_emit = record_if_changed(&mut inner.last_emitted, &status);
            (dropped_guard, maybe_emit)
        };
        if dropped_guard.is_some() {
            drop(dropped_guard);
            info!("Released OS keep-awake sleep inhibitor on shutdown");
        }
        if let Some(changed) = maybe_emit {
            self.state.emit_status(changed);
        }
    }
}

impl Drop for KeepAwakeManager {
    fn drop(&mut self) {
        self.state.cleanup_sync();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct MockHandle {
        alive: Arc<AtomicBool>,
        active_handles: Arc<AtomicUsize>,
        release_count: Arc<AtomicUsize>,
    }

    impl InhibitorHandle for MockHandle {
        fn is_alive(&mut self) -> bool {
            self.alive.load(Ordering::Acquire)
        }
    }

    impl Drop for MockHandle {
        fn drop(&mut self) {
            self.alive.store(false, Ordering::Release);
            self.active_handles.fetch_sub(1, Ordering::AcqRel);
            self.release_count.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct MockBackend {
        acquire_count: AtomicUsize,
        release_count: Arc<AtomicUsize>,
        active_handles: Arc<AtomicUsize>,
        fail_next: AtomicBool,
    }

    impl MockBackend {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                acquire_count: AtomicUsize::new(0),
                release_count: Arc::new(AtomicUsize::new(0)),
                active_handles: Arc::new(AtomicUsize::new(0)),
                fail_next: AtomicBool::new(false),
            })
        }

        fn acquires(&self) -> usize {
            self.acquire_count.load(Ordering::Acquire)
        }

        fn releases(&self) -> usize {
            self.release_count.load(Ordering::Acquire)
        }

        fn active(&self) -> usize {
            self.active_handles.load(Ordering::Acquire)
        }
    }

    impl SleepInhibitorBackend for MockBackend {
        fn name(&self) -> &'static str {
            "mock-inhibitor"
        }

        fn acquire(&self) -> std::io::Result<Option<KeepAwakeGuard>> {
            if self.fail_next.load(Ordering::Acquire) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "mock helper missing",
                ));
            }
            self.acquire_count.fetch_add(1, Ordering::AcqRel);
            self.active_handles.fetch_add(1, Ordering::AcqRel);
            Ok(Some(KeepAwakeGuard::new(Box::new(MockHandle {
                alive: Arc::new(AtomicBool::new(true)),
                active_handles: Arc::clone(&self.active_handles),
                release_count: Arc::clone(&self.release_count),
            }))))
        }
    }

    #[tokio::test]
    async fn acquire_and_release_transitions_across_agent_statuses() {
        let backend = MockBackend::new();
        let mgr = KeepAwakeManager::with_backend(true, Duration::ZERO, backend.clone());

        assert_eq!(
            mgr.status(),
            KeepAwakeStatus {
                enabled: true,
                active: false,
                active_agents: 0,
                backend: "mock-inhibitor".into(),
            }
        );

        // Idle never acquires
        mgr.notify_agent_status("ag_1", &AgentStatus::Idle);
        assert!(!mgr.status().active);
        assert_eq!(backend.acquires(), 0);

        // Foreground running acquires once
        mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        assert!(mgr.status().active);
        assert_eq!(mgr.status().active_agents, 1);
        assert_eq!(backend.acquires(), 1);
        assert_eq!(backend.active(), 1);

        // Duplicate running notification is idempotent (no duplicate spawn)
        mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t2".into(),
            },
        );
        assert_eq!(mgr.status().active_agents, 1);
        assert_eq!(backend.acquires(), 1);

        // Second concurrent agent (autonomous wakeup) shares the single inhibitor
        mgr.notify_agent_status(
            "ag_2",
            &AgentStatus::AutonomousWakeup {
                turn_id: "t3".into(),
                source: "subagent".into(),
            },
        );
        assert_eq!(mgr.status().active_agents, 2);
        assert_eq!(backend.acquires(), 1);
        assert_eq!(backend.active(), 1);

        // First agent returns to Idle -> still active because ag_2 is running
        mgr.notify_agent_status("ag_1", &AgentStatus::Idle);
        assert!(mgr.status().active);
        assert_eq!(mgr.status().active_agents, 1);
        assert_eq!(backend.releases(), 0);

        // Second agent errors -> count drops to 0 -> inhibitor released immediately (0 grace)
        mgr.notify_agent_status(
            "ag_2",
            &AgentStatus::Error {
                message: "boom".into(),
                recoverable: true,
            },
        );
        assert!(!mgr.status().active);
        assert_eq!(mgr.status().active_agents, 0);
        assert_eq!(backend.releases(), 1);
        assert_eq!(backend.active(), 0);
    }

    #[tokio::test]
    async fn grace_debounce_prevents_thrashing_and_releases_after_window() {
        let backend = MockBackend::new();
        let mgr = KeepAwakeManager::with_backend(true, Duration::from_millis(80), backend.clone());

        mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        assert!(mgr.status().active);
        assert_eq!(backend.acquires(), 1);

        // Transition to Idle starts grace debounce; inhibitor stays active inside window
        mgr.notify_agent_status("ag_1", &AgentStatus::Idle);
        assert_eq!(mgr.status().active_agents, 0);
        assert!(mgr.status().active);
        assert_eq!(backend.releases(), 0);

        // Another turn arrives within 30ms -> cancels pending release without respawning
        tokio::time::sleep(Duration::from_millis(30)).await;
        mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t2".into(),
            },
        );
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert!(mgr.status().active);
        assert_eq!(backend.acquires(), 1);
        assert_eq!(backend.releases(), 0);

        // Now settle to Closed and let grace window expire
        mgr.notify_agent_status("ag_1", &AgentStatus::Closed);
        assert!(mgr.status().active);
        tokio::time::sleep(Duration::from_millis(120)).await;
        assert!(!mgr.status().active);
        assert_eq!(backend.releases(), 1);
        assert_eq!(backend.active(), 0);
    }

    #[tokio::test]
    async fn disable_toggle_immediately_releases_and_reenable_reacquires_if_busy() {
        let backend = MockBackend::new();
        let mgr = KeepAwakeManager::with_backend(true, Duration::from_secs(15), backend.clone());

        mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        assert!(mgr.status().active);
        assert_eq!(backend.active(), 1);

        // Disabling releases immediately without waiting for the 15s grace period
        let disabled_status = mgr.set_enabled(false);
        assert!(!disabled_status.enabled);
        assert!(!disabled_status.active);
        assert_eq!(disabled_status.active_agents, 1);
        assert_eq!(backend.releases(), 1);
        assert_eq!(backend.active(), 0);

        // New busy turns while disabled do not acquire
        mgr.notify_agent_status("ag_2", &AgentStatus::Initializing);
        assert!(!mgr.status().active);
        assert_eq!(mgr.status().active_agents, 2);
        assert_eq!(backend.acquires(), 1);

        // Re-enabling while agents are still busy immediately re-acquires
        let reenabled_status = mgr.set_enabled(true);
        assert!(reenabled_status.enabled);
        assert!(reenabled_status.active);
        assert_eq!(backend.acquires(), 2);
        assert_eq!(backend.active(), 1);

        // Disabling during an active grace debounce also releases immediately
        mgr.notify_agent_status("ag_1", &AgentStatus::Idle);
        mgr.notify_agent_status("ag_2", &AgentStatus::Idle);
        assert!(mgr.status().active);
        let status = mgr.set_enabled(false);
        assert!(!status.active);
        assert_eq!(backend.releases(), 2);
        assert_eq!(backend.active(), 0);
    }

    #[tokio::test]
    async fn drop_cleans_up_active_inhibitor_immediately() {
        let backend = MockBackend::new();
        {
            let mgr =
                KeepAwakeManager::with_backend(true, Duration::from_secs(15), backend.clone());
            mgr.notify_agent_status(
                "ag_1",
                &AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            );
            assert_eq!(backend.active(), 1);
            // Put into grace debounce to ensure Drop cleans up even with a pending timer
            mgr.notify_agent_status("ag_1", &AgentStatus::Idle);
            assert_eq!(backend.active(), 1);
        }
        // Manager dropped -> handle must be released immediately without waiting 15s
        assert_eq!(backend.releases(), 1);
        assert_eq!(backend.active(), 0);
    }

    #[tokio::test]
    async fn lease_and_status_callback_track_background_workflows() {
        let backend = MockBackend::new();
        let mgr = KeepAwakeManager::with_backend(true, Duration::ZERO, backend.clone());
        let events: Arc<Mutex<Vec<KeepAwakeStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);
        mgr.set_on_status_change(move |st| {
            events_cb.lock().unwrap_or_else(|e| e.into_inner()).push(st);
        });

        {
            let _lease = mgr.acquire_lease("schedule:run_1");
            assert!(mgr.status().active);
            assert_eq!(mgr.status().active_agents, 1);
        }

        assert!(!mgr.status().active);
        assert_eq!(mgr.status().active_agents, 0);
        let recorded = events.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(recorded.len(), 2);
        assert!(recorded[0].active);
        assert!(!recorded[1].active);
    }

    #[tokio::test]
    async fn spawn_failure_degrades_gracefully_to_noop() {
        let backend = MockBackend::new();
        backend.fail_next.store(true, Ordering::Release);
        let mgr = KeepAwakeManager::with_backend(true, Duration::ZERO, backend);

        let st = mgr.notify_agent_status(
            "ag_1",
            &AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        assert!(st.enabled);
        assert!(!st.active);
        assert_eq!(st.active_agents, 1);
        assert_eq!(st.backend, "noop");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_inhibitor_handle_kills_and_reaps_child_on_drop() {
        struct SleepChildBackend {
            last_pid: Mutex<Option<u32>>,
        }

        impl SleepInhibitorBackend for SleepChildBackend {
            fn name(&self) -> &'static str {
                "test-sleep"
            }

            fn acquire(&self) -> std::io::Result<Option<KeepAwakeGuard>> {
                let child = Command::new("sleep")
                    .arg("30")
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                let handle = ProcessInhibitorHandle::new(child);
                *self.last_pid.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle.pid());
                Ok(Some(KeepAwakeGuard::new(Box::new(handle))))
            }
        }

        let backend = Arc::new(SleepChildBackend {
            last_pid: Mutex::new(None),
        });
        let pid = {
            let mgr =
                KeepAwakeManager::with_backend(true, Duration::from_secs(15), backend.clone());
            mgr.notify_agent_status(
                "ag_1",
                &AgentStatus::Running {
                    turn_id: "t1".into(),
                },
            );
            assert!(mgr.status().active);
            let pid = backend
                .last_pid
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .expect("sleep helper pid recorded");
            // Verify helper child is alive while manager holds the guard
            let alive = Command::new("ps")
                .args(["-p", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            assert!(alive, "sleep helper pid {pid} should be running");
            pid
        };

        // After Drop, child process must be killed and reaped (no longer present in process table)
        let still_present = Command::new("ps")
            .args(["-p", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            !still_present,
            "sleep helper pid {pid} should be killed and reaped on Drop"
        );
    }
}
