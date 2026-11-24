//! Pure-Rust Chrome DevTools Protocol (CDP) Browser Automation & Guarded Computer Use.
//!
//! Security & architectural invariants:
//! - Zero external Node.js / Python / Playwright / Puppeteer dependencies.
//! - Strict loopback-only CDP WebSocket (`127.0.0.1` / `localhost` / `[::1]` only, no credentials).
//! - Strict URL navigation guard + live CDP `Fetch` request interception: `http://` & `https://`
//!   only for documents, DNS-pinned SSRF blocking (`169.254.0.0/16`, `100.100.100.200`,
//!   `fe80::/10`, `fd00:ec2::254`, IPv4-mapped, IPv4-compatible including `::127.0.0.1`,
//!   NAT64 `64:ff9b::/96`), configurable `allowed_domains` / `blocked_domains`, and explicit
//!   `allow_localhost` toggle (default `true` for local dev servers).
//! - Fail-closed sensitive input guard: refuses typing into `<input type="password">`, OTP/PIN,
//!   credit-card fields, Shadow DOM inputs, or cross-origin iframes (both pre-focus and post-click)
//!   unless explicitly permitted by host policy.
//! - CAPTCHA boundary: never attempts to solve or bypass CAPTCHAs; surfaces `captcha_detected`
//!   clearly and blocks clicks via both snapshot metadata and live coordinate hit-testing.
//! - Resource bounds: atomic `MAX_BROWSER_SESSIONS = 4` slot reservation, <= 8 MiB screenshot cap,
//!   10-minute idle reaper, auto-eviction on browser crash/WS disconnect, symlink-safe isolated
//!   profiles under `$ORBIT_HOME/browser-profiles` (`0o700`), and safe process-group kill on drop/shutdown
//!   (zero `unsafe`).
//! - Host policy persisted atomically (`0o600` on Unix) in `$ORBIT_HOME/browser-policy.json`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use orbit_protocol::{
    BrowserActionAudit, BrowserActionRequest, BrowserElementRef, BrowserPolicyConfig,
    BrowserSessionSummary, MAX_BROWSER_SESSIONS, PickedElementInfo,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as TungMsg, protocol::WebSocketConfig},
};
use tracing::{debug, info, warn};
use uuid::Uuid;

const BROWSER_POLICY_FILE: &str = "browser-policy.json";
const BROWSER_PROFILES_DIR: &str = "browser-profiles";
const BROWSER_UPLOAD_RUNTIME_DIR: &str = "runtime";
const BROWSER_UPLOAD_STAGING_DIR: &str = "browser-uploads";
const MAX_UPLOAD_FILE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_UPLOAD_TOTAL_BYTES: u64 = 200 * 1024 * 1024;
const MAX_URL_LEN: usize = 4096;
const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
const MAX_SCREENSHOT_BASE64_LEN: usize = (MAX_SCREENSHOT_BYTES / 3 + 2) * 4;
const MAX_AUDIT_ENTRIES_PER_SESSION: usize = 50;
const MAX_CONSOLE_ERRORS_PER_SESSION: usize = 30;
const MAX_ELEMENTS_PER_SNAPSHOT: usize = 80;
const MAX_TEXT_EXCERPT_CHARS: usize = 4000;
const DEFAULT_VIEWPORT_WIDTH: u32 = 1280;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 800;
const CDP_STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const CDP_COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const DNS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
const IDLE_SESSION_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const IDLE_REAPER_INTERVAL: Duration = Duration::from_secs(30);

type PendingCdpReplies = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
type DisconnectCallback = Arc<dyn Fn() + Send + Sync>;

/// Result of executing a browser automation action on a live CDP session.
#[derive(Debug, Clone)]
pub struct BrowserActionOutcome {
    pub session: BrowserSessionSummary,
    pub elements: Vec<BrowserElementRef>,
    pub screenshot_base64: Option<String>,
    pub text_excerpt: Option<String>,
    pub captcha_detected: bool,
    pub picked_element: Option<PickedElementInfo>,
}

/// Snapshot of active browser sessions, policy, and detected Chrome binary on this host.
#[derive(Debug, Clone)]
pub struct BrowserHostSnapshot {
    pub sessions: Vec<BrowserSessionSummary>,
    pub policy: BrowserPolicyConfig,
    pub chrome_binary: Option<String>,
}

/// Parameters for creating a new managed CDP browser session.
#[derive(Debug, Clone, Default)]
pub struct CreateBrowserSessionSpec {
    pub agent_id: Option<String>,
    pub initial_url: Option<String>,
    pub headless: Option<bool>,
    pub viewport_width: Option<u32>,
    pub viewport_height: Option<u32>,
    pub workspace_root: Option<PathBuf>,
}

/// Slot state inside `BrowserManager::sessions` for atomic cap reservation before spawn.
enum SessionSlot {
    Starting,
    Active(Arc<BrowserSession>),
}

/// RAII rollback guard ensuring a `SessionSlot::Starting` reservation is always released
/// if session creation errors or its future is cancelled mid-flight.
struct SlotReservationGuard {
    sessions: Arc<RwLock<HashMap<String, SessionSlot>>>,
    session_id: String,
    committed: bool,
}

impl SlotReservationGuard {
    fn new(sessions: Arc<RwLock<HashMap<String, SessionSlot>>>, session_id: String) -> Self {
        Self {
            sessions,
            session_id,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }

    async fn release(mut self) {
        self.committed = true;
        let mut guard = self.sessions.write().await;
        if matches!(guard.get(&self.session_id), Some(SessionSlot::Starting)) {
            guard.remove(&self.session_id);
        }
    }
}

impl Drop for SlotReservationGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let sessions = Arc::clone(&self.sessions);
        let session_id = self.session_id.clone();
        if let Ok(mut guard) = sessions.try_write() {
            if matches!(guard.get(&session_id), Some(SessionSlot::Starting)) {
                guard.remove(&session_id);
            }
            return;
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut guard = sessions.write().await;
                if matches!(guard.get(&session_id), Some(SessionSlot::Starting)) {
                    guard.remove(&session_id);
                }
            });
        }
    }
}

/// Host-local manager for pure-Rust CDP browser automation sessions.
pub struct BrowserManager {
    home_dir: PathBuf,
    policy: Arc<RwLock<BrowserPolicyConfig>>,
    sessions: Arc<RwLock<HashMap<String, SessionSlot>>>,
    chrome_binary: RwLock<Option<String>>,
}

impl BrowserManager {
    /// Initialize the browser manager, loading `$ORBIT_HOME/browser-policy.json` if present
    /// and discovering a local Chromium-based browser binary.
    pub fn new(home_dir: PathBuf) -> Arc<Self> {
        let policy = load_browser_policy(&home_dir);
        let chrome_binary = discover_chrome_binary();
        Arc::new(Self {
            home_dir,
            policy: Arc::new(RwLock::new(policy)),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            chrome_binary: RwLock::new(chrome_binary),
        })
    }

    /// Spawn the periodic 10-minute idle session auto-shutdown reaper.
    pub fn spawn_idle_reaper(self: &Arc<Self>) -> JoinHandle<()> {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(IDLE_REAPER_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                let reaped = this.reap_idle_sessions(IDLE_SESSION_TIMEOUT).await;
                if reaped > 0 {
                    info!(reaped, "Reaped idle CDP browser sessions");
                }
            }
        })
    }

    pub async fn get_policy(&self) -> BrowserPolicyConfig {
        self.policy.read().await.clone()
    }

    pub async fn update_policy(
        &self,
        new_policy: BrowserPolicyConfig,
    ) -> Result<BrowserHostSnapshot, String> {
        let sanitized = new_policy.sanitized();
        let home = self.home_dir.clone();
        let to_save = sanitized.clone();
        tokio::task::spawn_blocking(move || save_browser_policy_atomic(&home, &to_save))
            .await
            .map_err(|e| format!("browser policy save task failed: {e}"))??;
        *self.policy.write().await = sanitized;
        Ok(self.list_snapshot().await)
    }

    pub async fn list_snapshot(&self) -> BrowserHostSnapshot {
        self.list_snapshot_with_options(true).await
    }

    pub async fn list_snapshot_with_options(
        &self,
        include_screenshots: bool,
    ) -> BrowserHostSnapshot {
        let policy = self.policy.read().await.clone();
        let chrome_binary = {
            let cur = self.chrome_binary.read().await.clone();
            if cur.is_some() {
                cur
            } else {
                let rediscovered = tokio::task::spawn_blocking(discover_chrome_binary)
                    .await
                    .ok()
                    .flatten();
                *self.chrome_binary.write().await = rediscovered.clone();
                rediscovered
            }
        };
        let sessions_arc: Vec<Arc<BrowserSession>> = self
            .sessions
            .read()
            .await
            .values()
            .filter_map(|slot| match slot {
                SessionSlot::Active(sess) => Some(Arc::clone(sess)),
                SessionSlot::Starting => None,
            })
            .collect();
        let mut sessions = Vec::with_capacity(sessions_arc.len());
        for sess in sessions_arc {
            sessions.push(sess.summary(include_screenshots).await);
        }
        sessions.sort_by(|a, b| {
            b.updated_at_ms
                .cmp(&a.updated_at_ms)
                .then_with(|| a.session_id.cmp(&b.session_id))
        });
        BrowserHostSnapshot {
            sessions,
            policy,
            chrome_binary,
        }
    }

    /// Return the owning `agent_id` for a session, or `Err` if the session does not exist.
    pub async fn get_session_owner(&self, session_id: &str) -> Result<Option<String>, String> {
        let guard = self.sessions.read().await;
        let Some(SessionSlot::Active(sess)) = guard.get(session_id) else {
            return Err(format!("browser session `{session_id}` not found"));
        };
        Ok(sess.agent_id.clone())
    }

    fn make_disconnect_evictor(&self, session_id: String) -> DisconnectCallback {
        let sessions = Arc::clone(&self.sessions);
        Arc::new(move || {
            let sessions = Arc::clone(&sessions);
            let sid = session_id.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let removed = {
                        let mut guard = sessions.write().await;
                        match guard.get(&sid) {
                            Some(SessionSlot::Active(_)) => guard.remove(&sid),
                            _ => None,
                        }
                    };
                    if let Some(SessionSlot::Active(sess)) = removed {
                        debug!(session_id = %sid, "Auto-evicting disconnected CDP browser session");
                        sess.cleanup_after_disconnect().await;
                    }
                });
            }
        })
    }

    /// Atomically reserve a `SessionSlot::Starting` under the write lock before spawning.
    async fn reserve_session_slot(
        &self,
        policy: &BrowserPolicyConfig,
    ) -> Result<(String, SlotReservationGuard), String> {
        let effective_cap = (policy.max_sessions.clamp(1, MAX_BROWSER_SESSIONS)) as usize;
        let session_id = format!("br_{}", &Uuid::new_v4().simple().to_string()[..12]);
        {
            let mut guard = self.sessions.write().await;
            if guard.len() >= effective_cap {
                return Err(format!(
                    "Maximum active browser sessions reached ({effective_cap}); close an existing session first"
                ));
            }
            guard.insert(session_id.clone(), SessionSlot::Starting);
        }
        Ok((
            session_id.clone(),
            SlotReservationGuard::new(Arc::clone(&self.sessions), session_id),
        ))
    }

    /// Spawn a new isolated Chrome/Chromium process and attach a pure-Rust CDP WebSocket session.
    pub async fn create_session(
        &self,
        spec: CreateBrowserSessionSpec,
    ) -> Result<BrowserSessionSummary, String> {
        let policy = self.policy.read().await.clone();
        if !policy.enabled {
            return Err("Browser automation is disabled by host policy".to_string());
        }

        // Pre-validate initial URL BEFORE reserving a slot or spawning a browser process.
        let validated_initial_url = if let Some(ref raw_url) = spec.initial_url {
            let trimmed = raw_url.trim();
            if trimmed.is_empty() {
                None
            } else {
                let parsed = validate_navigation_url_async(trimmed, &policy).await?;
                Some(parsed.to_string())
            }
        } else {
            None
        };

        let chrome_path = {
            let cur = self.chrome_binary.read().await.clone();
            if let Some(p) = cur {
                p
            } else {
                let discovered = tokio::task::spawn_blocking(discover_chrome_binary)
                    .await
                    .map_err(|e| format!("chrome discovery task failed: {e}"))?
                    .ok_or_else(|| {
                        "No supported Chrome, Chromium, Edge, or Brave binary found on this host (or set ORBIT_CHROME_BIN)".to_string()
                    })?;
                *self.chrome_binary.write().await = Some(discovered.clone());
                discovered
            }
        };

        // Atomically reserve slot BEFORE spawning Chrome (eliminates TOCTOU cap overshoot).
        let (session_id, mut reservation) = self.reserve_session_slot(&policy).await?;

        let headless = spec.headless.unwrap_or(policy.headless);
        let viewport_width = spec
            .viewport_width
            .unwrap_or(DEFAULT_VIEWPORT_WIDTH)
            .clamp(320, 3840);
        let viewport_height = spec
            .viewport_height
            .unwrap_or(DEFAULT_VIEWPORT_HEIGHT)
            .clamp(240, 2160);

        let mut spawned = match spawn_isolated_browser(
            &self.home_dir,
            &chrome_path,
            &session_id,
            headless,
            viewport_width,
            viewport_height,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                reservation.release().await;
                return Err(e);
            }
        };

        let page_ws_url = match discover_page_target_ws_url(&spawned.devtools_browser_ws_url).await
        {
            Ok(url) => url,
            Err(e) => {
                spawned.process.terminate_and_cleanup().await;
                reservation.release().await;
                return Err(e);
            }
        };

        let on_disconnect = self.make_disconnect_evictor(session_id.clone());
        let session = match BrowserSession::connect(
            session_id.clone(),
            spec.agent_id,
            spec.workspace_root,
            self.home_dir.clone(),
            &page_ws_url,
            headless,
            viewport_width,
            viewport_height,
            Some(spawned.process),
            Arc::clone(&self.policy),
            on_disconnect,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                reservation.release().await;
                return Err(e);
            }
        };

        // Promote reserved `Starting` slot to `Active`.
        {
            let mut guard = self.sessions.write().await;
            guard.insert(
                session_id.clone(),
                SessionSlot::Active(Arc::clone(&session)),
            );
            reservation.commit();
        }

        if let Some(url) = validated_initial_url
            && let Err(e) = session
                .execute_action(BrowserActionRequest::Navigate { url }, &policy, &[])
                .await
        {
            warn!(session_id = %session_id, error = %e, "Initial browser navigation failed; tearing down session");
            self.sessions.write().await.remove(&session_id);
            session.shutdown().await;
            return Err(e);
        }

        Ok(session.summary(true).await)
    }

    /// Attach a managed session directly to a loopback CDP page WebSocket URL (used in tests and
    /// loopback diagnostics; strictly rejects non-loopback or credentialed WebSocket URLs).
    pub async fn create_session_from_cdp_ws_url(
        &self,
        cdp_ws_url: &str,
        spec: CreateBrowserSessionSpec,
    ) -> Result<BrowserSessionSummary, String> {
        let policy = self.policy.read().await.clone();
        if !policy.enabled {
            return Err("Browser automation is disabled by host policy".to_string());
        }
        validate_cdp_ws_url(cdp_ws_url)?;

        let validated_initial_url = if let Some(ref raw_url) = spec.initial_url {
            let trimmed = raw_url.trim();
            if trimmed.is_empty() {
                None
            } else {
                let parsed = validate_navigation_url_async(trimmed, &policy).await?;
                Some(parsed.to_string())
            }
        } else {
            None
        };

        // Atomically reserve slot BEFORE connecting (eliminates TOCTOU cap overshoot).
        let (session_id, mut reservation) = self.reserve_session_slot(&policy).await?;

        let headless = spec.headless.unwrap_or(policy.headless);
        let viewport_width = spec
            .viewport_width
            .unwrap_or(DEFAULT_VIEWPORT_WIDTH)
            .clamp(320, 3840);
        let viewport_height = spec
            .viewport_height
            .unwrap_or(DEFAULT_VIEWPORT_HEIGHT)
            .clamp(240, 2160);

        let on_disconnect = self.make_disconnect_evictor(session_id.clone());
        let session = match BrowserSession::connect(
            session_id.clone(),
            spec.agent_id,
            spec.workspace_root,
            self.home_dir.clone(),
            cdp_ws_url,
            headless,
            viewport_width,
            viewport_height,
            None,
            Arc::clone(&self.policy),
            on_disconnect,
        )
        .await
        {
            Ok(s) => s,
            Err(e) => {
                reservation.release().await;
                return Err(e);
            }
        };

        {
            let mut guard = self.sessions.write().await;
            guard.insert(
                session_id.clone(),
                SessionSlot::Active(Arc::clone(&session)),
            );
            reservation.commit();
        }

        if let Some(url) = validated_initial_url
            && let Err(e) = session
                .execute_action(BrowserActionRequest::Navigate { url }, &policy, &[])
                .await
        {
            warn!(session_id = %session_id, error = %e, "Initial browser navigation failed; tearing down session");
            self.sessions.write().await.remove(&session_id);
            session.shutdown().await;
            return Err(e);
        }

        Ok(session.summary(true).await)
    }

    /// Execute an action against an existing managed browser session.
    pub async fn execute_action(
        &self,
        session_id: &str,
        action: BrowserActionRequest,
    ) -> Result<BrowserActionOutcome, String> {
        self.execute_action_with_allowed_roots(session_id, action, &[])
            .await
    }

    /// Execute an action against an existing managed browser session, supplying any additional
    /// daemon-verified workspace roots permitted for `UploadFile` confinement.
    pub async fn execute_action_with_allowed_roots(
        &self,
        session_id: &str,
        action: BrowserActionRequest,
        extra_allowed_roots: &[PathBuf],
    ) -> Result<BrowserActionOutcome, String> {
        let policy = self.policy.read().await.clone();
        if !policy.enabled {
            return Err("Browser automation is disabled by host policy".to_string());
        }
        let session = {
            let guard = self.sessions.read().await;
            match guard.get(session_id) {
                Some(SessionSlot::Active(sess)) => Arc::clone(sess),
                _ => return Err(format!("browser session `{session_id}` not found")),
            }
        };
        session
            .execute_action(action, &policy, extra_allowed_roots)
            .await
    }

    /// Close a specific browser session and return the updated host snapshot.
    pub async fn close_session(&self, session_id: &str) -> Result<BrowserHostSnapshot, String> {
        let removed = {
            let mut guard = self.sessions.write().await;
            match guard.get(session_id) {
                Some(SessionSlot::Active(_)) => guard.remove(session_id),
                _ => None,
            }
        };
        let Some(SessionSlot::Active(session)) = removed else {
            return Err(format!("browser session `{session_id}` not found"));
        };
        session.shutdown().await;
        Ok(self.list_snapshot().await)
    }

    /// Close all sessions that have been idle longer than `max_idle`.
    pub async fn reap_idle_sessions(&self, max_idle: Duration) -> usize {
        let candidate_ids: Vec<(String, Arc<BrowserSession>)> = {
            let guard = self.sessions.read().await;
            guard
                .iter()
                .filter_map(|(k, v)| match v {
                    SessionSlot::Active(sess) => Some((k.clone(), Arc::clone(sess))),
                    SessionSlot::Starting => None,
                })
                .collect()
        };

        let mut to_close = Vec::new();
        for (id, sess) in candidate_ids {
            if sess.idle_duration().await >= max_idle {
                to_close.push(id);
            }
        }

        let mut closed = 0usize;
        for id in to_close {
            let removed = self.sessions.write().await.remove(&id);
            if let Some(SessionSlot::Active(sess)) = removed {
                sess.shutdown().await;
                closed += 1;
            }
        }
        closed
    }

    /// Gracefully terminate all active CDP sessions, process groups, and temp user-data dirs.
    pub async fn shutdown_all(&self) {
        let drained: Vec<Arc<BrowserSession>> = {
            let mut guard = self.sessions.write().await;
            guard
                .drain()
                .filter_map(|(_, slot)| match slot {
                    SessionSlot::Active(sess) => Some(sess),
                    SessionSlot::Starting => None,
                })
                .collect()
        };
        for session in drained {
            session.shutdown().await;
        }
    }
}

/// Safe RAII guard owning the spawned browser child process and its isolated profile dir.
struct BrowserProcessHandle {
    child: Option<Child>,
    pid: Option<u32>,
    user_data_dir: PathBuf,
}

impl BrowserProcessHandle {
    async fn terminate_and_cleanup(&mut self) {
        if let Some(pid) = self.pid.take() {
            terminate_process_group_async(pid).await;
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
        let dir = self.user_data_dir.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = remove_profile_dir_if_real(&dir);
        })
        .await;
    }
}

impl Drop for BrowserProcessHandle {
    fn drop(&mut self) {
        if let Some(pid) = self.pid.take() {
            terminate_process_group_sync(pid);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
        }
        let _ = remove_profile_dir_if_real(&self.user_data_dir);
    }
}

/// Kill an isolated child process group asynchronously without using `unsafe` code.
async fn terminate_process_group_async(pid: u32) {
    crate::platform::terminate_process_tree_async(pid).await;
}

/// Synchronous fallback for `Drop` only.
fn terminate_process_group_sync(pid: u32) {
    crate::platform::terminate_process_tree_sync(pid);
}

/// Create a symlink-safe isolated browser profile directory under `$ORBIT_HOME/browser-profiles`.
fn create_isolated_profile_dir(home_dir: &Path, session_id: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create orbit home dir: {e}"))?;

    let profiles_root = home_dir.join(BROWSER_PROFILES_DIR);
    match std::fs::symlink_metadata(&profiles_root) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(format!(
                    "refusing symlink or non-directory browser profiles root `{}`",
                    profiles_root.display()
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&profiles_root)
                .map_err(|err| format!("failed to create browser profiles root: {err}"))?;
        }
        Err(e) => {
            return Err(format!("failed to inspect browser profiles root: {e}"));
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(&profiles_root)
            .map_err(|e| format!("failed to verify browser profiles root: {e}"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("refusing to chmod symlink browser profiles root".to_string());
        }
        let _ = std::fs::set_permissions(&profiles_root, std::fs::Permissions::from_mode(0o700));
    }

    let user_data_dir = profiles_root.join(format!("orbit-cdp-{session_id}"));
    // `create_dir` fails atomically if the path already exists (including pre-planted symlinks).
    std::fs::create_dir(&user_data_dir)
        .map_err(|e| format!("failed to create isolated browser profile dir: {e}"))?;

    let meta = std::fs::symlink_metadata(&user_data_dir)
        .map_err(|e| format!("failed to verify isolated browser profile dir: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("refusing symlink browser profile dir".to_string());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&user_data_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("failed to chmod browser profile dir: {e}"))?;
    }

    Ok(user_data_dir)
}

/// Remove a browser profile directory ONLY after verifying via `symlink_metadata` that it is a real
/// non-symlink directory (prevents symlink traversal deletion).
fn remove_profile_dir_if_real(dir: &Path) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(dir) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("failed to stat browser profile dir: {e}")),
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(format!(
            "refusing to delete symlink or non-directory path `{}`",
            dir.display()
        ));
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("failed to remove browser profile dir: {e}"))
}

struct SpawnedBrowser {
    process: BrowserProcessHandle,
    devtools_browser_ws_url: String,
}

