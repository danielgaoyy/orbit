//! Zero-Public-Webhook Real-Time IM Long-Connections & Discord Ed25519 Verification.
//!
//! Pure-Rust NAT-friendly outbound WebSocket runners so Orbit Daemon works behind home/corporate
//! NATs without exposing public webhook URLs:
//! 1. **Feishu / Lark WebSocket Long-Connection**: `POST /callback/ws/endpoint` + compact manual
//!    `pbbp2.Frame` protobuf binary codec + ping/pong keepalive + chunk reassembly + immediate
//!    <3s ACK + event-id dedup + bounded jittered backoff with fresh URL refresh.
//! 2. **Slack Socket Mode (WSS)**: `POST /api/apps.connections.open` (`xapp-...`) + `hello` /
//!    `disconnect` / `events_api` / `interactive` envelope dispatch + immediate `envelope_id` ACK.
//! 3. **Discord Gateway v10 (WSS) + Real Ed25519 Webhook Verification**: `GET /api/v10/gateway/bot`
//!    + `HELLO` (op 10) + jittered `HEARTBEAT` (op 1) / `HEARTBEAT_ACK` (op 11) watchdog
//!    + `IDENTIFY` (op 2) / `RESUME` (op 6) + `MESSAGE_CREATE` & `INTERACTION_CREATE` routing
//!    + fail-closed strict Ed25519 (`ed25519-dalek`) verification for Discord HTTP webhooks.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use orbit_protocol::{ImConnectionStatus, ImPlatform, ImTransportMode};
use orbit_store::StoredImChannel;
use rand::RngCore;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{Message as WsMessage, protocol::WebSocketConfig};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tracing::{debug, info, warn};

use crate::im_channels::{
    ImChannelBridge, InboundImMessage, build_pinned_im_client, format_reqwest_err,
    is_blocked_im_ip, lark_api_base_and_app_id, now_unix_ms, parse_lark_card_action,
    parse_lark_message_event, parse_slack_event_callback, parse_slack_interactive_action,
    read_bounded_response_json, truncate_for_status,
};

// ==================== CONSTANTS ====================

pub const MAX_WS_FRAME_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
pub const SIGNATURE_REPLAY_WINDOW_SECS: u64 = 300; // ±5 minutes

const MAX_FEISHU_CHUNKS_PER_MSG: usize = 32;
const MAX_FEISHU_INFLIGHT_CHUNKS: usize = 64;
const FEISHU_CHUNK_TTL_MS: u64 = 15_000;
const MAX_FEISHU_REASSEMBLED_BYTES: usize = 1024 * 1024; // 1 MiB

const FEISHU_ALLOWED_WS_SUFFIXES: &[&str] = &["feishu.cn", "larksuite.com", "larkoffice.com"];
const SLACK_ALLOWED_WS_SUFFIXES: &[&str] = &["slack.com", "slack-msgs.com"];
const DISCORD_ALLOWED_WS_SUFFIXES: &[&str] = &["discord.gg", "discord.com"];

const BACKOFF_INITIAL_SECS: u64 = 2;
const BACKOFF_MAX_SECS: u64 = 60;
const WS_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Feishu `pbbp2.Frame.method` values.
pub const FRAME_METHOD_CONTROL: i32 = 0;
pub const FRAME_METHOD_DATA: i32 = 1;

/// Discord Gateway v10 Intents: `GUILD_MESSAGES | DIRECT_MESSAGES | MESSAGE_CONTENT`.
pub const DISCORD_INTENT_GUILD_MESSAGES: u64 = 1 << 9;
pub const DISCORD_INTENT_DIRECT_MESSAGES: u64 = 1 << 12;
pub const DISCORD_INTENT_MESSAGE_CONTENT: u64 = 1 << 15;
pub const DISCORD_DEFAULT_INTENTS: u64 =
    DISCORD_INTENT_GUILD_MESSAGES | DISCORD_INTENT_DIRECT_MESSAGES | DISCORD_INTENT_MESSAGE_CONTENT;

// ==================== SHARED SECURITY, BACKOFF & WSS HELPERS ====================

/// Uniform random f64 in `[0.0, 1.0)` from OS CSPRNG.
fn rand_unit_f64() -> f64 {
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    let mantissa = u64::from_le_bytes(bytes) >> 11;
    (mantissa as f64) / ((1u64 << 53) as f64)
}

/// Compute bounded exponential backoff duration with ±25% jitter (`jitter_unit` in `[0.0, 1.0]`).
pub fn jittered_backoff(base_secs: u64, cap_secs: u64, jitter_unit: f64) -> Duration {
    let clamped_base = base_secs.clamp(1, cap_secs.max(1));
    let unit = jitter_unit.clamp(0.0, 1.0);
    let factor = 0.75 + 0.50 * unit;
    let millis = ((clamped_base as f64) * 1000.0 * factor).round();
    Duration::from_millis((millis.max(250.0)) as u64)
}

fn next_backoff_secs(current: u64, cap: u64) -> u64 {
    current.saturating_mul(2).clamp(BACKOFF_INITIAL_SECS, cap)
}

/// Redact all provided secrets AND strip query strings from any `wss://` or `https://` URLs
/// (which may carry Feishu `access_key` or Slack `ticket` tokens) before logging or persisting.
pub fn redact_secrets(text: &str, secrets: &[&str]) -> String {
    let mut out = scrub_url_query_params(text);
    for secret in secrets {
        let trimmed = secret.trim();
        if !trimmed.is_empty() {
            out = out.replace(trimmed, "[REDACTED]");
        }
    }
    out
}

/// Replace `?query...` on any `wss://`, `ws://`, `https://`, or `http://` URL inside arbitrary text.
fn scrub_url_query_params(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(scheme_pos) = find_url_scheme(rest) {
        out.push_str(&rest[..scheme_pos]);
        let url_part = &rest[scheme_pos..];
        let url_end = url_part
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ']' | '>'))
            .unwrap_or(url_part.len());
        let candidate = &url_part[..url_end];
        if let Some(q_idx) = candidate.find('?') {
            out.push_str(&candidate[..q_idx]);
            out.push_str("?[REDACTED]");
        } else {
            out.push_str(candidate);
        }
        rest = &url_part[url_end..];
    }
    out.push_str(rest);
    out
}

fn find_url_scheme(s: &str) -> Option<usize> {
    ["wss://", "ws://", "https://", "http://"]
        .into_iter()
        .filter_map(|p| s.find(p))
        .min()
}

/// Return safe `"wss://host"` origin for logs without path or query secrets.
pub fn safe_url_origin(raw: &str) -> String {
    match reqwest::Url::parse(raw) {
        Ok(parsed) => {
            let host = parsed.host_str().unwrap_or("unknown");
            format!("{}://{host}", parsed.scheme())
        }
        Err(_) => "wss://<invalid>".to_string(),
    }
}

/// Validate a platform WSS URL against scheme, credentials, trailing dot, and domain suffix allowlist.
pub fn validate_wss_url(
    raw_url: &str,
    allowed_suffixes: &[&str],
) -> Result<(reqwest::Url, String, u16), String> {
    let parsed = reqwest::Url::parse(raw_url).map_err(|_| "Invalid WebSocket URL".to_string())?;
    if parsed.scheme() != "wss" {
        return Err("IM WebSocket URL must use wss://".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("IM WebSocket URL must not embed credentials".to_string());
    }
    let host_raw = parsed
        .host_str()
        .ok_or_else(|| "IM WebSocket URL missing host".to_string())?;
    if host_raw.ends_with('.') || host_raw.is_empty() {
        return Err("IM WebSocket URL host is invalid".to_string());
    }
    let host_norm = host_raw.to_ascii_lowercase();
    let allowed = allowed_suffixes.iter().any(|suffix| {
        let s = suffix.to_ascii_lowercase();
        host_norm == s
            || host_norm
                .strip_suffix(&s)
                .is_some_and(|prefix| prefix.ends_with('.'))
    });
    if !allowed {
        return Err(format!(
            "Refusing non-allowlisted IM WebSocket host `{host_norm}`"
        ));
    }
    let port = parsed.port_or_known_default().unwrap_or(443);
    Ok((parsed, host_norm, port))
}

/// DNS-pinned, SSRF-checked, size-bounded TLS WebSocket dialer.
async fn connect_pinned_wss(
    raw_url: &str,
    allowed_suffixes: &[&str],
    secrets_to_redact: &[&str],
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, String> {
    let (_parsed, host_norm, port) = validate_wss_url(raw_url, allowed_suffixes)?;
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
                "Refusing IM WebSocket target `{host_norm}` resolved to non-public IP `{}`",
                addr.ip()
            ));
        }
    }

    let tcp = tokio::time::timeout(WS_CONNECT_TIMEOUT, TcpStream::connect(&addrs[..]))
        .await
        .map_err(|_| format!("Timed out connecting TCP to `{host_norm}`"))?
        .map_err(|e| format!("TCP connect failed for `{host_norm}`: {e}"))?;
    let _ = tcp.set_nodelay(true);

    let ws_config = WebSocketConfig::default()
        .max_message_size(Some(MAX_WS_FRAME_BYTES))
        .max_frame_size(Some(MAX_WS_FRAME_BYTES));

    let (ws_stream, _resp) = tokio::time::timeout(
        WS_CONNECT_TIMEOUT,
        tokio_tungstenite::client_async_tls_with_config(raw_url, tcp, Some(ws_config), None),
    )
    .await
    .map_err(|_| format!("Timed out completing TLS WebSocket handshake with `{host_norm}`"))?
    .map_err(|e| {
        let cleaned = redact_secrets(&e.to_string(), secrets_to_redact);
        format!("WebSocket handshake failed for `{host_norm}`: {cleaned}")
    })?;

    Ok(ws_stream)
}

fn update_status(
    bridge: &ImChannelBridge,
    channel_id: &str,
    status: ImConnectionStatus,
    message: &str,
) {
    let truncated = truncate_for_status(message);
    let _ = bridge.store().update_im_channel_status(
        channel_id,
        status,
        Some(&truncated),
        now_unix_ms(),
    );
}

/// Spawn `work` into a detached task and wait up to `deadline` for its result over a `oneshot`.
/// If `deadline` expires, returns `fallback_msg` so the WS ACK meets its 3-second SLA WITHOUT
/// dropping or cancelling `work` mid-flight.
pub async fn await_detached_with_deadline<F>(
    deadline: Duration,
    fallback_msg: &str,
    work: F,
) -> String
where
    F: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let res = work.await;
        let _ = tx.send(res);
    });
    match tokio::time::timeout(deadline, rx).await {
        Ok(Ok(Ok(msg))) => msg,
        Ok(Ok(Err(err))) => err,
        Ok(Err(_)) => "Approval handler terminated unexpectedly.".to_string(),
        Err(_) => fallback_msg.to_string(),
    }
}

// ==================== 1. FEISHU / LARK pbbp2.Frame PROTOBUF CODEC ====================

/// Protobuf `pbbp2.Header` (`key = 1`, `value = 2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pbbp2Header {
    pub key: String,
    pub value: String,
}

impl Pbbp2Header {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    fn encode(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.key.len() + self.value.len() + 8);
        encode_bytes_field(&mut buf, 1, self.key.as_bytes());
        encode_bytes_field(&mut buf, 2, self.value.as_bytes());
        buf
    }

    fn decode(mut bytes: &[u8]) -> Result<Self, String> {
        let mut key = String::new();
        let mut value = String::new();
        while !bytes.is_empty() {
            let (field_num, wire_type) = decode_tag(&mut bytes)?;
            match (field_num, wire_type) {
                (1, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    key = std::str::from_utf8(slice)
                        .map_err(|_| "pbbp2.Header.key is not valid UTF-8".to_string())?
                        .to_string();
                }
                (2, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    value = std::str::from_utf8(slice)
                        .map_err(|_| "pbbp2.Header.value is not valid UTF-8".to_string())?
                        .to_string();
                }
                (_, wt) => skip_wire_value(&mut bytes, wt)?,
            }
        }
        Ok(Self { key, value })
    }
}

/// Official Feishu / Lark WebSocket binary envelope (`pbbp2.Frame`).
///
/// Field numbers match `larksuite/oapi-sdk-go/ws/pbbp2.proto`:
/// - `1`: `SeqID` (`uint64`)
/// - `2`: `LogID` (`uint64`)
/// - `3`: `service` (`int32`)
/// - `4`: `method` (`int32`: `0 = control`, `1 = data`)
/// - `5`: `headers` (`repeated Header`)
/// - `6`: `payload_encoding` (`string`)
/// - `7`: `payload_type` (`string`)
/// - `8`: `payload` (`bytes`)
/// - `9`: `LogIDNew` (`string`)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pbbp2Frame {
    pub seq_id: u64,
    pub log_id: u64,
    pub service: i32,
    pub method: i32,
    pub headers: Vec<Pbbp2Header>,
    pub payload_encoding: Option<String>,
    pub payload_type: Option<String>,
    pub payload: Option<Vec<u8>>,
    pub log_id_new: Option<String>,
}

