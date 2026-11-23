//! Pure-Rust Native Mobile Background Push Engine (APNs / FCM / HarmonyOS Push Kit).
//!
//! Architectural guarantees:
//! - **Zero-Knowledge Privacy Boundary**: by default (`include_message_preview == false`),
//!   dispatched push payloads carry ONLY minimal metadata (`event_kind`, `host_id`, `agent_id`,
//!   generic title/body, and `orbit://open?...` deep link) — NEVER leaking plaintext prompts,
//!   tool parameters, assistant replies, or source code to Apple/Google/Huawei push relays.
//! - **DNS-Pinned SSRF Hardening**: outbound dispatch requires `https://`, rejects embedded
//!   credentials and trailing-dot hosts, resolves DNS via timeout-bounded `tokio::net::lookup_host`,
//!   blocks all loopback/private/link-local/CGNAT/reserved/IPv6-ULA/NAT64/mapped/compat/6to4
//!   addresses, pins resolved sockets via `resolve_to_addrs`, disables proxies (`.no_proxy()`) and
//!   redirects (`Policy::none()`), bounds HTTP response reads, and scrubs raw tokens/URLs from all
//!   error paths.
//! - **Anti-Spam Cooldown, Quiet Hours & Stale-Token Pruning**: deduplicates repeated events per
//!   `(agent_id, event_kind)`, respects local quiet-hours windows including overnight wrap-around
//!   (`23:00 -> 07:00`), and automatically disables stale/unregistered tokens (`HTTP 410`,
//!   `UNREGISTERED`, `BadDeviceToken`, HMS `80300007`).

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, Timelike, Utc};
use orbit_protocol::{
    AgentStatus, ConditionalTimerState, PushDeviceRecord, PushDispatchPayload, PushEventKind,
    PushPlatform, PushPreferences, TimelinePayload, build_push_deep_link, is_in_quiet_hours,
};
use orbit_store::{OrbitStore, StoredPushDevice, StoredPushPreferences};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Semaphore};
use tokio::task::AbortHandle;
use tracing::warn;

use crate::manager::{AgentOrchestrator, DaemonBroadcast};

pub const MAX_PUSH_DEVICES: usize = 32;
const MAX_PUSH_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_DEVICE_ID_LEN: usize = 160;
const MAX_DEVICE_NAME_LEN: usize = 128;
const MAX_DEVICE_TOKEN_LEN: usize = 4096;
const MAX_GATEWAY_URL_LEN: usize = 1024;
const MAX_GATEWAY_AUTH_TOKEN_LEN: usize = 4096;
const MAX_PREVIEW_CHARS: usize = 160;
const MAX_COOLDOWN_ENTRIES: usize = 2048;
const MAX_AGENT_STATES: usize = 4096;
const PERMISSION_COOLDOWN_MS: u64 = 5_000;
const DEFAULT_EVENT_COOLDOWN_MS: u64 = 15_000;
const PUSH_HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_DISPATCHES: usize = 8;

/// Snapshot returned by push RPCs (`ListPushDevices`, `RegisterPushDevice`, etc.).
#[derive(Debug, Clone)]
pub struct PushSnapshotData {
    pub devices: Vec<PushDeviceRecord>,
    pub preferences: PushPreferences,
}

/// Result returned by `TestPushNotification`.
#[derive(Debug, Clone)]
pub struct PushTestResult {
    pub attempted: u32,
    pub delivered: u32,
    pub detail: String,
}

/// Classified push dispatch failure distinguishing transient network/gateway faults from
/// permanently invalid/unregistered device tokens that should be auto-disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushDispatchError {
    Transient(String),
    InvalidToken(String),
}

impl PushDispatchError {
    pub fn message(&self) -> &str {
        match self {
            Self::Transient(msg) | Self::InvalidToken(msg) => msg,
        }
    }
}

/// Pure-Rust Push Notification Manager owned by `DaemonRuntime`.
pub struct PushManager {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    host_id: String,
    host_name: String,
    agent_states: Arc<Mutex<HashMap<String, AgentStatus>>>,
    cooldowns: Arc<Mutex<HashMap<(String, PushEventKind), u64>>>,
    broadcast_task: Arc<Mutex<Option<AbortHandle>>>,
    dispatch_sem: Arc<Semaphore>,
    /// Test-only hook allowing local `http://127.0.0.1` mock push gateway dispatch in unit tests.
    /// Always `false` in production builds.
    allow_insecure_http_for_test: bool,
}

