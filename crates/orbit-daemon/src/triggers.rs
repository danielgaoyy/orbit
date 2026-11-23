//! Pure-Rust External Event Triggers Engine (`EventTriggerEngine`).
//!
//! Supports inbound webhooks for GitHub, Slack, Linear, and Generic Webhooks with:
//! - Fail-closed constant-time pure-Rust HMAC-SHA256 & bearer-token verification (`sha2` + `zeroize`, zero `unsafe`).
//! - Slack ±5-minute overflow-safe replay-window timestamp validation & authenticated `url_verification` challenge replies.
//! - Atomic concurrent-safe bounded delivery-ID idempotency deduplication (in-memory + SQLite).
//! - 512 KiB HTTP body cap (`DefaultBodyLimit::max(512 * 1024)`), pre-auth in-memory burst cap,
//!   and post-auth per-trigger sliding-window rate limiting (max 20 deliveries / minute).
//! - Workspace `cwd` authorization via `authorize_workspace_mutation_cwd` and existing-agent validation.
//! - DNS-pinned, SSRF-safe, `.no_proxy()`, `Policy::none()`, 2 MiB streaming-bounded GitHub REST
//!   polling fallback using `ETag` / `If-None-Match` conditional requests.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::{
    Json,
    body::Bytes,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use orbit_protocol::{
    ActiveTurnBehavior, AgentMessageKind, EventTriggerDelivery, EventTriggerRecord,
    GitHubPollConfig, MAX_GITHUB_POLL_INTERVAL_SECS, MIN_GITHUB_POLL_INTERVAL_SECS,
    TriggerDispatchTarget, TriggerSourceKind, WorktreeCreateMode,
};
use orbit_store::{
    DEFAULT_RECENT_DELIVERIES_LIMIT, OrbitStore, StoredEventTrigger, UpsertEventTriggerParams,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::DaemonRuntime;
use crate::manager::{AgentOrchestrator, CreateAgentSpec};
use crate::messaging::{AgentMessageDelivery, AgentPromptSource, QueuedAgentMessage};

/// Hard cap on inbound webhook HTTP request bodies (512 KiB).
pub const MAX_WEBHOOK_BODY_BYTES: usize = 512 * 1024;
/// Hard streaming cap on outbound GitHub REST API response bodies (2 MiB).
pub const MAX_GITHUB_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// Maximum authenticated inbound webhook deliveries accepted per trigger within `RATE_LIMIT_WINDOW`.
pub const RATE_LIMIT_MAX_REQUESTS: usize = 20;
/// Lightweight in-memory pre-auth burst ceiling per trigger within `RATE_LIMIT_WINDOW` (never writes SQLite).
pub const PREAUTH_BURST_MAX_REQUESTS: usize = 60;
/// Maximum rejected-signature audit writes persisted to SQLite per trigger within `RATE_LIMIT_WINDOW`.
const REJECTED_AUDIT_MAX_PER_WINDOW: usize = 5;
/// Sliding window duration for per-trigger inbound rate limiting.
pub const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
/// Maximum replay skew allowed for Slack `X-Slack-Request-Timestamp` (±5 minutes).
pub const SLACK_REPLAY_WINDOW_SECS: i64 = 300;
/// Maximum byte length of a raw `dedup_key` before hashing down to a fixed-width digest.
const MAX_DEDUP_KEY_BYTES: usize = 256;
/// Maximum length of a custom `trigger_id`.
pub const MAX_TRIGGER_ID_LEN: usize = 64;
/// Bounded in-memory deduplication cache size across triggers.
const MAX_DEDUP_CACHE_ENTRIES: usize = 2048;
/// Maximum character length of extracted `{{body}}` injected into prompts.
const MAX_EXTRACTED_BODY_CHARS: usize = 12_000;
/// Maximum character length of delivery summary persisted to SQLite.
const MAX_DELIVERY_SUMMARY_CHARS: usize = 240;
/// Maximum seen items persisted per GitHub polling endpoint.
const MAX_POLL_SEEN_ENTRIES: usize = 500;
/// Background poller wake tick interval.
const POLLER_TICK_INTERVAL: Duration = Duration::from_secs(5);
/// Maximum number of triggers allowed per host.
pub const MAX_EVENT_TRIGGERS_PER_HOST: usize = 128;

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    let trimmed = raw.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let prefix: String = trimmed.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{prefix}…")
}

/// Normalize a `dedup_key` to at most `MAX_DEDUP_KEY_BYTES` bytes (hashing overly long keys
/// so unbounded header values can never inflate RAM or SQLite).
fn normalize_dedup_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.len() <= MAX_DEDUP_KEY_BYTES {
        Some(trimmed.to_string())
    } else {
        let digest = Sha256::digest(trimmed.as_bytes());
        Some(format!("sha256:{}", hex_encode(&digest)))
    }
}

// ==================== PURE-RUST CONSTANT-TIME CRYPTO & SIGNATURES ====================

/// Constant-time byte-slice equality check (no early return on first differing byte).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff: u8 = if a.len() == b.len() { 0 } else { 1 };
    let max_len = a.len().max(b.len());
    for i in 0..max_len {
        let ba = a.get(i).copied().unwrap_or(0);
        let bb = b.get(i).copied().unwrap_or(0);
        diff |= std::hint::black_box(ba ^ bb);
    }
    std::hint::black_box(diff) == 0
}

/// Pure-Rust RFC 2104 HMAC-SHA256 using `sha2::Sha256` with `zeroize` stack scrubbing.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
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
    inner.update(message);
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_digest);
    let out = outer.finalize();

    // Scrub intermediate key material from stack using zeroize
    key_block.zeroize();
    ipad.zeroize();
    opad.zeroize();

    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    result
}

/// Lowercase hex encoding helper.
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Strict hex decoding helper (accepts upper/lower ASCII hex, requires even length).
pub fn hex_decode(raw: &str) -> Option<Vec<u8>> {
    let s = raw.trim();
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.as_chunks::<2>().0 {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Verify GitHub `X-Hub-Signature-256: sha256=<hex>` (fails closed on empty secret).
pub fn verify_github_signature(
    secret: &str,
    body: &[u8],
    sig_header: Option<&str>,
) -> Result<(), String> {
    if secret.is_empty() {
        return Err("webhook signing secret is not configured".to_string());
    }
    let raw = sig_header
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing X-Hub-Signature-256 header".to_string())?;
    let hex_part = raw
        .strip_prefix("sha256=")
        .ok_or_else(|| "X-Hub-Signature-256 must start with sha256=".to_string())?;
    let provided =
        hex_decode(hex_part).ok_or_else(|| "malformed X-Hub-Signature-256 hex".to_string())?;
    let expected = hmac_sha256(secret.as_bytes(), body);
    if constant_time_eq(&expected, &provided) {
        Ok(())
    } else {
        Err("GitHub HMAC-SHA256 signature mismatch".to_string())
    }
}

/// Verify Slack `X-Slack-Request-Timestamp` (±300s, overflow-safe) and
/// `X-Slack-Signature: v0=<hex>` (fails closed on empty secret).
pub fn verify_slack_signature(
    secret: &str,
    body: &[u8],
    timestamp_header: Option<&str>,
    sig_header: Option<&str>,
    now_secs: i64,
) -> Result<(), String> {
    if secret.is_empty() {
        return Err("webhook signing secret is not configured".to_string());
    }
    let ts_str = timestamp_header
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing X-Slack-Request-Timestamp header".to_string())?;
    let ts: i64 = ts_str
        .parse()
        .map_err(|_| "invalid X-Slack-Request-Timestamp".to_string())?;
    if ts <= 0 || now_secs.abs_diff(ts) > SLACK_REPLAY_WINDOW_SECS as u64 {
        return Err("Slack request timestamp outside 5-minute replay window".to_string());
    }

    let raw_sig = sig_header
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing X-Slack-Signature header".to_string())?;
    let hex_part = raw_sig
        .strip_prefix("v0=")
        .ok_or_else(|| "X-Slack-Signature must start with v0=".to_string())?;
    let provided =
        hex_decode(hex_part).ok_or_else(|| "malformed X-Slack-Signature hex".to_string())?;

    let prefix = format!("v0:{ts_str}:");
    let mut signing_input = Vec::with_capacity(prefix.len() + body.len());
    signing_input.extend_from_slice(prefix.as_bytes());
    signing_input.extend_from_slice(body);
    let expected = hmac_sha256(secret.as_bytes(), &signing_input);
    if constant_time_eq(&expected, &provided) {
        Ok(())
    } else {
        Err("Slack v0 HMAC-SHA256 signature mismatch".to_string())
    }
}

/// Verify Linear `Linear-Signature` (HMAC-SHA256 hex digest of raw request body; fails closed on empty secret).
pub fn verify_linear_signature(
    secret: &str,
    body: &[u8],
    sig_header: Option<&str>,
) -> Result<(), String> {
    if secret.is_empty() {
        return Err("webhook signing secret is not configured".to_string());
    }
    let raw = sig_header
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing Linear-Signature header".to_string())?;
    let hex_part = raw.strip_prefix("sha256=").unwrap_or(raw);
    let provided =
        hex_decode(hex_part).ok_or_else(|| "malformed Linear-Signature hex".to_string())?;
    let expected = hmac_sha256(secret.as_bytes(), body);
    if constant_time_eq(&expected, &provided) {
        Ok(())
    } else {
        Err("Linear HMAC-SHA256 signature mismatch".to_string())
    }
}

/// Verify Generic Webhook authentication via `X-Orbit-Signature-256: sha256=<hex>` OR
/// `Authorization: Bearer <secret>` (both constant-time; fails closed on empty secret).
pub fn verify_generic_webhook_auth(
    secret: &str,
    body: &[u8],
    sig_header: Option<&str>,
    auth_header: Option<&str>,
) -> Result<(), String> {
    if secret.is_empty() {
        return Err("webhook signing secret is not configured".to_string());
    }
    if let Some(raw_sig) = sig_header.map(str::trim).filter(|s| !s.is_empty()) {
        let hex_part = raw_sig
            .strip_prefix("sha256=")
            .ok_or_else(|| "X-Orbit-Signature-256 must start with sha256=".to_string())?;
        let provided = hex_decode(hex_part)
            .ok_or_else(|| "malformed X-Orbit-Signature-256 hex".to_string())?;
        let expected = hmac_sha256(secret.as_bytes(), body);
        if constant_time_eq(&expected, &provided) {
            return Ok(());
        }
        return Err("X-Orbit-Signature-256 HMAC mismatch".to_string());
    }

    if let Some(raw_auth) = auth_header.map(str::trim).filter(|s| !s.is_empty()) {
        let token = raw_auth
            .strip_prefix("Bearer ")
            .or_else(|| raw_auth.strip_prefix("bearer "))
            .map(str::trim)
            .ok_or_else(|| "Authorization header must use Bearer scheme".to_string())?;
        if constant_time_eq(secret.as_bytes(), token.as_bytes()) {
            return Ok(());
        }
        return Err("Authorization Bearer token mismatch".to_string());
    }

    Err("missing X-Orbit-Signature-256 or Authorization: Bearer header".to_string())
}

// ==================== EVENT EXTRACTION, FILTERING & TEMPLATE RENDERING ====================

/// Normalized fields extracted from an external event payload for prompt templating.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractedEventContext {
    pub source: String,
    pub event: String,
    pub title: String,
    pub body: String,
    pub url: String,
    pub actor: String,
    pub repo: String,
    pub dedup_key: Option<String>,
}

fn json_str<'a>(val: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut cur = val;
    for key in path {
        cur = cur.get(*key)?;
    }
    cur.as_str().map(str::trim).filter(|s| !s.is_empty())
}