impl Pbbp2Frame {
    /// Look up the first header value matching `key`.
    pub fn header(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.key == key)
            .map(|h| h.value.as_str())
    }

    /// Build a Feishu `ping` control frame for the given `service` ID.
    pub fn ping(service: i32) -> Self {
        Self {
            seq_id: 0,
            log_id: 0,
            service,
            method: FRAME_METHOD_CONTROL,
            headers: vec![Pbbp2Header::new("type", "ping")],
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: None,
        }
    }

    /// Build a `pong` control frame echoing an incoming server `ping`.
    pub fn pong_from(req: &Self) -> Self {
        Self {
            seq_id: req.seq_id,
            log_id: req.log_id,
            service: req.service,
            method: FRAME_METHOD_CONTROL,
            headers: vec![Pbbp2Header::new("type", "pong")],
            payload_encoding: None,
            payload_type: None,
            payload: None,
            log_id_new: req.log_id_new.clone(),
        }
    }

    /// Build an immediate ACK Data frame (`{"code": 200}` or with base64 `data` for card toasts).
    pub fn ack_from(
        req: &Self,
        status_code: u16,
        biz_rt_ms: u64,
        response_data_json: Option<&[u8]>,
    ) -> Result<Self, String> {
        let mut headers: Vec<Pbbp2Header> = req
            .headers
            .iter()
            .filter(|h| h.key != "biz_rt")
            .cloned()
            .collect();
        headers.push(Pbbp2Header::new("biz_rt", biz_rt_ms.to_string()));

        let body_val = match response_data_json {
            Some(data_bytes) => {
                let b64 = base64::engine::general_purpose::STANDARD.encode(data_bytes);
                json!({
                    "code": status_code,
                    "data": b64,
                })
            }
            None => json!({ "code": status_code }),
        };
        let payload = serde_json::to_vec(&body_val)
            .map_err(|e| format!("Failed to serialize Feishu ACK JSON: {e}"))?;

        Ok(Self {
            seq_id: req.seq_id,
            log_id: req.log_id,
            service: req.service,
            method: FRAME_METHOD_DATA,
            headers,
            payload_encoding: req.payload_encoding.clone(),
            payload_type: req.payload_type.clone(),
            payload: Some(payload),
            log_id_new: req.log_id_new.clone(),
        })
    }

    /// Encode this frame into standard protobuf binary wire format.
    /// Required proto2 fields (`seq_id`, `log_id`, `service`, `method`) are always written.
    pub fn encode(&self) -> Vec<u8> {
        let payload_len = self.payload.as_ref().map_or(0, Vec::len);
        let mut buf = Vec::with_capacity(64 + payload_len);
        encode_varint_field(&mut buf, 1, self.seq_id);
        encode_varint_field(&mut buf, 2, self.log_id);
        encode_varint_field(&mut buf, 3, (self.service as i64) as u64);
        encode_varint_field(&mut buf, 4, (self.method as i64) as u64);
        for hdr in &self.headers {
            let encoded = hdr.encode();
            encode_bytes_field(&mut buf, 5, &encoded);
        }
        if let Some(ref enc) = self.payload_encoding {
            encode_bytes_field(&mut buf, 6, enc.as_bytes());
        }
        if let Some(ref ptype) = self.payload_type {
            encode_bytes_field(&mut buf, 7, ptype.as_bytes());
        }
        if let Some(ref data) = self.payload {
            encode_bytes_field(&mut buf, 8, data);
        }
        if let Some(ref log_new) = self.log_id_new {
            encode_bytes_field(&mut buf, 9, log_new.as_bytes());
        }
        buf
    }

    /// Decode a protobuf binary buffer into `Pbbp2Frame`, safely skipping unknown fields.
    pub fn decode(mut bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_WS_FRAME_BYTES {
            return Err("pbbp2.Frame exceeds maximum frame size".to_string());
        }
        let mut frame = Self::default();
        while !bytes.is_empty() {
            let (field_num, wire_type) = decode_tag(&mut bytes)?;
            match (field_num, wire_type) {
                (1, 0) => frame.seq_id = decode_varint(&mut bytes)?,
                (2, 0) => frame.log_id = decode_varint(&mut bytes)?,
                (3, 0) => frame.service = decode_varint(&mut bytes)? as i32,
                (4, 0) => frame.method = decode_varint(&mut bytes)? as i32,
                (5, 2) => {
                    if frame.headers.len() >= 128 {
                        return Err("pbbp2.Frame exceeded maximum header count".to_string());
                    }
                    let slice = decode_length_delimited(&mut bytes)?;
                    frame.headers.push(Pbbp2Header::decode(slice)?);
                }
                (6, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    frame.payload_encoding = Some(
                        std::str::from_utf8(slice)
                            .map_err(|_| "payload_encoding is not valid UTF-8".to_string())?
                            .to_string(),
                    );
                }
                (7, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    frame.payload_type = Some(
                        std::str::from_utf8(slice)
                            .map_err(|_| "payload_type is not valid UTF-8".to_string())?
                            .to_string(),
                    );
                }
                (8, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    frame.payload = Some(slice.to_vec());
                }
                (9, 2) => {
                    let slice = decode_length_delimited(&mut bytes)?;
                    frame.log_id_new = Some(
                        std::str::from_utf8(slice)
                            .map_err(|_| "LogIDNew is not valid UTF-8".to_string())?
                            .to_string(),
                    );
                }
                (_, wt) => skip_wire_value(&mut bytes, wt)?,
            }
        }
        Ok(frame)
    }
}

fn encode_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(byte);
            break;
        }
        buf.push(byte | 0x80);
    }
}

fn encode_varint_field(buf: &mut Vec<u8>, field_num: u32, v: u64) {
    encode_varint(buf, u64::from(field_num << 3));
    encode_varint(buf, v);
}

fn encode_bytes_field(buf: &mut Vec<u8>, field_num: u32, data: &[u8]) {
    encode_varint(buf, u64::from((field_num << 3) | 2));
    encode_varint(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

fn decode_varint(input: &mut &[u8]) -> Result<u64, String> {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for _ in 0..10 {
        let (&byte, rest) = input
            .split_first()
            .ok_or_else(|| "Unexpected EOF decoding protobuf varint".to_string())?;
        *input = rest;
        if shift == 63 && byte > 1 {
            return Err("Protobuf varint overflow".to_string());
        }
        result |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(result);
        }
        shift += 7;
    }
    Err("Protobuf varint exceeded 10 bytes".to_string())
}

fn decode_tag(input: &mut &[u8]) -> Result<(u32, u8), String> {
    let tag = decode_varint(input)?;
    let field_num =
        u32::try_from(tag >> 3).map_err(|_| "Protobuf field number out of range".to_string())?;
    if field_num == 0 {
        return Err("Invalid protobuf field number 0".to_string());
    }
    let wire_type = (tag & 0x07) as u8;
    Ok((field_num, wire_type))
}

fn decode_length_delimited<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], String> {
    let len_u64 = decode_varint(input)?;
    let len = usize::try_from(len_u64)
        .map_err(|_| "Protobuf length-delimited field overflow".to_string())?;
    if len > MAX_WS_FRAME_BYTES || len > input.len() {
        return Err("Protobuf length-delimited field truncated or oversized".to_string());
    }
    let (slice, rest) = input.split_at(len);
    *input = rest;
    Ok(slice)
}

fn skip_wire_value(input: &mut &[u8], wire_type: u8) -> Result<(), String> {
    match wire_type {
        0 => {
            let _ = decode_varint(input)?;
            Ok(())
        }
        1 => {
            if input.len() < 8 {
                return Err("Truncated 64-bit protobuf field".to_string());
            }
            *input = &input[8..];
            Ok(())
        }
        2 => {
            let _ = decode_length_delimited(input)?;
            Ok(())
        }
        5 => {
            if input.len() < 4 {
                return Err("Truncated 32-bit protobuf field".to_string());
            }
            *input = &input[4..];
            Ok(())
        }
        other => Err(format!("Unsupported protobuf wire type {other}")),
    }
}

/// Feishu WS `ClientConfig` returned by `/callback/ws/endpoint` and `pong` control frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeishuClientConfig {
    pub ping_interval_secs: u64,
    pub reconnect_interval_secs: u64,
    pub reconnect_nonce_secs: u64,
}

impl Default for FeishuClientConfig {
    fn default() -> Self {
        Self {
            ping_interval_secs: 120,
            reconnect_interval_secs: 120,
            reconnect_nonce_secs: 30,
        }
    }
}

impl FeishuClientConfig {
    pub fn from_json(val: &Value) -> Self {
        let mut cfg = Self::default();
        if let Some(ping) = val.get("PingInterval").and_then(Value::as_u64) {
            cfg.ping_interval_secs = ping.clamp(15, 300);
        }
        if let Some(reconn) = val.get("ReconnectInterval").and_then(Value::as_u64) {
            cfg.reconnect_interval_secs = reconn.clamp(2, 300);
        }
        if let Some(nonce) = val.get("ReconnectNonce").and_then(Value::as_u64) {
            cfg.reconnect_nonce_secs = nonce.clamp(0, 60);
        }
        cfg
    }
}

/// Bounded multi-chunk payload reassembler for Feishu `pbbp2.Frame` (`sum > 1`, `seq` in `0..sum`).
#[derive(Debug, Default)]
pub struct FeishuChunkAssembler {
    entries: HashMap<String, ChunkEntry>,
}

#[derive(Debug)]
struct ChunkEntry {
    sum: usize,
    parts: Vec<Option<Vec<u8>>>,
    received: usize,
    total_bytes: usize,
    created_at_ms: u64,
}

impl FeishuChunkAssembler {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Push a chunk; returns `Ok(Some(full_payload))` once all `sum` chunks are present.
    pub fn push(
        &mut self,
        message_id: &str,
        seq: usize,
        sum: usize,
        payload: Vec<u8>,
        now_ms: u64,
    ) -> Result<Option<Vec<u8>>, String> {
        if sum <= 1 {
            return Ok(Some(payload));
        }
        if sum > MAX_FEISHU_CHUNKS_PER_MSG || seq >= sum {
            return Err(format!("Invalid Feishu chunk metadata seq={seq} sum={sum}"));
        }
        // Evict stale incomplete entries
        self.entries
            .retain(|_, e| now_ms.saturating_sub(e.created_at_ms) <= FEISHU_CHUNK_TTL_MS);

        if !self.entries.contains_key(message_id)
            && self.entries.len() >= MAX_FEISHU_INFLIGHT_CHUNKS
            && let Some(oldest_key) = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.created_at_ms)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest_key);
        }

        let entry = self
            .entries
            .entry(message_id.to_string())
            .or_insert_with(|| ChunkEntry {
                sum,
                parts: vec![None; sum],
                received: 0,
                total_bytes: 0,
                created_at_ms: now_ms,
            });
        if entry.sum != sum {
            self.entries.remove(message_id);
            return Err("Mismatched Feishu chunk sum".to_string());
        }
        if entry.parts[seq].is_none() {
            let next_total = entry.total_bytes.saturating_add(payload.len());
            if next_total > MAX_FEISHU_REASSEMBLED_BYTES {
                self.entries.remove(message_id);
                return Err("Feishu reassembled payload exceeded 1 MiB limit".to_string());
            }
            entry.total_bytes = next_total;
            entry.parts[seq] = Some(payload);
            entry.received += 1;
        }
        if entry.received < entry.sum {
            return Ok(None);
        }
        let Some(completed) = self.entries.remove(message_id) else {
            return Ok(None);
        };
        let mut combined = Vec::with_capacity(completed.total_bytes);
        for part in completed.parts.into_iter().flatten() {
            combined.extend_from_slice(&part);
        }
        Ok(Some(combined))
    }
}

/// Extract `service_id=<i32>` from a Feishu signed WSS URL.
pub fn parse_feishu_service_id(ws_url: &str) -> i32 {
    reqwest::Url::parse(ws_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "service_id")
                .and_then(|(_, v)| v.parse::<i32>().ok())
        })
        .unwrap_or(0)
}

