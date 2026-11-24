//! Pure-Rust Bidirectional IM Bridge (Lark / Feishu, Telegram, Slack, Discord)
//! with default-deny sender/chat allowlists, constant-time webhook verification,
//! DNS-pinned `.no_proxy()` outbound API calls, bounded HTTP response reads,
//! NAT-friendly polling loops, and remote tool approval forwarding.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use orbit_protocol::{
    AgentStatus, ImChannelRecord, ImConnectionStatus, ImPlatform, ImRoutingMode, ImThreadBinding,
    ImTransportMode, ProviderKind, RouterImMirrorPolicy, SessionMode, TimelinePayload,
    ToolCallDetail, WorktreeCreateMode, sanitize_im_allowlist,
};
use orbit_store::{OrbitStore, StoredImChannel};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};
use tokio::task::AbortHandle;
use tracing::warn;
use uuid::Uuid;

use crate::DaemonRuntime;
use crate::im_longconn::{
    ParsedDiscordInteraction, is_valid_ed25519_public_key_hex, parse_discord_interaction_create,
    verify_discord_ed25519_signature,
};
use crate::manager::{AgentOrchestrator, CreateAgentSpec, DaemonBroadcast};

pub const MAX_WEBHOOK_BODY_BYTES: usize = 512 * 1024;
const MAX_IM_HTTP_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_DEDUP_ENTRIES: usize = 2_048;
const MAX_CALLBACK_TOKENS: usize = 1_024;
const CALLBACK_TOKEN_TTL_MS: u64 = 3_600_000; // 1 hour
const SIGNATURE_REPLAY_WINDOW_SECS: u64 = 300;
const MAX_TIMELINE_PAGES: usize = 32;
const TIMELINE_PAGE_SIZE: usize = 256;

const TELEGRAM_MAX_CHARS: usize = 4_000;
const DISCORD_MAX_CHARS: usize = 1_950;
const SLACK_MAX_CHARS: usize = 3_800;
const LARK_MAX_CHARS: usize = 3_800;

const ALLOWED_IM_API_HOSTS: &[&str] = &[
    "open.feishu.cn",
    "open.larksuite.com",
    "api.telegram.org",
    "slack.com",
    "discord.com",
];

/// Input payload for creating or updating an IM channel via wire RPC.
#[derive(Clone)]
pub struct UpsertImChannelSpec {
    pub channel_id: Option<String>,
    pub name: String,
    pub platform: ImPlatform,
    pub enabled: bool,
    pub transport_mode: ImTransportMode,
    pub app_id: Option<String>,
    pub primary_secret: Option<String>,
    pub signing_secret: Option<String>,
    pub app_token: Option<String>,
    pub public_key: Option<String>,
    pub keep_existing_secrets: bool,
    pub allowed_chat_ids: Vec<String>,
    pub allowed_user_ids: Vec<String>,
    pub default_workspace_cwd: String,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    pub forward_approvals: bool,
    pub routing_mode: ImRoutingMode,
    pub router_mirror_policy: RouterImMirrorPolicy,
}

impl std::fmt::Debug for UpsertImChannelSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UpsertImChannelSpec")
            .field("channel_id", &self.channel_id)
            .field("name", &self.name)
            .field("platform", &self.platform)
            .field("enabled", &self.enabled)
            .field("transport_mode", &self.transport_mode)
            .field("app_id", &self.app_id)
            .field(
                "primary_secret",
                &self.primary_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "signing_secret",
                &self.signing_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("app_token", &self.app_token.as_ref().map(|_| "[REDACTED]"))
            .field("public_key", &self.public_key)
            .field("keep_existing_secrets", &self.keep_existing_secrets)
            .field("allowed_chat_ids", &self.allowed_chat_ids)
            .field("allowed_user_ids", &self.allowed_user_ids)
            .field("default_workspace_cwd", &self.default_workspace_cwd)
            .field("default_provider", &self.default_provider)
            .field("default_mode", &self.default_mode)
            .field("forward_approvals", &self.forward_approvals)
            .field("routing_mode", &self.routing_mode)
            .field("router_mirror_policy", &self.router_mirror_policy)
            .finish()
    }
}

/// Snapshot returned by `ListImChannels`, `UpsertImChannel`, and `DeleteImChannel`.
#[derive(Debug, Clone)]
pub struct ImChannelsSnapshotData {
    pub channels: Vec<ImChannelRecord>,
    pub bindings: Vec<ImThreadBinding>,
    pub base_webhook_url: String,
}

/// Normalized inbound message extracted from any supported IM platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundImMessage {
    pub message_id: String,
    pub chat_id: String,
    pub thread_id: String,
    pub user_id: String,
    pub text: String,
}

/// Normalized inbound approval button action from Lark Card / Telegram Inline Keyboard / Slack Block Kit / Discord.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundApprovalAction {
    pub action_id: String,
    pub chat_id: String,
    pub thread_id: String,
    pub user_id: String,
    pub agent_id: Option<String>,
    pub request_id: Option<String>,
    pub decision: String,
}

/// Bounded FIFO deduplication cache preventing webhook retries or duplicate poll items
/// from executing multiple times while keeping memory strictly bounded.
#[derive(Debug)]
pub struct BoundedDedupCache {
    capacity: usize,
    order: VecDeque<String>,
    set: HashSet<String>,
}

impl BoundedDedupCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(16),
            order: VecDeque::new(),
            set: HashSet::new(),
        }
    }

    /// Returns `true` if `key` is new and was inserted; `false` if already seen.
    pub fn check_and_insert(&mut self, key: String) -> bool {
        if self.set.contains(&key) {
            return false;
        }
        while self.order.len() >= self.capacity {
            if let Some(evicted) = self.order.pop_front() {
                self.set.remove(&evicted);
            }
        }
        self.order.push_back(key.clone());
        self.set.insert(key);
        true
    }
}

#[derive(Clone)]
struct CachedLarkToken {
    token: String,
    expires_at_ms: u64,
}

