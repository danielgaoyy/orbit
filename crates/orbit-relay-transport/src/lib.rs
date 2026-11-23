//! Pluggable WebSocket transport for Orbit Relay.
//!
//! Supports both direct `ws://` / `wss://` Relay endpoints and zero-port `ssh+relay://`
//! endpoints backed by pure-Rust `russh` (`direct-tcpip` over SSH port 22).

use std::future::Future;
use std::net::Ipv4Addr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use futures_util::{Sink, Stream};
use russh::ChannelStream;
use russh::client::{self, AuthResult, Handler};
use russh::keys::ssh_key::{
    HashAlg, PrivateKey,
    private::{Ed25519Keypair, Ed25519PrivateKey, KeypairData},
};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, key::safe_rng};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::{Error as WsError, Message, protocol::WebSocketConfig};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_with_config, connect_async_with_config,
};

const DEFAULT_SSH_PORT: u16 = 22;
const DEFAULT_RELAY_PORT: u16 = 7878;
const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Max WebSocket frame/message size (48 MiB) so 25 MiB base64/E2EE payloads never drop sessions.
pub const MAX_RELAY_TRANSPORT_WS_FRAME_BYTES: usize = 48 * 1024 * 1024;

fn relay_ws_config() -> WebSocketConfig {
    let mut cfg = WebSocketConfig::default();
    cfg.max_frame_size = Some(MAX_RELAY_TRANSPORT_WS_FRAME_BYTES);
    cfg.max_message_size = Some(MAX_RELAY_TRANSPORT_WS_FRAME_BYTES);
    cfg
}

