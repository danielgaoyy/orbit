//! Daemon-to-Daemon Connected Hosts (Multi-Host Fleet Mesh) for cross-host agent orchestration.
//!
//! Each enabled registration maintains one persistent outbound link to the target Daemon
//! (direct WebSocket or Noise NK E2EE Relay / `ssh+relay://`), pins its remote `host_id`,
//! supports correlated single-hop `WireMessage` RPC forwarding, and broadcasts remote
//! `AgentUpdated` / `TimelineLive` events for cross-host Finish Subscriptions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::{ClientHandshake, PairingOffer};
use orbit_protocol::{
    AgentSummary, ConnectedHostStatus, ConnectedHostSummary, HostInfo, PROTOCOL_VERSION,
    TimelineEvent, WireMessage,
};
use orbit_relay_transport::RelayConnector;
use orbit_store::{OrbitStore, StoredConnectedHost};
use tokio::sync::{Mutex, RwLock, broadcast, mpsc, oneshot};
use tokio::task::AbortHandle;
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message as TungMsg, protocol::WebSocketConfig},
};
use tracing::{info, warn};
use uuid::Uuid;

const DIRECT_BIN_ZSTD_WIRE: u8 = 0x00;
const E2EE_FRAME_WIRE: u8 = 0x00;

/// Event emitted from a remote connected daemon for cross-host observation.
#[derive(Debug, Clone)]
pub enum RemoteHostEvent {
    AgentUpdated {
        host_id: String,
        agent: Box<AgentSummary>,
    },
    TimelineLive {
        host_id: String,
        event: Box<TimelineEvent>,
    },
    HostDisconnected {
        host_id: String,
        reason: String,
    },
    HostRemoved {
        host_id: String,
    },
}

struct LiveHostLink {
    host_id: Option<String>,
    status: ConnectedHostStatus,
    last_error: Option<String>,
    remote_info: Option<HostInfo>,
    tx: Option<mpsc::Sender<WireMessage>>,
    pending_rpcs: Arc<Mutex<HashMap<Uuid, oneshot::Sender<WireMessage>>>>,
    abort_handle: Option<AbortHandle>,
}

/// Manages outbound Daemon-to-Daemon links and single-hop RPC routing.
pub struct ConnectedHostsManager {
    self_host_id: String,
    store: Arc<OrbitStore>,
    links: RwLock<HashMap<String, LiveHostLink>>,
    pub remote_events_tx: broadcast::Sender<RemoteHostEvent>,
    pub status_updates_tx: broadcast::Sender<ConnectedHostSummary>,
}

impl ConnectedHostsManager {
    pub fn new(self_host_id: String, store: Arc<OrbitStore>) -> Arc<Self> {
        let (remote_events_tx, _) = broadcast::channel(512);
        let (status_updates_tx, _) = broadcast::channel(128);
        Arc::new(Self {
            self_host_id,
            store,
            links: RwLock::new(HashMap::new()),
            remote_events_tx,
            status_updates_tx,
        })
    }

    /// Bootstrap all saved connected hosts from SQLite on daemon startup.
    pub async fn start_all(self: &Arc<Self>) {
        let Ok(saved) = self.store.list_connected_hosts() else {
            return;
        };
        for host in saved {
            self.ensure_link_task(host).await;
        }
    }

    pub async fn list_summaries(&self) -> Vec<ConnectedHostSummary> {
        let saved = self.store.list_connected_hosts().unwrap_or_default();
        let links = self.links.read().await;
        saved
            .into_iter()
            .map(|row| build_summary(&row, links.get(&row.registration_id)))
            .collect()
    }

    pub async fn add_from_pairing_uri(
        self: &Arc<Self>,
        label: Option<String>,
        pairing_uri: &str,
    ) -> anyhow::Result<ConnectedHostSummary> {
        let offer = PairingOffer::from_qr_uri(pairing_uri)
            .map_err(|e| anyhow::anyhow!("invalid pairing URI: {e}"))?;
        if offer.host_id == self.self_host_id {
            anyhow::bail!("cannot register this daemon as its own connected host");
        }
        // Check if already registered for the same pubkey/host_id -> update endpoints idempotently
        let existing = self.store.list_connected_hosts()?.into_iter().find(|h| {
            h.daemon_pubkey_b64 == offer.daemon_pubkey_b64
                || h.host_id.as_deref() == Some(&offer.host_id)
        });

        let now = Utc::now();
        let record = if let Some(mut ex) = existing {
            if let Some(l) = label.filter(|s| !s.trim().is_empty()) {
                ex.label = l;
            }
            ex.direct_endpoints = offer.direct_endpoints;
            ex.relay_url = offer.relay_url;
            ex.enabled = true;
            ex.updated_at = now;
            ex
        } else {
            StoredConnectedHost {
                registration_id: format!("ch_{}", Uuid::new_v4().simple()),
                host_id: Some(offer.host_id),
                label: label
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(offer.hostname),
                daemon_pubkey_b64: offer.daemon_pubkey_b64,
                direct_endpoints: offer.direct_endpoints,
                relay_url: offer.relay_url,
                enabled: true,
                created_at: now,
                updated_at: now,
            }
        };

        self.store.upsert_connected_host(&record)?;
        self.restart_link_task(record.clone()).await;

        let links = self.links.read().await;
        Ok(build_summary(&record, links.get(&record.registration_id)))
    }