impl std::fmt::Debug for CachedLarkToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedLarkToken")
            .field("token", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct CallbackApprovalRef {
    channel_id: String,
    chat_id: String,
    agent_id: String,
    request_id: String,
    created_at_ms: u64,
}

struct PollerHandle {
    fingerprint: String,
    abort: AbortHandle,
}

/// Strictly FIFO arrival-order serializer keyed by `(channel_id, chat_id, thread_id)`.
/// Enqueueing never blocks the WebSocket read loop, guarantees tasks for the same key run
/// strictly in arrival order (preventing concurrent duplicate-agent creation races), and
/// automatically prunes completed keys so memory stays bounded.
pub struct KeyedFifoDispatcher {
    tails: std::sync::Mutex<HashMap<(String, String, String), tokio::sync::oneshot::Receiver<()>>>,
}

impl KeyedFifoDispatcher {
    pub fn new() -> Self {
        Self {
            tails: std::sync::Mutex::new(HashMap::new()),
        }
    }

    pub fn enqueue<F>(&self, key: (String, String, String), fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let prev = if let Ok(mut guard) = self.tails.lock() {
            if guard.len() >= 128 {
                guard.retain(|_, tail_rx| {
                    matches!(
                        tail_rx.try_recv(),
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                    )
                });
            }
            guard.insert(key, rx)
        } else {
            None
        };
        tokio::spawn(async move {
            if let Some(prev_rx) = prev {
                let _ = prev_rx.await;
            }
            fut.await;
            let _ = tx.send(());
        });
    }
}

impl Default for KeyedFifoDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure-Rust Bidirectional IM Bridge manager owned by `DaemonRuntime`.
pub struct ImChannelBridge {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    listen_port: Arc<RwLock<u16>>,
    dedup: Arc<Mutex<BoundedDedupCache>>,
    forwarded_approvals: Arc<Mutex<BoundedDedupCache>>,
    last_delivered_seq: Arc<Mutex<HashMap<String, u64>>>,
    lark_tokens: Arc<RwLock<HashMap<String, CachedLarkToken>>>,
    callback_refs: Arc<Mutex<VecDeque<(String, CallbackApprovalRef)>>>,
    pollers: Arc<Mutex<HashMap<String, PollerHandle>>>,
    broadcast_task: Arc<Mutex<Option<AbortHandle>>>,
    inbound_dispatcher: Arc<KeyedFifoDispatcher>,
    personal_router: RwLock<Option<Arc<crate::personal_router::PersonalRouterManager>>>,
    /// Test hook: skip `authorize_workspace_mutation_cwd` only in isolated unit tests.
    enforce_workspace_auth: bool,
}

impl ImChannelBridge {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        listen_port: Arc<RwLock<u16>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            listen_port,
            dedup: Arc::new(Mutex::new(BoundedDedupCache::new(MAX_DEDUP_ENTRIES))),
            forwarded_approvals: Arc::new(Mutex::new(BoundedDedupCache::new(MAX_DEDUP_ENTRIES))),
            last_delivered_seq: Arc::new(Mutex::new(HashMap::new())),
            lark_tokens: Arc::new(RwLock::new(HashMap::new())),
            callback_refs: Arc::new(Mutex::new(VecDeque::new())),
            pollers: Arc::new(Mutex::new(HashMap::new())),
            broadcast_task: Arc::new(Mutex::new(None)),
            inbound_dispatcher: Arc::new(KeyedFifoDispatcher::new()),
            personal_router: RwLock::new(None),
            enforce_workspace_auth: true,
        })
    }

    pub async fn set_personal_router(
        &self,
        router: Arc<crate::personal_router::PersonalRouterManager>,
    ) {
        *self.personal_router.write().await = Some(router);
    }

    pub(crate) fn store(&self) -> &Arc<OrbitStore> {
        &self.store
    }

    pub(crate) async fn check_and_mark_dedup(&self, key: String) -> bool {
        self.dedup.lock().await.check_and_insert(key)
    }

    /// Serialize inbound message execution in strict arrival order per `(channel_id, chat_id, thread_id)`.
    pub(crate) fn spawn_serialized_inbound_message(
        self: &Arc<Self>,
        channel: StoredImChannel,
        inbound: InboundImMessage,
    ) {
        let key = (
            channel.channel_id.clone(),
            inbound.chat_id.clone(),
            inbound.thread_id.clone(),
        );
        let bridge = Arc::clone(self);
        self.inbound_dispatcher.enqueue(key, async move {
            if let Ok(Some(reply)) = bridge
                .handle_inbound_message(&channel, inbound.clone())
                .await
            {
                let _ = bridge
                    .send_text_message(&channel, &inbound.chat_id, &inbound.thread_id, &reply)
                    .await;
            }
        });
    }

    #[cfg(test)]
    fn new_for_test(store: Arc<OrbitStore>, orchestrator: Arc<AgentOrchestrator>) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            listen_port: Arc::new(RwLock::new(7788)),
            dedup: Arc::new(Mutex::new(BoundedDedupCache::new(64))),
            forwarded_approvals: Arc::new(Mutex::new(BoundedDedupCache::new(64))),
            last_delivered_seq: Arc::new(Mutex::new(HashMap::new())),
            lark_tokens: Arc::new(RwLock::new(HashMap::new())),
            callback_refs: Arc::new(Mutex::new(VecDeque::new())),
            pollers: Arc::new(Mutex::new(HashMap::new())),
            broadcast_task: Arc::new(Mutex::new(None)),
            inbound_dispatcher: Arc::new(KeyedFifoDispatcher::new()),
            personal_router: RwLock::new(None),
            enforce_workspace_auth: false,
        })
    }

    /// Seed `last_delivered_seq` for existing bound agents (preventing post-restart replay),
    /// start the outbound `DaemonBroadcast` listener, and launch NAT-friendly pollers for
    /// enabled Telegram / Discord channels. Safe to call once during `activate_server`.
    pub async fn start(self: &Arc<Self>) {
        self.seed_delivered_cursors().await;
        {
            let mut task_slot = self.broadcast_task.lock().await;
            if task_slot.is_none() {
                *task_slot = Some(self.spawn_broadcast_listener());
            }
        }
        self.reconcile_pollers().await;
    }

    /// Gracefully abort the outbound broadcast listener and all active Telegram/Discord pollers.
    pub async fn shutdown_all(&self) {
        if let Some(handle) = self.broadcast_task.lock().await.take() {
            handle.abort();
        }
        let mut pollers = self.pollers.lock().await;
        for (_, handle) in pollers.drain() {
            handle.abort.abort();
        }
    }

    /// Seed `last_delivered_seq` from the current latest persisted timeline seq of every bound
    /// agent so restarting `orbit daemon` never replays pre-restart assistant replies.
    async fn seed_delivered_cursors(&self) {
        let Ok(bindings) = self.store.list_im_bindings(None) else {
            return;
        };
        if bindings.is_empty() {
            return;
        }
        let mut guard = self.last_delivered_seq.lock().await;
        for binding in bindings {
            if guard.contains_key(&binding.agent_id) {
                continue;
            }
            let latest_seq = self
                .store
                .get_agent(&binding.agent_id)
                .ok()
                .flatten()
                .map(|ag| ag.latest_seq)
                .unwrap_or(0);
            let effective_seq = if latest_seq > 0 {
                latest_seq
            } else {
                self.store
                    .fetch_recent_timeline(&binding.agent_id, 1)
                    .ok()
                    .and_then(|evs| evs.last().map(|e| e.seq))
                    .unwrap_or(0)
            };
            if effective_seq > 0 {
                guard.insert(binding.agent_id, effective_seq);
            }
        }
    }

    pub async fn base_webhook_url(&self) -> String {
        let port = *self.listen_port.read().await;
        if port == 0 {
            "http://127.0.0.1:7788".to_string()
        } else {
            format!("http://127.0.0.1:{port}")
        }
    }

    pub async fn list_snapshot(&self) -> anyhow::Result<ImChannelsSnapshotData> {
        let channels = self
            .store
            .list_im_channels()?
            .into_iter()
            .map(|c| c.to_public_record())
            .collect();
        let bindings = self.store.list_im_bindings(None)?;
        let base_webhook_url = self.base_webhook_url().await;
        Ok(ImChannelsSnapshotData {
            channels,
            bindings,
            base_webhook_url,
        })
    }

    pub async fn upsert_channel(
        self: &Arc<Self>,
        spec: UpsertImChannelSpec,
    ) -> anyhow::Result<ImChannelsSnapshotData> {
        let name = spec.name.trim().to_string();
        if name.is_empty() || name.len() > 120 {
            anyhow::bail!("Channel name must be between 1 and 120 characters");
        }

        let raw_cwd = spec.default_workspace_cwd.trim();
        if raw_cwd.is_empty() {
            anyhow::bail!("Default workspace directory is required");
        }
        let expanded_cwd = crate::expand_host_path(raw_cwd);
        if expanded_cwd
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            anyhow::bail!("Default workspace path must not contain `..` segments");
        }
        let authorized_cwd = if self.enforce_workspace_auth {
            crate::authorize_workspace_mutation_cwd(&self.store, &expanded_cwd)
                .await
                .map_err(|e| anyhow::anyhow!(e))?
                .to_string_lossy()
                .to_string()
        } else {
            expanded_cwd.to_string_lossy().to_string()
        };

        let existing = match spec
            .channel_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(cid) => self.store.get_im_channel(cid)?,
            None => None,
        };

        let now = now_unix_ms();
        let channel_id = existing
            .as_ref()
            .map(|e| e.channel_id.clone())
            .or_else(|| {
                spec.channel_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(sanitize_channel_id)
            })
            .unwrap_or_else(|| {
                format!(
                    "im_{}_{}",
                    spec.platform.slug(),
                    &Uuid::new_v4().simple().to_string()[..10]
                )
            });

        let clean_primary_input = spec
            .primary_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let clean_signing_input = spec
            .signing_secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let clean_app_token_input = spec
            .app_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        let (primary_secret, signing_secret, app_token) = if spec.keep_existing_secrets {
            (
                clean_primary_input
                    .or_else(|| existing.as_ref().and_then(|e| e.primary_secret.clone())),
                clean_signing_input
                    .or_else(|| existing.as_ref().and_then(|e| e.signing_secret.clone())),
                clean_app_token_input
                    .or_else(|| existing.as_ref().and_then(|e| e.app_token.clone())),
            )
        } else {
            (
                clean_primary_input,
                clean_signing_input,
                clean_app_token_input,
            )
        };

        let public_key = spec
            .public_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_ascii_lowercase());

        let app_id = spec
            .app_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);

        // Prevent duplicate enabled Slack Socket Mode channels sharing the same App-Level Token
        // (Slack Socket Mode load-balances envelopes across sockets on the same `xapp-...` token,
        // so a second active socket would steal events from the first channel).
        if spec.enabled
            && spec.platform == ImPlatform::Slack
            && spec.transport_mode == ImTransportMode::LongConnection
            && let Some(new_tok) = app_token.as_deref()
        {
            for other in self.store.list_im_channels()? {
                if other.channel_id != channel_id
                    && other.enabled
                    && other.platform == ImPlatform::Slack
                    && other.transport_mode == ImTransportMode::LongConnection
                    && let Some(other_tok) = other.app_token.as_deref().map(str::trim)
                    && constant_time_eq(new_tok.as_bytes(), other_tok.as_bytes())
                {
                    anyhow::bail!(
                        "Another enabled Slack Socket Mode channel (`{}`) already uses this App-Level Token; disable it or switch to Webhook mode first",
                        other.name
                    );
                }
            }
        }

        let allowed_chat_ids = sanitize_im_allowlist(spec.allowed_chat_ids);
        let allowed_user_ids = sanitize_im_allowlist(spec.allowed_user_ids);

        let (initial_status, initial_msg) = if !spec.enabled {
            (ImConnectionStatus::Disconnected, Some("Disabled".into()))
        } else {
            match (spec.platform, spec.transport_mode) {
                (ImPlatform::Telegram, ImTransportMode::LongConnection) => {
                    if primary_secret.is_some() {
                        (
                            ImConnectionStatus::Connecting,
                            Some("Starting long-polling".into()),
                        )
                    } else {
                        (ImConnectionStatus::Error, Some("Bot token required".into()))
                    }
                }
                (ImPlatform::Telegram, ImTransportMode::Webhook) => {
                    if primary_secret.is_some() && signing_secret.is_some() {
                        (ImConnectionStatus::Connected, Some("Webhook ready".into()))
                    } else {
                        (
                            ImConnectionStatus::Disconnected,
                            Some("Bot Token and Webhook Secret Token required".into()),
                        )
                    }
                }
                (ImPlatform::Discord, ImTransportMode::LongConnection) => {
                    if primary_secret.is_some() {
                        (
                            ImConnectionStatus::Connecting,
                            Some("Connecting (WS)".into()),
                        )
                    } else {
                        (ImConnectionStatus::Error, Some("Bot token required".into()))
                    }
                }
                (ImPlatform::Discord, ImTransportMode::Webhook) => {
                    if let Some(pk) = public_key.as_deref() {
                        if !is_valid_ed25519_public_key_hex(pk) {
                            (
                                ImConnectionStatus::Error,
                                Some("Invalid Ed25519 Public Key (expected 64 hex chars)".into()),
                            )
                        } else if primary_secret.is_some() {
                            (ImConnectionStatus::Connected, Some("Webhook ready".into()))
                        } else {
                            (
                                ImConnectionStatus::Disconnected,
                                Some("Bot Token required for outbound replies".into()),
                            )
                        }
                    } else {
                        (
                            ImConnectionStatus::Disconnected,
                            Some("Ed25519 Public Key required for Discord webhooks".into()),
                        )
                    }
                }
                (ImPlatform::Lark, ImTransportMode::LongConnection) => {
                    if app_id.is_some() && primary_secret.is_some() {
                        (
                            ImConnectionStatus::Connecting,
                            Some("Connecting (WS)".into()),
                        )
                    } else {
                        (
                            ImConnectionStatus::Error,
                            Some("App ID and App Secret required".into()),
                        )
                    }
                }
                (ImPlatform::Lark, ImTransportMode::Webhook) => {
                    if app_id.is_some() && primary_secret.is_some() && signing_secret.is_some() {
                        (ImConnectionStatus::Connected, Some("Webhook ready".into()))
                    } else {
                        (
                            ImConnectionStatus::Disconnected,
                            Some("App ID, App Secret, and Verification Token required".into()),
                        )
                    }
                }
                (ImPlatform::Slack, ImTransportMode::LongConnection) => {
                    if primary_secret.is_some() && app_token.is_some() {
                        (
                            ImConnectionStatus::Connecting,
                            Some("Connecting (WS)".into()),
                        )
                    } else {
                        (
                            ImConnectionStatus::Error,
                            Some("Bot Token and App-Level Token (xapp-...) required".into()),
                        )
                    }
                }
                (ImPlatform::Slack, ImTransportMode::Webhook) => {
                    if primary_secret.is_some() && signing_secret.is_some() {
                        (ImConnectionStatus::Connected, Some("Webhook ready".into()))
                    } else {
                        (
                            ImConnectionStatus::Disconnected,
                            Some("Bot Token and Signing Secret required".into()),
                        )
                    }
                }
            }
        };

        let record = StoredImChannel {
            channel_id: channel_id.clone(),
            name,
            platform: spec.platform,
            enabled: spec.enabled,
            transport_mode: spec.transport_mode,
            status: initial_status,
            status_message: initial_msg,
            webhook_path: spec.platform.webhook_path_for(&channel_id),
            app_id,
            primary_secret,
            signing_secret,
            app_token,
            public_key,
            allowed_chat_ids,
            allowed_user_ids,
            default_workspace_cwd: authorized_cwd,
            default_provider: spec.default_provider,
            default_mode: spec.default_mode,
            forward_approvals: spec.forward_approvals,
            routing_mode: spec.routing_mode,
            router_mirror_policy: spec.router_mirror_policy,
            message_count: existing.as_ref().map(|e| e.message_count).unwrap_or(0),
            last_activity_at_ms: existing.as_ref().and_then(|e| e.last_activity_at_ms),
            created_at_ms: existing.as_ref().map(|e| e.created_at_ms).unwrap_or(now),
            updated_at_ms: now,
        };

        self.store.upsert_im_channel(&record)?;
        self.lark_tokens.write().await.remove(&channel_id);
        self.reconcile_pollers().await;
        self.list_snapshot().await
    }

    pub async fn delete_channel(
        self: &Arc<Self>,
        channel_id: &str,
    ) -> anyhow::Result<ImChannelsSnapshotData> {
        let cid = channel_id.trim();
        if cid.is_empty() {
            anyhow::bail!("channel_id is required");
        }
        self.store.delete_im_channel(cid)?;
        self.lark_tokens.write().await.remove(cid);
        if let Some(handle) = self.pollers.lock().await.remove(cid) {
            handle.abort.abort();
        }
        self.list_snapshot().await
    }

    pub async fn test_channel(
        self: &Arc<Self>,
        channel_id: &str,
        target_chat_id: &str,
        message: &str,
    ) -> (bool, String) {
        let cid = channel_id.trim();
        let chat_id = target_chat_id.trim();
        if chat_id.is_empty() {
            return (false, "Target chat ID is required".to_string());
        }
        if chat_id.len() > 256 || chat_id.chars().any(char::is_control) {
            return (
                false,
                "Target chat ID is invalid (must be <= 256 chars with no control characters)"
                    .to_string(),
            );
        }
        let body = if message.trim().is_empty() {
            "Orbit IM channel test message — connection verified."
        } else {
            message.trim()
        };
        let channel = match self.store.get_im_channel(cid) {
            Ok(Some(ch)) => ch,
            Ok(None) => return (false, format!("IM channel `{cid}` not found")),
            Err(e) => return (false, format!("Store error: {e}")),
        };

        match self.send_text_message(&channel, chat_id, "", body).await {
            Ok(()) => {
                // Never flip a disabled channel's persisted status to `Connected`
                if channel.enabled {
                    let _ = self.store.update_im_channel_status(
                        &channel.channel_id,
                        ImConnectionStatus::Connected,
                        Some("Test message delivered"),
                        now_unix_ms(),
                    );
                }
                (
                    true,
                    format!(
                        "Delivered test message to {} ({})",
                        channel.platform.display_name(),
                        chat_id
                    ),
                )
            }
            Err(err) => {
                if channel.enabled {
                    let _ = self.store.update_im_channel_status(
                        &channel.channel_id,
                        ImConnectionStatus::Error,
                        Some(&truncate_for_status(&err)),
                        now_unix_ms(),
                    );
                }
                (false, err)
            }
        }
    }

    /// Reconcile background NAT-friendly runners:
    /// - Feishu / Lark WebSocket Long-Connection (`pbbp2.Frame`)
    /// - Slack Socket Mode (WSS)
    /// - Discord Gateway v10 (WSS)
    /// - Telegram `getUpdates` long-polling
    pub async fn reconcile_pollers(self: &Arc<Self>) {
        let channels = match self.store.list_im_channels() {
            Ok(list) => list,
            Err(e) => {
                warn!(error = %e, "Failed to list IM channels during poller reconciliation");
                return;
            }
        };

        let mut desired: HashMap<String, (String, StoredImChannel)> = HashMap::new();
        let mut seen_slack_app_token_hashes: HashSet<String> = HashSet::new();
        for ch in channels {
            if !ch.enabled || ch.transport_mode != ImTransportMode::LongConnection {
                continue;
            }
            match ch.platform {
                ImPlatform::Lark if ch.app_id.is_some() && ch.primary_secret.is_some() => {
                    let fp = format!(
                        "lark-ws:{}:{}",
                        ch.app_id.as_deref().unwrap_or(""),
                        sha256_hex(ch.primary_secret.as_deref().unwrap_or("").as_bytes()),
                    );
                    desired.insert(ch.channel_id.clone(), (fp, ch));
                }
                ImPlatform::Slack if ch.primary_secret.is_some() && ch.app_token.is_some() => {
                    let app_tok_hash =
                        sha256_hex(ch.app_token.as_deref().unwrap_or("").trim().as_bytes());
                    if !seen_slack_app_token_hashes.insert(app_tok_hash.clone()) {
                        warn!(
                            channel_id = %ch.channel_id,
                            "Skipping duplicate active Slack Socket Mode channel sharing the same App-Level Token"
                        );
                        let _ = self.store.update_im_channel_status(
                            &ch.channel_id,
                            ImConnectionStatus::Error,
                            Some("Duplicate Slack App-Level Token active on another channel"),
                            now_unix_ms(),
                        );
                        continue;
                    }
                    let fp = format!(
                        "slack-ws:{}:{}",
                        sha256_hex(ch.primary_secret.as_deref().unwrap_or("").as_bytes()),
                        app_tok_hash,
                    );
                    desired.insert(ch.channel_id.clone(), (fp, ch));
                }
                ImPlatform::Discord if ch.primary_secret.is_some() => {
                    let fp = format!(
                        "dc-ws:{}",
                        sha256_hex(ch.primary_secret.as_deref().unwrap_or("").as_bytes()),
                    );
                    desired.insert(ch.channel_id.clone(), (fp, ch));
                }
                ImPlatform::Telegram if ch.primary_secret.is_some() => {
                    let fp = format!(
                        "tg:{}:{}",
                        sha256_hex(ch.primary_secret.as_deref().unwrap_or("").as_bytes()),
                        ch.allowed_chat_ids.join(",")
                    );
                    desired.insert(ch.channel_id.clone(), (fp, ch));
                }
                _ => {}
            }
        }

        let mut pollers = self.pollers.lock().await;
        // Stop removed or changed runners
        let existing_ids: Vec<String> = pollers.keys().cloned().collect();
        for cid in existing_ids {
            let keep = desired
                .get(&cid)
                .is_some_and(|(fp, _)| pollers.get(&cid).is_some_and(|h| &h.fingerprint == fp));
            if !keep && let Some(handle) = pollers.remove(&cid) {
                handle.abort.abort();
            }
        }

        // Start new runners
        for (cid, (fp, ch)) in desired {
            if pollers.contains_key(&cid) {
                continue;
            }
            let bridge = Arc::clone(self);
            let channel_id_clone = cid.clone();
            let platform = ch.platform;
            let handle = tokio::spawn(async move {
                match platform {
                    ImPlatform::Lark => {
                        crate::im_longconn::run_lark_ws(bridge, channel_id_clone).await
                    }
                    ImPlatform::Slack => {
                        crate::im_longconn::run_slack_socket_mode(bridge, channel_id_clone).await
                    }
                    ImPlatform::Discord => {
                        crate::im_longconn::run_discord_gateway(bridge, channel_id_clone).await
                    }
                    ImPlatform::Telegram => bridge.run_telegram_poller(channel_id_clone).await,
                }
            });
            pollers.insert(
                cid,
                PollerHandle {
                    fingerprint: fp,
                    abort: handle.abort_handle(),
                },
            );
        }
    }

    /// Background subscriber on `DaemonBroadcast`:
    /// 1. Delivers assistant replies back to bound IM chats/threads when an agent settles to `Idle`.
    /// 2. Forwards `PermissionRequest` cards/prompts to bound IM chats when `forward_approvals == true`.
    fn spawn_broadcast_listener(self: &Arc<Self>) -> AbortHandle {
        let bridge = Arc::clone(self);
        let mut rx = self.orchestrator.broadcast_tx.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(DaemonBroadcast::AgentUpdated(summary)) => {
                        if matches!(summary.status, AgentStatus::Idle) {
                            let b = Arc::clone(&bridge);
                            let agent_id = summary.agent_id.clone();
                            tokio::spawn(async move {
                                b.deliver_idle_reply_if_bound(&agent_id).await;
                            });
                        }
                    }
                    Ok(DaemonBroadcast::TimelineLive(ev)) => {
                        if let TimelinePayload::PermissionRequest {
                            request_id,
                            title,
                            detail,
                            ..
                        } = ev.payload
                        {
                            let b = Arc::clone(&bridge);
                            let agent_id = ev.agent_id.clone();
                            tokio::spawn(async move {
                                b.forward_permission_if_bound(
                                    &agent_id,
                                    &request_id,
                                    &title,
                                    &detail,
                                )
                                .await;
                            });
                        }
                    }
                    Ok(DaemonBroadcast::AgentDeleted(agent_id)) => {
                        let _ = bridge.store.delete_im_bindings_for_agent(&agent_id);
                        bridge.last_delivered_seq.lock().await.remove(&agent_id);
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(skipped, "IM bridge broadcast listener lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        handle.abort_handle()
    }

    /// Collect the latest assistant reply since `last_delivered_seq`, paginating across
    /// `fetch_timeline_since` so long tool-heavy turns (>256 events) never stall.
    async fn collect_undelivered_assistant_reply(&self, agent_id: &str) -> Option<String> {
        let last_seq = {
            let guard = self.last_delivered_seq.lock().await;
            guard.get(agent_id).copied().unwrap_or(0)
        };

        let mut cursor = last_seq;
        let mut max_seq = last_seq;
        let mut latest_turn_msgs: Vec<(String, String)> = Vec::new();

        for _ in 0..MAX_TIMELINE_PAGES {
            let (events, has_more) =
                match self
                    .store
                    .fetch_timeline_since(agent_id, cursor, TIMELINE_PAGE_SIZE)
                {
                    Ok(page) => page,
                    Err(_) => return None,
                };
            if events.is_empty() {
                break;
            }
            for ev in &events {
                if ev.seq > max_seq {
                    max_seq = ev.seq;
                }
                match &ev.payload {
                    TimelinePayload::UserPrompt { .. } => {
                        latest_turn_msgs.clear();
                    }
                    TimelinePayload::AssistantMessage { item_id, text, .. } => {
                        let trimmed = text.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Some(existing) = latest_turn_msgs
                            .iter_mut()
                            .find(|(id, _)| !item_id.is_empty() && id == item_id)
                        {
                            existing.1 = trimmed.to_string();
                        } else {
                            latest_turn_msgs.push((item_id.clone(), trimmed.to_string()));
                        }
                    }
                    _ => {}
                }
            }
            if !has_more || max_seq <= cursor {
                break;
            }
            cursor = max_seq;
        }

        if max_seq > last_seq {
            let mut guard = self.last_delivered_seq.lock().await;
            let entry = guard.entry(agent_id.to_string()).or_insert(0);
            if max_seq <= *entry {
                return None;
            }
            *entry = max_seq;
        } else {
            return None;
        }

        if latest_turn_msgs.is_empty() {
            return None;
        }

        Some(
            latest_turn_msgs
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    }

    async fn deliver_idle_reply_if_bound(self: &Arc<Self>, agent_id: &str) {
        let bindings = match self.store.find_im_bindings_by_agent(agent_id) {
            Ok(b) if !b.is_empty() => b,
            _ => return,
        };

        let Some(reply_text) = self.collect_undelivered_assistant_reply(agent_id).await else {
            return;
        };

        for binding in bindings {
            let Ok(Some(channel)) = self.store.get_im_channel(&binding.channel_id) else {
                continue;
            };
            if !channel.enabled {
                continue;
            }
            if let Err(err) = self
                .send_text_message(
                    &channel,
                    &binding.external_chat_id,
                    &binding.external_thread_id,
                    &reply_text,
                )
                .await
            {
                warn!(
                    channel_id = %channel.channel_id,
                    agent_id = %agent_id,
                    error = %err,
                    "Failed to deliver assistant reply to IM channel"
                );
            }
        }
    }

    async fn forward_permission_if_bound(
        self: &Arc<Self>,
        agent_id: &str,
        request_id: &str,
        title: &str,
        detail: &ToolCallDetail,
    ) {
        let bindings = match self.store.find_im_bindings_by_agent(agent_id) {
            Ok(b) if !b.is_empty() => b,
            _ => return,
        };

        let summary = summarize_tool_detail(detail);
        for binding in bindings {
            let Ok(Some(channel)) = self.store.get_im_channel(&binding.channel_id) else {
                continue;
            };
            if !channel.enabled || !channel.forward_approvals {
                continue;
            }
            let dedup_key = format!("perm:{}:{}:{}", channel.channel_id, agent_id, request_id);
            if !self
                .forwarded_approvals
                .lock()
                .await
                .check_and_insert(dedup_key)
            {
                continue;
            }
            if let Err(err) = self
                .send_approval_prompt(
                    &channel,
                    &binding.external_chat_id,
                    &binding.external_thread_id,
                    agent_id,
                    request_id,
                    title,
                    &summary,
                )
                .await
            {
                warn!(
                    channel_id = %channel.channel_id,
                    agent_id = %agent_id,
                    request_id = %request_id,
                    error = %err,
                    "Failed to forward permission request to IM channel"
                );
            }
        }
    }

    /// Core inbound message pipeline shared by webhooks (Lark / Slack / Telegram / Discord)
    /// and NAT-friendly pollers (Telegram / Discord).
    /// Returns an optional immediate reply string (used for slash commands or error feedback).
    pub async fn handle_inbound_message(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        inbound: InboundImMessage,
    ) -> Result<Option<String>, String> {
        if !channel.enabled {
            return Err("IM channel is disabled".to_string());
        }
        if !is_sender_authorized(channel, &inbound.chat_id, &inbound.user_id) {
            warn!(
                channel_id = %channel.channel_id,
                chat_id = %inbound.chat_id,
                user_id = %inbound.user_id,
                "Rejected inbound IM message from unallowlisted chat/user (default-deny)"
            );
            return Err("Sender/chat not allowlisted".to_string());
        }

        let dedup_key = format!("msg:{}:{}", channel.channel_id, inbound.message_id.trim());
        if !inbound.message_id.trim().is_empty()
            && !self.dedup.lock().await.check_and_insert(dedup_key)
        {
            return Ok(None);
        }

        let text = inbound.text.trim();
        if text.is_empty() {
            return Ok(None);
        }

        let now = now_unix_ms();
        let _ = self
            .store
            .increment_im_channel_activity(&channel.channel_id, now);

        // Parse quick slash commands first
        if let Some(cmd) = parse_im_slash_command(text) {
            let reply = self.execute_slash_command(channel, &inbound, cmd).await;
            return Ok(Some(reply));
        }

        // PersonalRouter routing mode: route ALL authorized IM messages into the host's single
        // canonical Personal Router ("分工师") session so Feishu/IM and Orbit Native UI share
        // one continuous bidirectional conversation!
        if channel.routing_mode == ImRoutingMode::PersonalRouter {
            let router_opt = self.personal_router.read().await.clone();
            let Some(router) = router_opt else {
                return Ok(Some(
                    "Personal Router is not initialized on this host.".to_string(),
                ));
            };
            let router_agent = match router.ensure_router_agent().await {
                Ok(ag) => ag,
                Err(e) => {
                    return Ok(Some(format!("Unable to activate Personal Router: {e}")));
                }
            };
            let tagged_prompt = format!("[via {}] {text}", channel.platform.display_name());
            if let Err(e) = self
                .orchestrator
                .send_prompt(&router_agent.agent_id, tagged_prompt)
                .await
            {
                return Ok(Some(format!(
                    "Failed to deliver message to Personal Router: {e}"
                )));
            }
            let _ = self.store.upsert_im_binding(&ImThreadBinding {
                channel_id: channel.channel_id.clone(),
                external_chat_id: inbound.chat_id.clone(),
                external_thread_id: inbound.thread_id.clone(),
                agent_id: router_agent.agent_id,
                updated_at_ms: now,
            });
            return Ok(None);
        }

        // Route to existing bound agent or create a fresh one in `default_workspace_cwd`
        let existing_binding = self
            .store
            .find_im_binding(&channel.channel_id, &inbound.chat_id, &inbound.thread_id)
            .ok()
            .flatten();

        if let Some(binding) = existing_binding
            && let Ok(Some(agent)) = self.store.get_agent(&binding.agent_id)
            && agent.archived_at.is_none()
        {
            if let Err(e) = self
                .orchestrator
                .send_prompt(&agent.agent_id, text.to_string())
                .await
            {
                return Ok(Some(format!("Failed to deliver prompt to agent: {e}")));
            }
            let _ = self.store.upsert_im_binding(&ImThreadBinding {
                channel_id: channel.channel_id.clone(),
                external_chat_id: inbound.chat_id.clone(),
                external_thread_id: inbound.thread_id.clone(),
                agent_id: agent.agent_id,
                updated_at_ms: now,
            });
            return Ok(None);
        }

        // Create a new agent session bound to this chat/thread
        match self
            .create_bound_agent(channel, &inbound.chat_id, &inbound.thread_id, text)
            .await
        {
            Ok(_) => Ok(None),
            Err(e) => Ok(Some(format!("Unable to start Orbit agent: {e}"))),
        }
    }

    async fn execute_slash_command(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        inbound: &InboundImMessage,
        cmd: ImSlashCommand,
    ) -> String {
        let bound_agent_id = self
            .store
            .find_im_binding(&channel.channel_id, &inbound.chat_id, &inbound.thread_id)
            .ok()
            .flatten()
            .map(|b| b.agent_id);

        match cmd {
            ImSlashCommand::Approve(req_id) => {
                if !channel.forward_approvals {
                    return "Remote approvals are disabled on this IM channel.".to_string();
                }
                self.resolve_permission_from_im(
                    bound_agent_id.as_deref(),
                    req_id.as_deref(),
                    "allow",
                )
                .await
            }
            ImSlashCommand::Deny(req_id) => {
                if !channel.forward_approvals {
                    return "Remote approvals are disabled on this IM channel.".to_string();
                }
                self.resolve_permission_from_im(
                    bound_agent_id.as_deref(),
                    req_id.as_deref(),
                    "deny",
                )
                .await
            }
            ImSlashCommand::Status => {
                let Some(agent_id) = bound_agent_id else {
                    return format!(
                        "No active Orbit session bound to this chat yet.\nWorkspace: {}\nProvider: {}",
                        channel.default_workspace_cwd,
                        channel.default_provider.display_name()
                    );
                };
                match self.store.get_agent(&agent_id) {
                    Ok(Some(ag)) => {
                        let pending = self
                            .orchestrator
                            .list_pending_permissions(Some(&agent_id))
                            .await;
                        format!(
                            "Agent: {} ({})\nStatus: {:?}\nProvider: {}\nWorkspace: {}\nPending approvals: {}",
                            ag.title,
                            ag.agent_id,
                            ag.status,
                            ag.provider.display_name(),
                            ag.cwd,
                            pending.len()
                        )
                    }
                    _ => format!(
                        "Bound agent `{agent_id}` is no longer present. Send a message or `/new` to start a new session."
                    ),
                }
            }
            ImSlashCommand::Stop => {
                let Some(agent_id) = bound_agent_id else {
                    return "No active Orbit session bound to this chat.".to_string();
                };
                match self.orchestrator.interrupt_agent(&agent_id).await {
                    Ok(()) => format!("Stopped active turn on agent `{agent_id}`."),
                    Err(e) => format!("Failed to stop agent `{agent_id}`: {e}"),
                }
            }
            ImSlashCommand::New(prompt) => {
                match self
                    .create_bound_agent(channel, &inbound.chat_id, &inbound.thread_id, &prompt)
                    .await
                {
                    Ok(agent_id) => {
                        if prompt.trim().is_empty() {
                            format!(
                                "Started new Orbit session `{agent_id}` in `{}`.",
                                channel.default_workspace_cwd
                            )
                        } else {
                            format!("Started new Orbit session `{agent_id}`.")
                        }
                    }
                    Err(e) => format!("Failed to create new session: {e}"),
                }
            }
        }
    }

    /// Strictly resolve a pending permission ONLY on the agent bound to this `(channel_id, chat_id)`.
    /// Never falls back to host-wide `list_pending_permissions(None)` and rejects ambiguous prefixes.
    async fn resolve_permission_from_im(
        &self,
        bound_agent_id: Option<&str>,
        requested_id: Option<&str>,
        decision: &str,
    ) -> String {
        let Some(agent_id) = bound_agent_id.map(str::trim).filter(|s| !s.is_empty()) else {
            return "No active Orbit session bound to this chat.".to_string();
        };

        let pending = self
            .orchestrator
            .list_pending_permissions(Some(agent_id))
            .await;
        let target = if let Some(rid) = requested_id.map(str::trim).filter(|s| !s.is_empty()) {
            if let Some(exact) = pending.iter().find(|p| p.request_id == rid) {
                Some(exact.clone())
            } else {
                let prefix_matches: Vec<_> = pending
                    .iter()
                    .filter(|p| p.request_id.starts_with(rid))
                    .collect();
                match prefix_matches.len() {
                    0 => None,
                    1 => Some(prefix_matches[0].clone()),
                    n => {
                        return format!(
                            "Ambiguous approval prefix `{rid}` matches {n} pending requests; please specify the full request ID."
                        );
                    }
                }
            }
        } else if pending.len() == 1 {
            pending.first().cloned()
        } else if pending.len() > 1 {
            return format!(
                "Multiple pending approvals ({}) exist on this session; specify `/approve <request_id>` or `/deny <request_id>`.",
                pending.len()
            );
        } else {
            None
        };

        let Some(perm) = target else {
            return match requested_id {
                Some(rid) => format!("No pending approval matching `{rid}` on this session."),
                None => "No pending approval requests for this session.".to_string(),
            };
        };

        let normalized = normalize_decision(decision);
        match self
            .orchestrator
            .resolve_permission(&perm.agent_id, perm.request_id.clone(), normalized.clone())
            .await
        {
            Ok(()) => format!(
                "{} `{}` ({}) for agent `{}`.",
                if normalized == "allow" {
                    "Approved"
                } else {
                    "Denied"
                },
                perm.title,
                perm.request_id,
                perm.agent_id
            ),
            Err(e) => format!("Failed to resolve approval `{}`: {e}", perm.request_id),
        }
    }

    /// Verify that `agent_id` is genuinely bound to `(channel_id, chat_id)` in SQLite.
    fn is_agent_bound_to_chat(&self, channel_id: &str, chat_id: &str, agent_id: &str) -> bool {
        self.store
            .find_im_bindings_by_agent(agent_id)
            .ok()
            .is_some_and(|bindings| {
                bindings
                    .iter()
                    .any(|b| b.channel_id == channel_id && b.external_chat_id == chat_id)
            })
    }

    /// Resolve a button callback approval action (Lark Card / Telegram Inline Keyboard / Slack Block Kit / Discord).
    /// Enforces `forward_approvals == true`, allowlist authorization, binding ownership of `agent_id`
    /// to `(channel.channel_id, action.chat_id)`, and active pending permission verification.
    pub async fn handle_inbound_approval_action(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        action: InboundApprovalAction,
    ) -> Result<String, String> {
        if !channel.enabled {
            return Err("IM channel is disabled".to_string());
        }
        if !channel.forward_approvals {
            return Err("Remote approvals are disabled on this IM channel".to_string());
        }
        if !is_sender_authorized(channel, &action.chat_id, &action.user_id) {
            return Err("Sender/chat not allowlisted".to_string());
        }
        if !action.action_id.trim().is_empty() {
            let dedup_key = format!("act:{}:{}", channel.channel_id, action.action_id.trim());
            if !self.dedup.lock().await.check_and_insert(dedup_key) {
                return Ok("Already processed.".to_string());
            }
        }
        let _ = self
            .store
            .increment_im_channel_activity(&channel.channel_id, now_unix_ms());

        let normalized = normalize_decision(&action.decision);
        if let (Some(aid), Some(rid)) = (
            action
                .agent_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            action
                .request_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            // Security invariant: the target agent MUST be bound to this exact (channel_id, chat_id).
            if !self.is_agent_bound_to_chat(&channel.channel_id, &action.chat_id, aid) {
                return Err(format!(
                    "Agent `{aid}` is not bound to this IM channel/chat"
                ));
            }
            // Security invariant: (aid, rid) MUST exist in live pending permissions for `aid`.
            let pending = self.orchestrator.list_pending_permissions(Some(aid)).await;
            let Some(perm) = pending.into_iter().find(|p| p.request_id == rid) else {
                return Err(format!("No pending approval `{rid}` for agent `{aid}`"));
            };

            return match self
                .orchestrator
                .resolve_permission(aid, perm.request_id.clone(), normalized.clone())
                .await
            {
                Ok(()) => Ok(format!(
                    "{} `{}` ({rid}).",
                    if normalized == "allow" {
                        "Approved"
                    } else {
                        "Denied"
                    },
                    perm.title
                )),
                Err(e) => Err(format!("Failed to resolve approval: {e}")),
            };
        }

        let bound_agent = self
            .store
            .find_im_binding(&channel.channel_id, &action.chat_id, &action.thread_id)
            .ok()
            .flatten()
            .map(|b| b.agent_id);
        Ok(self
            .resolve_permission_from_im(
                bound_agent.as_deref(),
                action.request_id.as_deref(),
                &normalized,
            )
            .await)
    }

    async fn create_bound_agent(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        initial_prompt: &str,
    ) -> anyhow::Result<String> {
        let expanded = crate::expand_host_path(&channel.default_workspace_cwd);
        let cwd = if self.enforce_workspace_auth {
            crate::authorize_workspace_mutation_cwd(&self.store, &expanded)
                .await
                .map_err(|e| anyhow::anyhow!(e))?
                .to_string_lossy()
                .to_string()
        } else {
            expanded.to_string_lossy().to_string()
        };

        let title = format!("{} · {}", channel.name, truncate_short(chat_id, 16));
        let created = self
            .orchestrator
            .create_and_start_agent(CreateAgentSpec {
                cwd,
                extra_dirs: Vec::new(),
                title: Some(title),
                provider: channel.default_provider,
                model: None,
                effort: None,
                fast_mode: None,
                mode: channel.default_mode.clone(),
                initial_prompt: initial_prompt.to_string(),
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree: false,
                workspace_id: None,
                parent_agent_id: None,
                parent_host_id: None,
                profile_id: None,
                background: false,
                auto_archive: false,
                base_ref: None,
                worktree_mode: WorktreeCreateMode::BranchOff,
                branch: None,
                pr_number: None,
                handle: None,
                auto_cluster: None,
                acp_agent_id: None,
            })
            .await?;

        self.store.upsert_im_binding(&ImThreadBinding {
            channel_id: channel.channel_id.clone(),
            external_chat_id: chat_id.to_string(),
            external_thread_id: thread_id.to_string(),
            agent_id: created.agent_id.clone(),
            updated_at_ms: now_unix_ms(),
        })?;
        Ok(created.agent_id)
    }

    async fn register_callback_token(
        &self,
        channel_id: &str,
        chat_id: &str,
        agent_id: &str,
        request_id: &str,
    ) -> String {
        self.register_callback_token_at(channel_id, chat_id, agent_id, request_id, now_unix_ms())
            .await
    }

    async fn register_callback_token_at(
        &self,
        channel_id: &str,
        chat_id: &str,
        agent_id: &str,
        request_id: &str,
        created_at_ms: u64,
    ) -> String {
        let token = Uuid::new_v4().simple().to_string()[..12].to_string();
        let mut guard = self.callback_refs.lock().await;
        while guard.len() >= MAX_CALLBACK_TOKENS {
            guard.pop_front();
        }
        guard.push_back((
            token.clone(),
            CallbackApprovalRef {
                channel_id: channel_id.to_string(),
                chat_id: chat_id.to_string(),
                agent_id: agent_id.to_string(),
                request_id: request_id.to_string(),
                created_at_ms,
            },
        ));
        token
    }

    /// Single-use, channel+chat-owned, 1h-TTL callback token consumer.
    async fn take_callback_token(
        &self,
        token: &str,
        channel_id: &str,
        chat_id: &str,
        now_ms: u64,
    ) -> Option<(String, String)> {
        let mut guard = self.callback_refs.lock().await;
        // Evict expired entries first
        guard.retain(|(_, entry)| {
            now_ms.saturating_sub(entry.created_at_ms) <= CALLBACK_TOKEN_TTL_MS
        });

        let pos = guard.iter().position(|(k, entry)| {
            k == token && entry.channel_id == channel_id && entry.chat_id == chat_id
        })?;
        let (_, removed) = guard.remove(pos)?;
        Some((removed.agent_id, removed.request_id))
    }

    // ==================== OUTBOUND PLATFORM API CLIENTS (PURE RUST, DNS-PINNED, BOUNDED) ====================

    pub async fn send_text_message(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<(), String> {
        match channel.platform {
            ImPlatform::Lark => self.send_lark_text(channel, chat_id, thread_id, text).await,
            ImPlatform::Telegram => {
                self.send_telegram_text(channel, chat_id, thread_id, text)
                    .await
            }
            ImPlatform::Slack => {
                self.send_slack_text(channel, chat_id, thread_id, text)
                    .await
            }
            ImPlatform::Discord => self.send_discord_text(channel, chat_id, text).await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_approval_prompt(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        agent_id: &str,
        request_id: &str,
        title: &str,
        tool_summary: &str,
    ) -> Result<(), String> {
        match channel.platform {
            ImPlatform::Lark => {
                self.send_lark_approval_card(
                    channel,
                    chat_id,
                    thread_id,
                    agent_id,
                    request_id,
                    title,
                    tool_summary,
                )
                .await
            }
            ImPlatform::Telegram => {
                self.send_telegram_approval_prompt(
                    channel,
                    chat_id,
                    thread_id,
                    agent_id,
                    request_id,
                    title,
                    tool_summary,
                )
                .await
            }
            ImPlatform::Slack => {
                self.send_slack_approval_blocks(
                    channel,
                    chat_id,
                    thread_id,
                    agent_id,
                    request_id,
                    title,
                    tool_summary,
                )
                .await
            }
            ImPlatform::Discord => {
                self.send_discord_approval_prompt(
                    channel,
                    chat_id,
                    agent_id,
                    request_id,
                    title,
                    tool_summary,
                )
                .await
            }
        }
    }

    async fn get_lark_tenant_token(&self, channel: &StoredImChannel) -> Result<String, String> {
        let now = now_unix_ms();
        if let Some(cached) = self.lark_tokens.read().await.get(&channel.channel_id)
            && cached.expires_at_ms > now.saturating_add(120_000)
        {
            return Ok(cached.token.clone());
        }

        let (base_url, clean_app_id) = lark_api_base_and_app_id(channel.app_id.as_deref());
        let app_id = clean_app_id.ok_or_else(|| "Lark App ID is not configured".to_string())?;
        let app_secret = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Lark App Secret is not configured".to_string())?;

        let url = format!("{base_url}/open-apis/auth/v3/tenant_access_token/internal");
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
        let resp = client
            .post(parsed)
            .json(&json!({
                "app_id": app_id,
                "app_secret": app_secret,
            }))
            .send()
            .await
            .map_err(|e| format_reqwest_err("Lark auth request failed", e, Some(app_secret)))?;

        let status = resp.status();
        let body = read_bounded_response_json(resp, "Lark auth", Some(app_secret)).await?;
        let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if !status.is_success() || code != 0 {
            let msg = body
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!(
                "Lark tenant_access_token failed (code {code}): {msg}"
            ));
        }

        let token = body
            .get("tenant_access_token")
            .and_then(Value::as_str)
            .ok_or_else(|| "Lark auth response missing tenant_access_token".to_string())?
            .to_string();
        let expire_secs = body.get("expire").and_then(Value::as_u64).unwrap_or(7_200);
        let expires_at_ms = now.saturating_add(expire_secs.saturating_mul(1_000));
        self.lark_tokens.write().await.insert(
            channel.channel_id.clone(),
            CachedLarkToken {
                token: token.clone(),
                expires_at_ms,
            },
        );
        Ok(token)
    }

    async fn send_lark_text(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let token = self.get_lark_tenant_token(channel).await?;
        let (base_url, _) = lark_api_base_and_app_id(channel.app_id.as_deref());
        let chunks = chunk_utf8_message(text, LARK_MAX_CHARS);

        for chunk in chunks {
            let content_json = serde_json::to_string(&json!({ "text": chunk }))
                .map_err(|e| format!("serialize lark text: {e}"))?;
            let (url, payload) = if is_safe_lark_message_id(thread_id.trim()) {
                (
                    format!(
                        "{base_url}/open-apis/im/v1/messages/{}/reply",
                        thread_id.trim()
                    ),
                    json!({
                        "msg_type": "text",
                        "content": content_json,
                    }),
                )
            } else {
                let receive_id_type = lark_receive_id_type(chat_id);
                (
                    format!(
                        "{base_url}/open-apis/im/v1/messages?receive_id_type={receive_id_type}"
                    ),
                    json!({
                        "receive_id": chat_id,
                        "msg_type": "text",
                        "content": content_json,
                    }),
                )
            };

            let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
            let resp = client
                .post(parsed)
                .bearer_auth(&token)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format_reqwest_err("Lark sendMessage failed", e, Some(&token)))?;
            let status = resp.status();
            let body = read_bounded_response_json(resp, "Lark sendMessage", Some(&token)).await?;
            let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
            if !status.is_success() || code != 0 {
                let msg = body
                    .get("msg")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!("Lark sendMessage error (code {code}): {msg}"));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_lark_approval_card(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        agent_id: &str,
        request_id: &str,
        title: &str,
        tool_summary: &str,
    ) -> Result<(), String> {
        let token = self.get_lark_tenant_token(channel).await?;
        let (base_url, _) = lark_api_base_and_app_id(channel.app_id.as_deref());
        let card = build_lark_approval_card_json(agent_id, request_id, title, tool_summary);
        let content_json =
            serde_json::to_string(&card).map_err(|e| format!("serialize lark card: {e}"))?;

        let (url, payload) = if is_safe_lark_message_id(thread_id.trim()) {
            (
                format!(
                    "{base_url}/open-apis/im/v1/messages/{}/reply",
                    thread_id.trim()
                ),
                json!({
                    "msg_type": "interactive",
                    "content": content_json,
                }),
            )
        } else {
            let receive_id_type = lark_receive_id_type(chat_id);
            (
                format!("{base_url}/open-apis/im/v1/messages?receive_id_type={receive_id_type}"),
                json!({
                    "receive_id": chat_id,
                    "msg_type": "interactive",
                    "content": content_json,
                }),
            )
        };

        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
        let resp = client
            .post(parsed)
            .bearer_auth(&token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format_reqwest_err("Lark approval card send failed", e, Some(&token)))?;
        let status = resp.status();
        let body = read_bounded_response_json(resp, "Lark approval card", Some(&token)).await?;
        let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if !status.is_success() || code != 0 {
            let msg = body
                .get("msg")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("Lark approval card error (code {code}): {msg}"));
        }
        Ok(())
    }

    async fn send_telegram_text(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Telegram Bot Token is not configured".to_string())?;
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let chunks = chunk_utf8_message(text, TELEGRAM_MAX_CHARS);

        for chunk in chunks {
            let mut payload = json!({
                "chat_id": chat_id,
                "text": chunk,
            });
            if let Ok(mid) = thread_id.trim().parse::<i64>()
                && mid > 0
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("reply_to_message_id".to_string(), json!(mid));
            }
            let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
            let resp = client
                .post(parsed)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format_reqwest_err("Telegram sendMessage failed", e, Some(token)))?;
            let status = resp.status();
            let body =
                read_bounded_response_json(resp, "Telegram sendMessage", Some(token)).await?;
            if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                let desc = body
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error");
                return Err(format!(
                    "Telegram sendMessage error: {}",
                    redact_secret_in_text(desc, Some(token))
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_telegram_approval_prompt(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        agent_id: &str,
        request_id: &str,
        title: &str,
        tool_summary: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Telegram Bot Token is not configured".to_string())?;
        let cb_token = self
            .register_callback_token(&channel.channel_id, chat_id, agent_id, request_id)
            .await;
        let url = format!("https://api.telegram.org/bot{token}/sendMessage");
        let text = format!(
            "Orbit Approval Required\nAction: {title}\n{tool_summary}\n\nFallback: /approve {request_id} or /deny {request_id}"
        );
        let mut payload = json!({
            "chat_id": chat_id,
            "text": truncate_utf8_chars(&text, TELEGRAM_MAX_CHARS),
            "reply_markup": {
                "inline_keyboard": [[
                    { "text": "Approve", "callback_data": format!("orbit:allow:{cb_token}") },
                    { "text": "Deny", "callback_data": format!("orbit:deny:{cb_token}") },
                ]]
            }
        });
        if let Ok(mid) = thread_id.trim().parse::<i64>()
            && mid > 0
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("reply_to_message_id".to_string(), json!(mid));
        }

        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
        let resp = client
            .post(parsed)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Telegram approval sendMessage failed", e, Some(token))
            })?;
        let status = resp.status();
        let body = read_bounded_response_json(resp, "Telegram approval", Some(token)).await?;
        if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let desc = body
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!(
                "Telegram approval sendMessage error: {}",
                redact_secret_in_text(desc, Some(token))
            ));
        }
        Ok(())
    }

    async fn answer_telegram_callback(
        &self,
        token: &str,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let url = format!("https://api.telegram.org/bot{token}/answerCallbackQuery");
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(10)).await?;
        let _ = client
            .post(parsed)
            .json(&json!({
                "callback_query_id": callback_query_id,
                "text": truncate_utf8_chars(text, 180),
            }))
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Telegram answerCallbackQuery failed", e, Some(token))
            })?;
        Ok(())
    }

    async fn send_slack_text(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Slack Bot Token is not configured".to_string())?;
        let url = "https://slack.com/api/chat.postMessage";
        let chunks = chunk_utf8_message(text, SLACK_MAX_CHARS);

        for chunk in chunks {
            let mut payload = json!({
                "channel": chat_id,
                "text": chunk,
            });
            if !thread_id.trim().is_empty()
                && let Some(obj) = payload.as_object_mut()
            {
                obj.insert("thread_ts".to_string(), json!(thread_id.trim()));
            }
            let (client, parsed) = build_pinned_im_client(url, Duration::from_secs(15)).await?;
            let resp = client
                .post(parsed)
                .bearer_auth(token)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format_reqwest_err("Slack chat.postMessage failed", e, Some(token)))?;
            let status = resp.status();
            let body =
                read_bounded_response_json(resp, "Slack chat.postMessage", Some(token)).await?;
            if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                let err_code = body
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_error");
                return Err(format!("Slack chat.postMessage error: {err_code}"));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_slack_approval_blocks(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        thread_id: &str,
        agent_id: &str,
        request_id: &str,
        title: &str,
        tool_summary: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Slack Bot Token is not configured".to_string())?;
        let url = "https://slack.com/api/chat.postMessage";
        let allow_val = serde_json::to_string(&json!({
            "decision": "allow",
            "agent_id": agent_id,
            "request_id": request_id,
        }))
        .unwrap_or_default();
        let deny_val = serde_json::to_string(&json!({
            "decision": "deny",
            "agent_id": agent_id,
            "request_id": request_id,
        }))
        .unwrap_or_default();

        let mut payload = json!({
            "channel": chat_id,
            "text": format!("Orbit Approval Required: {title} ({request_id})"),
            "blocks": [
                {
                    "type": "section",
                    "text": {
                        "type": "mrkdwn",
                        "text": format!("*Orbit Approval Required*\n*Action:* {title}\n```{tool_summary}```\n_Fallback: `/approve {request_id}` or `/deny {request_id}`_")
                    }
                },
                {
                    "type": "actions",
                    "elements": [
                        {
                            "type": "button",
                            "style": "primary",
                            "action_id": "orbit_approve",
                            "text": { "type": "plain_text", "text": "Approve" },
                            "value": allow_val
                        },
                        {
                            "type": "button",
                            "style": "danger",
                            "action_id": "orbit_deny",
                            "text": { "type": "plain_text", "text": "Deny" },
                            "value": deny_val
                        }
                    ]
                }
            ]
        });
        if !thread_id.trim().is_empty()
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("thread_ts".to_string(), json!(thread_id.trim()));
        }

        let (client, parsed) = build_pinned_im_client(url, Duration::from_secs(15)).await?;
        let resp = client
            .post(parsed)
            .bearer_auth(token)
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Slack approval chat.postMessage failed", e, Some(token))
            })?;
        let status = resp.status();
        let body = read_bounded_response_json(resp, "Slack approval", Some(token)).await?;
        if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
            let err_code = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown_error");
            return Err(format!("Slack approval error: {err_code}"));
        }
        Ok(())
    }

    async fn send_discord_text(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Discord Bot Token is not configured".to_string())?;
        if chat_id.is_empty() || !chat_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "Discord target channel ID `{chat_id}` must be a numeric snowflake"
            ));
        }
        let url = format!("https://discord.com/api/v10/channels/{chat_id}/messages");
        let chunks = chunk_utf8_message(text, DISCORD_MAX_CHARS);

        for chunk in chunks {
            let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
            let resp = client
                .post(parsed)
                .header(reqwest::header::AUTHORIZATION, format!("Bot {token}"))
                .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
                .json(&json!({ "content": chunk }))
                .send()
                .await
                .map_err(|e| format_reqwest_err("Discord sendMessage failed", e, Some(token)))?;
            if !resp.status().is_success() {
                let code = resp.status().as_u16();
                let err_body = read_bounded_response_text(resp, Some(token))
                    .await
                    .unwrap_or_default();
                return Err(format!(
                    "Discord API error (HTTP {code}): {}",
                    truncate_short(&err_body, 180)
                ));
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn send_discord_approval_prompt(
        &self,
        channel: &StoredImChannel,
        chat_id: &str,
        agent_id: &str,
        request_id: &str,
        title: &str,
        tool_summary: &str,
    ) -> Result<(), String> {
        let token = channel
            .primary_secret
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| "Discord Bot Token is not configured".to_string())?;
        if chat_id.is_empty() || !chat_id.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!(
                "Discord target channel ID `{chat_id}` must be a numeric snowflake"
            ));
        }
        let cb_token = self
            .register_callback_token(&channel.channel_id, chat_id, agent_id, request_id)
            .await;
        let url = format!("https://discord.com/api/v10/channels/{chat_id}/messages");
        let content = truncate_utf8_chars(
            &format!(
                "**Orbit Approval Required**\n**Action:** {title}\n```\n{tool_summary}\n```\nFallback: `/approve {request_id}` or `/deny {request_id}`."
            ),
            DISCORD_MAX_CHARS,
        );
        let payload = json!({
            "content": content,
            "components": [{
                "type": 1,
                "components": [
                    {
                        "type": 2,
                        "style": 3,
                        "label": "Approve",
                        "custom_id": format!("orbit:allow:{cb_token}")
                    },
                    {
                        "type": 2,
                        "style": 4,
                        "label": "Deny",
                        "custom_id": format!("orbit:deny:{cb_token}")
                    }
                ]
            }]
        });
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
        let resp = client
            .post(parsed)
            .header(reqwest::header::AUTHORIZATION, format!("Bot {token}"))
            .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
            .json(&payload)
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Discord approval sendMessage failed", e, Some(token))
            })?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let err_body = read_bounded_response_text(resp, Some(token))
                .await
                .unwrap_or_default();
            return Err(format!(
                "Discord approval API error (HTTP {code}): {}",
                truncate_short(&err_body, 180)
            ));
        }
        Ok(())
    }

    async fn resolve_discord_interaction(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        interaction: &ParsedDiscordInteraction,
    ) -> Result<String, String> {
        if !channel.enabled {
            return Err("IM channel is disabled".to_string());
        }

        // Message component button click (`type == 3`): require single-use channel+chat callback token.
        // Security invariant: check `forward_approvals` and sender authorization BEFORE consuming
        // the single-use callback token so an unauthorized user in a shared chat cannot burn it.
        if interaction.kind == 3
            && let Some(custom_id) = interaction.custom_id.as_deref()
        {
            let parts: Vec<&str> = custom_id.split(':').collect();
            if parts.len() == 3 && parts[0] == "orbit" && !parts[2].trim().is_empty() {
                if !channel.forward_approvals {
                    return Err("Remote approvals are disabled on this IM channel".to_string());
                }
                if !is_sender_authorized(channel, &interaction.chat_id, &interaction.user_id) {
                    return Err("Sender/chat not allowlisted".to_string());
                }
                let decision = normalize_decision(parts[1]);
                let resolved = self
                    .take_callback_token(
                        parts[2].trim(),
                        &channel.channel_id,
                        &interaction.chat_id,
                        now_unix_ms(),
                    )
                    .await;
                return match resolved {
                    Some((agent_id, request_id)) => {
                        self.handle_inbound_approval_action(
                            channel,
                            InboundApprovalAction {
                                action_id: interaction.interaction_id.clone(),
                                chat_id: interaction.chat_id.clone(),
                                thread_id: String::new(),
                                user_id: interaction.user_id.clone(),
                                agent_id: Some(agent_id),
                                request_id: Some(request_id),
                                decision,
                            },
                        )
                        .await
                    }
                    None => Err("Approval prompt expired or invalid.".to_string()),
                };
            }
        }

        // Application command (`type == 2`)
        if interaction.kind == 2
            && let Some(cmd_text) = interaction.command_text.as_deref()
        {
            return match self
                .handle_inbound_message(
                    channel,
                    InboundImMessage {
                        message_id: interaction.interaction_id.clone(),
                        chat_id: interaction.chat_id.clone(),
                        thread_id: String::new(),
                        user_id: interaction.user_id.clone(),
                        text: cmd_text.to_string(),
                    },
                )
                .await
            {
                Ok(Some(reply)) => Ok(reply),
                Ok(None) => Ok("Command accepted.".to_string()),
                Err(err) => Err(err),
            };
        }

        Err("Unsupported Discord interaction".to_string())
    }

    pub(crate) async fn handle_discord_gateway_interaction(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        interaction: ParsedDiscordInteraction,
    ) {
        // Deduplicate by Discord interaction.id so retries never double-execute
        let inter_id = interaction.interaction_id.trim();
        if !inter_id.is_empty()
            && !self
                .check_and_mark_dedup(format!("discord_inter:{}:{inter_id}", channel.channel_id))
                .await
        {
            return;
        }

        let app_id_valid = is_safe_discord_snowflake(interaction.application_id.trim());
        if app_id_valid {
            // Immediately ACK within Discord's 3-second SLA using ephemeral type-5 DEFERRED response,
            // then run resolution and PATCH `@original` so slow commands/approvals never time out.
            if let Err(e) = self
                .defer_discord_interaction(
                    &interaction.interaction_id,
                    &interaction.interaction_token,
                )
                .await
            {
                warn!(
                    channel_id = %channel.channel_id,
                    error = %e,
                    "Failed to send Discord deferred interaction ACK"
                );
            }
            let feedback = match self
                .resolve_discord_interaction(channel, &interaction)
                .await
            {
                Ok(msg) => msg,
                Err(err) => err,
            };
            if let Err(e) = self
                .edit_discord_original_response(
                    &interaction.application_id,
                    &interaction.interaction_token,
                    &feedback,
                )
                .await
            {
                warn!(
                    channel_id = %channel.channel_id,
                    error = %e,
                    "Failed to edit deferred Discord Gateway interaction response"
                );
            }
            return;
        }

        // Fallback when application_id is absent: resolve inside a detached task with 2.2s ACK budget
        let b = Arc::clone(self);
        let ch = channel.clone();
        let inter_clone = interaction.clone();
        let feedback = crate::im_longconn::await_detached_with_deadline(
            Duration::from_millis(2_200),
            "Processing request…",
            async move { b.resolve_discord_interaction(&ch, &inter_clone).await },
        )
        .await;
        if let Err(e) = self
            .respond_discord_interaction(
                &interaction.interaction_id,
                &interaction.interaction_token,
                &feedback,
            )
            .await
        {
            warn!(
                channel_id = %channel.channel_id,
                error = %e,
                "Failed to answer Discord Gateway interaction"
            );
        }
    }

    async fn defer_discord_interaction(
        &self,
        interaction_id: &str,
        interaction_token: &str,
    ) -> Result<(), String> {
        let (id_trim, tok_trim) =
            validate_discord_interaction_coords(interaction_id, interaction_token)?;
        let url = format!("https://discord.com/api/v10/interactions/{id_trim}/{tok_trim}/callback");
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(10)).await?;
        let resp = client
            .post(parsed)
            .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
            .json(&json!({
                "type": 5,
                "data": { "flags": 64 }
            }))
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Discord deferred callback failed", e, Some(tok_trim))
            })?;
        if !resp.status().is_success() {
            return Err(format!(
                "Discord deferred callback HTTP {}",
                resp.status().as_u16()
            ));
        }
        Ok(())
    }

    async fn edit_discord_original_response(
        &self,
        application_id: &str,
        interaction_token: &str,
        content: &str,
    ) -> Result<(), String> {
        let app_trim = application_id.trim();
        let tok_trim = interaction_token.trim();
        if !is_safe_discord_snowflake(app_trim) {
            return Err("Invalid Discord application ID".to_string());
        }
        if !is_safe_discord_interaction_token(tok_trim) {
            return Err("Invalid Discord interaction token".to_string());
        }
        let url = format!(
            "https://discord.com/api/v10/webhooks/{app_trim}/{tok_trim}/messages/@original"
        );
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
        let resp = client
            .patch(parsed)
            .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
            .json(&json!({
                "content": truncate_utf8_chars(content, DISCORD_MAX_CHARS)
            }))
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err(
                    "Discord interaction followup edit failed",
                    e,
                    Some(tok_trim),
                )
            })?;
        if !resp.status().is_success() {
            return Err(format!(
                "Discord interaction followup edit HTTP {}",
                resp.status().as_u16()
            ));
        }
        Ok(())
    }

    async fn respond_discord_interaction(
        &self,
        interaction_id: &str,
        interaction_token: &str,
        content: &str,
    ) -> Result<(), String> {
        let (id_trim, tok_trim) =
            validate_discord_interaction_coords(interaction_id, interaction_token)?;
        let url = format!("https://discord.com/api/v10/interactions/{id_trim}/{tok_trim}/callback");
        let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(10)).await?;
        let resp = client
            .post(parsed)
            .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
            .json(&json!({
                "type": 4,
                "data": {
                    "content": truncate_utf8_chars(content, DISCORD_MAX_CHARS),
                    "flags": 64
                }
            }))
            .send()
            .await
            .map_err(|e| {
                format_reqwest_err("Discord interaction callback failed", e, Some(tok_trim))
            })?;
        if !resp.status().is_success() {
            return Err(format!(
                "Discord interaction callback HTTP {}",
                resp.status().as_u16()
            ));
        }
        Ok(())
    }

    // ==================== NAT-FRIENDLY POLLING LOOP (TELEGRAM) ====================

    async fn run_telegram_poller(self: Arc<Self>, channel_id: String) {
        let mut offset: i64 = 0;
        let mut backoff_secs: u64 = 2;

        loop {
            let channel = match self.store.get_im_channel(&channel_id) {
                Ok(Some(ch))
                    if ch.enabled
                        && ch.platform == ImPlatform::Telegram
                        && ch.transport_mode == ImTransportMode::LongConnection =>
                {
                    ch
                }
                _ => break,
            };
            let Some(token) = channel
                .primary_secret
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(ToOwned::to_owned)
            else {
                let _ = self.store.update_im_channel_status(
                    &channel_id,
                    ImConnectionStatus::Error,
                    Some("Bot token missing"),
                    now_unix_ms(),
                );
                break;
            };

            let url = format!(
                "https://api.telegram.org/bot{token}/getUpdates?timeout=25&offset={offset}&allowed_updates=%5B%22message%22%2C%22callback_query%22%5D"
            );
            let poll_res = async {
                let (client, parsed) =
                    build_pinned_im_client(&url, Duration::from_secs(35)).await?;
                let resp = client.get(parsed).send().await.map_err(|e| {
                    format_reqwest_err("Telegram getUpdates network error", e, Some(&token))
                })?;
                let status = resp.status();
                let body =
                    read_bounded_response_json(resp, "Telegram getUpdates", Some(&token)).await?;
                if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false)
                {
                    let desc = body
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("API error");
                    return Err(format!(
                        "Telegram getUpdates error: {}",
                        redact_secret_in_text(desc, Some(&token))
                    ));
                }
                Ok::<Value, String>(body)
            }
            .await;

            match poll_res {
                Ok(body) => {
                    backoff_secs = 2;
                    if channel.status != ImConnectionStatus::Connected {
                        let _ = self.store.update_im_channel_status(
                            &channel_id,
                            ImConnectionStatus::Connected,
                            Some("Long-polling active"),
                            now_unix_ms(),
                        );
                    }
                    if let Some(updates) = body.get("result").and_then(Value::as_array) {
                        for update in updates {
                            if let Some(uid) = update.get("update_id").and_then(Value::as_i64) {
                                offset = offset.max(uid.saturating_add(1));
                            }
                            self.process_telegram_update(&channel, &token, update).await;
                        }
                    }
                }
                Err(err) => {
                    warn!(channel_id = %channel_id, error = %err, "Telegram long-poll error");
                    let _ = self.store.update_im_channel_status(
                        &channel_id,
                        ImConnectionStatus::Error,
                        Some(&truncate_for_status(&err)),
                        now_unix_ms(),
                    );
                    tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
                    backoff_secs = (backoff_secs * 2).min(30);
                }
            }
        }
    }

    async fn process_telegram_update(
        self: &Arc<Self>,
        channel: &StoredImChannel,
        token: &str,
        update: &Value,
    ) {
        if let Some(cb) = parse_telegram_callback_query(update) {
            // Check authorization & forward_approvals BEFORE consuming the single-use callback token
            // so unauthorized users in a shared chat cannot burn an authorized approver's token.
            let outcome = if !channel.enabled {
                Err("IM channel is disabled".to_string())
            } else if !channel.forward_approvals {
                Err("Remote approvals are disabled on this IM channel".to_string())
            } else if !is_sender_authorized(channel, &cb.chat_id, &cb.user_id) {
                Err("Sender/chat not allowlisted".to_string())
            } else {
                // Require a valid, single-use, channel+chat-owned callback token — never accept forged raw IDs
                let resolved = match cb.callback_token.as_deref() {
                    Some(tok) => {
                        self.take_callback_token(
                            tok,
                            &channel.channel_id,
                            &cb.chat_id,
                            now_unix_ms(),
                        )
                        .await
                    }
                    None => None,
                };

                match resolved {
                    Some((resolved_agent, resolved_req)) => {
                        let action = InboundApprovalAction {
                            action_id: cb.callback_query_id.clone(),
                            chat_id: cb.chat_id.clone(),
                            thread_id: cb.thread_id.clone(),
                            user_id: cb.user_id.clone(),
                            agent_id: Some(resolved_agent),
                            request_id: Some(resolved_req),
                            decision: cb.decision.clone(),
                        };
                        self.handle_inbound_approval_action(channel, action).await
                    }
                    None => Err("Approval prompt expired or invalid.".to_string()),
                }
            };

            match outcome {
                Ok(msg) => {
                    let _ = self
                        .answer_telegram_callback(token, &cb.callback_query_id, &msg)
                        .await;
                    let _ = self
                        .send_telegram_text(channel, &cb.chat_id, &cb.thread_id, &msg)
                        .await;
                }
                Err(err) => {
                    // Only answer ephemeral callback toast — never spam the chat on rejected/unauthorized clicks
                    let _ = self
                        .answer_telegram_callback(token, &cb.callback_query_id, &err)
                        .await;
                }
            }
            return;
        }

        if let Some(msg) = parse_telegram_message(update)
            && let Ok(Some(reply)) = self.handle_inbound_message(channel, msg.clone()).await
        {
            let _ = self
                .send_telegram_text(channel, &msg.chat_id, &msg.thread_id, &reply)
                .await;
        }
    }

    // ==================== INBOUND WEBHOOK HANDLERS ====================

    pub async fn handle_lark_webhook(
        self: &Arc<Self>,
        channel_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Response {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Payload too large").into_response();
        }
        let Ok(Some(channel)) = self.store.get_im_channel(channel_id) else {
            return (StatusCode::NOT_FOUND, "Channel not found").into_response();
        };
        if channel.platform != ImPlatform::Lark
            || !channel.enabled
            || channel.transport_mode != ImTransportMode::Webhook
        {
            return (StatusCode::FORBIDDEN, "Channel unavailable").into_response();
        }

        let Ok(payload) = serde_json::from_slice::<Value>(body) else {
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        };

        let body_token = payload
            .get("token")
            .and_then(Value::as_str)
            .or_else(|| payload.pointer("/header/token").and_then(Value::as_str));

        let ts = header_str(headers, "x-lark-request-timestamp");
        let nonce = header_str(headers, "x-lark-request-nonce");
        let sig = header_str(headers, "x-lark-signature");
        let now_secs = Utc::now().timestamp();

        if let Err(reason) = verify_lark_callback(
            channel.signing_secret.as_deref().unwrap_or(""),
            ts,
            nonce,
            sig,
            body_token,
            body,
            now_secs,
        ) {
            warn!(channel_id = %channel.channel_id, reason = %reason, "Rejected Lark webhook");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }

        // 1. URL verification challenge
        if payload.get("type").and_then(Value::as_str) == Some("url_verification") {
            let challenge = payload
                .get("challenge")
                .and_then(Value::as_str)
                .unwrap_or("");
            return (StatusCode::OK, Json(json!({ "challenge": challenge }))).into_response();
        }

        // 2. Interactive Card approval button callback (`card.action.trigger` or legacy card action)
        if let Some(action) = parse_lark_card_action(&payload) {
            let feedback = match self.handle_inbound_approval_action(&channel, action).await {
                Ok(msg) => msg,
                Err(err) => err,
            };
            return (
                StatusCode::OK,
                Json(json!({
                    "toast": {
                        "type": "info",
                        "content": feedback,
                    }
                })),
            )
                .into_response();
        }

        // 3. `im.message.receive_v1` text event
        if let Some(inbound) = parse_lark_message_event(&payload) {
            self.spawn_serialized_inbound_message(channel, inbound);
        }

        (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
    }

    pub async fn handle_slack_webhook(
        self: &Arc<Self>,
        channel_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Response {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Payload too large").into_response();
        }
        let Ok(Some(channel)) = self.store.get_im_channel(channel_id) else {
            return (StatusCode::NOT_FOUND, "Channel not found").into_response();
        };
        if channel.platform != ImPlatform::Slack
            || !channel.enabled
            || channel.transport_mode != ImTransportMode::Webhook
        {
            return (StatusCode::FORBIDDEN, "Channel unavailable").into_response();
        }

        let ts = header_str(headers, "x-slack-request-timestamp").unwrap_or("");
        let sig = header_str(headers, "x-slack-signature").unwrap_or("");
        let now_secs = Utc::now().timestamp();
        if let Err(reason) = verify_slack_signature(
            channel.signing_secret.as_deref().unwrap_or(""),
            ts,
            sig,
            body,
            now_secs,
        ) {
            warn!(channel_id = %channel.channel_id, reason = %reason, "Rejected Slack webhook");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }

        // Slack Interactivity sends `application/x-www-form-urlencoded` with `payload=<json>`
        if let Some(form_json) = extract_slack_form_payload(body) {
            if let Some(action) = parse_slack_interactive_action(&form_json) {
                match self
                    .handle_inbound_approval_action(&channel, action.clone())
                    .await
                {
                    Ok(msg) => {
                        let bridge = Arc::clone(self);
                        let ch_clone = channel.clone();
                        let chat_id = action.chat_id.clone();
                        let thread_id = action.thread_id.clone();
                        let msg_clone = msg.clone();
                        tokio::spawn(async move {
                            let _ = bridge
                                .send_slack_text(&ch_clone, &chat_id, &thread_id, &msg_clone)
                                .await;
                        });
                        return (StatusCode::OK, Json(json!({ "text": msg }))).into_response();
                    }
                    Err(err) => {
                        // Return ephemeral HTTP response only — never spam the Slack channel on rejected clicks
                        return (StatusCode::OK, Json(json!({ "text": err }))).into_response();
                    }
                }
            }
            return (StatusCode::OK, Json(json!({ "ok": true }))).into_response();
        }

        let Ok(payload) = serde_json::from_slice::<Value>(body) else {
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        };

        if payload.get("type").and_then(Value::as_str) == Some("url_verification") {
            let challenge = payload
                .get("challenge")
                .and_then(Value::as_str)
                .unwrap_or("");
            return (StatusCode::OK, Json(json!({ "challenge": challenge }))).into_response();
        }

        if let Some(inbound) = parse_slack_event_callback(&payload) {
            self.spawn_serialized_inbound_message(channel, inbound);
        }

        (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
    }

    pub async fn handle_telegram_webhook(
        self: &Arc<Self>,
        channel_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Response {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Payload too large").into_response();
        }
        let Ok(Some(channel)) = self.store.get_im_channel(channel_id) else {
            return (StatusCode::NOT_FOUND, "Channel not found").into_response();
        };
        if channel.platform != ImPlatform::Telegram
            || !channel.enabled
            || channel.transport_mode != ImTransportMode::Webhook
        {
            return (StatusCode::FORBIDDEN, "Channel unavailable").into_response();
        }

        let secret_hdr = header_str(headers, "x-telegram-bot-api-secret-token");
        if let Err(reason) = verify_webhook_secret_header(
            channel.signing_secret.as_deref().unwrap_or(""),
            secret_hdr,
            "Telegram webhook secret",
        ) {
            warn!(channel_id = %channel.channel_id, reason = %reason, "Rejected Telegram webhook");
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }

        let Ok(update) = serde_json::from_slice::<Value>(body) else {
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        };
        let token = channel.primary_secret.clone().unwrap_or_default();
        let bridge = Arc::clone(self);
        let ch_clone = channel.clone();
        tokio::spawn(async move {
            bridge
                .process_telegram_update(&ch_clone, &token, &update)
                .await;
        });

        (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
    }

    pub async fn handle_discord_webhook(
        self: &Arc<Self>,
        channel_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> Response {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return (StatusCode::PAYLOAD_TOO_LARGE, "Payload too large").into_response();
        }
        let Ok(Some(channel)) = self.store.get_im_channel(channel_id) else {
            return (StatusCode::NOT_FOUND, "Channel not found").into_response();
        };
        if channel.platform != ImPlatform::Discord
            || !channel.enabled
            || channel.transport_mode != ImTransportMode::Webhook
        {
            return (StatusCode::FORBIDDEN, "Channel unavailable").into_response();
        }

        // Fail-closed strict Ed25519 signature verification (`X-Signature-Ed25519` + `X-Signature-Timestamp`)
        let sig_hdr = header_str(headers, "x-signature-ed25519").unwrap_or("");
        let ts_hdr = header_str(headers, "x-signature-timestamp").unwrap_or("");
        let now_secs = Utc::now().timestamp();
        if let Err(reason) = verify_discord_ed25519_signature(
            channel.public_key.as_deref().unwrap_or(""),
            ts_hdr,
            sig_hdr,
            body,
            now_secs,
        ) {
            warn!(
                channel_id = %channel.channel_id,
                reason = %reason,
                "Rejected Discord webhook (Ed25519 verification failed)"
            );
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }

        let Ok(payload) = serde_json::from_slice::<Value>(body) else {
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        };

        // 1. Discord interaction PING (`type == 1`)
        if payload.get("type").and_then(Value::as_u64) == Some(1) {
            return (StatusCode::OK, Json(json!({ "type": 1 }))).into_response();
        }

        // 2. Discord component button (`type == 3`) or slash command (`type == 2`) interaction
        if let Some(interaction) = parse_discord_interaction_create(&payload)
            && matches!(interaction.kind, 2 | 3)
        {
            let inter_id = interaction.interaction_id.trim();
            if !inter_id.is_empty()
                && !self
                    .check_and_mark_dedup(format!(
                        "discord_inter:{}:{inter_id}",
                        channel.channel_id
                    ))
                    .await
            {
                return (
                    StatusCode::OK,
                    Json(json!({
                        "type": 4,
                        "data": { "content": "Already processed.", "flags": 64 }
                    })),
                )
                    .into_response();
            }

            let can_followup = is_safe_discord_snowflake(interaction.application_id.trim())
                && is_safe_discord_interaction_token(interaction.interaction_token.trim());

            // Potentially slow slash commands (`type == 2`): acknowledge immediately with
            // ephemeral `type: 5` (DEFERRED_CHANNEL_MESSAGE_WITH_SOURCE) and PATCH `@original`
            // so Discord never hits its 3-second timeout and prompts duplicate retries.
            if interaction.kind == 2 && can_followup {
                let bridge = Arc::clone(self);
                let ch_clone = channel.clone();
                let inter_clone = interaction.clone();
                tokio::spawn(async move {
                    let feedback = match bridge
                        .resolve_discord_interaction(&ch_clone, &inter_clone)
                        .await
                    {
                        Ok(msg) => msg,
                        Err(err) => err,
                    };
                    let _ = bridge
                        .edit_discord_original_response(
                            &inter_clone.application_id,
                            &inter_clone.interaction_token,
                            &feedback,
                        )
                        .await;
                });
                return (
                    StatusCode::OK,
                    Json(json!({
                        "type": 5,
                        "data": { "flags": 64 }
                    })),
                )
                    .into_response();
            }

            // Button approvals (`type == 3`) or interactions without `application_id`: run inside
            // a detached task (never cancelled mid-flight!) and return instant `type: 4` if done
            // within 2s, or `type: 5` deferred + background PATCH `@original` if it runs longer.
            let (tx, rx) = tokio::sync::oneshot::channel();
            let bridge = Arc::clone(self);
            let ch_clone = channel.clone();
            let inter_clone = interaction.clone();
            tokio::spawn(async move {
                let feedback = match bridge
                    .resolve_discord_interaction(&ch_clone, &inter_clone)
                    .await
                {
                    Ok(msg) => msg,
                    Err(err) => err,
                };
                if tx.send(feedback.clone()).is_err() && can_followup {
                    let _ = bridge
                        .edit_discord_original_response(
                            &inter_clone.application_id,
                            &inter_clone.interaction_token,
                            &feedback,
                        )
                        .await;
                }
            });

            return match tokio::time::timeout(Duration::from_millis(2_000), rx).await {
                Ok(Ok(feedback)) => (
                    StatusCode::OK,
                    Json(json!({
                        "type": 4,
                        "data": {
                            "content": truncate_utf8_chars(&feedback, DISCORD_MAX_CHARS),
                            "flags": 64
                        }
                    })),
                )
                    .into_response(),
                _ if can_followup => (
                    StatusCode::OK,
                    Json(json!({
                        "type": 5,
                        "data": { "flags": 64 }
                    })),
                )
                    .into_response(),
                _ => (
                    StatusCode::OK,
                    Json(json!({
                        "type": 4,
                        "data": { "content": "Processing request…", "flags": 64 }
                    })),
                )
                    .into_response(),
            };
        }

        (StatusCode::BAD_REQUEST, "Unsupported Discord interaction").into_response()
    }
}

// ==================== AXUM HTTP ROUTE HANDLERS ====================

pub async fn lark_webhook_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    AxumPath(channel_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    runtime
        .im_channels
        .handle_lark_webhook(&channel_id, &headers, &body)
        .await
}

pub async fn slack_webhook_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    AxumPath(channel_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    runtime
        .im_channels
        .handle_slack_webhook(&channel_id, &headers, &body)
        .await
}

pub async fn telegram_webhook_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    AxumPath(channel_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    runtime
        .im_channels
        .handle_telegram_webhook(&channel_id, &headers, &body)
        .await
}

pub async fn discord_webhook_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    AxumPath(channel_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    runtime
        .im_channels
        .handle_discord_webhook(&channel_id, &headers, &body)
        .await
}

// ==================== CRYPTO, VERIFICATION & SECURITY HELPERS ====================

/// Format a `reqwest::Error` with its URL stripped (`without_url()`) AND any secret redacted
/// so plaintext Telegram bot tokens (embedded in URL paths) or bearer tokens NEVER reach logs,
/// SQLite `status_message`, or Wire RPC responses.
pub fn format_reqwest_err(
    prefix: &str,
    err: reqwest::Error,
    redact_secret: Option<&str>,
) -> String {
    let stripped = err.without_url().to_string();
    let safe = redact_secret_in_text(&stripped, redact_secret);
    format!("{prefix}: {safe}")
}

fn redact_secret_in_text(text: &str, redact_secret: Option<&str>) -> String {
    match redact_secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(secret) => text.replace(secret, "[REDACTED]"),
        None => text.to_string(),
    }
}

/// Read an outbound IM HTTP response body up to `MAX_IM_HTTP_RESPONSE_BYTES` (1 MiB hard cap).
async fn read_bounded_response_bytes(
    mut resp: reqwest::Response,
    label: &str,
    redact_secret: Option<&str>,
) -> Result<Vec<u8>, String> {
    if let Some(len) = resp.content_length()
        && len > MAX_IM_HTTP_RESPONSE_BYTES as u64
    {
        return Err(format!(
            "{label} response exceeds 1 MiB safety limit ({len} bytes)"
        ));
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| {
        format_reqwest_err(&format!("{label} response read failed"), e, redact_secret)
    })? {
        if buf.len().saturating_add(chunk.len()) > MAX_IM_HTTP_RESPONSE_BYTES {
            return Err(format!("{label} response exceeded 1 MiB safety limit"));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

pub(crate) async fn read_bounded_response_json(
    resp: reqwest::Response,
    label: &str,
    redact_secret: Option<&str>,
) -> Result<Value, String> {
    let bytes = read_bounded_response_bytes(resp, label, redact_secret).await?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{label} invalid JSON response: {e}"))
}

async fn read_bounded_response_text(
    resp: reqwest::Response,
    redact_secret: Option<&str>,
) -> Result<String, String> {
    let bytes = read_bounded_response_bytes(resp, "IM HTTP", redact_secret).await?;
    let raw = String::from_utf8_lossy(&bytes);
    Ok(redact_secret_in_text(&raw, redact_secret))
}

/// Constant-time byte-slice comparison (no early exit on content mismatch).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Pure-Rust RFC 2104 HMAC-SHA256 using `sha2::Sha256`.
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(msg);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let outer_digest = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer_digest);
    out
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn sha256_hex(input: &[u8]) -> String {
    let digest = Sha256::digest(input);
    hex_encode(&digest)
}

/// Verify Slack `X-Slack-Signature` (`v0=<hmac_sha256>`) and ±300s `X-Slack-Request-Timestamp`
/// using overflow-safe `abs_diff` and positive-timestamp checks.
pub fn verify_slack_signature(
    signing_secret: &str,
    timestamp_header: &str,
    signature_header: &str,
    raw_body: &[u8],
    now_unix_secs: i64,
) -> Result<(), String> {
    let secret = signing_secret.trim();
    if secret.is_empty() {
        return Err("Slack signing secret not configured".to_string());
    }
    let ts: i64 = timestamp_header
        .trim()
        .parse()
        .map_err(|_| "Missing or invalid X-Slack-Request-Timestamp".to_string())?;
    if ts <= 0 || now_unix_secs <= 0 || now_unix_secs.abs_diff(ts) > SIGNATURE_REPLAY_WINDOW_SECS {
        return Err("Slack request timestamp outside 5-minute replay window".to_string());
    }

    let mut base = Vec::with_capacity(16 + timestamp_header.len() + raw_body.len());
    base.extend_from_slice(b"v0:");
    base.extend_from_slice(timestamp_header.trim().as_bytes());
    base.push(b':');
    base.extend_from_slice(raw_body);

    let expected = format!("v0={}", hex_encode(&hmac_sha256(secret.as_bytes(), &base)));
    if !constant_time_eq(expected.as_bytes(), signature_header.trim().as_bytes()) {
        return Err("Invalid X-Slack-Signature".to_string());
    }
    Ok(())
}

/// Verify Lark/Feishu webhook callback via SHA-256 signature (`X-Lark-Signature` with timestamp + nonce + encrypt key)
/// or constant-time verification token (`token` / `header.token`). Fails closed if `signing_secret` is unset.
pub fn verify_lark_callback(
    signing_secret: &str,
    timestamp_header: Option<&str>,
    nonce_header: Option<&str>,
    signature_header: Option<&str>,
    body_token: Option<&str>,
    raw_body: &[u8],
    now_unix_secs: i64,
) -> Result<(), String> {
    let secret = signing_secret.trim();
    if secret.is_empty() {
        return Err("Lark verification token / encrypt key not configured".to_string());
    }

    if let (Some(ts_raw), Some(nonce), Some(sig)) =
        (timestamp_header, nonce_header, signature_header)
        && !sig.trim().is_empty()
    {
        let ts: i64 = ts_raw
            .trim()
            .parse()
            .map_err(|_| "Invalid X-Lark-Request-Timestamp".to_string())?;
        if ts <= 0
            || now_unix_secs <= 0
            || now_unix_secs.abs_diff(ts) > SIGNATURE_REPLAY_WINDOW_SECS
        {
            return Err("Lark request timestamp outside 5-minute replay window".to_string());
        }
        let mut hasher = Sha256::new();
        hasher.update(ts_raw.trim().as_bytes());
        hasher.update(nonce.trim().as_bytes());
        hasher.update(secret.as_bytes());
        hasher.update(raw_body);
        let expected = hex_encode(&hasher.finalize());
        if !constant_time_eq(
            expected.as_bytes(),
            sig.trim().to_ascii_lowercase().as_bytes(),
        ) {
            return Err("Invalid X-Lark-Signature".to_string());
        }
        return Ok(());
    }

    if let Some(tok) = body_token.map(str::trim).filter(|s| !s.is_empty()) {
        if constant_time_eq(secret.as_bytes(), tok.as_bytes()) {
            return Ok(());
        }
        return Err("Invalid Lark verification token".to_string());
    }

    Err("Missing Lark signature and verification token".to_string())
}

/// Constant-time header secret verification for Telegram / Discord webhook endpoints.
pub fn verify_webhook_secret_header(
    configured_secret: &str,
    provided_header: Option<&str>,
    label: &str,
) -> Result<(), String> {
    let secret = configured_secret.trim();
    if secret.is_empty() {
        return Err(format!("{label} is not configured"));
    }
    let provided = provided_header
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("Missing {label} header"))?;
    if !constant_time_eq(secret.as_bytes(), provided.as_bytes()) {
        return Err(format!("Invalid {label}"));
    }
    Ok(())
}

/// Default-deny sender/chat allowlist check.
/// - If BOTH `allowed_chat_ids` and `allowed_user_ids` are empty: DENY ALL (`false`).
/// - If only `allowed_chat_ids` is non-empty: require `chat_id` match.
/// - If only `allowed_user_ids` is non-empty: require `user_id` match.
/// - If both are non-empty: require BOTH `chat_id` and `user_id` match.
pub fn is_sender_authorized(channel: &StoredImChannel, chat_id: &str, user_id: &str) -> bool {
    let chats = &channel.allowed_chat_ids;
    let users = &channel.allowed_user_ids;
    if chats.is_empty() && users.is_empty() {
        return false;
    }
    let chat_trim = chat_id.trim();
    let user_trim = user_id.trim();
    let chat_ok = chats.is_empty() || chats.iter().any(|c| c == chat_trim);
    let user_ok = users.is_empty() || users.iter().any(|u| u == user_trim);
    chat_ok && user_ok
}

/// DNS-pinned, no-redirect, `.no_proxy()` HTTPS client restricted strictly to known IM platform hosts.
pub(crate) async fn build_pinned_im_client(
    url_raw: &str,
    timeout: Duration,
) -> Result<(reqwest::Client, reqwest::Url), String> {
    let parsed = reqwest::Url::parse(url_raw).map_err(|_| "Invalid IM API URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("IM platform API URL must use https://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("IM platform API URL must not embed credentials".to_string());
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "IM platform API URL missing host".to_string())?;
    if host_raw.ends_with('.') {
        return Err("IM platform API host must not have trailing dot".to_string());
    }
    let host_norm = host_raw.to_ascii_lowercase();
    if !ALLOWED_IM_API_HOSTS.contains(&host_norm.as_str()) {
        return Err(format!(
            "Refusing non-allowlisted IM API host `{host_norm}`"
        ));
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host_norm.as_str(), port))
        .await
        .map_err(|e| format!("DNS resolution failed for `{host_norm}`: {e}"))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("DNS returned no addresses for `{host_norm}`"));
    }
    for addr in &addrs {
        if is_blocked_im_ip(addr.ip()) {
            return Err(format!(
                "Refusing IM API target `{host_norm}` resolved to non-public IP `{}`",
                addr.ip()
            ));
        }
    }

    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .resolve_to_addrs(&host_norm, &addrs)
        .build()
        .map_err(|e| format_reqwest_err("Failed to construct pinned IM HTTP client", e, None))?;

    Ok((client, parsed))
}