async fn fetch_lark_ws_endpoint(
    channel: &StoredImChannel,
) -> Result<(String, i32, FeishuClientConfig), String> {
    let (base_url, clean_app_id) = lark_api_base_and_app_id(channel.app_id.as_deref());
    let app_id = clean_app_id.ok_or_else(|| "Lark App ID is not configured".to_string())?;
    let app_secret = channel
        .primary_secret
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Lark App Secret is not configured".to_string())?;

    let url = format!("{base_url}/callback/ws/endpoint");
    let (client, parsed) = build_pinned_im_client(&url, Duration::from_secs(15)).await?;
    let resp = client
        .post(parsed)
        .header("locale", "zh")
        .json(&json!({
            "AppID": app_id,
            "AppSecret": app_secret,
        }))
        .send()
        .await
        .map_err(|e| {
            format_reqwest_err("Feishu WS endpoint request failed", e, Some(app_secret))
        })?;

    let status = resp.status();
    let body = read_bounded_response_json(resp, "Feishu WS endpoint", Some(app_secret)).await?;
    let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if !status.is_success() || code != 0 {
        let msg = body
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("unknown error");
        let safe_msg = redact_secrets(msg, &[app_secret]);
        return Err(format!(
            "Feishu WS endpoint failed (code {code}): {safe_msg}"
        ));
    }

    let ws_url = body
        .pointer("/data/URL")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Feishu WS endpoint response missing data.URL".to_string())?
        .to_string();
    let client_cfg = body
        .pointer("/data/ClientConfig")
        .map(FeishuClientConfig::from_json)
        .unwrap_or_default();
    let service_id = parse_feishu_service_id(&ws_url);
    Ok((ws_url, service_id, client_cfg))
}

/// Long-running Feishu / Lark WebSocket Long-Connection runner.
pub async fn run_lark_ws(bridge: Arc<ImChannelBridge>, channel_id: String) {
    let mut backoff_secs = BACKOFF_INITIAL_SECS;
    let mut ever_connected = false;

    loop {
        let channel = match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Lark
                    && ch.transport_mode == ImTransportMode::LongConnection =>
            {
                ch
            }
            _ => break,
        };
        let app_secret = channel.primary_secret.clone().unwrap_or_default();
        if channel
            .app_id
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
            || app_secret.trim().is_empty()
        {
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Error,
                "App ID and App Secret required",
            );
            break;
        }

        if ever_connected {
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Reconnecting,
                "Reconnecting (WS)",
            );
        } else {
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Connecting,
                "Connecting (WS)",
            );
        }

        let (ws_url, service_id, mut client_cfg) = match fetch_lark_ws_endpoint(&channel).await {
            Ok(v) => v,
            Err(err) => {
                warn!(channel_id = %channel_id, error = %err, "Feishu WS endpoint discovery failed");
                update_status(
                    &bridge,
                    &channel_id,
                    if ever_connected {
                        ImConnectionStatus::Reconnecting
                    } else {
                        ImConnectionStatus::Error
                    },
                    &err,
                );
                tokio::time::sleep(jittered_backoff(
                    backoff_secs,
                    BACKOFF_MAX_SECS,
                    rand_unit_f64(),
                ))
                .await;
                backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                continue;
            }
        };

        let origin = safe_url_origin(&ws_url);
        let ws_stream = match connect_pinned_wss(
            &ws_url,
            FEISHU_ALLOWED_WS_SUFFIXES,
            &[app_secret.as_str()],
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                warn!(channel_id = %channel_id, origin = %origin, error = %err, "Feishu WS connect failed");
                update_status(
                    &bridge,
                    &channel_id,
                    if ever_connected {
                        ImConnectionStatus::Reconnecting
                    } else {
                        ImConnectionStatus::Error
                    },
                    &err,
                );
                tokio::time::sleep(jittered_backoff(
                    backoff_secs,
                    BACKOFF_MAX_SECS,
                    rand_unit_f64(),
                ))
                .await;
                backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                continue;
            }
        };

        info!(channel_id = %channel_id, origin = %origin, "Feishu WS long-connection established");
        ever_connected = true;
        backoff_secs = BACKOFF_INITIAL_SECS;
        update_status(
            &bridge,
            &channel_id,
            ImConnectionStatus::Connected,
            "Connected (WS)",
        );

        let disconnect_reason = run_lark_ws_session(
            &bridge,
            &channel_id,
            service_id,
            &mut client_cfg,
            ws_stream,
            &app_secret,
        )
        .await;

        // Check if channel was disabled/deleted/switched before reconnecting
        match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Lark
                    && ch.transport_mode == ImTransportMode::LongConnection => {}
            _ => break,
        }

        let safe_reason = redact_secrets(&disconnect_reason, &[app_secret.as_str()]);
        warn!(channel_id = %channel_id, reason = %safe_reason, "Feishu WS disconnected; refreshing endpoint");
        update_status(
            &bridge,
            &channel_id,
            ImConnectionStatus::Reconnecting,
            &format!("Reconnecting: {safe_reason}"),
        );
        let delay = jittered_backoff(backoff_secs, BACKOFF_MAX_SECS, rand_unit_f64());
        tokio::time::sleep(delay).await;
        backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
    }
}

async fn run_lark_ws_session(
    bridge: &Arc<ImChannelBridge>,
    channel_id: &str,
    service_id: i32,
    client_cfg: &mut FeishuClientConfig,
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    app_secret: &str,
) -> String {
    let (mut sink, mut stream) = ws_stream.split();
    let mut assembler = FeishuChunkAssembler::new();
    let mut last_activity = Instant::now();
    let mut ping_interval =
        tokio::time::interval(Duration::from_secs(client_cfg.ping_interval_secs));
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Consume immediate first tick
    ping_interval.tick().await;

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                let stale_limit = Duration::from_secs(client_cfg.ping_interval_secs.saturating_mul(2).saturating_add(20));
                if last_activity.elapsed() > stale_limit {
                    return "Ping watchdog timed out".to_string();
                }
                let ping_bytes = Pbbp2Frame::ping(service_id).encode();
                if let Err(e) = sink.send(WsMessage::Binary(ping_bytes.into())).await {
                    return format!("Ping send failed: {}", redact_secrets(&e.to_string(), &[app_secret]));
                }
            }
            next_msg = stream.next() => {
                let Some(msg_res) = next_msg else {
                    return "WebSocket stream closed".to_string();
                };
                let msg = match msg_res {
                    Ok(m) => m,
                    Err(e) => {
                        return format!("WebSocket read error: {}", redact_secrets(&e.to_string(), &[app_secret]));
                    }
                };
                last_activity = Instant::now();

                match msg {
                    WsMessage::Binary(bytes) => {
                        let frame = match Pbbp2Frame::decode(&bytes) {
                            Ok(f) => f,
                            Err(e) => {
                                warn!(channel_id = %channel_id, error = %e, "Failed to decode Feishu pbbp2 frame");
                                continue;
                            }
                        };
                        match frame.method {
                            FRAME_METHOD_CONTROL => {
                                match frame.header("type") {
                                    Some("pong") => {
                                        if let Some(ref payload) = frame.payload
                                            && let Ok(val) = serde_json::from_slice::<Value>(payload)
                                        {
                                            let updated = FeishuClientConfig::from_json(&val);
                                            if updated.ping_interval_secs != client_cfg.ping_interval_secs {
                                                *client_cfg = updated;
                                                ping_interval = tokio::time::interval(Duration::from_secs(client_cfg.ping_interval_secs));
                                                ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                                                ping_interval.tick().await;
                                            }
                                        }
                                    }
                                    Some("ping") => {
                                        let pong = Pbbp2Frame::pong_from(&frame).encode();
                                        let _ = sink.send(WsMessage::Binary(pong.into())).await;
                                    }
                                    _ => {}
                                }
                            }
                            FRAME_METHOD_DATA => {
                                let started = Instant::now();
                                let msg_type = frame.header("type").unwrap_or("");
                                let ws_msg_id = frame.header("message_id").unwrap_or("").to_string();
                                let sum = frame
                                    .header("sum")
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .unwrap_or(1);
                                let seq = frame
                                    .header("seq")
                                    .and_then(|s| s.parse::<usize>().ok())
                                    .unwrap_or(0);
                                let raw_payload = frame.payload.clone().unwrap_or_default();

                                let full_payload = match assembler.push(&ws_msg_id, seq, sum, raw_payload, now_unix_ms()) {
                                    Ok(Some(data)) => data,
                                    Ok(None) => continue, // Waiting for remaining chunks
                                    Err(e) => {
                                        warn!(channel_id = %channel_id, error = %e, "Feishu chunk assembly error");
                                        continue;
                                    }
                                };

                                if msg_type != "event" && msg_type != "card" {
                                    if let Ok(ack) = Pbbp2Frame::ack_from(&frame, 200, started.elapsed().as_millis() as u64, None) {
                                        let _ = sink.send(WsMessage::Binary(ack.encode().into())).await;
                                    }
                                    continue;
                                }

                                let payload_val = match serde_json::from_slice::<Value>(&full_payload) {
                                    Ok(v) => v,
                                    Err(e) => {
                                        warn!(channel_id = %channel_id, error = %e, "Invalid JSON inside Feishu WS frame");
                                        if let Ok(ack) = Pbbp2Frame::ack_from(&frame, 400, started.elapsed().as_millis() as u64, None) {
                                            let _ = sink.send(WsMessage::Binary(ack.encode().into())).await;
                                        }
                                        continue;
                                    }
                                };

                                // Reload live channel config (so allowlist/routing updates apply without reconnect)
                                let Ok(Some(live_ch)) = bridge.store().get_im_channel(channel_id) else {
                                    return "Channel deleted".to_string();
                                };
                                if !live_ch.enabled || live_ch.transport_mode != ImTransportMode::LongConnection {
                                    return "Channel disabled or switched mode".to_string();
                                }

                                // Deduplicate by Feishu event_id or WS message_id
                                let dedup_token = payload_val
                                    .pointer("/header/event_id")
                                    .and_then(Value::as_str)
                                    .filter(|s| !s.trim().is_empty())
                                    .or_else(|| (!ws_msg_id.trim().is_empty()).then_some(ws_msg_id.as_str()));
                                if let Some(ev_id) = dedup_token
                                    && !bridge
                                        .check_and_mark_dedup(format!("ws:lark:{channel_id}:{ev_id}"))
                                        .await
                                {
                                    debug!(channel_id = %channel_id, event_id = %ev_id, "Deduplicated Feishu WS event");
                                    if let Ok(ack) = Pbbp2Frame::ack_from(&frame, 200, started.elapsed().as_millis() as u64, None) {
                                        let _ = sink.send(WsMessage::Binary(ack.encode().into())).await;
                                    }
                                    continue;
                                }

                                // 1. Interactive card approval callback (`card.action.trigger` or legacy card)
                                // Detached task + oneshot timeout ensures the approval completes even if ACK hits 2.2s.
                                if let Some(action) = parse_lark_card_action(&payload_val) {
                                    let b = Arc::clone(bridge);
                                    let ch = live_ch.clone();
                                    let feedback = await_detached_with_deadline(
                                        Duration::from_millis(2_200),
                                        "Processing approval…",
                                        async move {
                                            b.handle_inbound_approval_action(&ch, action).await
                                        },
                                    )
                                    .await;
                                    let toast_bytes = serde_json::to_vec(&json!({
                                        "toast": { "type": "info", "content": feedback }
                                    }))
                                    .ok();
                                    let elapsed = started.elapsed().as_millis() as u64;
                                    if let Ok(ack) = Pbbp2Frame::ack_from(&frame, 200, elapsed, toast_bytes.as_deref())
                                        && let Err(e) = sink.send(WsMessage::Binary(ack.encode().into())).await
                                    {
                                        return format!("Feishu ACK send failed: {}", redact_secrets(&e.to_string(), &[app_secret]));
                                    }
                                    continue;
                                }

                                // 2. Immediate ACK (<3s Feishu SLA), then enqueue serialized async message pipeline
                                let elapsed = started.elapsed().as_millis() as u64;
                                if let Ok(ack) = Pbbp2Frame::ack_from(&frame, 200, elapsed, None)
                                    && let Err(e) = sink.send(WsMessage::Binary(ack.encode().into())).await
                                {
                                    return format!("Feishu ACK send failed: {}", redact_secrets(&e.to_string(), &[app_secret]));
                                }

                                if let Some(inbound) = parse_lark_message_event(&payload_val) {
                                    bridge.spawn_serialized_inbound_message(live_ch, inbound);
                                }
                            }
                            _ => {}
                        }
                    }
                    WsMessage::Ping(data) => {
                        let _ = sink.send(WsMessage::Pong(data)).await;
                    }
                    WsMessage::Pong(_) => {}
                    WsMessage::Close(frame_opt) => {
                        let detail = frame_opt
                            .map(|f| format!("code={} {}", f.code, f.reason))
                            .unwrap_or_else(|| "remote close".to_string());
                        return detail;
                    }
                    WsMessage::Text(_) | WsMessage::Frame(_) => {}
                }
            }
        }
    }
}

// ==================== 2. SLACK SOCKET MODE (WSS) ====================