async fn spawn_isolated_browser(
    home_dir: &Path,
    chrome_binary: &str,
    session_id: &str,
    headless: bool,
    viewport_width: u32,
    viewport_height: u32,
) -> Result<SpawnedBrowser, String> {
    let home_owned = home_dir.to_path_buf();
    let sid_owned = session_id.to_string();
    let user_data_dir =
        tokio::task::spawn_blocking(move || create_isolated_profile_dir(&home_owned, &sid_owned))
            .await
            .map_err(|e| format!("profile creation task failed: {e}"))??;

    let mut cmd = Command::new(chrome_binary);
    // Scrub dangerous / Orbit-internal environment variables before spawning browser child.
    for (key, _) in std::env::vars_os() {
        let key_str = key.to_string_lossy();
        if crate::mcp_servers::is_blocked_mcp_env_key(&key_str) {
            cmd.env_remove(&key);
        }
    }

    cmd.arg("--remote-debugging-port=0")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(format!("--user-data-dir={}", user_data_dir.display()))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg("--disable-background-networking")
        .arg("--disable-sync")
        .arg("--disable-translate")
        .arg("--disable-default-apps")
        .arg("--mute-audio")
        .arg(format!("--window-size={viewport_width},{viewport_height}"));
    if headless {
        cmd.arg("--headless=new");
    }
    cmd.arg("about:blank");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(unix)]
    {
        // Safe Rust since 1.64: isolates browser + renderer helpers in their own process group.
        cmd.process_group(0);
    }

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            let dir = user_data_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = remove_profile_dir_if_real(&dir);
            })
            .await;
            return Err(format!("failed to spawn browser `{chrome_binary}`: {e}"));
        }
    };

    let pid = child.id();
    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            let mut handle = BrowserProcessHandle {
                child: Some(child),
                pid,
                user_data_dir,
            };
            handle.terminate_and_cleanup().await;
            return Err("browser stderr pipe unavailable".to_string());
        }
    };

    let mut process = BrowserProcessHandle {
        child: Some(child),
        pid,
        user_data_dir,
    };

    let ws_result = tokio::time::timeout(CDP_STARTUP_TIMEOUT, async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(ws_url) = extract_devtools_ws_url(&line) {
                return Ok(ws_url);
            }
        }
        Err("browser exited before printing DevTools listening WebSocket URL".to_string())
    })
    .await;

    let devtools_browser_ws_url = match ws_result {
        Ok(Ok(url)) => {
            if let Err(e) = validate_cdp_ws_url(&url) {
                process.terminate_and_cleanup().await;
                return Err(e);
            }
            url
        }
        Ok(Err(e)) => {
            process.terminate_and_cleanup().await;
            return Err(e);
        }
        Err(_) => {
            process.terminate_and_cleanup().await;
            return Err(format!(
                "timed out after {}s waiting for browser DevTools WebSocket",
                CDP_STARTUP_TIMEOUT.as_secs()
            ));
        }
    };

    Ok(SpawnedBrowser {
        process,
        devtools_browser_ws_url,
    })
}