    pub async fn update_registration(
        self: &Arc<Self>,
        registration_id: &str,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> anyhow::Result<ConnectedHostSummary> {
        let mut row = self
            .store
            .get_connected_host(registration_id)?
            .ok_or_else(|| anyhow::anyhow!("connected host `{registration_id}` not found"))?;
        if let Some(l) = label.filter(|s| !s.trim().is_empty()) {
            row.label = l;
        }
        if let Some(en) = enabled {
            row.enabled = en;
        }
        row.updated_at = Utc::now();
        self.store.upsert_connected_host(&row)?;
        self.restart_link_task(row.clone()).await;

        let links = self.links.read().await;
        let summary = build_summary(&row, links.get(&row.registration_id));
        let _ = self.status_updates_tx.send(summary.clone());
        Ok(summary)
    }

    pub async fn remove_registration(
        &self,
        registration_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let existing = self.store.get_connected_host(registration_id)?;
        let old_host_id = existing.as_ref().and_then(|r| r.host_id.clone());
        if let Some(link) = self.links.write().await.remove(registration_id)
            && let Some(abort) = link.abort_handle
        {
            abort.abort();
        }
        self.store.delete_connected_host(registration_id)?;
        if let Some(ref hid) = old_host_id {
            let _ = self.remote_events_tx.send(RemoteHostEvent::HostRemoved {
                host_id: hid.clone(),
            });
        }
        Ok(old_host_id)
    }

    pub async fn is_host_registered(&self, host_id: &str) -> bool {
        self.store
            .get_connected_host_by_host_id(host_id)
            .ok()
            .flatten()
            .is_some_and(|h| h.enabled)
    }

    /// Resolve a target host by `host_id`, `registration_id`, or `label` (case-insensitive).
    pub async fn resolve_host_id(&self, query: &str) -> Option<String> {
        let q = query.trim();
        let saved = self.store.list_connected_hosts().ok()?;
        for row in saved {
            if !row.enabled {
                continue;
            }
            if row.host_id.as_deref() == Some(q)
                || row.registration_id == q
                || row.label.eq_ignore_ascii_case(q)
            {
                return row.host_id;
            }
        }
        None
    }

    /// Send a single-hop correlated RPC to a connected remote Daemon and await its reply.
    pub async fn send_rpc(
        &self,
        target_host_id: &str,
        request_id: Uuid,
        msg: WireMessage,
        timeout: Duration,
    ) -> anyhow::Result<WireMessage> {
        let (tx, pending_rpcs) = {
            let links = self.links.read().await;
            let found = links
                .values()
                .find(|l| l.host_id.as_deref() == Some(target_host_id));
            let Some(link) = found else {
                anyhow::bail!("connected host `{target_host_id}` is not registered");
            };
            if link.status != ConnectedHostStatus::Online {
                anyhow::bail!(
                    "connected host `{target_host_id}` is not online (status: {:?}{})",
                    link.status,
                    link.last_error
                        .as_deref()
                        .map(|e| format!(", {e}"))
                        .unwrap_or_default()
                );
            }
            let Some(tx) = link.tx.clone() else {
                anyhow::bail!("connected host `{target_host_id}` transport unavailable");
            };
            (tx, Arc::clone(&link.pending_rpcs))
        };

        let (resp_tx, resp_rx) = oneshot::channel();
        pending_rpcs.lock().await.insert(request_id, resp_tx);
        if tx.send(msg).await.is_err() {
            pending_rpcs.lock().await.remove(&request_id);
            anyhow::bail!("failed to send RPC to host `{target_host_id}`: link closed");
        }

        match tokio::time::timeout(timeout, resp_rx).await {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) => {
                pending_rpcs.lock().await.remove(&request_id);
                anyhow::bail!("connected host `{target_host_id}` disconnected before replying");
            }
            Err(_) => {
                pending_rpcs.lock().await.remove(&request_id);
                anyhow::bail!("RPC to connected host `{target_host_id}` timed out");
            }
        }
    }

    async fn restart_link_task(self: &Arc<Self>, row: StoredConnectedHost) {
        if let Some(old) = self.links.write().await.remove(&row.registration_id)
            && let Some(abort) = old.abort_handle
        {
            abort.abort();
        }
        self.ensure_link_task(row).await;
    }

    async fn ensure_link_task(self: &Arc<Self>, row: StoredConnectedHost) {
        let reg_id = row.registration_id.clone();
        if !row.enabled {
            self.links.write().await.insert(
                reg_id.clone(),
                LiveHostLink {
                    host_id: row.host_id.clone(),
                    status: ConnectedHostStatus::Disabled,
                    last_error: None,
                    remote_info: None,
                    tx: None,
                    pending_rpcs: Arc::new(Mutex::new(HashMap::new())),
                    abort_handle: None,
                },
            );
            return;
        }

        let pending_rpcs = Arc::new(Mutex::new(HashMap::new()));
        let this = Arc::clone(self);
        let row_bg = row.clone();
        let pending_bg = Arc::clone(&pending_rpcs);
        let handle = tokio::spawn(async move {
            this.run_host_connection_loop(row_bg, pending_bg).await;
        });

        self.links.write().await.insert(
            reg_id,
            LiveHostLink {
                host_id: row.host_id,
                status: ConnectedHostStatus::Connecting,
                last_error: None,
                remote_info: None,
                tx: None,
                pending_rpcs,
                abort_handle: Some(handle.abort_handle()),
            },
        );
    }