impl PushManager {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        host_id: String,
        host_name: String,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            host_id,
            host_name,
            agent_states: Arc::new(Mutex::new(HashMap::new())),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            broadcast_task: Arc::new(Mutex::new(None)),
            dispatch_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_DISPATCHES)),
            allow_insecure_http_for_test: false,
        })
    }

    #[cfg(test)]
    fn new_for_test(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        host_id: String,
        host_name: String,
        allow_insecure_http_for_test: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            host_id,
            host_name,
            agent_states: Arc::new(Mutex::new(HashMap::new())),
            cooldowns: Arc::new(Mutex::new(HashMap::new())),
            broadcast_task: Arc::new(Mutex::new(None)),
            dispatch_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_DISPATCHES)),
            allow_insecure_http_for_test,
        })
    }

    /// Seed initial agent statuses (preventing spurious restart notifications) and start
    /// the background `DaemonBroadcast` subscriber.
    pub async fn start(self: &Arc<Self>) {
        let store_for_seed = Arc::clone(&self.store);
        if let Ok(Ok(agents)) =
            tokio::task::spawn_blocking(move || store_for_seed.list_agents(None)).await
        {
            let mut states = self.agent_states.lock().await;
            for ag in agents.into_iter().take(MAX_AGENT_STATES) {
                states.insert(ag.agent_id, ag.status);
            }
        }
        let mut slot = self.broadcast_task.lock().await;
        if slot.is_none() {
            *slot = Some(self.spawn_broadcast_listener());
        }
    }

    /// Gracefully abort the background broadcast listener.
    pub async fn shutdown_all(&self) {
        if let Some(handle) = self.broadcast_task.lock().await.take() {
            handle.abort();
        }
    }

    /// Build a wire-safe snapshot of registered devices (tokens masked to `••••<last6>`)
    /// and current push preferences.
    pub fn list_snapshot(&self) -> Result<PushSnapshotData, String> {
        let devices = self
            .store
            .list_push_devices()
            .map_err(|e| format!("failed to list push devices: {e}"))?
            .into_iter()
            .map(|d| d.to_public_record())
            .collect();
        let preferences = self
            .store
            .get_push_preferences()
            .map_err(|e| format!("failed to load push preferences: {e}"))?
            .to_public_preferences();
        Ok(PushSnapshotData {
            devices,
            preferences,
        })
    }

    /// Register or update a mobile push device token.
    pub fn register_device(
        &self,
        device_id: String,
        platform: PushPlatform,
        device_name: String,
        device_token: String,
        sandbox: bool,
    ) -> Result<PushSnapshotData, String> {
        if contains_forbidden_control_chars(&device_id) {
            return Err("device_id must not contain control characters".to_string());
        }
        if contains_forbidden_control_chars(&device_token) {
            return Err("device_token must not contain control characters".to_string());
        }
        let clean_id = device_id.trim();
        if clean_id.is_empty() || clean_id.len() > MAX_DEVICE_ID_LEN {
            return Err("device_id must be 1..=160 characters".to_string());
        }
        let clean_token = device_token.trim();
        if clean_token.is_empty() || clean_token.len() > MAX_DEVICE_TOKEN_LEN {
            return Err("device_token must be non-empty and <= 4096 characters".to_string());
        }
        let clean_name = {
            let sanitized: String = device_name
                .chars()
                .filter(|c| !matches!(c, '\r' | '\n' | '\0'))
                .collect();
            let trimmed = sanitized.trim();
            if trimmed.is_empty() {
                platform.display_name().to_string()
            } else {
                truncate_chars(trimmed, MAX_DEVICE_NAME_LEN)
            }
        };

        let now = now_ms();
        let existing = self
            .store
            .get_push_device(clean_id)
            .map_err(|e| format!("failed to query push device: {e}"))?;

        if existing.is_none() {
            let current_count = self
                .store
                .count_push_devices()
                .map_err(|e| format!("failed to count push devices: {e}"))?;
            if current_count >= MAX_PUSH_DEVICES {
                return Err(format!(
                    "maximum registered push devices ({MAX_PUSH_DEVICES}) reached"
                ));
            }
        }

        let record = StoredPushDevice {
            device_id: clean_id.to_string(),
            platform,
            device_name: clean_name,
            device_token: clean_token.to_string(),
            sandbox,
            enabled: true,
            last_delivered_at_ms: existing.as_ref().and_then(|d| d.last_delivered_at_ms),
            last_error: None,
            registered_at_ms: existing.as_ref().map(|d| d.registered_at_ms).unwrap_or(now),
            updated_at_ms: now,
        };
        self.store
            .upsert_push_device(&record)
            .map_err(|e| format!("failed to save push device: {e}"))?;
        self.list_snapshot()
    }

    /// Unregister a mobile push device by ID.
    pub fn unregister_device(&self, device_id: &str) -> Result<PushSnapshotData, String> {
        let clean_id = device_id.trim();
        if clean_id.is_empty() {
            return Err("device_id must not be empty".to_string());
        }
        self.store
            .delete_push_device(clean_id)
            .map_err(|e| format!("failed to unregister push device: {e}"))?;
        self.list_snapshot()
    }

    /// Update push preferences and optionally rotate/clear the gateway auth token.
    pub fn update_preferences(
        &self,
        mut preferences: PushPreferences,
        gateway_auth_token: Option<String>,
        keep_existing_gateway_token: bool,
    ) -> Result<PushSnapshotData, String> {
        if preferences.quiet_start_hour > 23 || preferences.quiet_end_hour > 23 {
            return Err("quiet hours must be between 0 and 23".to_string());
        }

        if let Some(ref raw_tok) = gateway_auth_token {
            if contains_forbidden_control_chars(raw_tok) {
                return Err("gateway_auth_token must not contain control characters".to_string());
            }
            if raw_tok.trim().len() > MAX_GATEWAY_AUTH_TOKEN_LEN {
                return Err(format!(
                    "gateway_auth_token must be <= {MAX_GATEWAY_AUTH_TOKEN_LEN} bytes"
                ));
            }
        }

        preferences.push_gateway_url = preferences
            .push_gateway_url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty());

        if let Some(ref url_raw) = preferences.push_gateway_url {
            validate_push_gateway_url_syntax(url_raw, self.allow_insecure_http_for_test)?;
        }

        let existing = self
            .store
            .get_push_preferences()
            .map_err(|e| format!("failed to read existing push preferences: {e}"))?;

        let next_token = match gateway_auth_token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
        {
            Some(new_tok) => Some(new_tok),
            None if keep_existing_gateway_token => existing.gateway_auth_token,
            None => None,
        };

        preferences.has_gateway_auth_token = next_token.is_some();

        let stored = StoredPushPreferences {
            preferences,
            gateway_auth_token: next_token,
        };
        self.store
            .upsert_push_preferences(&stored, now_ms())
            .map_err(|e| format!("failed to save push preferences: {e}"))?;
        self.list_snapshot()
    }

    /// Dispatch a test notification to one specific device or all enabled registered devices.
    /// Explicitly bypasses quiet hours and cooldowns since the user triggered it manually.
    pub async fn test_push(&self, device_id: Option<&str>) -> PushTestResult {
        let store_for_load = Arc::clone(&self.store);
        let load_res = tokio::task::spawn_blocking(move || {
            let prefs = store_for_load.get_push_preferences()?;
            let devices = store_for_load.list_push_devices()?;
            Ok::<_, orbit_store::StoreError>((prefs, devices))
        })
        .await;

        let (stored_prefs, all_devices) = match load_res {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                return PushTestResult {
                    attempted: 0,
                    delivered: 0,
                    detail: format!("Failed to load push configuration: {e}"),
                };
            }
            Err(e) => {
                return PushTestResult {
                    attempted: 0,
                    delivered: 0,
                    detail: format!("Failed to load push configuration: {e}"),
                };
            }
        };

        let Some(gateway_url) = stored_prefs
            .preferences
            .push_gateway_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return PushTestResult {
                attempted: 0,
                delivered: 0,
                detail: "Configure a Push Gateway HTTPS URL before sending a test notification."
                    .to_string(),
            };
        };

        let targets: Vec<StoredPushDevice> =
            match device_id.map(str::trim).filter(|s| !s.is_empty()) {
                Some(target_id) => all_devices
                    .into_iter()
                    .filter(|d| d.device_id == target_id)
                    .collect(),
                None => all_devices.into_iter().filter(|d| d.enabled).collect(),
            };

        if targets.is_empty() {
            return PushTestResult {
                attempted: 0,
                delivered: 0,
                detail: "No matching registered mobile devices.".to_string(),
            };
        }

        // Build DNS-pinned client ONCE for this dispatch batch.
        let (client, parsed_url) = match build_pinned_push_client(
            gateway_url,
            self.allow_insecure_http_for_test,
            stored_prefs.gateway_auth_token.as_deref(),
        )
        .await
        {
            Ok(pair) => pair,
            Err(err) => {
                let ts = now_ms();
                let store_for_err = Arc::clone(&self.store);
                let device_ids: Vec<String> = targets.iter().map(|d| d.device_id.clone()).collect();
                let err_copy = err.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    for did in device_ids {
                        let _ = store_for_err.record_push_delivery_result(
                            &did,
                            None,
                            Some(&err_copy),
                            ts,
                        );
                    }
                })
                .await;
                return PushTestResult {
                    attempted: u32::try_from(targets.len()).unwrap_or(u32::MAX),
                    delivered: 0,
                    detail: format!("Delivered 0/{} device(s). Last error: {err}", targets.len()),
                };
            }
        };

        let payload = PushDispatchPayload {
            event_kind: PushEventKind::TurnCompleted,
            host_id: self.host_id.clone(),
            agent_id: None,
            title: format!("Orbit · {}", self.host_name),
            body: "Test notification from Orbit.".to_string(),
            deep_link: build_push_deep_link(&self.host_id, None),
        };

        let mut attempted: u32 = 0;
        let mut delivered: u32 = 0;
        let mut last_err: Option<String> = None;

        for device in &targets {
            attempted = attempted.saturating_add(1);
            let ts = now_ms();
            match dispatch_to_device(
                &client,
                &parsed_url,
                stored_prefs.gateway_auth_token.as_deref(),
                device,
                &payload,
            )
            .await
            {
                Ok(()) => {
                    delivered = delivered.saturating_add(1);
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ =
                            store_for_save.record_push_delivery_result(&did, Some(ts), None, ts);
                    })
                    .await;
                }
                Err(PushDispatchError::InvalidToken(reason)) => {
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let reason_copy = reason.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ =
                            store_for_save.disable_push_device_with_error(&did, &reason_copy, ts);
                    })
                    .await;
                    last_err = Some(reason);
                }
                Err(PushDispatchError::Transient(reason)) => {
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let reason_copy = reason.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = store_for_save.record_push_delivery_result(
                            &did,
                            None,
                            Some(&reason_copy),
                            ts,
                        );
                    })
                    .await;
                    last_err = Some(reason);
                }
            }
        }

        let detail = if delivered == attempted {
            format!("Delivered to {delivered}/{attempted} device(s).")
        } else if let Some(err) = last_err {
            format!("Delivered {delivered}/{attempted} device(s). Last error: {err}")
        } else {
            format!("Delivered {delivered}/{attempted} device(s).")
        };

        PushTestResult {
            attempted,
            delivered,
            detail,
        }
    }

    fn spawn_broadcast_listener(self: &Arc<Self>) -> AbortHandle {
        let mgr = Arc::clone(self);
        let mut rx = self.orchestrator.broadcast_tx.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(DaemonBroadcast::TimelineLive(ev)) => {
                        if let TimelinePayload::PermissionRequest { title, .. } = ev.payload {
                            mgr.schedule_event_dispatch(
                                PushEventKind::PermissionRequired,
                                Some(ev.agent_id),
                                Some(title),
                            );
                        }
                    }
                    Ok(DaemonBroadcast::AgentUpdated(summary)) => {
                        let prev = {
                            let mut states = mgr.agent_states.lock().await;
                            if !states.contains_key(&summary.agent_id)
                                && states.len() >= MAX_AGENT_STATES
                            {
                                states.retain(|_, st| !matches!(st, AgentStatus::Idle));
                                if states.len() >= MAX_AGENT_STATES {
                                    states.clear();
                                }
                            }
                            states.insert(summary.agent_id.clone(), summary.status.clone())
                        };
                        match (prev, &summary.status) {
                            (
                                Some(
                                    AgentStatus::Running { .. }
                                    | AgentStatus::AutonomousWakeup { .. }
                                    | AgentStatus::AwaitingPermission { .. },
                                ),
                                AgentStatus::Idle,
                            ) => {
                                let trimmed = summary.title.trim();
                                let preview = if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(format!("Completed: {trimmed}"))
                                };
                                mgr.schedule_event_dispatch(
                                    PushEventKind::TurnCompleted,
                                    Some(summary.agent_id),
                                    preview,
                                );
                            }
                            (prev_st, AgentStatus::Error { message, .. })
                                if !matches!(prev_st, Some(AgentStatus::Error { .. })) =>
                            {
                                let trimmed = summary.title.trim();
                                let preview = if !trimmed.is_empty() {
                                    Some(format!("Error in {trimmed}"))
                                } else if !message.trim().is_empty() {
                                    Some(message.trim().to_string())
                                } else {
                                    None
                                };
                                mgr.schedule_event_dispatch(
                                    PushEventKind::AgentError,
                                    Some(summary.agent_id),
                                    preview,
                                );
                            }
                            _ => {}
                        }
                    }
                    Ok(DaemonBroadcast::AgentDeleted(agent_id)) => {
                        mgr.agent_states.lock().await.remove(&agent_id);
                        let mut cd = mgr.cooldowns.lock().await;
                        cd.retain(|(aid, _), _| aid != &agent_id);
                    }
                    Ok(DaemonBroadcast::GoalRunEvent(goal)) => {
                        if goal.status.is_terminal() {
                            let preview =
                                format!("Goal {}: {}", goal.status.as_str(), goal.objective.trim());
                            mgr.schedule_event_dispatch(
                                PushEventKind::GoalFinished,
                                Some(goal.agent_id),
                                Some(preview),
                            );
                        }
                    }
                    Ok(DaemonBroadcast::ConditionalTimerEvent(timer)) => {
                        if timer.state == ConditionalTimerState::Fired {
                            let preview = format!("Timer fired: {}", timer.prompt.trim());
                            mgr.schedule_event_dispatch(
                                PushEventKind::TimerFired,
                                Some(timer.agent_id),
                                Some(preview),
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(skipped, "push manager broadcast listener lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        handle.abort_handle()
    }

    fn schedule_event_dispatch(
        self: &Arc<Self>,
        event_kind: PushEventKind,
        agent_id: Option<String>,
        optional_preview: Option<String>,
    ) {
        let mgr = Arc::clone(self);
        tokio::spawn(async move {
            let local_hour = u8::try_from(Local::now().hour()).unwrap_or(0);
            let _ = mgr
                .dispatch_event_at(
                    event_kind,
                    agent_id.as_deref(),
                    optional_preview.as_deref(),
                    local_hour,
                    now_ms(),
                )
                .await;
        });
    }

    /// Core push dispatch pipeline (factored for deterministic testing of quiet hours,
    /// cooldown deduplication, zero-knowledge payload omission, stale-token pruning, and HTTP delivery).
    pub async fn dispatch_event_at(
        &self,
        event_kind: PushEventKind,
        agent_id: Option<&str>,
        optional_preview: Option<&str>,
        local_hour: u8,
        timestamp_ms: u64,
    ) -> Result<u32, String> {
        let store_for_load = Arc::clone(&self.store);
        let (stored_prefs, all_devices) = tokio::task::spawn_blocking(move || {
            let prefs = store_for_load.get_push_preferences()?;
            let devices = store_for_load.list_push_devices()?;
            Ok::<_, orbit_store::StoreError>((prefs, devices))
        })
        .await
        .map_err(|e| format!("failed to load push configuration: {e}"))?
        .map_err(|e| format!("failed to load push configuration: {e}"))?;

        let prefs = &stored_prefs.preferences;

        if !prefs.allows_event(event_kind) {
            return Ok(0);
        }

        if prefs.quiet_hours_enabled
            && is_in_quiet_hours(local_hour, prefs.quiet_start_hour, prefs.quiet_end_hour)
        {
            return Ok(0);
        }

        let Some(gateway_url) = prefs
            .push_gateway_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return Ok(0);
        };

        let devices: Vec<StoredPushDevice> =
            all_devices.into_iter().filter(|d| d.enabled).collect();
        if devices.is_empty() {
            return Ok(0);
        }

        // Per-agent cooldown deduplication to prevent notification storms.
        let dedup_agent = agent_id.unwrap_or("_host").to_string();
        let cooldown_ms = match event_kind {
            PushEventKind::PermissionRequired => PERMISSION_COOLDOWN_MS,
            _ => DEFAULT_EVENT_COOLDOWN_MS,
        };
        {
            let mut cd = self.cooldowns.lock().await;
            let key = (dedup_agent, event_kind);
            if let Some(&prev_ms) = cd.get(&key)
                && timestamp_ms.saturating_sub(prev_ms) < cooldown_ms
            {
                return Ok(0);
            }
            if cd.len() >= MAX_COOLDOWN_ENTRIES {
                cd.retain(|_, &mut ts| timestamp_ms.saturating_sub(ts) < cooldown_ms * 4);
                if cd.len() >= MAX_COOLDOWN_ENTRIES {
                    cd.clear();
                }
            }
            cd.insert(key, timestamp_ms);
        }

        let payload = build_dispatch_payload(
            event_kind,
            &self.host_id,
            &self.host_name,
            agent_id,
            prefs.include_message_preview,
            optional_preview,
        );

        let _permit = self
            .dispatch_sem
            .acquire()
            .await
            .map_err(|_| "push dispatcher semaphore closed".to_string())?;

        // Build DNS-pinned client ONCE for all enabled devices in this dispatch batch.
        let (client, parsed_url) = build_pinned_push_client(
            gateway_url,
            self.allow_insecure_http_for_test,
            stored_prefs.gateway_auth_token.as_deref(),
        )
        .await?;

        let mut delivered: u32 = 0;
        for device in &devices {
            match dispatch_to_device(
                &client,
                &parsed_url,
                stored_prefs.gateway_auth_token.as_deref(),
                device,
                &payload,
            )
            .await
            {
                Ok(()) => {
                    delivered = delivered.saturating_add(1);
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = store_for_save.record_push_delivery_result(
                            &did,
                            Some(timestamp_ms),
                            None,
                            timestamp_ms,
                        );
                    })
                    .await;
                }
                Err(PushDispatchError::InvalidToken(reason)) => {
                    warn!(
                        device_id = %device.device_id,
                        platform = %device.platform.slug(),
                        error = %reason,
                        "disabling stale/unregistered mobile push device token"
                    );
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let reason_copy = reason.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = store_for_save.disable_push_device_with_error(
                            &did,
                            &reason_copy,
                            timestamp_ms,
                        );
                    })
                    .await;
                }
                Err(PushDispatchError::Transient(reason)) => {
                    warn!(
                        device_id = %device.device_id,
                        platform = %device.platform.slug(),
                        error = %reason,
                        "mobile push delivery failed"
                    );
                    let store_for_save = Arc::clone(&self.store);
                    let did = device.device_id.clone();
                    let reason_copy = reason.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = store_for_save.record_push_delivery_result(
                            &did,
                            None,
                            Some(&reason_copy),
                            timestamp_ms,
                        );
                    })
                    .await;
                }
            }
        }

        Ok(delivered)
    }
}