fn extract_devtools_ws_url(line: &str) -> Option<String> {
    let marker = "DevTools listening on ";
    let idx = line.find(marker)?;
    let rest = line[idx + marker.len()..].trim();
    let candidate = rest.split_whitespace().next()?;
    if candidate.starts_with("ws://") || candidate.starts_with("wss://") {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Query Chrome's local loopback HTTP DevTools endpoint to obtain the active page target WS URL.
async fn discover_page_target_ws_url(browser_ws_url: &str) -> Result<String, String> {
    let parsed = validate_cdp_ws_url(browser_ws_url)?;
    let port = parsed
        .port()
        .ok_or_else(|| "CDP WebSocket URL missing port".to_string())?;
    let host = parsed.host_str().unwrap_or("127.0.0.1");

    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("failed to build loopback HTTP client: {e}"))?;

    let list_url = format!("http://{host}:{port}/json/list");
    for _ in 0..8 {
        if let Ok(resp) = client.get(&list_url).send().await
            && resp.status().is_success()
            && let Ok(targets) = resp.json::<Vec<Value>>().await
        {
            for target in &targets {
                if target.get("type").and_then(Value::as_str) == Some("page")
                    && let Some(ws) = target.get("webSocketDebuggerUrl").and_then(Value::as_str)
                {
                    validate_cdp_ws_url(ws)?;
                    return Ok(ws.to_string());
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }

    // Fallback: create a fresh page target on loopback DevTools.
    let new_url = format!("http://{host}:{port}/json/new?about:blank");
    if let Ok(resp) = client.put(&new_url).send().await
        && resp.status().is_success()
        && let Ok(target) = resp.json::<Value>().await
        && let Some(ws) = target.get("webSocketDebuggerUrl").and_then(Value::as_str)
    {
        validate_cdp_ws_url(ws)?;
        return Ok(ws.to_string());
    }

    Err("no attachable CDP page target found on local browser".to_string())
}

/// Shared runtime state updated by CDP events and browser actions.
struct SessionMutableState {
    url: String,
    title: String,
    elements: Vec<CachedElement>,
    console_errors: VecDeque<String>,
    recent_actions: VecDeque<BrowserActionAudit>,
    last_screenshot_base64: Option<String>,
    captcha_detected: bool,
    captcha_reason: Option<String>,
    updated_at_ms: u64,
    last_active: Instant,
}

#[derive(Debug, Clone)]
struct CachedElement {
    wire: BrowserElementRef,
    sensitive: bool,
    captcha_like: bool,
}

/// Pure-Rust CDP JSON-RPC WebSocket client multiplexing commands, events, and live `Fetch` guards.
struct CdpClient {
    out_tx: mpsc::Sender<TungMsg>,
    next_id: Arc<AtomicU64>,
    pending: PendingCdpReplies,
    reader_task: Mutex<Option<JoinHandle<()>>>,
    writer_task: Mutex<Option<JoinHandle<()>>>,
}

impl CdpClient {
    async fn connect(
        ws_url: &str,
        shared_state: Arc<RwLock<SessionMutableState>>,
        policy_ref: Arc<RwLock<BrowserPolicyConfig>>,
        on_disconnect: DisconnectCallback,
    ) -> Result<Self, String> {
        validate_cdp_ws_url(ws_url)?;

        let mut ws_cfg = WebSocketConfig::default();
        ws_cfg.max_message_size = Some(16 * 1024 * 1024);
        ws_cfg.max_frame_size = Some(16 * 1024 * 1024);

        let (ws_stream, _) = tokio::time::timeout(
            CDP_STARTUP_TIMEOUT,
            connect_async_with_config(ws_url, Some(ws_cfg), false),
        )
        .await
        .map_err(|_| "timed out connecting to local CDP WebSocket".to_string())?
        .map_err(|e| format!("failed to connect to local CDP WebSocket: {e}"))?;

        let (mut ws_sink, mut ws_rx) = ws_stream.split();
        let (out_tx, mut out_rx) = mpsc::channel::<TungMsg>(128);
        let next_id = Arc::new(AtomicU64::new(1));
        let pending: PendingCdpReplies = Arc::new(Mutex::new(HashMap::new()));

        let writer_task = tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                if ws_sink.send(msg).await.is_err() {
                    break;
                }
            }
            let _ = ws_sink.close().await;
        });

        let pending_reader = Arc::clone(&pending);
        let state_reader = Arc::clone(&shared_state);
        let out_tx_reader = out_tx.clone();
        let next_id_reader = Arc::clone(&next_id);
        let reader_task = tokio::spawn(async move {
            while let Some(item) = ws_rx.next().await {
                match item {
                    Ok(TungMsg::Text(text)) => {
                        let Ok(val) = serde_json::from_str::<Value>(&text) else {
                            continue;
                        };
                        if let Some(id) = val.get("id").and_then(Value::as_u64) {
                            let res = if let Some(err_obj) = val.get("error") {
                                let msg = err_obj
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or("CDP command error")
                                    .to_string();
                                Err(msg)
                            } else {
                                Ok(val.get("result").cloned().unwrap_or(Value::Null))
                            };
                            if let Some(waiter) = pending_reader.lock().await.remove(&id) {
                                let _ = waiter.send(res);
                            }
                        } else if let Some(method) = val.get("method").and_then(Value::as_str) {
                            let params = val.get("params").cloned().unwrap_or(Value::Null);
                            if method == "Fetch.requestPaused" {
                                spawn_fetch_request_guard(
                                    params,
                                    Arc::clone(&policy_ref),
                                    Arc::clone(&state_reader),
                                    out_tx_reader.clone(),
                                    Arc::clone(&next_id_reader),
                                );
                            } else {
                                handle_cdp_event(method, &params, &state_reader).await;
                            }
                        }
                    }
                    Ok(TungMsg::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            // Fail any in-flight command waiters on disconnect.
            let mut waiters = pending_reader.lock().await;
            for (_, tx) in waiters.drain() {
                let _ = tx.send(Err("CDP WebSocket closed".to_string()));
            }
            drop(waiters);
            on_disconnect();
        });

        Ok(Self {
            out_tx,
            next_id,
            pending,
            reader_task: Mutex::new(Some(reader_task)),
            writer_task: Mutex::new(Some(writer_task)),
        })
    }

    async fn send_command(&self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let payload = json!({
            "id": id,
            "method": method,
            "params": params,
        });
        let text = serde_json::to_string(&payload)
            .map_err(|e| format!("failed to serialize CDP command `{method}`: {e}"))?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        if self.out_tx.send(TungMsg::Text(text.into())).await.is_err() {
            self.pending.lock().await.remove(&id);
            return Err(format!("CDP channel closed before sending `{method}`"));
        }

        match tokio::time::timeout(CDP_COMMAND_TIMEOUT, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(format!("CDP connection closed waiting for `{method}`")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(format!(
                    "CDP command `{method}` timed out after {}s",
                    CDP_COMMAND_TIMEOUT.as_secs()
                ))
            }
        }
    }

    async fn close(&self) {
        let _ = self.out_tx.send(TungMsg::Close(None)).await;
        if let Some(h) = self.reader_task.lock().await.take() {
            h.abort();
        }
        if let Some(h) = self.writer_task.lock().await.take() {
            h.abort();
        }
    }
}

/// Validate an intercepted `Fetch.requestPaused` request URL against live host policy.
fn spawn_fetch_request_guard(
    params: Value,
    policy_ref: Arc<RwLock<BrowserPolicyConfig>>,
    shared_state: Arc<RwLock<SessionMutableState>>,
    out_tx: mpsc::Sender<TungMsg>,
    next_id: Arc<AtomicU64>,
) {
    tokio::spawn(async move {
        let Some(request_id) = params.get("requestId").and_then(Value::as_str) else {
            return;
        };
        let req_url = params
            .get("request")
            .and_then(|r| r.get("url"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let resource_type = params
            .get("resourceType")
            .and_then(Value::as_str)
            .unwrap_or("Document");

        let policy = policy_ref.read().await.clone();
        let verdict = validate_fetch_intercept_url(req_url, resource_type, &policy).await;

        let cmd = match verdict {
            Ok(()) => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                json!({
                    "id": id,
                    "method": "Fetch.continueRequest",
                    "params": { "requestId": request_id },
                })
            }
            Err(reason) => {
                warn!(url = %req_url, resource_type, reason = %reason, "CDP Fetch interceptor blocked request");
                let entry = BrowserActionAudit {
                    id: format!("ba_{}", &Uuid::new_v4().simple().to_string()[..10]),
                    action: "fetch_blocked".to_string(),
                    target: truncate_chars(req_url, 200),
                    ok: false,
                    detail: truncate_chars(&reason, 300),
                    timestamp_ms: now_ms(),
                };
                {
                    let mut st = shared_state.write().await;
                    st.updated_at_ms = entry.timestamp_ms;
                    push_bounded(&mut st.recent_actions, entry, MAX_AUDIT_ENTRIES_PER_SESSION);
                }
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                json!({
                    "id": id,
                    "method": "Fetch.failRequest",
                    "params": {
                        "requestId": request_id,
                        "errorReason": "BlockedByClient",
                    },
                })
            }
        };

        if let Ok(text) = serde_json::to_string(&cmd) {
            let _ = out_tx.send(TungMsg::Text(text.into())).await;
        }
    });
}

/// Evaluate whether an intercepted CDP Fetch request URL is allowed under `policy`.
///
/// Permits `about:blank` and non-document `data:`/`blob:` subresources only; strictly validates
/// every other URL (including all `Document`, `XHR`, `Fetch`, `Script`, `Image`, `WebSocket`,
/// redirect targets, and `file://`/`chrome://` requests) against `validate_navigation_url_async`.
pub async fn validate_fetch_intercept_url(
    raw_url: &str,
    resource_type: &str,
    policy: &BrowserPolicyConfig,
) -> Result<(), String> {
    let trimmed = raw_url.trim();
    if trimmed.eq_ignore_ascii_case("about:blank") {
        return Ok(());
    }
    let is_document =
        resource_type.eq_ignore_ascii_case("document") || resource_type.trim().is_empty();
    let lower = trimmed.to_ascii_lowercase();
    if !is_document && (lower.starts_with("data:") || lower.starts_with("blob:")) {
        return Ok(());
    }
    validate_navigation_url_async(trimmed, policy)
        .await
        .map(|_| ())
}

async fn handle_cdp_event(
    method: &str,
    params: &Value,
    shared_state: &Arc<RwLock<SessionMutableState>>,
) {
    match method {
        "Runtime.consoleAPICalled" => {
            if params.get("type").and_then(Value::as_str) != Some("error") {
                return;
            }
            let text = params
                .get("args")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|arg| {
                            arg.get("value")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .or_else(|| {
                                    arg.get("description")
                                        .and_then(Value::as_str)
                                        .map(str::to_string)
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_else(|| "console.error".to_string());
            let trimmed = truncate_chars(text.trim(), 300);
            if trimmed.is_empty() {
                return;
            }
            let mut st = shared_state.write().await;
            push_bounded(
                &mut st.console_errors,
                trimmed,
                MAX_CONSOLE_ERRORS_PER_SESSION,
            );
            st.updated_at_ms = now_ms();
        }
        "Runtime.exceptionThrown" => {
            let details = params.get("exceptionDetails");
            let msg = details
                .and_then(|d| {
                    d.get("exception")
                        .and_then(|e| e.get("description"))
                        .and_then(Value::as_str)
                        .or_else(|| d.get("text").and_then(Value::as_str))
                })
                .unwrap_or("Uncaught page exception");
            let trimmed = truncate_chars(msg.trim(), 300);
            if !trimmed.is_empty() {
                let mut st = shared_state.write().await;
                push_bounded(
                    &mut st.console_errors,
                    trimmed,
                    MAX_CONSOLE_ERRORS_PER_SESSION,
                );
                st.updated_at_ms = now_ms();
            }
        }
        "Page.frameNavigated" => {
            if let Some(frame) = params.get("frame")
                && frame.get("parentId").is_none()
                && let Some(url) = frame.get("url").and_then(Value::as_str)
            {
                let mut st = shared_state.write().await;
                st.url = url.to_string();
                st.updated_at_ms = now_ms();
            }
        }
        _ => {}
    }
}

/// Live managed CDP browser session with serialized action execution.
struct BrowserSession {
    session_id: String,
    agent_id: Option<String>,
    workspace_root: Option<PathBuf>,
    home_dir: PathBuf,
    headless: bool,
    viewport_width: u32,
    viewport_height: u32,
    cdp: CdpClient,
    state: Arc<RwLock<SessionMutableState>>,
    action_lock: Mutex<()>,
    process: Mutex<Option<BrowserProcessHandle>>,
}

impl BrowserSession {
    #[allow(clippy::too_many_arguments)]
    async fn connect(
        session_id: String,
        agent_id: Option<String>,
        workspace_root: Option<PathBuf>,
        home_dir: PathBuf,
        cdp_ws_url: &str,
        headless: bool,
        viewport_width: u32,
        viewport_height: u32,
        process: Option<BrowserProcessHandle>,
        policy_ref: Arc<RwLock<BrowserPolicyConfig>>,
        on_disconnect: DisconnectCallback,
    ) -> Result<Arc<Self>, String> {
        let state = Arc::new(RwLock::new(SessionMutableState {
            url: "about:blank".to_string(),
            title: String::new(),
            elements: Vec::new(),
            console_errors: VecDeque::new(),
            recent_actions: VecDeque::new(),
            last_screenshot_base64: None,
            captcha_detected: false,
            captcha_reason: None,
            updated_at_ms: now_ms(),
            last_active: Instant::now(),
        }));

        let cdp =
            match CdpClient::connect(cdp_ws_url, Arc::clone(&state), policy_ref, on_disconnect)
                .await
            {
                Ok(c) => c,
                Err(e) => {
                    if let Some(mut proc_handle) = process {
                        proc_handle.terminate_and_cleanup().await;
                    }
                    return Err(e);
                }
            };

        // Enable core CDP domains, viewport emulation, and live `Fetch` request interception.
        for (method, params) in [
            ("Page.enable", json!({})),
            ("Runtime.enable", json!({})),
            ("DOM.enable", json!({})),
            (
                "Fetch.enable",
                json!({
                    "patterns": [{
                        "urlPattern": "*",
                        "requestStage": "Request"
                    }]
                }),
            ),
            (
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width": viewport_width,
                    "height": viewport_height,
                    "deviceScaleFactor": 1,
                    "mobile": false,
                }),
            ),
        ] {
            if let Err(e) = cdp.send_command(method, params).await {
                debug!(session_id = %session_id, method, error = %e, "CDP init command returned warning");
            }
        }

        Ok(Arc::new(Self {
            session_id,
            agent_id,
            workspace_root,
            home_dir,
            headless,
            viewport_width,
            viewport_height,
            cdp,
            state,
            action_lock: Mutex::new(()),
            process: Mutex::new(process),
        }))
    }

    async fn idle_duration(&self) -> Duration {
        self.state.read().await.last_active.elapsed()
    }

    async fn summary(&self, include_screenshot: bool) -> BrowserSessionSummary {
        let st = self.state.read().await;
        BrowserSessionSummary {
            session_id: self.session_id.clone(),
            agent_id: self.agent_id.clone(),
            url: st.url.clone(),
            title: st.title.clone(),
            viewport_width: self.viewport_width,
            viewport_height: self.viewport_height,
            headless: self.headless,
            element_count: st.elements.len() as u32,
            console_errors: st.console_errors.iter().cloned().collect(),
            recent_actions: st.recent_actions.iter().cloned().collect(),
            last_screenshot_base64: if include_screenshot {
                st.last_screenshot_base64.clone()
            } else {
                None
            },
            updated_at_ms: st.updated_at_ms,
        }
    }

    async fn record_audit(&self, action: &str, target: &str, ok: bool, detail: impl Into<String>) {
        let entry = BrowserActionAudit {
            id: format!("ba_{}", &Uuid::new_v4().simple().to_string()[..10]),
            action: action.to_string(),
            target: truncate_chars(target, 200),
            ok,
            detail: truncate_chars(&detail.into(), 300),
            timestamp_ms: now_ms(),
        };
        let mut st = self.state.write().await;
        st.last_active = Instant::now();
        st.updated_at_ms = entry.timestamp_ms;
        push_bounded(&mut st.recent_actions, entry, MAX_AUDIT_ENTRIES_PER_SESSION);
    }

    /// Enforce that both the observed document URL (if any) and cached `state.url` satisfy `policy`.
    /// On violation, immediately navigate back to `about:blank`, scrub cached screenshot, and fail.
    async fn enforce_current_document_url(
        &self,
        candidate_url: Option<&str>,
        policy: &BrowserPolicyConfig,
        action_name: &str,
    ) -> Result<(), String> {
        let state_url = self.state.read().await.url.clone();
        let mut urls_to_check: Vec<String> = Vec::with_capacity(2);
        if let Some(u) = candidate_url
            && !u.trim().is_empty()
            && !u.eq_ignore_ascii_case("about:blank")
        {
            urls_to_check.push(u.to_string());
        }
        if !state_url.trim().is_empty()
            && !state_url.eq_ignore_ascii_case("about:blank")
            && !urls_to_check.iter().any(|existing| existing == &state_url)
        {
            urls_to_check.push(state_url);
        }

        for url in urls_to_check {
            if let Err(err) = validate_navigation_url_async(&url, policy).await {
                let _ = self
                    .cdp
                    .send_command("Page.navigate", json!({ "url": "about:blank" }))
                    .await;
                {
                    let mut st = self.state.write().await;
                    st.url = "about:blank".to_string();
                    st.title.clear();
                    st.elements.clear();
                    st.last_screenshot_base64 = None;
                    st.updated_at_ms = now_ms();
                }
                let msg = format!("Document URL disallowed by host policy: {err}");
                self.record_audit(action_name, &url, false, msg.clone())
                    .await;
                return Err(msg);
            }
        }
        Ok(())
    }

    async fn execute_action(
        &self,
        action: BrowserActionRequest,
        policy: &BrowserPolicyConfig,
        extra_allowed_roots: &[PathBuf],
    ) -> Result<BrowserActionOutcome, String> {
        let _guard = self.action_lock.lock().await;
        {
            let mut st = self.state.write().await;
            st.last_active = Instant::now();
        }

        match action {
            BrowserActionRequest::Navigate { url } => {
                let validated = match validate_navigation_url_async(&url, policy).await {
                    Ok(u) => u,
                    Err(e) => {
                        self.record_audit("navigate", &url, false, e.clone()).await;
                        return Err(e);
                    }
                };
                let target_url = validated.to_string();
                if let Err(e) = self
                    .cdp
                    .send_command("Page.navigate", json!({ "url": target_url }))
                    .await
                {
                    self.record_audit("navigate", &target_url, false, e.clone())
                        .await;
                    return Err(e);
                }

                // Wait briefly for navigation to settle, then inspect the page.
                tokio::time::sleep(Duration::from_millis(120)).await;
                let snap = self.capture_dom_snapshot().await;

                // Post-navigation redirect guard: ALWAYS verify resulting document URL & state.url.
                let observed_url = snap.as_ref().ok().map(|s| s.url.as_str());
                self.enforce_current_document_url(observed_url, policy, "navigate")
                    .await?;

                match snap {
                    Ok(s) => {
                        self.apply_snapshot(&s).await;
                        self.record_audit(
                            "navigate",
                            &s.url,
                            true,
                            format!(
                                "Navigated to {}",
                                if s.title.is_empty() { &s.url } else { &s.title }
                            ),
                        )
                        .await;
                        if s.captcha_detected {
                            self.record_captcha_audit(s.captcha_reason.as_deref()).await;
                        }
                        let text_excerpt = format_excerpt_with_captcha(
                            s.text_excerpt,
                            s.captcha_detected,
                            s.captcha_reason.as_deref(),
                        );
                        Ok(BrowserActionOutcome {
                            session: self.summary(false).await,
                            elements: s.elements.into_iter().map(|c| c.wire).collect(),
                            screenshot_base64: None,
                            text_excerpt: Some(text_excerpt),
                            captcha_detected: s.captcha_detected,
                            picked_element: None,
                        })
                    }
                    Err(e) => {
                        {
                            let mut st = self.state.write().await;
                            st.url = target_url.clone();
                            st.updated_at_ms = now_ms();
                        }
                        self.record_audit(
                            "navigate",
                            &target_url,
                            true,
                            format!("Navigated (snapshot note: {e})"),
                        )
                        .await;
                        Ok(BrowserActionOutcome {
                            session: self.summary(false).await,
                            elements: Vec::new(),
                            screenshot_base64: None,
                            text_excerpt: None,
                            captcha_detected: false,
                            picked_element: None,
                        })
                    }
                }
            }

            BrowserActionRequest::Snapshot => {
                let snap = match self.capture_dom_snapshot().await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = self
                            .enforce_current_document_url(None, policy, "snapshot")
                            .await;
                        self.record_audit("snapshot", "page", false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                self.enforce_current_document_url(Some(&snap.url), policy, "snapshot")
                    .await?;
                self.apply_snapshot(&snap).await;
                self.record_audit(
                    "snapshot",
                    &snap.url,
                    true,
                    format!("Captured {} interactive elements", snap.elements.len()),
                )
                .await;
                if snap.captcha_detected {
                    self.record_captcha_audit(snap.captcha_reason.as_deref())
                        .await;
                }
                let text_excerpt = format_excerpt_with_captcha(
                    snap.text_excerpt,
                    snap.captcha_detected,
                    snap.captcha_reason.as_deref(),
                );
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements: snap.elements.into_iter().map(|c| c.wire).collect(),
                    screenshot_base64: None,
                    text_excerpt: Some(text_excerpt),
                    captcha_detected: snap.captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Screenshot { full_page } => {
                // Validate current live document URL BEFORE capturing pixels.
                let live_href = self.query_live_location_href().await;
                self.enforce_current_document_url(live_href.as_deref(), policy, "screenshot")
                    .await?;

                let res = match self
                    .cdp
                    .send_command(
                        "Page.captureScreenshot",
                        json!({
                            "format": "png",
                            "captureBeyondViewport": full_page,
                        }),
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        self.record_audit("screenshot", "viewport", false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                let b64 = res
                    .get("data")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "CDP Page.captureScreenshot returned no image data".to_string())?
                    .to_string();

                if b64.len() > MAX_SCREENSHOT_BASE64_LEN {
                    let msg = format!(
                        "Screenshot payload exceeds {} MiB cap",
                        MAX_SCREENSHOT_BYTES / (1024 * 1024)
                    );
                    self.record_audit("screenshot", "viewport", false, msg.clone())
                        .await;
                    return Err(msg);
                }
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(&b64)
                    .map_err(|e| format!("invalid base64 screenshot from CDP: {e}"))?;
                if decoded.len() > MAX_SCREENSHOT_BYTES {
                    let msg = format!(
                        "Screenshot payload ({} bytes) exceeds {} MiB cap",
                        decoded.len(),
                        MAX_SCREENSHOT_BYTES / (1024 * 1024)
                    );
                    self.record_audit("screenshot", "viewport", false, msg.clone())
                        .await;
                    return Err(msg);
                }

                {
                    let mut st = self.state.write().await;
                    st.last_screenshot_base64 = Some(b64.clone());
                    st.updated_at_ms = now_ms();
                }
                self.record_audit(
                    "screenshot",
                    if full_page { "full_page" } else { "viewport" },
                    true,
                    format!("Captured PNG ({} bytes)", decoded.len()),
                )
                .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                // Avoid duplicating the base64 screenshot inside `session.last_screenshot_base64`;
                // callers read the fresh image from top-level `screenshot_base64`.
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: Some(b64),
                    text_excerpt: None,
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Click { ref_id, x, y } => {
                let (click_x, click_y, target_label, is_cached_captcha) = if let Some(ref r) =
                    ref_id
                {
                    let st = self.state.read().await;
                    let found = st.elements.iter().find(|el| el.wire.ref_id == *r).cloned();
                    let Some(el) = found else {
                        drop(st);
                        let msg = format!(
                            "Element ref `{r}` not found in latest snapshot; call snapshot first"
                        );
                        self.record_audit("click", r, false, msg.clone()).await;
                        return Err(msg);
                    };
                    let cx = el.wire.x + (el.wire.width / 2.0);
                    let cy = el.wire.y + (el.wire.height / 2.0);
                    let label = if el.wire.name.is_empty() {
                        r.clone()
                    } else {
                        format!("{r} ({})", truncate_chars(&el.wire.name, 40))
                    };
                    (cx, cy, label, el.captcha_like)
                } else if let (Some(cx), Some(cy)) = (x, y) {
                    if !cx.is_finite()
                        || !cy.is_finite()
                        || cx < 0.0
                        || cy < 0.0
                        || cx > 10_000.0
                        || cy > 10_000.0
                    {
                        let msg =
                            "Click coordinates must be finite values within 0..=10000".to_string();
                        self.record_audit("click", "coords", false, msg.clone())
                            .await;
                        return Err(msg);
                    }
                    (cx, cy, format!("({cx:.0}, {cy:.0})"), false)
                } else {
                    let msg = "Click requires either `ref_id` or both `x` and `y` coordinates"
                        .to_string();
                    self.record_audit("click", "missing", false, msg.clone())
                        .await;
                    return Err(msg);
                };

                // 1. Check cached snapshot captcha boxes for BOTH `ref_id` and coordinate clicks.
                let hits_cached_captcha_box = is_cached_captcha || {
                    let st = self.state.read().await;
                    st.elements.iter().any(|el| {
                        el.captcha_like
                            && click_x >= el.wire.x
                            && click_x <= el.wire.x + el.wire.width
                            && click_y >= el.wire.y
                            && click_y <= el.wire.y + el.wire.height
                    })
                };
                if hits_cached_captcha_box {
                    let msg = "captcha_detected: Refusing to click CAPTCHA or human-verification widget; please complete it directly".to_string();
                    self.record_audit("click", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }

                // 2. Live coordinate hit-test via `document.elementFromPoint(cx, cy)` (fail-closed).
                match self.inspect_captcha_at_point(click_x, click_y).await {
                    Ok(Some(reason)) => {
                        let msg = format!(
                            "captcha_detected: Refusing to click CAPTCHA or human-verification widget ({reason}); please complete it directly"
                        );
                        self.record_audit("click", &target_label, false, msg.clone())
                            .await;
                        return Err(msg);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let msg = format!(
                            "captcha_detected: Refusing click (fail-closed CAPTCHA hit-test error: {e})"
                        );
                        self.record_audit("click", &target_label, false, msg.clone())
                            .await;
                        return Err(msg);
                    }
                }

                for (ev_type, button, click_count) in [
                    ("mouseMoved", "none", 0),
                    ("mousePressed", "left", 1),
                    ("mouseReleased", "left", 1),
                ] {
                    if let Err(e) = self
                        .cdp
                        .send_command(
                            "Input.dispatchMouseEvent",
                            json!({
                                "type": ev_type,
                                "x": click_x,
                                "y": click_y,
                                "button": button,
                                "clickCount": click_count,
                            }),
                        )
                        .await
                    {
                        self.record_audit("click", &target_label, false, e.clone())
                            .await;
                        return Err(e);
                    }
                }

                self.record_audit(
                    "click",
                    &target_label,
                    true,
                    format!("Clicked at ({click_x:.1}, {click_y:.1})"),
                )
                .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: None,
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Type {
                ref_id,
                text,
                submit,
            } => {
                let target_label = ref_id.as_deref().unwrap_or("active_element").to_string();

                // 1. Check cached snapshot metadata if `ref_id` was supplied.
                let cached_target = if let Some(ref r) = ref_id {
                    let st = self.state.read().await;
                    let found = st.elements.iter().find(|el| el.wire.ref_id == *r).cloned();
                    if found.is_none() {
                        drop(st);
                        let msg = format!(
                            "Element ref `{r}` not found in latest snapshot; call snapshot first"
                        );
                        self.record_audit("type", r, false, msg.clone()).await;
                        return Err(msg);
                    }
                    found
                } else {
                    None
                };

                if policy.block_password_inputs
                    && let Some(ref cached) = cached_target
                    && (cached.sensitive
                        || is_sensitive_element_metadata(
                            cached.wire.input_type.as_deref(),
                            &cached.wire.name,
                            &cached.wire.role,
                        ))
                {
                    let msg = format!(
                        "Refusing to type into sensitive password/payment input `{target_label}` (blocked by host browser policy)"
                    );
                    self.record_audit("type", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }

                // 2. Fail-closed live DOM check BEFORE focusing/typing.
                if policy.block_password_inputs {
                    match self.inspect_live_sensitive_input(ref_id.as_deref()).await {
                        Ok(Some(sensitive_reason)) => {
                            let msg = format!(
                                "Refusing to type into sensitive input `{target_label}`: {sensitive_reason}"
                            );
                            self.record_audit("type", &target_label, false, msg.clone())
                                .await;
                            return Err(msg);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let msg = format!(
                                "Refusing to type into `{target_label}` (fail-closed sensitive input check failed: {e})"
                            );
                            self.record_audit("type", &target_label, false, msg.clone())
                                .await;
                            return Err(msg);
                        }
                    }
                }

                // 3. Focus element if `ref_id` was given (click center).
                if let Some(ref cached) = cached_target {
                    let cx = cached.wire.x + (cached.wire.width / 2.0);
                    let cy = cached.wire.y + (cached.wire.height / 2.0);
                    for (ev_type, button, click_count) in [
                        ("mouseMoved", "none", 0),
                        ("mousePressed", "left", 1),
                        ("mouseReleased", "left", 1),
                    ] {
                        let _ = self
                            .cdp
                            .send_command(
                                "Input.dispatchMouseEvent",
                                json!({
                                    "type": ev_type,
                                    "x": cx,
                                    "y": cy,
                                    "button": button,
                                    "clickCount": click_count,
                                }),
                            )
                            .await;
                    }

                    // 3b. Re-verify `document.activeElement` AFTER clicking and BEFORE typing!
                    // Protects against shifted layouts, overlays, Shadow DOM, or cross-origin iframes.
                    if policy.block_password_inputs {
                        match self.inspect_live_sensitive_input(None).await {
                            Ok(Some(sensitive_reason)) => {
                                let msg = format!(
                                    "Refusing to type into post-focus sensitive input `{target_label}`: {sensitive_reason}"
                                );
                                self.record_audit("type", &target_label, false, msg.clone())
                                    .await;
                                return Err(msg);
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let msg = format!(
                                    "Refusing to type into `{target_label}` (fail-closed post-focus sensitive input check failed: {e})"
                                );
                                self.record_audit("type", &target_label, false, msg.clone())
                                    .await;
                                return Err(msg);
                            }
                        }
                    }
                }

                // 4. Insert text via CDP `Input.insertText`.
                if let Err(e) = self
                    .cdp
                    .send_command("Input.insertText", json!({ "text": text }))
                    .await
                {
                    self.record_audit("type", &target_label, false, e.clone())
                        .await;
                    return Err(e);
                }

                if submit {
                    for ev_type in ["keyDown", "keyUp"] {
                        if let Err(e) = self
                            .cdp
                            .send_command(
                                "Input.dispatchKeyEvent",
                                json!({
                                    "type": ev_type,
                                    "key": "Enter",
                                    "code": "Enter",
                                    "windowsVirtualKeyCode": 13,
                                    "nativeVirtualKeyCode": 13,
                                }),
                            )
                            .await
                        {
                            self.record_audit("type", &target_label, false, e.clone())
                                .await;
                            return Err(e);
                        }
                    }
                }

                // Never log raw typed content in audit log; record redacted length only.
                let char_count = text.chars().count();
                self.record_audit(
                    "type",
                    &target_label,
                    true,
                    format!(
                        "Inserted {char_count} chars{}",
                        if submit { " + Enter" } else { "" }
                    ),
                )
                .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: None,
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Scroll { delta_x, delta_y } => {
                if !delta_x.is_finite() || !delta_y.is_finite() {
                    let msg = "Scroll deltas must be finite numbers".to_string();
                    self.record_audit("scroll", "viewport", false, msg.clone())
                        .await;
                    return Err(msg);
                }
                let dx = delta_x.clamp(-20_000.0, 20_000.0);
                let dy = delta_y.clamp(-20_000.0, 20_000.0);
                let cx = f64::from(self.viewport_width) / 2.0;
                let cy = f64::from(self.viewport_height) / 2.0;

                if let Err(e) = self
                    .cdp
                    .send_command(
                        "Input.dispatchMouseEvent",
                        json!({
                            "type": "mouseWheel",
                            "x": cx,
                            "y": cy,
                            "deltaX": dx,
                            "deltaY": dy,
                        }),
                    )
                    .await
                {
                    self.record_audit("scroll", "viewport", false, e.clone())
                        .await;
                    return Err(e);
                }

                self.record_audit(
                    "scroll",
                    "viewport",
                    true,
                    format!("Scrolled dx={dx:.0}, dy={dy:.0}"),
                )
                .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: None,
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Back => {
                let hist = self
                    .cdp
                    .send_command("Page.getNavigationHistory", json!({}))
                    .await;
                let mut navigated = false;
                if let Ok(val) = hist
                    && let Some(cur_idx) = val.get("currentIndex").and_then(Value::as_i64)
                    && cur_idx > 0
                    && let Some(entries) = val.get("entries").and_then(Value::as_array)
                    && let Some(prev_entry) = entries.get((cur_idx - 1) as usize)
                    && let Some(entry_id) = prev_entry.get("id").and_then(Value::as_i64)
                {
                    if let Some(prev_url) = prev_entry.get("url").and_then(Value::as_str)
                        && !prev_url.eq_ignore_ascii_case("about:blank")
                        && let Err(e) = validate_navigation_url_async(prev_url, policy).await
                    {
                        self.record_audit("back", prev_url, false, e.clone()).await;
                        return Err(e);
                    }
                    self.cdp
                        .send_command(
                            "Page.navigateToHistoryEntry",
                            json!({ "entryId": entry_id }),
                        )
                        .await
                        .map_err(|e| format!("Page.navigateToHistoryEntry failed: {e}"))?;
                    navigated = true;
                }
                if !navigated {
                    let _ = self
                        .cdp
                        .send_command(
                            "Runtime.evaluate",
                            json!({ "expression": "window.history.back()", "returnByValue": true }),
                        )
                        .await;
                }

                tokio::time::sleep(Duration::from_millis(120)).await;
                let snap = self.capture_dom_snapshot().await.ok();
                self.enforce_current_document_url(
                    snap.as_ref().map(|s| s.url.as_str()),
                    policy,
                    "back",
                )
                .await?;
                if let Some(ref s) = snap {
                    self.apply_snapshot(s).await;
                }
                self.record_audit("back", "history", true, "Navigated back")
                    .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: snap.map(|s| {
                        format_excerpt_with_captcha(
                            s.text_excerpt,
                            s.captcha_detected,
                            s.captcha_reason.as_deref(),
                        )
                    }),
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::Reload => {
                if let Err(e) = self.cdp.send_command("Page.reload", json!({})).await {
                    self.record_audit("reload", "page", false, e.clone()).await;
                    return Err(e);
                }
                tokio::time::sleep(Duration::from_millis(120)).await;
                let snap = self.capture_dom_snapshot().await.ok();
                self.enforce_current_document_url(
                    snap.as_ref().map(|s| s.url.as_str()),
                    policy,
                    "reload",
                )
                .await?;
                if let Some(ref s) = snap {
                    self.apply_snapshot(s).await;
                }
                self.record_audit("reload", "page", true, "Reloaded page")
                    .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: snap.map(|s| {
                        format_excerpt_with_captcha(
                            s.text_excerpt,
                            s.captcha_detected,
                            s.captcha_reason.as_deref(),
                        )
                    }),
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::PressKey {
                key,
                modifiers,
                ref_id,
            } => {
                let desc = match parse_key_descriptor(&key, &modifiers) {
                    Ok(d) => d,
                    Err(e) => {
                        self.record_audit("press_key", &key, false, e.clone()).await;
                        return Err(e);
                    }
                };
                let target_label = ref_id.as_deref().unwrap_or("active_element").to_string();

                let cached_target = if let Some(ref r) = ref_id {
                    let st = self.state.read().await;
                    let found = st.elements.iter().find(|el| el.wire.ref_id == *r).cloned();
                    if found.is_none() {
                        drop(st);
                        let msg = format!(
                            "Element ref `{r}` not found in latest snapshot; call snapshot first"
                        );
                        self.record_audit("press_key", r, false, msg.clone()).await;
                        return Err(msg);
                    }
                    found
                } else {
                    None
                };

                // Block printable keystrokes or clipboard paste into sensitive password/payment inputs.
                let is_paste_chord = desc.commands.iter().any(|c| c == "paste");
                let guards_sensitive =
                    policy.block_password_inputs && (desc.emits_printable_text() || is_paste_chord);
                if guards_sensitive {
                    if let Some(ref cached) = cached_target
                        && (cached.sensitive
                            || is_sensitive_element_metadata(
                                cached.wire.input_type.as_deref(),
                                &cached.wire.name,
                                &cached.wire.role,
                            ))
                    {
                        let msg = format!(
                            "Refusing to press input key on sensitive password/payment input `{target_label}` (blocked by host browser policy)"
                        );
                        self.record_audit("press_key", &target_label, false, msg.clone())
                            .await;
                        return Err(msg);
                    }
                    match self.inspect_live_sensitive_input(ref_id.as_deref()).await {
                        Ok(Some(reason)) => {
                            let msg = format!(
                                "Refusing to press input key on sensitive input `{target_label}`: {reason}"
                            );
                            self.record_audit("press_key", &target_label, false, msg.clone())
                                .await;
                            return Err(msg);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let msg = format!(
                                "Refusing press_key on `{target_label}` (fail-closed sensitive input check failed: {e})"
                            );
                            self.record_audit("press_key", &target_label, false, msg.clone())
                                .await;
                            return Err(msg);
                        }
                    }
                }

                if let Some(ref cached) = cached_target {
                    let cx = cached.wire.x + (cached.wire.width / 2.0);
                    let cy = cached.wire.y + (cached.wire.height / 2.0);
                    for (ev_type, button, click_count) in [
                        ("mouseMoved", "none", 0),
                        ("mousePressed", "left", 1),
                        ("mouseReleased", "left", 1),
                    ] {
                        let _ = self
                            .cdp
                            .send_command(
                                "Input.dispatchMouseEvent",
                                json!({
                                    "type": ev_type,
                                    "x": cx,
                                    "y": cy,
                                    "button": button,
                                    "clickCount": click_count,
                                }),
                            )
                            .await;
                    }
                    if guards_sensitive {
                        match self.inspect_live_sensitive_input(None).await {
                            Ok(Some(reason)) => {
                                let msg = format!(
                                    "Refusing to press input key on post-focus sensitive input `{target_label}`: {reason}"
                                );
                                self.record_audit("press_key", &target_label, false, msg.clone())
                                    .await;
                                return Err(msg);
                            }
                            Ok(None) => {}
                            Err(e) => {
                                let msg = format!(
                                    "Refusing press_key on `{target_label}` (fail-closed post-focus sensitive input check failed: {e})"
                                );
                                self.record_audit("press_key", &target_label, false, msg.clone())
                                    .await;
                                return Err(msg);
                            }
                        }
                    }
                }

                let mut key_down_params = json!({
                    "type": "rawKeyDown",
                    "key": desc.key,
                    "code": desc.code,
                    "windowsVirtualKeyCode": desc.windows_virtual_key_code,
                    "nativeVirtualKeyCode": desc.windows_virtual_key_code,
                    "modifiers": desc.modifiers_mask,
                });
                if !desc.commands.is_empty()
                    && let Some(obj) = key_down_params.as_object_mut()
                {
                    obj.insert("commands".to_string(), json!(desc.commands));
                }
                if let Err(e) = self
                    .cdp
                    .send_command("Input.dispatchKeyEvent", key_down_params)
                    .await
                {
                    self.record_audit("press_key", &desc.audit_label, false, e.clone())
                        .await;
                    return Err(e);
                }

                if let Some(ref txt) = desc.text
                    && let Err(e) = self
                        .cdp
                        .send_command(
                            "Input.dispatchKeyEvent",
                            json!({
                                "type": "char",
                                "text": txt,
                                "unmodifiedText": txt,
                                "key": desc.key,
                                "code": desc.code,
                                "windowsVirtualKeyCode": desc.windows_virtual_key_code,
                                "nativeVirtualKeyCode": desc.windows_virtual_key_code,
                                "modifiers": desc.modifiers_mask,
                            }),
                        )
                        .await
                {
                    self.record_audit("press_key", &desc.audit_label, false, e.clone())
                        .await;
                    return Err(e);
                }

                if let Err(e) = self
                    .cdp
                    .send_command(
                        "Input.dispatchKeyEvent",
                        json!({
                            "type": "keyUp",
                            "key": desc.key,
                            "code": desc.code,
                            "windowsVirtualKeyCode": desc.windows_virtual_key_code,
                            "nativeVirtualKeyCode": desc.windows_virtual_key_code,
                            "modifiers": desc.modifiers_mask,
                        }),
                    )
                    .await
                {
                    self.record_audit("press_key", &desc.audit_label, false, e.clone())
                        .await;
                    return Err(e);
                }

                self.record_audit(
                    "press_key",
                    &target_label,
                    true,
                    format!("Dispatched key {}", desc.audit_label),
                )
                .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: None,
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::SelectOption {
                ref_id,
                selector,
                value,
                label,
                index,
            } => {
                let target_label = ref_id
                    .as_deref()
                    .or(selector.as_deref())
                    .unwrap_or("missing")
                    .to_string();
                if ref_id.as_deref().map(str::trim).unwrap_or("").is_empty()
                    && selector.as_deref().map(str::trim).unwrap_or("").is_empty()
                {
                    let msg = "SelectOption requires either `ref_id` or `selector`".to_string();
                    self.record_audit("select_option", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }
                if value.is_none() && label.is_none() && index.is_none() {
                    let msg = "SelectOption requires at least one of `value`, `label`, or `index`"
                        .to_string();
                    self.record_audit("select_option", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }

                let expr = build_select_option_js(
                    ref_id.as_deref(),
                    selector.as_deref(),
                    value.as_deref(),
                    label.as_deref(),
                    index,
                )?;
                let res = match self
                    .cdp
                    .send_command(
                        "Runtime.evaluate",
                        json!({
                            "expression": expr,
                            "returnByValue": true,
                        }),
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        self.record_audit("select_option", &target_label, false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                if res.get("exceptionDetails").is_some() {
                    let msg = "JavaScript exception while selecting <select> option".to_string();
                    self.record_audit("select_option", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }
                let val = res
                    .get("result")
                    .and_then(|r| r.get("value"))
                    .ok_or_else(|| "SelectOption evaluation returned no value".to_string())?;
                if !val.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    let err_msg = val
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("failed to select option")
                        .to_string();
                    self.record_audit("select_option", &target_label, false, err_msg.clone())
                        .await;
                    return Err(err_msg);
                }
                let selected_val = val
                    .get("selectedValue")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let selected_lbl = val
                    .get("selectedLabel")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                let selected_idx = val
                    .get("selectedIndex")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let detail = format!(
                    "Selected option #{selected_idx} (value={selected_val:?}, label={selected_lbl:?})"
                );
                self.record_audit("select_option", &target_label, true, detail.clone())
                    .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: Some(detail),
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::UploadFile {
                ref_id,
                selector,
                files,
            } => {
                let target_label = ref_id
                    .as_deref()
                    .or(selector.as_deref())
                    .unwrap_or("missing")
                    .to_string();
                if ref_id.as_deref().map(str::trim).unwrap_or("").is_empty()
                    && selector.as_deref().map(str::trim).unwrap_or("").is_empty()
                {
                    let msg = "UploadFile requires either `ref_id` or `selector`".to_string();
                    self.record_audit("upload_file", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }

                // Collect strictly authorized workspace roots (session-bound + daemon-verified).
                let mut allowed_roots: Vec<PathBuf> = Vec::new();
                if let Some(ref root) = self.workspace_root {
                    allowed_roots.push(root.clone());
                }
                for r in extra_allowed_roots {
                    if !allowed_roots.contains(r) {
                        allowed_roots.push(r.clone());
                    }
                }

                let staged_batch = match stage_upload_files_for_session(
                    &files,
                    &allowed_roots,
                    &self.home_dir,
                    &self.session_id,
                )
                .await
                {
                    Ok(batch) => batch,
                    Err(e) => {
                        self.record_audit("upload_file", &target_label, false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                let staged_file_strings: Vec<String> = staged_batch
                    .staged_paths
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();

                // Locate `<input type="file">` as a live CDP RemoteObject (never by value).
                let expr = match build_locate_file_input_js(
                    ref_id.as_deref(),
                    selector.as_deref(),
                    staged_file_strings.len(),
                ) {
                    Ok(expr) => expr,
                    Err(e) => {
                        staged_batch.cleanup_now().await;
                        return Err(e);
                    }
                };
                let eval_res = match self
                    .cdp
                    .send_command(
                        "Runtime.evaluate",
                        json!({
                            "expression": expr,
                            "returnByValue": false,
                        }),
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        staged_batch.cleanup_now().await;
                        self.record_audit("upload_file", &target_label, false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                if let Some(ex) = eval_res.get("exceptionDetails") {
                    staged_batch.cleanup_now().await;
                    let msg = ex
                        .get("exception")
                        .and_then(|e| e.get("description"))
                        .and_then(Value::as_str)
                        .or_else(|| ex.get("text").and_then(Value::as_str))
                        .unwrap_or("failed to locate <input type=\"file\"> element")
                        .to_string();
                    self.record_audit("upload_file", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }
                let object_id = eval_res
                    .get("result")
                    .and_then(|r| r.get("objectId"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                let Some(object_id) = object_id else {
                    staged_batch.cleanup_now().await;
                    let msg = format!(
                        "Target `{target_label}` is not a live <input type=\"file\"> element"
                    );
                    self.record_audit("upload_file", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                };

                // Pass `objectId` directly to `DOM.setFileInputFiles` with the daemon-private
                // staged snapshot paths (zero TOCTOU symlink-swap window, zero redundant
                // `DOM.describeNode` round trip).
                let upload_res = self
                    .cdp
                    .send_command(
                        "DOM.setFileInputFiles",
                        json!({
                            "files": staged_file_strings,
                            "objectId": object_id,
                        }),
                    )
                    .await;
                let _ = self
                    .cdp
                    .send_command("Runtime.releaseObject", json!({ "objectId": object_id }))
                    .await;

                if let Err(e) = upload_res {
                    staged_batch.cleanup_now().await;
                    self.record_audit("upload_file", &target_label, false, e.clone())
                        .await;
                    return Err(e);
                }

                let file_names: Vec<String> = staged_batch
                    .staged_paths
                    .iter()
                    .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
                    .collect();
                let detail = format!(
                    "Attached {} file(s): {}",
                    file_names.len(),
                    file_names.join(", ")
                );
                self.record_audit("upload_file", &target_label, true, detail.clone())
                    .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: Some(detail),
                    captcha_detected,
                    picked_element: None,
                })
            }

            BrowserActionRequest::PickElement {
                x,
                y,
                selector,
                ref_id,
                interactive,
            } => {
                // Enforce document URL policy before extracting DOM metadata.
                let live_href = self.query_live_location_href().await;
                self.enforce_current_document_url(live_href.as_deref(), policy, "pick_element")
                    .await?;

                let target_label = if let (Some(px), Some(py)) = (x, y) {
                    if !px.is_finite()
                        || !py.is_finite()
                        || px < 0.0
                        || py < 0.0
                        || px > 10_000.0
                        || py > 10_000.0
                    {
                        let msg = "PickElement coordinates must be finite values within 0..=10000"
                            .to_string();
                        self.record_audit("pick_element", "coords", false, msg.clone())
                            .await;
                        return Err(msg);
                    }
                    format!("({px:.0}, {py:.0})")
                } else if let Some(ref r) = ref_id
                    && !r.trim().is_empty()
                {
                    r.trim().to_string()
                } else if let Some(ref sel) = selector
                    && !sel.trim().is_empty()
                {
                    sel.trim().to_string()
                } else if interactive {
                    "interactive".to_string()
                } else {
                    let msg = "PickElement requires `(x, y)`, `selector`, `ref_id`, or `interactive: true`".to_string();
                    self.record_audit("pick_element", "missing", false, msg.clone())
                        .await;
                    return Err(msg);
                };

                let use_interactive = interactive
                    && x.is_none()
                    && y.is_none()
                    && selector.as_deref().map(str::trim).unwrap_or("").is_empty()
                    && ref_id.as_deref().map(str::trim).unwrap_or("").is_empty();

                let expr = build_pick_element_js(
                    x,
                    y,
                    selector.as_deref(),
                    ref_id.as_deref(),
                    use_interactive,
                    policy.block_password_inputs,
                )?;
                let res = match self
                    .cdp
                    .send_command(
                        "Runtime.evaluate",
                        json!({
                            "expression": expr,
                            "returnByValue": true,
                            "awaitPromise": use_interactive,
                        }),
                    )
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        self.record_audit("pick_element", &target_label, false, e.clone())
                            .await;
                        return Err(e);
                    }
                };
                if res.get("exceptionDetails").is_some() {
                    let msg = "JavaScript exception while inspecting DOM element".to_string();
                    self.record_audit("pick_element", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }
                let val = res
                    .get("result")
                    .and_then(|r| r.get("value"))
                    .ok_or_else(|| "PickElement returned no value".to_string())?;
                if val
                    .get("cancelled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    let msg = "Element picking cancelled".to_string();
                    self.record_audit("pick_element", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }
                if let Some(err_str) = val.get("error").and_then(Value::as_str)
                    && !err_str.is_empty()
                {
                    let msg = err_str.to_string();
                    self.record_audit("pick_element", &target_label, false, msg.clone())
                        .await;
                    return Err(msg);
                }

                let picked: PickedElementInfo = serde_json::from_value(val.clone())
                    .map_err(|e| format!("failed to decode PickedElementInfo: {e}"))?;
                let sanitized = sanitize_picked_element(picked);
                let detail = format!("Picked <{}> {}", sanitized.tag, sanitized.selector);
                self.record_audit("pick_element", &target_label, true, detail.clone())
                    .await;

                let (elements, captcha_detected) = {
                    let st = self.state.read().await;
                    (
                        st.elements.iter().map(|c| c.wire.clone()).collect(),
                        st.captcha_detected,
                    )
                };
                Ok(BrowserActionOutcome {
                    session: self.summary(false).await,
                    elements,
                    screenshot_base64: None,
                    text_excerpt: Some(detail),
                    captcha_detected,
                    picked_element: Some(sanitized),
                })
            }
        }
    }

    async fn query_live_location_href(&self) -> Option<String> {
        let res = self
            .cdp
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": "window.location.href || ''",
                    "returnByValue": true,
                }),
            )
            .await
            .ok()?;
        res.get("result")
            .and_then(|r| r.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    async fn capture_dom_snapshot(&self) -> Result<RawDomSnapshot, String> {
        let res = self
            .cdp
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": SNAPSHOT_EXTRACTION_JS,
                    "returnByValue": true,
                    "awaitPromise": false,
                }),
            )
            .await?;

        let value = res
            .get("result")
            .and_then(|r| r.get("value"))
            .ok_or_else(|| "CDP Runtime.evaluate snapshot returned no value".to_string())?;

        let parsed: RawDomSnapshotPayload = serde_json::from_value(value.clone())
            .map_err(|e| format!("failed to decode snapshot payload: {e}"))?;

        let elements = parsed
            .elements
            .into_iter()
            .take(MAX_ELEMENTS_PER_SNAPSHOT)
            .map(|item| {
                let sensitive = item.sensitive
                    || is_sensitive_element_metadata(
                        item.input_type.as_deref(),
                        &item.name,
                        &item.role,
                    );
                let captcha_like = item.captcha_like
                    || is_captcha_label(&item.name)
                    || is_captcha_label(&item.role);
                CachedElement {
                    wire: BrowserElementRef {
                        ref_id: item.ref_id,
                        role: item.role,
                        name: truncate_chars(&item.name, 120),
                        tag: item.tag,
                        input_type: item.input_type,
                        x: item.x,
                        y: item.y,
                        width: item.width,
                        height: item.height,
                    },
                    sensitive,
                    captcha_like,
                }
            })
            .collect();

        Ok(RawDomSnapshot {
            url: parsed.url,
            title: truncate_chars(&parsed.title, 200),
            text_excerpt: truncate_chars(&parsed.text_excerpt, MAX_TEXT_EXCERPT_CHARS),
            captcha_detected: parsed.captcha_detected,
            captcha_reason: parsed.captcha_reason,
            elements,
        })
    }

    async fn apply_snapshot(&self, snap: &RawDomSnapshot) {
        let mut st = self.state.write().await;
        if !snap.url.is_empty() {
            st.url = snap.url.clone();
        }
        st.title = snap.title.clone();
        st.elements = snap.elements.clone();
        st.captcha_detected = snap.captcha_detected;
        st.captcha_reason = snap.captcha_reason.clone();
        st.updated_at_ms = now_ms();
    }

    async fn record_captcha_audit(&self, reason: Option<&str>) {
        let detail = match reason.filter(|s| !s.trim().is_empty()) {
            Some(r) => format!(
                "captcha_detected ({r}): Orbit will not attempt to solve or bypass verification; please complete it directly"
            ),
            None => "captcha_detected: Orbit will not attempt to solve or bypass verification; please complete it directly".to_string(),
        };
        self.record_audit("captcha_detected", "page", false, detail)
            .await;
    }

    /// Fail-closed live DOM inspection for password, PIN/OTP, credit-card, Shadow DOM, and
    /// cross-origin iframe inputs. Returns `Err` if inspection fails or returns invalid data.
    async fn inspect_live_sensitive_input(
        &self,
        ref_id: Option<&str>,
    ) -> Result<Option<String>, String> {
        let ref_json = serde_json::to_string(&ref_id).unwrap_or_else(|_| "null".to_string());
        let expr = format!(
            r#"(() => {{
  const refId = {ref_json};
  const findByRef = (root, depth) => {{
    if (!root || depth > 8) return null;
    try {{
      const direct = root.querySelector(`[data-orbit-ref="${{refId}}"]`);
      if (direct) return direct;
      const all = root.querySelectorAll('*');
      for (const n of all) {{
        if (n.shadowRoot) {{
          const hit = findByRef(n.shadowRoot, depth + 1);
          if (hit) return hit;
        }}
        if (n.tagName === 'IFRAME' || n.tagName === 'FRAME') {{
          try {{
            if (n.contentDocument) {{
              const hit = findByRef(n.contentDocument, depth + 1);
              if (hit) return hit;
            }}
          }} catch (_) {{}}
        }}
      }}
    }} catch (_) {{}}
    return null;
  }};

  const resolveDeepActive = (doc, depth) => {{
    if (!doc || depth > 10) return {{ el: null, crossOriginIframe: false }};
    let el = doc.activeElement;
    let steps = 0;
    while (el && steps < 10) {{
      steps += 1;
      if (el.shadowRoot && el.shadowRoot.activeElement) {{
        el = el.shadowRoot.activeElement;
        continue;
      }}
      const tag = (el.tagName || '').toUpperCase();
      if (tag === 'IFRAME' || tag === 'FRAME') {{
        try {{
          const subDoc = el.contentDocument;
          if (!subDoc) {{
            return {{ el: null, crossOriginIframe: true }};
          }}
          if (subDoc.activeElement && subDoc.activeElement !== subDoc.body) {{
            el = subDoc.activeElement;
            continue;
          }}
        }} catch (_) {{
          return {{ el: null, crossOriginIframe: true }};
        }}
      }}
      break;
    }}
    return {{ el, crossOriginIframe: false }};
  }};

  let el = null;
  if (refId) {{
    el = findByRef(document, 0);
  }} else {{
    const resolved = resolveDeepActive(document, 0);
    if (resolved.crossOriginIframe) {{
      return {{ sensitive: true, reason: 'cross-origin iframe (fail-closed payment/auth guard)' }};
    }}
    el = resolved.el;
  }}

  if (!el) return {{ sensitive: false, reason: '' }};
  const tag = (el.tagName || '').toLowerCase();
  if (tag === 'iframe' || tag === 'frame') {{
    try {{
      if (!el.contentDocument) {{
        return {{ sensitive: true, reason: 'cross-origin iframe (fail-closed payment/auth guard)' }};
      }}
    }} catch (_) {{
      return {{ sensitive: true, reason: 'cross-origin iframe (fail-closed payment/auth guard)' }};
    }}
  }}
  const type = (el.getAttribute('type') || el.type || '').toLowerCase();
  const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
  let labelText = '';
  try {{
    if (el.id && el.ownerDocument) {{
      const lbl = el.ownerDocument.querySelector(`label[for="${{CSS.escape(el.id)}}"]`);
      if (lbl && lbl.innerText) labelText = lbl.innerText;
    }}
  }} catch (_) {{}}
  const hint = [
    el.getAttribute('name') || '',
    el.getAttribute('id') || '',
    el.getAttribute('aria-label') || '',
    el.getAttribute('placeholder') || '',
    labelText
  ].join(' ').toLowerCase();
  if (tag === 'input' && type === 'password') {{
    return {{ sensitive: true, reason: 'password input field' }};
  }}
  if (/(^|\s)(current-password|new-password|one-time-code|cc-number|cc-csc|cc-exp|cc-exp-month|cc-exp-year|cc-name)(\s|$)/.test(ac)) {{
    return {{ sensitive: true, reason: `sensitive autocomplete=${{ac}}` }};
  }}
  if (/(password|passcode|\bpin\b|\botp\b|credit.?card|card.?number|cvv|cvc|security.?code|expiration.?date)/.test(hint)) {{
    return {{ sensitive: true, reason: 'password/PIN/OTP/payment field' }};
  }}
  return {{ sensitive: false, reason: '' }};
}})()"#
        );

        let res = self
            .cdp
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": expr,
                    "returnByValue": true,
                }),
            )
            .await
            .map_err(|e| format!("CDP sensitive input evaluation failed: {e}"))?;

        if res.get("exceptionDetails").is_some() {
            return Err("JavaScript exception during sensitive input check".to_string());
        }
        let val = res
            .get("result")
            .and_then(|r| r.get("value"))
            .ok_or_else(|| "sensitive input check returned no value".to_string())?;
        let is_sensitive = val
            .get("sensitive")
            .and_then(Value::as_bool)
            .ok_or_else(|| "sensitive input check returned malformed result".to_string())?;
        if is_sensitive {
            let reason = val
                .get("reason")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("sensitive input field")
                .to_string();
            Ok(Some(reason))
        } else {
            Ok(None)
        }
    }

    /// Fail-closed coordinate hit-test checking whether `(x, y)` lands on a CAPTCHA / anti-bot
    /// widget (descending Shadow DOM and walking up ancestors).
    async fn inspect_captcha_at_point(&self, x: f64, y: f64) -> Result<Option<String>, String> {
        let expr = format!(
            r#"(() => {{
  const selectors = [
    'iframe[src*="recaptcha"]',
    'iframe[src*="hcaptcha"]',
    'iframe[src*="turnstile"]',
    'iframe[src*="challenges.cloudflare.com"]',
    '.g-recaptcha',
    '.h-captcha',
    '.cf-turnstile',
    'altcha-widget',
    '#challenge-running',
    '[data-sitekey]'
  ];
  const labelRe = /(recaptcha|hcaptcha|turnstile|verify you are human|i'm not a robot|im not a robot|altcha|human verification)/i;

  const inspectNode = (node) => {{
    if (!node || node.nodeType !== 1) return null;
    for (const sel of selectors) {{
      try {{
        if (node.matches && node.matches(sel)) return sel;
      }} catch (_) {{}}
    }}
    const hint = [
      node.getAttribute && (node.getAttribute('aria-label') || ''),
      node.getAttribute && (node.getAttribute('title') || ''),
      node.getAttribute && (node.getAttribute('id') || ''),
      node.getAttribute && (node.getAttribute('class') || ''),
      node.getAttribute && (node.getAttribute('src') || ''),
      (node.innerText || '').slice(0, 120)
    ].join(' ');
    if (labelRe.test(hint)) {{
      return 'captcha_label_match';
    }}
    return null;
  }};

  let cur = document.elementFromPoint({x}, {y});
  const visited = [];
  let depth = 0;
  while (cur && depth < 12) {{
    depth += 1;
    visited.push(cur);
    if (cur.shadowRoot && cur.shadowRoot.elementFromPoint) {{
      const inner = cur.shadowRoot.elementFromPoint({x}, {y});
      if (inner && inner !== cur) {{
        cur = inner;
        continue;
      }}
    }}
    break;
  }}

  for (const leaf of visited) {{
    let anc = leaf;
    let hops = 0;
    while (anc && hops < 24) {{
      hops += 1;
      const hit = inspectNode(anc);
      if (hit) return {{ captcha: true, reason: hit }};
      if (anc.parentElement) {{
        anc = anc.parentElement;
      }} else if (anc.getRootNode && anc.getRootNode().host) {{
        anc = anc.getRootNode().host;
      }} else {{
        break;
      }}
    }}
  }}
  return {{ captcha: false, reason: '' }};
}})()"#
        );

        let res = self
            .cdp
            .send_command(
                "Runtime.evaluate",
                json!({
                    "expression": expr,
                    "returnByValue": true,
                }),
            )
            .await
            .map_err(|e| format!("CDP coordinate hit-test failed: {e}"))?;

        if res.get("exceptionDetails").is_some() {
            return Err("JavaScript exception during CAPTCHA coordinate hit-test".to_string());
        }
        let val = res
            .get("result")
            .and_then(|r| r.get("value"))
            .ok_or_else(|| "CAPTCHA coordinate hit-test returned no value".to_string())?;
        let is_captcha = val
            .get("captcha")
            .and_then(Value::as_bool)
            .ok_or_else(|| "CAPTCHA coordinate hit-test returned malformed result".to_string())?;
        if is_captcha {
            let reason = val
                .get("reason")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("captcha widget")
                .to_string();
            Ok(Some(reason))
        } else {
            Ok(None)
        }
    }

    fn upload_staging_dir(&self) -> PathBuf {
        session_upload_staging_dir(&self.home_dir, &self.session_id)
    }

    async fn cleanup_after_disconnect(&self) {
        self.cdp.close().await;
        if let Some(mut proc_handle) = self.process.lock().await.take() {
            proc_handle.terminate_and_cleanup().await;
        }
        let staging_dir = self.upload_staging_dir();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = remove_profile_dir_if_real(&staging_dir);
        })
        .await;
    }

    async fn shutdown(&self) {
        let _ = tokio::time::timeout(
            Duration::from_millis(500),
            self.cdp.send_command("Browser.close", json!({})),
        )
        .await;
        self.cleanup_after_disconnect().await;
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        let _ = remove_profile_dir_if_real(&self.upload_staging_dir());
    }
}

struct RawDomSnapshot {
    url: String,
    title: String,
    text_excerpt: String,
    captcha_detected: bool,
    captcha_reason: Option<String>,
    elements: Vec<CachedElement>,
}

#[derive(Debug, Deserialize)]
struct RawDomSnapshotPayload {
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    text_excerpt: String,
    #[serde(default)]
    captcha_detected: bool,
    #[serde(default)]
    captcha_reason: Option<String>,
    #[serde(default)]
    elements: Vec<RawDomElementPayload>,
}

#[derive(Debug, Deserialize)]
struct RawDomElementPayload {
    ref_id: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    tag: String,
    #[serde(default)]
    input_type: Option<String>,
    #[serde(default)]
    x: f64,
    #[serde(default)]
    y: f64,
    #[serde(default)]
    width: f64,
    #[serde(default)]
    height: f64,
    #[serde(default)]
    sensitive: bool,
    #[serde(default)]
    captcha_like: bool,
}

const SNAPSHOT_EXTRACTION_JS: &str = r#"(() => {
  const MAX_ELS = 80;
  const selector = [
    'a[href]',
    'button',
    'input',
    'textarea',
    'select',
    '[role="button"]',
    '[role="link"]',
    '[role="textbox"]',
    '[role="searchbox"]',
    '[role="checkbox"]',
    '[role="radio"]',
    '[role="tab"]',
    '[role="menuitem"]',
    '[contenteditable="true"]'
  ].join(',');

  const captchaSelectors = [
    'iframe[src*="recaptcha"]',
    'iframe[src*="hcaptcha"]',
    'iframe[src*="turnstile"]',
    'iframe[src*="challenges.cloudflare.com"]',
    '.g-recaptcha',
    '.h-captcha',
    '.cf-turnstile',
    'altcha-widget',
    '#challenge-running',
    '[data-sitekey]'
  ];
  let captchaDetected = false;
  let captchaReason = null;
  for (const sel of captchaSelectors) {
    if (document.querySelector(sel)) {
      captchaDetected = true;
      captchaReason = sel;
      break;
    }
  }

  const labelTextFor = (el) => {
    const aria = el.getAttribute('aria-label');
    if (aria && aria.trim()) return aria.trim();
    if (el.id) {
      const lbl = document.querySelector(`label[for="${CSS.escape(el.id)}"]`);
      if (lbl && lbl.innerText && lbl.innerText.trim()) return lbl.innerText.trim();
    }
    const ph = el.getAttribute('placeholder');
    if (ph && ph.trim()) return ph.trim();
    const txt = (el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim();
    if (txt) return txt;
    const title = el.getAttribute('title') || el.getAttribute('name') || '';
    return title.trim();
  };

  const elements = [];
  const nodes = Array.from(document.querySelectorAll(selector));
  let idx = 0;
  for (const el of nodes) {
    if (elements.length >= MAX_ELS) break;
    const rect = el.getBoundingClientRect();
    if (!rect || rect.width <= 0 || rect.height <= 0) continue;
    const style = window.getComputedStyle(el);
    if (style && (style.visibility === 'hidden' || style.display === 'none')) continue;
    idx += 1;
    const refId = `e${idx}`;
    try { el.setAttribute('data-orbit-ref', refId); } catch (_) {}
    const tag = (el.tagName || '').toLowerCase();
    const inputType = tag === 'input' ? ((el.getAttribute('type') || 'text').toLowerCase()) : null;
    const role = (el.getAttribute('role') || (
      tag === 'a' ? 'link' :
      tag === 'button' ? 'button' :
      tag === 'textarea' ? 'textbox' :
      tag === 'select' ? 'combobox' :
      tag === 'input' ? (inputType === 'checkbox' ? 'checkbox' : inputType === 'radio' ? 'radio' : inputType === 'submit' || inputType === 'button' ? 'button' : 'textbox') :
      'interactive'
    )).toLowerCase();
    const name = labelTextFor(el).slice(0, 120);
    const ac = (el.getAttribute('autocomplete') || '').toLowerCase();
    const hint = `${name} ${el.getAttribute('name') || ''} ${el.id || ''} ${ac}`.toLowerCase();
    const sensitive = inputType === 'password'
      || /(current-password|new-password|one-time-code|password|passcode|\bpin\b|\botp\b|cc-number|cc-csc|cc-exp|credit.?card|card.?number|cvv|cvc)/.test(hint);
    const captchaLike = /(recaptcha|hcaptcha|turnstile|not a robot|verify you are human|altcha)/.test(hint);
    elements.push({
      ref_id: refId,
      role,
      name,
      tag,
      input_type: inputType,
      x: Math.round(rect.left * 10) / 10,
      y: Math.round(rect.top * 10) / 10,
      width: Math.round(rect.width * 10) / 10,
      height: Math.round(rect.height * 10) / 10,
      sensitive,
      captcha_like: captchaLike
    });
  }

  const textExcerpt = ((document.body && document.body.innerText) || '')
    .replace(/\s+\n/g, '\n')
    .trim()
    .slice(0, 4000);

  return {
    url: window.location.href || '',
    title: document.title || '',
    text_excerpt: textExcerpt,
    captcha_detected: captchaDetected,
    captcha_reason: captchaReason,
    elements
  };
})()"#;

fn format_excerpt_with_captcha(
    excerpt: String,
    captcha_detected: bool,
    captcha_reason: Option<&str>,
) -> String {
    if !captcha_detected {
        return excerpt;
    }
    let notice = match captcha_reason.filter(|s| !s.trim().is_empty()) {
        Some(r) => format!(
            "[captcha_detected: {r} — Orbit will not attempt to solve or bypass this verification; please complete it directly]"
        ),
        None => "[captcha_detected — Orbit will not attempt to solve or bypass this verification; please complete it directly]".to_string(),
    };
    if excerpt.is_empty() {
        notice
    } else {
        format!("{notice}\n\n{excerpt}")
    }
}

fn is_sensitive_element_metadata(input_type: Option<&str>, name: &str, role: &str) -> bool {
    if input_type.is_some_and(|t| t.eq_ignore_ascii_case("password")) {
        return true;
    }
    let combined = format!("{name} {role}").to_ascii_lowercase();
    const PATTERNS: &[&str] = &[
        "password",
        "passcode",
        "one-time-code",
        "credit card",
        "credit-card",
        "card number",
        "card-number",
        "cc-number",
        "cc-csc",
        "cc-exp",
        "cvv",
        "cvc",
        "security code",
    ];
    if PATTERNS.iter().any(|p| combined.contains(p)) {
        return true;
    }
    combined
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| matches!(tok, "pin" | "otp"))
}

fn is_captcha_label(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    const PATTERNS: &[&str] = &[
        "recaptcha",
        "hcaptcha",
        "turnstile",
        "verify you are human",
        "i'm not a robot",
        "im not a robot",
        "altcha",
        "human verification",
    ];
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Strictly validate that a CDP WebSocket URL points ONLY to loopback (`127.0.0.1` / `localhost` / `[::1]`)
/// with an explicit port and no embedded credentials.
pub fn validate_cdp_ws_url(raw: &str) -> Result<reqwest::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_URL_LEN {
        return Err("Invalid CDP WebSocket URL".to_string());
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid CDP WebSocket URL: {e}"))?;
    if !matches!(parsed.scheme(), "ws" | "wss") {
        return Err(format!(
            "Refusing non-WebSocket CDP URL scheme `{}`",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Refusing CDP WebSocket URL with embedded credentials".to_string());
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "CDP WebSocket URL missing host".to_string())?;
    let host = normalize_host(host_raw);
    let is_loopback = host == "127.0.0.1" || host == "localhost" || host == "::1";
    if !is_loopback {
        return Err(format!(
            "Refusing non-loopback CDP WebSocket host `{host}`; CDP must stay on 127.0.0.1/localhost"
        ));
    }
    if parsed.port().unwrap_or(0) == 0 {
        return Err("CDP WebSocket URL must specify an explicit port".to_string());
    }
    Ok(parsed)
}

/// Synchronous URL navigation guard (scheme, embedded credentials, metadata hostnames, IP literals,
/// loopback policy, and `blocked_domains` / `allowed_domains`).
pub fn validate_navigation_url(
    raw: &str,
    policy: &BrowserPolicyConfig,
) -> Result<reqwest::Url, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Navigation URL cannot be empty".to_string());
    }
    if trimmed.len() > MAX_URL_LEN {
        return Err(format!("Navigation URL exceeds {MAX_URL_LEN} characters"));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Navigation URL contains invalid whitespace or control characters".to_string());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "Unsupported URL scheme `{}` (only http:// and https:// are allowed)",
            parsed.scheme()
        ));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Refusing URL with embedded credentials".to_string());
    }

    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Navigation URL is missing a host".to_string())?;
    let host = normalize_host(host_raw);
    if host.is_empty() {
        return Err("Navigation URL has an empty host".to_string());
    }

    if is_cloud_metadata_hostname(&host) {
        return Err(format!(
            "Refusing navigation to cloud metadata host `{host}`"
        ));
    }

    let is_localhost_name = host == "localhost" || host.ends_with(".localhost");
    if let Ok(ip) = host.parse::<IpAddr>() {
        check_resolved_ip_against_policy(ip, policy)?;
    } else if is_localhost_name && !policy.allow_localhost {
        return Err("Navigation to localhost is disabled by host browser policy".to_string());
    }

    check_domain_lists(&host, policy)?;
    Ok(parsed)
}

/// Async URL navigation guard adding DNS-pinned SSRF resolution for non-IP hostnames.
pub async fn validate_navigation_url_async(
    raw: &str,
    policy: &BrowserPolicyConfig,
) -> Result<reqwest::Url, String> {
    validate_navigation_url_with_resolver(raw, policy, |host, port| async move {
        let lookup = tokio::time::timeout(
            DNS_LOOKUP_TIMEOUT,
            tokio::net::lookup_host((host.as_str(), port)),
        )
        .await
        .map_err(|_| format!("DNS lookup timed out for `{host}`"))?
        .map_err(|e| format!("DNS lookup failed for `{host}`: {e}"))?;
        Ok(lookup.map(|addr| addr.ip()).collect::<Vec<_>>())
    })
    .await
}

/// Testable DNS-pinned URL guard accepting a custom async resolver.
pub async fn validate_navigation_url_with_resolver<F, Fut>(
    raw: &str,
    policy: &BrowserPolicyConfig,
    resolver: F,
) -> Result<reqwest::Url, String>
where
    F: FnOnce(String, u16) -> Fut,
    Fut: Future<Output = Result<Vec<IpAddr>, String>>,
{
    let parsed = validate_navigation_url(raw, policy)?;
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Navigation URL is missing a host".to_string())?;
    let host = normalize_host(host_raw);

    // Literal IP addresses and explicit `localhost` names are already validated synchronously.
    if host.parse::<IpAddr>().is_ok() || host == "localhost" || host.ends_with(".localhost") {
        return Ok(parsed);
    }

    let port = parsed.port_or_known_default().unwrap_or(80);
    let ips = resolver(host.clone(), port).await?;
    if ips.is_empty() {
        return Err(format!("DNS lookup returned no addresses for `{host}`"));
    }
    for ip in ips {
        check_resolved_ip_against_policy(ip, policy)?;
    }
    Ok(parsed)
}

fn check_resolved_ip_against_policy(
    ip: IpAddr,
    policy: &BrowserPolicyConfig,
) -> Result<(), String> {
    if is_blocked_browser_ssrf_ip(ip) {
        return Err(format!(
            "Refusing navigation to link-local, metadata, or unspecified IP `{ip}`"
        ));
    }
    if is_loopback_ip(ip) && !policy.allow_localhost {
        return Err(format!(
            "Navigation to loopback IP `{ip}` is disabled by host browser policy"
        ));
    }
    Ok(())
}

fn check_domain_lists(host: &str, policy: &BrowserPolicyConfig) -> Result<(), String> {
    for blocked in &policy.blocked_domains {
        if domain_matches(host, blocked) {
            return Err(format!(
                "Navigation to `{host}` is blocked by host browser policy (`{blocked}`)"
            ));
        }
    }
    if !policy.allowed_domains.is_empty()
        && !policy
            .allowed_domains
            .iter()
            .any(|allowed| domain_matches(host, allowed))
    {
        return Err(format!(
            "Navigation to `{host}` is not permitted by `allowed_domains` policy"
        ));
    }
    Ok(())
}

fn domain_matches(host: &str, raw_pattern: &str) -> bool {
    let pattern = raw_pattern
        .trim()
        .trim_start_matches("*.")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if pattern.is_empty() {
        return false;
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

fn normalize_host(host_raw: &str) -> String {
    let trimmed = host_raw.trim().trim_end_matches('.');
    trimmed
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(trimmed)
        .to_ascii_lowercase()
}

fn is_cloud_metadata_hostname(host: &str) -> bool {
    matches!(
        host,
        "metadata.google.internal" | "metadata.goog" | "metadata" | "instance-data"
    )
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            if v6.is_loopback() {
                return true;
            }
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_loopback();
            }
            // Deprecated IPv4-compatible IPv6 addresses (e.g. `::127.0.0.1`).
            if let Some(compat) = v6.to_ipv4()
                && compat.is_loopback()
            {
                return true;
            }
            let seg = v6.segments();
            if seg[0] == 0x0064
                && seg[1] == 0xff9b
                && seg[2] == 0
                && seg[3] == 0
                && seg[4] == 0
                && seg[5] == 0
            {
                let nat64_v4 = Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    (seg[6] & 0xff) as u8,
                    (seg[7] >> 8) as u8,
                    (seg[7] & 0xff) as u8,
                );
                return nat64_v4.is_loopback();
            }
            false
        }
    }
}

/// Returns `true` for link-local, cloud metadata, unspecified, broadcast, or multicast IPs
/// (including IPv4-mapped, IPv4-compatible, and NAT64 `64:ff9b::/96` encodings).
pub fn is_blocked_browser_ssrf_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let oct = v4.octets();
            v4.is_unspecified()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                // Alibaba Cloud metadata endpoint
                || oct == [100, 100, 100, 200]
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_browser_ssrf_ip(IpAddr::V4(mapped));
            }
            // Deprecated IPv4-compatible IPv6 addresses (e.g. ::169.254.169.254), excluding ::1.
            if !v6.is_loopback()
                && let Some(compat_v4) = v6.to_ipv4()
                && is_blocked_browser_ssrf_ip(IpAddr::V4(compat_v4))
            {
                return true;
            }
            let seg = v6.segments();
            // NAT64 well-known prefix 64:ff9b::/96 wrapping an IPv4 address.
            if seg[0] == 0x0064
                && seg[1] == 0xff9b
                && seg[2] == 0
                && seg[3] == 0
                && seg[4] == 0
                && seg[5] == 0
            {
                let nat64_v4 = Ipv4Addr::new(
                    (seg[6] >> 8) as u8,
                    (seg[6] & 0xff) as u8,
                    (seg[7] >> 8) as u8,
                    (seg[7] & 0xff) as u8,
                );
                if is_blocked_browser_ssrf_ip(IpAddr::V4(nat64_v4)) {
                    return true;
                }
            }
            v6.is_unspecified()
                || v6.is_multicast()
                // Link-local fe80::/10
                || (seg[0] & 0xffc0) == 0xfe80
                // AWS EC2 IMDSv2 IPv6 endpoint fd00:ec2::254
                || (seg[0] == 0xfd00
                    && seg[1] == 0x0ec2
                    && seg[2] == 0
                    && seg[3] == 0
                    && seg[4] == 0
                    && seg[5] == 0
                    && seg[6] == 0
                    && seg[7] == 0x0254)
        }
    }
}

/// Validate that a user-supplied `ORBIT_CHROME_BIN` override is an absolute, executable file path.
pub fn is_valid_chrome_override_path(path: &Path) -> bool {
    if !path.is_absolute() {
        return false;
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        (meta.permissions().mode() & 0o111) != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Auto-discover an installed Chromium-based browser binary (Chrome, Chromium, Brave, Edge).
pub fn discover_chrome_binary() -> Option<String> {
    if let Ok(override_bin) = std::env::var("ORBIT_CHROME_BIN") {
        let trimmed = override_bin.trim();
        let path = Path::new(trimmed);
        if !trimmed.is_empty() && is_valid_chrome_override_path(path) {
            return Some(trimmed.to_string());
        }
    }

    let mut fixed_candidates: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        fixed_candidates.extend([
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
            PathBuf::from("/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"),
            PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        ]);
        if let Ok(home) = std::env::var("HOME") {
            let h = PathBuf::from(home);
            fixed_candidates
                .push(h.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"));
            fixed_candidates.push(h.join("Applications/Chromium.app/Contents/MacOS/Chromium"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        fixed_candidates.extend([
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/google-chrome-stable"),
            PathBuf::from("/usr/bin/chromium"),
            PathBuf::from("/usr/bin/chromium-browser"),
            PathBuf::from("/snap/bin/chromium"),
            PathBuf::from("/usr/bin/brave-browser"),
            PathBuf::from("/usr/bin/microsoft-edge"),
        ]);
    }

    #[cfg(target_os = "windows")]
    {
        for env_key in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
            if let Ok(base) = std::env::var(env_key) {
                let root = PathBuf::from(base);
                fixed_candidates.push(root.join("Google\\Chrome\\Application\\chrome.exe"));
                fixed_candidates.push(root.join("Microsoft\\Edge\\Application\\msedge.exe"));
                fixed_candidates
                    .push(root.join("BraveSoftware\\Brave-Browser\\Application\\brave.exe"));
            }
        }
    }

    for candidate in fixed_candidates {
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }

    #[cfg(not(target_os = "windows"))]
    let path_names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "microsoft-edge",
    ];
    #[cfg(target_os = "windows")]
    let path_names = ["chrome.exe", "msedge.exe", "brave.exe"];

    if let Some(path_env) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_env) {
            for bin in path_names {
                let candidate = dir.join(bin);
                if candidate.is_file() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Load `$ORBIT_HOME/browser-policy.json`, falling back to safe defaults if missing or invalid.
pub fn load_browser_policy(home_dir: &Path) -> BrowserPolicyConfig {
    let path = home_dir.join(BROWSER_POLICY_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return BrowserPolicyConfig::default();
    };
    serde_json::from_str::<BrowserPolicyConfig>(&raw)
        .map(BrowserPolicyConfig::sanitized)
        .unwrap_or_default()
}

/// Atomically persist `$ORBIT_HOME/browser-policy.json` with `0o600` permissions on Unix.
pub fn save_browser_policy_atomic(
    home_dir: &Path,
    policy: &BrowserPolicyConfig,
) -> Result<(), String> {
    use std::io::Write;

    std::fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create orbit home dir: {e}"))?;
    let sanitized = policy.clone().sanitized();
    let payload = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| format!("failed to serialize browser policy: {e}"))?;

    let final_path = home_dir.join(BROWSER_POLICY_FILE);
    let tmp_path = home_dir.join(format!(
        ".{BROWSER_POLICY_FILE}.tmp-{}",
        Uuid::new_v4().simple()
    ));

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let mut file = opts
        .open(&tmp_path)
        .map_err(|e| format!("failed to open temp browser policy file: {e}"))?;
    if let Err(e) = file
        .write_all(payload.as_bytes())
        .and_then(|()| file.sync_all())
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(format!("failed to write browser policy: {e}"));
    }
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("failed to chmod browser policy: {e}"))?;
    }

    std::fs::rename(&tmp_path, &final_path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp_path);
        format!("failed to atomically replace browser policy: {e}")
    })?;
    Ok(())
}

fn push_bounded<T>(buf: &mut VecDeque<T>, item: T, max_len: usize) {
    buf.push_back(item);
    while buf.len() > max_len {
        buf.pop_front();
    }
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

const MAX_UPLOAD_FILES_PER_ACTION: usize = 32;

const CDP_MODIFIER_ALT: u32 = 1;
const CDP_MODIFIER_CTRL: u32 = 2;
const CDP_MODIFIER_META: u32 = 4;
const CDP_MODIFIER_SHIFT: u32 = 8;

/// Resolved CDP keyboard event descriptor parsed from a key name or modifier chord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDescriptor {
    pub key: String,
    pub code: String,
    pub windows_virtual_key_code: u32,
    pub modifiers_mask: u32,
    pub text: Option<String>,
    pub commands: Vec<String>,
    pub audit_label: String,
}

impl KeyDescriptor {
    pub fn emits_printable_text(&self) -> bool {
        self.text.as_deref().is_some_and(|t| t != "\r")
    }
}

fn modifier_bit_for_token(token: &str) -> Option<u32> {
    match token.trim().to_ascii_lowercase().as_str() {
        "alt" | "option" | "opt" => Some(CDP_MODIFIER_ALT),
        "control" | "ctrl" => Some(CDP_MODIFIER_CTRL),
        "meta" | "cmd" | "command" | "super" | "win" => Some(CDP_MODIFIER_META),
        "shift" => Some(CDP_MODIFIER_SHIFT),
        _ => None,
    }
}

/// Split a chord like `"Meta+Shift+A"` or `"Control++"` into `(modifier_tokens, main_key)`.
fn split_key_chord(raw: &str) -> (Vec<&str>, &str) {
    if raw == "+" || raw == " " {
        return (Vec::new(), raw);
    }
    // Preserve trailing literal `+` when expressing chords like `Meta++` or `Control++`.
    if let Some(prefix) = raw.strip_suffix("++") {
        let mods = prefix
            .split('+')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        return (mods, "+");
    }
    let parts: Vec<&str> = raw.split('+').collect();
    if parts.len() <= 1 {
        return (Vec::new(), raw);
    }
    let last_idx = parts.len() - 1;
    let main_key = parts[last_idx];
    let mods = parts[..last_idx]
        .iter()
        .copied()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    (mods, main_key)
}

/// Parse a key name or chord (e.g. `"Enter"`, `"Escape"`, `"Tab"`, `"ArrowDown"`, `"Space"`,
/// `"Meta+A"`, `"Control+C"`) plus optional explicit modifier names into a CDP [`KeyDescriptor`].
pub fn parse_key_descriptor(
    raw_key: &str,
    extra_modifiers: &[String],
) -> Result<KeyDescriptor, String> {
    if raw_key.is_empty() || raw_key.len() > 64 {
        return Err("Key descriptor must be between 1 and 64 characters".to_string());
    }
    if raw_key.chars().any(|c| c.is_control()) {
        return Err("Key descriptor must not contain control characters".to_string());
    }

    let mut modifiers_mask = 0u32;
    for m in extra_modifiers {
        let Some(bit) = modifier_bit_for_token(m) else {
            return Err(format!("Unsupported keyboard modifier `{m}`"));
        };
        modifiers_mask |= bit;
    }

    let (chord_mods, raw_main) = split_key_chord(raw_key);
    for m in chord_mods {
        let Some(bit) = modifier_bit_for_token(m) else {
            return Err(format!(
                "Unsupported modifier `{m}` in key chord `{raw_key}`"
            ));
        };
        modifiers_mask |= bit;
    }

    let main_trimmed = if raw_main == " " {
        " "
    } else {
        raw_main.trim()
    };
    if main_trimmed.is_empty() {
        return Err(format!("Missing primary key in `{raw_key}`"));
    }

    let has_cmd_mod =
        (modifiers_mask & (CDP_MODIFIER_ALT | CDP_MODIFIER_CTRL | CDP_MODIFIER_META)) != 0;
    let has_shift = (modifiers_mask & CDP_MODIFIER_SHIFT) != 0;

    let (key, code, vk, text) = match main_trimmed.to_ascii_lowercase().as_str() {
        "enter" | "return" => (
            "Enter".to_string(),
            "Enter".to_string(),
            13u32,
            (!has_cmd_mod).then(|| "\r".to_string()),
        ),
        "escape" | "esc" => ("Escape".to_string(), "Escape".to_string(), 27, None),
        "tab" => ("Tab".to_string(), "Tab".to_string(), 9, None),
        "backspace" => ("Backspace".to_string(), "Backspace".to_string(), 8, None),
        "delete" | "del" => ("Delete".to_string(), "Delete".to_string(), 46, None),
        "space" | "spacebar" | " " => (
            " ".to_string(),
            "Space".to_string(),
            32,
            (!has_cmd_mod).then(|| " ".to_string()),
        ),
        "arrowup" | "up" => ("ArrowUp".to_string(), "ArrowUp".to_string(), 38, None),
        "arrowdown" | "down" => ("ArrowDown".to_string(), "ArrowDown".to_string(), 40, None),
        "arrowleft" | "left" => ("ArrowLeft".to_string(), "ArrowLeft".to_string(), 37, None),
        "arrowright" | "right" => ("ArrowRight".to_string(), "ArrowRight".to_string(), 39, None),
        "home" => ("Home".to_string(), "Home".to_string(), 36, None),
        "end" => ("End".to_string(), "End".to_string(), 35, None),
        "pageup" => ("PageUp".to_string(), "PageUp".to_string(), 33, None),
        "pagedown" => ("PageDown".to_string(), "PageDown".to_string(), 34, None),
        "insert" => ("Insert".to_string(), "Insert".to_string(), 45, None),
        "f1" => ("F1".to_string(), "F1".to_string(), 112, None),
        "f2" => ("F2".to_string(), "F2".to_string(), 113, None),
        "f3" => ("F3".to_string(), "F3".to_string(), 114, None),
        "f4" => ("F4".to_string(), "F4".to_string(), 115, None),
        "f5" => ("F5".to_string(), "F5".to_string(), 116, None),
        "f6" => ("F6".to_string(), "F6".to_string(), 117, None),
        "f7" => ("F7".to_string(), "F7".to_string(), 118, None),
        "f8" => ("F8".to_string(), "F8".to_string(), 119, None),
        "f9" => ("F9".to_string(), "F9".to_string(), 120, None),
        "f10" => ("F10".to_string(), "F10".to_string(), 121, None),
        "f11" => ("F11".to_string(), "F11".to_string(), 122, None),
        "f12" => ("F12".to_string(), "F12".to_string(), 123, None),
        _ => {
            let mut chars = main_trimmed.chars();
            let Some(ch) = chars.next() else {
                return Err(format!("Unsupported key `{raw_key}`"));
            };
            if chars.next().is_some() {
                return Err(format!("Unsupported key `{main_trimmed}` in `{raw_key}`"));
            }
            if ch.is_ascii_alphabetic() {
                let upper = ch.to_ascii_uppercase();
                let rendered = if has_shift || (ch.is_ascii_uppercase() && !has_cmd_mod) {
                    if ch.is_ascii_uppercase() && !has_cmd_mod {
                        modifiers_mask |= CDP_MODIFIER_SHIFT;
                    }
                    upper.to_string()
                } else {
                    ch.to_ascii_lowercase().to_string()
                };
                let text = (!has_cmd_mod).then(|| rendered.clone());
                (
                    rendered,
                    format!("Key{upper}"),
                    u32::from(upper as u8),
                    text,
                )
            } else if ch.is_ascii_digit() {
                let s = ch.to_string();
                let text = (!has_cmd_mod).then(|| s.clone());
                (s, format!("Digit{ch}"), u32::from(ch as u8), text)
            } else {
                let (code, vk) = match ch {
                    '-' | '_' => ("Minus", 189u32),
                    '=' | '+' => ("Equal", 187),
                    '[' | '{' => ("BracketLeft", 219),
                    ']' | '}' => ("BracketRight", 221),
                    '\\' | '|' => ("Backslash", 220),
                    ';' | ':' => ("Semicolon", 186),
                    '\'' | '"' => ("Quote", 222),
                    ',' | '<' => ("Comma", 188),
                    '.' | '>' => ("Period", 190),
                    '/' | '?' => ("Slash", 191),
                    '`' | '~' => ("Backquote", 192),
                    _ => ("Unidentified", 0),
                };
                let s = ch.to_string();
                let text = (!has_cmd_mod).then(|| s.clone());
                (s, code.to_string(), vk, text)
            }
        }
    };

    // Attach Chromium editing commands for common Meta/Control chords so headless Chrome executes
    // them consistently across macOS and Linux.
    let mut commands = Vec::new();
    let is_primary_shortcut_mod = (modifiers_mask & (CDP_MODIFIER_META | CDP_MODIFIER_CTRL)) != 0
        && (modifiers_mask & CDP_MODIFIER_ALT) == 0;
    if is_primary_shortcut_mod {
        match key.to_ascii_lowercase().as_str() {
            "a" if !has_shift => commands.push("selectAll".to_string()),
            "c" if !has_shift => commands.push("copy".to_string()),
            "v" if !has_shift => commands.push("paste".to_string()),
            "x" if !has_shift => commands.push("cut".to_string()),
            "z" if !has_shift => commands.push("undo".to_string()),
            "z" if has_shift => commands.push("redo".to_string()),
            "y" if !has_shift => commands.push("redo".to_string()),
            _ => {}
        }
    }

    let audit_label =
        if text.as_deref().is_some_and(|t| t != "\r" && t != " ") && modifiers_mask == 0 {
            "[char]".to_string()
        } else {
            let mut parts: Vec<&str> = Vec::new();
            if (modifiers_mask & CDP_MODIFIER_META) != 0 {
                parts.push("Meta");
            }
            if (modifiers_mask & CDP_MODIFIER_CTRL) != 0 {
                parts.push("Control");
            }
            if (modifiers_mask & CDP_MODIFIER_ALT) != 0 {
                parts.push("Alt");
            }
            if (modifiers_mask & CDP_MODIFIER_SHIFT) != 0 {
                parts.push("Shift");
            }
            let display_key = if key == " " { "Space" } else { key.as_str() };
            parts.push(display_key);
            parts.join("+")
        };

    Ok(KeyDescriptor {
        key,
        code,
        windows_virtual_key_code: vk,
        modifiers_mask,
        text,
        commands,
        audit_label,
    })
}

/// Sensitive dot-directories and credential filenames strictly forbidden from browser file uploads,
/// even if a broad directory such as `$HOME` were configured as a workspace root.
fn is_forbidden_upload_component(component: &str) -> bool {
    let lower = component.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        ".ssh"
            | ".gnupg"
            | ".gpg"
            | ".aws"
            | ".kube"
            | ".docker"
            | ".azure"
            | ".gcloud"
            | ".orbit"
            | ".git"
            | ".netrc"
            | ".npmrc"
            | ".pypirc"
            | ".git-credentials"
            | "authorized_keys"
            | "known_hosts"
            | "id_rsa"
            | "id_ed25519"
            | "id_ecdsa"
            | "id_dsa"
    )
}

/// Canonicalize and strictly confine all `UploadFile` paths to `allowed_roots`.
///
/// Security invariants:
/// - Fail-closed when `allowed_roots` is empty or cannot be canonicalized.
/// - Rejects lexical `..` traversal components, NUL/control bytes, and non-regular files.
/// - Canonicalizes every path via `std::fs::canonicalize` so symlinks escaping `allowed_roots`
///   are ALWAYS rejected.
/// - Rejects any path inside `$ORBIT_HOME` or sensitive dot-directories/credential files
///   (`~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.kube`, `~/.docker`, `.git`, etc.).
pub fn validate_upload_file_paths(
    raw_paths: &[String],
    allowed_roots: &[PathBuf],
    orbit_home: &Path,
) -> Result<Vec<PathBuf>, String> {
    if raw_paths.is_empty() {
        return Err("UploadFile requires at least one file path".to_string());
    }
    if raw_paths.len() > MAX_UPLOAD_FILES_PER_ACTION {
        return Err(format!(
            "UploadFile exceeds maximum of {MAX_UPLOAD_FILES_PER_ACTION} files per action"
        ));
    }

    let canon_roots: Vec<PathBuf> = allowed_roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .filter(|r| r.is_dir())
        .collect();
    if canon_roots.is_empty() {
        return Err(
            "UploadFile refused: no valid workspace root is bound to this browser session"
                .to_string(),
        );
    }

    let canon_orbit_home = std::fs::canonicalize(orbit_home).ok();
    let mut blocked_home_prefixes: Vec<PathBuf> = Vec::new();
    if let Some(home_dir) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        && let Ok(canon_home) = std::fs::canonicalize(home_dir)
    {
        for rel in [
            ".ssh",
            ".gnupg",
            ".aws",
            ".kube",
            ".docker",
            ".azure",
            ".gcloud",
            ".orbit",
            ".config/gh",
        ] {
            let candidate = canon_home.join(rel);
            blocked_home_prefixes.push(std::fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }

    let mut validated = Vec::with_capacity(raw_paths.len());
    for raw in raw_paths {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.contains('\0') || trimmed.chars().any(|c| c.is_control()) {
            return Err(
                "UploadFile path is empty or contains invalid control characters".to_string(),
            );
        }
        let raw_path = Path::new(trimmed);
        if raw_path
            .components()
            .any(|c| matches!(c, Component::ParentDir))
        {
            return Err(format!(
                "UploadFile refused path `{trimmed}`: parent directory traversal (`..`) is forbidden"
            ));
        }

        // Resolve relative paths strictly against allowed workspace roots.
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            let mut found = None;
            for root in &canon_roots {
                let joined = root.join(raw_path);
                if joined.exists() {
                    found = Some(joined);
                    break;
                }
            }
            match found {
                Some(p) => p,
                None => {
                    return Err(format!(
                        "UploadFile path `{trimmed}` was not found inside workspace root"
                    ));
                }
            }
        };

        let canon_file = std::fs::canonicalize(&candidate).map_err(|e| {
            format!("UploadFile refused path `{trimmed}`: cannot resolve canonical path ({e})")
        })?;
        let meta = std::fs::metadata(&canon_file)
            .map_err(|e| format!("UploadFile refused path `{trimmed}`: cannot stat file ({e})"))?;
        if !meta.is_file() {
            return Err(format!(
                "UploadFile refused path `{trimmed}`: target must be a regular file"
            ));
        }

        if !canon_roots.iter().any(|root| canon_file.starts_with(root)) {
            return Err(format!(
                "UploadFile refused path `{trimmed}`: resolved path escapes allowed workspace root"
            ));
        }

        if let Some(ref oh) = canon_orbit_home
            && canon_file.starts_with(oh)
        {
            return Err(format!(
                "UploadFile refused path `{trimmed}`: uploading from $ORBIT_HOME is forbidden"
            ));
        }

        if blocked_home_prefixes
            .iter()
            .any(|prefix| canon_file.starts_with(prefix))
        {
            return Err(format!(
                "UploadFile refused path `{trimmed}`: uploading from sensitive user directory is forbidden"
            ));
        }

        for comp in canon_file.components() {
            if let Component::Normal(os_name) = comp
                && let Some(name) = os_name.to_str()
                && is_forbidden_upload_component(name)
            {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: sensitive directory or credential file `{name}` is forbidden"
                ));
            }
        }

        validated.push(canon_file);
    }

    Ok(validated)
}

fn session_upload_staging_dir(home_dir: &Path, session_id: &str) -> PathBuf {
    let safe_sid: String = session_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .collect();
    let dir_name = if safe_sid.is_empty() {
        "session".to_string()
    } else {
        safe_sid
    };
    home_dir
        .join(BROWSER_UPLOAD_RUNTIME_DIR)
        .join(BROWSER_UPLOAD_STAGING_DIR)
        .join(dir_name)
}

fn ensure_private_dir_0700(dir: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(format!(
                    "refusing symlink or non-directory upload staging path `{}`",
                    dir.display()
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(dir).map_err(|err| {
                format!(
                    "failed to create upload staging dir `{}`: {err}",
                    dir.display()
                )
            })?;
        }
        Err(e) => {
            return Err(format!(
                "failed to inspect upload staging dir `{}`: {e}",
                dir.display()
            ));
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(dir).map_err(|e| {
            format!(
                "failed to verify upload staging dir `{}`: {e}",
                dir.display()
            )
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(format!(
                "refusing to chmod symlink upload staging path `{}`",
                dir.display()
            ));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("failed to set 0700 permissions on `{}`: {e}", dir.display()))?;
    }
    Ok(())
}

fn create_upload_nonce_dir(home_dir: &Path, session_id: &str) -> Result<PathBuf, String> {
    if session_id.is_empty()
        || !session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err("UploadFile refused: invalid browser session id".to_string());
    }
    std::fs::create_dir_all(home_dir)
        .map_err(|e| format!("failed to create orbit home dir: {e}"))?;
    let runtime_dir = home_dir.join(BROWSER_UPLOAD_RUNTIME_DIR);
    ensure_private_dir_0700(&runtime_dir)?;
    let uploads_root = runtime_dir.join(BROWSER_UPLOAD_STAGING_DIR);
    ensure_private_dir_0700(&uploads_root)?;
    let session_dir = uploads_root.join(session_id);
    ensure_private_dir_0700(&session_dir)?;

    let nonce_dir = session_dir.join(format!("u-{}", Uuid::new_v4().simple()));
    std::fs::create_dir(&nonce_dir)
        .map_err(|e| format!("failed to create upload nonce dir: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(&nonce_dir)
            .map_err(|e| format!("failed to verify upload nonce dir: {e}"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("refusing symlink upload nonce dir".to_string());
        }
        std::fs::set_permissions(&nonce_dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("failed to chmod upload nonce dir: {e}"))?;
    }
    Ok(nonce_dir)
}

struct StagedUploadBatch {
    nonce_dir: PathBuf,
    staged_paths: Vec<PathBuf>,
}

impl StagedUploadBatch {
    async fn cleanup_now(&self) {
        let dir = self.nonce_dir.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let _ = remove_profile_dir_if_real(&dir);
        })
        .await;
    }
}

/// Validate workspace confinement, open each regular file with `O_NOFOLLOW` + `(dev, ino)`
/// verification, enforce size bounds on the open handle, and copy its bytes into a `0700`
/// daemon-private staging directory (`$ORBIT_HOME/runtime/browser-uploads/<session_id>/`).
///
/// Eliminates the TOCTOU symlink-swap window between validation and Chrome reading the file.
async fn stage_upload_files_for_session(
    raw_paths: &[String],
    allowed_roots: &[PathBuf],
    orbit_home: &Path,
    session_id: &str,
) -> Result<StagedUploadBatch, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let canonical_files = validate_upload_file_paths(raw_paths, allowed_roots, orbit_home)?;
    let home_owned = orbit_home.to_path_buf();
    let sid_owned = session_id.to_string();
    let nonce_dir =
        tokio::task::spawn_blocking(move || create_upload_nonce_dir(&home_owned, &sid_owned))
            .await
            .map_err(|e| format!("UploadFile staging dir task failed: {e}"))??;

    let stage_result: Result<Vec<PathBuf>, String> = async {
        let mut staged_paths = Vec::with_capacity(canonical_files.len());
        let mut used_names = std::collections::HashSet::new();
        let mut total_bytes: u64 = 0;

        for (idx, (raw, canon_file)) in raw_paths.iter().zip(canonical_files.iter()).enumerate() {
            let trimmed = raw.trim();
            let (open_file, fmeta) =
                crate::git::open_validated_regular_file_async(canon_file, trimmed).await?;
            if !fmeta.is_file() {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: target must be a regular file"
                ));
            }
            if fmeta.len() > MAX_UPLOAD_FILE_BYTES {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: file exceeds maximum size of {} MiB",
                    MAX_UPLOAD_FILE_BYTES / (1024 * 1024)
                ));
            }
            if total_bytes.saturating_add(fmeta.len()) > MAX_UPLOAD_TOTAL_BYTES {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: combined upload exceeds maximum size of {} MiB",
                    MAX_UPLOAD_TOTAL_BYTES / (1024 * 1024)
                ));
            }

            let file_name = canon_file
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| {
                    !n.is_empty()
                        && *n != "."
                        && *n != ".."
                        && !n.contains('/')
                        && !n.contains('\\')
                        && !n.contains('\0')
                        && !n.chars().any(|c| c.is_control())
                })
                .ok_or_else(|| {
                    format!("UploadFile refused path `{trimmed}`: invalid file basename")
                })?;

            let target_dir = if used_names.insert(file_name.to_string()) {
                nonce_dir.clone()
            } else {
                let sub = nonce_dir.join(format!("f{idx}"));
                let sub_clone = sub.clone();
                tokio::task::spawn_blocking(move || ensure_private_dir_0700(&sub_clone))
                    .await
                    .map_err(|e| format!("UploadFile subdir task failed: {e}"))??;
                sub
            };
            let dest_path = target_dir.join(file_name);

            let mut dst_opts = tokio::fs::OpenOptions::new();
            dst_opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                dst_opts.mode(0o600);
                if let Some(flag) = crate::git::unix_o_nofollow_flag() {
                    dst_opts.custom_flags(flag);
                }
            }
            let mut staged_file = dst_opts
                .open(&dest_path)
                .await
                .map_err(|e| format!("UploadFile failed creating staged file `{file_name}`: {e}"))?;

            let mut limited_reader = open_file.take(MAX_UPLOAD_FILE_BYTES + 1);
            let copied = tokio::io::copy(&mut limited_reader, &mut staged_file)
                .await
                .map_err(|e| format!("UploadFile failed staging `{trimmed}`: {e}"))?;
            if copied > MAX_UPLOAD_FILE_BYTES {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: file grew past maximum size of {} MiB",
                    MAX_UPLOAD_FILE_BYTES / (1024 * 1024)
                ));
            }
            total_bytes = total_bytes.saturating_add(copied);
            if total_bytes > MAX_UPLOAD_TOTAL_BYTES {
                return Err(format!(
                    "UploadFile refused path `{trimmed}`: combined upload exceeds maximum size of {} MiB",
                    MAX_UPLOAD_TOTAL_BYTES / (1024 * 1024)
                ));
            }
            staged_file
                .flush()
                .await
                .map_err(|e| format!("UploadFile failed flushing staged file `{file_name}`: {e}"))?;
            staged_paths.push(dest_path);
        }

        Ok(staged_paths)
    }
    .await;

    match stage_result {
        Ok(staged_paths) => Ok(StagedUploadBatch {
            nonce_dir,
            staged_paths,
        }),
        Err(err) => {
            let dir = nonce_dir.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = remove_profile_dir_if_real(&dir);
            })
            .await;
            Err(err)
        }
    }
}