    async fn run_host_connection_loop(
        self: Arc<Self>,
        mut row: StoredConnectedHost,
        pending_rpcs: Arc<Mutex<HashMap<Uuid, oneshot::Sender<WireMessage>>>>,
    ) {
        let mut backoff_secs = 1u64;
        loop {
            self.set_link_state(
                &row.registration_id,
                ConnectedHostStatus::Connecting,
                None,
                None,
                None,
            )
            .await;

            match connect_to_peer_daemon(&row, &self.self_host_id).await {
                Ok((host_info, tx, mut rx)) => {
                    // Verify pinned host_id identity to prevent silent MITM / identity swap
                    if let Some(ref pinned) = row.host_id
                        && pinned != &host_info.host_id
                    {
                        let msg = format!(
                            "pinned host identity mismatch: expected `{pinned}`, got `{}`",
                            host_info.host_id
                        );
                        warn!(reg_id = %row.registration_id, "{msg}");
                        self.set_link_state(
                            &row.registration_id,
                            ConnectedHostStatus::Error,
                            Some(msg),
                            None,
                            None,
                        )
                        .await;
                        return;
                    }
                    if row.host_id.is_none() {
                        row.host_id = Some(host_info.host_id.clone());
                        let _ = self
                            .store
                            .pin_connected_host_id(&row.registration_id, &host_info.host_id);
                    }

                    let remote_host_id = host_info.host_id.clone();
                    info!(
                        reg_id = %row.registration_id,
                        remote_host = %remote_host_id,
                        label = %row.label,
                        "Daemon-to-Daemon fleet mesh link ONLINE"
                    );
                    backoff_secs = 1;
                    self.set_link_state(
                        &row.registration_id,
                        ConnectedHostStatus::Online,
                        None,
                        Some(host_info),
                        Some(tx.clone()),
                    )
                    .await;

                    // Keepalive ping loop + inbound frame demuxer
                    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));
                    loop {
                        tokio::select! {
                            maybe_msg = rx.recv() => {
                                let Some(msg) = maybe_msg else { break; };
                                if let Some(req_id) = extract_response_request_id(&msg)
                                    && let Some(waiter) = pending_rpcs.lock().await.remove(&req_id)
                                {
                                    let _ = waiter.send(msg);
                                    continue;
                                }
                                match msg {
                                    WireMessage::AgentUpdated { agent } => {
                                        let _ = self.remote_events_tx.send(RemoteHostEvent::AgentUpdated {
                                            host_id: remote_host_id.clone(),
                                            agent: Box::new(agent),
                                        });
                                    }
                                    WireMessage::TimelineLive { event } => {
                                        let _ = self.remote_events_tx.send(RemoteHostEvent::TimelineLive {
                                            host_id: remote_host_id.clone(),
                                            event: Box::new(event),
                                        });
                                    }
                                    _ => {}
                                }
                            }
                            _ = ping_interval.tick() => {
                                let ts_ms = Utc::now().timestamp_millis().max(0) as u64;
                                if tx.send(WireMessage::Ping { ts_ms }).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }

                    // Drain failed RPC waiters on disconnect
                    pending_rpcs.lock().await.clear();
                    let _ = self
                        .remote_events_tx
                        .send(RemoteHostEvent::HostDisconnected {
                            host_id: remote_host_id,
                            reason: "connection dropped".into(),
                        });
                    self.set_link_state(
                        &row.registration_id,
                        ConnectedHostStatus::Connecting,
                        Some("connection lost; reconnecting".into()),
                        None,
                        None,
                    )
                    .await;
                }
                Err(e) => {
                    self.set_link_state(
                        &row.registration_id,
                        ConnectedHostStatus::Error,
                        Some(e.to_string()),
                        None,
                        None,
                    )
                    .await;
                }
            }

            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            backoff_secs = (backoff_secs * 2).min(15);
        }
    }

    async fn set_link_state(
        &self,
        registration_id: &str,
        status: ConnectedHostStatus,
        last_error: Option<String>,
        remote_info: Option<HostInfo>,
        tx: Option<mpsc::Sender<WireMessage>>,
    ) {
        {
            let mut links = self.links.write().await;
            if let Some(link) = links.get_mut(registration_id) {
                link.status = status;
                link.last_error = last_error;
                if let Some(ref info) = remote_info {
                    link.host_id = Some(info.host_id.clone());
                }
                link.remote_info = remote_info;
                link.tx = tx;
            }
        }
        if let Ok(Some(row)) = self.store.get_connected_host(registration_id) {
            let links = self.links.read().await;
            let summary = build_summary(&row, links.get(registration_id));
            let _ = self.status_updates_tx.send(summary);
        }
    }
}

