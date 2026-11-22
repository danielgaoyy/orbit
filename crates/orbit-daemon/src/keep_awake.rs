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
//! - **Linux**: spawns `systemd-inhibit --what=idle --who=Orbit --why="Active AI coding agents running" --mode=block cat`
//!   with a piped stdin handle so closing stdin (or daemon `SIGKILL`) cleanly sends EOF to `cat`
//!   and lets `systemd-inhibit` release the lock and exit with zero orphaned child processes,
//!   with graceful no-op fallback on headless/non-systemd hosts.
//! - **Windows**: spawns a hidden `powershell.exe` / `pwsh.exe` helper with piped stdin that calls
//!   `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` and blocks on
//!   `[Console]::In.ReadLine()`, so closing stdin (or daemon termination) automatically clears the
//!   execution state and exits cleanly with zero Rust `unsafe` code.
//! - **Other**: clean no-op stub behind the `SleepInhibitorBackend` trait.

use std::collections::HashMap;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use orbit_protocol::AgentStatus;
pub use orbit_protocol::KeepAwakeStatus;
use tracing::{debug, info, warn};

/// Default grace period before releasing the OS sleep inhibitor after the last busy task settles,
/// preventing rapid process thrashing across consecutive agent turns.
pub const DEFAULT_KEEP_AWAKE_GRACE_PERIOD: Duration = Duration::from_secs(15);

/// If an inhibitor helper exits with a non-zero status faster than this threshold (for example,
/// `systemd-inhibit` failing immediately on headless Linux without logind/D-Bus), latch the
/// backend unavailable (`noop`) instead of respawning on every busy transition.
const RAPID_EXIT_FAILURE_WINDOW: Duration = Duration::from_secs(1);

/// Clean EOF shutdown grace window for stdin-piped helpers (`systemd-inhibit ... cat`) before
/// escalating to `SIGKILL`.
const STDIN_EOF_REAP_TIMEOUT: Duration = Duration::from_millis(150);

/// Returns `true` if the given agent lifecycle state requires keeping the host OS awake.
pub fn status_requires_keep_awake(status: &AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Initializing
            | AgentStatus::Running { .. }
            | AgentStatus::AutonomousWakeup { .. }
    )
}

/// Liveness outcome of an underlying OS sleep inhibitor handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InhibitorLiveness {
    Alive,
    ExitedCleanly,
    ExitedWithError,
}

/// Low-level handle to a held OS sleep-prevention assertion.
/// Implementations must release/reap any underlying OS resource in `Drop`.
pub trait InhibitorHandle: Send + 'static {
    /// Returns `true` while the underlying inhibitor process/assertion is still live.
    fn is_alive(&mut self) -> bool;

    /// Detailed liveness check distinguishing clean exits from non-zero helper failures.
    fn poll_liveness(&mut self) -> InhibitorLiveness {
        if self.is_alive() {
            InhibitorLiveness::Alive
        } else {
            InhibitorLiveness::ExitedCleanly
        }
    }

    /// Returns `true` when dropping this handle performs blocking OS work (such as waiting on
    /// a helper process) and should be offloaded to a blocking thread pool inside Tokio.
    fn is_blocking_drop(&self) -> bool {
        false
    }
}

/// Child-process-backed sleep inhibitor handle (`caffeinate`, `systemd-inhibit`, etc.).
/// Guarantees the child process is cleanly terminated and reaped on drop with zero zombie leaks.
pub struct ProcessInhibitorHandle {
    child: Child,
    stdin: Option<ChildStdin>,
    exited_state: Option<InhibitorLiveness>,
}