/// Parsed Slack Socket Mode WebSocket envelope.
#[derive(Debug, Clone, PartialEq)]
pub enum SlackSocketEnvelope {
    Hello,
    Disconnect {
        reason: String,
    },
    EventsApi {
        envelope_id: String,
        payload: Value,
    },
    Interactive {
        envelope_id: String,
        payload: Value,
    },
    Other {
        envelope_id: Option<String>,
        kind: String,
    },
}

/// Parse a raw Slack Socket Mode JSON frame into a typed `SlackSocketEnvelope`.
pub fn parse_slack_socket_envelope(raw: &str) -> Result<SlackSocketEnvelope, String> {
    let val: Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid Slack Socket Mode JSON: {e}"))?;
    let kind = val
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "Slack Socket Mode frame missing `type`".to_string())?;
    let envelope_id = val
        .get("envelope_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    match kind {
        "hello" => Ok(SlackSocketEnvelope::Hello),
        "disconnect" => {
            let reason = val
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("refresh_requested")
                .to_string();
            Ok(SlackSocketEnvelope::Disconnect { reason })
        }
        "events_api" => {
            let id = envelope_id
                .ok_or_else(|| "Slack events_api frame missing `envelope_id`".to_string())?;
            let payload = val.get("payload").cloned().unwrap_or(Value::Null);
            Ok(SlackSocketEnvelope::EventsApi {
                envelope_id: id,
                payload,
            })
        }
        "interactive" => {
            let id = envelope_id
                .ok_or_else(|| "Slack interactive frame missing `envelope_id`".to_string())?;
            let payload = val.get("payload").cloned().unwrap_or(Value::Null);
            Ok(SlackSocketEnvelope::Interactive {
                envelope_id: id,
                payload,
            })
        }
        other => Ok(SlackSocketEnvelope::Other {
            envelope_id,
            kind: other.to_string(),
        }),
    }
}

/// Serialize a Slack Socket Mode acknowledgement frame (`{"envelope_id":"..."}`).
pub fn build_slack_socket_ack(envelope_id: &str) -> String {
    serde_json::to_string(&json!({ "envelope_id": envelope_id })).unwrap_or_default()
}

async fn open_slack_socket_url(app_token: &str) -> Result<(String, bool), String> {
    let url = "https://slack.com/api/apps.connections.open";
    let (client, parsed) = build_pinned_im_client(url, Duration::from_secs(15)).await?;
    let resp = client
        .post(parsed)
        .bearer_auth(app_token)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .send()
        .await
        .map_err(|e| {
            format_reqwest_err("Slack apps.connections.open failed", e, Some(app_token))
        })?;

    let status = resp.status();
    let body =
        read_bounded_response_json(resp, "Slack apps.connections.open", Some(app_token)).await?;
    if !status.is_success() || !body.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        let err_code = body
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("unknown_error");
        let permanent = matches!(
            err_code,
            "invalid_auth"
                | "not_allowed_token_type"
                | "token_revoked"
                | "account_inactive"
                | "missing_scope"
        );
        return Err(if permanent {
            format!("PERMANENT:Slack apps.connections.open error: {err_code}")
        } else {
            format!("Slack apps.connections.open error: {err_code}")
        });
    }

    let ws_url = body
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Slack apps.connections.open response missing url".to_string())?
        .to_string();
    Ok((ws_url, false))
}

/// Long-running Slack Socket Mode WSS runner.
pub async fn run_slack_socket_mode(bridge: Arc<ImChannelBridge>, channel_id: String) {
    let mut backoff_secs = BACKOFF_INITIAL_SECS;
    let mut ever_connected = false;

    loop {
        let channel = match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Slack
                    && ch.transport_mode == ImTransportMode::LongConnection =>
            {
                ch
            }
            _ => break,
        };
        let app_token = channel
            .app_token
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let bot_token = channel
            .primary_secret
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        if app_token.is_empty() || bot_token.is_empty() {
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Error,
                "Bot Token and App-Level Token (xapp-...) required",
            );
            break;
        }

        update_status(
            &bridge,
            &channel_id,
            if ever_connected {
                ImConnectionStatus::Reconnecting
            } else {
                ImConnectionStatus::Connecting
            },
            if ever_connected {
                "Reconnecting (WS)"
            } else {
                "Connecting (WS)"
            },
        );

        let ws_url = match open_slack_socket_url(&app_token).await {
            Ok((u, _)) => u,
            Err(err) => {
                if let Some(perm_msg) = err.strip_prefix("PERMANENT:") {
                    warn!(channel_id = %channel_id, error = %perm_msg, "Slack Socket Mode permanent auth error");
                    update_status(&bridge, &channel_id, ImConnectionStatus::Error, perm_msg);
                    break;
                }
                warn!(channel_id = %channel_id, error = %err, "Slack apps.connections.open failed");
                update_status(
                    &bridge,
                    &channel_id,
                    if ever_connected {
                        ImConnectionStatus::Reconnecting
                    } else {
                        ImConnectionStatus::Error
                    },
                    &err,
                );
                tokio::time::sleep(jittered_backoff(
                    backoff_secs,
                    BACKOFF_MAX_SECS,
                    rand_unit_f64(),
                ))
                .await;
                backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                continue;
            }
        };

        let origin = safe_url_origin(&ws_url);
        let ws_stream = match connect_pinned_wss(
            &ws_url,
            SLACK_ALLOWED_WS_SUFFIXES,
            &[app_token.as_str(), bot_token.as_str()],
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                warn!(channel_id = %channel_id, origin = %origin, error = %err, "Slack Socket Mode connect failed");
                update_status(
                    &bridge,
                    &channel_id,
                    if ever_connected {
                        ImConnectionStatus::Reconnecting
                    } else {
                        ImConnectionStatus::Error
                    },
                    &err,
                );
                tokio::time::sleep(jittered_backoff(
                    backoff_secs,
                    BACKOFF_MAX_SECS,
                    rand_unit_f64(),
                ))
                .await;
                backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                continue;
            }
        };

        let (graceful_refresh, disconnect_reason) = run_slack_ws_session(
            &bridge,
            &channel_id,
            &mut ever_connected,
            &mut backoff_secs,
            ws_stream,
            &app_token,
            &bot_token,
        )
        .await;

        match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Slack
                    && ch.transport_mode == ImTransportMode::LongConnection => {}
            _ => break,
        }

        let safe_reason = redact_secrets(
            &disconnect_reason,
            &[app_token.as_str(), bot_token.as_str()],
        );
        if graceful_refresh {
            info!(channel_id = %channel_id, reason = %safe_reason, "Slack Socket Mode refresh requested; reconnecting");
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Reconnecting,
                "Reconnecting (refresh requested)",
            );
            let jitter_ms = 250 + (rand_unit_f64() * 500.0) as u64;
            tokio::time::sleep(Duration::from_millis(jitter_ms)).await;
        } else {
            warn!(channel_id = %channel_id, reason = %safe_reason, "Slack Socket Mode disconnected");
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Reconnecting,
                &format!("Reconnecting: {safe_reason}"),
            );
            tokio::time::sleep(jittered_backoff(
                backoff_secs,
                BACKOFF_MAX_SECS,
                rand_unit_f64(),
            ))
            .await;
            backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
        }
    }
}

async fn run_slack_ws_session(
    bridge: &Arc<ImChannelBridge>,
    channel_id: &str,
    ever_connected: &mut bool,
    backoff_secs: &mut u64,
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
    app_token: &str,
    bot_token: &str,
) -> (bool, String) {
    let (mut sink, mut stream) = ws_stream.split();
    let mut last_activity = Instant::now();
    let mut ping_interval = tokio::time::interval(Duration::from_secs(30));
    ping_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    ping_interval.tick().await;

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if last_activity.elapsed() > Duration::from_secs(95) {
                    return (false, "Keepalive watchdog timed out".to_string());
                }
                if let Err(e) = sink.send(WsMessage::Ping(Vec::new().into())).await {
                    return (false, format!("WS ping failed: {}", redact_secrets(&e.to_string(), &[app_token, bot_token])));
                }
            }
            next_msg = stream.next() => {
                let Some(msg_res) = next_msg else {
                    return (false, "WebSocket stream closed".to_string());
                };
                let msg = match msg_res {
                    Ok(m) => m,
                    Err(e) => {
                        return (false, format!("WebSocket read error: {}", redact_secrets(&e.to_string(), &[app_token, bot_token])));
                    }
                };
                last_activity = Instant::now();

                match msg {
                    WsMessage::Text(text) => {
                        let envelope = match parse_slack_socket_envelope(&text) {
                            Ok(env) => env,
                            Err(e) => {
                                warn!(channel_id = %channel_id, error = %e, "Failed to parse Slack Socket Mode envelope");
                                continue;
                            }
                        };
                        match envelope {
                            SlackSocketEnvelope::Hello => {
                                *ever_connected = true;
                                *backoff_secs = BACKOFF_INITIAL_SECS;
                                info!(channel_id = %channel_id, "Slack Socket Mode hello received");
                                update_status(
                                    bridge,
                                    channel_id,
                                    ImConnectionStatus::Connected,
                                    "Connected (WS)",
                                );
                            }
                            SlackSocketEnvelope::Disconnect { reason } => {
                                let _ = sink.send(WsMessage::Close(None)).await;
                                return (true, reason);
                            }
                            SlackSocketEnvelope::EventsApi { envelope_id, payload } => {
                                // ACK immediately before running agent work
                                let ack = build_slack_socket_ack(&envelope_id);
                                if let Err(e) = sink.send(WsMessage::Text(ack.into())).await {
                                    return (false, format!("Slack ACK failed: {}", redact_secrets(&e.to_string(), &[app_token, bot_token])));
                                }
                                if !bridge.check_and_mark_dedup(format!("ws:slack:{channel_id}:{envelope_id}")).await {
                                    continue;
                                }
                                let Ok(Some(live_ch)) = bridge.store().get_im_channel(channel_id) else {
                                    return (false, "Channel deleted".to_string());
                                };
                                if !live_ch.enabled || live_ch.transport_mode != ImTransportMode::LongConnection {
                                    return (false, "Channel disabled".to_string());
                                }
                                if let Some(inbound) = parse_slack_event_callback(&payload) {
                                    bridge.spawn_serialized_inbound_message(live_ch, inbound);
                                }
                            }
                            SlackSocketEnvelope::Interactive { envelope_id, payload } => {
                                let ack = build_slack_socket_ack(&envelope_id);
                                if let Err(e) = sink.send(WsMessage::Text(ack.into())).await {
                                    return (false, format!("Slack interactive ACK failed: {}", redact_secrets(&e.to_string(), &[app_token, bot_token])));
                                }
                                if !bridge.check_and_mark_dedup(format!("ws:slack:{channel_id}:{envelope_id}")).await {
                                    continue;
                                }
                                let Ok(Some(live_ch)) = bridge.store().get_im_channel(channel_id) else {
                                    return (false, "Channel deleted".to_string());
                                };
                                if !live_ch.enabled || live_ch.transport_mode != ImTransportMode::LongConnection {
                                    return (false, "Channel disabled".to_string());
                                }
                                if let Some(action) = parse_slack_interactive_action(&payload) {
                                    let b = Arc::clone(bridge);
                                    tokio::spawn(async move {
                                        let chat_id = action.chat_id.clone();
                                        let thread_id = action.thread_id.clone();
                                        if let Ok(msg) = b.handle_inbound_approval_action(&live_ch, action).await {
                                            let _ = b.send_text_message(&live_ch, &chat_id, &thread_id, &msg).await;
                                        }
                                    });
                                }
                            }
                            SlackSocketEnvelope::Other { envelope_id, .. } => {
                                if let Some(id) = envelope_id {
                                    let ack = build_slack_socket_ack(&id);
                                    let _ = sink.send(WsMessage::Text(ack.into())).await;
                                }
                            }
                        }
                    }
                    WsMessage::Ping(data) => {
                        let _ = sink.send(WsMessage::Pong(data)).await;
                    }
                    WsMessage::Pong(_) => {}
                    WsMessage::Close(frame_opt) => {
                        let detail = frame_opt
                            .map(|f| format!("code={} {}", f.code, f.reason))
                            .unwrap_or_else(|| "remote close".to_string());
                        return (false, detail);
                    }
                    WsMessage::Binary(_) | WsMessage::Frame(_) => {}
                }
            }
        }
    }
}

// ==================== 3. DISCORD ED25519 VERIFICATION & GATEWAY v10 (WSS) ====================