fn build_summary(row: &StoredConnectedHost, link: Option<&LiveHostLink>) -> ConnectedHostSummary {
    ConnectedHostSummary {
        registration_id: row.registration_id.clone(),
        host_id: link
            .and_then(|l| l.host_id.clone())
            .or_else(|| row.host_id.clone()),
        label: row.label.clone(),
        daemon_pubkey_b64: row.daemon_pubkey_b64.clone(),
        direct_endpoints: row.direct_endpoints.clone(),
        relay_url: row.relay_url.clone(),
        enabled: row.enabled,
        status: link.map(|l| l.status).unwrap_or(if row.enabled {
            ConnectedHostStatus::Connecting
        } else {
            ConnectedHostStatus::Disabled
        }),
        last_error: link.and_then(|l| l.last_error.clone()),
        available_providers: link
            .and_then(|l| l.remote_info.as_ref())
            .map(|i| i.available_providers.clone())
            .unwrap_or_default(),
        hostname: link
            .and_then(|l| l.remote_info.as_ref())
            .map(|i| i.hostname.clone()),
        daemon_version: link
            .and_then(|l| l.remote_info.as_ref())
            .map(|i| i.daemon_version.clone()),
        updated_at: row.updated_at,
    }
}

fn extract_response_request_id(msg: &WireMessage) -> Option<Uuid> {
    match msg {
        WireMessage::DiscoveredProjectsSnapshot { request_id, .. }
        | WireMessage::DirectoryEntriesResponse { request_id, .. }
        | WireMessage::ModelsSnapshot { request_id, .. }
        | WireMessage::WorkspacesSnapshot { request_id, .. }
        | WireMessage::WorkspaceCreated { request_id, .. }
        | WireMessage::WorkspaceScriptsSnapshot { request_id, .. }
        | WireMessage::GitStatusResponse { request_id, .. }
        | WireMessage::GitDiffResponse { request_id, .. }
        | WireMessage::WorkspaceFilesResponse { request_id, .. }
        | WireMessage::WorkspaceSymbolsResponse { request_id, .. }
        | WireMessage::WorkspaceFileContentResponse { request_id, .. }
        | WireMessage::WorkspaceFileWritten { request_id, .. }
        | WireMessage::WorkspaceEntryMutated { request_id, .. }
        | WireMessage::AgentsSnapshot { request_id, .. }
        | WireMessage::AgentCreated { request_id, .. }
        | WireMessage::AgentForked { request_id, .. }
        | WireMessage::PendingPermissionsSnapshot { request_id, .. }
        | WireMessage::Ack { request_id }
        | WireMessage::TimelineCatchup { request_id, .. }
        | WireMessage::TerminalsSnapshot { request_id, .. }
        | WireMessage::TerminalAttached { request_id, .. }
        | WireMessage::TerminalCaptured { request_id, .. }
        | WireMessage::ProviderModelsSnapshot { request_id, .. }
        | WireMessage::AttachmentStaged { request_id, .. }
        | WireMessage::ProfilesSnapshot { request_id, .. }
        | WireMessage::ProfileUpserted { request_id, .. }
        | WireMessage::SchedulesSnapshot { request_id, .. }
        | WireMessage::ScheduleCreated { request_id, .. }
        | WireMessage::ScheduleRunsSnapshot { request_id, .. }
        | WireMessage::ConnectedHostsSnapshot { request_id, .. }
        | WireMessage::ConnectedHostAdded { request_id, .. }
        | WireMessage::OrchestrationConfigSnapshot { request_id, .. }
        | WireMessage::OrchestrationSkillsSnapshot { request_id, .. }
        | WireMessage::ManagedSkillsSnapshot { request_id, .. }
        | WireMessage::ManagedSkillContentResponse { request_id, .. }
        | WireMessage::KeepAwakeStatusSnapshot { request_id, .. }
        | WireMessage::ProviderSubagentsSnapshot { request_id, .. }
        | WireMessage::ProviderSubagentTimelineCatchup { request_id, .. }
        | WireMessage::WorktreeHandoffPreviewResult { request_id, .. }
        | WireMessage::WorktreeHandoffExecuted { request_id, .. }
        | WireMessage::ArtifactsSnapshot { request_id, .. }
        | WireMessage::ArtifactContent { request_id, .. }
        | WireMessage::InstructionFilesSnapshot { request_id, .. }
        | WireMessage::InstructionFileContentResponse { request_id, .. }
        | WireMessage::InstructionFileSavedResponse { request_id, .. }
        | WireMessage::ExternalMcpServersSnapshot { request_id, .. }
        | WireMessage::ExternalMcpProbeResponse { request_id, .. }
        | WireMessage::PathRulesSnapshot { request_id, .. }
        | WireMessage::MatchedPathRulesResponse { request_id, .. }
        | WireMessage::LspServersSnapshot { request_id, .. }
        | WireMessage::LspSymbolsResponse { request_id, .. }
        | WireMessage::LspDiagnosticsResponse { request_id, .. }
        | WireMessage::LspLocationsResponse { request_id, .. }
        | WireMessage::UsageInsightsResponse { request_id, .. }
        | WireMessage::ContextOccupancyResponse { request_id, .. }
        | WireMessage::LifecycleHooksSnapshot { request_id, .. }
        | WireMessage::HookTestResultResponse { request_id, .. }
        | WireMessage::ConditionalTimerCreated { request_id, .. }
        | WireMessage::ConditionalTimersSnapshot { request_id, .. }
        | WireMessage::ConditionalTimerUpdated { request_id, .. }
        | WireMessage::GuardianConfigSnapshot { request_id, .. }
        | WireMessage::GoalRunUpdated { request_id, .. }
        | WireMessage::GoalRunsSnapshot { request_id, .. }
        | WireMessage::MonitorWatchUpdated { request_id, .. }
        | WireMessage::MonitorWatchesSnapshot { request_id, .. }
        | WireMessage::EventTriggersSnapshot { request_id, .. }
        | WireMessage::EventTriggerTestResponse { request_id, .. }
        | WireMessage::BrowserSessionsSnapshot { request_id, .. }
        | WireMessage::BrowserSessionUpdated { request_id, .. }
        | WireMessage::BrowserActionResponse { request_id, .. }
        | WireMessage::WorkflowRunUpdated { request_id, .. }
        | WireMessage::WorkflowRunsSnapshot { request_id, .. }
        | WireMessage::ImChannelsSnapshot { request_id, .. }
        | WireMessage::ImChannelTestResponse { request_id, .. } => Some(*request_id),
        WireMessage::QuotaReport { request_id, .. }
        | WireMessage::ArtifactUpdated { request_id, .. }
        | WireMessage::ArtifactDeleted { request_id, .. }
        | WireMessage::RpcError { request_id, .. } => *request_id,
        _ => None,
    }
}