fn build_select_option_js(
    ref_id: Option<&str>,
    selector: Option<&str>,
    value: Option<&str>,
    label: Option<&str>,
    index: Option<u32>,
) -> Result<String, String> {
    let ref_json = serde_json::to_string(&ref_id).map_err(|e| e.to_string())?;
    let sel_json = serde_json::to_string(&selector).map_err(|e| e.to_string())?;
    let val_json = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    let lbl_json = serde_json::to_string(&label).map_err(|e| e.to_string())?;
    let idx_json = serde_json::to_string(&index).map_err(|e| e.to_string())?;
    Ok(format!(
        r#"(() => {{
  const refId = {ref_json};
  const selector = {sel_json};
  const targetValue = {val_json};
  const targetLabel = {lbl_json};
  const targetIndex = {idx_json};

  const findDeep = (root, query, depth) => {{
    if (!root || depth > 8) return null;
    try {{
      const direct = root.querySelector(query);
      if (direct) return direct;
      const all = root.querySelectorAll('*');
      for (const n of all) {{
        if (n.shadowRoot) {{
          const hit = findDeep(n.shadowRoot, query, depth + 1);
          if (hit) return hit;
        }}
        if (n.tagName === 'IFRAME' || n.tagName === 'FRAME') {{
          try {{
            if (n.contentDocument) {{
              const hit = findDeep(n.contentDocument, query, depth + 1);
              if (hit) return hit;
            }}
          }} catch (_) {{}}
        }}
      }}
    }} catch (_) {{}}
    return null;
  }};

  let el = null;
  if (refId) {{
    el = findDeep(document, `[data-orbit-ref="${{CSS.escape(refId)}}"]`, 0);
  }}
  if (!el && selector) {{
    try {{
      el = findDeep(document, selector, 0);
    }} catch (err) {{
      return {{ ok: false, error: `Invalid CSS selector: ${{String(err)}}` }};
    }}
  }}
  if (!el) {{
    return {{ ok: false, error: 'Target <select> element not found' }};
  }}
  if ((el.tagName || '').toLowerCase() !== 'select') {{
    return {{ ok: false, error: `Target element is <${{(el.tagName || '').toLowerCase()}}>, expected <select>` }};
  }}
  if (el.disabled) {{
    return {{ ok: false, error: 'Target <select> element is disabled' }};
  }}

  const options = Array.from(el.options || []);
  let matchedIdx = -1;
  if (targetValue !== null) {{
    matchedIdx = options.findIndex((o) => o.value === targetValue);
  }} else if (targetLabel !== null) {{
    const norm = targetLabel.trim();
    matchedIdx = options.findIndex((o) => (o.label || o.textContent || '').trim() === norm);
    if (matchedIdx < 0) {{
      const lower = norm.toLowerCase();
      matchedIdx = options.findIndex((o) => (o.label || o.textContent || '').trim().toLowerCase() === lower);
    }}
  }} else if (targetIndex !== null) {{
    if (targetIndex >= 0 && targetIndex < options.length) {{
      matchedIdx = targetIndex;
    }}
  }}

  if (matchedIdx < 0 || !options[matchedIdx]) {{
    return {{ ok: false, error: 'No matching <option> found in <select>' }};
  }}
  const opt = options[matchedIdx];
  if (opt.disabled) {{
    return {{ ok: false, error: `Option #${{matchedIdx}} is disabled` }};
  }}

  try {{ el.focus(); }} catch (_) {{}}
  const nativeSetter = Object.getOwnPropertyDescriptor(window.HTMLSelectElement.prototype, 'value')?.set;
  if (nativeSetter) {{
    nativeSetter.call(el, opt.value);
  }} else {{
    el.value = opt.value;
  }}
  el.selectedIndex = matchedIdx;
  opt.selected = true;
  el.dispatchEvent(new Event('input', {{ bubbles: true, composed: true }}));
  el.dispatchEvent(new Event('change', {{ bubbles: true, composed: true }}));

  return {{
    ok: true,
    selectedIndex: matchedIdx,
    selectedValue: opt.value || '',
    selectedLabel: (opt.label || opt.textContent || '').trim()
  }};
}})()"#
    ))
}