/// Extract normalized template context from headers + JSON payload for a given trigger source.
pub fn extract_event_context(
    source: TriggerSourceKind,
    headers: &HeaderMap,
    payload: &Value,
    explicit_event_type: Option<&str>,
) -> ExtractedEventContext {
    let header_str = |name: &str| -> Option<String> {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    match source {
        TriggerSourceKind::GitHub => {
            let base_event = explicit_event_type
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| header_str("x-github-event"))
                .unwrap_or_else(|| "webhook".to_string());
            let action = json_str(payload, &["action"]);
            let event = match action {
                Some(act) if !base_event.contains('.') => format!("{base_event}.{act}"),
                _ => base_event,
            };
            let title = json_str(payload, &["pull_request", "title"])
                .or_else(|| json_str(payload, &["issue", "title"]))
                .or_else(|| json_str(payload, &["workflow_run", "name"]))
                .or_else(|| json_str(payload, &["check_run", "name"]))
                .or_else(|| json_str(payload, &["head_commit", "message"]))
                .or_else(|| json_str(payload, &["title"]))
                .unwrap_or("")
                .to_string();
            let body_raw = json_str(payload, &["comment", "body"])
                .or_else(|| json_str(payload, &["review", "body"]))
                .or_else(|| json_str(payload, &["pull_request", "body"]))
                .or_else(|| json_str(payload, &["issue", "body"]))
                .or_else(|| json_str(payload, &["workflow_run", "conclusion"]))
                .or_else(|| json_str(payload, &["body"]))
                .unwrap_or("");
            let url = json_str(payload, &["comment", "html_url"])
                .or_else(|| json_str(payload, &["review", "html_url"]))
                .or_else(|| json_str(payload, &["pull_request", "html_url"]))
                .or_else(|| json_str(payload, &["issue", "html_url"]))
                .or_else(|| json_str(payload, &["workflow_run", "html_url"]))
                .or_else(|| json_str(payload, &["check_run", "html_url"]))
                .or_else(|| json_str(payload, &["compare"]))
                .or_else(|| json_str(payload, &["repository", "html_url"]))
                .or_else(|| json_str(payload, &["url"]))
                .unwrap_or("")
                .to_string();
            let actor = json_str(payload, &["sender", "login"])
                .or_else(|| json_str(payload, &["pusher", "name"]))
                .or_else(|| json_str(payload, &["actor"]))
                .unwrap_or("")
                .to_string();
            let repo = json_str(payload, &["repository", "full_name"])
                .or_else(|| json_str(payload, &["repo"]))
                .unwrap_or("")
                .to_string();
            let dedup_key = header_str("x-github-delivery")
                .as_deref()
                .and_then(normalize_dedup_key);

            ExtractedEventContext {
                source: source.slug().to_string(),
                event,
                title: truncate_chars(&title, 240),
                body: truncate_chars(body_raw, MAX_EXTRACTED_BODY_CHARS),
                url,
                actor,
                repo,
                dedup_key,
            }
        }
        TriggerSourceKind::Slack => {
            let event = explicit_event_type
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| json_str(payload, &["event", "type"]).map(str::to_string))
                .or_else(|| json_str(payload, &["type"]).map(str::to_string))
                .unwrap_or_else(|| "slack_event".to_string());
            let body_raw = json_str(payload, &["event", "text"])
                .or_else(|| json_str(payload, &["text"]))
                .or_else(|| json_str(payload, &["body"]))
                .unwrap_or("");
            let title = json_str(payload, &["title"])
                .map(str::to_string)
                .unwrap_or_else(|| {
                    body_raw
                        .lines()
                        .next()
                        .map(|l| truncate_chars(l, 120))
                        .unwrap_or_default()
                });
            let url = json_str(payload, &["event", "permalink"])
                .or_else(|| json_str(payload, &["url"]))
                .unwrap_or("")
                .to_string();
            let actor = json_str(payload, &["event", "user"])
                .or_else(|| json_str(payload, &["user", "name"]))
                .or_else(|| json_str(payload, &["actor"]))
                .unwrap_or("")
                .to_string();
            let repo = json_str(payload, &["event", "channel"])
                .or_else(|| json_str(payload, &["channel"]))
                .or_else(|| json_str(payload, &["team_id"]))
                .unwrap_or("")
                .to_string();
            let dedup_key = json_str(payload, &["event_id"])
                .map(str::to_string)
                .or_else(|| header_str("x-slack-delivery"))
                .as_deref()
                .and_then(normalize_dedup_key);

            ExtractedEventContext {
                source: source.slug().to_string(),
                event,
                title: truncate_chars(&title, 240),
                body: truncate_chars(body_raw, MAX_EXTRACTED_BODY_CHARS),
                url,
                actor,
                repo,
                dedup_key,
            }
        }
        TriggerSourceKind::Linear => {
            let base = explicit_event_type
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| json_str(payload, &["type"]).map(str::to_string))
                .unwrap_or_else(|| "Issue".to_string());
            let action = json_str(payload, &["action"]);
            let event = match action {
                Some(act) if !base.contains('.') => format!("{base}.{act}"),
                _ => base,
            };
            let title = json_str(payload, &["data", "title"])
                .or_else(|| json_str(payload, &["title"]))
                .unwrap_or("")
                .to_string();
            let body_raw = json_str(payload, &["data", "description"])
                .or_else(|| json_str(payload, &["data", "body"]))
                .or_else(|| json_str(payload, &["body"]))
                .unwrap_or("");
            let url = json_str(payload, &["url"])
                .or_else(|| json_str(payload, &["data", "url"]))
                .unwrap_or("")
                .to_string();
            let actor = json_str(payload, &["actor", "name"])
                .or_else(|| json_str(payload, &["data", "creator", "name"]))
                .or_else(|| json_str(payload, &["actor"]))
                .unwrap_or("")
                .to_string();
            let repo = json_str(payload, &["data", "team", "key"])
                .or_else(|| json_str(payload, &["data", "team", "name"]))
                .or_else(|| json_str(payload, &["data", "identifier"]))
                .or_else(|| json_str(payload, &["repo"]))
                .unwrap_or("")
                .to_string();
            let dedup_key = header_str("linear-delivery")
                .or_else(|| {
                    let wid = json_str(payload, &["webhookId"])?;
                    let ts = payload
                        .get("webhookTimestamp")
                        .and_then(|v| v.as_i64())
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    Some(format!("{wid}:{ts}"))
                })
                .as_deref()
                .and_then(normalize_dedup_key);

            ExtractedEventContext {
                source: source.slug().to_string(),
                event,
                title: truncate_chars(&title, 240),
                body: truncate_chars(body_raw, MAX_EXTRACTED_BODY_CHARS),
                url,
                actor,
                repo,
                dedup_key,
            }
        }
        TriggerSourceKind::GenericWebhook => {
            let event = explicit_event_type
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .or_else(|| header_str("x-orbit-event"))
                .or_else(|| json_str(payload, &["event"]).map(str::to_string))
                .or_else(|| json_str(payload, &["type"]).map(str::to_string))
                .unwrap_or_else(|| "webhook".to_string());
            let title = json_str(payload, &["title"])
                .or_else(|| json_str(payload, &["subject"]))
                .unwrap_or("")
                .to_string();
            let body_raw = json_str(payload, &["body"])
                .or_else(|| json_str(payload, &["message"]))
                .or_else(|| json_str(payload, &["text"]))
                .unwrap_or("");
            let url = json_str(payload, &["url"])
                .or_else(|| json_str(payload, &["link"]))
                .unwrap_or("")
                .to_string();
            let actor = json_str(payload, &["actor"])
                .or_else(|| json_str(payload, &["sender"]))
                .or_else(|| json_str(payload, &["user"]))
                .unwrap_or("")
                .to_string();
            let repo = json_str(payload, &["repo"])
                .or_else(|| json_str(payload, &["project"]))
                .or_else(|| json_str(payload, &["repository"]))
                .unwrap_or("")
                .to_string();
            let dedup_key = header_str("x-orbit-delivery")
                .or_else(|| header_str("x-delivery-id"))
                .or_else(|| header_str("idempotency-key"))
                .as_deref()
                .and_then(normalize_dedup_key);

            ExtractedEventContext {
                source: source.slug().to_string(),
                event,
                title: truncate_chars(&title, 240),
                body: truncate_chars(body_raw, MAX_EXTRACTED_BODY_CHARS),
                url,
                actor,
                repo,
                dedup_key,
            }
        }
    }
}

/// Check if an incoming `event_type` matches the trigger's configured `event_filters`.
///
/// - Empty filters or `"*"` match everything.
/// - Case-insensitive exact match (`pull_request.opened`).
/// - Base event match (`pull_request` matches `pull_request.opened`).
/// - Wildcard prefix (`pull_request.*` matches `pull_request.opened`).
pub fn matches_event_filters(filters: &[String], event_type: &str) -> bool {
    let cleaned: Vec<String> = filters
        .iter()
        .map(|f| f.trim().to_ascii_lowercase())
        .filter(|f| !f.is_empty())
        .collect();
    if cleaned.is_empty() || cleaned.iter().any(|f| f == "*") {
        return true;
    }
    let ev = event_type.trim().to_ascii_lowercase();
    let ev_base = ev.split('.').next().unwrap_or("");
    cleaned.iter().any(|pattern| {
        if pattern == &ev {
            return true;
        }
        if let Some(prefix) = pattern.strip_suffix(".*") {
            return ev == prefix || ev.starts_with(&format!("{prefix}."));
        }
        !ev_base.is_empty() && pattern == ev_base
    })
}