/// Connect to a remote peer Orbit Daemon via direct endpoints first, falling back to Noise NK Relay.
async fn connect_to_peer_daemon(
    row: &StoredConnectedHost,
    self_host_id: &str,
) -> anyhow::Result<(
    HostInfo,
    mpsc::Sender<WireMessage>,
    mpsc::Receiver<WireMessage>,
)> {
    let mut last_err = anyhow::anyhow!("no reachable endpoints");

    // 1. Direct WebSocket endpoints
    for ep in &row.direct_endpoints {
        match connect_direct_peer(ep, self_host_id).await {
            Ok((info, tx, rx)) => {
                if let Some(ref pinned) = row.host_id
                    && pinned != &info.host_id
                {
                    last_err = anyhow::anyhow!(
                        "direct endpoint `{ep}` belongs to `{}` instead of pinned `{pinned}`",
                        info.host_id
                    );
                    continue;
                }
                return Ok((info, tx, rx));
            }
            Err(e) => last_err = e,
        }
    }

    // 2. Noise NK E2EE Relay fallback (supports `ws://`, `wss://`, and zero-port `ssh+relay://`)
    if let Some(ref relay_url) = row.relay_url {
        match connect_relay_peer(relay_url, &row.daemon_pubkey_b64, self_host_id).await {
            Ok(conn) => return Ok(conn),
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}

async fn connect_direct_peer(
    ws_url: &str,
    self_host_id: &str,
) -> anyhow::Result<(
    HostInfo,
    mpsc::Sender<WireMessage>,
    mpsc::Receiver<WireMessage>,
)> {
    let mut ws_cfg = WebSocketConfig::default();
    ws_cfg.max_frame_size = Some(orbit_protocol::MAX_WS_FRAME_BYTES);
    ws_cfg.max_message_size = Some(orbit_protocol::MAX_WS_FRAME_BYTES);
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(2),
        connect_async_with_config(ws_url, Some(ws_cfg), false),
    )
    .await??;
    let (mut sink, mut stream) = ws.split();

    let hello = WireMessage::Hello {
        client_id: format!("daemon-mesh-{self_host_id}"),
        client_name: format!("Orbit Daemon Mesh ({self_host_id})"),
        protocol_version: PROTOCOL_VERSION,
        supports_zstd: true,
    };
    sink.send(TungMsg::Text(serde_json::to_string(&hello)?.into()))
        .await?;

    let host_info = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(Ok(msg)) = stream.next().await {
            let decoded = match msg {
                TungMsg::Text(t) => serde_json::from_str::<WireMessage>(&t).ok(),
                TungMsg::Binary(b) if !b.is_empty() && b[0] == DIRECT_BIN_ZSTD_WIRE => {
                    WireMessage::decode_zstd(&b[1..]).ok()
                }
                _ => None,
            };
            if let Some(WireMessage::Welcome { host }) = decoded {
                return Some(host);
            }
        }
        None
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("peer closed before Welcome"))?;

    let (out_tx, mut out_rx) = mpsc::channel::<WireMessage>(256);
    let (in_tx, in_rx) = mpsc::channel::<WireMessage>(512);

    tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            if let Ok(z) = msg.encode_zstd(3) {
                let mut frame = Vec::with_capacity(1 + z.len());
                frame.push(DIRECT_BIN_ZSTD_WIRE);
                frame.extend_from_slice(&z);
                if sink.send(TungMsg::Binary(frame.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let decoded = match msg {
                TungMsg::Text(t) => serde_json::from_str::<WireMessage>(&t).ok(),
                TungMsg::Binary(b) if !b.is_empty() && b[0] == DIRECT_BIN_ZSTD_WIRE => {
                    WireMessage::decode_zstd(&b[1..]).ok()
                }
                TungMsg::Close(_) => break,
                _ => None,
            };
            if let Some(wire) = decoded
                && in_tx.send(wire).await.is_err()
            {
                break;
            }
        }
    });

    Ok((host_info, out_tx, in_rx))
}

