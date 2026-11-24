//! Pluggable WebSocket transport for Orbit Relay.
//!
//! Supports both direct `ws://` / `wss://` Relay endpoints and zero-port `ssh+relay://`
//! endpoints backed by pure-Rust `russh` (`direct-tcpip` over SSH port 22).

use std::future::Future;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use futures_util::{Sink, Stream};
use hmac::{Hmac, KeyInit, Mac};
use russh::ChannelStream;
use russh::client::{self, AuthResult, Handler};
use russh::keys::ssh_key::{
    HashAlg, PrivateKey,
    private::{Ed25519Keypair, Ed25519PrivateKey, KeypairData},
};
use russh::keys::{PrivateKeyWithHashAlg, PublicKeyOrCertificate, key::safe_rng};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Mutex as AsyncMutex;
use tokio_tungstenite::tungstenite::{
    Error as WsError, Message, client::IntoClientRequest, protocol::WebSocketConfig,
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, client_async_with_config, connect_async_with_config,
};

const DEFAULT_SSH_PORT: u16 = 22;
const DEFAULT_RELAY_PORT: u16 = 7878;
const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Max WebSocket frame/message size (48 MiB) so 25 MiB base64/E2EE payloads never drop sessions.
pub const MAX_RELAY_TRANSPORT_WS_FRAME_BYTES: usize = 48 * 1024 * 1024;
/// Tight WebSocket frame/message cap (64 KiB) for Relay control sockets (`/v1/daemon/control`).
pub const MAX_RELAY_CONTROL_WS_FRAME_BYTES: usize = 64 * 1024;

fn relay_ws_config(max_frame_bytes: usize) -> WebSocketConfig {
    let mut cfg = WebSocketConfig::default();
    cfg.max_frame_size = Some(max_frame_bytes);
    cfg.max_message_size = Some(max_frame_bytes);
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
    #[error(
        "SSH host key mismatch for {host}:{port} (possible MITM attack): {detail} (observed server fingerprint: {observed_fp})"
    )]
    HostKeyMismatch {
        host: String,
        port: u16,
        observed_fp: String,
        detail: String,
    },
    #[error(
        "untrusted SSH host key for {host}:{port} (observed fingerprint: {observed_fp}): not found in {known_hosts_path} and no `&fp=` pin provided. Verify the host key out-of-band, then either run `{ssh_hint}` / add it to ~/.ssh/known_hosts or pin `&fp={observed_fp}` in the ssh+relay:// URL"
    )]
    HostKeyUnknown {
        host: String,
        port: u16,
        observed_fp: String,
        known_hosts_path: String,
        ssh_hint: String,
    },
    #[error("SSH host key verification failed: {0}")]
    HostKeyVerification(String),
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

/// Parsed `ssh+relay://<user>@<ssh_host>:<ssh_port>/<relay_port>?key=<seed_b64>[&fp=<fps>][&ft=<token>]` specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshRelaySpec {
    pub user: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub relay_port: u16,
    pub key_seed_b64: String,
    pub host_key_fps_b64: Vec<String>,
    /// Optional shared fleet admission token (`&ft=...`). Never logged or displayed verbatim.
    pub fleet_token: Option<String>,
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
        let mut fleet_token: Option<String> = None;

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
                    "ft" | "token" => {
                        let tok = v.trim();
                        if tok.is_empty() || tok.chars().any(char::is_whitespace) {
                            return Err(RelayTransportError::InvalidUrl(
                                "invalid `ft` fleet token parameter in ssh+relay:// URL".into(),
                            ));
                        }
                        fleet_token = Some(tok.to_string());
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
            fleet_token,
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
        if let Some(ft) = self
            .fleet_token
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            uri.push_str("&ft=");
            uri.push_str(ft);
        }
        uri
    }

    /// Redacted human-readable representation that never exposes `?key=` or `&ft=`.
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