#[derive(Debug, thiserror::Error)]
pub enum RelayTransportError {
    #[error("invalid relay URL: {0}")]
    InvalidUrl(String),
    #[error("invalid SSH tunnel key: {0}")]
    InvalidKey(String),
    #[error("SSH connection failed: {0}")]
    Ssh(#[from] russh::Error),
    #[error("SSH key error: {0}")]
    SshKey(#[from] russh::keys::ssh_key::Error),
    #[error("SSH authentication failed")]
    SshAuthFailed,
    #[error("SSH connection timed out")]
    Timeout,
    #[error("websocket error: {0}")]
    WebSocket(Box<WsError>),
}

impl From<WsError> for RelayTransportError {
    fn from(err: WsError) -> Self {
        Self::WebSocket(Box::new(err))
    }
}

/// Parsed `ssh+relay://<user>@<ssh_host>:<ssh_port>/<relay_port>?key=<seed_b64>[&fp=<fps>]` specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshRelaySpec {
    pub user: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub relay_port: u16,
    pub key_seed_b64: String,
    pub host_key_fps_b64: Vec<String>,
}

impl SshRelaySpec {
    /// Parse and validate an `ssh+relay://` URI.
    pub fn parse(raw: &str) -> Result<Self, RelayTransportError> {
        let trimmed = raw.trim();
        let after_scheme = strip_scheme_ci(trimmed, "ssh+relay://").ok_or_else(|| {
            RelayTransportError::InvalidUrl(format!("expected ssh+relay:// scheme: {raw}"))
        })?;

        let (before_query, query_opt) = match after_scheme.split_once('?') {
            Some((left, q)) => (left, Some(q)),
            None => (after_scheme, None),
        };

        let (authority, path_opt) = match before_query.split_once('/') {
            Some((auth, path)) => (auth, Some(path)),
            None => (before_query, None),
        };

        let (user_raw, host_port_raw) = authority.split_once('@').ok_or_else(|| {
            RelayTransportError::InvalidUrl("missing `<user>@` in ssh+relay:// URL".into())
        })?;
        let user = user_raw.trim();
        if user.is_empty() || user.chars().any(char::is_whitespace) {
            return Err(RelayTransportError::InvalidUrl(
                "invalid SSH username in ssh+relay:// URL".into(),
            ));
        }

        let (ssh_host, ssh_port) = parse_host_port(host_port_raw.trim(), DEFAULT_SSH_PORT)?;
        let relay_port = match path_opt.map(|p| p.trim_matches('/')) {
            None | Some("") => DEFAULT_RELAY_PORT,
            Some(seg) => {
                let first = seg.split('/').next().unwrap_or("");
                let parsed = first.parse::<u16>().map_err(|_| {
                    RelayTransportError::InvalidUrl(format!("invalid relay port in path: {first}"))
                })?;
                if parsed == 0 {
                    return Err(RelayTransportError::InvalidUrl(
                        "relay port cannot be 0".into(),
                    ));
                }
                parsed
            }
        };

        let mut key_seed_b64: Option<String> = None;
        let mut host_key_fps_b64: Vec<String> = Vec::new();

        if let Some(query) = query_opt {
            for pair in query.split('&').filter(|s| !s.is_empty()) {
                let Some((k, v)) = pair.split_once('=') else {
                    return Err(RelayTransportError::InvalidUrl(format!(
                        "malformed query parameter: {pair}"
                    )));
                };
                match k {
                    "key" => {
                        let seed = decode_32_bytes_flexible(v).map_err(|e| {
                            RelayTransportError::InvalidKey(format!("invalid tunnel key seed: {e}"))
                        })?;
                        key_seed_b64 = Some(URL_SAFE_NO_PAD.encode(seed));
                    }
                    "fp" => {
                        for raw_fp in v.split(',').filter(|s| !s.trim().is_empty()) {
                            let canonical = normalize_sha256_fp(raw_fp)?;
                            if !host_key_fps_b64.contains(&canonical) {
                                host_key_fps_b64.push(canonical);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        let key_seed_b64 = key_seed_b64.ok_or_else(|| {
            RelayTransportError::InvalidUrl("missing `?key=` parameter in ssh+relay:// URL".into())
        })?;

        Ok(Self {
            user: user.to_string(),
            ssh_host,
            ssh_port,
            relay_port,
            key_seed_b64,
            host_key_fps_b64,
        })
    }

    /// Serialize into canonical `ssh+relay://` URI form.
    pub fn to_uri(&self) -> String {
        let host_fmt = format_host_for_uri(&self.ssh_host);
        let mut uri = format!(
            "ssh+relay://{}@{}:{}/{}?key={}",
            self.user, host_fmt, self.ssh_port, self.relay_port, self.key_seed_b64
        );
        if !self.host_key_fps_b64.is_empty() {
            uri.push_str("&fp=");
            uri.push_str(&self.host_key_fps_b64.join(","));
        }
        uri
    }

    /// Redacted human-readable representation that never exposes `?key=`.
    pub fn redacted_display(&self) -> String {
        let host_fmt = format_host_for_uri(&self.ssh_host);
        format!(
            "ssh+relay://{}@{}:{}/{} (SSH E2EE Tunnel)",
            self.user, host_fmt, self.ssh_port, self.relay_port
        )
    }

    /// Decode the 32-byte Ed25519 seed for client authentication.
    pub fn key_seed_bytes(&self) -> Result<[u8; 32], RelayTransportError> {
        decode_32_bytes_flexible(&self.key_seed_b64)
    }

    fn decoded_host_key_fps(&self) -> Result<Vec<[u8; 32]>, RelayTransportError> {
        self.host_key_fps_b64
            .iter()
            .map(|fp| decode_32_bytes_flexible(fp))
            .collect()
    }
}

/// Parsed Relay endpoint variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelayEndpoint {
    DirectWs(String),
    SshTunnel(SshRelaySpec),
}

/// Check whether a raw Relay URL uses the `ssh+relay://` transport scheme.
pub fn is_ssh_relay_url(raw: &str) -> bool {
    strip_scheme_ci(raw.trim(), "ssh+relay://").is_some()
}

/// Parse a Relay URL into either direct WebSocket or SSH-tunneled WebSocket.
pub fn parse_relay_endpoint(raw: &str) -> Result<RelayEndpoint, RelayTransportError> {
    let normalized = normalize_relay_url(raw)?;
    if is_ssh_relay_url(&normalized) {
        Ok(RelayEndpoint::SshTunnel(SshRelaySpec::parse(&normalized)?))
    } else {
        Ok(RelayEndpoint::DirectWs(normalized))
    }
}

/// Normalize a Relay URL (`ssh+relay://...`, `ws://...`, `wss://...`, `http(s)://...`, or `host:port`).
pub fn normalize_relay_url(raw: &str) -> Result<String, RelayTransportError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(RelayTransportError::InvalidUrl(
            "relay URL cannot be empty".into(),
        ));
    }

    if is_ssh_relay_url(trimmed) {
        return Ok(SshRelaySpec::parse(trimmed)?.to_uri());
    }

    let trimmed = trimmed.trim_end_matches('/');
    let had_scheme = trimmed.contains("://");
    let with_scheme = if let Some(rest) = strip_scheme_ci(trimmed, "http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = strip_scheme_ci(trimmed, "https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = strip_scheme_ci(trimmed, "ws://") {
        format!("ws://{rest}")
    } else if let Some(rest) = strip_scheme_ci(trimmed, "wss://") {
        format!("wss://{rest}")
    } else {
        format!("ws://{trimmed}")
    };

    let (scheme, after_scheme) = with_scheme
        .split_once("://")
        .ok_or_else(|| RelayTransportError::InvalidUrl(format!("invalid relay URL: {raw}")))?;

    let authority = after_scheme
        .split('/')
        .next()
        .unwrap_or(after_scheme)
        .trim();
    if authority.is_empty() {
        return Err(RelayTransportError::InvalidUrl(format!(
            "missing host in relay URL: {raw}"
        )));
    }

    let final_authority = if !had_scheme
        && !authority.contains(':')
        && (authority == "localhost" || authority.parse::<Ipv4Addr>().is_ok())
    {
        format!("{authority}:7878")
    } else {
        authority.to_string()
    };

    Ok(format!("{scheme}://{final_authority}"))
}

/// Return a log/UI-safe Relay URL string that masks any `ssh+relay://` tunnel secret.
pub fn redact_relay_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if is_ssh_relay_url(trimmed) {
        match SshRelaySpec::parse(trimmed) {
            Ok(spec) => spec.redacted_display(),
            Err(_) => "ssh+relay://<redacted>".to_string(),
        }
    } else {
        trimmed.to_string()
    }
}

/// Generated Ed25519 tunnel identity keypair for `ssh+relay://`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedTunnelKey {
    pub seed_b64: String,
    pub openssh_pubkey: String,
}

/// Generate a fresh Ed25519 tunnel keypair using the OS CSPRNG.
pub fn generate_ssh_tunnel_key() -> Result<GeneratedTunnelKey, RelayTransportError> {
    let mut rng = safe_rng();
    let ed_priv = Ed25519PrivateKey::random(&mut rng);
    let seed_b64 = URL_SAFE_NO_PAD.encode(ed_priv.to_bytes());
    let openssh_pubkey = derive_openssh_pubkey_from_seed(&seed_b64)?;
    Ok(GeneratedTunnelKey {
        seed_b64,
        openssh_pubkey,
    })
}

/// Derive the OpenSSH `ssh-ed25519 ...` public key line from a base64url 32-byte seed.
pub fn derive_openssh_pubkey_from_seed(seed_b64: &str) -> Result<String, RelayTransportError> {
    let seed = decode_32_bytes_flexible(seed_b64)?;
    let priv_key = private_key_from_seed(&seed)?;
    Ok(priv_key.public_key().to_openssh()?)
}

/// Compute the SHA-256 fingerprint (`URL_SAFE_NO_PAD` base64) of an OpenSSH public key line.
pub fn compute_openssh_pubkey_sha256_b64(
    openssh_pub_line: &str,
) -> Result<String, RelayTransportError> {
    let key_b64 = openssh_pub_line.split_whitespace().nth(1).ok_or_else(|| {
        RelayTransportError::InvalidKey("malformed OpenSSH public key line".into())
    })?;
    let blob = STANDARD
        .decode(key_b64)
        .or_else(|_| STANDARD_NO_PAD.decode(key_b64))
        .map_err(|e| RelayTransportError::InvalidKey(format!("invalid public key base64: {e}")))?;
    let digest = Sha256::digest(&blob);
    Ok(URL_SAFE_NO_PAD.encode(digest))
}

/// Build an OpenSSH `authorized_keys` line restricted strictly to `127.0.0.1:<relay_port>` port forwarding.
pub fn format_restricted_authorized_key_line(
    openssh_pubkey: &str,
    relay_port: u16,
    tag: &str,
) -> String {
    let pub_clean = openssh_pubkey
        .split_whitespace()
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    let tag_clean = tag.split_whitespace().collect::<Vec<_>>().join("_");
    format!(
        r#"restrict,port-forwarding,permitopen="127.0.0.1:{relay_port}",command="exit 1" {pub_clean} {tag_clean}"#
    )
}

/// Persistent SSH session backing multiplexed `direct-tcpip` channels to `127.0.0.1:<relay_port>`.
pub struct SshRelaySession {
    handle: AsyncMutex<client::Handle<OrbitSshHandler>>,
    relay_port: u16,
}

impl SshRelaySession {
    async fn connect(spec: &SshRelaySpec) -> Result<Arc<Self>, RelayTransportError> {
        let seed = spec.key_seed_bytes()?;
        let expected_fps = spec.decoded_host_key_fps()?;
        let priv_key = private_key_from_seed(&seed)?;

        let config = Arc::new(client::Config {
            inactivity_timeout: None,
            keepalive_interval: Some(Duration::from_secs(15)),
            keepalive_max: 3,
            nodelay: true,
            ..Default::default()
        });
        let handler = OrbitSshHandler { expected_fps };
        let target = (spec.ssh_host.as_str(), spec.ssh_port);

        let mut handle = tokio::time::timeout(
            SSH_CONNECT_TIMEOUT,
            client::connect(config, target, handler),
        )
        .await
        .map_err(|_| RelayTransportError::Timeout)??;

        let auth_res = tokio::time::timeout(
            SSH_CONNECT_TIMEOUT,
            handle.authenticate_publickey(
                spec.user.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(priv_key), None),
            ),
        )
        .await
        .map_err(|_| RelayTransportError::Timeout)??;

        if !matches!(auth_res, AuthResult::Success) {
            return Err(RelayTransportError::SshAuthFailed);
        }

        Ok(Arc::new(Self {
            handle: AsyncMutex::new(handle),
            relay_port: spec.relay_port,
        }))
    }

    async fn open_channel(self: &Arc<Self>) -> Result<SshChannelStreamGuard, RelayTransportError> {
        let channel = {
            let guard = self.handle.lock().await;
            tokio::time::timeout(
                SSH_CONNECT_TIMEOUT,
                guard.channel_open_direct_tcpip(
                    "127.0.0.1",
                    u32::from(self.relay_port),
                    "127.0.0.1",
                    0,
                ),
            )
            .await
            .map_err(|_| RelayTransportError::Timeout)??
        };
        Ok(SshChannelStreamGuard {
            stream: channel.into_stream(),
            _session: Arc::clone(self),
        })
    }
}

struct OrbitSshHandler {
    expected_fps: Vec<[u8; 32]>,
}

impl Handler for OrbitSshHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let accepted = if self.expected_fps.is_empty() {
            true
        } else {
            let pk = server_public_key.public_key();
            pk.fingerprint(HashAlg::Sha256)
                .sha256()
                .is_some_and(|digest| self.expected_fps.iter().any(|fp| fp == &digest))
        };
        async move { Ok(accepted) }
    }
}