fn contains_forbidden_control_chars(raw: &str) -> bool {
    raw.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0)
}

/// Scrub raw device tokens and gateway Bearer auth tokens from any outbound error or response
/// text before logging, persisting to SQLite, or returning over RPC.
pub fn redact_push_secrets(
    text: &str,
    device_token: Option<&str>,
    auth_token: Option<&str>,
) -> String {
    let mut out = text.to_string();
    for secret in [device_token, auth_token].into_iter().flatten() {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            out = out.replace(trimmed, "[REDACTED]");
        }
    }
    out
}

/// Format a `reqwest::Error` safely without leaking URL query/path secrets or raw push tokens.
pub fn format_reqwest_err(
    prefix: &str,
    err: reqwest::Error,
    device_token: Option<&str>,
    auth_token: Option<&str>,
) -> String {
    let raw = format!("{prefix}: {}", err.without_url());
    redact_push_secrets(&raw, device_token, auth_token)
}

/// Classify a non-2xx push gateway / FCM / APNs / HarmonyOS Push Kit HTTP response into either
/// `InvalidToken` (device token permanently dead -> disable device) or `Transient`.
pub fn classify_push_http_error(
    status: reqwest::StatusCode,
    redacted_snippet: &str,
) -> PushDispatchError {
    let message = if redacted_snippet.is_empty() {
        format!("push gateway returned HTTP {}", status.as_u16())
    } else {
        format!(
            "push gateway returned HTTP {}: {redacted_snippet}",
            status.as_u16()
        )
    };

    let lower = redacted_snippet.to_ascii_lowercase();
    let is_invalid_token = status == reqwest::StatusCode::GONE
        || (status == reqwest::StatusCode::NOT_FOUND && lower.contains("unregistered"))
        || redacted_snippet.contains("Unregistered")
        || redacted_snippet.contains("BadDeviceToken")
        || redacted_snippet.contains("80300007");

    if is_invalid_token {
        PushDispatchError::InvalidToken(message)
    } else {
        PushDispatchError::Transient(message)
    }
}