impl ProcessInhibitorHandle {
    pub fn new(mut child: Child) -> Self {
        let stdin = child.stdin.take();
        Self {
            child,
            stdin,
            exited_state: None,
        }
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    fn poll_child(&mut self) -> InhibitorLiveness {
        if let Some(state) = self.exited_state {
            return state;
        }
        match self.child.try_wait() {
            Ok(None) => InhibitorLiveness::Alive,
            Ok(Some(status)) => {
                let state = if status.success() {
                    InhibitorLiveness::ExitedCleanly
                } else {
                    InhibitorLiveness::ExitedWithError
                };
                self.exited_state = Some(state);
                state
            }
            Err(_) => {
                self.exited_state = Some(InhibitorLiveness::ExitedWithError);
                InhibitorLiveness::ExitedWithError
            }
        }
    }

    fn terminate_and_reap(&mut self) {
        let had_stdin = self.stdin.take().is_some();
        if self.poll_child() != InhibitorLiveness::Alive {
            return;
        }
        if had_stdin {
            // Closing stdin delivers EOF to `cat`, letting `systemd-inhibit` release its D-Bus
            // inhibitor cleanly and exit 0 without leaving orphaned child processes.
            let deadline = Instant::now() + STDIN_EOF_REAP_TIMEOUT;
            while Instant::now() < deadline {
                if self.poll_child() != InhibitorLiveness::Alive {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.exited_state = Some(InhibitorLiveness::ExitedCleanly);
    }
}

impl InhibitorHandle for ProcessInhibitorHandle {
    fn is_alive(&mut self) -> bool {
        self.poll_child() == InhibitorLiveness::Alive
    }

    fn poll_liveness(&mut self) -> InhibitorLiveness {
        self.poll_child()
    }

    fn is_blocking_drop(&self) -> bool {
        true
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

    pub fn poll_liveness(&mut self) -> InhibitorLiveness {
        self.handle.poll_liveness()
    }

    fn is_blocking_drop(&self) -> bool {
        self.handle.is_blocking_drop()
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
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
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
        #[cfg(target_os = "windows")]
        {
            let binary = find_in_path("powershell.exe")
                .or_else(|| find_in_path("pwsh.exe"))
                .or_else(|| {
                    std::env::var_os("SystemRoot").and_then(|root| {
                        let fallback = PathBuf::from(root)
                            .join("System32")
                            .join("WindowsPowerShell")
                            .join("v1.0")
                            .join("powershell.exe");
                        fallback.is_file().then_some(fallback)
                    })
                });
            Self { binary }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
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
            if self.binary.is_some() {
                "windows-powershell"
            } else {
                "noop"
            }
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
                    "cat",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            Ok(Some(KeepAwakeGuard::new(Box::new(
                ProcessInhibitorHandle::new(child),
            ))))
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let Some(ref bin) = self.binary else {
                return Ok(None);
            };
            // Pure safe Rust: delegates `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
            // to a hidden PowerShell helper that blocks until stdin closes (EOF on guard drop or
            // daemon crash), then clears the assertion cleanly.
            const PS_KEEP_AWAKE_SCRIPT: &str = concat!(
                "$sig = '[DllImport(\"kernel32.dll\")] public static extern uint SetThreadExecutionState(uint esFlags);';",
                "$t = Add-Type -MemberDefinition $sig -Name 'OrbitKeepAwake' -Namespace 'Orbit' -PassThru;",
                "$null = $t::SetThreadExecutionState(2147483649);",
                "try { [Console]::In.ReadLine() | Out-Null } finally { $null = $t::SetThreadExecutionState(2147483648) }"
            );
            let child = Command::new(bin)
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    PS_KEEP_AWAKE_SCRIPT,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(crate::platform::WIN_CREATE_NO_WINDOW)
                .spawn()?;
            Ok(Some(KeepAwakeGuard::new(Box::new(
                ProcessInhibitorHandle::new(child),
            ))))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Ok(None)
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
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

struct ActiveGuard {
    guard: KeepAwakeGuard,
    acquired_at: Instant,
}

struct KeepAwakeInner {
    busy_keys: HashMap<String, usize>,
    guard: Option<ActiveGuard>,
    acquiring: bool,
    backend_latched_unavailable: bool,
    release_gen: u64,
    pending_release: Option<tokio::task::AbortHandle>,
    effective_backend: String,
    logged_backend_error: bool,
    status_seq: u64,
    last_emitted: Option<KeepAwakeStatus>,
}

struct KeepAwakeState {
    enabled: AtomicBool,
    grace_period: Duration,
    backend: Arc<dyn SleepInhibitorBackend>,
    inner: Mutex<KeepAwakeInner>,
    /// Serializes status callbacks and tracks the last broadcast sequence so stale snapshots can
    /// never be emitted out-of-order after newer transitions.
    emit_gate: Mutex<u64>,
    on_status_change: Mutex<Option<StatusCallback>>,
}

struct AcquiringResetGuard<'a>(&'a KeepAwakeState);

impl Drop for AcquiringResetGuard<'_> {
    fn drop(&mut self) {
        let mut inner = self.0.lock_inner();
        inner.acquiring = false;
    }
}

/// Drop a guard, offloading blocking child-process reaping onto Tokio's blocking pool whenever
/// invoked from inside a Tokio runtime.
fn release_guard(guard: KeepAwakeGuard) {
    if guard.is_blocking_drop()
        && let Ok(rt) = tokio::runtime::Handle::try_current()
    {
        rt.spawn_blocking(move || {
            drop(guard);
        });
        return;
    }
    drop(guard);
}

impl KeepAwakeState {
    fn lock_inner(&self) -> MutexGuard<'_, KeepAwakeInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn reconcile_guard_locked(&self, inner: &mut KeepAwakeInner) -> Option<KeepAwakeGuard> {
        let active = inner.guard.as_mut()?;
        match active.guard.poll_liveness() {
            InhibitorLiveness::Alive => None,
            liveness => {
                let elapsed = active.acquired_at.elapsed();
                let dead = inner.guard.take().map(|a| a.guard);
                if liveness == InhibitorLiveness::ExitedWithError
                    && elapsed < RAPID_EXIT_FAILURE_WINDOW
                {
                    inner.backend_latched_unavailable = true;
                    inner.effective_backend = "noop".to_string();
                    if !inner.logged_backend_error {
                        inner.logged_backend_error = true;
                        warn!(
                            backend = %self.backend.name(),
                            elapsed_ms = elapsed.as_millis() as u64,
                            "OS keep-awake helper exited rapidly with non-zero status; latching backend to noop"
                        );
                    }
                }
                dead
            }
        }
    }

    fn snapshot_status_locked(&self, inner: &KeepAwakeInner) -> KeepAwakeStatus {
        KeepAwakeStatus {
            enabled: self.enabled.load(Ordering::Acquire),
            active: inner.guard.is_some(),
            active_agents: inner.busy_keys.values().copied().sum(),
            backend: inner.effective_backend.clone(),
        }
    }

    fn emit_status(&self, seq: u64, status: KeepAwakeStatus) {
        let mut last_seq = self.emit_gate.lock().unwrap_or_else(|e| e.into_inner());
        if seq <= *last_seq {
            return;
        }
        let cb = self
            .on_status_change
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        *last_seq = seq;
        if let Some(cb) = cb {
            cb(status);
        }
    }

    /// Acquire a sleep-inhibitor guard without holding `self.inner` across blocking `Command::spawn()`.
    fn finish_acquire_outside_lock(&self) -> KeepAwakeStatus {
        let reset = AcquiringResetGuard(self);
        let acquire_res = self.backend.acquire();

        let (extra_guard, status, maybe_emit) = {
            let mut inner = self.lock_inner();
            inner.acquiring = false;
            std::mem::forget(reset);

            let still_needed = self.enabled.load(Ordering::Acquire)
                && !inner.busy_keys.is_empty()
                && inner.guard.is_none()
                && !inner.backend_latched_unavailable;
            let mut extra_guard = None;

            match acquire_res {
                Ok(Some(guard)) if still_needed => {
                    inner.guard = Some(ActiveGuard {
                        guard,
                        acquired_at: Instant::now(),
                    });
                    inner.effective_backend = self.backend.name().to_string();
                    inner.logged_backend_error = false;
                    info!(
                        backend = %inner.effective_backend,
                        active_agents = inner.busy_keys.values().copied().sum::<usize>(),
                        "Acquired OS keep-awake sleep inhibitor"
                    );
                }
                Ok(Some(guard)) => {
                    extra_guard = Some(guard);
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

            let status = self.snapshot_status_locked(&inner);
            let maybe_emit = record_if_changed(&mut inner, &status);
            (extra_guard, status, maybe_emit)
        };

        if let Some(guard) = extra_guard {
            release_guard(guard);
        }
        if let Some((seq, changed)) = maybe_emit {
            self.emit_status(seq, changed);
        }
        status
    }

    fn mark_busy_inner(self: &Arc<Self>, key: String, refcounted: bool) -> KeepAwakeStatus {
        let (dead_guard, should_acquire, status, maybe_emit) = {
            let mut inner = self.lock_inner();
            if refcounted {
                let count = inner.busy_keys.entry(key).or_insert(0);
                *count = count.saturating_add(1);
            } else {
                inner.busy_keys.entry(key).or_insert(1);
            }
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            let dead_guard = self.reconcile_guard_locked(&mut inner);
            let should_acquire = self.enabled.load(Ordering::Acquire)
                && !inner.busy_keys.is_empty()
                && inner.guard.is_none()
                && !inner.backend_latched_unavailable
                && !inner.acquiring;
            if should_acquire {
                inner.acquiring = true;
                (dead_guard, true, self.snapshot_status_locked(&inner), None)
            } else {
                let status = self.snapshot_status_locked(&inner);
                let maybe_emit = record_if_changed(&mut inner, &status);
                (dead_guard, false, status, maybe_emit)
            }
        };

        if let Some(guard) = dead_guard {
            release_guard(guard);
        }
        if should_acquire {
            return self.finish_acquire_outside_lock();
        }
        if let Some((seq, changed)) = maybe_emit {
            self.emit_status(seq, changed);
        }
        status
    }

    fn mark_idle_inner(self: &Arc<Self>, key: &str, remove_all: bool) -> KeepAwakeStatus {
        let (dead_guard, dropped_guard, status, maybe_emit) = {
            let mut inner = self.lock_inner();
            if remove_all {
                inner.busy_keys.remove(key);
            } else if let Some(count) = inner.busy_keys.get_mut(key) {
                if *count <= 1 {
                    inner.busy_keys.remove(key);
                } else {
                    *count -= 1;
                }
            }

            let dead_guard = self.reconcile_guard_locked(&mut inner);
            let mut dropped_guard = None;
            if inner.busy_keys.is_empty() && inner.guard.is_some() {
                let enabled = self.enabled.load(Ordering::Acquire);
                if !enabled || self.grace_period.is_zero() {
                    inner.release_gen = inner.release_gen.wrapping_add(1);
                    if let Some(handle) = inner.pending_release.take() {
                        handle.abort();
                    }
                    dropped_guard = inner.guard.take().map(|a| a.guard);
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
                        dropped_guard = inner.guard.take().map(|a| a.guard);
                    }
                }
            }

            let status = self.snapshot_status_locked(&inner);
            let maybe_emit = record_if_changed(&mut inner, &status);
            (dead_guard, dropped_guard, status, maybe_emit)
        };

        if let Some(guard) = dead_guard {
            release_guard(guard);
        }
        if let Some(guard) = dropped_guard {
            release_guard(guard);
            info!("Released OS keep-awake sleep inhibitor");
        }
        if let Some((seq, changed)) = maybe_emit {
            self.emit_status(seq, changed);
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
            let dropped_guard = inner.guard.take().map(|a| a.guard);
            let status = self.snapshot_status_locked(&inner);
            let maybe_emit = record_if_changed(&mut inner, &status);
            (dropped_guard, maybe_emit)
        };

        if let Some(guard) = dropped_guard {
            release_guard(guard);
            info!("Released OS keep-awake sleep inhibitor after idle grace window");
        }
        if let Some((seq, changed)) = maybe_emit {
            self.emit_status(seq, changed);
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
            inner.guard.take().map(|a| a.guard)
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
    inner: &mut KeepAwakeInner,
    current: &KeepAwakeStatus,
) -> Option<(u64, KeepAwakeStatus)> {
    if inner.last_emitted.as_ref() == Some(current) {
        return None;
    }
    inner.status_seq = inner.status_seq.saturating_add(1);
    inner.last_emitted = Some(current.clone());
    Some((inner.status_seq, current.clone()))
}

/// Scoped RAII lease keeping the host awake for a background workflow, scheduled run, or task.
/// Dropping the lease decrements its reference count (subject to normal grace debounce).
pub struct KeepAwakeLease {
    state: Weak<KeepAwakeState>,
    key: String,
}

impl Drop for KeepAwakeLease {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            let _ = state.mark_idle_inner(&self.key, false);
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
                    busy_keys: HashMap::new(),
                    guard: None,
                    acquiring: false,
                    backend_latched_unavailable: false,
                    release_gen: 0,
                    pending_release: None,
                    effective_backend: initial_backend,
                    logged_backend_error: false,
                    status_seq: 0,
                    last_emitted: None,
                }),
                emit_gate: Mutex::new(0),
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
        let (dead_guard, dropped_guard, should_acquire, status, maybe_emit) = {
            let mut inner = self.state.lock_inner();
            inner.release_gen = inner.release_gen.wrapping_add(1);
            if let Some(handle) = inner.pending_release.take() {
                handle.abort();
            }
            let dead_guard = self.state.reconcile_guard_locked(&mut inner);
            if enabled {
                let should_acquire = !inner.busy_keys.is_empty()
                    && inner.guard.is_none()
                    && !inner.backend_latched_unavailable
                    && !inner.acquiring;
                if should_acquire {
                    inner.acquiring = true;
                    (
                        dead_guard,
                        None,
                        true,
                        self.state.snapshot_status_locked(&inner),
                        None,
                    )
                } else {
                    let status = self.state.snapshot_status_locked(&inner);
                    let maybe_emit = record_if_changed(&mut inner, &status);
                    (dead_guard, None, false, status, maybe_emit)
                }
            } else {
                let dropped_guard = inner.guard.take().map(|a| a.guard);
                let status = self.state.snapshot_status_locked(&inner);
                let maybe_emit = record_if_changed(&mut inner, &status);
                (dead_guard, dropped_guard, false, status, maybe_emit)
            }
        };

        if let Some(guard) = dead_guard {
            release_guard(guard);
        }
        if let Some(guard) = dropped_guard {
            release_guard(guard);
            info!("Released OS keep-awake sleep inhibitor (disabled by config)");
        }
        if should_acquire {
            return self.state.finish_acquire_outside_lock();
        }
        if let Some((seq, changed)) = maybe_emit {
            self.state.emit_status(seq, changed);
        }
        status
    }

    /// Synchronize an agent's lifecycle state into the keep-awake busy set (idempotent per agent).
    pub fn notify_agent_status(&self, agent_id: &str, status: &AgentStatus) -> KeepAwakeStatus {
        if status_requires_keep_awake(status) {
            self.mark_agent_busy(agent_id)
        } else {
            self.mark_agent_idle(agent_id)
        }
    }

    /// Explicitly mark an agent busy (idempotent per agent key).
    pub fn mark_agent_busy(&self, agent_id: &str) -> KeepAwakeStatus {
        self.state
            .mark_busy_inner(format!("agent:{agent_id}"), false)
    }

    /// Explicitly mark an agent idle/removed (e.g. archived, killed, deleted, or driver closed).
    pub fn mark_agent_idle(&self, agent_id: &str) -> KeepAwakeStatus {
        self.state
            .mark_idle_inner(&format!("agent:{agent_id}"), true)
    }

    /// Mark an arbitrary busy workload key active (refcounted for safe concurrent callers).
    pub fn mark_busy(&self, key: impl Into<String>) -> KeepAwakeStatus {
        self.state.mark_busy_inner(key.into(), true)
    }

    /// Decrement an arbitrary busy workload key's refcount (releasing when it reaches zero).
    pub fn mark_idle(&self, key: &str) -> KeepAwakeStatus {
        self.state.mark_idle_inner(key, false)
    }

    /// Acquire an RAII refcounted lease for a scoped background run or workflow.
    pub fn acquire_lease(&self, key: impl Into<String>) -> KeepAwakeLease {
        let key = key.into();
        let _ = self.state.mark_busy_inner(key.clone(), true);
        KeepAwakeLease {
            state: Arc::downgrade(&self.state),
            key,
        }
    }

    /// Return a live snapshot of keep-awake state, reconciling any unexpectedly exited helper
    /// process and emitting a status update if a dead helper was detected.
    pub fn status(&self) -> KeepAwakeStatus {
        let (dead_guard, status, maybe_emit) = {
            let mut inner = self.state.lock_inner();
            let dead_guard = self.state.reconcile_guard_locked(&mut inner);
            let status = self.state.snapshot_status_locked(&inner);
            let maybe_emit = if dead_guard.is_some() {
                record_if_changed(&mut inner, &status)
            } else {
                None
            };
            (dead_guard, status, maybe_emit)
        };
        if let Some(guard) = dead_guard {
            release_guard(guard);
        }
        if let Some((seq, changed)) = maybe_emit {
            self.state.emit_status(seq, changed);
        }
        status
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
            let dropped_guard = inner.guard.take().map(|a| a.guard);
            let status = self.state.snapshot_status_locked(&inner);
            let maybe_emit = record_if_changed(&mut inner, &status);
            (dropped_guard, maybe_emit)
        };
        if let Some(guard) = dropped_guard {
            // Reap synchronously on shutdown so the helper process exits before daemon teardown.
            drop(guard);
            info!("Released OS keep-awake sleep inhibitor on shutdown");
        }
        if let Some((seq, changed)) = maybe_emit {
            self.state.emit_status(seq, changed);
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

    #[tokio::test]
    async fn rapid_nonzero_helper_exit_latches_noop_and_emits_status_on_refresh() {
        struct RapidFailHandle;
        impl InhibitorHandle for RapidFailHandle {
            fn is_alive(&mut self) -> bool {
                false
            }
            fn poll_liveness(&mut self) -> InhibitorLiveness {
                InhibitorLiveness::ExitedWithError
            }
        }

        struct RapidFailBackend {
            acquires: AtomicUsize,
        }
        impl SleepInhibitorBackend for RapidFailBackend {
            fn name(&self) -> &'static str {
                "systemd-inhibit"
            }
            fn acquire(&self) -> std::io::Result<Option<KeepAwakeGuard>> {
                self.acquires.fetch_add(1, Ordering::AcqRel);
                Ok(Some(KeepAwakeGuard::new(Box::new(RapidFailHandle))))
            }
        }

        let backend = Arc::new(RapidFailBackend {
            acquires: AtomicUsize::new(0),
        });
        let mgr = KeepAwakeManager::with_backend(true, Duration::ZERO, backend.clone());
        let events: Arc<Mutex<Vec<KeepAwakeStatus>>> = Arc::new(Mutex::new(Vec::new()));
        let events_cb = Arc::clone(&events);
        mgr.set_on_status_change(move |st| {
            events_cb.lock().unwrap_or_else(|e| e.into_inner()).push(st);
        });

        let st1 = mgr.mark_busy("job:1");
        assert!(st1.active);
        assert_eq!(backend.acquires.load(Ordering::Acquire), 1);

        // Refreshing status detects the rapid non-zero exit, latches noop, and emits status.
        let st2 = mgr.status();
        assert!(!st2.active);
        assert_eq!(st2.backend, "noop");
        assert_eq!(st2.active_agents, 1);

        // Subsequent mark_busy must NOT respawn the latched-broken helper.
        let st3 = mgr.mark_busy("job:2");
        assert!(!st3.active);
        assert_eq!(st3.backend, "noop");
        assert_eq!(st3.active_agents, 2);
        assert_eq!(backend.acquires.load(Ordering::Acquire), 1);

        let recorded = events.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            recorded
                .iter()
                .any(|e| !e.active && e.backend == "noop" && e.active_agents == 1),
            "expected dead-child status update event, got: {:?}",
            *recorded
        );
    }

    #[tokio::test]
    async fn lease_refcounting_and_agent_idempotency() {
        let backend = MockBackend::new();
        let mgr = KeepAwakeManager::with_backend(true, Duration::ZERO, backend.clone());

        // Overlapping leases on the same key refcount cleanly.
        let lease_a = mgr.acquire_lease("shared:workflow");
        let lease_b = mgr.acquire_lease("shared:workflow");
        assert!(mgr.status().active);
        assert_eq!(mgr.status().active_agents, 2);
        assert_eq!(backend.acquires(), 1);

        drop(lease_a);
        assert!(
            mgr.status().active,
            "dropping one of two overlapping leases must keep inhibitor active"
        );
        assert_eq!(mgr.status().active_agents, 1);
        assert_eq!(backend.releases(), 0);

        drop(lease_b);
        assert!(!mgr.status().active);
        assert_eq!(mgr.status().active_agents, 0);
        assert_eq!(backend.releases(), 1);

        // Agent status notifications remain idempotent per agent key.
        mgr.notify_agent_status(
            "ag_idem",
            &AgentStatus::Running {
                turn_id: "t1".into(),
            },
        );
        mgr.notify_agent_status(
            "ag_idem",
            &AgentStatus::Running {
                turn_id: "t2".into(),
            },
        );
        assert_eq!(mgr.status().active_agents, 1);
        mgr.notify_agent_status("ag_idem", &AgentStatus::Idle);
        assert!(!mgr.status().active);
        assert_eq!(mgr.status().active_agents, 0);
        assert_eq!(backend.releases(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn piped_stdin_inhibitor_handle_exits_cleanly_on_eof() {
        let child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn cat");
        let handle = ProcessInhibitorHandle::new(child);
        let pid = handle.pid();
        assert!(handle.stdin.is_some());
        drop(handle);

        let still_present = Command::new("ps")
            .args(["-p", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            !still_present,
            "piped cat process pid {pid} should exit cleanly on stdin EOF and be reaped"
        );
    }
}