fn build_locate_file_input_js(
    ref_id: Option<&str>,
    selector: Option<&str>,
    file_count: usize,
) -> Result<String, String> {
    let ref_json = serde_json::to_string(&ref_id).map_err(|e| e.to_string())?;
    let sel_json = serde_json::to_string(&selector).map_err(|e| e.to_string())?;
    Ok(format!(
        r#"(() => {{
  const refId = {ref_json};
  const selector = {sel_json};
  const fileCount = {file_count};

  const findDeep = (root, query, depth) => {{
    if (!root || depth > 8) return null;
    try {{
      const direct = root.querySelector(query);
      if (direct) return direct;
      const all = root.querySelectorAll('*');
      for (const n of all) {{
        if (n.shadowRoot) {{
          const hit = findDeep(n.shadowRoot, query, depth + 1);
          if (hit) return hit;
        }}
        if (n.tagName === 'IFRAME' || n.tagName === 'FRAME') {{
          try {{
            if (n.contentDocument) {{
              const hit = findDeep(n.contentDocument, query, depth + 1);
              if (hit) return hit;
            }}
          }} catch (_) {{}}
        }}
      }}
    }} catch (_) {{}}
    return null;
  }};

  let el = null;
  if (refId) {{
    el = findDeep(document, `[data-orbit-ref="${{CSS.escape(refId)}}"]`, 0);
  }}
  if (!el && selector) {{
    el = findDeep(document, selector, 0);
  }}
  if (!el) {{
    throw new Error('Target <input type="file"> element not found');
  }}
  const tag = (el.tagName || '').toLowerCase();
  const type = (el.getAttribute('type') || el.type || '').toLowerCase();
  if (tag !== 'input' || type !== 'file') {{
    throw new Error(`Target element is <${{tag}} type="${{type}}">, expected <input type="file">`);
  }}
  if (fileCount > 1 && !el.multiple) {{
    throw new Error('Target file input does not accept multiple files');
  }}
  return el;
}})()"#
    ))
}