async fn connect_relay_peer(
    relay_base: &str,
    daemon_pubkey_b64: &str,
    self_host_id: &str,
) -> anyhow::Result<(
    HostInfo,
    mpsc::Sender<WireMessage>,
    mpsc::Receiver<WireMessage>,
)> {
    let connector = RelayConnector::connect(relay_base).await?;
    let path = format!("/v1/client/{daemon_pubkey_b64}");
    let ws = connector.open_ws(&path).await?;
    let (mut sink, mut stream) = ws.split();

    let pubkey_bytes = orbit_crypto::decode_key_32(daemon_pubkey_b64)?;
    let (hs, msg1) = ClientHandshake::new(&pubkey_bytes)?;
    sink.send(TungMsg::Binary(msg1.into())).await?;

    let msg2 = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(m)) = stream.next().await {
            if let TungMsg::Binary(b) = m {
                return Some(b);
            }
        }
        None
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("relay closed before Noise msg2"))?;

    let chan = Arc::new(Mutex::new(hs.finalize(&msg2)?));

    // Send Hello inside E2EE channel
    let hello = WireMessage::Hello {
        client_id: format!("daemon-mesh-{self_host_id}"),
        client_name: format!("Orbit Daemon Mesh ({self_host_id})"),
        protocol_version: PROTOCOL_VERSION,
        supports_zstd: true,
    };
    let z = hello.encode_zstd(3)?;
    let mut plain = Vec::with_capacity(1 + z.len());
    plain.push(E2EE_FRAME_WIRE);
    plain.extend_from_slice(&z);
    let ct = chan.lock().await.seal_record(&plain)?;
    sink.send(TungMsg::Binary(ct.into())).await?;

    let host_info = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(Ok(m)) = stream.next().await {
            if let TungMsg::Binary(ct) = m
                && let Ok(p) = chan.lock().await.open_record(&ct)
                && !p.is_empty()
                && p[0] == E2EE_FRAME_WIRE
                && let Ok(WireMessage::Welcome { host }) = WireMessage::decode_zstd(&p[1..])
            {
                return Some(host);
            }
        }
        None
    })
    .await?
    .ok_or_else(|| anyhow::anyhow!("relay peer closed before Welcome"))?;

    let (out_tx, mut out_rx) = mpsc::channel::<WireMessage>(256);
    let (in_tx, in_rx) = mpsc::channel::<WireMessage>(512);

    let chan_w = Arc::clone(&chan);
    tokio::spawn(async move {
        let _keep_connector = connector;
        while let Some(msg) = out_rx.recv().await {
            if let Ok(z) = msg.encode_zstd(3) {
                let mut plain = Vec::with_capacity(1 + z.len());
                plain.push(E2EE_FRAME_WIRE);
                plain.extend_from_slice(&z);
                if let Ok(ct) = chan_w.lock().await.seal_record(&plain)
                    && sink.send(TungMsg::Binary(ct.into())).await.is_err()
                {
                    break;
                }
            }
        }
    });

    tokio::spawn(async move {
        while let Some(Ok(m)) = stream.next().await {
            match m {
                TungMsg::Binary(ct) => {
                    if let Ok(p) = chan.lock().await.open_record(&ct)
                        && !p.is_empty()
                        && p[0] == E2EE_FRAME_WIRE
                        && let Ok(wire) = WireMessage::decode_zstd(&p[1..])
                        && in_tx.send(wire).await.is_err()
                    {
                        break;
                    }
                }
                TungMsg::Close(_) => break,
                _ => {}
            }
        }
    });

    Ok((host_info, out_tx, in_rx))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemotePeerUpgradeOutcome {
    Upgraded {
        from_version: String,
        target_version: String,
    },
    AlreadyUpToDate {
        version: String,
    },
    SkippedDisabled,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct RemotePeerUpgradeReport {
    pub label: String,
    pub host_id: Option<String>,
    pub outcome: RemotePeerUpgradeOutcome,
}

fn compare_semver_triplet(a: &str, b: &str) -> std::cmp::Ordering {
    fn parse_parts(v: &str) -> [u32; 3] {
        let clean = v
            .trim()
            .trim_start_matches(['v', 'V'])
            .split(['+', '-'])
            .next()
            .unwrap_or("0.0.0");
        let mut it = clean.split('.');
        [
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            it.next().and_then(|s| s.parse().ok()).unwrap_or(0),
        ]
    }
    parse_parts(a).cmp(&parse_parts(b))
}

/// Connect to every enabled daemon-to-daemon Connected Host in `store` concurrently and trigger
/// `WireMessage::UpgradeDaemon` on each reachable peer (direct WS, Noise NK Relay, or `ssh+relay://`).
pub async fn upgrade_all_saved_connected_hosts(
    store: &OrbitStore,
    self_host_id: &str,
    target_version: &str,
    force: bool,
) -> anyhow::Result<Vec<RemotePeerUpgradeReport>> {
    let saved = store.list_connected_hosts()?;
    let target = target_version
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();

    let mut handles = Vec::with_capacity(saved.len());
    for row in saved {
        let self_id = self_host_id.to_string();
        let target_ver = target.clone();
        handles.push(tokio::spawn(async move {
            let label = row.label.clone();
            let host_id = row.host_id.clone();
            if !row.enabled {
                return RemotePeerUpgradeReport {
                    label,
                    host_id,
                    outcome: RemotePeerUpgradeOutcome::SkippedDisabled,
                };
            }
            let outcome = upgrade_single_saved_peer(&row, &self_id, &target_ver, force).await;
            RemotePeerUpgradeReport {
                label,
                host_id,
                outcome,
            }
        }));
    }

    let mut reports = Vec::with_capacity(handles.len());
    for h in handles {
        match h.await {
            Ok(rep) => reports.push(rep),
            Err(e) => reports.push(RemotePeerUpgradeReport {
                label: "unknown".to_string(),
                host_id: None,
                outcome: RemotePeerUpgradeOutcome::Failed(format!("upgrade task aborted: {e}")),
            }),
        }
    }
    Ok(reports)
}

async fn upgrade_single_saved_peer(
    row: &StoredConnectedHost,
    self_host_id: &str,
    target_version: &str,
    force: bool,
) -> RemotePeerUpgradeOutcome {
    let (host_info, tx, mut rx) = match connect_to_peer_daemon(row, self_host_id).await {
        Ok(conn) => conn,
        Err(e) => {
            return RemotePeerUpgradeOutcome::Failed(format!("unreachable ({e})"));
        }
    };

    if let Some(ref pinned) = row.host_id
        && pinned != &host_info.host_id
    {
        return RemotePeerUpgradeOutcome::Failed(format!(
            "pinned host identity mismatch: expected `{pinned}`, got `{}`",
            host_info.host_id
        ));
    }

    let current_ver = host_info
        .daemon_version
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();

    if !force
        && !target_version.is_empty()
        && compare_semver_triplet(&current_ver, target_version) != std::cmp::Ordering::Less
    {
        return RemotePeerUpgradeOutcome::AlreadyUpToDate {
            version: current_ver,
        };
    }

    let req_id = Uuid::new_v4();
    let msg = WireMessage::UpgradeDaemon {
        request_id: req_id,
        target_version: if target_version.is_empty() {
            None
        } else {
            Some(target_version.to_string())
        },
    };
    if let Err(e) = tx.send(msg).await {
        return RemotePeerUpgradeOutcome::Failed(format!("failed to send UpgradeDaemon: {e}"));
    }

    let wait_res = tokio::time::timeout(Duration::from_secs(95), async {
        while let Some(incoming) = rx.recv().await {
            match incoming {
                WireMessage::Ack { request_id } if request_id == req_id => return Ok(()),
                WireMessage::RpcError {
                    request_id: Some(r),
                    message,
                    ..
                } if r == req_id => return Err(message),
                _ => {}
            }
        }
        // Connection drop right after daemon spawns hot-restart is expected & healthy.
        Ok(())
    })
    .await;

    match wait_res {
        Ok(Ok(())) => RemotePeerUpgradeOutcome::Upgraded {
            from_version: current_ver,
            target_version: target_version.to_string(),
        },
        Ok(Err(rpc_err)) => RemotePeerUpgradeOutcome::Failed(rpc_err),
        Err(_) => RemotePeerUpgradeOutcome::Failed(
            "timed out waiting for remote daemon self-upgrade".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_semver_triplet_orders_correctly() {
        assert_eq!(
            compare_semver_triplet("0.7.0", "0.7.1"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_semver_triplet("v0.7.1", "0.7.1"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_semver_triplet("0.8.0-rc.1", "0.7.9"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn extract_response_request_id_recognizes_agent_forked() {
        let req_id = Uuid::new_v4();
        let now = Utc::now();
        let msg = WireMessage::AgentForked {
            request_id: req_id,
            agent: orbit_protocol::AgentSummary {
                agent_id: "ag_fork".into(),
                workspace_id: "ws_1".into(),
                cwd: "/tmp/repo".into(),
                title: "Forked".into(),
                provider: orbit_protocol::ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                fast_mode: false,
                mode: orbit_protocol::SessionMode::Default,
                status: orbit_protocol::AgentStatus::Idle,
                parent_agent_id: None,
                parent_host_id: None,
                latest_seq: 2,
                timeline_epoch: 0,
                created_at: now,
                updated_at: now,
                archived_at: None,
                background: false,
                auto_archive: false,
                profile_id: None,
                labels: HashMap::new(),
                attention: orbit_protocol::AttentionState::None,
                last_activity_at: Some(now),
                queued_prompts: Vec::new(),
                read_seq: 2,
                is_pinned: false,
                pinned_order: 0,
                handle: None,
                cluster_suggestion: None,
                task_summary: None,
                forked_from_agent_id: Some("ag_src".into()),
                forked_at_seq: Some(2),
                acp_agent_id: None,
            },
            workspace: None,
        };
        assert_eq!(extract_response_request_id(&msg), Some(req_id));
    }

    #[test]
    fn extract_response_request_id_recognizes_workspace_file_responses() {
        let req_id = Uuid::new_v4();
        let read_resp = WireMessage::WorkspaceFileContentResponse {
            request_id: req_id,
            cwd: "/tmp/repo".into(),
            path: "a.rs".into(),
            resolved_path: "/tmp/repo/a.rs".into(),
            content_kind: "text".into(),
            mime_type: Some("text/plain".into()),
            content: "fn main() {}\n".into(),
            size_bytes: 13,
            truncated: false,
            etag: Some("abc".into()),
            mtime_unix_ms: Some(1),
            writable: true,
            line_ending: Some("lf".into()),
        };
        let write_resp = WireMessage::WorkspaceFileWritten {
            request_id: req_id,
            cwd: "/tmp/repo".into(),
            path: "a.rs".into(),
            etag: "def".into(),
            size_bytes: 14,
            mtime_unix_ms: Some(2),
            conflict: None,
        };
        let mutate_resp = WireMessage::WorkspaceEntryMutated {
            request_id: req_id,
            cwd: "/tmp/repo".into(),
            path: "a.rs".into(),
            new_path: Some("b.rs".into()),
            action: "renamed".into(),
        };
        assert_eq!(extract_response_request_id(&read_resp), Some(req_id));
        assert_eq!(extract_response_request_id(&write_resp), Some(req_id));
        assert_eq!(extract_response_request_id(&mutate_resp), Some(req_id));
    }

    #[test]
    fn extract_response_request_id_recognizes_external_mcp_responses() {
        let req_id = Uuid::new_v4();
        let snap = WireMessage::ExternalMcpServersSnapshot {
            request_id: req_id,
            servers: Vec::new(),
        };
        let probe = WireMessage::ExternalMcpProbeResponse {
            request_id: req_id,
            server_id: "orbit_user:github".into(),
            result: orbit_protocol::McpProbeResult {
                ok: true,
                server_name: Some("github".into()),
                server_version: Some("1.0.0".into()),
                protocol_version: Some("2025-03-26".into()),
                tools: Vec::new(),
                latency_ms: 12,
                error: None,
                probed_at_ms: 1_760_000_000_000,
            },
        };
        assert_eq!(extract_response_request_id(&snap), Some(req_id));
        assert_eq!(extract_response_request_id(&probe), Some(req_id));
    }

    #[test]
    fn extract_response_request_id_recognizes_conditional_timer_responses() {
        let req_id = Uuid::new_v4();
        let rec = orbit_protocol::ConditionalTimerRecord {
            timer_id: "tmr_1".into(),
            agent_id: "ag_1".into(),
            workspace_id: "ws_1".into(),
            prompt: "Ping".into(),
            duration_seconds: 60,
            condition: orbit_protocol::TimerCondition::AnyMessage,
            state: orbit_protocol::ConditionalTimerState::Armed,
            created_at_ms: 1,
            fire_at_ms: 61_000,
            resolved_at_ms: None,
            resolved_reason: None,
        };
        assert_eq!(
            extract_response_request_id(&WireMessage::ConditionalTimerCreated {
                request_id: req_id,
                timer: rec.clone(),
            }),
            Some(req_id)
        );
        assert_eq!(
            extract_response_request_id(&WireMessage::ConditionalTimersSnapshot {
                request_id: req_id,
                timers: vec![rec.clone()],
            }),
            Some(req_id)
        );
        assert_eq!(
            extract_response_request_id(&WireMessage::ConditionalTimerUpdated {
                request_id: req_id,
                timer: rec,
            }),
            Some(req_id)
        );
    }

    #[test]
    fn extract_response_request_id_recognizes_browser_responses() {
        let req_id = Uuid::new_v4();
        let sess = orbit_protocol::BrowserSessionSummary {
            session_id: "br_1".into(),
            agent_id: Some("ag_1".into()),
            url: "http://localhost:3000".into(),
            title: "App".into(),
            viewport_width: 1280,
            viewport_height: 800,
            headless: true,
            element_count: 1,
            console_errors: Vec::new(),
            recent_actions: Vec::new(),
            last_screenshot_base64: None,
            updated_at_ms: 1,
        };
        assert_eq!(
            extract_response_request_id(&WireMessage::BrowserSessionsSnapshot {
                request_id: req_id,
                sessions: vec![sess.clone()],
                policy: orbit_protocol::BrowserPolicyConfig::default(),
                chrome_binary: None,
            }),
            Some(req_id)
        );
        assert_eq!(
            extract_response_request_id(&WireMessage::BrowserSessionUpdated {
                request_id: req_id,
                session: sess.clone(),
            }),
            Some(req_id)
        );
        assert_eq!(
            extract_response_request_id(&WireMessage::BrowserActionResponse {
                request_id: req_id,
                session: sess,
                elements: Vec::new(),
                screenshot_base64: None,
                text_excerpt: None,
                captcha_detected: false,
            }),
            Some(req_id)
        );
    }

    #[test]
    fn extract_response_request_id_recognizes_workflow_responses() {
        let req_id = Uuid::new_v4();
        let run = orbit_protocol::WorkflowRunRecord {
            run_id: "wf_1".into(),
            name: "Batch".into(),
            kind: orbit_protocol::WorkflowKind::BatchCsv,
            workspace_cwd: "/tmp/repo".into(),
            owner_agent_id: None,
            status: orbit_protocol::WorkflowRunStatus::Running,
            max_concurrency: 2,
            verify_command: None,
            winner_step_id: None,
            steps: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
        };
        assert_eq!(
            extract_response_request_id(&WireMessage::WorkflowRunUpdated {
                request_id: req_id,
                run: run.clone(),
            }),
            Some(req_id)
        );
        assert_eq!(
            extract_response_request_id(&WireMessage::WorkflowRunsSnapshot {
                request_id: req_id,
                runs: vec![run],
            }),
            Some(req_id)
        );
    }
}