/// Async stream over an SSH `direct-tcpip` channel that keeps the parent `SshRelaySession` alive.
pub struct SshChannelStreamGuard {
    stream: ChannelStream<client::Msg>,
    _session: Arc<SshRelaySession>,
}

impl AsyncRead for SshChannelStreamGuard {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}

impl AsyncWrite for SshChannelStreamGuard {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}

/// Unified WebSocket stream over either direct TCP/TLS or an SSH `direct-tcpip` tunnel.
pub enum RelayWsStream {
    Direct(Box<WebSocketStream<MaybeTlsStream<TcpStream>>>),
    Ssh(Box<WebSocketStream<SshChannelStreamGuard>>),
}

impl Stream for RelayWsStream {
    type Item = Result<Message, WsError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.get_mut() {
            Self::Direct(inner) => Pin::new(inner).poll_next(cx),
            Self::Ssh(inner) => Pin::new(inner).poll_next(cx),
        }
    }
}

impl Sink<Message> for RelayWsStream {
    type Error = WsError;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Direct(inner) => Pin::new(inner).poll_ready(cx),
            Self::Ssh(inner) => Pin::new(inner).poll_ready(cx),
        }
    }

    fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        match self.get_mut() {
            Self::Direct(inner) => Pin::new(inner).start_send(item),
            Self::Ssh(inner) => Pin::new(inner).start_send(item),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Direct(inner) => Pin::new(inner).poll_flush(cx),
            Self::Ssh(inner) => Pin::new(inner).poll_flush(cx),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        match self.get_mut() {
            Self::Direct(inner) => Pin::new(inner).poll_close(cx),
            Self::Ssh(inner) => Pin::new(inner).poll_close(cx),
        }
    }
}