/// Split a normalized or raw direct `ws://`/`wss://` Relay URL into `(base_url, fleet_token)`.
pub fn split_direct_relay_url(raw: &str) -> Result<(String, Option<String>), RelayTransportError> {
    let normalized = normalize_relay_url(raw)?;
    if is_ssh_relay_url(&normalized) {
        return Err(RelayTransportError::InvalidUrl(
            "expected direct ws(s):// relay URL, got ssh+relay://".into(),
        ));
    }
    Ok(match normalized.split_once('?') {
        Some((base, query)) => {
            let mut token = None;
            for pair in query.split('&').filter(|s| !s.is_empty()) {
                if let Some((k, v)) = pair.split_once('=')
                    && matches!(k, "ft" | "token")
                    && !v.trim().is_empty()
                {
                    token = Some(v.trim().to_string());
                }
            }
            (base.to_string(), token)
        }
        None => (normalized, None),
    })
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
/// Preserves optional `?ft=<fleet_token>` admission tokens on direct WebSocket URLs.
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

    let (before_query_raw, query_opt) = match trimmed.split_once('?') {
        Some((left, q)) => (left, Some(q)),
        None => (trimmed, None),
    };
    let mut fleet_token: Option<String> = None;
    if let Some(query) = query_opt {
        for pair in query.split('&').filter(|s| !s.is_empty()) {
            if let Some((k, v)) = pair.split_once('=')
                && matches!(k, "ft" | "token")
            {
                let tok = v.trim();
                if tok.is_empty() || tok.chars().any(char::is_whitespace) {
                    return Err(RelayTransportError::InvalidUrl(
                        "invalid fleet token query parameter in relay URL".into(),
                    ));
                }
                fleet_token = Some(tok.to_string());
            }
        }
    }

    let before_query = before_query_raw.trim_end_matches('/');
    let had_scheme = before_query.contains("://");
    let with_scheme = if let Some(rest) = strip_scheme_ci(before_query, "http://") {
        format!("ws://{rest}")
    } else if let Some(rest) = strip_scheme_ci(before_query, "https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = strip_scheme_ci(before_query, "ws://") {
        format!("ws://{rest}")
    } else if let Some(rest) = strip_scheme_ci(before_query, "wss://") {
        format!("wss://{rest}")
    } else {
        format!("ws://{before_query}")
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

    let mut out = format!("{scheme}://{final_authority}");
    if let Some(ft) = fleet_token {
        out.push_str("?ft=");
        out.push_str(&ft);
    }
    Ok(out)
}

/// Return a log/UI-safe Relay URL string that masks any `ssh+relay://` tunnel secret or `?ft=` token.
pub fn redact_relay_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if is_ssh_relay_url(trimmed) {
        match SshRelaySpec::parse(trimmed) {
            Ok(spec) => spec.redacted_display(),
            Err(_) => "ssh+relay://<redacted>".to_string(),
        }
    } else if let Some((base, query)) = trimmed.split_once('?') {
        if query
            .split('&')
            .any(|p| p.starts_with("ft=") || p.starts_with("token="))
        {
            format!("{base}?ft=<redacted>")
        } else {
            trimmed.to_string()
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
        let handler = OrbitSshHandler {
            ssh_host: spec.ssh_host.clone(),
            ssh_port: spec.ssh_port,
            expected_fps,
            known_hosts_path: None,
        };
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
    ssh_host: String,
    ssh_port: u16,
    expected_fps: Vec<[u8; 32]>,
    known_hosts_path: Option<PathBuf>,
}

impl OrbitSshHandler {
    fn verify_server_key(
        &self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<(), RelayTransportError> {
        let pk = server_public_key.public_key();
        let digest = pk.fingerprint(HashAlg::Sha256).sha256().ok_or_else(|| {
            RelayTransportError::HostKeyVerification(format!(
                "failed to compute SHA-256 fingerprint for {}:{}",
                self.ssh_host, self.ssh_port
            ))
        })?;
        let observed_fp = format_openssh_sha256_fingerprint(&digest);

        if !self.expected_fps.is_empty() {
            let matched = self.expected_fps.iter().any(|fp| fp == &digest);
            if matched {
                return Ok(());
            }
            return Err(RelayTransportError::HostKeyMismatch {
                host: self.ssh_host.clone(),
                port: self.ssh_port,
                observed_fp,
                detail: "server host key does not match any pinned `&fp=` fingerprint".to_string(),
            });
        }

        let known_hosts_path = match &self.known_hosts_path {
            Some(path) => path.clone(),
            None => resolve_default_known_hosts_path().ok_or_else(|| {
                RelayTransportError::HostKeyUnknown {
                    host: self.ssh_host.clone(),
                    port: self.ssh_port,
                    observed_fp: observed_fp.clone(),
                    known_hosts_path: "~/.ssh/known_hosts (home directory unavailable)".to_string(),
                    ssh_hint: format_ssh_trust_hint(&self.ssh_host, self.ssh_port),
                }
            })?,
        };

        verify_against_known_hosts_file(
            &self.ssh_host,
            self.ssh_port,
            &pk,
            &digest,
            &known_hosts_path,
            &observed_fp,
        )
    }
}

impl Handler for OrbitSshHandler {
    type Error = RelayTransportError;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let result = self.verify_server_key(server_public_key).map(|()| true);
        async move { result }
    }
}

fn resolve_default_known_hosts_path() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(|home| PathBuf::from(home).join(".ssh").join("known_hosts"))
}

fn format_ssh_trust_hint(host: &str, port: u16) -> String {
    if port == DEFAULT_SSH_PORT {
        format!("ssh {host}")
    } else {
        format!("ssh -p {port} {host}")
    }
}

/// Pure-Rust OpenSSH `known_hosts` verification with strict `@revoked` precedence, multi-key
/// rotation support, space/tab field parsing, HMAC-SHA1 hashed (`|1|...`) host matching, and
/// explicit propagation of non-`NotFound` I/O errors.
fn verify_against_known_hosts_file(
    host: &str,
    port: u16,
    pubkey: &russh::keys::ssh_key::PublicKey,
    server_sha256: &[u8; 32],
    known_hosts_path: &Path,
    observed_fp: &str,
) -> Result<(), RelayTransportError> {
    use std::io::BufRead;

    let file = match std::fs::File::open(known_hosts_path) {
        Ok(f) => f,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(RelayTransportError::HostKeyUnknown {
                host: host.to_string(),
                port,
                observed_fp: observed_fp.to_string(),
                known_hosts_path: known_hosts_path.display().to_string(),
                ssh_hint: format_ssh_trust_hint(host, port),
            });
        }
        Err(err) => {
            return Err(RelayTransportError::HostKeyVerification(format!(
                "failed to read {} while verifying {}:{} (observed fingerprint: {}): {err}",
                known_hosts_path.display(),
                host,
                port,
                observed_fp
            )));
        }
    };

    let mut reader = std::io::BufReader::new(file);
    let mut line_buf = String::new();
    let mut line_no: usize = 0;

    let mut revoked_line: Option<usize> = None;
    let mut matched_valid = false;
    let mut first_mismatch_line: Option<usize> = None;

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf).map_err(|err| {
            RelayTransportError::HostKeyVerification(format!(
                "I/O error reading {} at line {} while verifying {}:{} (observed fingerprint: {}): {err}",
                known_hosts_path.display(),
                line_no + 1,
                host,
                port,
                observed_fp
            ))
        })?;
        if bytes_read == 0 {
            break;
        }
        line_no += 1;

        let trimmed = line_buf.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let mut tokens = trimmed.split_whitespace();
        let Some(first) = tokens.next() else {
            continue;
        };

        let (is_revoked, host_patterns, key_type, key_b64) = if first == "@revoked" {
            let (Some(hp), Some(kt), Some(kb)) = (tokens.next(), tokens.next(), tokens.next())
            else {
                continue;
            };
            (true, hp, kt, kb)
        } else if first == "@cert-authority" {
            // Certificate authority entries are not direct host keys.
            continue;
        } else if first.starts_with('@') {
            // Unknown OpenSSH marker: ignore safely.
            continue;
        } else {
            let (Some(kt), Some(kb)) = (tokens.next(), tokens.next()) else {
                continue;
            };
            (false, first, kt, kb)
        };

        if !known_hosts_pattern_list_matches(host_patterns, host, port) {
            continue;
        }

        let Some(key_matches) =
            verify_known_hosts_entry_key(is_revoked, key_type, key_b64, pubkey, server_sha256)
        else {
            continue;
        };
        if is_revoked {
            if key_matches && revoked_line.is_none() {
                revoked_line = Some(line_no);
            }
        } else if key_matches {
            matched_valid = true;
        } else if first_mismatch_line.is_none() {
            first_mismatch_line = Some(line_no);
        }
    }

    if let Some(line) = revoked_line {
        return Err(RelayTransportError::HostKeyMismatch {
            host: host.to_string(),
            port,
            observed_fp: observed_fp.to_string(),
            detail: format!(
                "server host key is marked @revoked at {}:{}",
                known_hosts_path.display(),
                line
            ),
        });
    }

    if matched_valid {
        return Ok(());
    }

    if let Some(line) = first_mismatch_line {
        return Err(RelayTransportError::HostKeyMismatch {
            host: host.to_string(),
            port,
            observed_fp: observed_fp.to_string(),
            detail: format!(
                "remote host identification has changed compared to {}:{}",
                known_hosts_path.display(),
                line
            ),
        });
    }

    Err(RelayTransportError::HostKeyUnknown {
        host: host.to_string(),
        port,
        observed_fp: observed_fp.to_string(),
        known_hosts_path: known_hosts_path.display().to_string(),
        ssh_hint: format_ssh_trust_hint(host, port),
    })
}