/// Construct a push notification payload strictly honoring the zero-knowledge privacy boundary.
///
/// When `include_message_preview == false`, `optional_preview` is completely ignored and
/// NEVER included anywhere in `title`, `body`, or `deep_link`.
pub fn build_dispatch_payload(
    event_kind: PushEventKind,
    host_id: &str,
    host_name: &str,
    agent_id: Option<&str>,
    include_message_preview: bool,
    optional_preview: Option<&str>,
) -> PushDispatchPayload {
    let clean_host_name = {
        let trimmed = host_name.trim();
        if trimmed.is_empty() { "orbit" } else { trimmed }
    };
    let title = format!("{} · {clean_host_name}", event_kind.minimal_title_prefix());
    let body = if include_message_preview {
        match optional_preview
            .map(sanitize_preview_text)
            .filter(|s| !s.is_empty())
        {
            Some(preview) => preview,
            None => event_kind.minimal_body().to_string(),
        }
    } else {
        event_kind.minimal_body().to_string()
    };

    PushDispatchPayload {
        event_kind,
        host_id: host_id.to_string(),
        agent_id: agent_id.map(str::to_string),
        title,
        body,
        deep_link: build_push_deep_link(host_id, agent_id),
    }
}

fn sanitize_preview_text(raw: &str) -> String {
    let single_line: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&single_line, MAX_PREVIEW_CHARS)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    let mut chars = raw.chars();
    let taken: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{taken}…")
    } else {
        taken
    }
}