/// Multiplexed Relay connector (holds a single persistent SSH session when using `ssh+relay://`).
#[derive(Clone)]
pub enum RelayConnector {
    Direct {
        base_url: String,
    },
    Ssh {
        relay_port: u16,
        session: Arc<SshRelaySession>,
    },
}

impl RelayConnector {
    /// Establish a reusable Relay connector for `relay_base` (`ws://`, `wss://`, or `ssh+relay://`).
    pub async fn connect(relay_base: &str) -> Result<Self, RelayTransportError> {
        match parse_relay_endpoint(relay_base)? {
            RelayEndpoint::DirectWs(base_url) => Ok(Self::Direct { base_url }),
            RelayEndpoint::SshTunnel(spec) => {
                let session = SshRelaySession::connect(&spec).await?;
                Ok(Self::Ssh {
                    relay_port: spec.relay_port,
                    session,
                })
            }
        }
    }

    /// Open a WebSocket stream to `path_and_query` (for example `/v1/daemon/control?pubkey=...`).
    pub async fn open_ws(
        &self,
        path_and_query: &str,
    ) -> Result<RelayWsStream, RelayTransportError> {
        let suffix = if path_and_query.starts_with('/') {
            path_and_query.to_string()
        } else {
            format!("/{path_and_query}")
        };

        match self {
            Self::Direct { base_url } => {
                let url = format!("{}{}", base_url.trim_end_matches('/'), suffix);
                let (ws, _) =
                    connect_async_with_config(&url, Some(relay_ws_config()), false).await?;
                Ok(RelayWsStream::Direct(Box::new(ws)))
            }
            Self::Ssh {
                relay_port,
                session,
            } => {
                let guard = session.open_channel().await?;
                let ws_url = format!("ws://127.0.0.1:{relay_port}{suffix}");
                let (ws, _) =
                    client_async_with_config(&ws_url, guard, Some(relay_ws_config())).await?;
                Ok(RelayWsStream::Ssh(Box::new(ws)))
            }
        }
    }
}