const OPENSSH_HASHED_HOST_SHA1_LEN: usize = 20;

fn format_openssh_sha256_fingerprint(digest: &[u8; 32]) -> String {
    format!("SHA256:{}", STANDARD_NO_PAD.encode(digest))
}

fn verify_known_hosts_entry_key(
    is_revoked: bool,
    key_type: &str,
    key_b64: &str,
    server_pubkey: &russh::keys::ssh_key::PublicKey,
    server_sha256: &[u8; 32],
) -> Option<bool> {
    let openssh_line = format!("{key_type} {key_b64}");
    if let Ok(recorded) = russh::keys::ssh_key::PublicKey::from_openssh(&openssh_line)
        && recorded.algorithm().as_str() == key_type
    {
        return Some(&recorded == server_pubkey);
    }
    if is_revoked {
        // Fail closed for `@revoked` entries with exotic/unparsed key types (e.g. `sk-*`)
        // by comparing the raw RFC 4253 wire key blob SHA-256. Never used for positive trust.
        let revoked_blob_matches = STANDARD
            .decode(key_b64)
            .or_else(|_| STANDARD_NO_PAD.decode(key_b64))
            .ok()
            .is_some_and(|blob| Sha256::digest(&blob).as_slice() == server_sha256);
        return Some(revoked_blob_matches);
    }
    // Unparseable or key-type-mismatched ordinary entries are ignored cleanly and never trusted.
    None
}

fn canonical_known_hosts_lookup_name(host: &str, port: u16) -> String {
    let clean_host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if port == DEFAULT_SSH_PORT {
        clean_host
    } else {
        format!("[{clean_host}]:{port}")
    }
}

fn known_hosts_pattern_list_matches(pattern_list: &str, host: &str, port: u16) -> bool {
    let lookup_name = canonical_known_hosts_lookup_name(host, port);

    let mut positive_match = false;
    for raw_entry in pattern_list.split(',').filter(|s| !s.is_empty()) {
        let (negated, pattern) = match raw_entry.strip_prefix('!') {
            Some(p) => (true, p),
            None => (false, raw_entry),
        };
        if pattern.is_empty() {
            continue;
        }
        let entry_matches = if pattern.starts_with("|1|") {
            hashed_known_host_matches(pattern, &lookup_name)
        } else {
            wildcard_match(&pattern.to_ascii_lowercase(), &lookup_name)
        };
        if entry_matches {
            if negated {
                return false;
            }
            positive_match = true;
        }
    }
    positive_match
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let t = text.as_bytes();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_idx: Option<usize> = None;
    let mut match_ti = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star_idx = Some(pi);
            pi += 1;
            match_ti = ti;
        } else if let Some(s) = star_idx {
            pi = s + 1;
            match_ti += 1;
            ti = match_ti;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

fn hashed_known_host_matches(pattern: &str, candidate_host: &str) -> bool {
    let Some(rest) = pattern.strip_prefix("|1|") else {
        return false;
    };
    let Some((salt_b64, hash_b64)) = rest.split_once('|') else {
        return false;
    };
    if hash_b64.contains('|') {
        return false;
    }
    let Ok(salt) = STANDARD
        .decode(salt_b64)
        .or_else(|_| STANDARD_NO_PAD.decode(salt_b64))
    else {
        return false;
    };
    let Ok(expected) = STANDARD
        .decode(hash_b64)
        .or_else(|_| STANDARD_NO_PAD.decode(hash_b64))
    else {
        return false;
    };
    if salt.len() != OPENSSH_HASHED_HOST_SHA1_LEN || expected.len() != OPENSSH_HASHED_HOST_SHA1_LEN
    {
        return false;
    }
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.update(candidate_host.as_bytes());
    mac.verify_slice(&expected).is_ok()
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
        fleet_token: Option<String>,
    },
    Ssh {
        relay_port: u16,
        fleet_token: Option<String>,
        session: Arc<SshRelaySession>,
    },
}