/// Block loopback, private, link-local, CGNAT (`100.64.0.0/10`), `0.0.0.0/8`,
/// benchmark (`198.18.0.0/15`), reserved/future (`240.0.0.0/4`), multicast, broadcast,
/// IPv6 ULA/link-local, and IPv4-mapped non-public IPv6 addresses.
pub fn is_blocked_im_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || o[0] == 0 // 0.0.0.0/8 ("this network")
                || (o[0] == 100 && (o[1] & 0b1100_0000) == 64) // 100.64.0.0/10 CGNAT
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15 benchmarking
                || o[0] >= 240 // 240.0.0.0/4 reserved
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_blocked_im_ip(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Validate a Lark message ID (`om_` + `[A-Za-z0-9_-]`) before interpolating into reply URLs.
pub fn is_safe_lark_message_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.len() <= 3 || trimmed.len() > 128 || !trimmed.starts_with("om_") {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

// ==================== PLATFORM PAYLOAD PARSERS & FORMATTERS ====================

/// Resolve Lark/Feishu API base URL and clean App ID.
/// Prefix `app_id` with `lark:` to target international `https://open.larksuite.com`;
/// default (or `feishu:` prefix) targets `https://open.feishu.cn`.
pub fn lark_api_base_and_app_id(app_id: Option<&str>) -> (&'static str, Option<&str>) {
    let Some(raw) = app_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return ("https://open.feishu.cn", None);
    };
    if let Some(rest) = raw.strip_prefix("lark:") {
        let trimmed = rest.trim();
        (
            "https://open.larksuite.com",
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            },
        )
    } else if let Some(rest) = raw.strip_prefix("feishu:") {
        let trimmed = rest.trim();
        (
            "https://open.feishu.cn",
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            },
        )
    } else {
        ("https://open.feishu.cn", Some(raw))
    }
}