/// Validate a push gateway URL syntactically (and check IP literals immediately) before saving.
pub fn validate_push_gateway_url_syntax(
    url_raw: &str,
    allow_insecure_http_for_test: bool,
) -> Result<reqwest::Url, String> {
    let trimmed = url_raw.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_GATEWAY_URL_LEN {
        return Err("Push gateway URL must be 1..=1024 characters".to_string());
    }
    let parsed =
        reqwest::Url::parse(trimmed).map_err(|_| "Invalid push gateway URL".to_string())?;
    let scheme = parsed.scheme();
    if scheme != "https" && !(allow_insecure_http_for_test && scheme == "http") {
        return Err("Push gateway URL must use https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Push gateway URL must not embed credentials".to_string());
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Push gateway URL is missing host".to_string())?;
    if host_raw.ends_with('.') {
        return Err("Push gateway host must not have a trailing dot".to_string());
    }
    if !allow_insecure_http_for_test {
        let host_norm = host_raw.trim_matches(['[', ']']).to_ascii_lowercase();
        if host_norm == "localhost" || host_norm.ends_with(".localhost") {
            return Err("Refusing loopback push gateway host".to_string());
        }
        if let Ok(ip) = host_norm.parse::<IpAddr>()
            && is_blocked_push_ssrf_ip(ip)
        {
            return Err(format!("Refusing non-public push gateway IP `{ip}`"));
        }
    }
    Ok(parsed)
}

/// Comprehensive SSRF IP filter blocking loopback, RFC1918 private, link-local (AWS/GCP/Azure
/// metadata `169.254.169.254`), CGNAT (`100.64.0.0/10`), `0.0.0.0/8`, benchmark (`198.18.0.0/15`),
/// reserved (`240.0.0.0/4`), multicast, broadcast, IPv6 ULA (`fc00::/7`), link-local (`fe80::/10`),
/// IPv4-mapped (`::ffff:0:0/96`), IPv4-compatible (`::/96`), NAT64 (`64:ff9b::/96`,
/// `64:ff9b:1::/48`), and 6to4 (`2002::/16`) non-public addresses.
pub fn is_blocked_push_ssrf_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0b1100_0000) == 64)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_push_ssrf_ip(IpAddr::V4(mapped));
            }
            let segs = v6.segments();
            // IPv4-compatible ::a.b.c.d (excluding :: and ::1 handled below)
            if segs[0..6] == [0, 0, 0, 0, 0, 0] && !(segs[6] == 0 && segs[7] <= 1) {
                let compat = Ipv4Addr::new(
                    (segs[6] >> 8) as u8,
                    (segs[6] & 0xff) as u8,
                    (segs[7] >> 8) as u8,
                    (segs[7] & 0xff) as u8,
                );
                if is_blocked_push_ssrf_ip(IpAddr::V4(compat)) {
                    return true;
                }
            }
            // NAT64 well-known prefix 64:ff9b::/96 or local-use 64:ff9b:1::/48
            if segs[0] == 0x0064 && segs[1] == 0xff9b {
                let nat64 = Ipv4Addr::new(
                    (segs[6] >> 8) as u8,
                    (segs[6] & 0xff) as u8,
                    (segs[7] >> 8) as u8,
                    (segs[7] & 0xff) as u8,
                );
                if segs[2] == 0x0001 || is_blocked_push_ssrf_ip(IpAddr::V4(nat64)) {
                    return true;
                }
            }
            // 6to4 prefix 2002::/16 embeds IPv4 in segments 1..3
            if segs[0] == 0x2002 {
                let six_to_four = Ipv4Addr::new(
                    (segs[1] >> 8) as u8,
                    (segs[1] & 0xff) as u8,
                    (segs[2] >> 8) as u8,
                    (segs[2] & 0xff) as u8,
                );
                if is_blocked_push_ssrf_ip(IpAddr::V4(six_to_four)) {
                    return true;
                }
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (segs[0] & 0xfe00) == 0xfc00
                || (segs[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Build a DNS-pinned, `.no_proxy()`, `Policy::none()`, timeout-bounded HTTP client for push
/// gateway dispatch. Intended to be constructed ONCE per dispatch batch and reused across devices.
pub async fn build_pinned_push_client(
    url_raw: &str,
    allow_insecure_http_for_test: bool,
    auth_token: Option<&str>,
) -> Result<(reqwest::Client, reqwest::Url), String> {
    let parsed = validate_push_gateway_url_syntax(url_raw, allow_insecure_http_for_test)?;
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "Push gateway URL missing host".to_string())?;
    let host_norm = host_raw.trim_matches(['[', ']']).to_ascii_lowercase();
    let port = parsed
        .port_or_known_default()
        .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });

    let lookup_res = tokio::time::timeout(
        PUSH_HTTP_TIMEOUT,
        tokio::net::lookup_host((host_norm.as_str(), port)),
    )
    .await
    .map_err(|_| format!("DNS resolution timed out for `{host_norm}`"))?
    .map_err(|e| format!("DNS resolution failed for `{host_norm}`: {e}"))?;

    let addrs: Vec<SocketAddr> = lookup_res.collect();
    if addrs.is_empty() {
        return Err(format!("DNS returned no addresses for `{host_norm}`"));
    }
    if !allow_insecure_http_for_test {
        for addr in &addrs {
            if is_blocked_push_ssrf_ip(addr.ip()) {
                return Err(format!(
                    "Refusing push gateway `{host_norm}` resolved to non-public IP `{}`",
                    addr.ip()
                ));
            }
        }
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PUSH_HTTP_TIMEOUT)
        .resolve_to_addrs(&host_norm, &addrs)
        .build()
        .map_err(|e| {
            format_reqwest_err(
                "Failed to construct pinned push HTTP client",
                e,
                None,
                auth_token,
            )
        })?;

    Ok((client, parsed))
}

/// Format the JSON body for either:
/// 1. Direct FCM HTTP v1 endpoint (`https://fcm.googleapis.com/v1/projects/.../messages:send`)
/// 2. Direct HarmonyOS Push Kit REST endpoint (`https://push-api.cloud.huawei.com/v1/.../messages:send`)
/// 3. Orbit Zero-Knowledge Push Relay Gateway (handling APNs, FCM, and HarmonyOS Push Kit)
pub fn build_gateway_request_body(
    gateway_url: &reqwest::Url,
    device: &StoredPushDevice,
    payload: &PushDispatchPayload,
) -> Value {
    let host = gateway_url
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let path = gateway_url.path();

    // Direct Google FCM HTTP v1 API
    if device.platform == PushPlatform::Fcm
        && host == "fcm.googleapis.com"
        && path.ends_with("/messages:send")
    {
        return json!({
            "message": {
                "token": device.device_token,
                "notification": {
                    "title": payload.title,
                    "body": payload.body,
                },
                "data": {
                    "event_kind": payload.event_kind.slug(),
                    "host_id": payload.host_id,
                    "agent_id": payload.agent_id.as_deref().unwrap_or(""),
                    "deep_link": payload.deep_link,
                },
                "android": {
                    "priority": "HIGH",
                }
            }
        });
    }

    // Direct Huawei HarmonyOS Push Kit REST API
    if device.platform == PushPlatform::HarmonyPushKit
        && host == "push-api.cloud.huawei.com"
        && path.ends_with("/messages:send")
    {
        let data_str = json!({
            "event_kind": payload.event_kind.slug(),
            "host_id": payload.host_id,
            "agent_id": payload.agent_id.as_deref().unwrap_or(""),
            "deep_link": payload.deep_link,
        })
        .to_string();
        return json!({
            "validate_only": false,
            "message": {
                "token": [device.device_token],
                "notification": {
                    "title": payload.title,
                    "body": payload.body,
                },
                "android": {
                    "urgency": "HIGH",
                    "notification": {
                        "title": payload.title,
                        "body": payload.body,
                        "click_action": {
                            "type": 1,
                            "intent": payload.deep_link,
                        }
                    }
                },
                "data": data_str,
            }
        });
    }

    // Standard Orbit Zero-Knowledge Push Gateway envelope (APNs / FCM / HarmonyOS Push Kit)
    json!({
        "protocol_version": 1,
        "platform": device.platform.slug(),
        "device_id": device.device_id,
        "device_token": device.device_token,
        "sandbox": device.sandbox,
        "payload": payload,
    })
}