impl RelayConnector {
    /// Establish a reusable Relay connector for `relay_base` (`ws://`, `wss://`, or `ssh+relay://`).
    pub async fn connect(relay_base: &str) -> Result<Self, RelayTransportError> {
        match parse_relay_endpoint(relay_base)? {
            RelayEndpoint::DirectWs(raw_base) => {
                let (base_url, fleet_token) = split_direct_relay_url(&raw_base)?;
                Ok(Self::Direct {
                    base_url,
                    fleet_token,
                })
            }
            RelayEndpoint::SshTunnel(spec) => {
                let session = SshRelaySession::connect(&spec).await?;
                Ok(Self::Ssh {
                    relay_port: spec.relay_port,
                    fleet_token: spec.fleet_token,
                    session,
                })
            }
        }
    }

    /// Open a WebSocket stream to `path_and_query` with the default 48 MiB frame/message cap.
    pub async fn open_ws(
        &self,
        path_and_query: &str,
    ) -> Result<RelayWsStream, RelayTransportError> {
        self.open_ws_with_limit(path_and_query, MAX_RELAY_TRANSPORT_WS_FRAME_BYTES)
            .await
    }

    /// Open a WebSocket stream to `path_and_query` with an explicit frame/message size cap.
    pub async fn open_ws_with_limit(
        &self,
        path_and_query: &str,
        max_frame_bytes: usize,
    ) -> Result<RelayWsStream, RelayTransportError> {
        let suffix = if path_and_query.starts_with('/') {
            path_and_query.to_string()
        } else {
            format!("/{path_and_query}")
        };
        let ws_cfg = relay_ws_config(max_frame_bytes);

        match self {
            Self::Direct {
                base_url,
                fleet_token,
            } => {
                let url = format!("{}{}", base_url.trim_end_matches('/'), suffix);
                let mut req = url.as_str().into_client_request()?;
                if let Some(tok) = fleet_token.as_deref() {
                    let hdr = format!("Bearer {tok}").parse().map_err(|_| {
                        RelayTransportError::InvalidUrl("invalid fleet token header".into())
                    })?;
                    req.headers_mut().insert("Authorization", hdr);
                }
                let (ws, _) = connect_async_with_config(req, Some(ws_cfg), false).await?;
                Ok(RelayWsStream::Direct(Box::new(ws)))
            }
            Self::Ssh {
                relay_port,
                fleet_token,
                session,
            } => {
                let guard = session.open_channel().await?;
                let ws_url = format!("ws://127.0.0.1:{relay_port}{suffix}");
                let mut req = ws_url.as_str().into_client_request()?;
                if let Some(tok) = fleet_token.as_deref() {
                    let hdr = format!("Bearer {tok}").parse().map_err(|_| {
                        RelayTransportError::InvalidUrl("invalid fleet token header".into())
                    })?;
                    req.headers_mut().insert("Authorization", hdr);
                }
                let (ws, _) = client_async_with_config(req, guard, Some(ws_cfg)).await?;
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
    connect_relay_ws_with_limit(
        relay_base,
        path_and_query,
        MAX_RELAY_TRANSPORT_WS_FRAME_BYTES,
    )
    .await
}

/// One-shot convenience helper to connect a WebSocket with an explicit frame/message size cap.
pub async fn connect_relay_ws_with_limit(
    relay_base: &str,
    path_and_query: &str,
    max_frame_bytes: usize,
) -> Result<RelayWsStream, RelayTransportError> {
    let connector = RelayConnector::connect(relay_base).await?;
    connector
        .open_ws_with_limit(path_and_query, max_frame_bytes)
        .await
}

/// Perform a lightweight read-only HTTP/1.1 GET request through an `ssh+relay://` tunnel
/// (for example `/v1/daemon/probe?pubkey=...`), returning `(status_code, body)`.
pub async fn http_get_over_ssh_relay(
    spec: &SshRelaySpec,
    path_and_query: &str,
) -> Result<(u16, String), RelayTransportError> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let suffix = if path_and_query.starts_with('/') {
        path_and_query.to_string()
    } else {
        format!("/{path_and_query}")
    };
    let session = SshRelaySession::connect(spec).await?;
    let mut channel = session.open_channel().await?;
    let auth_header = spec
        .fleet_token
        .as_deref()
        .map(|tok| format!("Authorization: Bearer {tok}\r\n"))
        .unwrap_or_default();
    let req = format!(
        "GET {suffix} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n{auth_header}Connection: close\r\n\r\n",
        spec.relay_port
    );
    channel.write_all(req.as_bytes()).await.map_err(|e| {
        RelayTransportError::InvalidUrl(format!("SSH HTTP probe write failed: {e}"))
    })?;
    channel.flush().await.map_err(|e| {
        RelayTransportError::InvalidUrl(format!("SSH HTTP probe flush failed: {e}"))
    })?;

    let mut resp = Vec::with_capacity(512);
    let mut chunk = [0u8; 512];
    const MAX_PROBE_RESPONSE_BYTES: usize = 4096;
    while resp.len() < MAX_PROBE_RESPONSE_BYTES {
        match channel.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                let take = n.min(MAX_PROBE_RESPONSE_BYTES - resp.len());
                resp.extend_from_slice(&chunk[..take]);
            }
            Err(e) => {
                return Err(RelayTransportError::InvalidUrl(format!(
                    "SSH HTTP probe read failed: {e}"
                )));
            }
        }
    }

    let text = String::from_utf8_lossy(&resp);
    let status_line = text.lines().next().ok_or_else(|| {
        RelayTransportError::InvalidUrl("empty HTTP response over SSH relay".into())
    })?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            RelayTransportError::InvalidUrl(format!("malformed HTTP status line: {status_line}"))
        })?;
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    Ok((status, body))
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

fn decode_hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode_fp(raw: &str) -> Result<String, RelayTransportError> {
    let bytes = raw.as_bytes();
    if !bytes.contains(&b'%') {
        return Ok(raw.to_string());
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(RelayTransportError::InvalidKey(
                    "truncated percent-encoding in fingerprint".into(),
                ));
            }
            let hi = decode_hex_nibble(bytes[i + 1]).ok_or_else(|| {
                RelayTransportError::InvalidKey("invalid percent-encoding in fingerprint".into())
            })?;
            let lo = decode_hex_nibble(bytes[i + 2]).ok_or_else(|| {
                RelayTransportError::InvalidKey("invalid percent-encoding in fingerprint".into())
            })?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| {
        RelayTransportError::InvalidKey("non-UTF-8 percent-encoded fingerprint".into())
    })
}