fn lark_receive_id_type(receive_id: &str) -> &'static str {
    if receive_id.starts_with("ou_") {
        "open_id"
    } else if receive_id.starts_with("on_") {
        "union_id"
    } else {
        "chat_id"
    }
}

/// Strip Lark bot mention placeholders like `@_user_1`, `@_user_2` cleanly.
pub fn strip_lark_mentions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b"@_user_") {
            let mut j = i + b"@_user_".len();
            let start_digits = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > start_digits {
                i = j;
                continue;
            }
        }
        if let Some(ch) = text[i..].chars().next() {
            out.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strip Slack bot mention tokens like `<@U01234567>` cleanly.
pub fn strip_slack_mentions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<@") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        if let Some(end) = after.find('>') {
            rest = &after[end + 1..];
        } else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

pub fn build_lark_approval_card_json(
    agent_id: &str,
    request_id: &str,
    title: &str,
    tool_summary: &str,
) -> Value {
    json!({
        "config": { "wide_screen_mode": true },
        "header": {
            "template": "orange",
            "title": {
                "tag": "plain_text",
                "content": "Orbit Approval Required"
            }
        },
        "elements": [
            {
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": format!("**Action:** {title}\n```\n{tool_summary}\n```\nFallback: `/approve {request_id}` or `/deny {request_id}`")
                }
            },
            {
                "tag": "action",
                "actions": [
                    {
                        "tag": "button",
                        "text": { "tag": "plain_text", "content": "Approve" },
                        "type": "primary",
                        "value": {
                            "orbit_action": "permission",
                            "decision": "allow",
                            "agent_id": agent_id,
                            "request_id": request_id
                        }
                    },
                    {
                        "tag": "button",
                        "text": { "tag": "plain_text", "content": "Deny" },
                        "type": "danger",
                        "value": {
                            "orbit_action": "permission",
                            "decision": "deny",
                            "agent_id": agent_id,
                            "request_id": request_id
                        }
                    }
                ]
            }
        ]
    })
}