/// Decode an exact-length hexadecimal string into `[u8; N]`.
pub fn decode_hex_fixed<const N: usize>(raw: &str) -> Result<[u8; N], String> {
    let trimmed = raw.trim();
    if trimmed.len() != N * 2 {
        return Err(format!(
            "Expected {} hex characters, got {}",
            N * 2,
            trimmed.len()
        ));
    }
    let bytes = trimmed.as_bytes();
    let mut out = [0u8; N];
    for i in 0..N {
        let hi =
            hex_nibble(bytes[i * 2]).ok_or_else(|| "Invalid hexadecimal character".to_string())?;
        let lo = hex_nibble(bytes[i * 2 + 1])
            .ok_or_else(|| "Invalid hexadecimal character".to_string())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Check whether `raw` is a valid non-weak 32-byte Ed25519 public key in hex (64 hex chars).
pub fn is_valid_ed25519_public_key_hex(raw: &str) -> bool {
    let Ok(pk_bytes) = decode_hex_fixed::<32>(raw) else {
        return false;
    };
    VerifyingKey::from_bytes(&pk_bytes).is_ok_and(|vk| !vk.is_weak())
}

/// Fail-closed Discord Ed25519 webhook signature verification (`X-Signature-Ed25519`,
/// `X-Signature-Timestamp`, and raw body) with ±5-minute replay protection and strict
/// non-malleable Ed25519 verification (`VerifyingKey::verify_strict`).
pub fn verify_discord_ed25519_signature(
    public_key_hex: &str,
    timestamp_header: &str,
    signature_header: &str,
    raw_body: &[u8],
    now_unix_secs: i64,
) -> Result<(), String> {
    let pk_trim = public_key_hex.trim();
    if pk_trim.is_empty() {
        return Err("Discord Ed25519 public key is not configured".to_string());
    }
    let pk_bytes = decode_hex_fixed::<32>(pk_trim)
        .map_err(|_| "Invalid Discord Ed25519 public key hex".to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|_| "Invalid Discord Ed25519 public key".to_string())?;
    if verifying_key.is_weak() {
        return Err("Rejected weak Ed25519 public key".to_string());
    }

    let ts_trim = timestamp_header.trim();
    let ts: i64 = ts_trim
        .parse()
        .map_err(|_| "Missing or invalid X-Signature-Timestamp".to_string())?;
    if ts <= 0 || now_unix_secs <= 0 || now_unix_secs.abs_diff(ts) > SIGNATURE_REPLAY_WINDOW_SECS {
        return Err("Discord request timestamp outside 5-minute replay window".to_string());
    }

    let sig_bytes = decode_hex_fixed::<64>(signature_header)
        .map_err(|_| "Missing or invalid X-Signature-Ed25519".to_string())?;
    let signature = Signature::from_bytes(&sig_bytes);

    let mut signed_msg = Vec::with_capacity(ts_trim.len() + raw_body.len());
    signed_msg.extend_from_slice(ts_trim.as_bytes());
    signed_msg.extend_from_slice(raw_body);

    verifying_key
        .verify_strict(&signed_msg, &signature)
        .map_err(|_| "Invalid Discord Ed25519 signature".to_string())
}

/// Parsed Discord Gateway v10 opcode frame.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscordGatewayFrame {
    Dispatch {
        seq: Option<u64>,
        event_name: String,
        data: Value,
    },
    HeartbeatRequest,
    Reconnect,
    InvalidSession {
        resumable: bool,
    },
    Hello {
        heartbeat_interval_ms: u64,
    },
    HeartbeatAck,
    Unknown(u64),
}

/// Parse a raw Discord Gateway v10 JSON text frame.
pub fn parse_discord_gateway_frame(raw: &str) -> Result<DiscordGatewayFrame, String> {
    let val: Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid Discord Gateway JSON: {e}"))?;
    let op = val
        .get("op")
        .and_then(Value::as_u64)
        .ok_or_else(|| "Discord Gateway frame missing `op`".to_string())?;

    match op {
        0 => {
            let seq = val.get("s").and_then(Value::as_u64);
            let event_name = val
                .get("t")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let data = val.get("d").cloned().unwrap_or(Value::Null);
            Ok(DiscordGatewayFrame::Dispatch {
                seq,
                event_name,
                data,
            })
        }
        1 => Ok(DiscordGatewayFrame::HeartbeatRequest),
        7 => Ok(DiscordGatewayFrame::Reconnect),
        9 => {
            let resumable = val.get("d").and_then(Value::as_bool).unwrap_or(false);
            Ok(DiscordGatewayFrame::InvalidSession { resumable })
        }
        10 => {
            let interval = val
                .pointer("/d/heartbeat_interval")
                .and_then(Value::as_u64)
                .ok_or_else(|| "Discord HELLO missing heartbeat_interval".to_string())?;
            Ok(DiscordGatewayFrame::Hello {
                heartbeat_interval_ms: interval.clamp(5_000, 120_000),
            })
        }
        11 => Ok(DiscordGatewayFrame::HeartbeatAck),
        other => Ok(DiscordGatewayFrame::Unknown(other)),
    }
}

pub fn build_discord_heartbeat_payload(last_seq: Option<u64>) -> String {
    serde_json::to_string(&json!({ "op": 1, "d": last_seq })).unwrap_or_default()
}

pub fn build_discord_identify_payload(bot_token: &str, intents: u64) -> String {
    serde_json::to_string(&json!({
        "op": 2,
        "d": {
            "token": bot_token,
            "intents": intents,
            "properties": {
                "os": std::env::consts::OS,
                "browser": "orbit",
                "device": "orbit",
            }
        }
    }))
    .unwrap_or_default()
}

pub fn build_discord_resume_payload(bot_token: &str, session_id: &str, seq: u64) -> String {
    serde_json::to_string(&json!({
        "op": 6,
        "d": {
            "token": bot_token,
            "session_id": session_id,
            "seq": seq,
        }
    }))
    .unwrap_or_default()
}

/// Append `?v=10&encoding=json` cleanly to a Discord gateway base URL.
pub fn append_discord_gateway_query(base: &str) -> String {
    let trimmed = base.trim();
    if trimmed.contains('?') {
        if trimmed.contains("v=10") {
            trimmed.to_string()
        } else {
            format!("{trimmed}&v=10&encoding=json")
        }
    } else if trimmed.ends_with('/') {
        format!("{trimmed}?v=10&encoding=json")
    } else {
        format!("{trimmed}/?v=10&encoding=json")
    }
}

/// Strip Discord user/bot mentions (`<@12345>` / `<@!12345>`) cleanly.
pub fn strip_discord_mentions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<@") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let digits_start = after.strip_prefix('!').unwrap_or(after);
        if let Some(end) = digits_start.find('>') {
            let id_slice = &digits_start[..end];
            if !id_slice.is_empty() && id_slice.bytes().all(|b| b.is_ascii_digit()) {
                rest = &digits_start[end + 1..];
                continue;
            }
        }
        out.push_str("<@");
        rest = after;
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// Parse a Discord Gateway `MESSAGE_CREATE` dispatch payload into `InboundImMessage`.
pub fn parse_discord_gateway_message_create(data: &Value) -> Option<InboundImMessage> {
    if data
        .pointer("/author/bot")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return None;
    }
    let raw_content = data.get("content").and_then(Value::as_str)?;
    let text = strip_discord_mentions(raw_content);
    if text.is_empty() {
        return None;
    }
    let chat_id = data.get("channel_id").and_then(Value::as_str)?.to_string();
    let user_id = data
        .pointer("/author/id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let message_id = data
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(InboundImMessage {
        message_id,
        chat_id,
        thread_id: String::new(),
        user_id,
        text,
    })
}

/// Parsed Discord `INTERACTION_CREATE` (from either Gateway v10 or signed Ed25519 Webhook).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiscordInteraction {
    pub interaction_id: String,
    pub application_id: String,
    pub interaction_token: String,
    pub kind: u64,
    pub chat_id: String,
    pub user_id: String,
    pub custom_id: Option<String>,
    pub command_text: Option<String>,
}

/// Parse a Discord `INTERACTION_CREATE` payload (supports component approval buttons `type=3`
/// and application commands `type=2`, in both guild channels and DMs).
pub fn parse_discord_interaction_create(data: &Value) -> Option<ParsedDiscordInteraction> {
    let kind = data.get("type").and_then(Value::as_u64)?;
    let interaction_id = data.get("id").and_then(Value::as_str)?.to_string();
    let application_id = data
        .get("application_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let interaction_token = data
        .get("token")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let chat_id = data
        .get("channel_id")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/channel/id").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let user_id = data
        .pointer("/member/user/id")
        .and_then(Value::as_str)
        .or_else(|| data.pointer("/user/id").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    let custom_id = data
        .pointer("/data/custom_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let command_text = if kind == 2 {
        data.pointer("/data/name")
            .and_then(Value::as_str)
            .map(|name| {
                let first_opt = data
                    .pointer("/data/options/0/value")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                match first_opt {
                    Some(arg) => format!("/{name} {arg}"),
                    None => format!("/{name}"),
                }
            })
    } else {
        None
    };

    Some(ParsedDiscordInteraction {
        interaction_id,
        application_id,
        interaction_token,
        kind,
        chat_id,
        user_id,
        custom_id,
        command_text,
    })
}

/// Classification of Discord Gateway close codes per Discord API v10 documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscordCloseDisposition {
    Fatal(String),
    ResetSession,
    Resumable,
}

pub fn classify_discord_close_code(code: u16) -> DiscordCloseDisposition {
    match code {
        4004 => {
            DiscordCloseDisposition::Fatal("Authentication failed (4004): invalid Bot Token".into())
        }
        4010 => DiscordCloseDisposition::Fatal("Invalid shard (4010)".into()),
        4011 => DiscordCloseDisposition::Fatal("Sharding required (4011)".into()),
        4012 => DiscordCloseDisposition::Fatal("Invalid API version (4012)".into()),
        4013 => DiscordCloseDisposition::Fatal("Invalid intents (4013)".into()),
        4014 => DiscordCloseDisposition::Fatal(
            "Disallowed intents (4014): enable Message Content Intent in Discord Developer Portal"
                .into(),
        ),
        4007 | 4009 => DiscordCloseDisposition::ResetSession,
        _ => DiscordCloseDisposition::Resumable,
    }
}

/// Discord `/gateway/bot` `session_start_limit` metadata governing fresh `IDENTIFY` pacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscordSessionStartLimit {
    pub total: u64,
    pub remaining: u64,
    pub reset_after_ms: u64,
    pub max_concurrency: u64,
}

/// Minimum spacing between fresh Discord `IDENTIFY` calls (Discord enforces 1 IDENTIFY / 5s per bucket).
const DISCORD_IDENTIFY_MIN_INTERVAL: Duration = Duration::from_secs(5);

/// Tracks Discord `session_start_limit` and enforces fresh `IDENTIFY` pacing (while leaving
/// `RESUME` outside the `IDENTIFY` budget).
#[derive(Debug, Default)]
pub struct DiscordIdentifyPacer {
    last_identify_at: Option<Instant>,
    limit: Option<DiscordSessionStartLimit>,
    limit_fetched_at: Option<Instant>,
}

impl DiscordIdentifyPacer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_limit(&mut self, limit: Option<DiscordSessionStartLimit>, now: Instant) {
        if let Some(lim) = limit {
            self.limit = Some(lim);
            self.limit_fetched_at = Some(now);
        }
    }

    /// Compute how long to wait before opening a fresh `IDENTIFY` session (`None` if ready now).
    /// Returns `(wait_duration, needs_refetch_after_wait)`.
    pub fn compute_wait(&self, now: Instant) -> Option<(Duration, bool)> {
        if let (Some(lim), Some(fetched_at)) = (self.limit, self.limit_fetched_at)
            && lim.remaining == 0
        {
            let reset_at = fetched_at + Duration::from_millis(lim.reset_after_ms.max(1_000));
            if reset_at > now {
                return Some((reset_at - now, true));
            }
            // Reset window elapsed; caller should refetch `/gateway/bot` for fresh quota
            return Some((Duration::ZERO, true));
        }

        if let Some(last) = self.last_identify_at {
            let pacing = DISCORD_IDENTIFY_MIN_INTERVAL;
            let next_allowed = last + pacing;
            if next_allowed > now {
                return Some((next_allowed - now, false));
            }
        }
        None
    }

    pub fn record_identify(&mut self, now: Instant) {
        self.last_identify_at = Some(now);
        if let Some(ref mut lim) = self.limit {
            lim.remaining = lim.remaining.saturating_sub(1);
        }
    }
}

pub fn parse_discord_gateway_bot_response(
    body: &Value,
) -> Result<(String, Option<DiscordSessionStartLimit>), String> {
    let base_url = body
        .get("url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Discord gateway/bot response missing url".to_string())?;
    let limit = body.get("session_start_limit").and_then(|ssl| {
        Some(DiscordSessionStartLimit {
            total: ssl.get("total").and_then(Value::as_u64).unwrap_or(1_000),
            remaining: ssl.get("remaining").and_then(Value::as_u64)?,
            reset_after_ms: ssl.get("reset_after").and_then(Value::as_u64).unwrap_or(0),
            max_concurrency: ssl
                .get("max_concurrency")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1),
        })
    });
    Ok((append_discord_gateway_query(base_url), limit))
}