fn build_pick_element_js(
    x: Option<f64>,
    y: Option<f64>,
    selector: Option<&str>,
    ref_id: Option<&str>,
    interactive: bool,
    redact_sensitive: bool,
) -> Result<String, String> {
    let x_json = serde_json::to_string(&x).map_err(|e| e.to_string())?;
    let y_json = serde_json::to_string(&y).map_err(|e| e.to_string())?;
    let sel_json = serde_json::to_string(&selector).map_err(|e| e.to_string())?;
    let ref_json = serde_json::to_string(&ref_id).map_err(|e| e.to_string())?;
    let interactive_json = if interactive { "true" } else { "false" };
    let redact_json = if redact_sensitive { "true" } else { "false" };
    Ok(format!(
        r#"(() => {{
  const px = {x_json};
  const py = {y_json};
  const selector = {sel_json};
  const refId = {ref_json};
  const interactive = {interactive_json};
  const redactSensitive = {redact_json};

  const isSensitiveEl = (el) => {{
    if (!redactSensitive || !el) return false;
    const tag = (el.tagName || '').toLowerCase();
    const type = (el.getAttribute && (el.getAttribute('type') || el.type || '') || '').toLowerCase();
    const ac = (el.getAttribute && el.getAttribute('autocomplete') || '').toLowerCase();
    if (tag === 'input' && type === 'password') return true;
    if (/(current-password|new-password|one-time-code|cc-number|cc-csc)/.test(ac)) return true;
    return false;
  }};

  const buildUniqueSelector = (el) => {{
    if (!el || el.nodeType !== 1) return '';
    const tag = (el.tagName || 'div').toLowerCase();
    if (el.id) {{
      try {{
        const idSel = `#${{CSS.escape(el.id)}}`;
        if (document.querySelectorAll(idSel).length === 1) return `${{tag}}${{idSel}}`;
      }} catch (_) {{}}
    }}
    const testId = el.getAttribute && (el.getAttribute('data-testid') || el.getAttribute('data-test-id'));
    if (testId) {{
      const attr = el.hasAttribute('data-testid') ? 'data-testid' : 'data-test-id';
      const sel = `[${{attr}}="${{testId.replace(/"/g, '\\"')}}"]`;
      try {{
        if (document.querySelectorAll(sel).length === 1) return sel;
      }} catch (_) {{}}
    }}
    const parts = [];
    let cur = el;
    let depth = 0;
    while (cur && cur.nodeType === 1 && depth < 5) {{
      depth += 1;
      const t = (cur.tagName || '').toLowerCase();
      if (!t || t === 'html') break;
      if (cur.id) {{
        try {{
          const idSel = `#${{CSS.escape(cur.id)}}`;
          if (document.querySelectorAll(idSel).length === 1) {{
            parts.unshift(`${{t}}${{idSel}}`);
            break;
          }}
        }} catch (_) {{}}
      }}
      let seg = t;
      const clsList = Array.from(cur.classList || [])
        .filter((c) => c && !c.startsWith('orbit-') && c.length <= 36)
        .slice(0, 2);
      if (clsList.length > 0) {{
        seg += clsList.map((c) => `.${{CSS.escape(c)}}`).join('');
      }}
      if (cur.parentElement) {{
        const siblings = Array.from(cur.parentElement.children).filter(
          (s) => s.tagName === cur.tagName
        );
        if (siblings.length > 1) {{
          const idx = siblings.indexOf(cur) + 1;
          seg += `:nth-of-type(${{idx}})`;
        }}
      }}
      parts.unshift(seg);
      if (t === 'body') break;
      cur = cur.parentElement;
    }}
    return parts.join(' > ') || tag;
  }};

  const extractElementInfo = (el) => {{
    if (!el || el.nodeType !== 1) return {{ error: 'No DOM element found at target' }};
    const tag = (el.tagName || '').toLowerCase();
    const sensitive = isSensitiveEl(el);
    const rect = el.getBoundingClientRect ? el.getBoundingClientRect() : null;
    const attrs = {{}};
    if (el.attributes) {{
      let count = 0;
      for (const a of Array.from(el.attributes)) {{
        if (count >= 20) break;
        if (!a || !a.name || a.name === 'data-orbit-ref') continue;
        const val = (sensitive && a.name.toLowerCase() === 'value')
          ? '[redacted]'
          : String(a.value || '').slice(0, 200);
        attrs[a.name] = val;
        count += 1;
      }}
    }}
    const textPreview = sensitive
      ? ''
      : ((el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 240));
    const outerHtml = sensitive
      ? `<${{tag}} type="${{(el.getAttribute('type') || '')}}" [redacted]>`
      : ((el.outerHTML || '').replace(/\s+/g, ' ').trim().slice(0, 600));
    return {{
      selector: buildUniqueSelector(el),
      tag,
      text_preview: textPreview,
      attributes: attrs,
      computed_box: rect
        ? {{
            x: Math.round(rect.left * 10) / 10,
            y: Math.round(rect.top * 10) / 10,
            width: Math.round(rect.width * 10) / 10,
            height: Math.round(rect.height * 10) / 10
          }}
        : null,
      outer_html_excerpt: outerHtml,
      page_url: window.location.href || ''
    }};
  }};

  const elementAtPointDeep = (x, y) => {{
    let cur = document.elementFromPoint(x, y);
    let depth = 0;
    while (cur && depth < 12) {{
      depth += 1;
      if (cur.shadowRoot && cur.shadowRoot.elementFromPoint) {{
        const inner = cur.shadowRoot.elementFromPoint(x, y);
        if (inner && inner !== cur) {{
          cur = inner;
          continue;
        }}
      }}
      break;
    }}
    return cur;
  }};

  const findDeep = (root, query, depth) => {{
    if (!root || depth > 8) return null;
    try {{
      const direct = root.querySelector(query);
      if (direct) return direct;
      for (const n of root.querySelectorAll('*')) {{
        if (n.shadowRoot) {{
          const hit = findDeep(n.shadowRoot, query, depth + 1);
          if (hit) return hit;
        }}
      }}
    }} catch (_) {{}}
    return null;
  }};

  if (px !== null && py !== null) {{
    return extractElementInfo(elementAtPointDeep(px, py));
  }}
  if (refId) {{
    const el = findDeep(document, `[data-orbit-ref="${{CSS.escape(refId)}}"]`, 0);
    return extractElementInfo(el);
  }}
  if (selector) {{
    try {{
      const el = findDeep(document, selector, 0);
      return extractElementInfo(el);
    }} catch (err) {{
      return {{ error: `Invalid CSS selector: ${{String(err)}}` }};
    }}
  }}
  if (!interactive) {{
    return {{ error: 'Missing PickElement target' }};
  }}

  return new Promise((resolve) => {{
    const overlay = document.createElement('div');
    overlay.setAttribute('data-orbit-picker-overlay', '1');
    overlay.style.cssText = 'position:fixed;pointer-events:none;z-index:2147483647;border:1.5px solid #2563EB;background:rgba(37,99,235,0.10);border-radius:4px;box-sizing:border-box;display:none;transition:transform 40ms ease-out;';
    const badge = document.createElement('div');
    badge.style.cssText = 'position:fixed;pointer-events:none;z-index:2147483647;background:#14161D;color:#F1F2F7;font:500 11px/1.35 ui-monospace,SFMono-Regular,Menlo,monospace;padding:2px 6px;border-radius:4px;white-space:nowrap;max-width:320px;overflow:hidden;text-overflow:ellipsis;display:none;';
    document.documentElement.appendChild(overlay);
    document.documentElement.appendChild(badge);

    let settled = false;
    const cleanup = () => {{
      window.removeEventListener('mousemove', onMove, true);
      window.removeEventListener('click', onClick, true);
      window.removeEventListener('keydown', onKey, true);
      try {{ overlay.remove(); }} catch (_) {{}}
      try {{ badge.remove(); }} catch (_) {{}}
      clearTimeout(timer);
    }};
    const finish = (payload) => {{
      if (settled) return;
      settled = true;
      cleanup();
      resolve(payload);
    }};
    const onMove = (e) => {{
      const el = elementAtPointDeep(e.clientX, e.clientY);
      if (!el || el === overlay || el === badge) return;
      const r = el.getBoundingClientRect();
      overlay.style.display = 'block';
      overlay.style.left = `${{r.left}}px`;
      overlay.style.top = `${{r.top}}px`;
      overlay.style.width = `${{r.width}}px`;
      overlay.style.height = `${{r.height}}px`;
      badge.textContent = buildUniqueSelector(el);
      badge.style.display = 'block';
      badge.style.left = `${{Math.max(4, Math.min(window.innerWidth - 200, r.left))}}px`;
      badge.style.top = `${{Math.max(4, r.top > 24 ? r.top - 22 : r.bottom + 4)}}px`;
    }};
    const onClick = (e) => {{
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();
      const el = elementAtPointDeep(e.clientX, e.clientY);
      finish(extractElementInfo(el));
    }};
    const onKey = (e) => {{
      if (e.key === 'Escape') {{
        e.preventDefault();
        e.stopPropagation();
        finish({{ cancelled: true }});
      }}
    }};
    window.addEventListener('mousemove', onMove, true);
    window.addEventListener('click', onClick, true);
    window.addEventListener('keydown', onKey, true);
    const timer = setTimeout(() => finish({{ cancelled: true }}), 12000);
  }});
}})()"#
    ))
}

