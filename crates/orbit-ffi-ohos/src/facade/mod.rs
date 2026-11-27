mod automation;
mod cloud_sync;
mod composer;
mod files_workspace;
mod fleet;
mod git_terminal;
#[cfg(test)]
pub(crate) use git_terminal::validate_ohos_git_discard_path;
mod settings;
mod stage;

use orbit_client_core::app_core::OrbitAppCore;
use orbit_client_core::composer::ClientPlatform;
use orbit_client_core::sidebar::{SidebarProjectSort, SidebarThreadFilter};
use orbit_protocol::{PromptBadge, ProviderKind, ReasoningEffort, SessionMode};
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

struct OhosThreadWaker(std::thread::Thread);

impl Wake for OhosThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Polls an `OrbitAppCore` async RPC future on the calling thread without `unsafe` or a second
/// Tokio runtime.
///
/// # Precondition
/// Only pass **bounded** `OrbitAppCore` RPC futures that spawn their I/O work onto
/// `OrbitAppCore`'s internal Tokio runtime and await a bounded `oneshot` reply (with timeout).
/// Do **not** pass unbounded futures or futures that rely on an ambient Tokio reactor on the
/// calling thread, as `std::thread::park` will block the calling thread until the waker fires.
fn block_on_core_rpc<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(OhosThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut fut = Box::pin(future);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(val) => return val,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn parse_provider_str(raw: &str) -> ProviderKind {
    ProviderKind::from_slug(raw).unwrap_or(ProviderKind::Antigravity)
}

pub(crate) fn parse_mode_str(raw: &str) -> SessionMode {
    match raw.trim() {
        "default" | "" => SessionMode::Default,
        "auto" => SessionMode::Auto,
        "plan" => SessionMode::Plan,
        "accept_edits" => SessionMode::AcceptEdits,
        "full_access" => SessionMode::FullAccess,
        custom => SessionMode::Custom(custom.to_string()),
    }
}

fn parse_effort_opt(raw: Option<&str>) -> Option<ReasoningEffort> {
    raw.and_then(ReasoningEffort::from_slug)
}

fn effort_to_slug(effort: ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Off => "off",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Xhigh => "xhigh",
        ReasoningEffort::Max => "max",
        ReasoningEffort::Ultra => "ultra",
    }
}

fn parse_sidebar_sort(raw: Option<&str>) -> SidebarProjectSort {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "alphabetical" | "name" => SidebarProjectSort::Alphabetical,
        "manual" | "custom" => SidebarProjectSort::Manual,
        _ => SidebarProjectSort::Recent,
    }
}

fn parse_sidebar_filter(raw: Option<&str>) -> SidebarThreadFilter {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "active_unread" | "activeunread" | "unread" | "needs_attention" | "needsattention"
        | "active" | "running" => SidebarThreadFilter::ActiveUnread,
        _ => SidebarThreadFilter::All,
    }
}

fn parse_client_platform(raw: Option<&str>) -> ClientPlatform {
    match raw
        .unwrap_or("harmony")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "desktop" => ClientPlatform::Desktop,
        "ios" => ClientPlatform::Ios,
        "android" => ClientPlatform::Android,
        _ => ClientPlatform::Harmony,
    }
}

fn parse_string_vec_json(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<String>>(trimmed).map_err(|e| e.to_string())
}

fn parse_badges_json(raw: Option<&str>) -> Result<Vec<PromptBadge>, String> {
    let Some(trimmed) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Vec::new());
    };
    serde_json::from_str::<Vec<PromptBadge>>(trimmed).map_err(|e| e.to_string())
}

/// Pure-Rust JSON facade shared by both host unit tests and the HarmonyOS N-API wrapper.
#[derive(Clone)]
pub struct OhosCoreFacade {
    #[cfg(feature = "perf-hooks")]
    pub(crate) replay: Arc<std::sync::Mutex<crate::replay::Replay>>,
    pub(crate) inner: Arc<OrbitAppCore>,
}

impl OhosCoreFacade {
    fn from_core(inner: Arc<OrbitAppCore>) -> Self {
        Self {
            inner,
            #[cfg(feature = "perf-hooks")]
            replay: Default::default(),
        }
    }

    pub fn open(db_path: &str) -> Result<Self, String> {
        Self::open_with_options(db_path, false)
    }

    pub fn open_with_options(db_path: &str, auto_discover_local: bool) -> Result<Self, String> {
        let inner = OrbitAppCore::open(db_path, auto_discover_local).map_err(|e| e.to_string())?;
        Ok(Self::from_core(inner))
    }

    pub fn open_in_memory() -> Result<Self, String> {
        let inner = OrbitAppCore::open_in_memory().map_err(|e| e.to_string())?;
        Ok(Self::from_core(inner))
    }

    pub fn on_app_foreground(&self) {
        self.inner.on_app_foreground();
    }

    pub fn on_app_background(&self) {
        self.inner.on_app_background();
    }

    pub fn on_network_changed(&self) {
        self.inner.on_network_changed();
    }

    pub fn new_with_master_key(db_path: String, master_key_b64: String) -> Result<Self, String> {
        use base64::Engine as _;
        let master_key_b64 = zeroize::Zeroizing::new(master_key_b64);
        let bytes = zeroize::Zeroizing::new(
            base64::engine::general_purpose::STANDARD
                .decode(master_key_b64.as_str())
                .map_err(|_| "invalid master key base64")?,
        );
        let key: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| "expected a 32-byte master key")?;
        let key = zeroize::Zeroizing::new(key);
        Ok(Self::from_core(
            OrbitAppCore::open_with_master_key(db_path, Some(*key)).map_err(|e| e.to_string())?,
        ))
    }
}