async fn fetch_discord_gateway_bot(
    bot_token: &str,
) -> Result<(String, Option<DiscordSessionStartLimit>), String> {
    let url = "https://discord.com/api/v10/gateway/bot";
    let (client, parsed) = build_pinned_im_client(url, Duration::from_secs(15)).await?;
    let resp = client
        .get(parsed)
        .header(reqwest::header::AUTHORIZATION, format!("Bot {bot_token}"))
        .header(reqwest::header::USER_AGENT, "Orbit-IM-Bridge/1.0")
        .send()
        .await
        .map_err(|e| {
            format_reqwest_err("Discord gateway/bot request failed", e, Some(bot_token))
        })?;

    let status = resp.status();
    let body = read_bounded_response_json(resp, "Discord gateway/bot", Some(bot_token)).await?;
    if !status.is_success() {
        let msg = body
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("API error");
        let code = status.as_u16();
        let safe_msg = redact_secrets(msg, &[bot_token]);
        if code == 401 {
            return Err(format!(
                "PERMANENT:Discord auth failed (HTTP 401): {safe_msg}"
            ));
        }
        return Err(format!("Discord gateway/bot HTTP {code}: {safe_msg}"));
    }

    parse_discord_gateway_bot_response(&body)
}

struct DiscordSessionState {
    session_id: Option<String>,
    resume_gateway_url: Option<String>,
    last_seq: Option<u64>,
}

impl DiscordSessionState {
    fn clear(&mut self) {
        self.session_id = None;
        self.resume_gateway_url = None;
        self.last_seq = None;
    }

    fn is_resumable(&self) -> bool {
        self.resume_gateway_url.is_some() && self.session_id.is_some() && self.last_seq.is_some()
    }
}