pub fn parse_lark_message_event(payload: &Value) -> Option<InboundImMessage> {
    let event_type = payload
        .pointer("/header/event_type")
        .and_then(Value::as_str)?;
    if event_type != "im.message.receive_v1" {
        return None;
    }
    let sender_type = payload
        .pointer("/event/sender/sender_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    if sender_type == "app" || sender_type == "bot" {
        return None;
    }
    let message = payload.pointer("/event/message")?;
    let msg_type = message.get("message_type").and_then(Value::as_str)?;
    if msg_type != "text" {
        return None;
    }
    let content_raw = message.get("content").and_then(Value::as_str)?;
    let content_val: Value = serde_json::from_str(content_raw).ok()?;
    let raw_text = content_val.get("text").and_then(Value::as_str)?;
    let text = strip_lark_mentions(raw_text);
    if text.is_empty() {
        return None;
    }

    let chat_id = message.get("chat_id").and_then(Value::as_str)?.to_string();
    let msg_id = message
        .get("message_id")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/header/event_id").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let thread_id = message
        .get("root_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            message
                .get("thread_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("")
        .to_string();
    let user_id = payload
        .pointer("/event/sender/sender_id/open_id")
        .and_then(Value::as_str)
        .or_else(|| {
            payload
                .pointer("/event/sender/sender_id/user_id")
                .and_then(Value::as_str)
        })
        .unwrap_or("")
        .to_string();

    Some(InboundImMessage {
        message_id: msg_id,
        chat_id,
        thread_id,
        user_id,
        text,
    })
}

pub fn parse_lark_card_action(payload: &Value) -> Option<InboundApprovalAction> {
    // Supports v2 `card.action.trigger` schema AND legacy card callback schema
    let is_v2 = payload
        .pointer("/header/event_type")
        .and_then(Value::as_str)
        == Some("card.action.trigger");
    let action_value = if is_v2 {
        payload.pointer("/event/action/value")?
    } else if payload.pointer("/action/value").is_some() {
        payload.pointer("/action/value")?
    } else {
        return None;
    };

    let decision = action_value
        .get("decision")
        .and_then(Value::as_str)?
        .to_string();
    let agent_id = action_value
        .get("agent_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let request_id = action_value
        .get("request_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let chat_id = if is_v2 {
        payload
            .pointer("/event/context/open_chat_id")
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        payload
            .get("open_chat_id")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    .to_string();
    let user_id = if is_v2 {
        payload
            .pointer("/event/operator/open_id")
            .and_then(Value::as_str)
            .or_else(|| {
                payload
                    .pointer("/event/operator/user_id")
                    .and_then(Value::as_str)
            })
            .unwrap_or("")
    } else {
        payload.get("open_id").and_then(Value::as_str).unwrap_or("")
    }
    .to_string();
    let thread_id = if is_v2 {
        payload
            .pointer("/event/context/open_message_id")
            .and_then(Value::as_str)
            .unwrap_or("")
    } else {
        payload
            .get("open_message_id")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
    .to_string();

    // NEVER use the static Lark verification `token` as `action_id` (would collide across all callbacks and store a secret in dedup).
    let action_id = if let Some(evt_id) = payload
        .pointer("/header/event_id")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        evt_id.to_string()
    } else {
        match (
            thread_id.trim().is_empty(),
            request_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
        ) {
            (false, Some(rid)) => format!("{}:{}", thread_id.trim(), rid),
            (false, None) => thread_id.trim().to_string(),
            (true, Some(rid)) => rid.to_string(),
            (true, None) => String::new(),
        }
    };

    Some(InboundApprovalAction {
        action_id,
        chat_id,
        thread_id,
        user_id,
        agent_id,
        request_id,
        decision,
    })
}

pub fn parse_telegram_message(update: &Value) -> Option<InboundImMessage> {
    let msg = update.get("message")?;
    if msg
        .pointer("/from/is_bot")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let text = msg.get("text").and_then(Value::as_str)?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let chat_id = value_id_to_string(msg.pointer("/chat/id")?)?;
    let user_id = msg
        .pointer("/from/id")
        .and_then(value_id_to_string)
        .unwrap_or_default();
    let message_id = msg
        .get("message_id")
        .and_then(value_id_to_string)
        .or_else(|| update.get("update_id").and_then(value_id_to_string))
        .unwrap_or_default();
    let thread_id = msg
        .get("message_thread_id")
        .and_then(value_id_to_string)
        .unwrap_or_default();

    Some(InboundImMessage {
        message_id,
        chat_id,
        thread_id,
        user_id,
        text,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTelegramCallback {
    pub callback_query_id: String,
    pub chat_id: String,
    pub thread_id: String,
    pub user_id: String,
    pub decision: String,
    pub callback_token: Option<String>,
}

pub fn parse_telegram_callback_query(update: &Value) -> Option<ParsedTelegramCallback> {
    let cq = update.get("callback_query")?;
    let cq_id = cq.get("id").and_then(Value::as_str)?.to_string();
    let data = cq.get("data").and_then(Value::as_str)?;
    let parts: Vec<&str> = data.split(':').collect();
    if parts.len() != 3 || parts[0] != "orbit" || parts[2].trim().is_empty() {
        return None;
    }
    let decision = normalize_decision(parts[1]);
    let callback_token = Some(parts[2].trim().to_string());

    let chat_id = cq
        .pointer("/message/chat/id")
        .and_then(value_id_to_string)
        .unwrap_or_default();
    let thread_id = cq
        .pointer("/message/message_thread_id")
        .and_then(value_id_to_string)
        .unwrap_or_default();
    let user_id = cq
        .pointer("/from/id")
        .and_then(value_id_to_string)
        .unwrap_or_default();

    Some(ParsedTelegramCallback {
        callback_query_id: cq_id,
        chat_id,
        thread_id,
        user_id,
        decision,
        callback_token,
    })
}

pub fn parse_slack_event_callback(payload: &Value) -> Option<InboundImMessage> {
    if payload.get("type").and_then(Value::as_str) != Some("event_callback") {
        return None;
    }
    let ev = payload.get("event")?;
    let ev_type = ev.get("type").and_then(Value::as_str)?;
    if ev_type != "message" && ev_type != "app_mention" {
        return None;
    }
    if ev.get("bot_id").is_some()
        || ev.get("subtype").and_then(Value::as_str) == Some("bot_message")
    {
        return None;
    }
    let raw_text = ev.get("text").and_then(Value::as_str)?;
    let text = strip_slack_mentions(raw_text);
    if text.is_empty() {
        return None;
    }
    let chat_id = ev.get("channel").and_then(Value::as_str)?.to_string();
    let user_id = ev
        .get("user")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let thread_id = ev
        .get("thread_ts")
        .and_then(Value::as_str)
        .or_else(|| ev.get("ts").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let message_id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .or_else(|| ev.get("client_msg_id").and_then(Value::as_str))
        .or_else(|| ev.get("ts").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    Some(InboundImMessage {
        message_id,
        chat_id,
        thread_id,
        user_id,
        text,
    })
}

fn extract_slack_form_payload(body: &[u8]) -> Option<Value> {
    let raw = std::str::from_utf8(body).ok()?;
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        return None;
    }
    for pair in trimmed.split('&') {
        if let Some(val) = pair.strip_prefix("payload=") {
            let decoded = percent_decode_form(val);
            if let Ok(json_val) = serde_json::from_str::<Value>(&decoded) {
                return Some(json_val);
            }
        }
    }
    None
}

pub fn parse_slack_interactive_action(payload: &Value) -> Option<InboundApprovalAction> {
    if payload.get("type").and_then(Value::as_str) != Some("block_actions") {
        return None;
    }
    let first_action = payload.get("actions")?.as_array()?.first()?;
    let action_id_name = first_action.get("action_id").and_then(Value::as_str)?;
    if action_id_name != "orbit_approve" && action_id_name != "orbit_deny" {
        return None;
    }
    let value_str = first_action.get("value").and_then(Value::as_str)?;
    let parsed_val: Value = serde_json::from_str(value_str).ok()?;
    let decision = parsed_val
        .get("decision")
        .and_then(Value::as_str)
        .unwrap_or(if action_id_name == "orbit_approve" {
            "allow"
        } else {
            "deny"
        })
        .to_string();
    let agent_id = parsed_val
        .get("agent_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let request_id = parsed_val
        .get("request_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let chat_id = payload
        .pointer("/channel/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let user_id = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let thread_id = payload
        .pointer("/message/thread_ts")
        .and_then(Value::as_str)
        .or_else(|| payload.pointer("/message/ts").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let action_id = first_action
        .get("action_ts")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Some(InboundApprovalAction {
        action_id,
        chat_id,
        thread_id,
        user_id,
        agent_id,
        request_id,
        decision,
    })
}

fn is_safe_discord_snowflake(raw: &str) -> bool {
    !raw.is_empty() && raw.len() <= 32 && raw.bytes().all(|b| b.is_ascii_digit())
}

fn is_safe_discord_interaction_token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 512
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

fn validate_discord_interaction_coords<'a>(
    interaction_id: &'a str,
    interaction_token: &'a str,
) -> Result<(&'a str, &'a str), String> {
    let id_trim = interaction_id.trim();
    let tok_trim = interaction_token.trim();
    if !is_safe_discord_snowflake(id_trim) {
        return Err("Invalid Discord interaction ID".to_string());
    }
    if !is_safe_discord_interaction_token(tok_trim) {
        return Err("Invalid Discord interaction token".to_string());
    }
    Ok((id_trim, tok_trim))
}

// ==================== SLASH COMMANDS & UTF-8 CHUNKING ====================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImSlashCommand {
    Approve(Option<String>),
    Deny(Option<String>),
    Status,
    Stop,
    New(String),
}

pub fn parse_im_slash_command(raw: &str) -> Option<ImSlashCommand> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let mut parts = trimmed.splitn(2, char::is_whitespace);
    let cmd_raw = parts.next()?.to_ascii_lowercase();
    // Strip optional Telegram `@botname` suffix from `/status@my_orbit_bot`
    let cmd = cmd_raw.split('@').next().unwrap_or(&cmd_raw);
    let arg = parts.next().map(str::trim).unwrap_or("");
    let opt_arg = if arg.is_empty() {
        None
    } else {
        Some(arg.to_string())
    };

    match cmd {
        "/approve" | "/allow" => Some(ImSlashCommand::Approve(opt_arg)),
        "/deny" | "/reject" => Some(ImSlashCommand::Deny(opt_arg)),
        "/status" => Some(ImSlashCommand::Status),
        "/stop" | "/cancel" => Some(ImSlashCommand::Stop),
        "/new" => Some(ImSlashCommand::New(arg.to_string())),
        _ => None,
    }
}

/// Split a UTF-8 message cleanly into chunks of at most `max_chars` Unicode scalar values,
/// preferring paragraph (`\n\n`), line (`\n`), or whitespace boundaries. Never panics on CJK/emoji.
pub fn chunk_utf8_message(text: &str, max_chars: usize) -> Vec<String> {
    let limit = max_chars.max(1);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return vec![String::new()];
    }
    if trimmed.chars().count() <= limit {
        return vec![trimmed.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = trimmed;
    while !remaining.is_empty() {
        if remaining.chars().count() <= limit {
            chunks.push(remaining.to_string());
            break;
        }
        // Find byte offset of the `limit`-th char
        let hard_byte_end = remaining
            .char_indices()
            .nth(limit)
            .map(|(idx, _)| idx)
            .unwrap_or(remaining.len());
        let window = &remaining[..hard_byte_end];

        let split_at = window
            .rfind("\n\n")
            .filter(|&pos| pos > hard_byte_end / 4)
            .or_else(|| window.rfind('\n').filter(|&pos| pos > hard_byte_end / 3))
            .or_else(|| window.rfind(' ').filter(|&pos| pos > hard_byte_end / 3))
            .unwrap_or(hard_byte_end);

        let piece = remaining[..split_at].trim_end();
        if !piece.is_empty() {
            chunks.push(piece.to_string());
        }
        remaining = remaining[split_at..].trim_start();
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

fn truncate_utf8_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn truncate_short(text: &str, max_chars: usize) -> String {
    truncate_utf8_chars(text.trim(), max_chars)
}

pub(crate) fn truncate_for_status(text: &str) -> String {
    truncate_utf8_chars(text.trim(), 160)
}

fn summarize_tool_detail(detail: &ToolCallDetail) -> String {
    let raw = match detail {
        ToolCallDetail::Shell { command, cwd, .. } => match cwd {
            Some(dir) if !dir.is_empty() => format!("$ {command}  (in {dir})"),
            _ => format!("$ {command}"),
        },
        ToolCallDetail::FileEdit { path, .. } => format!("Edit file: {path}"),
        ToolCallDetail::FileRead { path, .. } => format!("Read file: {path}"),
        ToolCallDetail::Search { query, scope } => match scope {
            Some(s) => format!("Search `{query}` in {s}"),
            None => format!("Search `{query}`"),
        },
        ToolCallDetail::Subagent {
            role,
            prompt_preview,
            ..
        } => format!("Subagent ({role}): {prompt_preview}"),
        ToolCallDetail::Generic {
            tool_name,
            arguments_json,
        } => format!("{tool_name}: {arguments_json}"),
    };
    truncate_utf8_chars(&raw, 400)
}

fn normalize_decision(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "allow" | "approve" | "allow_once" | "allow_always" | "yes" | "ok" => "allow".to_string(),
        _ => "deny".to_string(),
    }
}

fn sanitize_channel_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        format!("im_{}", &Uuid::new_v4().simple().to_string()[..10])
    } else {
        cleaned
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

fn value_id_to_string(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        Some(s.to_string())
    } else if let Some(n) = v.as_i64() {
        Some(n.to_string())
    } else {
        v.as_u64().map(|n| n.to_string())
    }
}

fn percent_decode_form(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let (Some(hi), Some(lo)) = (hex_nibble(bytes[i + 1]), hex_nibble(bytes[i + 2])) {
                    out.push((hi << 4) | lo);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub(crate) fn now_unix_ms() -> u64 {
    u64::try_from(Utc::now().timestamp_millis()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::HooksEngine;
    use crate::keep_awake::KeepAwakeManager;
    use crate::workspace_runtime::WorkspaceScriptRegistry;
    use orbit_protocol::{
        AgentSummary, AttentionState, OrchestrationConfig, PendingPermissionSummary,
        WorkspaceSetupStatus, WorkspaceSummary,
    };

    async fn build_test_bridge() -> (
        Arc<OrbitStore>,
        Arc<AgentOrchestrator>,
        Arc<ImChannelBridge>,
    ) {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let tmp = std::env::temp_dir().join(format!("orbit-im-test-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let hooks = HooksEngine::new(tmp.clone());
        let orchestrator = AgentOrchestrator::new(
            tmp,
            "host_test".into(),
            "mcp_tok".into(),
            Arc::new(RwLock::new(7788)),
            Arc::new(RwLock::new(OrchestrationConfig::default())),
            Arc::clone(&store),
            WorkspaceScriptRegistry::new(),
            Arc::new(KeepAwakeManager::new(false)),
            Arc::new(RwLock::new(None)),
            hooks,
        )
        .await;
        let bridge = ImChannelBridge::new_for_test(Arc::clone(&store), Arc::clone(&orchestrator));
        (store, orchestrator, bridge)
    }

    fn sample_lark_channel() -> StoredImChannel {
        StoredImChannel {
            channel_id: "im_lark_test".into(),
            name: "Feishu Bot".into(),
            platform: ImPlatform::Lark,
            enabled: true,
            transport_mode: ImTransportMode::Webhook,
            status: ImConnectionStatus::Connected,
            status_message: None,
            webhook_path: "/im/lark/im_lark_test".into(),
            app_id: Some("cli_abc".into()),
            primary_secret: Some("app_secret_9999".into()),
            signing_secret: Some("lark_verify_secret_1234".into()),
            app_token: None,
            public_key: None,
            allowed_chat_ids: vec!["oc_allowed_1".into()],
            allowed_user_ids: vec!["ou_allowed_1".into()],
            default_workspace_cwd: "/tmp".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PerThreadAgent,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            message_count: 0,
            last_activity_at_ms: None,
            created_at_ms: 1_760_000_000_000,
            updated_at_ms: 1_760_000_000_000,
        }
    }

    fn seed_test_workspace_and_agent(store: &OrbitStore, agent_id: &str) {
        let now = Utc::now();
        store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "Test WS".into(),
                cwd: "/tmp".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::default(),
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("upsert workspace");
        store
            .upsert_agent(
                &AgentSummary {
                    agent_id: agent_id.into(),
                    workspace_id: "ws_1".into(),
                    cwd: "/tmp".into(),
                    title: "Test Agent".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    effort: None,
                    fast_mode: false,
                    mode: SessionMode::Default,
                    status: AgentStatus::Idle,
                    parent_agent_id: None,
                    parent_host_id: None,
                    latest_seq: 0,
                    timeline_epoch: 1,
                    created_at: now,
                    updated_at: now,
                    archived_at: None,
                    background: false,
                    auto_archive: false,
                    profile_id: None,
                    labels: HashMap::new(),
                    attention: AttentionState::None,
                    last_activity_at: Some(now),
                    queued_prompts: Vec::new(),
                    read_seq: 0,
                    is_pinned: false,
                    pinned_order: 0,
                    handle: None,
                    cluster_suggestion: None,
                    task_summary: None,
                    forked_from_agent_id: None,
                    forked_at_seq: None,
                    acp_agent_id: None,
                },
                None,
            )
            .expect("upsert agent");
    }

    #[tokio::test]
    async fn format_reqwest_err_strips_url_and_redacts_telegram_token() {
        let secret = "123456789:AAH_SUPER_SECRET_TELEGRAM_TOKEN";
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(200))
            .build()
            .expect("client");
        // Connect to an invalid/closed local port with the secret embedded in the URL path
        let url = format!("http://127.0.0.1:1/bot{secret}/getUpdates");
        let err = client.get(&url).send().await.expect_err("must fail");
        assert!(
            err.to_string().contains(secret),
            "raw reqwest::Error normally embeds URL with secret"
        );
        let formatted = format_reqwest_err("Telegram getUpdates failed", err, Some(secret));
        assert!(
            !formatted.contains(secret),
            "formatted error must NEVER leak bot token: {formatted}"
        );
        assert!(
            !formatted.contains("127.0.0.1:1/bot"),
            "formatted error must strip URL: {formatted}"
        );
    }

    #[test]
    fn debug_impls_redact_secrets() {
        let spec = UpsertImChannelSpec {
            channel_id: Some("im_1".into()),
            name: "Bot".into(),
            platform: ImPlatform::Slack,
            enabled: true,
            transport_mode: ImTransportMode::LongConnection,
            app_id: None,
            primary_secret: Some("super_secret_bot_token".into()),
            signing_secret: Some("super_secret_signing".into()),
            app_token: Some("xapp-1-SUPER-SECRET-APP-TOKEN".into()),
            public_key: None,
            keep_existing_secrets: true,
            allowed_chat_ids: vec!["123".into()],
            allowed_user_ids: Vec::new(),
            default_workspace_cwd: "/tmp".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PerThreadAgent,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
        };
        let dbg = format!("{spec:?}");
        assert!(!dbg.contains("super_secret_bot_token"));
        assert!(!dbg.contains("super_secret_signing"));
        assert!(!dbg.contains("xapp-1-SUPER-SECRET-APP-TOKEN"));
        assert!(dbg.contains("[REDACTED]"));

        let tok = CachedLarkToken {
            token: "t-secret-lark-token".into(),
            expires_at_ms: 999,
        };
        let tok_dbg = format!("{tok:?}");
        assert!(!tok_dbg.contains("t-secret-lark-token"));
        assert!(tok_dbg.contains("[REDACTED]"));
    }

    #[test]
    fn default_deny_allowlist_enforces_strict_authorization() {
        let mut ch = sample_lark_channel();
        // 1. Both empty -> DENY ALL (default-deny)
        ch.allowed_chat_ids.clear();
        ch.allowed_user_ids.clear();
        assert!(!is_sender_authorized(&ch, "oc_any", "ou_any"));

        // 2. Only chat allowlist populated -> requires chat match
        ch.allowed_chat_ids = vec!["oc_team".into()];
        assert!(is_sender_authorized(&ch, "oc_team", "ou_anyone"));
        assert!(!is_sender_authorized(&ch, "oc_other", "ou_anyone"));

        // 3. Only user allowlist populated -> requires user match
        ch.allowed_chat_ids.clear();
        ch.allowed_user_ids = vec!["ou_alice".into()];
        assert!(is_sender_authorized(&ch, "oc_any", "ou_alice"));
        assert!(!is_sender_authorized(&ch, "oc_any", "ou_bob"));

        // 4. Both populated -> requires BOTH chat AND user match
        ch.allowed_chat_ids = vec!["oc_team".into()];
        ch.allowed_user_ids = vec!["ou_alice".into()];
        assert!(is_sender_authorized(&ch, "oc_team", "ou_alice"));
        assert!(!is_sender_authorized(&ch, "oc_team", "ou_bob"));
        assert!(!is_sender_authorized(&ch, "oc_other", "ou_alice"));
    }

    #[test]
    fn slack_and_lark_replay_window_handles_i64_min_and_negative_timestamps_without_panic() {
        let secret = "8f742231b10e8888abcd99yyyzzz85a5";
        let now = 1_760_000_000i64;
        let ts = now.to_string();
        let body = br#"{"type":"url_verification","challenge":"abc123"}"#;
        let mut base = Vec::new();
        base.extend_from_slice(format!("v0:{ts}:").as_bytes());
        base.extend_from_slice(body);
        let valid_sig = format!("v0={}", hex_encode(&hmac_sha256(secret.as_bytes(), &base)));

        assert!(verify_slack_signature(secret, &ts, &valid_sig, body, now).is_ok());
        assert!(verify_slack_signature(secret, &ts, "v0=deadbeef", body, now).is_err());
        // Replay older than 5 minutes (301s) must fail
        let old_ts = (now - 301).to_string();
        assert!(verify_slack_signature(secret, &old_ts, &valid_sig, body, now).is_err());
        // Missing signing secret must fail closed
        assert!(verify_slack_signature("", &ts, &valid_sig, body, now).is_err());

        // Regression: i64::MIN / negative / zero / i64::MAX must NEVER panic or bypass
        for extreme in [
            i64::MIN.to_string(),
            "-1".to_string(),
            "0".to_string(),
            i64::MAX.to_string(),
        ] {
            assert!(
                verify_slack_signature(secret, &extreme, &valid_sig, body, now).is_err(),
                "Slack must reject extreme timestamp {extreme}"
            );
            assert!(
                verify_lark_callback(
                    secret,
                    Some(&extreme),
                    Some("nonce"),
                    Some("deadbeef"),
                    None,
                    body,
                    now
                )
                .is_err(),
                "Lark must reject extreme timestamp {extreme}"
            );
        }
    }

    #[test]
    fn lark_url_verification_mention_strip_card_action_never_uses_static_token_for_dedup() {
        assert_eq!(
            strip_lark_mentions("@_user_1  check build status"),
            "check build status"
        );
        assert_eq!(
            strip_lark_mentions("hello @_user_2 please @_user_10 deploy"),
            "hello please deploy"
        );
        assert_eq!(
            lark_api_base_and_app_id(Some("cli_123")),
            ("https://open.feishu.cn", Some("cli_123"))
        );
        assert_eq!(
            lark_api_base_and_app_id(Some("lark:cli_intl")),
            ("https://open.larksuite.com", Some("cli_intl"))
        );
        assert!(is_safe_lark_message_id("om_abc123_def-456"));
        assert!(!is_safe_lark_message_id("om_../evil"));
        assert!(!is_safe_lark_message_id("oc_not_message"));
        assert!(!is_safe_lark_message_id("om_"));

        // Legacy card callback payload containing static verification `token`
        let legacy_card = json!({
            "token": "STATIC_LARK_VERIFICATION_SECRET",
            "open_chat_id": "oc_allowed_1",
            "open_id": "ou_allowed_1",
            "open_message_id": "om_msg_100",
            "action": {
                "value": {
                    "decision": "allow",
                    "agent_id": "ag_1",
                    "request_id": "perm_7"
                }
            }
        });
        let action = parse_lark_card_action(&legacy_card).expect("parse legacy card");
        assert!(
            !action.action_id.contains("STATIC_LARK_VERIFICATION_SECRET"),
            "action_id must never contain static verification token"
        );
        assert_eq!(action.action_id, "om_msg_100:perm_7");

        // `im.message.receive_v1` event parsing with `@_user_1` strip
        let msg_payload = json!({
            "header": {
                "event_id": "evt_1",
                "event_type": "im.message.receive_v1",
                "token": "lark_verify_secret_1234"
            },
            "event": {
                "sender": {
                    "sender_type": "user",
                    "sender_id": { "open_id": "ou_allowed_1" }
                },
                "message": {
                    "message_id": "om_msg_1",
                    "root_id": "om_root_1",
                    "chat_id": "oc_allowed_1",
                    "message_type": "text",
                    "content": "{\"text\":\"@_user_1 /status\"}"
                }
            }
        });
        let parsed = parse_lark_message_event(&msg_payload).expect("parse lark message");
        assert_eq!(parsed.chat_id, "oc_allowed_1");
        assert_eq!(parsed.thread_id, "om_root_1");
        assert_eq!(parsed.user_id, "ou_allowed_1");
        assert_eq!(parsed.text, "/status");
    }

    #[test]
    fn blocked_im_ip_covers_all_non_public_ranges() {
        for blocked in [
            "0.0.0.0",
            "0.1.2.3",
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "198.18.0.1",
            "198.19.255.254",
            "240.0.0.1",
            "255.255.255.255",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "::ffff:198.18.0.5",
        ] {
            let ip: IpAddr = blocked.parse().expect("parse ip");
            assert!(is_blocked_im_ip(ip), "{blocked} must be blocked");
        }
        for allowed in ["1.1.1.1", "8.8.8.8", "149.154.167.220", "198.20.0.1"] {
            let ip: IpAddr = allowed.parse().expect("parse ip");
            assert!(!is_blocked_im_ip(ip), "{allowed} should be allowed");
        }
    }

    #[test]
    fn telegram_callback_query_and_utf8_chunking() {
        let update = json!({
            "update_id": 1001,
            "callback_query": {
                "id": "cq_99",
                "from": { "id": 777123 },
                "message": { "chat": { "id": -100555 }, "message_thread_id": 42 },
                "data": "orbit:allow:tok123456789"
            }
        });
        let cb = parse_telegram_callback_query(&update).expect("parse telegram callback");
        assert_eq!(cb.callback_query_id, "cq_99");
        assert_eq!(cb.chat_id, "-100555");
        assert_eq!(cb.thread_id, "42");
        assert_eq!(cb.user_id, "777123");
        assert_eq!(cb.decision, "allow");
        assert_eq!(cb.callback_token.as_deref(), Some("tok123456789"));

        // Forged 4-segment raw agent/request callback_data is rejected
        let forged = json!({
            "callback_query": {
                "id": "cq_forged",
                "from": { "id": 777123 },
                "message": { "chat": { "id": -100555 } },
                "data": "orbit:allow:ag_victim:perm_1"
            }
        });
        assert!(parse_telegram_callback_query(&forged).is_none());

        // UTF-8 CJK + emoji chunking must never split mid-codepoint or panic
        let cjk_emoji = "你好世界🚀".repeat(50);
        let chunks = chunk_utf8_message(&cjk_emoji, 30);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= 30);
        }
        assert_eq!(chunks.concat(), cjk_emoji);
    }

    #[test]
    fn bounded_dedup_cache_evicts_oldest() {
        let mut cache = BoundedDedupCache::new(16);
        for i in 0..16 {
            assert!(cache.check_and_insert(format!("k{i}")));
        }
        assert!(!cache.check_and_insert("k0".into()));
        assert!(cache.check_and_insert("k16".into()));
        assert!(cache.check_and_insert("k0".into()));
    }

    #[tokio::test]
    async fn upsert_keep_existing_secrets_preserves_and_explicit_false_clears() {
        let (store, _orch, bridge) = build_test_bridge().await;
        let snap = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_tg_1".into()),
                name: "Ops Telegram".into(),
                platform: ImPlatform::Telegram,
                enabled: false,
                transport_mode: ImTransportMode::LongConnection,
                app_id: None,
                primary_secret: Some("123456:ABCDEF_SECRET_TOKEN".into()),
                signing_secret: Some("tg_hook_secret_9876".into()),
                app_token: Some("xapp-1-A123-SECRET-5555".into()),
                public_key: None,
                keep_existing_secrets: true,
                allowed_chat_ids: vec!["10001".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("create channel");
        assert_eq!(snap.channels.len(), 1);
        assert!(snap.channels[0].has_primary_secret);
        assert!(snap.channels[0].has_signing_secret);
        assert!(snap.channels[0].has_app_token);
        assert_eq!(
            snap.channels[0].masked_primary_secret.as_deref(),
            Some("••••OKEN")
        );
        assert_eq!(
            snap.channels[0].masked_app_token.as_deref(),
            Some("••••5555")
        );

        let snap2 = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_tg_1".into()),
                name: "Ops Telegram Renamed".into(),
                platform: ImPlatform::Telegram,
                enabled: false,
                transport_mode: ImTransportMode::LongConnection,
                app_id: None,
                primary_secret: None,
                signing_secret: None,
                app_token: None,
                public_key: None,
                keep_existing_secrets: true,
                allowed_chat_ids: vec!["10001".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("update keeping secrets");
        assert!(snap2.channels[0].has_primary_secret);
        assert!(snap2.channels[0].has_signing_secret);
        assert!(snap2.channels[0].has_app_token);

        let snap3 = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_tg_1".into()),
                name: "Ops Telegram Cleared".into(),
                platform: ImPlatform::Telegram,
                enabled: false,
                transport_mode: ImTransportMode::LongConnection,
                app_id: None,
                primary_secret: Some("new_bot_token_1111".into()),
                signing_secret: None,
                app_token: None,
                public_key: None,
                keep_existing_secrets: false,
                allowed_chat_ids: vec!["10001".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("update clearing signing secret");
        assert!(snap3.channels[0].has_primary_secret);
        assert!(!snap3.channels[0].has_signing_secret);
        assert!(!snap3.channels[0].has_app_token);
        assert_eq!(snap3.channels[0].masked_signing_secret, None);
        assert_eq!(snap3.channels[0].masked_app_token, None);
        let stored = store
            .get_im_channel("im_tg_1")
            .expect("load")
            .expect("exists");
        assert_eq!(stored.signing_secret, None);
        assert_eq!(stored.app_token, None);
        assert_eq!(stored.primary_secret.as_deref(), Some("new_bot_token_1111"));
    }

    #[tokio::test]
    async fn approval_isolation_blocks_cross_agent_hijack_ambiguous_prefix_and_disabled_forwarding()
    {
        let (store, orchestrator, bridge) = build_test_bridge().await;
        let ch = sample_lark_channel();
        store.upsert_im_channel(&ch).expect("save channel");
        store
            .upsert_im_binding(&ImThreadBinding {
                channel_id: ch.channel_id.clone(),
                external_chat_id: "oc_allowed_1".into(),
                external_thread_id: "om_root_1".into(),
                agent_id: "ag_bound".into(),
                updated_at_ms: now_unix_ms(),
            })
            .expect("save binding");

        // Seed permission on an UNBOUND victim agent
        orchestrator
            .insert_pending_permission_for_test(PendingPermissionSummary {
                agent_id: "ag_victim".into(),
                agent_title: "Victim Agent".into(),
                workspace_id: "ws_1".into(),
                request_id: "perm_victim_1".into(),
                title: "Sensitive op".into(),
                detail: ToolCallDetail::Shell {
                    command: "echo secret".into(),
                    cwd: Some("/tmp".into()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
                human_only: false,
                created_at: Utc::now(),
            })
            .await;

        // 1. `/approve perm_victim_1` from chat bound to `ag_bound` MUST NOT resolve `ag_victim`'s permission
        let res = bridge
            .handle_inbound_message(
                &ch,
                InboundImMessage {
                    message_id: "m_hijack".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    text: "/approve perm_victim_1".into(),
                },
            )
            .await
            .expect("handled");
        assert!(res.as_deref().unwrap_or("").contains("No pending approval"));
        assert!(
            orchestrator.has_pending_permissions("ag_victim").await,
            "victim agent permission must remain untouched"
        );

        // 2. Direct card action naming `ag_victim` MUST fail ownership check
        let hijack_card = bridge
            .handle_inbound_approval_action(
                &ch,
                InboundApprovalAction {
                    action_id: "act_hijack".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    agent_id: Some("ag_victim".into()),
                    request_id: Some("perm_victim_1".into()),
                    decision: "allow".into(),
                },
            )
            .await;
        assert!(hijack_card.is_err());
        assert!(orchestrator.has_pending_permissions("ag_victim").await);

        // 3. Ambiguous prefix on `ag_bound` MUST be rejected
        for rid in ["perm_dup_1", "perm_dup_2"] {
            orchestrator
                .insert_pending_permission_for_test(PendingPermissionSummary {
                    agent_id: "ag_bound".into(),
                    agent_title: "Bound Agent".into(),
                    workspace_id: "ws_1".into(),
                    request_id: rid.into(),
                    title: "Op".into(),
                    detail: ToolCallDetail::Shell {
                        command: "ls".into(),
                        cwd: Some("/tmp".into()),
                        exit_code: None,
                        output_preview: None,
                    },
                    allowed_decisions: vec!["allow".into(), "deny".into()],
                    guardian_assessment: None,
                    plan_approval: None,
                    human_only: false,
                    created_at: Utc::now(),
                })
                .await;
        }
        let ambig = bridge
            .handle_inbound_message(
                &ch,
                InboundImMessage {
                    message_id: "m_ambig".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    text: "/approve perm_dup".into(),
                },
            )
            .await
            .expect("handled");
        assert!(ambig.as_deref().unwrap_or("").contains("Ambiguous"));
        assert_eq!(
            orchestrator
                .list_pending_permissions(Some("ag_bound"))
                .await
                .len(),
            2
        );

        // 4. `forward_approvals == false` MUST reject approval card actions
        let mut ch_no_fwd = ch.clone();
        ch_no_fwd.forward_approvals = false;
        let blocked_fwd = bridge
            .handle_inbound_approval_action(
                &ch_no_fwd,
                InboundApprovalAction {
                    action_id: "act_nofwd".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    agent_id: Some("ag_bound".into()),
                    request_id: Some("perm_dup_1".into()),
                    decision: "allow".into(),
                },
            )
            .await;
        assert!(blocked_fwd.is_err());

        // 5. Non-existent request_id on bound agent MUST be rejected
        let missing_req = bridge
            .handle_inbound_approval_action(
                &ch,
                InboundApprovalAction {
                    action_id: "act_missing".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    agent_id: Some("ag_bound".into()),
                    request_id: Some("perm_does_not_exist".into()),
                    decision: "allow".into(),
                },
            )
            .await;
        assert!(missing_req.is_err());
    }

    #[tokio::test]
    async fn callback_token_is_single_use_chat_owned_and_ttl_bounded() {
        let (_store, _orch, bridge) = build_test_bridge().await;
        let t0 = 1_760_000_000_000u64;
        let tok = bridge
            .register_callback_token_at("im_tg_1", "chat_a", "ag_1", "perm_1", t0)
            .await;

        // Wrong channel or wrong chat must fail without consuming token
        assert!(
            bridge
                .take_callback_token(&tok, "im_tg_other", "chat_a", t0 + 1000)
                .await
                .is_none()
        );
        assert!(
            bridge
                .take_callback_token(&tok, "im_tg_1", "chat_b", t0 + 1000)
                .await
                .is_none()
        );

        // Correct channel+chat within 1h TTL succeeds once, then single-use consumed
        let taken = bridge
            .take_callback_token(&tok, "im_tg_1", "chat_a", t0 + 5_000)
            .await;
        assert_eq!(taken, Some(("ag_1".to_string(), "perm_1".to_string())));
        assert!(
            bridge
                .take_callback_token(&tok, "im_tg_1", "chat_a", t0 + 6_000)
                .await
                .is_none(),
            "token must be single-use"
        );

        // Expired token (> 1h TTL) must be rejected
        let tok2 = bridge
            .register_callback_token_at("im_tg_1", "chat_a", "ag_1", "perm_2", t0)
            .await;
        assert!(
            bridge
                .take_callback_token(&tok2, "im_tg_1", "chat_a", t0 + CALLBACK_TOKEN_TTL_MS + 1)
                .await
                .is_none(),
            "expired token must be rejected"
        );
    }

    #[tokio::test]
    async fn pagination_delivers_replies_past_256_events_and_start_seeds_cursors_against_replay() {
        let (store, _orch, bridge) = build_test_bridge().await;
        let ch = sample_lark_channel();
        store.upsert_im_channel(&ch).expect("save channel");
        seed_test_workspace_and_agent(&store, "ag_long");
        store
            .upsert_im_binding(&ImThreadBinding {
                channel_id: ch.channel_id.clone(),
                external_chat_id: "oc_allowed_1".into(),
                external_thread_id: "".into(),
                agent_id: "ag_long".into(),
                updated_at_ms: now_unix_ms(),
            })
            .expect("save binding");

        // Append 270 Notice events followed by final AssistantMessage (beyond single 256-event page!)
        for i in 0..270 {
            store
                .append_timeline_event(
                    "ag_long",
                    1,
                    None,
                    TimelinePayload::Notice {
                        level: "info".into(),
                        message: format!("step {i}"),
                    },
                )
                .expect("append notice");
        }
        store
            .append_timeline_event(
                "ag_long",
                1,
                None,
                TimelinePayload::AssistantMessage {
                    item_id: "msg_final".into(),
                    text: "Final answer past 256 events".into(),
                    streaming: false,
                },
            )
            .expect("append assistant reply");

        let reply = bridge
            .collect_undelivered_assistant_reply("ag_long")
            .await
            .expect("must paginate past 256 events");
        assert_eq!(reply, "Final answer past 256 events");

        // Second call returns None (cursor advanced)
        assert!(
            bridge
                .collect_undelivered_assistant_reply("ag_long")
                .await
                .is_none()
        );

        // Simulate daemon restart with fresh ImChannelBridge: `start()` seeds cursor -> NO replay!
        let fresh_bridge =
            ImChannelBridge::new_for_test(Arc::clone(&store), Arc::clone(&bridge.orchestrator));
        fresh_bridge.start().await;
        assert!(
            fresh_bridge
                .collect_undelivered_assistant_reply("ag_long")
                .await
                .is_none(),
            "post-restart bridge must not replay pre-restart reply"
        );
        fresh_bridge.shutdown_all().await;
    }

    #[tokio::test]
    async fn bidirectional_slash_approve_and_lark_card_resolve_pending_permission() {
        let (store, orchestrator, bridge) = build_test_bridge().await;
        let ch = sample_lark_channel();
        store.upsert_im_channel(&ch).expect("save channel");
        store
            .upsert_im_binding(&ImThreadBinding {
                channel_id: ch.channel_id.clone(),
                external_chat_id: "oc_allowed_1".into(),
                external_thread_id: "om_root_1".into(),
                agent_id: "ag_live_1".into(),
                updated_at_ms: now_unix_ms(),
            })
            .expect("save binding");

        orchestrator
            .insert_pending_permission_for_test(PendingPermissionSummary {
                agent_id: "ag_live_1".into(),
                agent_title: "Test Agent".into(),
                workspace_id: "ws_1".into(),
                request_id: "perm_42".into(),
                title: "Run cargo test".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: Some("/tmp".into()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
                human_only: false,
                created_at: Utc::now(),
            })
            .await;
        assert!(orchestrator.has_pending_permissions("ag_live_1").await);

        // 1. Unallowlisted user trying `/approve` is rejected (default-deny)
        let denied_res = bridge
            .handle_inbound_message(
                &ch,
                InboundImMessage {
                    message_id: "m_unauth".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_attacker".into(),
                    text: "/approve perm_42".into(),
                },
            )
            .await;
        assert!(denied_res.is_err());
        assert!(orchestrator.has_pending_permissions("ag_live_1").await);

        // 2. Allowlisted user sending `/approve perm_42` resolves the pending permission
        let ok_res = bridge
            .handle_inbound_message(
                &ch,
                InboundImMessage {
                    message_id: "m_auth".into(),
                    chat_id: "oc_allowed_1".into(),
                    thread_id: "om_root_1".into(),
                    user_id: "ou_allowed_1".into(),
                    text: "/approve perm_42".into(),
                },
            )
            .await
            .expect("authorized approve");
        assert!(ok_res.as_deref().unwrap_or("").contains("Approved"));
        assert!(!orchestrator.has_pending_permissions("ag_live_1").await);

        // 3. Lark `card.action.trigger` webhook resolves a second pending permission
        orchestrator
            .insert_pending_permission_for_test(PendingPermissionSummary {
                agent_id: "ag_live_1".into(),
                agent_title: "Test Agent".into(),
                workspace_id: "ws_1".into(),
                request_id: "perm_99".into(),
                title: "Delete temp file".into(),
                detail: ToolCallDetail::Shell {
                    command: "rm /tmp/foo".into(),
                    cwd: Some("/tmp".into()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
                human_only: false,
                created_at: Utc::now(),
            })
            .await;
        let card_body = json!({
            "header": {
                "event_id": "evt_card_1",
                "event_type": "card.action.trigger",
                "token": "lark_verify_secret_1234"
            },
            "event": {
                "operator": { "open_id": "ou_allowed_1" },
                "context": {
                    "open_chat_id": "oc_allowed_1",
                    "open_message_id": "om_root_1"
                },
                "action": {
                    "value": {
                        "orbit_action": "permission",
                        "decision": "deny",
                        "agent_id": "ag_live_1",
                        "request_id": "perm_99"
                    }
                }
            }
        });
        let raw_bytes = serde_json::to_vec(&card_body).expect("encode card body");
        let resp = bridge
            .handle_lark_webhook("im_lark_test", &HeaderMap::new(), &raw_bytes)
            .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!orchestrator.has_pending_permissions("ag_live_1").await);
    }

    #[tokio::test]
    async fn test_channel_rejects_control_chars_and_oversized_chat_id() {
        let (_store, _orch, bridge) = build_test_bridge().await;
        let (ok1, msg1) = bridge.test_channel("im_any", "chat\nid", "hi").await;
        assert!(!ok1);
        assert!(msg1.contains("invalid"));

        let long_id = "a".repeat(257);
        let (ok2, msg2) = bridge.test_channel("im_any", &long_id, "hi").await;
        assert!(!ok2);
        assert!(msg2.contains("invalid"));
    }

    #[tokio::test]
    async fn discord_webhook_enforces_ed25519_and_resolves_button_approval() {
        use axum::http::HeaderValue;
        use ed25519_dalek::{Signer, SigningKey};

        let (store, orchestrator, bridge) = build_test_bridge().await;
        let signing_key = SigningKey::from_bytes(&[0x5au8; 32]);
        let pubkey_hex = hex_encode(signing_key.verifying_key().as_bytes());

        let dc_ch = StoredImChannel {
            channel_id: "im_dc_ed25519".into(),
            name: "Discord Webhook".into(),
            platform: ImPlatform::Discord,
            enabled: true,
            transport_mode: ImTransportMode::Webhook,
            status: ImConnectionStatus::Connected,
            status_message: Some("Webhook ready".into()),
            webhook_path: "/im/discord/im_dc_ed25519".into(),
            app_id: None,
            primary_secret: Some("dc_bot_token_secret".into()),
            signing_secret: None,
            app_token: None,
            public_key: Some(pubkey_hex),
            allowed_chat_ids: vec!["9988776655".into()],
            allowed_user_ids: vec!["444333".into()],
            default_workspace_cwd: "/tmp".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PerThreadAgent,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            message_count: 0,
            last_activity_at_ms: None,
            created_at_ms: now_unix_ms(),
            updated_at_ms: now_unix_ms(),
        };
        store.upsert_im_channel(&dc_ch).expect("save dc channel");
        store
            .upsert_im_binding(&ImThreadBinding {
                channel_id: dc_ch.channel_id.clone(),
                external_chat_id: "9988776655".into(),
                external_thread_id: String::new(),
                agent_id: "ag_dc_1".into(),
                updated_at_ms: now_unix_ms(),
            })
            .expect("save dc binding");

        let sign_headers = |body: &[u8], ts: i64| -> HeaderMap {
            let ts_str = ts.to_string();
            let mut msg = Vec::new();
            msg.extend_from_slice(ts_str.as_bytes());
            msg.extend_from_slice(body);
            let sig_hex = hex_encode(&signing_key.sign(&msg).to_bytes());
            let mut map = HeaderMap::new();
            map.insert(
                "x-signature-ed25519",
                HeaderValue::from_str(&sig_hex).expect("sig hdr"),
            );
            map.insert(
                "x-signature-timestamp",
                HeaderValue::from_str(&ts_str).expect("ts hdr"),
            );
            map
        };

        let now_secs = Utc::now().timestamp();
        let ping_body = br#"{"type":1}"#;

        // 1. Missing headers -> 401 UNAUTHORIZED (fail-closed)
        let resp_missing = bridge
            .handle_discord_webhook("im_dc_ed25519", &HeaderMap::new(), ping_body)
            .await;
        assert_eq!(resp_missing.status(), StatusCode::UNAUTHORIZED);

        // 2. Stale timestamp (> 5 min) -> 401 UNAUTHORIZED
        let stale_hdrs = sign_headers(ping_body, now_secs - 400);
        let resp_stale = bridge
            .handle_discord_webhook("im_dc_ed25519", &stale_hdrs, ping_body)
            .await;
        assert_eq!(resp_stale.status(), StatusCode::UNAUTHORIZED);

        // 3. Valid Ed25519 PING -> 200 OK
        let valid_ping_hdrs = sign_headers(ping_body, now_secs);
        let resp_ping = bridge
            .handle_discord_webhook("im_dc_ed25519", &valid_ping_hdrs, ping_body)
            .await;
        assert_eq!(resp_ping.status(), StatusCode::OK);

        // 4. Tampered body with valid signature for original body -> 401 UNAUTHORIZED
        let resp_tampered = bridge
            .handle_discord_webhook("im_dc_ed25519", &valid_ping_hdrs, br#"{"type":2}"#)
            .await;
        assert_eq!(resp_tampered.status(), StatusCode::UNAUTHORIZED);

        // 5. Signed type=3 button approval resolves pending permission
        orchestrator
            .insert_pending_permission_for_test(PendingPermissionSummary {
                agent_id: "ag_dc_1".into(),
                agent_title: "Discord Agent".into(),
                workspace_id: "ws_1".into(),
                request_id: "perm_dc_1".into(),
                title: "Run tests".into(),
                detail: ToolCallDetail::Shell {
                    command: "cargo test".into(),
                    cwd: Some("/tmp".into()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
                human_only: false,
                created_at: Utc::now(),
            })
            .await;
        let cb_tok = bridge
            .register_callback_token("im_dc_ed25519", "9988776655", "ag_dc_1", "perm_dc_1")
            .await;
        let btn_body = serde_json::to_vec(&json!({
            "id": "8877665544",
            "token": "interaction_tok",
            "type": 3,
            "channel_id": "9988776655",
            "member": { "user": { "id": "444333" } },
            "data": { "custom_id": format!("orbit:allow:{cb_tok}") }
        }))
        .expect("btn json");
        let btn_hdrs = sign_headers(&btn_body, now_secs);
        let resp_btn = bridge
            .handle_discord_webhook("im_dc_ed25519", &btn_hdrs, &btn_body)
            .await;
        assert_eq!(resp_btn.status(), StatusCode::OK);
        assert!(
            !orchestrator.has_pending_permissions("ag_dc_1").await,
            "signed Discord button click must resolve pending permission"
        );
    }

    #[tokio::test]
    async fn upsert_channel_validates_transport_modes_and_never_leaks_secrets() {
        use ed25519_dalek::SigningKey;

        let (_store, _orch, bridge) = build_test_bridge().await;

        // 1. Slack LongConnection without app_token -> Error status (no WS runner spawned)
        let slack_missing = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_slack_lc".into()),
                name: "Slack Socket".into(),
                platform: ImPlatform::Slack,
                enabled: true,
                transport_mode: ImTransportMode::LongConnection,
                app_id: None,
                primary_secret: Some("xoxb-secret-bot-token-9999".into()),
                signing_secret: None,
                app_token: None,
                public_key: None,
                keep_existing_secrets: false,
                allowed_chat_ids: vec!["C1".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("upsert slack missing app_token");
        let rec = slack_missing
            .channels
            .iter()
            .find(|c| c.channel_id == "im_slack_lc")
            .expect("slack channel present");
        assert_eq!(rec.status, ImConnectionStatus::Error);
        assert!(
            rec.status_message
                .as_deref()
                .unwrap_or("")
                .contains("xapp-")
        );

        // 2. Discord Webhook with invalid hex public_key -> Error; with valid Ed25519 key -> Webhook ready
        let dc_bad = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_dc_wh".into()),
                name: "Discord WH".into(),
                platform: ImPlatform::Discord,
                enabled: true,
                transport_mode: ImTransportMode::Webhook,
                app_id: None,
                primary_secret: Some("dc_bot_secret_8888".into()),
                signing_secret: None,
                app_token: None,
                public_key: Some("not-valid-hex".into()),
                keep_existing_secrets: false,
                allowed_chat_ids: vec!["12345".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("upsert dc bad key");
        let bad_rec = dc_bad
            .channels
            .iter()
            .find(|c| c.channel_id == "im_dc_wh")
            .expect("dc channel");
        assert_eq!(bad_rec.status, ImConnectionStatus::Error);

        let valid_pk = hex_encode(
            SigningKey::from_bytes(&[0x33u8; 32])
                .verifying_key()
                .as_bytes(),
        );
        let dc_ok = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_dc_wh".into()),
                name: "Discord WH".into(),
                platform: ImPlatform::Discord,
                enabled: true,
                transport_mode: ImTransportMode::Webhook,
                app_id: None,
                primary_secret: Some("dc_bot_secret_8888".into()),
                signing_secret: None,
                app_token: None,
                public_key: Some(valid_pk.clone()),
                keep_existing_secrets: false,
                allowed_chat_ids: vec!["12345".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect("upsert dc valid key");
        let ok_rec = dc_ok
            .channels
            .iter()
            .find(|c| c.channel_id == "im_dc_wh")
            .expect("dc channel ok");
        assert_eq!(ok_rec.status, ImConnectionStatus::Connected);
        assert_eq!(ok_rec.status_message.as_deref(), Some("Webhook ready"));
        assert_eq!(ok_rec.public_key.as_deref(), Some(valid_pk.as_str()));

        let json_out = serde_json::to_string(&dc_ok.channels).expect("serialize snapshot");
        assert!(!json_out.contains("xoxb-secret-bot-token-9999"));
        assert!(!json_out.contains("dc_bot_secret_8888"));
    }

    #[tokio::test]
    async fn webhook_handlers_reject_channels_in_long_connection_mode() {
        let (store, _orch, bridge) = build_test_bridge().await;
        let now = now_unix_ms();

        for (cid, platform) in [
            ("im_lc_lark", ImPlatform::Lark),
            ("im_lc_slack", ImPlatform::Slack),
            ("im_lc_tg", ImPlatform::Telegram),
            ("im_lc_dc", ImPlatform::Discord),
        ] {
            store
                .upsert_im_channel(&StoredImChannel {
                    channel_id: cid.into(),
                    name: format!("LC {cid}"),
                    platform,
                    enabled: true,
                    transport_mode: ImTransportMode::LongConnection,
                    status: ImConnectionStatus::Connected,
                    status_message: Some("Connected (WS)".into()),
                    webhook_path: platform.webhook_path_for(cid),
                    app_id: Some("cli_abc".into()),
                    primary_secret: Some("secret_123456789".into()),
                    signing_secret: Some("signing_123456789".into()),
                    app_token: Some("xapp-1-token-1234".into()),
                    public_key: Some("a".repeat(64)),
                    allowed_chat_ids: vec!["12345".into()],
                    allowed_user_ids: Vec::new(),
                    default_workspace_cwd: "/tmp".into(),
                    default_provider: ProviderKind::ClaudeCode,
                    default_mode: SessionMode::Default,
                    forward_approvals: true,
                    routing_mode: ImRoutingMode::PerThreadAgent,
                    router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
                    message_count: 0,
                    last_activity_at_ms: None,
                    created_at_ms: now,
                    updated_at_ms: now,
                })
                .expect("save lc channel");
        }

        assert_eq!(
            bridge
                .handle_lark_webhook("im_lc_lark", &HeaderMap::new(), b"{}")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            bridge
                .handle_slack_webhook("im_lc_slack", &HeaderMap::new(), b"{}")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            bridge
                .handle_telegram_webhook("im_lc_tg", &HeaderMap::new(), b"{}")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            bridge
                .handle_discord_webhook("im_lc_dc", &HeaderMap::new(), b"{}")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn discord_unauthorized_click_does_not_burn_callback_token_and_interaction_dedups() {
        use axum::http::HeaderValue;
        use ed25519_dalek::{Signer, SigningKey};

        let (store, orchestrator, bridge) = build_test_bridge().await;
        let signing_key = SigningKey::from_bytes(&[0x7bu8; 32]);
        let pubkey_hex = hex_encode(signing_key.verifying_key().as_bytes());

        let dc_ch = StoredImChannel {
            channel_id: "im_dc_auth_first".into(),
            name: "Discord Auth First".into(),
            platform: ImPlatform::Discord,
            enabled: true,
            transport_mode: ImTransportMode::Webhook,
            status: ImConnectionStatus::Connected,
            status_message: Some("Webhook ready".into()),
            webhook_path: "/im/discord/im_dc_auth_first".into(),
            app_id: None,
            primary_secret: Some("dc_bot_secret".into()),
            signing_secret: None,
            app_token: None,
            public_key: Some(pubkey_hex),
            allowed_chat_ids: vec!["9988776655".into()],
            allowed_user_ids: vec!["444333".into()],
            default_workspace_cwd: "/tmp".into(),
            default_provider: ProviderKind::ClaudeCode,
            default_mode: SessionMode::Default,
            forward_approvals: true,
            routing_mode: ImRoutingMode::PerThreadAgent,
            router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            message_count: 0,
            last_activity_at_ms: None,
            created_at_ms: now_unix_ms(),
            updated_at_ms: now_unix_ms(),
        };
        store.upsert_im_channel(&dc_ch).expect("save dc channel");
        store
            .upsert_im_binding(&ImThreadBinding {
                channel_id: dc_ch.channel_id.clone(),
                external_chat_id: "9988776655".into(),
                external_thread_id: String::new(),
                agent_id: "ag_dc_auth".into(),
                updated_at_ms: now_unix_ms(),
            })
            .expect("save dc binding");

        orchestrator
            .insert_pending_permission_for_test(PendingPermissionSummary {
                agent_id: "ag_dc_auth".into(),
                agent_title: "Discord Agent".into(),
                workspace_id: "ws_1".into(),
                request_id: "perm_auth_1".into(),
                title: "Deploy".into(),
                detail: ToolCallDetail::Shell {
                    command: "echo deploy".into(),
                    cwd: Some("/tmp".into()),
                    exit_code: None,
                    output_preview: None,
                },
                allowed_decisions: vec!["allow".into(), "deny".into()],
                guardian_assessment: None,
                plan_approval: None,
                human_only: false,
                created_at: Utc::now(),
            })
            .await;

        let cb_tok = bridge
            .register_callback_token(
                "im_dc_auth_first",
                "9988776655",
                "ag_dc_auth",
                "perm_auth_1",
            )
            .await;

        let sign_headers = |body: &[u8], ts: i64| -> HeaderMap {
            let ts_str = ts.to_string();
            let mut msg = Vec::new();
            msg.extend_from_slice(ts_str.as_bytes());
            msg.extend_from_slice(body);
            let sig_hex = hex_encode(&signing_key.sign(&msg).to_bytes());
            let mut map = HeaderMap::new();
            map.insert(
                "x-signature-ed25519",
                HeaderValue::from_str(&sig_hex).expect("sig hdr"),
            );
            map.insert(
                "x-signature-timestamp",
                HeaderValue::from_str(&ts_str).expect("ts hdr"),
            );
            map
        };
        let now_secs = Utc::now().timestamp();

        // 1. Unauthorized user in the same chat clicks first -> MUST NOT burn the callback token!
        let unauth_body = serde_json::to_vec(&json!({
            "id": "1000001",
            "token": "tok_unauth",
            "type": 3,
            "channel_id": "9988776655",
            "member": { "user": { "id": "999999" } },
            "data": { "custom_id": format!("orbit:allow:{cb_tok}") }
        }))
        .expect("unauth body");
        let resp_unauth = bridge
            .handle_discord_webhook(
                "im_dc_auth_first",
                &sign_headers(&unauth_body, now_secs),
                &unauth_body,
            )
            .await;
        assert_eq!(resp_unauth.status(), StatusCode::OK);
        assert!(
            orchestrator.has_pending_permissions("ag_dc_auth").await,
            "unauthorized click must leave pending permission unresolved"
        );

        // 2. Authorized user clicks the exact same token -> MUST SUCCEED because token wasn't burned!
        let auth_body = serde_json::to_vec(&json!({
            "id": "1000002",
            "token": "tok_auth",
            "type": 3,
            "channel_id": "9988776655",
            "member": { "user": { "id": "444333" } },
            "data": { "custom_id": format!("orbit:allow:{cb_tok}") }
        }))
        .expect("auth body");
        let resp_auth = bridge
            .handle_discord_webhook(
                "im_dc_auth_first",
                &sign_headers(&auth_body, now_secs),
                &auth_body,
            )
            .await;
        assert_eq!(resp_auth.status(), StatusCode::OK);
        assert!(
            !orchestrator.has_pending_permissions("ag_dc_auth").await,
            "authorized click must succeed because unauthorized click did not burn callback token"
        );

        // 3. Replaying same interaction_id returns immediate deduped 200 OK
        let resp_dup = bridge
            .handle_discord_webhook(
                "im_dc_auth_first",
                &sign_headers(&auth_body, now_secs),
                &auth_body,
            )
            .await;
        assert_eq!(resp_dup.status(), StatusCode::OK);

        // 4. Non-interaction MESSAGE_CREATE payload is rejected (dead fallback stripped)
        let msg_body = serde_json::to_vec(&json!({
            "id": "555",
            "channel_id": "9988776655",
            "author": { "id": "444333", "bot": false },
            "content": "hello"
        }))
        .expect("msg body");
        let resp_msg = bridge
            .handle_discord_webhook(
                "im_dc_auth_first",
                &sign_headers(&msg_body, now_secs),
                &msg_body,
            )
            .await;
        assert_eq!(resp_msg.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn duplicate_slack_socket_mode_app_token_rejected_on_upsert_and_reconcile() {
        let (store, _orch, bridge) = build_test_bridge().await;
        let shared_xapp = "xapp-1-SHARED-SOCKET-TOKEN-9999";

        // 1. First Slack channel with shared_xapp (disabled initially so reconcile doesn't dial network,
        //    then inserted directly as enabled for duplicate check)
        let now = now_unix_ms();
        store
            .upsert_im_channel(&StoredImChannel {
                channel_id: "im_slack_1".into(),
                name: "Slack Primary".into(),
                platform: ImPlatform::Slack,
                enabled: true,
                transport_mode: ImTransportMode::LongConnection,
                status: ImConnectionStatus::Connected,
                status_message: Some("Connected (WS)".into()),
                webhook_path: "/im/slack/im_slack_1".into(),
                app_id: None,
                primary_secret: Some("xoxb-primary-1111".into()),
                signing_secret: None,
                app_token: Some(shared_xapp.into()),
                public_key: None,
                allowed_chat_ids: vec!["C1".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
                message_count: 0,
                last_activity_at_ms: None,
                created_at_ms: now,
                updated_at_ms: now,
            })
            .expect("seed primary slack channel");

        // 2. Upserting a second enabled Slack LongConnection channel with the SAME xapp token MUST fail
        let dup_err = bridge
            .upsert_channel(UpsertImChannelSpec {
                channel_id: Some("im_slack_2".into()),
                name: "Slack Competing".into(),
                platform: ImPlatform::Slack,
                enabled: true,
                transport_mode: ImTransportMode::LongConnection,
                app_id: None,
                primary_secret: Some("xoxb-second-2222".into()),
                signing_secret: None,
                app_token: Some(shared_xapp.into()),
                public_key: None,
                keep_existing_secrets: false,
                allowed_chat_ids: vec!["C2".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
            })
            .await
            .expect_err("duplicate enabled Slack app_token must be rejected");
        let err_str = dup_err.to_string();
        assert!(err_str.contains("Slack Primary"));
        assert!(
            !err_str.contains(shared_xapp),
            "error must never leak raw app_token"
        );

        // 3. Defense-in-depth: if SQLite has two enabled Slack channels with the same xapp token,
        //    reconcile_pollers refuses to spawn the second socket and marks it Error
        store
            .upsert_im_channel(&StoredImChannel {
                channel_id: "im_slack_2".into(),
                name: "Slack Competing".into(),
                platform: ImPlatform::Slack,
                enabled: true,
                transport_mode: ImTransportMode::LongConnection,
                status: ImConnectionStatus::Connecting,
                status_message: None,
                webhook_path: "/im/slack/im_slack_2".into(),
                app_id: None,
                primary_secret: Some("xoxb-second-2222".into()),
                signing_secret: None,
                app_token: Some(shared_xapp.into()),
                public_key: None,
                allowed_chat_ids: vec!["C2".into()],
                allowed_user_ids: Vec::new(),
                default_workspace_cwd: "/tmp".into(),
                default_provider: ProviderKind::ClaudeCode,
                default_mode: SessionMode::Default,
                forward_approvals: true,
                routing_mode: ImRoutingMode::PerThreadAgent,
                router_mirror_policy: RouterImMirrorPolicy::MilestonesAndApprovals,
                message_count: 0,
                last_activity_at_ms: None,
                created_at_ms: now + 100,
                updated_at_ms: now + 100,
            })
            .expect("seed duplicate slack channel");

        bridge.reconcile_pollers().await;
        let second = store
            .get_im_channel("im_slack_2")
            .expect("load second")
            .expect("present");
        assert_eq!(second.status, ImConnectionStatus::Error);
        assert!(
            second
                .status_message
                .as_deref()
                .unwrap_or("")
                .contains("Duplicate Slack App-Level Token")
        );
        // Ensure only 1 poller task spawned (not 2)
        assert_eq!(bridge.pollers.lock().await.len(), 1);
        bridge.shutdown_all().await;
    }
}