/// Single-pass template renderer for `{{source}}`, `{{event}}`, `{{title}}`, `{{body}}`,
/// `{{url}}`, `{{actor}}`, and `{{repo}}` (immune to recursive placeholder expansion).
pub fn render_prompt_template(template: &str, ctx: &ExtractedEventContext) -> String {
    let mut out = String::with_capacity(template.len() + 128);
    let mut rest = template;
    while let Some(open_idx) = rest.find("{{") {
        out.push_str(&rest[..open_idx]);
        let after_open = &rest[open_idx + 2..];
        if let Some(close_idx) = after_open.find("}}") {
            let key = after_open[..close_idx].trim();
            match key {
                "source" => out.push_str(&ctx.source),
                "event" => out.push_str(&ctx.event),
                "title" => out.push_str(&ctx.title),
                "body" => out.push_str(&ctx.body),
                "url" => out.push_str(&ctx.url),
                "actor" => out.push_str(&ctx.actor),
                "repo" => out.push_str(&ctx.repo),
                _ => {
                    out.push_str("{{");
                    out.push_str(&after_open[..close_idx]);
                    out.push_str("}}");
                }
            }
            rest = &after_open[close_idx + 2..];
        } else {
            out.push_str(&rest[open_idx..]);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Strictly validate a GitHub `owner/repo` slug to prevent URL path traversal or query injection.
pub fn validate_github_repo_slug(repo: &str) -> Result<(&str, &str), String> {
    let trimmed = repo.trim();
    let mut parts = trimmed.split('/');
    let owner = parts.next().unwrap_or("");
    let name = parts.next().unwrap_or("");
    if parts.next().is_some() || owner.is_empty() || name.is_empty() {
        return Err("GitHub repository must be in `owner/repo` format".to_string());
    }
    let valid_seg = |s: &str| {
        s != "."
            && s != ".."
            && s.len() <= 100
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    if !valid_seg(owner) || !valid_seg(name) {
        return Err(format!("invalid GitHub repository slug `{trimmed}`"));
    }
    Ok((owner, name))
}

// ==================== DISPATCHER ABSTRACTION ====================

/// Result of dispatching a rendered trigger prompt to an existing or newly spawned agent.
#[derive(Debug, Clone)]
pub struct DispatchOutcome {
    pub agent_id: String,
    pub summary: String,
}

/// Pluggable dispatch sink so unit tests can assert Axum ingress end-to-end without spawning CLIs.
pub trait TriggerDispatcher: Send + Sync + 'static {
    fn dispatch(
        &self,
        trigger: EventTriggerRecord,
        event_type: String,
        rendered_prompt: String,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchOutcome, String>> + Send + '_>>;
}

/// Production dispatcher backed by `AgentOrchestrator` and `AgentMessageDelivery`.
pub struct OrchestratorTriggerDispatcher {
    store: Arc<OrbitStore>,
    orchestrator: Arc<AgentOrchestrator>,
    delivery: Arc<AgentMessageDelivery>,
}

impl OrchestratorTriggerDispatcher {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            orchestrator,
            delivery,
        })
    }
}

impl TriggerDispatcher for OrchestratorTriggerDispatcher {
    fn dispatch(
        &self,
        trigger: EventTriggerRecord,
        event_type: String,
        rendered_prompt: String,
    ) -> Pin<Box<dyn Future<Output = Result<DispatchOutcome, String>> + Send + '_>> {
        Box::pin(async move {
            match trigger.target {
                TriggerDispatchTarget::ExistingAgent { agent_id } => {
                    let ag = self
                        .store
                        .get_agent(&agent_id)
                        .map_err(|e| format!("failed to inspect target agent: {e}"))?
                        .ok_or_else(|| format!("target agent `{agent_id}` not found"))?;
                    if ag.archived_at.is_some() {
                        return Err(format!("target agent `{agent_id}` is archived"));
                    }
                    let body = format!(
                        "[External Trigger · {} · {}]\n\n{}",
                        trigger.name, event_type, rendered_prompt
                    );
                    self.delivery
                        .enqueue(QueuedAgentMessage {
                            recipient_id: agent_id.clone(),
                            message_id: format!("trg_{}", Uuid::new_v4().simple()),
                            source: AgentPromptSource {
                                kind: AgentMessageKind::DirectMessage,
                                source_agent_id: None,
                                source_host_id: None,
                                source_title: Some(format!(
                                    "{} Trigger: {}",
                                    trigger.source.display_name(),
                                    trigger.name
                                )),
                                event: Some(event_type.clone()),
                                permission_context: None,
                            },
                            body,
                            active_turn_behavior: ActiveTurnBehavior::Queue,
                            session_mode: None,
                            unarchive: false,
                        })
                        .await
                        .map_err(|e| format!("failed to enqueue trigger prompt: {e}"))?;
                    Ok(DispatchOutcome {
                        agent_id: agent_id.clone(),
                        summary: format!("Queued `{event_type}` on agent {agent_id}"),
                    })
                }
                TriggerDispatchTarget::SpawnWorkspaceAgent {
                    cwd,
                    provider,
                    model,
                    mode,
                } => {
                    let authorized_cwd = authorize_trigger_cwd(&self.store, &cwd).await?;
                    let spec = CreateAgentSpec {
                        cwd: authorized_cwd,
                        extra_dirs: Vec::new(),
                        title: Some(format!("{} · {}", trigger.name, event_type)),
                        provider,
                        model,
                        effort: None,
                        fast_mode: None,
                        mode,
                        initial_prompt: rendered_prompt,
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
                        auto_cluster: Some(false),
                        acp_agent_id: None,
                    };
                    let agent = self
                        .orchestrator
                        .create_and_start_agent(spec)
                        .await
                        .map_err(|e| format!("failed to spawn workspace agent: {e}"))?;
                    Ok(DispatchOutcome {
                        agent_id: agent.agent_id.clone(),
                        summary: format!("Spawned agent {} for `{event_type}`", agent.agent_id),
                    })
                }
            }
        })
    }
}

/// Authorize a `SpawnWorkspaceAgent` `cwd` against registered host projects/workspaces,
/// rejecting `..` traversal segments and arbitrary directories outside registered roots.
pub(crate) async fn authorize_trigger_cwd(
    store: &Arc<OrbitStore>,
    raw_cwd: &str,
) -> Result<String, String> {
    let trimmed = raw_cwd.trim();
    if trimmed.is_empty() || trimmed == "~" {
        return Err("SpawnWorkspaceAgent requires a registered project workspace path".to_string());
    }
    let expanded = crate::expand_host_path(trimmed);
    if expanded
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Workspace path must not contain `..` traversal segments".to_string());
    }
    let authorized = crate::authorize_workspace_mutation_cwd(store, &expanded).await?;
    Ok(authorized.to_string_lossy().to_string())
}

// ==================== BOUNDED DEDUP & SLIDING-WINDOW RATE LIMITER ====================

fn check_sliding_window(
    map: &mut HashMap<String, VecDeque<Instant>>,
    key: &str,
    now: Instant,
    max_requests: usize,
) -> bool {
    // Prune stale entries across keys periodically so deleted/idle triggers don't leak memory
    map.retain(|_, q| {
        while let Some(&front) = q.front() {
            if now.saturating_duration_since(front) >= RATE_LIMIT_WINDOW {
                q.pop_front();
            } else {
                break;
            }
        }
        !q.is_empty()
    });
    let queue = map.entry(key.to_string()).or_default();
    if queue.len() >= max_requests {
        return false;
    }
    queue.push_back(now);
    true
}

#[derive(Default)]
struct GuardState {
    /// Pre-auth burst cap per `trigger_id` (in-memory only, never writes SQLite).
    preauth_windows: HashMap<String, VecDeque<Instant>>,
    /// Post-auth legitimate delivery rate limit per `trigger_id` (20 req/60s).
    rate_windows: HashMap<String, VecDeque<Instant>>,
    /// Throttle for writing `rejected_signature` audit rows to SQLite (prevents DB spam).
    rejected_audit_windows: HashMap<String, VecDeque<Instant>>,
    /// Bounded FIFO + set of `(trigger_id, dedup_key)` pairs.
    dedup_order: VecDeque<(String, String)>,
    dedup_set: HashSet<(String, String)>,
}

impl GuardState {
    fn check_preauth_burst(&mut self, trigger_id: &str, now: Instant) -> bool {
        check_sliding_window(
            &mut self.preauth_windows,
            trigger_id,
            now,
            PREAUTH_BURST_MAX_REQUESTS,
        )
    }

    fn check_postauth_rate_limit(&mut self, trigger_id: &str, now: Instant) -> bool {
        check_sliding_window(
            &mut self.rate_windows,
            trigger_id,
            now,
            RATE_LIMIT_MAX_REQUESTS,
        )
    }

    fn allow_rejected_audit_write(&mut self, trigger_id: &str, now: Instant) -> bool {
        check_sliding_window(
            &mut self.rejected_audit_windows,
            trigger_id,
            now,
            REJECTED_AUDIT_MAX_PER_WINDOW,
        )
    }

    fn contains_dedup(&self, trigger_id: &str, key: &str) -> bool {
        self.dedup_set
            .contains(&(trigger_id.to_string(), key.to_string()))
    }

    /// Atomically claim `(trigger_id, key)` in-memory. Returns `true` if newly claimed,
    /// or `false` if already present (prevents concurrent duplicate-delivery TOCTOU races).
    fn try_claim_dedup(&mut self, trigger_id: &str, key: &str) -> bool {
        let pair = (trigger_id.to_string(), key.to_string());
        if self.dedup_set.insert(pair.clone()) {
            self.dedup_order.push_back(pair);
            while self.dedup_order.len() > MAX_DEDUP_CACHE_ENTRIES {
                if let Some(evicted) = self.dedup_order.pop_front() {
                    self.dedup_set.remove(&evicted);
                }
            }
            true
        } else {
            false
        }
    }

    fn remove_trigger(&mut self, trigger_id: &str) {
        self.preauth_windows.remove(trigger_id);
        self.rate_windows.remove(trigger_id);
        self.rejected_audit_windows.remove(trigger_id);
        self.dedup_order.retain(|(tid, _)| tid != trigger_id);
        self.dedup_set.retain(|(tid, _)| tid != trigger_id);
    }
}

// ==================== GITHUB POLLING STATE ====================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GitHubPollState {
    #[serde(default)]
    prs_initialized: bool,
    #[serde(default)]
    issues_initialized: bool,
    #[serde(default)]
    ci_initialized: bool,
    #[serde(default)]
    prs_etag: Option<String>,
    #[serde(default)]
    issues_etag: Option<String>,
    #[serde(default)]
    ci_etag: Option<String>,
    #[serde(default)]
    seen_prs: HashMap<String, String>,
    #[serde(default)]
    seen_issues: HashMap<String, String>,
    #[serde(default)]
    seen_ci_runs: HashMap<String, String>,
}

fn trim_seen_map(map: &mut HashMap<String, String>) {
    if map.len() <= MAX_POLL_SEEN_ENTRIES {
        return;
    }
    let mut entries: Vec<(String, String)> = map.drain().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, v) in entries.into_iter().take(MAX_POLL_SEEN_ENTRIES) {
        map.insert(k, v);
    }
}

// ==================== ENGINE CORE ====================

/// Snapshot returned for `ListEventTriggers` / `UpsertEventTrigger` / `DeleteEventTrigger`.
#[derive(Debug, Clone)]
pub struct EventTriggersSnapshotData {
    pub triggers: Vec<EventTriggerRecord>,
    pub recent_deliveries: Vec<EventTriggerDelivery>,
    pub base_webhook_url: String,
}

/// Request payload for `EventTriggerEngine::upsert_trigger`.
#[derive(Clone)]
pub struct UpsertTriggerRequest {
    pub trigger_id: Option<String>,
    pub name: String,
    pub source: TriggerSourceKind,
    pub event_filters: Vec<String>,
    pub prompt_template: String,
    pub target: TriggerDispatchTarget,
    pub secret: Option<String>,
    pub keep_existing_secret: bool,
    pub github_poll: Option<GitHubPollConfig>,
    pub github_token: Option<String>,
    pub keep_existing_token: bool,
    pub enabled: bool,
}

impl fmt::Debug for UpsertTriggerRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UpsertTriggerRequest")
            .field("trigger_id", &self.trigger_id)
            .field("name", &self.name)
            .field("source", &self.source)
            .field("event_filters", &self.event_filters)
            .field("prompt_template", &self.prompt_template)
            .field("target", &self.target)
            .field("secret", &self.secret.as_ref().map(|_| "[REDACTED]"))
            .field("keep_existing_secret", &self.keep_existing_secret)
            .field("github_poll", &self.github_poll)
            .field(
                "github_token",
                &self.github_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("keep_existing_token", &self.keep_existing_token)
            .field("enabled", &self.enabled)
            .finish()
    }
}

/// Outcome returned by `EventTriggerEngine::handle_webhook_request`.
#[derive(Debug)]
pub struct WebhookHttpResult {
    pub status: StatusCode,
    pub body: Value,
}

pub struct EventTriggerEngine {
    store: Arc<OrbitStore>,
    dispatcher: Arc<dyn TriggerDispatcher>,
    listen_port: Arc<tokio::sync::RwLock<u16>>,
    home_dir: std::path::PathBuf,
    guard: Mutex<GuardState>,
    poller_abort: Mutex<Option<AbortHandle>>,
    enforce_registered_cwd: bool,
}