fn normalize_sha256_fp(raw: &str) -> Result<String, RelayTransportError> {
    let decoded = percent_decode_fp(raw.trim())?;
    let trimmed = decoded.trim();
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

    #[tokio::test]
    async fn ssh_host_key_verification_pinned_fp_match_and_mismatch() {
        let server_key = generate_ssh_tunnel_key().expect("server keygen");
        let server_pk = russh::keys::ssh_key::PublicKey::from_openssh(&server_key.openssh_pubkey)
            .expect("parse server pk");
        let expected_display_fp = server_pk.fingerprint(HashAlg::Sha256).to_string();
        let expected_fp_b64 =
            compute_openssh_pubkey_sha256_b64(&server_key.openssh_pubkey).expect("fp");
        let expected_fp_bytes = decode_32_bytes_flexible(&expected_fp_b64).expect("decode fp");
        let cert_or_pk = PublicKeyOrCertificate::from(server_pk);

        // Matching pinned `&fp=` passes
        let mut ok_handler = OrbitSshHandler {
            ssh_host: "relay.example.com".into(),
            ssh_port: 22,
            expected_fps: vec![expected_fp_bytes],
            known_hosts_path: None,
        };
        assert!(
            ok_handler
                .check_server_key(&cert_or_pk)
                .await
                .expect("pinned fp match must succeed")
        );

        // Mismatched pinned `&fp=` fails with HostKeyMismatch containing ssh-keygen STANDARD_NO_PAD fingerprint
        let mut bad_handler = OrbitSshHandler {
            ssh_host: "relay.example.com".into(),
            ssh_port: 22,
            expected_fps: vec![[0xAA; 32]],
            known_hosts_path: None,
        };
        let err = bad_handler
            .check_server_key(&cert_or_pk)
            .await
            .expect_err("wrong pinned fp must fail");
        assert!(matches!(err, RelayTransportError::HostKeyMismatch { .. }));
        let msg = err.to_string();
        assert!(
            msg.contains(&expected_display_fp),
            "error must include OpenSSH STANDARD_NO_PAD observed fingerprint ({expected_display_fp}), got: {msg}"
        );
    }

    #[test]
    fn ssh_fp_display_uses_standard_no_pad_and_uri_parses_all_variants() {
        // Choose bytes whose STANDARD_NO_PAD encoding contains both '+' and '/'
        let digest: [u8; 32] = [
            0xfb, 0xff, 0xfe, 0x3e, 0x3f, 0xfb, 0xff, 0xfe, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06,
            0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14,
            0x15, 0x16, 0x17, 0x18,
        ];
        let displayed = format_openssh_sha256_fingerprint(&digest);
        assert!(displayed.starts_with("SHA256:"));
        assert!(
            displayed.contains('+') && displayed.contains('/'),
            "expected STANDARD_NO_PAD characters '+' and '/' in {displayed}"
        );
        assert!(
            !displayed.ends_with('='),
            "OpenSSH SHA256 display must be unpadded, got {displayed}"
        );

        let url_safe = URL_SAFE_NO_PAD.encode(digest);
        let pct_encoded = displayed
            .replace(':', "%3A")
            .replace('+', "%2B")
            .replace('/', "%2F");
        let padded_std = format!("SHA256:{}", STANDARD.encode(digest));
        let key = generate_ssh_tunnel_key().expect("keygen");

        for fp_param in [
            displayed.as_str(),
            pct_encoded.as_str(),
            padded_std.as_str(),
            url_safe.as_str(),
            &format!("SHA256:{url_safe}"),
        ] {
            let uri = format!(
                "ssh+relay://user@relay.example.com:22/7878?key={}&fp={fp_param}",
                key.seed_b64
            );
            let spec = SshRelaySpec::parse(&uri).expect("parse fp variant");
            assert_eq!(spec.host_key_fps_b64, vec![url_safe.clone()]);
            assert_eq!(spec.decoded_host_key_fps().expect("decode"), vec![digest]);
        }

        // Malformed percent-encoding must be rejected
        assert!(
            SshRelaySpec::parse(&format!(
                "ssh+relay://user@relay.example.com:22/7878?key={}&fp=SHA256%3Aabc%ZZ",
                key.seed_b64
            ))
            .is_err()
        );
        assert!(
            SshRelaySpec::parse(&format!(
                "ssh+relay://user@relay.example.com:22/7878?key={}&fp=SHA256%2",
                key.seed_b64
            ))
            .is_err()
        );
    }

    #[tokio::test]
    async fn ssh_host_key_verification_empty_fp_enforces_known_hosts() {
        let server_key = generate_ssh_tunnel_key().expect("server keygen");
        let other_key = generate_ssh_tunnel_key().expect("other keygen");
        let server_pk = russh::keys::ssh_key::PublicKey::from_openssh(&server_key.openssh_pubkey)
            .expect("parse server pk");
        let other_pk = russh::keys::ssh_key::PublicKey::from_openssh(&other_key.openssh_pubkey)
            .expect("parse other pk");
        let server_display_fp = server_pk.fingerprint(HashAlg::Sha256).to_string();
        let other_display_fp = other_pk.fingerprint(HashAlg::Sha256).to_string();
        let server_fp_url =
            compute_openssh_pubkey_sha256_b64(&server_key.openssh_pubkey).expect("fp");

        let kh_path = std::env::temp_dir().join(format!(
            "orbit_known_hosts_test_{}_{}",
            std::process::id(),
            server_fp_url.chars().take(8).collect::<String>()
        ));
        let hashed_openssh_key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";
        let hashed_pk = russh::keys::ssh_key::PublicKey::from_openssh(hashed_openssh_key)
            .expect("parse hashed pk");
        let hashed_cert_or_pk = PublicKeyOrCertificate::from(hashed_pk);

        let kh_content = format!(
            "relay.example.com {}\n[custom.example.com]:2222 {}\n|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= {}\n",
            server_key.openssh_pubkey, server_key.openssh_pubkey, hashed_openssh_key
        );
        std::fs::write(&kh_path, kh_content).expect("write known_hosts");

        let server_cert_or_pk = PublicKeyOrCertificate::from(server_pk);
        let other_cert_or_pk = PublicKeyOrCertificate::from(other_pk);

        // 1. Empty `&fp=` against matching port-22 known_hosts entry passes
        let mut match_22 = OrbitSshHandler {
            ssh_host: "relay.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(
            match_22
                .check_server_key(&server_cert_or_pk)
                .await
                .expect("matching known_hosts port 22 must pass")
        );

        // 2. Empty `&fp=` against matching non-22 `[host]:port` known_hosts entry passes
        let mut match_2222 = OrbitSshHandler {
            ssh_host: "custom.example.com".into(),
            ssh_port: 2222,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(
            match_2222
                .check_server_key(&server_cert_or_pk)
                .await
                .expect("matching known_hosts custom port must pass")
        );

        // 2b. Empty `&fp=` against matching HMAC-SHA1 hashed `|1|...` known_hosts entry passes
        let mut match_hashed = OrbitSshHandler {
            ssh_host: "example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(
            match_hashed
                .check_server_key(&hashed_cert_or_pk)
                .await
                .expect("matching hashed known_hosts entry must pass")
        );

        // 3. Empty `&fp=` against MISMATCHED key in known_hosts fails with MITM error + observed fp
        let mut mismatch_handler = OrbitSshHandler {
            ssh_host: "relay.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        let mismatch_err = mismatch_handler
            .check_server_key(&other_cert_or_pk)
            .await
            .expect_err("mismatched known_hosts key must be rejected");
        assert!(matches!(
            mismatch_err,
            RelayTransportError::HostKeyMismatch { .. }
        ));
        let mismatch_msg = mismatch_err.to_string();
        assert!(
            mismatch_msg.contains("MITM") && mismatch_msg.contains(&other_display_fp),
            "expected MITM error with observed fingerprint ({other_display_fp}), got: {mismatch_msg}"
        );

        // 4. Empty `&fp=` against UNKNOWN host fails with actionable error + observed fp
        let mut unknown_handler = OrbitSshHandler {
            ssh_host: "unknown.example.com".into(),
            ssh_port: 2222,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        let unknown_err = unknown_handler
            .check_server_key(&server_cert_or_pk)
            .await
            .expect_err("unknown host must be rejected when no &fp= is pinned");
        assert!(matches!(
            unknown_err,
            RelayTransportError::HostKeyUnknown { .. }
        ));
        let unknown_msg = unknown_err.to_string();
        assert!(
            unknown_msg.contains(&format!("&fp={server_display_fp}")),
            "unknown host error must suggest &fp={server_display_fp}, got: {unknown_msg}"
        );
        // Pasting the suggested `&fp={observed_fp}` straight back into ssh+relay:// must parse and verify
        let pinned_uri = format!(
            "ssh+relay://user@unknown.example.com:2222/7878?key={}&fp={server_display_fp}",
            server_key.seed_b64
        );
        let parsed_pinned = SshRelaySpec::parse(&pinned_uri).expect("parse suggested fp pin");
        let mut pinned_handler = OrbitSshHandler {
            ssh_host: parsed_pinned.ssh_host.clone(),
            ssh_port: parsed_pinned.ssh_port,
            expected_fps: parsed_pinned.decoded_host_key_fps().expect("decode fps"),
            known_hosts_path: None,
        };
        assert!(
            pinned_handler
                .check_server_key(&server_cert_or_pk)
                .await
                .expect("pasted observed_fp pin must authorize matching server key")
        );
        assert!(
            unknown_msg.contains("ssh -p 2222 unknown.example.com")
                && unknown_msg.contains("known_hosts"),
            "unknown host error must suggest ssh/known_hosts, got: {unknown_msg}"
        );

        // 5. Missing known_hosts file also rejects with HostKeyUnknown (never silent Ok(true))
        let missing_kh = kh_path.with_extension("does_not_exist");
        let mut missing_handler = OrbitSshHandler {
            ssh_host: "relay.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(missing_kh),
        };
        let missing_err = missing_handler
            .check_server_key(&server_cert_or_pk)
            .await
            .expect_err("missing known_hosts must reject unknown host");
        assert!(matches!(
            missing_err,
            RelayTransportError::HostKeyUnknown { .. }
        ));
        assert!(
            missing_err
                .to_string()
                .contains(&format!("&fp={server_display_fp}"))
        );

        let _ = std::fs::remove_file(&kh_path);
    }

    #[tokio::test]
    async fn ssh_known_hosts_handles_revoked_rotation_tabs_ipv6_and_io_errors() {
        let key_old = generate_ssh_tunnel_key().expect("old key");
        let key_new = generate_ssh_tunnel_key().expect("new key");
        let key_revoked = generate_ssh_tunnel_key().expect("revoked key");

        let pk_old = PublicKeyOrCertificate::from(
            russh::keys::ssh_key::PublicKey::from_openssh(&key_old.openssh_pubkey).expect("old pk"),
        );
        let pk_new = PublicKeyOrCertificate::from(
            russh::keys::ssh_key::PublicKey::from_openssh(&key_new.openssh_pubkey).expect("new pk"),
        );
        let revoked_pubkey =
            russh::keys::ssh_key::PublicKey::from_openssh(&key_revoked.openssh_pubkey)
                .expect("revoked pk");
        let revoked_display_fp = revoked_pubkey.fingerprint(HashAlg::Sha256).to_string();
        let pk_revoked = PublicKeyOrCertificate::from(revoked_pubkey);
        let revoked_fp_url =
            compute_openssh_pubkey_sha256_b64(&key_revoked.openssh_pubkey).expect("fp");

        let kh_dir = std::env::temp_dir().join(format!(
            "orbit_kh_edge_{}_{}",
            std::process::id(),
            revoked_fp_url.chars().take(8).collect::<String>()
        ));
        std::fs::create_dir_all(&kh_dir).expect("mkdir");
        let kh_path = kh_dir.join("known_hosts");

        let kh_content = format!(
            "# Rotation: two Ed25519 keys for the same host\n\
             rotate.example.com {}\n\
             rotate.example.com {}\n\
             # Tab-separated fields and IPv6 custom port\n\
             tabs.example.com\t{}\n\
             [2001:db8::1]:2222 {}\n\
             # Revoked key ALSO present as ordinary entry (revoked MUST win regardless of order)\n\
             revoked-after.example.com {}\n\
             @revoked revoked-after.example.com {}\n\
             @revoked revoked-before.example.com {}\n\
             revoked-before.example.com {}\n",
            key_old.openssh_pubkey,
            key_new.openssh_pubkey,
            key_new.openssh_pubkey.replace(' ', "\t"),
            key_old.openssh_pubkey,
            key_revoked.openssh_pubkey,
            key_revoked.openssh_pubkey,
            key_revoked.openssh_pubkey,
            key_revoked.openssh_pubkey,
        );
        std::fs::write(&kh_path, kh_content).expect("write known_hosts");

        // 1. Key rotation: BOTH keys of the same algorithm pass
        for pk in [&pk_old, &pk_new] {
            let mut h = OrbitSshHandler {
                ssh_host: "rotate.example.com".into(),
                ssh_port: 22,
                expected_fps: Vec::new(),
                known_hosts_path: Some(kh_path.clone()),
            };
            assert!(
                h.check_server_key(pk)
                    .await
                    .expect("rotated key must be accepted")
            );
        }
        // Unrelated key against rotated host still fails as HostKeyMismatch
        let mut h_rot_bad = OrbitSshHandler {
            ssh_host: "rotate.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(matches!(
            h_rot_bad.check_server_key(&pk_revoked).await,
            Err(RelayTransportError::HostKeyMismatch { .. })
        ));

        // 2. Tab-separated known_hosts line passes
        let mut h_tabs = OrbitSshHandler {
            ssh_host: "tabs.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(
            h_tabs
                .check_server_key(&pk_new)
                .await
                .expect("tab-separated known_hosts line must pass")
        );

        // 3. IPv6 `[2001:db8::1]:2222` passes
        let mut h_v6 = OrbitSshHandler {
            ssh_host: "2001:db8::1".into(),
            ssh_port: 2222,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        assert!(
            h_v6.check_server_key(&pk_old)
                .await
                .expect("IPv6 custom port known_hosts must pass")
        );

        // 4. `@revoked` precedence wins whether placed BEFORE or AFTER a matching ordinary line
        for host in ["revoked-after.example.com", "revoked-before.example.com"] {
            let mut h_rev = OrbitSshHandler {
                ssh_host: host.into(),
                ssh_port: 22,
                expected_fps: Vec::new(),
                known_hosts_path: Some(kh_path.clone()),
            };
            let err = h_rev
                .check_server_key(&pk_revoked)
                .await
                .expect_err("revoked key must be rejected");
            assert!(matches!(err, RelayTransportError::HostKeyMismatch { .. }));
            let msg = err.to_string();
            assert!(
                msg.contains("@revoked") && msg.contains(&revoked_display_fp),
                "expected @revoked error with observed fp ({revoked_display_fp}), got: {msg}"
            );
        }

        // 5. Non-NotFound read error (known_hosts path is a directory) -> HostKeyVerification, NOT HostKeyUnknown
        let mut h_io_err = OrbitSshHandler {
            ssh_host: "rotate.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_dir.clone()),
        };
        let io_err = h_io_err
            .check_server_key(&pk_old)
            .await
            .expect_err("directory as known_hosts must fail with HostKeyVerification");
        assert!(
            matches!(io_err, RelayTransportError::HostKeyVerification(_)),
            "expected HostKeyVerification, got: {io_err:?}"
        );

        let _ = std::fs::remove_dir_all(&kh_dir);
    }

    #[tokio::test]
    async fn ssh_known_hosts_rejects_malformed_ordinary_entries_and_enforces_canonical_port22_and_salt_len()
     {
        let server_key = generate_ssh_tunnel_key().expect("server key");
        let server_pk = russh::keys::ssh_key::PublicKey::from_openssh(&server_key.openssh_pubkey)
            .expect("parse server pk");
        let server_b64 = server_key
            .openssh_pubkey
            .split_whitespace()
            .nth(1)
            .expect("key b64");
        let server_cert = PublicKeyOrCertificate::from(server_pk);

        let build_hashed = |salt: &[u8], host_entry: &str| -> String {
            let mut mac = Hmac::<Sha1>::new_from_slice(salt).expect("hmac");
            mac.update(host_entry.as_bytes());
            let tag = mac.finalize().into_bytes();
            format!("|1|{}|{}", STANDARD.encode(salt), STANDARD.encode(tag))
        };

        let valid_salt_20 = [0x11u8; 20];
        let short_salt_16 = [0x22u8; 16];
        let long_salt_24 = [0x33u8; 24];

        // 1. Canonical port-22 lookup name & wildcard/negation semantics
        assert!(known_hosts_pattern_list_matches(
            "relay.example.com",
            "relay.example.com",
            22
        ));
        assert!(
            !known_hosts_pattern_list_matches("[relay.example.com]:22", "relay.example.com", 22),
            "port 22 must only match canonical bare host, never [host]:22"
        );
        assert!(
            !known_hosts_pattern_list_matches("*,!relay.example.com", "relay.example.com", 22),
            "port 22 negation must exclude relay.example.com"
        );
        assert!(known_hosts_pattern_list_matches(
            "*,!relay.example.com",
            "other.example.com",
            22
        ));
        assert!(
            !known_hosts_pattern_list_matches(
                "*.example.com,!blocked.example.com",
                "blocked.example.com",
                22
            ),
            "wildcard + negation on port 22 must reject negated host"
        );
        assert!(known_hosts_pattern_list_matches(
            "*.example.com,!blocked.example.com",
            "allowed.example.com",
            22
        ));
        assert!(
            !known_hosts_pattern_list_matches("[*]:22", "relay.example.com", 22),
            "bracketed port-22 wildcard must not match canonical port-22 host"
        );
        assert!(known_hosts_pattern_list_matches(
            "[*.example.com]:2222,![blocked.example.com]:2222",
            "allowed.example.com",
            2222
        ));
        assert!(!known_hosts_pattern_list_matches(
            "[*.example.com]:2222,![blocked.example.com]:2222",
            "blocked.example.com",
            2222
        ));
        assert!(!known_hosts_pattern_list_matches(
            "*.example.com",
            "allowed.example.com",
            2222
        ));

        // 2. Strict 20-byte salt enforcement for |1|salt|hash
        let valid_hash_22 = build_hashed(&valid_salt_20, "hashed22.example.com");
        let valid_hash_2222 = build_hashed(&valid_salt_20, "[hashed2222.example.com]:2222");
        let short_salt_hash = build_hashed(&short_salt_16, "short-salt.example.com");
        let long_salt_hash = build_hashed(&long_salt_24, "long-salt.example.com");

        assert!(known_hosts_pattern_list_matches(
            &valid_hash_22,
            "hashed22.example.com",
            22
        ));
        assert!(known_hosts_pattern_list_matches(
            &valid_hash_2222,
            "hashed2222.example.com",
            2222
        ));
        assert!(
            !known_hosts_pattern_list_matches(&short_salt_hash, "short-salt.example.com", 22),
            "16-byte salt must be rejected"
        );
        assert!(
            !known_hosts_pattern_list_matches(&long_salt_hash, "long-salt.example.com", 22),
            "24-byte salt must be rejected"
        );

        // 3. End-to-end known_hosts file checks: malformed ordinary entries never authorize,
        //    non-20-byte salts never authorize, and @revoked exotic fallback still fails closed.
        let kh_path = std::env::temp_dir().join(format!(
            "orbit_kh_strict_{}_{}",
            std::process::id(),
            server_key.seed_b64.chars().take(8).collect::<String>()
        ));
        let kh_content = format!(
            "bad-type.example.com ssh-rsa {server_b64}\n\
             exotic-ordinary.example.com sk-ssh-ed25519@openssh.com {server_b64}\n\
             corrupt-b64.example.com ssh-ed25519 !!!not-base64!!!\n\
             [port22-bracket.example.com]:22 {}\n\
             {short_salt_hash} {}\n\
             {long_salt_hash} {}\n\
             @revoked exotic-revoked.example.com sk-ssh-ed25519@openssh.com {server_b64}\n\
             exotic-revoked.example.com {}\n",
            server_key.openssh_pubkey,
            server_key.openssh_pubkey,
            server_key.openssh_pubkey,
            server_key.openssh_pubkey,
        );
        std::fs::write(&kh_path, kh_content).expect("write strict known_hosts");

        for rejected_host in [
            "bad-type.example.com",
            "exotic-ordinary.example.com",
            "corrupt-b64.example.com",
            "port22-bracket.example.com",
            "short-salt.example.com",
            "long-salt.example.com",
        ] {
            let mut h = OrbitSshHandler {
                ssh_host: rejected_host.into(),
                ssh_port: 22,
                expected_fps: Vec::new(),
                known_hosts_path: Some(kh_path.clone()),
            };
            let err = h
                .check_server_key(&server_cert)
                .await
                .expect_err("malformed/non-canonical/bad-salt entry must never authorize");
            assert!(
                matches!(err, RelayTransportError::HostKeyUnknown { .. }),
                "expected HostKeyUnknown for {rejected_host}, got: {err:?}"
            );
        }

        // Exotic @revoked entry still fails closed even when a valid ordinary entry exists
        let mut h_exotic_rev = OrbitSshHandler {
            ssh_host: "exotic-revoked.example.com".into(),
            ssh_port: 22,
            expected_fps: Vec::new(),
            known_hosts_path: Some(kh_path.clone()),
        };
        let rev_err = h_exotic_rev
            .check_server_key(&server_cert)
            .await
            .expect_err("exotic @revoked entry must still fail closed");
        assert!(matches!(
            rev_err,
            RelayTransportError::HostKeyMismatch { .. }
        ));
        assert!(rev_err.to_string().contains("@revoked"));

        let _ = std::fs::remove_file(&kh_path);
    }

    #[test]
    fn fleet_token_preserved_across_normalization_and_redacted_in_display() {
        let key = generate_ssh_tunnel_key().expect("key");
        let secret = "fleet_tok_secret_value_123456";
        let ssh_uri = format!(
            "ssh+relay://ec2-user@10.0.0.1:22/7878?key={}&ft={secret}",
            key.seed_b64
        );
        let parsed = SshRelaySpec::parse(&ssh_uri).expect("parse ssh");
        assert_eq!(parsed.fleet_token.as_deref(), Some(secret));
        assert_eq!(parsed.to_uri(), ssh_uri);
        assert!(!parsed.redacted_display().contains(secret));
        assert!(!redact_relay_url(&ssh_uri).contains(secret));

        let direct = format!("wss://relay.example.com/?ft={secret}");
        let normalized = normalize_relay_url(&direct).expect("normalize direct");
        assert_eq!(normalized, format!("wss://relay.example.com?ft={secret}"));
        let (clean, tok) = split_direct_relay_url(&normalized).expect("split direct");
        assert_eq!(clean, "wss://relay.example.com");
        assert_eq!(tok.as_deref(), Some(secret));
        let redacted = redact_relay_url(&normalized);
        assert!(!redacted.contains(secret));
        assert!(redacted.contains("ft=<redacted>"));
    }
}