/// Long-running Discord Gateway v10 WSS runner (replaces REST polling in `LongConnection` mode).
pub async fn run_discord_gateway(bridge: Arc<ImChannelBridge>, channel_id: String) {
    let mut backoff_secs = BACKOFF_INITIAL_SECS;
    let mut ever_connected = false;
    let mut pacer = DiscordIdentifyPacer::new();
    let mut state = DiscordSessionState {
        session_id: None,
        resume_gateway_url: None,
        last_seq: None,
    };

    loop {
        let channel = match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Discord
                    && ch.transport_mode == ImTransportMode::LongConnection =>
            {
                ch
            }
            _ => break,
        };
        let bot_token = channel
            .primary_secret
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        if bot_token.is_empty() {
            update_status(
                &bridge,
                &channel_id,
                ImConnectionStatus::Error,
                "Bot token required",
            );
            break;
        }

        update_status(
            &bridge,
            &channel_id,
            if ever_connected {
                ImConnectionStatus::Reconnecting
            } else {
                ImConnectionStatus::Connecting
            },
            if ever_connected {
                "Reconnecting (WS)"
            } else {
                "Connecting (WS)"
            },
        );

        let is_resume_attempt = state.is_resumable();
        let ws_url = if let (Some(resume_base), true) =
            (state.resume_gateway_url.as_deref(), is_resume_attempt)
        {
            // RESUME stays outside the fresh-IDENTIFY `session_start_limit` budget
            append_discord_gateway_query(resume_base)
        } else {
            match fetch_discord_gateway_bot(&bot_token).await {
                Ok((u, limit)) => {
                    pacer.update_limit(limit, Instant::now());
                    // Respect `session_start_limit` (`remaining == 0` -> wait `reset_after`;
                    // otherwise enforce the 5-second IDENTIFY pacing floor).
                    if let Some((wait_dur, needs_refetch)) = pacer.compute_wait(Instant::now()) {
                        if needs_refetch {
                            let secs = wait_dur.as_secs().max(1);
                            warn!(
                                channel_id = %channel_id,
                                wait_secs = secs,
                                "Discord session_start_limit exhausted; waiting for reset_after"
                            );
                            update_status(
                                &bridge,
                                &channel_id,
                                ImConnectionStatus::Reconnecting,
                                &format!("Waiting {secs}s for Discord IDENTIFY quota reset"),
                            );
                            if !wait_dur.is_zero() {
                                tokio::time::sleep(wait_dur).await;
                            }
                            continue;
                        }
                        if !wait_dur.is_zero() {
                            tokio::time::sleep(wait_dur).await;
                        }
                    }
                    u
                }
                Err(err) => {
                    if let Some(perm) = err.strip_prefix("PERMANENT:") {
                        warn!(channel_id = %channel_id, error = %perm, "Discord Gateway permanent auth error");
                        update_status(&bridge, &channel_id, ImConnectionStatus::Error, perm);
                        break;
                    }
                    warn!(channel_id = %channel_id, error = %err, "Discord gateway/bot lookup failed");
                    update_status(
                        &bridge,
                        &channel_id,
                        if ever_connected {
                            ImConnectionStatus::Reconnecting
                        } else {
                            ImConnectionStatus::Error
                        },
                        &err,
                    );
                    tokio::time::sleep(jittered_backoff(
                        backoff_secs,
                        BACKOFF_MAX_SECS,
                        rand_unit_f64(),
                    ))
                    .await;
                    backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                    continue;
                }
            }
        };

        let origin = safe_url_origin(&ws_url);
        let ws_stream = match connect_pinned_wss(
            &ws_url,
            DISCORD_ALLOWED_WS_SUFFIXES,
            &[bot_token.as_str()],
        )
        .await
        {
            Ok(s) => s,
            Err(err) => {
                warn!(channel_id = %channel_id, origin = %origin, error = %err, "Discord Gateway WS connect failed");
                // Fall back to fresh gateway URL on connect error
                state.resume_gateway_url = None;
                update_status(
                    &bridge,
                    &channel_id,
                    if ever_connected {
                        ImConnectionStatus::Reconnecting
                    } else {
                        ImConnectionStatus::Error
                    },
                    &err,
                );
                tokio::time::sleep(jittered_backoff(
                    backoff_secs,
                    BACKOFF_MAX_SECS,
                    rand_unit_f64(),
                ))
                .await;
                backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
                continue;
            }
        };

        let (fatal_error, disconnect_reason) = run_discord_ws_session(
            &bridge,
            &channel_id,
            &bot_token,
            &mut state,
            &mut pacer,
            &mut ever_connected,
            &mut backoff_secs,
            ws_stream,
        )
        .await;

        if let Some(fatal_msg) = fatal_error {
            warn!(channel_id = %channel_id, error = %fatal_msg, "Discord Gateway fatal close");
            update_status(&bridge, &channel_id, ImConnectionStatus::Error, &fatal_msg);
            break;
        }

        match bridge.store().get_im_channel(&channel_id) {
            Ok(Some(ch))
                if ch.enabled
                    && ch.platform == ImPlatform::Discord
                    && ch.transport_mode == ImTransportMode::LongConnection => {}
            _ => break,
        }

        let safe_reason = redact_secrets(&disconnect_reason, &[bot_token.as_str()]);
        warn!(channel_id = %channel_id, reason = %safe_reason, "Discord Gateway disconnected");
        update_status(
            &bridge,
            &channel_id,
            ImConnectionStatus::Reconnecting,
            &format!("Reconnecting: {safe_reason}"),
        );
        tokio::time::sleep(jittered_backoff(
            backoff_secs,
            BACKOFF_MAX_SECS,
            rand_unit_f64(),
        ))
        .await;
        backoff_secs = next_backoff_secs(backoff_secs, BACKOFF_MAX_SECS);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_discord_ws_session(
    bridge: &Arc<ImChannelBridge>,
    channel_id: &str,
    bot_token: &str,
    state: &mut DiscordSessionState,
    pacer: &mut DiscordIdentifyPacer,
    ever_connected: &mut bool,
    backoff_secs: &mut u64,
    ws_stream: WebSocketStream<MaybeTlsStream<TcpStream>>,
) -> (Option<String>, String) {
    let (mut sink, mut stream) = ws_stream.split();

    // 1. Wait for HELLO (op 10)
    let heartbeat_interval_ms = match tokio::time::timeout(Duration::from_secs(15), async {
        while let Some(msg_res) = stream.next().await {
            let msg = msg_res.map_err(|e| redact_secrets(&e.to_string(), &[bot_token]))?;
            if let WsMessage::Text(text) = msg {
                match parse_discord_gateway_frame(&text)? {
                    DiscordGatewayFrame::Hello {
                        heartbeat_interval_ms,
                    } => return Ok(heartbeat_interval_ms),
                    other => {
                        debug!(channel_id = %channel_id, frame = ?other, "Ignoring pre-HELLO Discord frame");
                    }
                }
            } else if let WsMessage::Close(cf) = msg {
                return Err(cf
                    .map(|f| format!("Closed before HELLO (code {})", f.code))
                    .unwrap_or_else(|| "Closed before HELLO".to_string()));
            }
        }
        Err("Stream closed before Discord HELLO".to_string())
    })
    .await
    {
        Ok(Ok(ms)) => ms,
        Ok(Err(e)) => return (None, e),
        Err(_) => return (None, "Timed out waiting for Discord HELLO".to_string()),
    };

    // 2. Send RESUME (op 6) if resumable session exists, otherwise IDENTIFY (op 2) with pacing record
    let auth_frame = if let (Some(sid), Some(seq)) = (state.session_id.as_deref(), state.last_seq) {
        build_discord_resume_payload(bot_token, sid, seq)
    } else {
        pacer.record_identify(Instant::now());
        build_discord_identify_payload(bot_token, DISCORD_DEFAULT_INTENTS)
    };
    if let Err(e) = sink.send(WsMessage::Text(auth_frame.into())).await {
        return (
            None,
            format!(
                "Discord IDENTIFY/RESUME send failed: {}",
                redact_secrets(&e.to_string(), &[bot_token])
            ),
        );
    }

    // 3. Schedule first heartbeat with jitter in [15%, 85%] of heartbeat_interval
    let first_delay_ms =
        ((heartbeat_interval_ms as f64) * (0.15 + 0.70 * rand_unit_f64())).round() as u64;
    let hb_timer = tokio::time::sleep(Duration::from_millis(first_delay_ms.max(1_000)));
    tokio::pin!(hb_timer);
    let mut awaiting_ack = false;

    loop {
        tokio::select! {
            _ = &mut hb_timer => {
                if awaiting_ack {
                    let _ = sink.send(WsMessage::Close(None)).await;
                    return (None, "Discord HEARTBEAT_ACK watchdog timed out (zombie connection)".to_string());
                }
                let hb = build_discord_heartbeat_payload(state.last_seq);
                if let Err(e) = sink.send(WsMessage::Text(hb.into())).await {
                    return (None, format!("Discord heartbeat send failed: {}", redact_secrets(&e.to_string(), &[bot_token])));
                }
                awaiting_ack = true;
                hb_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(heartbeat_interval_ms));
            }
            next_msg = stream.next() => {
                let Some(msg_res) = next_msg else {
                    return (None, "Discord Gateway stream closed".to_string());
                };
                let msg = match msg_res {
                    Ok(m) => m,
                    Err(e) => {
                        return (None, format!("Discord Gateway read error: {}", redact_secrets(&e.to_string(), &[bot_token])));
                    }
                };

                match msg {
                    WsMessage::Text(text) => {
                        let frame = match parse_discord_gateway_frame(&text) {
                            Ok(f) => f,
                            Err(e) => {
                                warn!(channel_id = %channel_id, error = %e, "Failed to parse Discord Gateway frame");
                                continue;
                            }
                        };
                        match frame {
                            DiscordGatewayFrame::HeartbeatAck => {
                                awaiting_ack = false;
                            }
                            DiscordGatewayFrame::HeartbeatRequest => {
                                let hb = build_discord_heartbeat_payload(state.last_seq);
                                if let Err(e) = sink.send(WsMessage::Text(hb.into())).await {
                                    return (None, format!("Discord immediate heartbeat failed: {}", redact_secrets(&e.to_string(), &[bot_token])));
                                }
                            }
                            DiscordGatewayFrame::Reconnect => {
                                let _ = sink.send(WsMessage::Close(None)).await;
                                return (None, "Discord Gateway requested RECONNECT (op 7)".to_string());
                            }
                            DiscordGatewayFrame::InvalidSession { resumable } => {
                                if !resumable {
                                    state.clear();
                                }
                                let _ = sink.send(WsMessage::Close(None)).await;
                                return (None, format!("Discord INVALID_SESSION (resumable={resumable})"));
                            }
                            DiscordGatewayFrame::Dispatch { seq, event_name, data } => {
                                if let Some(s) = seq {
                                    state.last_seq = Some(s);
                                }
                                match event_name.as_str() {
                                    "READY" => {
                                        state.session_id = data.get("session_id").and_then(Value::as_str).map(ToOwned::to_owned);
                                        state.resume_gateway_url = data.get("resume_gateway_url").and_then(Value::as_str).map(ToOwned::to_owned);
                                        *ever_connected = true;
                                        *backoff_secs = BACKOFF_INITIAL_SECS;
                                        info!(channel_id = %channel_id, "Discord Gateway READY");
                                        update_status(bridge, channel_id, ImConnectionStatus::Connected, "Connected (WS)");
                                    }
                                    "RESUMED" => {
                                        *ever_connected = true;
                                        *backoff_secs = BACKOFF_INITIAL_SECS;
                                        info!(channel_id = %channel_id, "Discord Gateway RESUMED");
                                        update_status(bridge, channel_id, ImConnectionStatus::Connected, "Connected (WS)");
                                    }
                                    "MESSAGE_CREATE" => {
                                        let Ok(Some(live_ch)) = bridge.store().get_im_channel(channel_id) else {
                                            return (None, "Channel deleted".to_string());
                                        };
                                        if !live_ch.enabled || live_ch.transport_mode != ImTransportMode::LongConnection {
                                            return (None, "Channel disabled".to_string());
                                        }
                                        if let Some(inbound) = parse_discord_gateway_message_create(&data) {
                                            bridge.spawn_serialized_inbound_message(live_ch, inbound);
                                        }
                                    }
                                    "INTERACTION_CREATE" => {
                                        let Ok(Some(live_ch)) = bridge.store().get_im_channel(channel_id) else {
                                            return (None, "Channel deleted".to_string());
                                        };
                                        if !live_ch.enabled || live_ch.transport_mode != ImTransportMode::LongConnection {
                                            return (None, "Channel disabled".to_string());
                                        }
                                        if let Some(interaction) = parse_discord_interaction_create(&data) {
                                            let b = Arc::clone(bridge);
                                            tokio::spawn(async move {
                                                b.handle_discord_gateway_interaction(&live_ch, interaction).await;
                                            });
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            DiscordGatewayFrame::Hello { .. } | DiscordGatewayFrame::Unknown(_) => {}
                        }
                    }
                    WsMessage::Ping(data) => {
                        let _ = sink.send(WsMessage::Pong(data)).await;
                    }
                    WsMessage::Pong(_) => {}
                    WsMessage::Close(frame_opt) => {
                        if let Some(cf) = frame_opt {
                            let code_u16: u16 = cf.code.into();
                            match classify_discord_close_code(code_u16) {
                                DiscordCloseDisposition::Fatal(msg) => {
                                    state.clear();
                                    return (Some(msg), format!("Fatal Discord close {code_u16}"));
                                }
                                DiscordCloseDisposition::ResetSession => {
                                    state.clear();
                                    return (None, format!("Discord close {code_u16} (resetting session)"));
                                }
                                DiscordCloseDisposition::Resumable => {
                                    return (None, format!("Discord close {code_u16}: {}", cf.reason));
                                }
                            }
                        }
                        return (None, "Discord Gateway closed".to_string());
                    }
                    WsMessage::Binary(_) | WsMessage::Frame(_) => {}
                }
            }
        }
    }
}

// ==================== UNIT TESTS ====================

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn feishu_pbbp2_frame_protobuf_roundtrip_ping_data_and_ack() {
        // 1. Control ping frame
        let ping = Pbbp2Frame::ping(33554433);
        let ping_bytes = ping.encode();
        let decoded_ping = Pbbp2Frame::decode(&ping_bytes).expect("decode ping");
        assert_eq!(decoded_ping, ping);
        assert_eq!(decoded_ping.method, FRAME_METHOD_CONTROL);
        assert_eq!(decoded_ping.service, 33554433);
        assert_eq!(decoded_ping.header("type"), Some("ping"));

        // 2. Data event frame with payload, headers, negative service (signed varint test), and LogIDNew
        let event_json =
            br#"{"header":{"event_id":"ev_123","event_type":"im.message.receive_v1"},"event":{}}"#;
        let data_frame = Pbbp2Frame {
            seq_id: 9876543210,
            log_id: 1234567890,
            service: -42,
            method: FRAME_METHOD_DATA,
            headers: vec![
                Pbbp2Header::new("type", "event"),
                Pbbp2Header::new("message_id", "msg_abc_1"),
                Pbbp2Header::new("sum", "1"),
                Pbbp2Header::new("seq", "0"),
                Pbbp2Header::new("trace_id", "tr_99"),
            ],
            payload_encoding: Some("json".into()),
            payload_type: Some("application/json".into()),
            payload: Some(event_json.to_vec()),
            log_id_new: Some("log_new_001".into()),
        };
        let encoded = data_frame.encode();
        let decoded = Pbbp2Frame::decode(&encoded).expect("decode data frame");
        assert_eq!(decoded, data_frame);
        assert_eq!(decoded.header("message_id"), Some("msg_abc_1"));
        assert_eq!(decoded.header("trace_id"), Some("tr_99"));
        assert_eq!(decoded.payload.as_deref(), Some(event_json.as_slice()));

        // 3. ACK frame construction without response data (im.message.receive_v1)
        let ack = Pbbp2Frame::ack_from(&decoded, 200, 14, None).expect("build ack");
        assert_eq!(ack.seq_id, decoded.seq_id);
        assert_eq!(ack.log_id, decoded.log_id);
        assert_eq!(ack.service, decoded.service);
        assert_eq!(ack.method, FRAME_METHOD_DATA);
        assert_eq!(ack.header("biz_rt"), Some("14"));
        let ack_decoded = Pbbp2Frame::decode(&ack.encode()).expect("roundtrip ack");
        let ack_json: Value =
            serde_json::from_slice(ack_decoded.payload.as_deref().expect("payload"))
                .expect("parse ack json");
        assert_eq!(ack_json, json!({ "code": 200 }));

        // 4. ACK frame with card toast data (base64 encoded JSON inside `data`)
        let toast = br#"{"toast":{"type":"info","content":"Approved"}}"#;
        let card_ack = Pbbp2Frame::ack_from(&decoded, 200, 27, Some(toast)).expect("card ack");
        let card_ack_json: Value =
            serde_json::from_slice(card_ack.payload.as_deref().expect("payload"))
                .expect("parse card ack json");
        assert_eq!(card_ack_json.get("code").and_then(Value::as_u64), Some(200));
        let b64 = card_ack_json
            .get("data")
            .and_then(Value::as_str)
            .expect("base64 data");
        let raw_toast = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("decode b64");
        assert_eq!(raw_toast, toast);

        // 5. Unknown fields are skipped cleanly, truncated frames fail safely
        let mut with_unknown = encoded.clone();
        encode_varint_field(&mut with_unknown, 42, 999);
        encode_bytes_field(&mut with_unknown, 43, b"ignored");
        let decoded_unknown = Pbbp2Frame::decode(&with_unknown).expect("skip unknown fields");
        assert_eq!(decoded_unknown, data_frame);
        assert!(Pbbp2Frame::decode(&[0x08, 0x80, 0x80]).is_err());
        assert!(Pbbp2Frame::decode(&[0x42, 0x7f, 0x01]).is_err());
    }

    #[test]
    fn feishu_chunk_assembler_reassembles_out_of_order_and_enforces_bounds() {
        let mut asm = FeishuChunkAssembler::new();
        let t0 = 1_760_000_000_000u64;
        assert_eq!(
            asm.push("m1", 1, 3, b"world".to_vec(), t0).expect("part 1"),
            None
        );
        assert_eq!(
            asm.push("m1", 0, 3, b"hello ".to_vec(), t0 + 10)
                .expect("part 0"),
            None
        );
        // Duplicate chunk ignored cleanly
        assert_eq!(
            asm.push("m1", 0, 3, b"hello ".to_vec(), t0 + 15)
                .expect("dup part 0"),
            None
        );
        let assembled = asm
            .push("m1", 2, 3, b"!".to_vec(), t0 + 20)
            .expect("part 2")
            .expect("complete");
        assert_eq!(assembled, b"hello world!");

        // Out-of-bounds seq/sum rejected cleanly
        assert!(asm.push("bad", 5, 3, vec![1], t0).is_err());
        assert!(asm.push("bad2", 0, 100, vec![1], t0).is_err());

        // Service ID extraction from Feishu URL
        assert_eq!(
            parse_feishu_service_id(
                "wss://msg-frontier.feishu.cn/ws/v2?fpid=2&access_key=secret&service_id=33554433"
            ),
            33554433
        );
    }

    #[test]
    fn slack_socket_mode_envelope_parsing_and_ack_serialization() {
        assert_eq!(
            parse_slack_socket_envelope(r#"{"type":"hello","num_connections":1}"#).expect("hello"),
            SlackSocketEnvelope::Hello
        );
        assert_eq!(
            parse_slack_socket_envelope(r#"{"type":"disconnect","reason":"refresh_requested"}"#)
                .expect("disconnect"),
            SlackSocketEnvelope::Disconnect {
                reason: "refresh_requested".into()
            }
        );

        let events_raw = r#"{
            "envelope_id": "env-001",
            "type": "events_api",
            "accepts_response_payload": false,
            "payload": {
                "type": "event_callback",
                "event_id": "Ev123",
                "event": {
                    "type": "app_mention",
                    "channel": "C12345",
                    "user": "U99999",
                    "text": "<@U00BOT> /status",
                    "ts": "1760000000.000100"
                }
            }
        }"#;
        let env = parse_slack_socket_envelope(events_raw).expect("events_api");
        match env {
            SlackSocketEnvelope::EventsApi {
                envelope_id,
                payload,
            } => {
                assert_eq!(envelope_id, "env-001");
                let ack = build_slack_socket_ack(&envelope_id);
                assert_eq!(ack, r#"{"envelope_id":"env-001"}"#);
                let msg = parse_slack_event_callback(&payload).expect("parse slack msg");
                assert_eq!(msg.chat_id, "C12345");
                assert_eq!(msg.user_id, "U99999");
                assert_eq!(msg.text, "/status");
            }
            other => panic!("expected EventsApi, got {other:?}"),
        }

        let interactive_raw = r#"{
            "envelope_id": "env-002",
            "type": "interactive",
            "accepts_response_payload": true,
            "payload": {
                "type": "block_actions",
                "user": { "id": "U99999" },
                "channel": { "id": "C12345" },
                "message": { "ts": "1760000000.000200" },
                "actions": [{
                    "action_id": "orbit_approve",
                    "action_ts": "1760000001.000000",
                    "value": "{\"decision\":\"allow\",\"agent_id\":\"ag_1\",\"request_id\":\"perm_1\"}"
                }]
            }
        }"#;
        let int_env = parse_slack_socket_envelope(interactive_raw).expect("interactive");
        match int_env {
            SlackSocketEnvelope::Interactive {
                envelope_id,
                payload,
            } => {
                assert_eq!(envelope_id, "env-002");
                let act = parse_slack_interactive_action(&payload).expect("parse block action");
                assert_eq!(act.decision, "allow");
                assert_eq!(act.agent_id.as_deref(), Some("ag_1"));
                assert_eq!(act.request_id.as_deref(), Some("perm_1"));
            }
            other => panic!("expected Interactive, got {other:?}"),
        }

        // Missing envelope_id on events_api fails cleanly
        assert!(parse_slack_socket_envelope(r#"{"type":"events_api","payload":{}}"#).is_err());
    }

    #[test]
    fn discord_gateway_v10_op_dispatch_and_close_classification() {
        // HELLO (op 10)
        assert_eq!(
            parse_discord_gateway_frame(r#"{"op":10,"d":{"heartbeat_interval":41250}}"#)
                .expect("hello"),
            DiscordGatewayFrame::Hello {
                heartbeat_interval_ms: 41250
            }
        );
        // HEARTBEAT_ACK (op 11)
        assert_eq!(
            parse_discord_gateway_frame(r#"{"op":11}"#).expect("ack"),
            DiscordGatewayFrame::HeartbeatAck
        );
        // RECONNECT (op 7)
        assert_eq!(
            parse_discord_gateway_frame(r#"{"op":7}"#).expect("reconnect"),
            DiscordGatewayFrame::Reconnect
        );
        // INVALID_SESSION (op 9)
        assert_eq!(
            parse_discord_gateway_frame(r#"{"op":9,"d":false}"#).expect("invalid_session"),
            DiscordGatewayFrame::InvalidSession { resumable: false }
        );
        // DISPATCH (op 0) MESSAGE_CREATE
        let msg_frame = parse_discord_gateway_frame(
            r#"{
                "op": 0,
                "s": 42,
                "t": "MESSAGE_CREATE",
                "d": {
                    "id": "1234567890",
                    "channel_id": "9988776655",
                    "content": "<@!555111> check tests",
                    "author": { "id": "444333", "bot": false }
                }
            }"#,
        )
        .expect("dispatch");
        match msg_frame {
            DiscordGatewayFrame::Dispatch {
                seq,
                event_name,
                data,
            } => {
                assert_eq!(seq, Some(42));
                assert_eq!(event_name, "MESSAGE_CREATE");
                let msg = parse_discord_gateway_message_create(&data).expect("parse msg");
                assert_eq!(msg.message_id, "1234567890");
                assert_eq!(msg.chat_id, "9988776655");
                assert_eq!(msg.user_id, "444333");
                assert_eq!(msg.text, "check tests");
            }
            other => panic!("expected Dispatch, got {other:?}"),
        }

        // INTERACTION_CREATE button approval
        let inter_data = json!({
            "id": "111222333",
            "token": "inter_tok_abc",
            "type": 3,
            "channel_id": "9988776655",
            "member": { "user": { "id": "444333" } },
            "data": { "custom_id": "orbit:allow:tok123456789", "component_type": 2 }
        });
        let inter = parse_discord_interaction_create(&inter_data).expect("parse interaction");
        assert_eq!(inter.kind, 3);
        assert_eq!(inter.chat_id, "9988776655");
        assert_eq!(inter.user_id, "444333");
        assert_eq!(inter.custom_id.as_deref(), Some("orbit:allow:tok123456789"));

        // Intents bitmask check
        assert_eq!(DISCORD_DEFAULT_INTENTS, 512 | 4096 | 32768);
        // Gateway URL normalization
        assert_eq!(
            append_discord_gateway_query("wss://gateway.discord.gg"),
            "wss://gateway.discord.gg/?v=10&encoding=json"
        );
        // Heartbeat / Identify / Resume payloads
        assert_eq!(
            build_discord_heartbeat_payload(Some(7)),
            r#"{"d":7,"op":1}"#
        );
        let identify: Value = serde_json::from_str(&build_discord_identify_payload(
            "tok",
            DISCORD_DEFAULT_INTENTS,
        ))
        .expect("identify json");
        assert_eq!(identify.get("op").and_then(Value::as_u64), Some(2));
        assert_eq!(
            identify.pointer("/d/intents").and_then(Value::as_u64),
            Some(DISCORD_DEFAULT_INTENTS)
        );
        let resume: Value =
            serde_json::from_str(&build_discord_resume_payload("tok", "sess_1", 99))
                .expect("resume json");
        assert_eq!(resume.get("op").and_then(Value::as_u64), Some(6));
        assert_eq!(resume.pointer("/d/seq").and_then(Value::as_u64), Some(99));

        // Close code classification
        assert!(matches!(
            classify_discord_close_code(4004),
            DiscordCloseDisposition::Fatal(_)
        ));
        assert!(matches!(
            classify_discord_close_code(4014),
            DiscordCloseDisposition::Fatal(_)
        ));
        assert_eq!(
            classify_discord_close_code(4009),
            DiscordCloseDisposition::ResetSession
        );
        assert_eq!(
            classify_discord_close_code(1001),
            DiscordCloseDisposition::Resumable
        );
    }

    #[test]
    fn discord_real_ed25519_signature_verification_passes_valid_and_rejects_attacks() {
        let seed: [u8; 32] = [
            0x42, 0x11, 0x9c, 0x77, 0x05, 0xa8, 0xf1, 0x33, 0x6b, 0x20, 0xd4, 0xe9, 0x55, 0x81,
            0x19, 0x0c, 0x7e, 0x3a, 0xbc, 0x90, 0x22, 0x4f, 0x61, 0x88, 0xa1, 0xc3, 0xe5, 0x07,
            0x29, 0x4b, 0x6d, 0x8f,
        ];
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let pubkey_hex = crate::im_channels::hex_encode(verifying_key.as_bytes());
        assert!(is_valid_ed25519_public_key_hex(&pubkey_hex));

        let now = 1_760_000_000i64;
        let ts = now.to_string();
        let body = br#"{"type":1}"#;
        let mut msg = Vec::new();
        msg.extend_from_slice(ts.as_bytes());
        msg.extend_from_slice(body);
        let sig = signing_key.sign(&msg);
        let sig_hex = crate::im_channels::hex_encode(&sig.to_bytes());

        // 1. Valid signature passes
        assert!(verify_discord_ed25519_signature(&pubkey_hex, &ts, &sig_hex, body, now).is_ok());

        // 2. Tampered body fails
        assert!(
            verify_discord_ed25519_signature(&pubkey_hex, &ts, &sig_hex, br#"{"type":2}"#, now)
                .is_err()
        );

        // 3. Tampered timestamp (even within window) fails
        let other_ts = (now - 1).to_string();
        assert!(
            verify_discord_ed25519_signature(&pubkey_hex, &other_ts, &sig_hex, body, now).is_err()
        );

        // 4. Stale timestamp (> 5 min) fails even with valid signature for that old timestamp
        let stale_ts = (now - 301).to_string();
        let mut stale_msg = Vec::new();
        stale_msg.extend_from_slice(stale_ts.as_bytes());
        stale_msg.extend_from_slice(body);
        let stale_sig_hex =
            crate::im_channels::hex_encode(&signing_key.sign(&stale_msg).to_bytes());
        assert!(
            verify_discord_ed25519_signature(&pubkey_hex, &stale_ts, &stale_sig_hex, body, now)
                .is_err()
        );

        // 5. Extreme/negative timestamps fail without panic
        for extreme in [
            i64::MIN.to_string(),
            "-5".into(),
            "0".into(),
            i64::MAX.to_string(),
        ] {
            assert!(
                verify_discord_ed25519_signature(&pubkey_hex, &extreme, &sig_hex, body, now)
                    .is_err()
            );
        }

        // 6. Bad hex, wrong length, missing public key, or wrong keypair fail closed
        assert!(verify_discord_ed25519_signature("", &ts, &sig_hex, body, now).is_err());
        assert!(verify_discord_ed25519_signature("zzzz", &ts, &sig_hex, body, now).is_err());
        assert!(verify_discord_ed25519_signature(&pubkey_hex, &ts, "not_hex!", body, now).is_err());
        assert!(verify_discord_ed25519_signature(&pubkey_hex, &ts, "", body, now).is_err());

        let other_key = SigningKey::from_bytes(&[0x99u8; 32]).verifying_key();
        let other_pk_hex = crate::im_channels::hex_encode(other_key.as_bytes());
        assert!(verify_discord_ed25519_signature(&other_pk_hex, &ts, &sig_hex, body, now).is_err());
    }

    #[test]
    fn wss_url_validation_and_secret_redaction_never_leak_tokens() {
        assert!(
            validate_wss_url(
                "wss://msg-frontier.feishu.cn/ws/v2?access_key=topsecret",
                FEISHU_ALLOWED_WS_SUFFIXES
            )
            .is_ok()
        );
        assert!(
            validate_wss_url(
                "wss://wss-primary.slack.com/link/?ticket=xapp_ticket",
                SLACK_ALLOWED_WS_SUFFIXES
            )
            .is_ok()
        );
        assert!(
            validate_wss_url(
                "wss://gateway.discord.gg/?v=10&encoding=json",
                DISCORD_ALLOWED_WS_SUFFIXES
            )
            .is_ok()
        );
        // Reject non-wss, lookalike domains, embedded creds, and trailing dots
        assert!(validate_wss_url("ws://gateway.discord.gg/", DISCORD_ALLOWED_WS_SUFFIXES).is_err());
        assert!(validate_wss_url("wss://evilfeishu.cn/ws", FEISHU_ALLOWED_WS_SUFFIXES).is_err());
        assert!(
            validate_wss_url("wss://user:pass@slack.com/link", SLACK_ALLOWED_WS_SUFFIXES).is_err()
        );
        assert!(
            validate_wss_url("wss://gateway.discord.gg./", DISCORD_ALLOWED_WS_SUFFIXES).is_err()
        );

        let raw_err = "connect failed to wss://msg-frontier.feishu.cn/ws/v2?access_key=SECRET_KEY_1&fpid=2 with xapp-1-SUPER-SECRET and bot_tok_99";
        let redacted = redact_secrets(raw_err, &["xapp-1-SUPER-SECRET", "bot_tok_99"]);
        assert!(!redacted.contains("SECRET_KEY_1"));
        assert!(!redacted.contains("xapp-1-SUPER-SECRET"));
        assert!(!redacted.contains("bot_tok_99"));
        assert!(redacted.contains("wss://msg-frontier.feishu.cn/ws/v2?[REDACTED]"));
        assert_eq!(
            safe_url_origin("wss://wss-primary.slack.com/link/?ticket=SUPER_SECRET"),
            "wss://wss-primary.slack.com"
        );

        // Jittered backoff stays within ±25% and respects cap
        assert_eq!(jittered_backoff(10, 60, 0.0), Duration::from_millis(7_500));
        assert_eq!(jittered_backoff(10, 60, 0.5), Duration::from_millis(10_000));
        assert_eq!(jittered_backoff(10, 60, 1.0), Duration::from_millis(12_500));
        assert_eq!(
            jittered_backoff(200, 60, 0.5),
            Duration::from_millis(60_000)
        );
    }

    #[tokio::test]
    async fn feishu_detached_approval_does_not_cancel_mid_flight_after_ack_deadline() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let completed = Arc::new(AtomicBool::new(false));
        let completed_flag = Arc::clone(&completed);

        let ack_feedback = await_detached_with_deadline(
            Duration::from_millis(30),
            "Processing approval…",
            async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                completed_flag.store(true, Ordering::SeqCst);
                Ok("Approved".to_string())
            },
        )
        .await;

        assert_eq!(ack_feedback, "Processing approval…");
        assert!(
            !completed.load(Ordering::SeqCst),
            "work should still be running right after deadline"
        );

        // Wait past work completion and confirm the detached approval finished cleanly
        tokio::time::sleep(Duration::from_millis(180)).await;
        assert!(
            completed.load(Ordering::SeqCst),
            "detached approval task must NOT be cancelled when ACK deadline expires"
        );
    }

    #[tokio::test]
    async fn keyed_fifo_dispatcher_serializes_same_thread_in_strict_arrival_order() {
        use crate::im_channels::KeyedFifoDispatcher;
        use tokio::sync::Mutex;

        let dispatcher = KeyedFifoDispatcher::new();
        let order = Arc::new(Mutex::new(Vec::<u32>::new()));
        let key = ("ch_1".to_string(), "chat_1".to_string(), "th_1".to_string());

        for (idx, delay_ms) in [(1u32, 45u64), (2u32, 15u64), (3u32, 0u64)] {
            let log = Arc::clone(&order);
            dispatcher.enqueue(key.clone(), async move {
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }
                log.lock().await.push(idx);
            });
        }

        // Sentinel task on same key runs strictly after tasks 1, 2, 3
        let (tx, rx) = tokio::sync::oneshot::channel();
        dispatcher.enqueue(key, async move {
            let _ = tx.send(());
        });
        rx.await.expect("sentinel completes");

        let recorded = order.lock().await.clone();
        assert_eq!(recorded, vec![1, 2, 3]);
    }

    #[test]
    fn discord_identify_pacer_enforces_5s_floor_and_reset_after_on_zero_remaining() {
        let body = json!({
            "url": "wss://gateway.discord.gg",
            "shards": 1,
            "session_start_limit": {
                "total": 1000,
                "remaining": 2,
                "reset_after": 14_400_000,
                "max_concurrency": 1
            }
        });
        let (ws_url, limit) = parse_discord_gateway_bot_response(&body).expect("parse gateway/bot");
        assert_eq!(ws_url, "wss://gateway.discord.gg/?v=10&encoding=json");
        let lim = limit.expect("session_start_limit present");
        assert_eq!(lim.remaining, 2);
        assert_eq!(lim.reset_after_ms, 14_400_000);

        let t0 = Instant::now();
        let mut pacer = DiscordIdentifyPacer::new();
        pacer.update_limit(Some(lim), t0);

        // First IDENTIFY: ready immediately
        assert!(pacer.compute_wait(t0).is_none());
        pacer.record_identify(t0);

        // Second IDENTIFY 1s later: must wait 4s to satisfy 5s pacing floor (no refetch needed)
        let (wait_dur, needs_refetch) = pacer
            .compute_wait(t0 + Duration::from_secs(1))
            .expect("must pace 5s floor");
        assert!(!needs_refetch);
        assert_eq!(wait_dur, Duration::from_secs(4));

        // Record second IDENTIFY at t0 + 5s -> remaining drops to 0
        pacer.record_identify(t0 + Duration::from_secs(5));
        let (reset_wait, refetch) = pacer
            .compute_wait(t0 + Duration::from_secs(6))
            .expect("must wait for reset_after when remaining == 0");
        assert!(refetch);
        assert!(reset_wait > Duration::from_secs(14_000));
    }
}