impl EventTriggerEngine {
    pub fn new(
        store: Arc<OrbitStore>,
        orchestrator: Arc<AgentOrchestrator>,
        delivery: Arc<AgentMessageDelivery>,
        listen_port: Arc<tokio::sync::RwLock<u16>>,
        home_dir: &Path,
    ) -> Arc<Self> {
        let dispatcher = OrchestratorTriggerDispatcher::new(store.clone(), orchestrator, delivery);
        Self::with_dispatcher(store, dispatcher, listen_port, home_dir, true)
    }

    pub fn with_dispatcher(
        store: Arc<OrbitStore>,
        dispatcher: Arc<dyn TriggerDispatcher>,
        listen_port: Arc<tokio::sync::RwLock<u16>>,
        home_dir: &Path,
        enforce_registered_cwd: bool,
    ) -> Arc<Self> {
        Arc::new(Self {
            store,
            dispatcher,
            listen_port,
            home_dir: home_dir.to_path_buf(),
            guard: Mutex::new(GuardState::default()),
            poller_abort: Mutex::new(None),
            enforce_registered_cwd,
        })
    }

    /// Compute the local daemon base URL for inbound webhooks (`http://127.0.0.1:{port}`).
    pub async fn base_webhook_url(&self) -> String {
        let port = *self.listen_port.read().await;
        let effective = if port > 0 {
            port
        } else {
            crate::read_live_daemon_port(&self.home_dir)
        };
        format!("http://127.0.0.1:{effective}")
    }

    /// Fetch the sanitized trigger list, recent delivery log, and base webhook URL.
    pub async fn list_snapshot(&self) -> Result<EventTriggersSnapshotData, String> {
        let triggers = self
            .store
            .list_event_triggers()
            .map_err(|e| format!("failed to list event triggers: {e}"))?;
        let recent_deliveries = self
            .store
            .list_recent_event_trigger_deliveries(DEFAULT_RECENT_DELIVERIES_LIMIT)
            .map_err(|e| format!("failed to list trigger deliveries: {e}"))?;
        let base_webhook_url = self.base_webhook_url().await;
        Ok(EventTriggersSnapshotData {
            triggers,
            recent_deliveries,
            base_webhook_url,
        })
    }