/// One-shot convenience helper to connect a WebSocket directly or through an SSH Relay tunnel.
pub async fn connect_relay_ws(
    relay_base: &str,
    path_and_query: &str,
) -> Result<RelayWsStream, RelayTransportError> {
    let connector = RelayConnector::connect(relay_base).await?;
    connector.open_ws(path_and_query).await
}

fn private_key_from_seed(seed: &[u8; 32]) -> Result<PrivateKey, RelayTransportError> {
    let kp = Ed25519Keypair::from_seed(seed);
    let key_data = KeypairData::from(kp);
    Ok(PrivateKey::new(key_data, "")?)
}

fn strip_scheme_ci<'a>(input: &'a str, scheme: &str) -> Option<&'a str> {
    if input.len() < scheme.len() {
        return None;
    }
    let prefix = input.get(..scheme.len())?;
    if prefix.eq_ignore_ascii_case(scheme) {
        Some(&input[scheme.len()..])
    } else {
        None
    }
}

fn parse_host_port(raw: &str, default_port: u16) -> Result<(String, u16), RelayTransportError> {
    if raw.is_empty() {
        return Err(RelayTransportError::InvalidUrl("missing SSH host".into()));
    }
    if let Some(rest) = raw.strip_prefix('[') {
        let (host, after_bracket) = rest
            .split_once(']')
            .ok_or_else(|| RelayTransportError::InvalidUrl(format!("unclosed IPv6 host: {raw}")))?;
        if host.is_empty() {
            return Err(RelayTransportError::InvalidUrl("empty IPv6 host".into()));
        }
        let port = if after_bracket.is_empty() {
            default_port
        } else if let Some(port_str) = after_bracket.strip_prefix(':') {
            parse_nonzero_port(port_str)?
        } else {
            return Err(RelayTransportError::InvalidUrl(format!(
                "invalid host:port: {raw}"
            )));
        };
        return Ok((host.to_string(), port));
    }

    match raw.rsplit_once(':') {
        Some((host, port_str)) if !host.contains(':') => {
            if host.is_empty() || host.chars().any(char::is_whitespace) {
                return Err(RelayTransportError::InvalidUrl(format!(
                    "invalid SSH host: {raw}"
                )));
            }
            Ok((host.to_string(), parse_nonzero_port(port_str)?))
        }
        _ => {
            if raw.chars().any(char::is_whitespace) {
                return Err(RelayTransportError::InvalidUrl(format!(
                    "invalid SSH host: {raw}"
                )));
            }
            Ok((raw.to_string(), default_port))
        }
    }
}