fn sanitize_picked_element(mut picked: PickedElementInfo) -> PickedElementInfo {
    picked.selector = truncate_chars(picked.selector.trim(), 240);
    picked.tag = truncate_chars(picked.tag.trim(), 48).to_ascii_lowercase();
    picked.text_preview = truncate_chars(picked.text_preview.trim(), 240);
    picked.outer_html_excerpt = truncate_chars(picked.outer_html_excerpt.trim(), 600);
    picked.page_url = truncate_chars(picked.page_url.trim(), 512);
    let mut clean_attrs = BTreeMap::new();
    for (k, v) in picked.attributes.into_iter().take(20) {
        let key = truncate_chars(k.trim(), 64);
        if !key.is_empty() {
            clean_attrs.insert(key, truncate_chars(&v, 200));
        }
    }
    picked.attributes = clean_attrs;
    picked.computed_box = picked.computed_box.filter(|b| {
        b.x.is_finite() && b.y.is_finite() && b.width.is_finite() && b.height.is_finite()
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use tokio::net::TcpListener;

    /// Pure-Rust mock CDP WebSocket server for deterministic end-to-end testing without Chrome.
    struct MockCdpServer {
        ws_url: String,
        received_commands: Arc<Mutex<Vec<(String, Value)>>>,
        event_tx: tokio::sync::broadcast::Sender<Value>,
        shutdown_tx: Option<tokio::sync::broadcast::Sender<()>>,
    }

    #[derive(Clone)]
    struct MockCdpBehavior {
        url: String,
        title: String,
        text_excerpt: String,
        elements: Vec<Value>,
        screenshot_b64: String,
        captcha_detected: bool,
        captcha_reason: Option<String>,
        live_sensitive_ref: Option<String>,
        post_click_sensitive: bool,
        fail_sensitive_eval: bool,
        captcha_point: Option<(f64, f64)>,
        fail_navigate: bool,
        redirect_to_url: Option<String>,
    }

    impl Default for MockCdpBehavior {
        fn default() -> Self {
            let tiny_png =
                base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\nmock");
            Self {
                url: "http://127.0.0.1:3000/dashboard".to_string(),
                title: "Orbit Mock App".to_string(),
                text_excerpt: "Welcome to Orbit Dashboard".to_string(),
                elements: vec![
                    json!({
                        "ref_id": "e1",
                        "role": "button",
                        "name": "Deploy",
                        "tag": "button",
                        "input_type": null,
                        "x": 100.0,
                        "y": 50.0,
                        "width": 80.0,
                        "height": 30.0,
                        "sensitive": false,
                        "captcha_like": false
                    }),
                    json!({
                        "ref_id": "e2",
                        "role": "textbox",
                        "name": "Search",
                        "tag": "input",
                        "input_type": "text",
                        "x": 20.0,
                        "y": 120.0,
                        "width": 200.0,
                        "height": 32.0,
                        "sensitive": false,
                        "captcha_like": false
                    }),
                    json!({
                        "ref_id": "e3",
                        "role": "textbox",
                        "name": "Password",
                        "tag": "input",
                        "input_type": "password",
                        "x": 20.0,
                        "y": 170.0,
                        "width": 200.0,
                        "height": 32.0,
                        "sensitive": true,
                        "captcha_like": false
                    }),
                ],
                screenshot_b64: tiny_png,
                captcha_detected: false,
                captcha_reason: None,
                live_sensitive_ref: Some("e3".to_string()),
                post_click_sensitive: false,
                fail_sensitive_eval: false,
                captcha_point: None,
                fail_navigate: false,
                redirect_to_url: None,
            }
        }
    }

    impl MockCdpServer {
        async fn start(behavior: MockCdpBehavior) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind loopback mock cdp");
            let addr: SocketAddr = listener.local_addr().expect("local addr");
            let received_commands = Arc::new(Mutex::new(Vec::new()));
            let cmds_bg = Arc::clone(&received_commands);
            let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(4);
            let shutdown_tx_accept = shutdown_tx.clone();
            let (event_tx, _) = tokio::sync::broadcast::channel::<Value>(32);
            let event_tx_bg = event_tx.clone();
            let state = Arc::new(Mutex::new(behavior));

            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown_rx.recv() => break,
                        accept = listener.accept() => {
                            let Ok((stream, _)) = accept else { break; };
                            let cmds = Arc::clone(&cmds_bg);
                            let st = Arc::clone(&state);
                            let mut ev_rx = event_tx_bg.subscribe();
                            let mut conn_shutdown = shutdown_tx_accept.subscribe();
                            tokio::spawn(async move {
                                let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
                                    return;
                                };
                                let (mut sink, mut rx) = ws.split();
                                loop {
                                    tokio::select! {
                                        _ = conn_shutdown.recv() => {
                                            let _ = sink.send(TungMsg::Close(None)).await;
                                            break;
                                        }
                                        Ok(ev) = ev_rx.recv() => {
                                            if sink.send(TungMsg::Text(ev.to_string().into())).await.is_err() {
                                                break;
                                            }
                                        }
                                        msg_opt = rx.next() => {
                                            let Some(Ok(msg)) = msg_opt else { break; };
                                            let TungMsg::Text(txt) = msg else { continue; };
                                            let Ok(val) = serde_json::from_str::<Value>(&txt) else { continue; };
                                            let id = val.get("id").and_then(Value::as_u64).unwrap_or(0);
                                            let method = val
                                                .get("method")
                                                .and_then(Value::as_str)
                                                .unwrap_or("")
                                                .to_string();
                                            let params = val.get("params").cloned().unwrap_or(Value::Null);
                                            cmds.lock().await.push((method.clone(), params.clone()));

                                            let reply = match method.as_str() {
                                                "Page.navigate" => {
                                                    let mut guard = st.lock().await;
                                                    if guard.fail_navigate {
                                                        json!({ "id": id, "error": { "message": "mock navigation failure" } })
                                                    } else {
                                                        if let Some(u) = params.get("url").and_then(Value::as_str) {
                                                            if let Some(ref redir) = guard.redirect_to_url
                                                                && u != "about:blank"
                                                            {
                                                                guard.url = redir.clone();
                                                            } else {
                                                                guard.url = u.to_string();
                                                            }
                                                        }
                                                        json!({ "id": id, "result": { "frameId": "frame_1" } })
                                                    }
                                                }
                                                "Page.captureScreenshot" => {
                                                    let data = st.lock().await.screenshot_b64.clone();
                                                    json!({ "id": id, "result": { "data": data } })
                                                }
                                                "Runtime.evaluate" => {
                                                    let expr = params
                                                        .get("expression")
                                                        .and_then(Value::as_str)
                                                        .unwrap_or("");
                                                    let cur = st.lock().await.clone();
                                                    if expr == "window.location.href || ''" {
                                                        json!({ "id": id, "result": { "result": { "value": cur.url } } })
                                                    } else if expr.contains("resolveDeepActive") {
                                                        if cur.fail_sensitive_eval {
                                                            json!({ "id": id, "error": { "message": "eval failed" } })
                                                        } else {
                                                            let is_null_ref = expr.contains("const refId = null;");
                                                            let is_sens = if is_null_ref {
                                                                cur.post_click_sensitive
                                                            } else {
                                                                cur.live_sensitive_ref
                                                                    .as_deref()
                                                                    .is_some_and(|r| expr.contains(&format!("\"{r}\"")))
                                                            };
                                                            json!({
                                                                "id": id,
                                                                "result": {
                                                                    "result": {
                                                                        "value": {
                                                                            "sensitive": is_sens,
                                                                            "reason": if is_sens { "password input field" } else { "" }
                                                                        }
                                                                    }
                                                                }
                                                            })
                                                        }
                                                    } else if expr.contains("HTMLSelectElement") {
                                                        json!({
                                                            "id": id,
                                                            "result": {
                                                                "result": {
                                                                    "value": {
                                                                        "ok": true,
                                                                        "selectedIndex": 1,
                                                                        "selectedValue": "pro",
                                                                        "selectedLabel": "Pro Plan"
                                                                    }
                                                                }
                                                            }
                                                        })
                                                    } else if expr.contains("Target <input type=\"file\">") {
                                                        json!({
                                                            "id": id,
                                                            "result": {
                                                                "result": {
                                                                    "type": "object",
                                                                    "objectId": "obj_file_input_1"
                                                                }
                                                            }
                                                        })
                                                    } else if expr.contains("buildUniqueSelector") {
                                                        json!({
                                                            "id": id,
                                                            "result": {
                                                                "result": {
                                                                    "value": {
                                                                        "selector": "button#submit-order",
                                                                        "tag": "button",
                                                                        "text_preview": "Submit Order",
                                                                        "attributes": {
                                                                            "id": "submit-order",
                                                                            "data-testid": "submit-btn"
                                                                        },
                                                                        "computed_box": {
                                                                            "x": 42.0,
                                                                            "y": 108.5,
                                                                            "width": 120.0,
                                                                            "height": 36.0
                                                                        },
                                                                        "outer_html_excerpt": "<button id=\"submit-order\" data-testid=\"submit-btn\">Submit Order</button>",
                                                                        "page_url": cur.url
                                                                    }
                                                                }
                                                            }
                                                        })
                                                    } else if expr.contains("document.elementFromPoint") {
                                                        let hit = cur.captcha_point.is_some_and(|(cx, cy)| {
                                                            expr.contains(&format!("elementFromPoint({cx}, {cy})"))
                                                        });
                                                        json!({
                                                            "id": id,
                                                            "result": {
                                                                "result": {
                                                                    "value": {
                                                                        "captcha": hit,
                                                                        "reason": if hit { "iframe[src*=\"turnstile\"]" } else { "" }
                                                                    }
                                                                }
                                                            }
                                                        })
                                                    } else {
                                                        json!({
                                                            "id": id,
                                                            "result": {
                                                                "result": {
                                                                    "value": {
                                                                        "url": cur.url,
                                                                        "title": cur.title,
                                                                        "text_excerpt": cur.text_excerpt,
                                                                        "captcha_detected": cur.captcha_detected,
                                                                        "captcha_reason": cur.captcha_reason,
                                                                        "elements": cur.elements
                                                                    }
                                                                }
                                                            }
                                                        })
                                                    }
                                                }
                                                "DOM.describeNode" => {
                                                    json!({
                                                        "id": id,
                                                        "result": {
                                                            "node": {
                                                                "backendNodeId": 4242
                                                            }
                                                        }
                                                    })
                                                }
                                                _ => json!({ "id": id, "result": {} }),
                                            };

                                            if sink
                                                .send(TungMsg::Text(reply.to_string().into()))
                                                .await
                                                .is_err()
                                            {
                                                break;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }
                }
            });

            Self {
                ws_url: format!("ws://127.0.0.1:{}/devtools/page/mock1", addr.port()),
                received_commands,
                event_tx,
                shutdown_tx: Some(shutdown_tx),
            }
        }

        fn shutdown(&mut self) {
            if let Some(tx) = self.shutdown_tx.take() {
                let _ = tx.send(());
            }
        }
    }

    impl Drop for MockCdpServer {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    #[test]
    fn url_guard_blocks_dangerous_schemes_ssrf_and_domain_policies() {
        let policy = BrowserPolicyConfig::default();

        for bad in [
            "file:///etc/passwd",
            "chrome://settings",
            "chrome-extension://abcdef/index.html",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://example.com/file",
            "http://user:pass@example.com/",
        ] {
            assert!(
                validate_navigation_url(bad, &policy).is_err(),
                "expected `{bad}` to be blocked"
            );
        }

        for ssrf in [
            "http://169.254.169.254/latest/meta-data/",
            "http://100.100.100.200/latest/meta-data/",
            "http://[fe80::1]/",
            "http://[::ffff:169.254.169.254]/",
            "http://[::169.254.169.254]/",
            "http://[64:ff9b::169.254.169.254]/",
            "http://[fd00:ec2::254]/",
            "http://0.0.0.0:8080/",
            "http://metadata.google.internal/computeMetadata/v1/",
            "http://metadata.goog/",
        ] {
            assert!(
                validate_navigation_url(ssrf, &policy).is_err(),
                "expected SSRF URL `{ssrf}` to be blocked"
            );
        }

        assert!(validate_navigation_url("http://localhost:3000/app", &policy).is_ok());
        assert!(validate_navigation_url("http://127.0.0.1:5173/", &policy).is_ok());
        assert!(validate_navigation_url("http://[::1]:8080/", &policy).is_ok());

        // Localhost (including deprecated IPv4-compatible `::127.0.0.1`) blocked when `allow_localhost = false`
        let no_local = BrowserPolicyConfig {
            allow_localhost: false,
            ..Default::default()
        };
        assert!(validate_navigation_url("http://localhost:3000/", &no_local).is_err());
        assert!(validate_navigation_url("http://127.0.0.1:3000/", &no_local).is_err());
        assert!(validate_navigation_url("http://[::1]:3000/", &no_local).is_err());
        assert!(validate_navigation_url("http://[::127.0.0.1]:3000/", &no_local).is_err());

        let domain_policy = BrowserPolicyConfig {
            allowed_domains: vec!["example.com".into(), "docs.rs".into()],
            blocked_domains: vec!["evil.example.com".into()],
            ..Default::default()
        };
        assert!(validate_navigation_url("https://example.com/page", &domain_policy).is_ok());
        assert!(validate_navigation_url("https://sub.example.com/", &domain_policy).is_ok());
        assert!(validate_navigation_url("https://evil.example.com/", &domain_policy).is_err());
        assert!(validate_navigation_url("https://other.org/", &domain_policy).is_err());

        // Loopback-only & credential-free CDP WebSocket enforcement (P2 #14)
        assert!(validate_cdp_ws_url("ws://127.0.0.1:9222/devtools/page/1").is_ok());
        assert!(validate_cdp_ws_url("ws://localhost:9222/devtools/page/1").is_ok());
        assert!(validate_cdp_ws_url("ws://[::1]:9222/devtools/page/1").is_ok());
        assert!(validate_cdp_ws_url("ws://user:pass@127.0.0.1:9222/devtools/page/1").is_err());
        assert!(validate_cdp_ws_url("ws://192.168.1.10:9222/devtools/page/1").is_err());
        assert!(validate_cdp_ws_url("ws://example.com:9222/devtools/page/1").is_err());
        assert!(validate_cdp_ws_url("http://127.0.0.1:9222/devtools/page/1").is_err());
    }

    #[tokio::test]
    async fn dns_pinned_ssrf_blocks_metadata_resolution() {
        let policy = BrowserPolicyConfig::default();

        let res = validate_navigation_url_with_resolver(
            "https://rebind.attacker.test/",
            &policy,
            |_host, _port| async { Ok(vec!["169.254.169.254".parse::<IpAddr>().expect("ip")]) },
        )
        .await;
        assert!(res.is_err());

        let res_nat64 = validate_navigation_url_with_resolver(
            "https://nat64.attacker.test/",
            &policy,
            |_host, _port| async {
                Ok(vec![
                    "64:ff9b::169.254.169.254".parse::<IpAddr>().expect("ip"),
                ])
            },
        )
        .await;
        assert!(res_nat64.is_err());

        let ok = validate_navigation_url_with_resolver(
            "https://example.com/",
            &policy,
            |_host, _port| async { Ok(vec!["93.184.216.34".parse::<IpAddr>().expect("ip")]) },
        )
        .await;
        assert!(ok.is_ok());
    }

    #[tokio::test]
    async fn fetch_interception_blocks_ssrf_and_allows_safe_requests() {
        let tmp_home = std::env::temp_dir().join(format!("orbit-browser-fetch-{}", Uuid::new_v4()));
        let manager = BrowserManager::new(tmp_home.clone());
        let server = MockCdpServer::start(MockCdpBehavior::default()).await;

        let summary = manager
            .create_session_from_cdp_ws_url(&server.ws_url, CreateBrowserSessionSpec::default())
            .await
            .expect("create session");

        // Verify Fetch.enable was requested on connect
        assert!(
            server
                .received_commands
                .lock()
                .await
                .iter()
                .any(|(m, _)| m == "Fetch.enable")
        );

        // Emit a Fetch.requestPaused for AWS IMDS (must be blocked via Fetch.failRequest)
        let _ = server.event_tx.send(json!({
            "method": "Fetch.requestPaused",
            "params": {
                "requestId": "req_ssrf",
                "resourceType": "XHR",
                "request": { "url": "http://169.254.169.254/latest/meta-data/" }
            }
        }));
        // Emit a Fetch.requestPaused for a data: Document (must be blocked)
        let _ = server.event_tx.send(json!({
            "method": "Fetch.requestPaused",
            "params": {
                "requestId": "req_data_doc",
                "resourceType": "Document",
                "request": { "url": "data:text/html,<h1>pwn</h1>" }
            }
        }));
        // Emit a Fetch.requestPaused for a data: Image subresource (must continue)
        let _ = server.event_tx.send(json!({
            "method": "Fetch.requestPaused",
            "params": {
                "requestId": "req_data_img",
                "resourceType": "Image",
                "request": { "url": "data:image/png;base64,iVBORw0KGgo=" }
            }
        }));

        // Wait briefly for spawned Fetch guard tasks to reply
        tokio::time::sleep(Duration::from_millis(120)).await;
        let cmds = server.received_commands.lock().await.clone();
        assert!(cmds.iter().any(|(m, p)| {
            m == "Fetch.failRequest"
                && p.get("requestId").and_then(Value::as_str) == Some("req_ssrf")
                && p.get("errorReason").and_then(Value::as_str) == Some("BlockedByClient")
        }));
        assert!(cmds.iter().any(|(m, p)| {
            m == "Fetch.failRequest"
                && p.get("requestId").and_then(Value::as_str) == Some("req_data_doc")
        }));
        assert!(cmds.iter().any(|(m, p)| {
            m == "Fetch.continueRequest"
                && p.get("requestId").and_then(Value::as_str) == Some("req_data_img")
        }));

        let _ = manager.close_session(&summary.session_id).await;
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn mock_cdp_navigate_snapshot_screenshot_click_type_and_password_guard() {
        let tmp_home = std::env::temp_dir().join(format!("orbit-browser-test-{}", Uuid::new_v4()));
        let manager = BrowserManager::new(tmp_home.clone());
        let server = MockCdpServer::start(MockCdpBehavior::default()).await;

        let summary = manager
            .create_session_from_cdp_ws_url(
                &server.ws_url,
                CreateBrowserSessionSpec {
                    agent_id: Some("ag_test".into()),
                    initial_url: Some("http://127.0.0.1:3000/dashboard".into()),
                    headless: Some(true),
                    viewport_width: Some(1280),
                    viewport_height: Some(800),
                    workspace_root: None,
                },
            )
            .await
            .expect("create mock browser session");
        assert_eq!(summary.agent_id.as_deref(), Some("ag_test"));
        assert_eq!(summary.url, "http://127.0.0.1:3000/dashboard");
        assert_eq!(summary.title, "Orbit Mock App");
        assert_eq!(summary.element_count, 3);

        let snap = manager
            .execute_action(&summary.session_id, BrowserActionRequest::Snapshot)
            .await
            .expect("snapshot");
        assert_eq!(snap.elements.len(), 3);
        assert_eq!(snap.elements[0].ref_id, "e1");
        assert_eq!(snap.elements[0].name, "Deploy");
        assert_eq!(
            snap.text_excerpt.as_deref(),
            Some("Welcome to Orbit Dashboard")
        );

        // Screenshot returns top-level base64 without duplicating inside `session.last_screenshot_base64`
        let shot = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::Screenshot { full_page: false },
            )
            .await
            .expect("screenshot");
        assert!(shot.screenshot_base64.is_some());
        assert!(shot.session.last_screenshot_base64.is_none());

        // `list_snapshot_with_options(true)` includes cached screenshot; `false` strips it
        assert!(
            manager.list_snapshot_with_options(true).await.sessions[0]
                .last_screenshot_base64
                .is_some()
        );
        assert!(
            manager.list_snapshot_with_options(false).await.sessions[0]
                .last_screenshot_base64
                .is_none()
        );

        manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::Click {
                    ref_id: Some("e1".into()),
                    x: None,
                    y: None,
                },
            )
            .await
            .expect("click e1");

        manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::Type {
                    ref_id: Some("e2".into()),
                    text: "orbit status".into(),
                    submit: true,
                },
            )
            .await
            .expect("type into search");

        let pwd_err = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::Type {
                    ref_id: Some("e3".into()),
                    text: "super-secret".into(),
                    submit: false,
                },
            )
            .await
            .expect_err("password typing must be refused");
        assert!(
            pwd_err.to_lowercase().contains("sensitive")
                || pwd_err.to_lowercase().contains("password"),
            "unexpected error: {pwd_err}"
        );

        let cmds = server.received_commands.lock().await.clone();
        assert!(cmds.iter().any(|(m, _)| m == "Page.navigate"));
        assert!(cmds.iter().any(|(m, _)| m == "Page.captureScreenshot"));
        assert!(cmds.iter().any(|(m, p)| {
            m == "Input.dispatchMouseEvent"
                && p.get("type").and_then(Value::as_str) == Some("mousePressed")
                && p.get("x").and_then(Value::as_f64) == Some(140.0)
                && p.get("y").and_then(Value::as_f64) == Some(65.0)
        }));
        assert!(cmds.iter().any(|(m, p)| {
            m == "Input.insertText" && p.get("text").and_then(Value::as_str) == Some("orbit status")
        }));
        assert!(!cmds.iter().any(|(m, p)| {
            m == "Input.insertText" && p.get("text").and_then(Value::as_str) == Some("super-secret")
        }));

        let after_close = manager
            .close_session(&summary.session_id)
            .await
            .expect("close session");
        assert!(after_close.sessions.is_empty());

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn p0_regressions_captcha_coords_post_click_sensitive_fail_closed_and_doc_url_guard() {
        let tmp_home = std::env::temp_dir().join(format!("orbit-browser-p0-{}", Uuid::new_v4()));
        let manager = BrowserManager::new(tmp_home.clone());

        // 1. Coordinate click on a CAPTCHA widget must be refused via live hit-testing (P0 #3)
        let captcha_server = MockCdpServer::start(MockCdpBehavior {
            captcha_point: Some((320.0, 240.0)),
            ..Default::default()
        })
        .await;
        let s_cap = manager
            .create_session_from_cdp_ws_url(
                &captcha_server.ws_url,
                CreateBrowserSessionSpec::default(),
            )
            .await
            .expect("create captcha session");
        let click_err = manager
            .execute_action(
                &s_cap.session_id,
                BrowserActionRequest::Click {
                    ref_id: None,
                    x: Some(320.0),
                    y: Some(240.0),
                },
            )
            .await
            .expect_err("coordinate click on CAPTCHA must fail");
        assert!(click_err.contains("captcha_detected"));
        assert!(
            !captcha_server
                .received_commands
                .lock()
                .await
                .iter()
                .any(|(m, _)| m == "Input.dispatchMouseEvent")
        );
        let _ = manager.close_session(&s_cap.session_id).await;

        // 2. Post-click focus landing on sensitive input must block Input.insertText (P0 #2)
        let post_click_server = MockCdpServer::start(MockCdpBehavior {
            post_click_sensitive: true,
            ..Default::default()
        })
        .await;
        let s_post = manager
            .create_session_from_cdp_ws_url(
                &post_click_server.ws_url,
                CreateBrowserSessionSpec::default(),
            )
            .await
            .expect("create post-click session");
        manager
            .execute_action(&s_post.session_id, BrowserActionRequest::Snapshot)
            .await
            .expect("snapshot");
        let type_err = manager
            .execute_action(
                &s_post.session_id,
                BrowserActionRequest::Type {
                    ref_id: Some("e2".into()),
                    text: "secret-pin".into(),
                    submit: false,
                },
            )
            .await
            .expect_err("post-click sensitive focus must refuse typing");
        assert!(type_err.contains("post-focus sensitive"));
        assert!(
            !post_click_server
                .received_commands
                .lock()
                .await
                .iter()
                .any(|(m, _)| m == "Input.insertText")
        );
        let _ = manager.close_session(&s_post.session_id).await;

        // 3. Evaluation error during sensitive input check must fail closed (P0 #2)
        let eval_err_server = MockCdpServer::start(MockCdpBehavior {
            fail_sensitive_eval: true,
            ..Default::default()
        })
        .await;
        let s_eval = manager
            .create_session_from_cdp_ws_url(
                &eval_err_server.ws_url,
                CreateBrowserSessionSpec::default(),
            )
            .await
            .expect("create fail-closed session");
        let fc_err = manager
            .execute_action(
                &s_eval.session_id,
                BrowserActionRequest::Type {
                    ref_id: None,
                    text: "hello".into(),
                    submit: false,
                },
            )
            .await
            .expect_err("eval failure must fail closed");
        assert!(fc_err.contains("fail-closed"));
        let _ = manager.close_session(&s_eval.session_id).await;

        // 4. Snapshot/Screenshot must reject & clear disallowed current document URL (P0 #1)
        let bad_doc_server = MockCdpServer::start(MockCdpBehavior {
            url: "file:///etc/passwd".to_string(),
            ..Default::default()
        })
        .await;
        let s_doc = manager
            .create_session_from_cdp_ws_url(
                &bad_doc_server.ws_url,
                CreateBrowserSessionSpec::default(),
            )
            .await
            .expect("create doc guard session");
        let snap_err = manager
            .execute_action(&s_doc.session_id, BrowserActionRequest::Snapshot)
            .await
            .expect_err("snapshot on file:// document must fail");
        assert!(snap_err.contains("disallowed"));
        assert!(
            bad_doc_server
                .received_commands
                .lock()
                .await
                .iter()
                .any(|(m, p)| m == "Page.navigate"
                    && p.get("url").and_then(Value::as_str) == Some("about:blank"))
        );

        manager.shutdown_all().await;
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn p1_and_p2_regressions_cleanup_on_nav_fail_concurrent_cap_disconnect_evict_and_symlinks()
     {
        let tmp_home = std::env::temp_dir().join(format!("orbit-browser-p1-{}", Uuid::new_v4()));
        let manager = BrowserManager::new(tmp_home.clone());

        // 1. Failed initial navigation must remove & shut down the session (P1 #5)
        let fail_nav_server = MockCdpServer::start(MockCdpBehavior {
            fail_navigate: true,
            ..Default::default()
        })
        .await;
        let err = manager
            .create_session_from_cdp_ws_url(
                &fail_nav_server.ws_url,
                CreateBrowserSessionSpec {
                    initial_url: Some("http://127.0.0.1:3000/fail".into()),
                    ..Default::default()
                },
            )
            .await
            .expect_err("initial nav failure must return error");
        assert!(err.contains("mock navigation failure"));
        assert!(
            manager.list_snapshot().await.sessions.is_empty(),
            "failed initial navigation must not leak a live session"
        );

        // 2. Concurrent session creation must never exceed `max_sessions` (P1 #6)
        manager
            .update_policy(BrowserPolicyConfig {
                max_sessions: 2,
                ..Default::default()
            })
            .await
            .expect("set max_sessions=2");
        let mut cap_server = MockCdpServer::start(MockCdpBehavior::default()).await;
        let mut handles = Vec::new();
        for _ in 0..6 {
            let mgr = Arc::clone(&manager);
            let url = cap_server.ws_url.clone();
            handles.push(tokio::spawn(async move {
                mgr.create_session_from_cdp_ws_url(&url, CreateBrowserSessionSpec::default())
                    .await
            }));
        }
        let mut ok_count = 0usize;
        for h in handles {
            if h.await.expect("join").is_ok() {
                ok_count += 1;
            }
        }
        assert_eq!(
            ok_count, 2,
            "concurrent creates must strictly honor max_sessions=2"
        );
        assert_eq!(manager.list_snapshot().await.sessions.len(), 2);

        // 3. Auto-evict crashed/disconnected sessions when WS drops (P2 #13)
        cap_server.shutdown();
        for _ in 0..20 {
            if manager.list_snapshot().await.sessions.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            manager.list_snapshot().await.sessions.is_empty(),
            "disconnected sessions must auto-evict"
        );

        // 4. ORBIT_CHROME_BIN validation rejects relative & non-executable paths (P1 #7)
        assert!(!is_valid_chrome_override_path(Path::new("relative/chrome")));
        let non_exec = tmp_home.join("fake-chrome");
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        std::fs::write(&non_exec, "#!/bin/sh\n").expect("write");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&non_exec, std::fs::Permissions::from_mode(0o644))
                .expect("chmod 644");
            assert!(!is_valid_chrome_override_path(&non_exec));
            std::fs::set_permissions(&non_exec, std::fs::Permissions::from_mode(0o755))
                .expect("chmod 755");
            assert!(is_valid_chrome_override_path(&non_exec));
        }

        // 5. Symlink-safe profile dir refuses pre-planted symlinks (P1 #8)
        #[cfg(unix)]
        {
            let target_dir = tmp_home.join("victim-dir");
            std::fs::create_dir_all(&target_dir).expect("mkdir victim");
            let symlink_root = tmp_home.join("symlink-home");
            std::fs::create_dir_all(&symlink_root).expect("mkdir sym home");
            std::os::unix::fs::symlink(&target_dir, symlink_root.join(BROWSER_PROFILES_DIR))
                .expect("symlink");
            assert!(create_isolated_profile_dir(&symlink_root, "s1").is_err());
            assert!(remove_profile_dir_if_real(&symlink_root.join(BROWSER_PROFILES_DIR)).is_err());
            assert!(target_dir.exists());
        }

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn session_cap_screenshot_cap_captcha_notice_and_atomic_policy_persist() {
        let tmp_home = std::env::temp_dir().join(format!("orbit-browser-cap-{}", Uuid::new_v4()));
        let manager = BrowserManager::new(tmp_home.clone());

        let updated = BrowserPolicyConfig {
            enabled: true,
            headless: true,
            allow_localhost: true,
            allowed_domains: vec![],
            blocked_domains: vec!["blocked.example".into()],
            block_password_inputs: true,
            max_sessions: 2,
        };
        let snap_after_update = manager
            .update_policy(updated.clone())
            .await
            .expect("update policy");
        assert_eq!(snap_after_update.policy.max_sessions, 2);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(tmp_home.join(BROWSER_POLICY_FILE)).expect("policy file");
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }

        let oversized_png =
            base64::engine::general_purpose::STANDARD.encode(vec![0u8; MAX_SCREENSHOT_BYTES + 64]);
        let server = MockCdpServer::start(MockCdpBehavior {
            captcha_detected: true,
            captcha_reason: Some(".cf-turnstile".into()),
            screenshot_b64: oversized_png,
            ..Default::default()
        })
        .await;

        let s1 = manager
            .create_session_from_cdp_ws_url(&server.ws_url, CreateBrowserSessionSpec::default())
            .await
            .expect("session 1");
        let s2 = manager
            .create_session_from_cdp_ws_url(&server.ws_url, CreateBrowserSessionSpec::default())
            .await
            .expect("session 2");

        let cap_err = manager
            .create_session_from_cdp_ws_url(&server.ws_url, CreateBrowserSessionSpec::default())
            .await
            .expect_err("must enforce session cap");
        assert!(cap_err.contains("Maximum active browser sessions"));

        let snap = manager
            .execute_action(&s1.session_id, BrowserActionRequest::Snapshot)
            .await
            .expect("snapshot with captcha");
        assert!(snap.captcha_detected);
        assert!(
            snap.text_excerpt
                .as_deref()
                .unwrap_or("")
                .contains("captcha_detected")
        );

        let shot_err = manager
            .execute_action(
                &s1.session_id,
                BrowserActionRequest::Screenshot { full_page: true },
            )
            .await
            .expect_err("oversized screenshot must fail");
        assert!(shot_err.contains("8 MiB"));

        assert_eq!(manager.list_snapshot().await.sessions.len(), 2);
        let _ = s2;
        manager.shutdown_all().await;
        assert!(manager.list_snapshot().await.sessions.is_empty());

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[test]
    fn parse_key_descriptor_special_keys_and_modifier_chords() {
        let enter = parse_key_descriptor("Enter", &[]).expect("Enter");
        assert_eq!(enter.key, "Enter");
        assert_eq!(enter.code, "Enter");
        assert_eq!(enter.windows_virtual_key_code, 13);
        assert_eq!(enter.modifiers_mask, 0);
        assert_eq!(enter.text.as_deref(), Some("\r"));
        assert!(!enter.emits_printable_text());

        let esc = parse_key_descriptor("Escape", &[]).expect("Escape");
        assert_eq!(esc.key, "Escape");
        assert_eq!(esc.windows_virtual_key_code, 27);
        assert_eq!(esc.text, None);

        let tab = parse_key_descriptor("Shift+Tab", &[]).expect("Shift+Tab");
        assert_eq!(tab.key, "Tab");
        assert_eq!(tab.windows_virtual_key_code, 9);
        assert_eq!(tab.modifiers_mask, CDP_MODIFIER_SHIFT);

        let bs = parse_key_descriptor("Backspace", &[]).expect("Backspace");
        assert_eq!(bs.windows_virtual_key_code, 8);

        let down = parse_key_descriptor("ArrowDown", &[]).expect("ArrowDown");
        assert_eq!(down.code, "ArrowDown");
        assert_eq!(down.windows_virtual_key_code, 40);

        let space = parse_key_descriptor("Space", &[]).expect("Space");
        assert_eq!(space.key, " ");
        assert_eq!(space.code, "Space");
        assert_eq!(space.windows_virtual_key_code, 32);
        assert_eq!(space.text.as_deref(), Some(" "));
        assert!(space.emits_printable_text());

        let meta_a = parse_key_descriptor("Meta+A", &[]).expect("Meta+A");
        assert_eq!(meta_a.code, "KeyA");
        assert_eq!(meta_a.modifiers_mask, CDP_MODIFIER_META);
        assert_eq!(meta_a.text, None);
        assert_eq!(meta_a.commands, vec!["selectAll"]);

        let ctrl_c = parse_key_descriptor("Control+C", &[]).expect("Control+C");
        assert_eq!(ctrl_c.code, "KeyC");
        assert_eq!(ctrl_c.modifiers_mask, CDP_MODIFIER_CTRL);
        assert_eq!(ctrl_c.commands, vec!["copy"]);

        let redo = parse_key_descriptor("Control+Shift+Z", &[]).expect("Control+Shift+Z");
        assert_eq!(redo.modifiers_mask, CDP_MODIFIER_CTRL | CDP_MODIFIER_SHIFT);
        assert_eq!(redo.commands, vec!["redo"]);

        let plus_chord = parse_key_descriptor("Meta++", &[]).expect("Meta++");
        assert_eq!(plus_chord.key, "+");
        assert_eq!(plus_chord.code, "Equal");
        assert_eq!(plus_chord.modifiers_mask, CDP_MODIFIER_META);

        assert!(parse_key_descriptor("BogusMod+A", &[]).is_err());
        assert!(parse_key_descriptor("", &[]).is_err());
    }

    #[test]
    fn upload_file_path_confinement_blocks_escapes_symlinks_and_sensitive_dirs() {
        let root = std::env::temp_dir().join(format!("orbit-upload-guard-{}", Uuid::new_v4()));
        let workspace = root.join("workspace");
        let orbit_home = root.join("orbit_home");
        let outside = root.join("outside.txt");
        let nested_orbit = workspace.join(".orbit");
        let nested_ssh = workspace.join(".ssh");
        std::fs::create_dir_all(workspace.join("fixtures")).expect("mkdir fixtures");
        std::fs::create_dir_all(&orbit_home).expect("mkdir orbit_home");
        std::fs::create_dir_all(&nested_orbit).expect("mkdir .orbit");
        std::fs::create_dir_all(&nested_ssh).expect("mkdir .ssh");
        std::fs::write(workspace.join("fixtures/sample.png"), b"PNGDATA").expect("write sample");
        std::fs::write(&outside, b"TOP_SECRET").expect("write outside");
        std::fs::write(orbit_home.join("mcp-token"), b"SECRET_TOKEN").expect("write token");
        std::fs::write(nested_orbit.join("pairing.key"), b"PAIRING").expect("write pairing");
        std::fs::write(nested_ssh.join("id_ed25519"), b"PRIVATE_KEY").expect("write ssh key");

        // 1. Confined relative & absolute files inside workspace MUST pass.
        let ok_rel = validate_upload_file_paths(
            &["fixtures/sample.png".into()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect("confined relative path must pass");
        assert_eq!(ok_rel.len(), 1);
        assert!(ok_rel[0].ends_with("fixtures/sample.png"));

        let ok_abs = validate_upload_file_paths(
            &[workspace
                .join("fixtures/sample.png")
                .to_string_lossy()
                .to_string()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect("confined absolute path must pass");
        assert_eq!(ok_abs, ok_rel);

        // 2. Empty allowed roots MUST fail closed.
        assert!(
            validate_upload_file_paths(&["fixtures/sample.png".into()], &[], &orbit_home).is_err()
        );

        // 3. Parent directory traversal `..` MUST be rejected.
        let err_traversal = validate_upload_file_paths(
            &["../outside.txt".into()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect_err(".. traversal must be rejected");
        assert!(err_traversal.contains("traversal"));

        // 4. Absolute path outside workspace root MUST be rejected.
        let err_outside = validate_upload_file_paths(
            &[outside.to_string_lossy().to_string()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect_err("outside path must be rejected");
        assert!(err_outside.contains("escapes allowed workspace root"));

        // 5. Symlink inside workspace pointing outside MUST be rejected after canonicalization!
        #[cfg(unix)]
        {
            let escape_link = workspace.join("fixtures/evil-link.txt");
            std::os::unix::fs::symlink(&outside, &escape_link).expect("create escaping symlink");
            let err_sym = validate_upload_file_paths(
                &["fixtures/evil-link.txt".into()],
                std::slice::from_ref(&workspace),
                &orbit_home,
            )
            .expect_err("escaping symlink must be rejected");
            assert!(err_sym.contains("escapes allowed workspace root"));
        }

        // 6. Sensitive dot-directories (`.ssh`, `.orbit`) and `$ORBIT_HOME` MUST be rejected
        //    even when inside an allowed root!
        let err_ssh = validate_upload_file_paths(
            &[".ssh/id_ed25519".into()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect_err(".ssh upload must be rejected");
        assert!(err_ssh.contains("sensitive"));

        let err_dot_orbit = validate_upload_file_paths(
            &[".orbit/pairing.key".into()],
            std::slice::from_ref(&workspace),
            &orbit_home,
        )
        .expect_err(".orbit upload must be rejected");
        assert!(err_dot_orbit.contains("sensitive"));

        let err_orbit_home = validate_upload_file_paths(
            &[orbit_home.join("mcp-token").to_string_lossy().to_string()],
            std::slice::from_ref(&root),
            &orbit_home,
        )
        .expect_err("$ORBIT_HOME upload must be rejected");
        assert!(err_orbit_home.contains("$ORBIT_HOME"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn mock_cdp_press_key_select_option_upload_file_and_pick_element() {
        let root = std::env::temp_dir().join(format!("orbit-cdp-actions-{}", Uuid::new_v4()));
        let tmp_home = root.join("orbit_home");
        let workspace = root.join("workspace");
        let outside = root.join("secret.txt");
        std::fs::create_dir_all(workspace.join("assets")).expect("mkdir assets");
        std::fs::create_dir_all(&tmp_home).expect("mkdir home");
        std::fs::write(workspace.join("assets/logo.png"), b"PNG").expect("write logo");
        std::fs::write(&outside, b"SECRET").expect("write secret");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, workspace.join("assets/escape.png"))
            .expect("escape symlink");

        let manager = BrowserManager::new(tmp_home.clone());
        let server = MockCdpServer::start(MockCdpBehavior::default()).await;

        let summary = manager
            .create_session_from_cdp_ws_url(
                &server.ws_url,
                CreateBrowserSessionSpec {
                    agent_id: Some("ag_actions".into()),
                    initial_url: Some("http://127.0.0.1:5173/checkout".into()),
                    headless: Some(true),
                    viewport_width: Some(1280),
                    viewport_height: Some(800),
                    workspace_root: Some(workspace.clone()),
                },
            )
            .await
            .expect("create session");
        manager
            .execute_action(&summary.session_id, BrowserActionRequest::Snapshot)
            .await
            .expect("snapshot");

        // 1. PressKey: Meta+A dispatches rawKeyDown + keyUp with selectAll command.
        manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::PressKey {
                    key: "Meta+A".into(),
                    modifiers: vec![],
                    ref_id: Some("e2".into()),
                },
            )
            .await
            .expect("press Meta+A");

        // Printable key on password field (`e3`) MUST be blocked by sensitive input guard.
        let pw_err = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::PressKey {
                    key: "a".into(),
                    modifiers: vec![],
                    ref_id: Some("e3".into()),
                },
            )
            .await
            .expect_err("printable press_key on password field must be blocked");
        assert!(pw_err.contains("sensitive"));

        // 2. SelectOption by value
        let sel_out = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::SelectOption {
                    ref_id: None,
                    selector: Some("select#plan".into()),
                    value: Some("pro".into()),
                    label: None,
                    index: None,
                },
            )
            .await
            .expect("select option");
        assert!(
            sel_out
                .text_excerpt
                .as_deref()
                .unwrap_or("")
                .contains("Pro Plan")
        );

        // 3. UploadFile: confined file invokes DOM.setFileInputFiles; escaping symlink fails BEFORE CDP.
        let up_out = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::UploadFile {
                    ref_id: None,
                    selector: Some("input[type=file]".into()),
                    files: vec!["assets/logo.png".into()],
                },
            )
            .await
            .expect("confined upload succeeds");
        assert!(
            up_out
                .text_excerpt
                .as_deref()
                .unwrap_or("")
                .contains("logo.png")
        );

        #[cfg(unix)]
        {
            let esc_err = manager
                .execute_action(
                    &summary.session_id,
                    BrowserActionRequest::UploadFile {
                        ref_id: None,
                        selector: Some("input[type=file]".into()),
                        files: vec!["assets/escape.png".into()],
                    },
                )
                .await
                .expect_err("escaping symlink upload must be rejected");
            assert!(esc_err.contains("escapes allowed workspace root"));
        }

        // 4. PickElement returns structured PickedElementInfo
        let pick_out = manager
            .execute_action(
                &summary.session_id,
                BrowserActionRequest::PickElement {
                    x: Some(50.0),
                    y: Some(120.0),
                    selector: None,
                    ref_id: None,
                    interactive: false,
                },
            )
            .await
            .expect("pick element");
        let picked = pick_out
            .picked_element
            .expect("picked_element must be populated");
        assert_eq!(picked.selector, "button#submit-order");
        assert_eq!(picked.tag, "button");
        assert_eq!(picked.text_preview, "Submit Order");
        assert_eq!(
            picked.attributes.get("data-testid").map(String::as_str),
            Some("submit-btn")
        );
        assert!(picked.computed_box.is_some());
        assert_eq!(picked.page_url, "http://127.0.0.1:5173/checkout");

        // Verify CDP commands sent to mock server.
        let cmds = server.received_commands.lock().await.clone();
        assert!(cmds.iter().any(|(m, p)| {
            m == "Input.dispatchKeyEvent"
                && p.get("type").and_then(Value::as_str) == Some("rawKeyDown")
                && p.get("code").and_then(Value::as_str) == Some("KeyA")
                && p.get("modifiers").and_then(Value::as_u64) == Some(4)
        }));
        let set_files_calls: Vec<&Value> = cmds
            .iter()
            .filter(|(m, _)| m == "DOM.setFileInputFiles")
            .map(|(_, p)| p)
            .collect();
        assert_eq!(set_files_calls.len(), 1);
        assert!(
            !cmds.iter().any(|(m, _)| m == "DOM.describeNode"),
            "redundant DOM.describeNode round trip must not be called"
        );
        assert_eq!(
            set_files_calls[0].get("objectId").and_then(Value::as_str),
            Some("obj_file_input_1")
        );
        assert!(set_files_calls[0].get("backendNodeId").is_none());

        // Verify UploadFile staged a private 0700/0600 snapshot copy inside
        // `$ORBIT_HOME/runtime/browser-uploads/<session_id>/` (zero TOCTOU symlink-swap window).
        let staged_path_str = set_files_calls[0]
            .get("files")
            .and_then(Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(Value::as_str)
            .expect("staged file path in DOM.setFileInputFiles");
        let staged_path = PathBuf::from(staged_path_str);
        let session_staging_dir = tmp_home
            .join("runtime")
            .join("browser-uploads")
            .join(&summary.session_id);
        assert!(
            staged_path.starts_with(&session_staging_dir),
            "staged file `{}` must live under `{}`",
            staged_path.display(),
            session_staging_dir.display()
        );
        assert_eq!(
            staged_path.file_name().and_then(|n| n.to_str()),
            Some("logo.png")
        );
        assert_eq!(
            std::fs::read(&staged_path).expect("read staged file"),
            b"PNG"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = std::fs::symlink_metadata(&session_staging_dir)
                .expect("stat staging dir")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(dir_mode, 0o700, "staging dir must be 0700");
            let file_mode = std::fs::symlink_metadata(&staged_path)
                .expect("stat staged file")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(file_mode, 0o600, "staged file must be 0600");
        }
        // Mutating/deleting workspace source AFTER staging must not corrupt the staged copy.
        std::fs::write(workspace.join("assets/logo.png"), b"MUTATED").expect("mutate src");
        assert_eq!(
            std::fs::read(&staged_path).expect("staged copy remains immutable snapshot"),
            b"PNG"
        );

        // Closing the browser session MUST delete `$ORBIT_HOME/runtime/browser-uploads/<session_id>/`.
        manager
            .close_session(&summary.session_id)
            .await
            .expect("close session");
        assert!(
            !session_staging_dir.exists(),
            "session upload staging dir must be removed on session close"
        );

        manager.shutdown_all().await;
        let _ = std::fs::remove_dir_all(&root);
    }
}