    /// Validate, authorize, and persist an external event trigger.
    pub async fn upsert_trigger(
        &self,
        req: UpsertTriggerRequest,
    ) -> Result<EventTriggersSnapshotData, String> {
        let name = req.name.trim().to_string();
        if name.is_empty() {
            return Err("Trigger name is required".to_string());
        }
        if name.chars().count() > 120 {
            return Err("Trigger name must be 120 characters or fewer".to_string());
        }
        let prompt_template = req.prompt_template.trim().to_string();
        if prompt_template.is_empty() {
            return Err("Prompt template is required".to_string());
        }

        let target = match req.target {
            TriggerDispatchTarget::ExistingAgent { agent_id } => {
                let clean = agent_id.trim().to_string();
                if clean.is_empty() {
                    return Err("Target agent ID is required".to_string());
                }
                if self.enforce_registered_cwd {
                    let ag = self
                        .store
                        .get_agent(&clean)
                        .map_err(|e| format!("failed to inspect target agent `{clean}`: {e}"))?
                        .ok_or_else(|| format!("target agent `{clean}` not found"))?;
                    if ag.archived_at.is_some() {
                        return Err(format!("target agent `{clean}` is archived"));
                    }
                }
                TriggerDispatchTarget::ExistingAgent { agent_id: clean }
            }
            TriggerDispatchTarget::SpawnWorkspaceAgent {
                cwd,
                provider,
                model,
                mode,
            } => {
                let final_cwd = if self.enforce_registered_cwd {
                    authorize_trigger_cwd(&self.store, &cwd).await?
                } else {
                    let clean = cwd.trim().to_string();
                    if clean.is_empty() {
                        return Err("Workspace cwd is required".to_string());
                    }
                    clean
                };
                TriggerDispatchTarget::SpawnWorkspaceAgent {
                    cwd: final_cwd,
                    provider,
                    model: model
                        .map(|m| m.trim().to_string())
                        .filter(|m| !m.is_empty()),
                    mode,
                }
            }
        };

        let github_poll = if req.source == TriggerSourceKind::GitHub {
            if let Some(cfg) = req.github_poll {
                let clean_repo = cfg.repo.trim();
                if !clean_repo.is_empty() {
                    validate_github_repo_slug(clean_repo)?;
                    Some(cfg.clamped())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let trigger_id = match req
            .trigger_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(custom_id) => {
                if custom_id.len() > MAX_TRIGGER_ID_LEN {
                    return Err(format!(
                        "trigger_id must be {MAX_TRIGGER_ID_LEN} characters or fewer"
                    ));
                }
                if !custom_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                {
                    return Err(
                        "trigger_id must contain only ASCII alphanumeric, `_`, or `-`".into(),
                    );
                }
                let already_exists = self
                    .store
                    .get_event_trigger(custom_id)
                    .map_err(|e| format!("failed to inspect trigger `{custom_id}`: {e}"))?
                    .is_some();
                if !already_exists {
                    let current_count = self
                        .store
                        .list_event_triggers()
                        .map_err(|e| format!("failed to inspect existing triggers: {e}"))?
                        .len();
                    if current_count >= MAX_EVENT_TRIGGERS_PER_HOST {
                        return Err(format!(
                            "maximum of {MAX_EVENT_TRIGGERS_PER_HOST} event triggers per host reached"
                        ));
                    }
                }
                custom_id.to_string()
            }
            None => {
                let current_count = self
                    .store
                    .list_event_triggers()
                    .map_err(|e| format!("failed to inspect existing triggers: {e}"))?
                    .len();
                if current_count >= MAX_EVENT_TRIGGERS_PER_HOST {
                    return Err(format!(
                        "maximum of {MAX_EVENT_TRIGGERS_PER_HOST} event triggers per host reached"
                    ));
                }
                format!("trg_{}", &Uuid::new_v4().simple().to_string()[..12])
            }
        };

        let event_filters: Vec<String> = req
            .event_filters
            .into_iter()
            .map(|f| f.trim().to_string())
            .filter(|f| !f.is_empty())
            .take(32)
            .collect();

        let params = UpsertEventTriggerParams {
            webhook_path: format!("/webhooks/{trigger_id}"),
            trigger_id,
            name,
            source: req.source,
            event_filters,
            prompt_template,
            target,
            secret: req.secret,
            keep_existing_secret: req.keep_existing_secret,
            github_poll,
            github_token: req.github_token,
            keep_existing_token: req.keep_existing_token,
            enabled: req.enabled,
            now_ms: now_unix_ms(),
        };

        self.store
            .upsert_event_trigger(&params)
            .map_err(|e| format!("failed to save event trigger: {e}"))?;

        self.list_snapshot().await
    }

    /// Delete an event trigger by ID, evict in-memory rate/dedup state, and return the updated snapshot.
    pub async fn delete_trigger(
        &self,
        trigger_id: &str,
    ) -> Result<EventTriggersSnapshotData, String> {
        let id = trigger_id.trim();
        if id.is_empty() {
            return Err("trigger_id is required".to_string());
        }
        self.store
            .delete_event_trigger(id)
            .map_err(|e| format!("failed to delete event trigger: {e}"))?;
        {
            let mut guard = self.guard.lock().await;
            guard.remove_trigger(id);
        }
        self.list_snapshot().await
    }

    /// Simulate a trigger delivery with a sample event type and JSON payload (bypasses HMAC
    /// verification because it is invoked from an authenticated Orbit RPC client).
    pub async fn test_trigger(
        &self,
        trigger_id: &str,
        sample_event_type: &str,
        sample_payload: Value,
    ) -> Result<EventTriggerDelivery, String> {
        let stored = self
            .store
            .get_event_trigger(trigger_id.trim())
            .map_err(|e| format!("failed to load trigger: {e}"))?
            .ok_or_else(|| format!("event trigger `{trigger_id}` not found"))?;
        let headers = HeaderMap::new();
        let ctx = extract_event_context(
            stored.record.source,
            &headers,
            &sample_payload,
            Some(sample_event_type),
        );
        let delivery = self.evaluate_and_dispatch(&stored.record, ctx, false).await;
        Ok(delivery)
    }

    /// Atomically check & claim a deduplication key without holding `self.guard` across SQLite I/O.
    ///
    /// Returns `true` if the key is fresh and now claimed by this caller; `false` if duplicate.
    async fn claim_dedup_key(&self, trigger_id: &str, dedup_key: &str) -> bool {
        // Fast path: already in bounded in-memory set
        {
            let guard = self.guard.lock().await;
            if guard.contains_dedup(trigger_id, dedup_key) {
                return false;
            }
        }
        // Slow path: check persisted SQLite history without holding async mutex
        let seen_in_db = self
            .store
            .has_event_trigger_dedup_key(trigger_id, dedup_key)
            .unwrap_or(false);
        // Atomically re-check and claim under a single lock acquisition
        let mut guard = self.guard.lock().await;
        if seen_in_db {
            guard.try_claim_dedup(trigger_id, dedup_key);
            return false;
        }
        guard.try_claim_dedup(trigger_id, dedup_key)
    }

    /// Process an inbound `POST /webhooks/{trigger_id}` request.
    pub async fn handle_webhook_request(
        &self,
        trigger_id: &str,
        headers: &HeaderMap,
        body: &[u8],
    ) -> WebhookHttpResult {
        if body.len() > MAX_WEBHOOK_BODY_BYTES {
            return WebhookHttpResult {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                body: serde_json::json!({ "error": "webhook payload exceeds 512 KiB limit" }),
            };
        }

        let stored = match self.store.get_event_trigger(trigger_id) {
            Ok(Some(t)) => t,
            Ok(None) => {
                return WebhookHttpResult {
                    status: StatusCode::NOT_FOUND,
                    body: serde_json::json!({ "error": "trigger not found" }),
                };
            }
            Err(e) => {
                warn!(trigger_id = %trigger_id, error = %e, "Failed to load trigger");
                return WebhookHttpResult {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: serde_json::json!({ "error": "internal trigger store error" }),
                };
            }
        };

        if !stored.record.enabled {
            return WebhookHttpResult {
                status: StatusCode::FORBIDDEN,
                body: serde_json::json!({ "error": "trigger is disabled" }),
            };
        }

        let header_str =
            |name: &str| -> Option<&str> { headers.get(name).and_then(|v| v.to_str().ok()) };

        // 1. Lightweight in-memory pre-auth burst cap (never consumes legitimate trigger quota
        //    and NEVER writes to SQLite so unauthenticated floods cannot DoS DB or audit table).
        {
            let mut guard = self.guard.lock().await;
            if !guard.check_preauth_burst(&stored.record.trigger_id, Instant::now()) {
                return WebhookHttpResult {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    body: serde_json::json!({
                        "status": "rate_limited",
                        "error": "pre-authentication burst limit exceeded",
                    }),
                };
            }
        }

        // 2. Fail-closed signature verification BEFORE consuming legitimate quota!
        //    Triggers without a configured signing secret ALWAYS reject inbound HTTP webhooks.
        let Some(secret) = stored.secret.as_deref().filter(|s| !s.is_empty()) else {
            return WebhookHttpResult {
                status: StatusCode::UNAUTHORIZED,
                body: serde_json::json!({
                    "status": "rejected_signature",
                    "error": "webhook signing secret is not configured for this trigger",
                }),
            };
        };

        let verify_result = match stored.record.source {
            TriggerSourceKind::GitHub => {
                verify_github_signature(secret, body, header_str("x-hub-signature-256"))
            }
            TriggerSourceKind::Slack => verify_slack_signature(
                secret,
                body,
                header_str("x-slack-request-timestamp"),
                header_str("x-slack-signature"),
                now_unix_secs(),
            ),
            TriggerSourceKind::Linear => {
                verify_linear_signature(secret, body, header_str("linear-signature"))
            }
            TriggerSourceKind::GenericWebhook => verify_generic_webhook_auth(
                secret,
                body,
                header_str("x-orbit-signature-256"),
                header_str("authorization"),
            ),
        };

        if let Err(reason) = verify_result {
            let allow_audit = {
                let mut guard = self.guard.lock().await;
                guard.allow_rejected_audit_write(&stored.record.trigger_id, Instant::now())
            };
            let delivery = self.make_delivery(
                &stored.record,
                header_str("x-github-event")
                    .or_else(|| header_str("x-orbit-event"))
                    .unwrap_or("webhook"),
                "rejected_signature",
                reason.clone(),
                None,
            );
            if allow_audit {
                let _ = self
                    .store
                    .record_event_trigger_delivery(&delivery, None, false);
            }
            return WebhookHttpResult {
                status: StatusCode::UNAUTHORIZED,
                body: serde_json::json!({
                    "status": "rejected_signature",
                    "error": reason,
                    "delivery_id": delivery.delivery_id,
                }),
            };
        }

        // 3. Post-auth per-trigger sliding-window rate limit (20 authenticated req / 60s).
        //    Lock is dropped BEFORE writing the audit delivery to SQLite.
        let allowed = {
            let mut guard = self.guard.lock().await;
            guard.check_postauth_rate_limit(&stored.record.trigger_id, Instant::now())
        };
        if !allowed {
            let delivery = self.make_delivery(
                &stored.record,
                "rate_limit",
                "rate_limited",
                format!("Exceeded rate limit ({RATE_LIMIT_MAX_REQUESTS} deliveries / 60s)"),
                None,
            );
            let _ = self
                .store
                .record_event_trigger_delivery(&delivery, None, false);
            return WebhookHttpResult {
                status: StatusCode::TOO_MANY_REQUESTS,
                body: serde_json::json!({
                    "status": "rate_limited",
                    "delivery_id": delivery.delivery_id,
                }),
            };
        }

        // 4. Parse JSON payload
        let payload: Value = if body.is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            match serde_json::from_slice::<Value>(body) {
                Ok(v) => v,
                Err(e) => {
                    let delivery = self.make_delivery(
                        &stored.record,
                        "invalid_json",
                        "error",
                        format!("Invalid JSON payload: {e}"),
                        None,
                    );
                    let _ = self
                        .store
                        .record_event_trigger_delivery(&delivery, None, false);
                    return WebhookHttpResult {
                        status: StatusCode::BAD_REQUEST,
                        body: serde_json::json!({
                            "status": "error",
                            "error": "invalid JSON body",
                            "delivery_id": delivery.delivery_id,
                        }),
                    };
                }
            }
        };

        // 5. Slack automatic `url_verification` challenge reply (after signature verification!)
        if stored.record.source == TriggerSourceKind::Slack
            && json_str(&payload, &["type"]) == Some("url_verification")
            && let Some(challenge) = json_str(&payload, &["challenge"])
        {
            return WebhookHttpResult {
                status: StatusCode::OK,
                body: serde_json::json!({ "challenge": challenge }),
            };
        }

        // 6. Extract normalized context & atomically claim delivery ID for deduplication
        let ctx = extract_event_context(stored.record.source, headers, &payload, None);
        if let Some(ref dedup_key) = ctx.dedup_key
            && !self
                .claim_dedup_key(&stored.record.trigger_id, dedup_key)
                .await
        {
            let delivery = self.make_delivery(
                &stored.record,
                &ctx.event,
                "duplicate",
                format!("Duplicate delivery `{dedup_key}` ignored"),
                None,
            );
            let _ = self
                .store
                .record_event_trigger_delivery(&delivery, Some(dedup_key), false);
            return WebhookHttpResult {
                status: StatusCode::OK,
                body: serde_json::json!({
                    "status": "duplicate",
                    "delivery_id": delivery.delivery_id,
                }),
            };
        }

        // 7. Filter, render template, dispatch, and record delivery
        let delivery = self.evaluate_and_dispatch(&stored.record, ctx, true).await;
        let status_code = if delivery.status == "error" {
            StatusCode::INTERNAL_SERVER_ERROR
        } else {
            StatusCode::OK
        };
        WebhookHttpResult {
            status: status_code,
            body: serde_json::json!({
                "status": delivery.status,
                "delivery_id": delivery.delivery_id,
                "dispatched_agent_id": delivery.dispatched_agent_id,
                "summary": delivery.summary,
            }),
        }
    }

    async fn evaluate_and_dispatch(
        &self,
        trigger: &EventTriggerRecord,
        ctx: ExtractedEventContext,
        record_dedup: bool,
    ) -> EventTriggerDelivery {
        let dedup_ref = if record_dedup {
            ctx.dedup_key.as_deref()
        } else {
            None
        };

        if !matches_event_filters(&trigger.event_filters, &ctx.event) {
            let delivery = self.make_delivery(
                trigger,
                &ctx.event,
                "filtered",
                format!(
                    "Event `{}` did not match filters [{}]",
                    ctx.event,
                    trigger.event_filters.join(", ")
                ),
                None,
            );
            let _ = self
                .store
                .record_event_trigger_delivery(&delivery, dedup_ref, false);
            return delivery;
        }

        let rendered = render_prompt_template(&trigger.prompt_template, &ctx);
        match self
            .dispatcher
            .dispatch(trigger.clone(), ctx.event.clone(), rendered)
            .await
        {
            Ok(outcome) => {
                let delivery = self.make_delivery(
                    trigger,
                    &ctx.event,
                    "dispatched",
                    outcome.summary,
                    Some(outcome.agent_id),
                );
                let _ = self
                    .store
                    .record_event_trigger_delivery(&delivery, dedup_ref, true);
                delivery
            }
            Err(err) => {
                let delivery = self.make_delivery(trigger, &ctx.event, "error", err, None);
                let _ = self
                    .store
                    .record_event_trigger_delivery(&delivery, dedup_ref, false);
                delivery
            }
        }
    }

    fn make_delivery(
        &self,
        trigger: &EventTriggerRecord,
        event_type: &str,
        status: &str,
        summary: String,
        dispatched_agent_id: Option<String>,
    ) -> EventTriggerDelivery {
        EventTriggerDelivery {
            delivery_id: format!("del_{}", &Uuid::new_v4().simple().to_string()[..12]),
            trigger_id: trigger.trigger_id.clone(),
            source: trigger.source,
            event_type: truncate_chars(event_type, 80),
            status: status.to_string(),
            summary: truncate_chars(&summary, MAX_DELIVERY_SUMMARY_CHARS),
            dispatched_agent_id,
            received_at_ms: now_unix_ms(),
        }
    }

    // ==================== GITHUB PR / ISSUE / CI POLLING FALLBACK ====================

    /// Spawn the background GitHub REST polling loop for private/NAT-bound hosts.
    pub async fn spawn_background_poller(self: &Arc<Self>) {
        let mut slot = self.poller_abort.lock().await;
        if let Some(prev) = slot.take() {
            prev.abort();
        }
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut last_polled: HashMap<String, Instant> = HashMap::new();
            let mut ticker = tokio::time::interval(POLLER_TICK_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                this.poll_due_github_triggers(&mut last_polled).await;
            }
        });
        *slot = Some(handle.abort_handle());
    }

    /// Stop the background GitHub polling loop cleanly on daemon shutdown.
    pub async fn shutdown(&self) {
        if let Some(handle) = self.poller_abort.lock().await.take() {
            handle.abort();
        }
    }

    async fn poll_due_github_triggers(&self, last_polled: &mut HashMap<String, Instant>) {
        let Ok(triggers) = self.store.list_stored_event_triggers() else {
            return;
        };
        let now = Instant::now();
        let active_ids: HashSet<String> = triggers
            .iter()
            .map(|t| t.record.trigger_id.clone())
            .collect();
        last_polled.retain(|k, _| active_ids.contains(k));

        for stored in triggers {
            if !stored.record.enabled || stored.record.source != TriggerSourceKind::GitHub {
                continue;
            }
            let Some(ref poll_cfg) = stored.record.github_poll else {
                continue;
            };
            if !poll_cfg.watch_prs && !poll_cfg.watch_issues && !poll_cfg.watch_ci_failures {
                continue;
            }
            let interval_secs = poll_cfg
                .poll_interval_secs
                .clamp(MIN_GITHUB_POLL_INTERVAL_SECS, MAX_GITHUB_POLL_INTERVAL_SECS);
            let due = last_polled
                .get(&stored.record.trigger_id)
                .is_none_or(|prev| {
                    now.saturating_duration_since(*prev) >= Duration::from_secs(interval_secs)
                });
            if !due {
                continue;
            }
            last_polled.insert(stored.record.trigger_id.clone(), now);
            if let Err(err) = self.poll_single_github_trigger(&stored, poll_cfg).await {
                debug!(
                    trigger_id = %stored.record.trigger_id,
                    repo = %poll_cfg.repo,
                    error = %err,
                    "GitHub polling tick failed"
                );
            }
        }
    }

    async fn poll_single_github_trigger(
        &self,
        stored: &StoredEventTrigger,
        cfg: &GitHubPollConfig,
    ) -> Result<(), String> {
        let (owner, repo_name) = validate_github_repo_slug(&cfg.repo)?;
        let base_url = "https://api.github.com";
        let (_, host_norm, pinned_addrs) =
            crate::goals::validate_and_pin_http_health_url(base_url).await?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(15))
            .resolve_to_addrs(&host_norm, &pinned_addrs)
            .build()
            .map_err(|e| format!("failed to build SSRF-safe GitHub client: {e}"))?;

        let mut state: GitHubPollState = stored
            .github_poll_state_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        let mut state_changed = false;

        if cfg.watch_prs {
            let url = format!(
                "{base_url}/repos/{owner}/{repo_name}/pulls?state=open&sort=updated&direction=desc&per_page=15"
            );
            match fetch_github_json_conditional(
                &client,
                &url,
                stored.github_token.as_deref(),
                state.prs_etag.as_deref(),
            )
            .await
            {
                Ok(Some((items, new_etag))) => {
                    if new_etag.is_some() && new_etag != state.prs_etag {
                        state.prs_etag = new_etag;
                        state_changed = true;
                    }
                    let seed_only = !state.prs_initialized;
                    if let Some(arr) = items.as_array() {
                        for pr in arr {
                            let Some(num) = pr.get("number").and_then(|v| v.as_u64()) else {
                                continue;
                            };
                            let updated = json_str(pr, &["updated_at"]).unwrap_or("").to_string();
                            let key = num.to_string();
                            let prev = state.seen_prs.get(&key);
                            if prev.map(String::as_str) != Some(updated.as_str()) {
                                let is_new = prev.is_none();
                                state.seen_prs.insert(key.clone(), updated.clone());
                                state_changed = true;
                                if !seed_only {
                                    let event = if is_new {
                                        "pull_request.opened"
                                    } else {
                                        "pull_request.synchronize"
                                    };
                                    let ctx = ExtractedEventContext {
                                        source: "github".to_string(),
                                        event: event.to_string(),
                                        title: truncate_chars(
                                            json_str(pr, &["title"]).unwrap_or(""),
                                            240,
                                        ),
                                        body: truncate_chars(
                                            json_str(pr, &["body"]).unwrap_or(""),
                                            MAX_EXTRACTED_BODY_CHARS,
                                        ),
                                        url: json_str(pr, &["html_url"]).unwrap_or("").to_string(),
                                        actor: json_str(pr, &["user", "login"])
                                            .unwrap_or("")
                                            .to_string(),
                                        repo: cfg.repo.clone(),
                                        dedup_key: Some(format!("poll:pr:{num}:{updated}")),
                                    };
                                    self.dispatch_polled_event(&stored.record, ctx).await;
                                }
                            }
                        }
                        trim_seen_map(&mut state.seen_prs);
                    }
                    if seed_only {
                        state.prs_initialized = true;
                        state_changed = true;
                        info!(
                            trigger_id = %stored.record.trigger_id,
                            repo = %cfg.repo,
                            "Seeded initial GitHub PR polling cursor"
                        );
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    debug!(
                        trigger_id = %stored.record.trigger_id,
                        repo = %cfg.repo,
                        error = %err,
                        "GitHub PR poll endpoint error (continuing remaining endpoints)"
                    );
                }
            }
        }

        if cfg.watch_issues {
            let url = format!(
                "{base_url}/repos/{owner}/{repo_name}/issues?state=open&sort=updated&direction=desc&per_page=15"
            );
            match fetch_github_json_conditional(
                &client,
                &url,
                stored.github_token.as_deref(),
                state.issues_etag.as_deref(),
            )
            .await
            {
                Ok(Some((items, new_etag))) => {
                    if new_etag.is_some() && new_etag != state.issues_etag {
                        state.issues_etag = new_etag;
                        state_changed = true;
                    }
                    let seed_only = !state.issues_initialized;
                    if let Some(arr) = items.as_array() {
                        for issue in arr {
                            // GitHub's /issues endpoint includes PRs with a `pull_request` key
                            if issue.get("pull_request").is_some() {
                                continue;
                            }
                            let Some(num) = issue.get("number").and_then(|v| v.as_u64()) else {
                                continue;
                            };
                            let updated =
                                json_str(issue, &["updated_at"]).unwrap_or("").to_string();
                            let key = num.to_string();
                            let prev = state.seen_issues.get(&key);
                            if prev.map(String::as_str) != Some(updated.as_str()) {
                                let is_new = prev.is_none();
                                state.seen_issues.insert(key.clone(), updated.clone());
                                state_changed = true;
                                if !seed_only {
                                    let event = if is_new {
                                        "issues.opened"
                                    } else {
                                        "issues.edited"
                                    };
                                    let ctx = ExtractedEventContext {
                                        source: "github".to_string(),
                                        event: event.to_string(),
                                        title: truncate_chars(
                                            json_str(issue, &["title"]).unwrap_or(""),
                                            240,
                                        ),
                                        body: truncate_chars(
                                            json_str(issue, &["body"]).unwrap_or(""),
                                            MAX_EXTRACTED_BODY_CHARS,
                                        ),
                                        url: json_str(issue, &["html_url"])
                                            .unwrap_or("")
                                            .to_string(),
                                        actor: json_str(issue, &["user", "login"])
                                            .unwrap_or("")
                                            .to_string(),
                                        repo: cfg.repo.clone(),
                                        dedup_key: Some(format!("poll:issue:{num}:{updated}")),
                                    };
                                    self.dispatch_polled_event(&stored.record, ctx).await;
                                }
                            }
                        }
                        trim_seen_map(&mut state.seen_issues);
                    }
                    if seed_only {
                        state.issues_initialized = true;
                        state_changed = true;
                        info!(
                            trigger_id = %stored.record.trigger_id,
                            repo = %cfg.repo,
                            "Seeded initial GitHub issues polling cursor"
                        );
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    debug!(
                        trigger_id = %stored.record.trigger_id,
                        repo = %cfg.repo,
                        error = %err,
                        "GitHub issues poll endpoint error (continuing remaining endpoints)"
                    );
                }
            }
        }

        if cfg.watch_ci_failures {
            let url = format!(
                "{base_url}/repos/{owner}/{repo_name}/actions/runs?status=failure&per_page=10"
            );
            match fetch_github_json_conditional(
                &client,
                &url,
                stored.github_token.as_deref(),
                state.ci_etag.as_deref(),
            )
            .await
            {
                Ok(Some((payload, new_etag))) => {
                    if new_etag.is_some() && new_etag != state.ci_etag {
                        state.ci_etag = new_etag;
                        state_changed = true;
                    }
                    let seed_only = !state.ci_initialized;
                    if let Some(arr) = payload.get("workflow_runs").and_then(|v| v.as_array()) {
                        for run in arr {
                            let Some(run_id) = run.get("id").and_then(|v| v.as_u64()) else {
                                continue;
                            };
                            let updated = json_str(run, &["updated_at"]).unwrap_or("").to_string();
                            let key = run_id.to_string();
                            if state.seen_ci_runs.get(&key).map(String::as_str)
                                != Some(updated.as_str())
                            {
                                state.seen_ci_runs.insert(key.clone(), updated.clone());
                                state_changed = true;
                                if !seed_only {
                                    let title = json_str(run, &["display_title"])
                                        .or_else(|| json_str(run, &["name"]))
                                        .unwrap_or("Workflow failure");
                                    let branch = json_str(run, &["head_branch"]).unwrap_or("");
                                    let ctx = ExtractedEventContext {
                                        source: "github".to_string(),
                                        event: "workflow_run.failure".to_string(),
                                        title: truncate_chars(title, 240),
                                        body: format!(
                                            "Workflow run #{run_id} failed on branch `{branch}`"
                                        ),
                                        url: json_str(run, &["html_url"]).unwrap_or("").to_string(),
                                        actor: json_str(run, &["triggering_actor", "login"])
                                            .or_else(|| json_str(run, &["actor", "login"]))
                                            .unwrap_or("")
                                            .to_string(),
                                        repo: cfg.repo.clone(),
                                        dedup_key: Some(format!("poll:ci:{run_id}:{updated}")),
                                    };
                                    self.dispatch_polled_event(&stored.record, ctx).await;
                                }
                            }
                        }
                        trim_seen_map(&mut state.seen_ci_runs);
                    }
                    if seed_only {
                        state.ci_initialized = true;
                        state_changed = true;
                        info!(
                            trigger_id = %stored.record.trigger_id,
                            repo = %cfg.repo,
                            "Seeded initial GitHub CI polling cursor"
                        );
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    debug!(
                        trigger_id = %stored.record.trigger_id,
                        repo = %cfg.repo,
                        error = %err,
                        "GitHub CI poll endpoint error"
                    );
                }
            }
        }

        if state_changed && let Ok(serialized) = serde_json::to_string(&state) {
            let _ = self
                .store
                .update_event_trigger_poll_state(&stored.record.trigger_id, &serialized);
        }
        Ok(())
    }

    async fn dispatch_polled_event(
        &self,
        trigger: &EventTriggerRecord,
        ctx: ExtractedEventContext,
    ) {
        if let Some(ref key) = ctx.dedup_key
            && !self.claim_dedup_key(&trigger.trigger_id, key).await
        {
            return;
        }
        let _ = self.evaluate_and_dispatch(trigger, ctx, true).await;
    }
}

/// Stream a `reqwest::Response` body into a bounded buffer, aborting as soon as accumulated
/// bytes exceed `max_bytes` (prevents unbounded OOM from malicious/chunked responses).
pub(crate) async fn read_bounded_response_body(
    mut resp: reqwest::Response,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    if let Some(content_len) = resp.content_length()
        && content_len > max_bytes as u64
    {
        return Err(format!(
            "GitHub API response Content-Length ({content_len} bytes) exceeded {max_bytes}-byte safety cap"
        ));
    }
    let initial_cap = resp
        .content_length()
        .map(|len| (len as usize).min(max_bytes))
        .unwrap_or(8 * 1024);
    let mut buf = Vec::with_capacity(initial_cap);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("failed to read GitHub API response chunk: {e}"))?
    {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!(
                "GitHub API response exceeded {max_bytes}-byte safety cap"
            ));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

/// Perform a conditional `GET` against GitHub REST API with `If-None-Match` support and
/// streaming 2 MiB body cap. Returns `Ok(None)` on `304 Not Modified`.
async fn fetch_github_json_conditional(
    client: &reqwest::Client,
    url: &str,
    token: Option<&str>,
    etag: Option<&str>,
) -> Result<Option<(Value, Option<String>)>, String> {
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "Orbit-Event-Triggers/1.0")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(tok) = token.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {tok}"));
    }
    if let Some(tag) = etag.map(str::trim).filter(|s| !s.is_empty()) {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub API returned HTTP {}",
            resp.status().as_u16()
        ));
    }
    let new_etag = resp
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let body_bytes = read_bounded_response_body(resp, MAX_GITHUB_RESPONSE_BYTES).await?;
    let json: Value =
        serde_json::from_slice(&body_bytes).map_err(|e| format!("invalid GitHub API JSON: {e}"))?;
    Ok(Some((json, new_etag)))
}