fn parse_nonzero_port(raw: &str) -> Result<u16, RelayTransportError> {
    let port = raw
        .parse::<u16>()
        .map_err(|_| RelayTransportError::InvalidUrl(format!("invalid port: {raw}")))?;
    if port == 0 {
        return Err(RelayTransportError::InvalidUrl("port cannot be 0".into()));
    }
    Ok(port)
}

fn format_host_for_uri(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    }
}

fn decode_32_bytes_flexible(raw: &str) -> Result<[u8; 32], RelayTransportError> {
    let clean = raw.trim();
    let bytes = URL_SAFE_NO_PAD
        .decode(clean)
        .or_else(|_| URL_SAFE.decode(clean))
        .or_else(|_| STANDARD_NO_PAD.decode(clean))
        .or_else(|_| STANDARD.decode(clean))
        .map_err(|e| RelayTransportError::InvalidKey(format!("base64 decode failed: {e}")))?;
    bytes.try_into().map_err(|v: Vec<u8>| {
        RelayTransportError::InvalidKey(format!("expected 32 bytes, got {}", v.len()))
    })
}

fn normalize_sha256_fp(raw: &str) -> Result<String, RelayTransportError> {
    let trimmed = raw.trim();
    let without_prefix = if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("SHA256:") {
        &trimmed[7..]
    } else {
        trimmed
    };
    let bytes = decode_32_bytes_flexible(without_prefix)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_relay_uri_roundtrip_and_defaults() {
        let key = generate_ssh_tunnel_key().expect("keygen");
        let fp = compute_openssh_pubkey_sha256_b64(&key.openssh_pubkey).expect("fp");
        let uri = format!(
            "ssh+relay://ec2-user@13.228.27.154/?key={}&fp=SHA256:{}",
            key.seed_b64, fp
        );
        let spec = SshRelaySpec::parse(&uri).expect("parse");
        assert_eq!(spec.user, "ec2-user");
        assert_eq!(spec.ssh_host, "13.228.27.154");
        assert_eq!(spec.ssh_port, 22);
        assert_eq!(spec.relay_port, 7878);
        assert_eq!(spec.key_seed_b64, key.seed_b64);
        assert_eq!(spec.host_key_fps_b64, vec![fp.clone()]);

        let canonical = spec.to_uri();
        assert_eq!(
            canonical,
            format!(
                "ssh+relay://ec2-user@13.228.27.154:22/7878?key={}&fp={}",
                key.seed_b64, fp
            )
        );
        assert_eq!(normalize_relay_url(&uri).expect("normalize"), canonical);
    }

    #[test]
    fn ssh_relay_ipv6_and_custom_ports() {
        let key = generate_ssh_tunnel_key().expect("keygen");
        let uri = format!(
            "ssh+relay://admin@[2001:db8::1]:2222/9000?key={}",
            key.seed_b64
        );
        let spec = SshRelaySpec::parse(&uri).expect("parse ipv6");
        assert_eq!(spec.ssh_host, "2001:db8::1");
        assert_eq!(spec.ssh_port, 2222);
        assert_eq!(spec.relay_port, 9000);
        assert_eq!(spec.to_uri(), uri);
    }

    #[test]
    fn ssh_relay_rejects_missing_user_or_key() {
        let key = generate_ssh_tunnel_key().expect("keygen");
        assert!(
            SshRelaySpec::parse(&format!("ssh+relay://1.2.3.4:22/7878?key={}", key.seed_b64))
                .is_err()
        );
        assert!(SshRelaySpec::parse("ssh+relay://user@1.2.3.4:22/7878").is_err());
        assert!(SshRelaySpec::parse("ssh+relay://user@1.2.3.4:22/7878?key=short").is_err());
        assert!(SshRelaySpec::parse("ssh+relay://user@1.2.3.4:0/7878?key=abc").is_err());
    }

    #[test]
    fn redaction_never_leaks_tunnel_key() {
        let key = generate_ssh_tunnel_key().expect("keygen");
        let uri = format!(
            "ssh+relay://ec2-user@13.228.27.154:22/7878?key={}&fp={}",
            key.seed_b64, key.seed_b64
        );
        let redacted = redact_relay_url(&uri);
        assert!(!redacted.contains(&key.seed_b64));
        assert!(!redacted.contains("key="));
        assert_eq!(
            redacted,
            "ssh+relay://ec2-user@13.228.27.154:22/7878 (SSH E2EE Tunnel)"
        );

        let broken_redacted = redact_relay_url("ssh+relay://broken?key=secret123");
        assert!(!broken_redacted.contains("secret123"));
        assert_eq!(broken_redacted, "ssh+relay://<redacted>");
    }

    #[test]
    fn tunnel_key_seed_derivation_and_authorized_keys_line() {
        let key = generate_ssh_tunnel_key().expect("keygen");
        let rederived = derive_openssh_pubkey_from_seed(&key.seed_b64).expect("rederive");
        assert_eq!(key.openssh_pubkey, rederived);
        assert!(key.openssh_pubkey.starts_with("ssh-ed25519 "));

        let line = format_restricted_authorized_key_line(
            &key.openssh_pubkey,
            7878,
            "orbit-relay-tunnel:host_abc:7878",
        );
        assert!(line.starts_with(
            r#"restrict,port-forwarding,permitopen="127.0.0.1:7878",command="exit 1" ssh-ed25519 "#
        ));
        assert!(line.ends_with(" orbit-relay-tunnel:host_abc:7878"));

        let fp = compute_openssh_pubkey_sha256_b64(&key.openssh_pubkey).expect("fp");
        let parsed_pk = russh::keys::ssh_key::PublicKey::from_openssh(&key.openssh_pubkey)
            .expect("ssh-key parse");
        let expected_fp = URL_SAFE_NO_PAD.encode(
            parsed_pk
                .fingerprint(HashAlg::Sha256)
                .sha256()
                .expect("sha256"),
        );
        assert_eq!(fp, expected_fp);
    }

    #[test]
    fn normalize_direct_ws_variants_unchanged() {
        assert_eq!(
            normalize_relay_url("203.0.113.10:7878").ok().as_deref(),
            Some("ws://203.0.113.10:7878")
        );
        assert_eq!(
            normalize_relay_url("203.0.113.10").ok().as_deref(),
            Some("ws://203.0.113.10:7878")
        );
        assert_eq!(
            normalize_relay_url("https://relay.example.com/v1/daemon/control")
                .ok()
                .as_deref(),
            Some("wss://relay.example.com")
        );
    }
}