async fn dispatch_to_device(
    client: &reqwest::Client,
    parsed_url: &reqwest::Url,
    auth_token: Option<&str>,
    device: &StoredPushDevice,
    payload: &PushDispatchPayload,
) -> Result<(), PushDispatchError> {
    let body = build_gateway_request_body(parsed_url, device, payload);
    let dev_tok = Some(device.device_token.as_str());

    let mut req = client
        .post(parsed_url.clone())
        .header("content-type", "application/json")
        .header(
            "user-agent",
            format!("orbit-daemon/{}", env!("CARGO_PKG_VERSION")),
        )
        .json(&body);

    if let Some(tok) = auth_token.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.bearer_auth(tok);
    }

    let mut resp = req.send().await.map_err(|e| {
        PushDispatchError::Transient(format_reqwest_err(
            "push gateway request failed",
            e,
            dev_tok,
            auth_token,
        ))
    })?;
    let status = resp.status();

    // Bounded response body read (prevents OOM from malicious/misconfigured gateways).
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| {
        PushDispatchError::Transient(format_reqwest_err(
            "failed reading push gateway response",
            e,
            dev_tok,
            auth_token,
        ))
    })? {
        let remaining = MAX_PUSH_RESPONSE_BYTES.saturating_sub(buf.len());
        if remaining == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }

    if status.is_success() {
        Ok(())
    } else {
        let snippet = String::from_utf8_lossy(&buf);
        let redacted = redact_push_secrets(snippet.trim(), dev_tok, auth_token);
        let clean_snippet = truncate_chars(&redacted, 200);
        Err(classify_push_http_error(status, &clean_snippet))
    }
}