// ==================== AXUM HTTP INGRESS HANDLER ====================

/// Axum handler mounted at `POST /webhooks/{trigger_id}` with a 512 KiB `DefaultBodyLimit`.
pub async fn webhook_ingress_handler(
    State(runtime): State<Arc<DaemonRuntime>>,
    AxumPath(trigger_id): AxumPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let outcome = runtime
        .triggers
        .handle_webhook_request(&trigger_id, &headers, &body)
        .await;
    (outcome.status, Json(outcome.body)).into_response()
}

// ==================== TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::DefaultBodyLimit,
        routing::{get, post},
    };
    use chrono::Utc;
    use orbit_protocol::{
        AgentStatus, AgentSummary, AttentionState, ProviderKind, SessionMode, WorkspaceSetupStatus,
        WorkspaceSummary,
    };
    use std::sync::Mutex as StdMutex;

    struct RecordingDispatcher {
        calls: StdMutex<Vec<(String, String, String)>>,
    }

    impl RecordingDispatcher {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                calls: StdMutex::new(Vec::new()),
            })
        }

        fn take_calls(&self) -> Vec<(String, String, String)> {
            self.calls.lock().expect("lock").drain(..).collect()
        }
    }

    impl TriggerDispatcher for RecordingDispatcher {
        fn dispatch(
            &self,
            trigger: EventTriggerRecord,
            event_type: String,
            rendered_prompt: String,
        ) -> Pin<Box<dyn Future<Output = Result<DispatchOutcome, String>> + Send + '_>> {
            Box::pin(async move {
                self.calls.lock().expect("lock").push((
                    trigger.trigger_id,
                    event_type.clone(),
                    rendered_prompt,
                ));
                Ok(DispatchOutcome {
                    agent_id: "ag_mock_1".to_string(),
                    summary: format!("Dispatched {event_type} to ag_mock_1"),
                })
            })
        }
    }

    fn make_test_engine() -> (Arc<EventTriggerEngine>, Arc<RecordingDispatcher>) {
        make_test_engine_with_enforcement(false)
    }

    fn make_test_engine_with_enforcement(
        enforce_registered_cwd: bool,
    ) -> (Arc<EventTriggerEngine>, Arc<RecordingDispatcher>) {
        let store = Arc::new(OrbitStore::open_in_memory().expect("open store"));
        let dispatcher = RecordingDispatcher::new();
        let port = Arc::new(tokio::sync::RwLock::new(6868));
        let engine = EventTriggerEngine::with_dispatcher(
            store,
            dispatcher.clone(),
            port,
            Path::new("/tmp"),
            enforce_registered_cwd,
        );
        (engine, dispatcher)
    }

    async fn create_trigger(
        engine: &EventTriggerEngine,
        id: &str,
        source: TriggerSourceKind,
        secret: Option<&str>,
        filters: Vec<&str>,
    ) {
        engine
            .upsert_trigger(UpsertTriggerRequest {
                trigger_id: Some(id.to_string()),
                name: format!("Test {id}"),
                source,
                event_filters: filters.into_iter().map(str::to_string).collect(),
                prompt_template: "Source={{source}} Event={{event}} Title={{title}} Body={{body}} URL={{url}} Actor={{actor}} Repo={{repo}}".to_string(),
                target: TriggerDispatchTarget::SpawnWorkspaceAgent {
                    cwd: "/tmp/repo".into(),
                    provider: ProviderKind::ClaudeCode,
                    model: None,
                    mode: SessionMode::AcceptEdits,
                },
                secret: secret.map(str::to_string),
                keep_existing_secret: false,
                github_poll: None,
                github_token: None,
                keep_existing_token: false,
                enabled: true,
            })
            .await
            .expect("create trigger");
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_vector_and_constant_time_eq() {
        // RFC 4231 Test Case 2: key = "Jefe", data = "what do ya want for nothing?"
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex_encode(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert!(constant_time_eq(b"secret123", b"secret123"));
        assert!(!constant_time_eq(b"secret123", b"secret124"));
        assert!(!constant_time_eq(b"secret123", b"secret12"));
    }

    #[test]
    fn signature_verifiers_cover_github_slack_linear_generic_and_overflow_timestamps() {
        let body = br#"{"action":"opened","title":"Hello"}"#;
        let secret = "whsec_test_key";

        // GitHub
        let gh_sig = format!(
            "sha256={}",
            hex_encode(&hmac_sha256(secret.as_bytes(), body))
        );
        assert!(verify_github_signature(secret, body, Some(&gh_sig)).is_ok());
        assert!(verify_github_signature("", body, Some(&gh_sig)).is_err());
        assert!(verify_github_signature(secret, body, Some("sha256=0000")).is_err());
        assert!(verify_github_signature(secret, body, None).is_err());

        // Slack (valid + replay rejection + overflow/negative/i64::MIN regression!)
        let now = 1_760_000_000i64;
        let ts_ok = (now - 60).to_string();
        let mut base = format!("v0:{ts_ok}:").into_bytes();
        base.extend_from_slice(body);
        let slack_sig = format!("v0={}", hex_encode(&hmac_sha256(secret.as_bytes(), &base)));
        assert!(verify_slack_signature(secret, body, Some(&ts_ok), Some(&slack_sig), now).is_ok());
        assert!(verify_slack_signature("", body, Some(&ts_ok), Some(&slack_sig), now).is_err());

        let ts_stale = (now - 301).to_string();
        let mut stale_base = format!("v0:{ts_stale}:").into_bytes();
        stale_base.extend_from_slice(body);
        let stale_sig = format!(
            "v0={}",
            hex_encode(&hmac_sha256(secret.as_bytes(), &stale_base))
        );
        assert!(
            verify_slack_signature(secret, body, Some(&ts_stale), Some(&stale_sig), now).is_err(),
            "Slack timestamps older than 5 minutes must be rejected"
        );

        // Regression test for P0 #2: i64::MIN, negative, zero, and i64::MAX must reject without panic/overflow
        for bogus_ts in [
            i64::MIN.to_string(),
            "-1".to_string(),
            "0".to_string(),
            i64::MAX.to_string(),
        ] {
            let mut b = format!("v0:{bogus_ts}:").into_bytes();
            b.extend_from_slice(body);
            let sig = format!("v0={}", hex_encode(&hmac_sha256(secret.as_bytes(), &b)));
            assert!(
                verify_slack_signature(secret, body, Some(&bogus_ts), Some(&sig), now).is_err(),
                "Slack timestamp `{bogus_ts}` must be rejected without overflow"
            );
        }

        // Linear
        let lin_sig = hex_encode(&hmac_sha256(secret.as_bytes(), body));
        assert!(verify_linear_signature(secret, body, Some(&lin_sig)).is_ok());
        assert!(verify_linear_signature("", body, Some(&lin_sig)).is_err());
        assert!(verify_linear_signature(secret, body, Some("deadbeef")).is_err());

        // Generic webhook: HMAC or Bearer
        assert!(verify_generic_webhook_auth(secret, body, Some(&gh_sig), None).is_ok());
        assert!(
            verify_generic_webhook_auth(secret, body, None, Some("Bearer whsec_test_key")).is_ok()
        );
        assert!(verify_generic_webhook_auth("", body, None, Some("Bearer ")).is_err());
        assert!(verify_generic_webhook_auth(secret, body, None, Some("Bearer wrong")).is_err());
        assert!(verify_generic_webhook_auth(secret, body, None, None).is_err());
    }

    #[test]
    fn template_rendering_is_single_pass_and_filters_match_patterns() {
        let ctx = ExtractedEventContext {
            source: "github".into(),
            event: "pull_request.opened".into(),
            title: "Fix {{body}} injection".into(),
            body: "Safe body".into(),
            url: "https://github.com/octocat/orbit/pull/42".into(),
            actor: "octocat".into(),
            repo: "octocat/orbit".into(),
            dedup_key: None,
        };
        let rendered = render_prompt_template(
            "[{{source}}:{{event}}] {{title}} by {{actor}} in {{repo}} ({{url}})\n{{body}}",
            &ctx,
        );
        assert_eq!(
            rendered,
            "[github:pull_request.opened] Fix {{body}} injection by octocat in octocat/orbit (https://github.com/octocat/orbit/pull/42)\nSafe body"
        );

        assert!(matches_event_filters(&[], "pull_request.opened"));
        assert!(matches_event_filters(&["*".into()], "anything"));
        assert!(matches_event_filters(
            &["pull_request".into()],
            "pull_request.opened"
        ));
        assert!(matches_event_filters(
            &["pull_request.*".into()],
            "pull_request.synchronize"
        ));
        assert!(!matches_event_filters(
            &["issues.opened".into()],
            "pull_request.opened"
        ));

        assert!(validate_github_repo_slug("octocat/orbit").is_ok());
        assert!(validate_github_repo_slug("../etc/passwd").is_err());
        assert!(validate_github_repo_slug("owner/repo?foo=bar").is_err());

        // Oversized dedup_key is hashed down cleanly to <= 256 bytes
        let huge_key = "x".repeat(1000);
        let norm = normalize_dedup_key(&huge_key).expect("normalized");
        assert!(norm.starts_with("sha256:"));
        assert!(norm.len() <= MAX_DEDUP_KEY_BYTES);
    }

    #[tokio::test]
    async fn unsigned_webhook_triggers_fail_closed_with_401() {
        let (engine, dispatcher) = make_test_engine();
        for (id, source) in [
            ("trg_no_sec_gh", TriggerSourceKind::GitHub),
            ("trg_no_sec_slack", TriggerSourceKind::Slack),
            ("trg_no_sec_linear", TriggerSourceKind::Linear),
            ("trg_no_sec_generic", TriggerSourceKind::GenericWebhook),
        ] {
            create_trigger(&engine, id, source, None, vec![]).await;
            let res = engine
                .handle_webhook_request(id, &HeaderMap::new(), br#"{"event":"ping"}"#)
                .await;
            assert_eq!(
                res.status,
                StatusCode::UNAUTHORIZED,
                "trigger {id} without secret must fail closed with 401"
            );
            assert_eq!(res.body["status"], "rejected_signature");
        }
        assert!(dispatcher.take_calls().is_empty());
    }

    #[tokio::test]
    async fn unauthenticated_flood_does_not_exhaust_legitimate_trigger_quota() {
        let (engine, dispatcher) = make_test_engine();
        create_trigger(
            &engine,
            "trg_quota",
            TriggerSourceKind::GenericWebhook,
            Some("real_secret"),
            vec![],
        )
        .await;

        // Send 25 invalid-signature requests (more than RATE_LIMIT_MAX_REQUESTS=20, under PREAUTH_BURST=60)
        let mut bad_headers = HeaderMap::new();
        bad_headers.insert("authorization", "Bearer wrong".parse().expect("hdr"));
        for _ in 0..25 {
            let r = engine
                .handle_webhook_request("trg_quota", &bad_headers, br#"{"event":"ping"}"#)
                .await;
            assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        }

        // Legitimate authenticated request MUST still succeed because bad requests never consumed post-auth quota
        let mut good_headers = HeaderMap::new();
        good_headers.insert("authorization", "Bearer real_secret".parse().expect("hdr"));
        let ok = engine
            .handle_webhook_request("trg_quota", &good_headers, br#"{"event":"deploy"}"#)
            .await;
        assert_eq!(ok.status, StatusCode::OK);
        assert_eq!(ok.body["status"], "dispatched");
        assert_eq!(dispatcher.take_calls().len(), 1);
    }

    #[tokio::test]
    async fn concurrent_duplicate_deliveries_dispatch_only_once() {
        let (engine, dispatcher) = make_test_engine();
        create_trigger(
            &engine,
            "trg_conc",
            TriggerSourceKind::GitHub,
            Some("conc_sec"),
            vec!["pull_request.opened"],
        )
        .await;

        let pr_body = br#"{"action":"opened","pull_request":{"title":"Concurrent PR","body":"Body","html_url":"https://github.com/o/r/pull/2"},"sender":{"login":"bob"},"repository":{"full_name":"o/r"}}"#;
        let gh_sig = format!("sha256={}", hex_encode(&hmac_sha256(b"conc_sec", pr_body)));
        let mut headers = HeaderMap::new();
        headers.insert("x-hub-signature-256", gh_sig.parse().expect("hdr"));
        headers.insert("x-github-event", "pull_request".parse().expect("hdr"));
        headers.insert("x-github-delivery", "race-deliv-1".parse().expect("hdr"));

        let (r1, r2, r3) = tokio::join!(
            engine.handle_webhook_request("trg_conc", &headers, pr_body),
            engine.handle_webhook_request("trg_conc", &headers, pr_body),
            engine.handle_webhook_request("trg_conc", &headers, pr_body),
        );
        let statuses = [
            r1.body["status"].as_str().unwrap_or(""),
            r2.body["status"].as_str().unwrap_or(""),
            r3.body["status"].as_str().unwrap_or(""),
        ];
        assert_eq!(
            statuses.iter().filter(|&&s| s == "dispatched").count(),
            1,
            "exactly one concurrent duplicate should dispatch; got {statuses:?}"
        );
        assert_eq!(
            statuses.iter().filter(|&&s| s == "duplicate").count(),
            2,
            "remaining two concurrent duplicates must be deduplicated; got {statuses:?}"
        );
        assert_eq!(dispatcher.take_calls().len(), 1);
    }

    #[tokio::test]
    async fn upsert_validates_existing_agent_and_custom_trigger_id_length() {
        let (engine, _) = make_test_engine_with_enforcement(true);

        // Missing agent -> rejected
        let missing_res = engine
            .upsert_trigger(UpsertTriggerRequest {
                trigger_id: None,
                name: "Missing Agent".into(),
                source: TriggerSourceKind::GenericWebhook,
                event_filters: vec![],
                prompt_template: "Hi".into(),
                target: TriggerDispatchTarget::ExistingAgent {
                    agent_id: "ag_does_not_exist".into(),
                },
                secret: Some("sec".into()),
                keep_existing_secret: false,
                github_poll: None,
                github_token: None,
                keep_existing_token: false,
                enabled: true,
            })
            .await;
        assert!(missing_res.is_err());

        // Seed workspace + active and archived agents
        let now = Utc::now();
        engine
            .store
            .upsert_workspace(&WorkspaceSummary {
                workspace_id: "ws_1".into(),
                title: "Repo".into(),
                cwd: "/tmp/repo".into(),
                git_branch: None,
                is_worktree: false,
                updated_at: now,
                source_cwd: None,
                base_ref: None,
                parent_workspace_id: None,
                archived_at: None,
                background: false,
                setup_status: WorkspaceSetupStatus::Idle,
                mission_goal: None,
                project_root: None,
                board_revision: 0,
                title_locked: false,
                auto_clustered: false,
                is_curated: false,
            })
            .expect("ws");

        let make_agent = |id: &str, archived: bool| AgentSummary {
            agent_id: id.into(),
            workspace_id: "ws_1".into(),
            cwd: "/tmp/repo".into(),
            title: id.into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            fast_mode: false,
            mode: SessionMode::AcceptEdits,
            status: AgentStatus::Idle,
            parent_agent_id: None,
            parent_host_id: None,
            latest_seq: 0,
            timeline_epoch: 0,
            created_at: now,
            updated_at: now,
            archived_at: if archived { Some(now) } else { None },
            background: false,
            auto_archive: false,
            profile_id: None,
            labels: HashMap::new(),
            attention: AttentionState::None,
            last_activity_at: None,
            queued_prompts: vec![],
            read_seq: 0,
            is_pinned: false,
            pinned_order: 0,
            handle: None,
            cluster_suggestion: None,
            task_summary: None,
            forked_from_agent_id: None,
            forked_at_seq: None,
            acp_agent_id: None,
        };
        engine
            .store
            .upsert_agent(&make_agent("ag_archived", true), None)
            .expect("archived agent");
        engine
            .store
            .upsert_agent(&make_agent("ag_active", false), None)
            .expect("active agent");

        // Archived agent -> rejected
        let archived_res = engine
            .upsert_trigger(UpsertTriggerRequest {
                trigger_id: None,
                name: "Archived Agent".into(),
                source: TriggerSourceKind::GenericWebhook,
                event_filters: vec![],
                prompt_template: "Hi".into(),
                target: TriggerDispatchTarget::ExistingAgent {
                    agent_id: "ag_archived".into(),
                },
                secret: Some("sec".into()),
                keep_existing_secret: false,
                github_poll: None,
                github_token: None,
                keep_existing_token: false,
                enabled: true,
            })
            .await;
        assert!(archived_res.is_err());

        // Overlong custom trigger_id (> 64 chars) -> rejected
        let long_id_res = engine
            .upsert_trigger(UpsertTriggerRequest {
                trigger_id: Some("a".repeat(MAX_TRIGGER_ID_LEN + 1)),
                name: "Too Long ID".into(),
                source: TriggerSourceKind::GenericWebhook,
                event_filters: vec![],
                prompt_template: "Hi".into(),
                target: TriggerDispatchTarget::ExistingAgent {
                    agent_id: "ag_active".into(),
                },
                secret: Some("sec".into()),
                keep_existing_secret: false,
                github_poll: None,
                github_token: None,
                keep_existing_token: false,
                enabled: true,
            })
            .await;
        assert!(long_id_res.is_err());

        // Active agent + valid custom trigger_id -> accepted
        let ok_res = engine
            .upsert_trigger(UpsertTriggerRequest {
                trigger_id: Some("trg_valid_custom".into()),
                name: "Active Agent".into(),
                source: TriggerSourceKind::GenericWebhook,
                event_filters: vec![],
                prompt_template: "Hi".into(),
                target: TriggerDispatchTarget::ExistingAgent {
                    agent_id: "ag_active".into(),
                },
                secret: Some("sec".into()),
                keep_existing_secret: false,
                github_poll: None,
                github_token: None,
                keep_existing_token: false,
                enabled: true,
            })
            .await;
        assert!(ok_res.is_ok());
    }

    #[tokio::test]
    async fn streaming_response_reader_enforces_hard_byte_cap() {
        async fn big_handler() -> Vec<u8> {
            vec![b'x'; 2048]
        }
        async fn small_handler() -> &'static str {
            r#"{"ok":true}"#
        }
        let app = Router::new()
            .route("/big", get(big_handler))
            .route("/small", get(small_handler));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let srv = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client");

        let resp_big = client
            .get(format!("http://{addr}/big"))
            .send()
            .await
            .expect("send big");
        assert!(
            read_bounded_response_body(resp_big, 1024).await.is_err(),
            "response exceeding cap must be rejected"
        );

        let resp_small = client
            .get(format!("http://{addr}/small"))
            .send()
            .await
            .expect("send small");
        let bytes = read_bounded_response_body(resp_small, 1024)
            .await
            .expect("small response passes");
        assert_eq!(bytes, br#"{"ok":true}"#);

        srv.abort();
    }

    #[tokio::test]
    async fn slack_url_verification_and_dedup_and_rate_limit() {
        let (engine, dispatcher) = make_test_engine();
        create_trigger(
            &engine,
            "trg_slack",
            TriggerSourceKind::Slack,
            Some("slk_sec"),
            vec![],
        )
        .await;

        let now = now_unix_secs();
        let challenge_body = br#"{"type":"url_verification","challenge":"challenge_xyz_123"}"#;
        let ts = now.to_string();
        let mut base = format!("v0:{ts}:").into_bytes();
        base.extend_from_slice(challenge_body);
        let sig = format!("v0={}", hex_encode(&hmac_sha256(b"slk_sec", &base)));

        let mut headers = HeaderMap::new();
        headers.insert("x-slack-request-timestamp", ts.parse().expect("hdr"));
        headers.insert("x-slack-signature", sig.parse().expect("hdr"));

        let res = engine
            .handle_webhook_request("trg_slack", &headers, challenge_body)
            .await;
        assert_eq!(res.status, StatusCode::OK);
        assert_eq!(res.body["challenge"], "challenge_xyz_123");
        assert!(dispatcher.take_calls().is_empty());

        // Deduplication test on GitHub trigger
        create_trigger(
            &engine,
            "trg_gh",
            TriggerSourceKind::GitHub,
            Some("gh_sec"),
            vec!["pull_request.opened"],
        )
        .await;
        let pr_body = br#"{"action":"opened","pull_request":{"title":"Add triggers","body":"Details","html_url":"https://github.com/o/r/pull/1"},"sender":{"login":"alice"},"repository":{"full_name":"o/r"}}"#;
        let gh_sig = format!("sha256={}", hex_encode(&hmac_sha256(b"gh_sec", pr_body)));
        let mut gh_headers = HeaderMap::new();
        gh_headers.insert("x-hub-signature-256", gh_sig.parse().expect("hdr"));
        gh_headers.insert("x-github-event", "pull_request".parse().expect("hdr"));
        gh_headers.insert("x-github-delivery", "deliv-100".parse().expect("hdr"));

        let first = engine
            .handle_webhook_request("trg_gh", &gh_headers, pr_body)
            .await;
        assert_eq!(first.status, StatusCode::OK);
        assert_eq!(first.body["status"], "dispatched");
        assert_eq!(dispatcher.take_calls().len(), 1);

        let second = engine
            .handle_webhook_request("trg_gh", &gh_headers, pr_body)
            .await;
        assert_eq!(second.status, StatusCode::OK);
        assert_eq!(second.body["status"], "duplicate");
        assert!(dispatcher.take_calls().is_empty());

        // Rate limit test on generic trigger (20 allowed, 21st returns 429)
        create_trigger(
            &engine,
            "trg_rl",
            TriggerSourceKind::GenericWebhook,
            Some("bearer_tok"),
            vec![],
        )
        .await;
        let mut rl_headers = HeaderMap::new();
        rl_headers.insert("authorization", "Bearer bearer_tok".parse().expect("hdr"));
        for i in 0..RATE_LIMIT_MAX_REQUESTS {
            let body = format!(r#"{{"event":"ping","title":"msg {i}"}}"#);
            let r = engine
                .handle_webhook_request("trg_rl", &rl_headers, body.as_bytes())
                .await;
            assert_eq!(r.status, StatusCode::OK, "request {i} should succeed");
        }
        let over = engine
            .handle_webhook_request("trg_rl", &rl_headers, br#"{"event":"ping"}"#)
            .await;
        assert_eq!(over.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(over.body["status"], "rate_limited");

        // Deleting trigger evicts in-memory guard state
        engine.delete_trigger("trg_rl").await.expect("delete");
        {
            let guard = engine.guard.lock().await;
            assert!(!guard.rate_windows.contains_key("trg_rl"));
            assert!(!guard.preauth_windows.contains_key("trg_rl"));
        }
    }

    #[tokio::test]
    async fn end_to_end_axum_post_webhooks_route() {
        let (engine, dispatcher) = make_test_engine();
        create_trigger(
            &engine,
            "trg_e2e",
            TriggerSourceKind::GitHub,
            Some("e2e_secret"),
            vec!["pull_request.opened"],
        )
        .await;

        async fn test_handler(
            State(eng): State<Arc<EventTriggerEngine>>,
            AxumPath(trigger_id): AxumPath<String>,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response {
            let outcome = eng
                .handle_webhook_request(&trigger_id, &headers, &body)
                .await;
            (outcome.status, Json(outcome.body)).into_response()
        }

        let app = Router::new()
            .route(
                "/webhooks/{trigger_id}",
                post(test_handler).layer(DefaultBodyLimit::max(MAX_WEBHOOK_BODY_BYTES)),
            )
            .with_state(Arc::clone(&engine));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("local_addr");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("client");
        let url = format!("http://{addr}/webhooks/trg_e2e");

        // 1. Bad signature -> 401
        let bad = client
            .post(&url)
            .header("X-Hub-Signature-256", "sha256=deadbeef")
            .header("X-GitHub-Event", "pull_request")
            .body(r#"{"action":"opened"}"#)
            .send()
            .await
            .expect("send bad");
        assert_eq!(bad.status(), reqwest::StatusCode::UNAUTHORIZED);

        // 2. Valid signature -> 200 dispatched
        let payload = br#"{"action":"opened","pull_request":{"title":"E2E PR","body":"Looks great","html_url":"https://github.com/org/repo/pull/9"},"sender":{"login":"jess"},"repository":{"full_name":"org/repo"}}"#;
        let sig = format!(
            "sha256={}",
            hex_encode(&hmac_sha256(b"e2e_secret", payload))
        );
        let good = client
            .post(&url)
            .header("X-Hub-Signature-256", sig)
            .header("X-GitHub-Event", "pull_request")
            .header("X-GitHub-Delivery", "e2e-deliv-1")
            .body(payload.to_vec())
            .send()
            .await
            .expect("send good");
        assert_eq!(good.status(), reqwest::StatusCode::OK);
        let json: Value = good.json().await.expect("parse json");
        assert_eq!(json["status"], "dispatched");
        assert_eq!(json["dispatched_agent_id"], "ag_mock_1");

        let calls = dispatcher.take_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "trg_e2e");
        assert_eq!(calls[0].1, "pull_request.opened");
        assert!(calls[0].2.contains("Title=E2E PR"));
        assert!(calls[0].2.contains("Actor=jess"));

        // 3. Oversized payload (> 512 KiB) -> 413 Payload Too Large
        let big = vec![b'a'; MAX_WEBHOOK_BODY_BYTES + 1024];
        let oversized = client
            .post(&url)
            .body(big)
            .send()
            .await
            .expect("send oversized");
        assert_eq!(oversized.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        // 4. Snapshot masks secret strictly
        let snap = engine.list_snapshot().await.expect("snapshot");
        assert_eq!(snap.triggers.len(), 1);
        assert_eq!(snap.triggers[0].masked_secret.as_deref(), Some("••••"));
        assert_eq!(snap.triggers[0].trigger_count, 1);
        assert!(snap.recent_deliveries.len() >= 2);

        server.abort();
    }
}