fn now_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::State, http::HeaderMap, http::StatusCode, routing::post};
    use tokio::net::TcpListener;

    use crate::hooks::HooksEngine;
    use crate::keep_awake::KeepAwakeManager;
    use crate::workspace_runtime::WorkspaceScriptRegistry;

    async fn make_test_orchestrator(
        store: Arc<OrbitStore>,
        home_dir: &std::path::Path,
    ) -> Arc<AgentOrchestrator> {
        let scripts = WorkspaceScriptRegistry::new();
        let keep_awake = Arc::new(KeepAwakeManager::new(false));
        let hooks = HooksEngine::new(home_dir.to_path_buf());
        let listen_port = Arc::new(tokio::sync::RwLock::new(7788));
        let orch_config = Arc::new(tokio::sync::RwLock::new(
            orbit_protocol::OrchestrationConfig::default(),
        ));
        let provider_catalogs = Arc::new(tokio::sync::RwLock::new(None));
        AgentOrchestrator::new(
            home_dir.to_path_buf(),
            "host_test_1".to_string(),
            "test_mcp_tok".to_string(),
            listen_port,
            orch_config,
            store,
            scripts,
            keep_awake,
            provider_catalogs,
            hooks,
        )
        .await
    }

    #[test]
    fn zero_knowledge_payload_omits_plaintext_preview_by_default() {
        let secret_prompt = "SECRET_API_KEY=sk-live-99999 refactor src/payments.rs";
        let zk = build_dispatch_payload(
            PushEventKind::PermissionRequired,
            "host_devbox",
            "devbox-01",
            Some("ag_7"),
            false,
            Some(secret_prompt),
        );
        assert_eq!(zk.event_kind, PushEventKind::PermissionRequired);
        assert_eq!(zk.host_id, "host_devbox");
        assert_eq!(zk.agent_id.as_deref(), Some("ag_7"));
        assert_eq!(zk.title, "Approval required · devbox-01");
        assert_eq!(zk.deep_link, "orbit://open?host=host_devbox&agent=ag_7");
        assert!(!zk.title.contains("SECRET_API_KEY"));
        assert!(!zk.body.contains("SECRET_API_KEY"));
        assert!(!zk.body.contains("payments.rs"));

        // Opt-in preview includes sanitized summary.
        let with_preview = build_dispatch_payload(
            PushEventKind::PermissionRequired,
            "host_devbox",
            "devbox-01",
            Some("ag_7"),
            true,
            Some("Run cargo   test\nin workspace"),
        );
        assert_eq!(with_preview.body, "Run cargo test in workspace");
    }

    #[test]
    fn quiet_hours_wraparound_and_daytime_math() {
        // Wrap-around 23:00 -> 07:00
        assert!(is_in_quiet_hours(23, 23, 7));
        assert!(is_in_quiet_hours(0, 23, 7));
        assert!(is_in_quiet_hours(6, 23, 7));
        assert!(!is_in_quiet_hours(7, 23, 7));
        assert!(!is_in_quiet_hours(22, 23, 7));

        // Daytime 09:00 -> 17:00
        assert!(!is_in_quiet_hours(8, 9, 17));
        assert!(is_in_quiet_hours(9, 9, 17));
        assert!(is_in_quiet_hours(16, 9, 17));
        assert!(!is_in_quiet_hours(17, 9, 17));
    }

    #[test]
    fn ssrf_guard_blocks_loopback_private_metadata_ipv6_and_insecure_urls() {
        let blocked_urls = [
            "http://push.example.com/v1/send",
            "https://user:pass@push.example.com/v1/send",
            "https://push.example.com./v1/send",
            "https://localhost/v1/send",
            "https://sub.localhost/v1/send",
            "https://127.0.0.1/v1/send",
            "https://10.0.0.5/v1/send",
            "https://172.16.0.1/v1/send",
            "https://192.168.1.10/v1/send",
            "https://169.254.169.254/latest/meta-data/",
            "https://100.64.0.1/v1/send",
            "https://[::1]/v1/send",
            "https://[fc00::1]/v1/send",
            "https://[fe80::1]/v1/send",
            "https://[::ffff:169.254.169.254]/v1/send",
            "https://[64:ff9b::169.254.169.254]/v1/send",
            "https://[2002:a9fe:a9fe::1]/v1/send",
        ];
        for url in blocked_urls {
            assert!(
                validate_push_gateway_url_syntax(url, false).is_err(),
                "expected SSRF / invalid push gateway URL `{url}` to be rejected"
            );
        }

        // Public HTTPS URL passes syntactic check
        assert!(
            validate_push_gateway_url_syntax("https://push.orbit.example/v1/dispatch", false)
                .is_ok()
        );
        assert!(!is_blocked_push_ssrf_ip(
            "93.184.216.34".parse().expect("public ipv4")
        ));
    }

    #[test]
    fn direct_fcm_and_harmony_payload_formats_match_platform_specs() {
        let payload = build_dispatch_payload(
            PushEventKind::GoalFinished,
            "host_1",
            "mac-studio",
            Some("ag_99"),
            false,
            Some("never leaked"),
        );

        let fcm_dev = StoredPushDevice {
            device_id: "android_1".into(),
            platform: PushPlatform::Fcm,
            device_name: "Pixel".into(),
            device_token: "fcm_tok_abcdef123456".into(),
            sandbox: false,
            enabled: true,
            last_delivered_at_ms: None,
            last_error: None,
            registered_at_ms: 1,
            updated_at_ms: 1,
        };
        let fcm_url =
            reqwest::Url::parse("https://fcm.googleapis.com/v1/projects/orbit-prod/messages:send")
                .expect("fcm url");
        let fcm_body = build_gateway_request_body(&fcm_url, &fcm_dev, &payload);
        assert_eq!(fcm_body["message"]["token"], "fcm_tok_abcdef123456");
        assert_eq!(fcm_body["message"]["data"]["host_id"], "host_1");
        assert_eq!(
            fcm_body["message"]["data"]["deep_link"],
            "orbit://open?host=host_1&agent=ag_99"
        );
        assert!(!fcm_body.to_string().contains("never leaked"));

        let hms_dev = StoredPushDevice {
            device_id: "ohos_1".into(),
            platform: PushPlatform::HarmonyPushKit,
            device_name: "Mate 70".into(),
            device_token: "hms_tok_abcdef654321".into(),
            sandbox: false,
            enabled: true,
            last_delivered_at_ms: None,
            last_error: None,
            registered_at_ms: 1,
            updated_at_ms: 1,
        };
        let hms_url =
            reqwest::Url::parse("https://push-api.cloud.huawei.com/v1/123456/messages:send")
                .expect("hms url");
        let hms_body = build_gateway_request_body(&hms_url, &hms_dev, &payload);
        assert_eq!(hms_body["message"]["token"][0], "hms_tok_abcdef654321");
        assert_eq!(
            hms_body["message"]["android"]["notification"]["click_action"]["intent"],
            "orbit://open?host=host_1&agent=ag_99"
        );
        assert!(!hms_body.to_string().contains("never leaked"));
    }

    #[tokio::test]
    async fn error_redaction_scrubs_device_token_auth_token_and_url_secrets() {
        let dev_secret = "apns_raw_device_secret_987654321";
        let auth_secret = "orbit_gateway_bearer_secret_xyz123";

        let scrubbed = redact_push_secrets(
            &format!("rejected token={dev_secret} bearer={auth_secret}"),
            Some(dev_secret),
            Some(auth_secret),
        );
        assert!(!scrubbed.contains(dev_secret));
        assert!(!scrubbed.contains(auth_secret));
        assert_eq!(scrubbed, "rejected token=[REDACTED] bearer=[REDACTED]");

        // Verify reqwest errors strip URL query secrets and redact tokens.
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(50))
            .build()
            .expect("client");
        let err = client
            .post(format!(
                "http://127.0.0.1:1/push?token={dev_secret}&auth={auth_secret}"
            ))
            .send()
            .await
            .expect_err("connection refused");
        let formatted = format_reqwest_err(
            "push gateway request failed",
            err,
            Some(dev_secret),
            Some(auth_secret),
        );
        assert!(!formatted.contains(dev_secret));
        assert!(!formatted.contains(auth_secret));
        assert!(!formatted.contains("?token="));

        // Verify non-2xx gateway response echoing both raw secrets is scrubbed before persistence & RPC return.
        async fn echo_error_handler(
            headers: HeaderMap,
            axum::Json(body): axum::Json<Value>,
        ) -> (StatusCode, String) {
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            let tok = body["device_token"].as_str().unwrap_or("");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("upstream failed for token {tok} with header {auth}"),
            )
        }

        let app = Router::new().route("/v1/fail", post(echo_error_handler));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp_home =
            std::env::temp_dir().join(format!("orbit-push-redact-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = make_test_orchestrator(Arc::clone(&store), &tmp_home).await;
        let mgr = PushManager::new_for_test(
            Arc::clone(&store),
            orch,
            "host_redact".into(),
            "redact-box".into(),
            true,
        );

        mgr.register_device(
            "ios_redact".into(),
            PushPlatform::Apns,
            "iPhone".into(),
            dev_secret.into(),
            false,
        )
        .expect("register");
        mgr.update_preferences(
            PushPreferences {
                push_gateway_url: Some(format!("http://{addr}/v1/fail")),
                ..PushPreferences::default()
            },
            Some(auth_secret.into()),
            false,
        )
        .expect("update prefs");

        let test_res = mgr.test_push(Some("ios_redact")).await;
        assert_eq!(test_res.attempted, 1);
        assert_eq!(test_res.delivered, 0);
        assert!(!test_res.detail.contains(dev_secret));
        assert!(!test_res.detail.contains(auth_secret));
        assert!(test_res.detail.contains("[REDACTED]"));

        let snap = mgr.list_snapshot().expect("snap");
        let persisted_err = snap.devices[0].last_error.as_deref().unwrap_or("");
        assert!(!persisted_err.contains(dev_secret));
        assert!(!persisted_err.contains(auth_secret));
        assert!(persisted_err.contains("[REDACTED]"));

        server_task.abort();
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn register_device_enforces_max_cap_and_rejects_control_chars() {
        let tmp_home =
            std::env::temp_dir().join(format!("orbit-push-cap-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = make_test_orchestrator(Arc::clone(&store), &tmp_home).await;
        let mgr = PushManager::new_for_test(
            Arc::clone(&store),
            orch,
            "host_cap".into(),
            "cap-box".into(),
            false,
        );

        // Control chars rejected in device_id, device_token, and gateway_auth_token.
        assert!(
            mgr.register_device(
                "dev\nid".into(),
                PushPlatform::Apns,
                "iPhone".into(),
                "valid_token_123456".into(),
                false,
            )
            .is_err()
        );
        assert!(
            mgr.register_device(
                "dev_id".into(),
                PushPlatform::Apns,
                "iPhone".into(),
                "bad\r\ntoken".into(),
                false,
            )
            .is_err()
        );
        assert!(
            mgr.register_device(
                "dev_id".into(),
                PushPlatform::Apns,
                "iPhone".into(),
                "bad\0token".into(),
                false,
            )
            .is_err()
        );
        assert!(
            mgr.update_preferences(
                PushPreferences::default(),
                Some("bearer\r\nX-Injected: 1".into()),
                false,
            )
            .is_err()
        );
        assert!(
            mgr.update_preferences(
                PushPreferences::default(),
                Some("a".repeat(MAX_GATEWAY_AUTH_TOKEN_LEN + 1)),
                false,
            )
            .is_err()
        );

        // Register up to MAX_PUSH_DEVICES (32) devices.
        for idx in 0..MAX_PUSH_DEVICES {
            mgr.register_device(
                format!("dev_{idx:02}"),
                PushPlatform::Fcm,
                format!("Phone {idx}"),
                format!("fcm_token_number_{idx:02}_abcdef"),
                false,
            )
            .expect("register within cap");
        }

        // Updating an existing device at cap still succeeds.
        mgr.register_device(
            "dev_00".into(),
            PushPlatform::Fcm,
            "Phone 0 Updated".into(),
            "fcm_token_number_00_rotated".into(),
            false,
        )
        .expect("update existing at cap");

        // Registering a 33rd new device is rejected.
        let overflow_err = mgr
            .register_device(
                "dev_overflow".into(),
                PushPlatform::Apns,
                "Overflow".into(),
                "apns_token_overflow_999999".into(),
                false,
            )
            .expect_err("33rd device must be rejected");
        assert!(overflow_err.contains(&MAX_PUSH_DEVICES.to_string()));

        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn invalid_token_responses_classify_and_disable_stale_device() {
        // Classification checks across APNs, FCM, and HarmonyOS Push Kit.
        assert!(matches!(
            classify_push_http_error(reqwest::StatusCode::GONE, "gone"),
            PushDispatchError::InvalidToken(_)
        ));
        assert!(matches!(
            classify_push_http_error(
                reqwest::StatusCode::NOT_FOUND,
                r#"{"error":{"details":[{"errorCode":"UNREGISTERED"}]}}"#
            ),
            PushDispatchError::InvalidToken(_)
        ));
        assert!(matches!(
            classify_push_http_error(
                reqwest::StatusCode::BAD_REQUEST,
                r#"{"reason":"BadDeviceToken"}"#
            ),
            PushDispatchError::InvalidToken(_)
        ));
        assert!(matches!(
            classify_push_http_error(
                reqwest::StatusCode::BAD_REQUEST,
                r#"{"reason":"Unregistered"}"#
            ),
            PushDispatchError::InvalidToken(_)
        ));
        assert!(matches!(
            classify_push_http_error(reqwest::StatusCode::OK, r#"{"code":"80300007"}"#),
            PushDispatchError::InvalidToken(_)
        ));
        assert!(matches!(
            classify_push_http_error(reqwest::StatusCode::SERVICE_UNAVAILABLE, "try again later"),
            PushDispatchError::Transient(_)
        ));

        // Live mock HTTP 410 auto-disables the stale device so subsequent dispatches skip it.
        let hit_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let hits_for_route = Arc::clone(&hit_count);
        let app = Router::new().route(
            "/v1/gone",
            post(move || {
                let hits = Arc::clone(&hits_for_route);
                async move {
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    (StatusCode::GONE, r#"{"reason":"Unregistered"}"#)
                }
            }),
        );
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp_home =
            std::env::temp_dir().join(format!("orbit-push-gone-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = make_test_orchestrator(Arc::clone(&store), &tmp_home).await;
        let mgr = PushManager::new_for_test(
            Arc::clone(&store),
            orch,
            "host_gone".into(),
            "gone-box".into(),
            true,
        );

        mgr.register_device(
            "ios_stale".into(),
            PushPlatform::Apns,
            "Old iPhone".into(),
            "apns_stale_token_123456".into(),
            false,
        )
        .expect("register");
        mgr.update_preferences(
            PushPreferences {
                push_gateway_url: Some(format!("http://{addr}/v1/gone")),
                ..PushPreferences::default()
            },
            None,
            false,
        )
        .expect("prefs");

        // First dispatch hits 410 and automatically disables `ios_stale`.
        let first = mgr
            .dispatch_event_at(
                PushEventKind::TurnCompleted,
                Some("ag_1"),
                None,
                12,
                1_760_000_000_000,
            )
            .await
            .expect("first dispatch");
        assert_eq!(first, 0);
        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        let snap = mgr.list_snapshot().expect("snap");
        assert!(
            !snap.devices[0].enabled,
            "stale token must be auto-disabled"
        );
        assert!(
            snap.devices[0]
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("410"))
        );

        // Second dispatch (after cooldown) skips the disabled device completely (0 extra HTTP calls).
        let second = mgr
            .dispatch_event_at(
                PushEventKind::TurnCompleted,
                Some("ag_1"),
                None,
                12,
                1_760_000_030_000,
            )
            .await
            .expect("second dispatch");
        assert_eq!(second, 0);
        assert_eq!(hit_count.load(std::sync::atomic::Ordering::SeqCst), 1);

        server_task.abort();
        let _ = std::fs::remove_dir_all(&tmp_home);
    }

    #[tokio::test]
    async fn mock_http_push_gateway_dispatches_zero_knowledge_envelope_and_honors_quiet_hours_and_cooldown()
     {
        type CapturedPushRequests = Arc<Mutex<Vec<(Option<String>, Value)>>>;

        #[derive(Clone)]
        struct CaptureState {
            received: CapturedPushRequests,
        }

        async fn handler(
            State(st): State<CaptureState>,
            headers: HeaderMap,
            axum::Json(body): axum::Json<Value>,
        ) -> StatusCode {
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            st.received.lock().await.push((auth, body));
            StatusCode::OK
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/push", post(handler))
            .with_state(CaptureState {
                received: Arc::clone(&captured),
            });
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock push gateway");
        let addr = listener.local_addr().expect("local addr");
        let server_task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let tmp_home =
            std::env::temp_dir().join(format!("orbit-push-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&tmp_home).expect("mkdir");
        let store = Arc::new(OrbitStore::open_in_memory().expect("store"));
        let orch = make_test_orchestrator(Arc::clone(&store), &tmp_home).await;
        let mgr = PushManager::new_for_test(
            Arc::clone(&store),
            orch,
            "host_devbox".into(),
            "devbox".into(),
            true,
        );

        let snap = mgr
            .register_device(
                "ios_dev_1".into(),
                PushPlatform::Apns,
                "iPhone 16".into(),
                "apns_live_token_abc123456".into(),
                false,
            )
            .expect("register device");
        assert_eq!(snap.devices.len(), 1);
        assert_eq!(snap.devices[0].masked_token, "••••123456");

        let gateway_url = format!("http://{addr}/v1/push");
        let prefs_snap = mgr
            .update_preferences(
                PushPreferences {
                    enabled: true,
                    notify_permission_required: true,
                    notify_turn_completed: true,
                    notify_agent_error: true,
                    notify_goal_finished: true,
                    notify_timer_fired: true,
                    include_message_preview: false,
                    quiet_hours_enabled: true,
                    quiet_start_hour: 23,
                    quiet_end_hour: 7,
                    push_gateway_url: Some(gateway_url),
                    has_gateway_auth_token: false,
                },
                Some("bearer_test_secret".into()),
                false,
            )
            .expect("update prefs");
        assert!(prefs_snap.preferences.has_gateway_auth_token);

        // 1. Suppressed during quiet hours (02:00 is inside 23..7)
        let delivered_quiet = mgr
            .dispatch_event_at(
                PushEventKind::PermissionRequired,
                Some("ag_1"),
                Some("SUPER_SECRET_PROMPT"),
                2,
                1_760_000_000_000,
            )
            .await
            .expect("dispatch quiet");
        assert_eq!(delivered_quiet, 0);
        assert!(captured.lock().await.is_empty());

        // 2. Delivered outside quiet hours (14:00), zero-knowledge payload omits secret prompt
        let delivered_ok = mgr
            .dispatch_event_at(
                PushEventKind::PermissionRequired,
                Some("ag_1"),
                Some("SUPER_SECRET_PROMPT"),
                14,
                1_760_000_010_000,
            )
            .await
            .expect("dispatch ok");
        assert_eq!(delivered_ok, 1);

        // 3. Deduplicated within 5s cooldown window for same (ag_1, PermissionRequired)
        let delivered_dedup = mgr
            .dispatch_event_at(
                PushEventKind::PermissionRequired,
                Some("ag_1"),
                Some("SUPER_SECRET_PROMPT"),
                14,
                1_760_000_012_000,
            )
            .await
            .expect("dispatch dedup");
        assert_eq!(delivered_dedup, 0);

        // Verify mock gateway received exact Bearer auth & zero-knowledge envelope
        {
            let got = captured.lock().await;
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].0.as_deref(), Some("Bearer bearer_test_secret"));
            let body = &got[0].1;
            assert_eq!(body["platform"], "apns");
            assert_eq!(body["device_token"], "apns_live_token_abc123456");
            assert_eq!(body["payload"]["event_kind"], "permission_required");
            assert_eq!(body["payload"]["host_id"], "host_devbox");
            assert_eq!(body["payload"]["agent_id"], "ag_1");
            assert_eq!(
                body["payload"]["deep_link"],
                "orbit://open?host=host_devbox&agent=ag_1"
            );
            assert!(!body.to_string().contains("SUPER_SECRET_PROMPT"));
        }

        // 4. Test push bypasses quiet hours and updates last_delivered_at_ms
        let test_res = mgr.test_push(Some("ios_dev_1")).await;
        assert_eq!(test_res.attempted, 1);
        assert_eq!(test_res.delivered, 1);

        let final_snap = mgr.list_snapshot().expect("final snap");
        assert!(final_snap.devices[0].last_delivered_at_ms.is_some());
        assert_eq!(final_snap.devices[0].last_error, None);

        mgr.shutdown_all().await;
        server_task.abort();
        let _ = std::fs::remove_dir_all(&tmp_home);
    }
}
