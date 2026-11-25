//! End-to-end encryption (`Noise_NKpsk0_25519_ChaChaPoly_SHA256` for mutually authenticated
//! client<->daemon tunnels, `Noise_NK_25519_ChaChaPoly_SHA256` for Relay PoP), Multi-Host
//! pairing credentials, and Zero-Knowledge E2EE Cloud Vault (`orbit-cloud-vault/v1`).
//!
//! Ensures the Relay and Cloud Storage providers are strictly zero-knowledge and allows strict
//! `--allow-pubkey` admission control.

pub mod checksum;
pub mod vault;
pub use checksum::{
    ChecksumError, SHA256SUMS_FILENAME, Sha256Hasher, find_expected_sha256, parse_sha256sums,
    sha256_hex_bytes, verify_sha256_hex,
};
pub use vault::{
    CLOUD_VAULT_CIPHER, CLOUD_VAULT_ENVELOPE_FORMAT, CLOUD_VAULT_KDF, DerivedVaultKey,
    EncryptedVaultEnvelope, VaultCryptoError, VaultKdfParams, decrypt_vault_bytes,
    decrypt_vault_json, derive_new_vault_key, derive_vault_key, encrypt_vault_bytes,
    encrypt_vault_json, generate_random_vault_key, generate_recovery_passphrase,
    generate_vault_salt_b64, validate_envelope_header,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::params::{CipherChoice, DHChoice, HashChoice};
use snow::resolvers::{CryptoResolver, DefaultResolver};
use snow::types::{Cipher, Dh, Hash, Random};
use snow::{Builder, HandshakeState, TransportState};
use zeroize::Zeroizing;

pub const NOISE_PATTERN: &str = "Noise_NK_25519_ChaChaPoly_SHA256";
/// Mutually authenticated client<->daemon E2EE Noise pattern using the host's persisted
/// `pairing_secret` as a pre-shared key (`psk0`).
pub const CLIENT_E2EE_NOISE_PATTERN: &str = "Noise_NKpsk0_25519_ChaChaPoly_SHA256";
/// Domain-separated Noise prologue for mutually authenticated client<->daemon E2EE tunnels.
pub const NOISE_PROLOGUE: &[u8] = b"orbit-relay-e2ee/2";
/// Exact byte length of each empty-payload `Noise_NKpsk0` client<->daemon handshake message
/// (32-byte X25519 ephemeral pubkey + 16-byte Poly1305 AEAD tag).
pub const CLIENT_E2EE_MSG_LEN: usize = 48;
/// Domain-separation prefix for deriving the 32-byte `Noise_NKpsk0` PSK from `pairing_secret`.
const CLIENT_E2EE_PSK_DOMAIN: &[u8] = b"orbit-e2ee-psk/v1\0";
/// Dedicated Noise prologue for 1-RTT Relay Daemon Control Proof-of-Possession (PoP).
/// Strictly domain-separated from [`NOISE_PROLOGUE`] so client E2EE sessions can never be used
/// as cross-protocol signing/decryption oracles.
pub const RELAY_POP_PROLOGUE: &[u8] = b"orbit-relay-pop/1";
/// Exact byte length of each empty-payload `Noise_NK` PoP handshake message
/// (32-byte X25519 ephemeral pubkey + 16-byte Poly1305 AEAD tag).
pub const RELAY_POP_MSG_LEN: usize = 48;
/// Byte length of the random nonce sent by Relay when authenticating `/v1/daemon/tunnel`.
pub const TUNNEL_ATTACH_NONCE_LEN: usize = 16;
/// Byte length of the HMAC-SHA256 proof returned by Daemon on `/v1/daemon/tunnel`.
pub const TUNNEL_ATTACH_MAC_LEN: usize = 32;
const MAX_NOISE_CHUNK_PLAINTEXT: usize = 65_000;

/// DH wrapper enforcing contributory X25519 outputs (RFC 7748 §6.1).
///
/// Snow's default resolver accepts non-zero low-order Curve25519 points (such as `[1, 0, ...]`
/// or order-8 points) that yield an all-zero shared secret. Rejecting any all-zero DH result
/// blocks low-order public-key forgery attacks on static-key handshakes.
struct ContributoryDh {
    inner: Box<dyn Dh>,
}

impl Dh for ContributoryDh {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn pub_len(&self) -> usize {
        self.inner.pub_len()
    }

    fn priv_len(&self) -> usize {
        self.inner.priv_len()
    }

    fn set(&mut self, privkey: &[u8]) {
        self.inner.set(privkey);
    }

    fn generate(&mut self, rng: &mut dyn Random) {
        self.inner.generate(rng);
    }

    fn pubkey(&self) -> &[u8] {
        self.inner.pubkey()
    }

    fn privkey(&self) -> &[u8] {
        self.inner.privkey()
    }

    fn dh(&self, pubkey: &[u8], out: &mut [u8]) -> Result<(), snow::Error> {
        self.inner.dh(pubkey, out)?;
        let len = self.pub_len().min(out.len());
        let mut acc = 0u8;
        for &b in &out[..len] {
            acc |= b;
        }
        if len == 0 || acc == 0 {
            return Err(snow::Error::Decrypt);
        }
        Ok(())
    }
}

struct ContributoryCryptoResolver;

impl CryptoResolver for ContributoryCryptoResolver {
    fn resolve_rng(&self) -> Option<Box<dyn Random>> {
        DefaultResolver.resolve_rng()
    }

    fn resolve_dh(&self, choice: &DHChoice) -> Option<Box<dyn Dh>> {
        DefaultResolver
            .resolve_dh(choice)
            .map(|inner| Box::new(ContributoryDh { inner }) as Box<dyn Dh>)
    }

    fn resolve_hash(&self, choice: &HashChoice) -> Option<Box<dyn Hash>> {
        DefaultResolver.resolve_hash(choice)
    }

    fn resolve_cipher(&self, choice: &CipherChoice) -> Option<Box<dyn Cipher>> {
        DefaultResolver.resolve_cipher(choice)
    }
}

fn noise_builder() -> Result<Builder<'static>, CryptoError> {
    let params = NOISE_PATTERN.parse()?;
    Ok(Builder::with_resolver(
        params,
        Box::new(ContributoryCryptoResolver),
    ))
}

fn client_e2ee_noise_builder() -> Result<Builder<'static>, CryptoError> {
    let params = CLIENT_E2EE_NOISE_PATTERN.parse()?;
    Ok(Builder::with_resolver(
        params,
        Box::new(ContributoryCryptoResolver),
    ))
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("noise protocol error: {0}")]
    Noise(#[from] snow::Error),
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid key length: expected 32 bytes, got {0}")]
    InvalidKeyLength(usize),
    #[error("malformed encrypted record")]
    MalformedRecord,
    #[error("invalid or empty pairing secret")]
    InvalidPairingSecret,
    #[error("E2EE mutual authentication failed")]
    AuthenticationFailed,
    #[error("proof-of-possession verification failed")]
    PopVerificationFailed,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Derive a 32-byte `Noise_NKpsk0` pre-shared key from the host's `pairing_secret`.
///
/// Fails closed on empty/whitespace-only secrets or secrets containing `\r`, `\n`, or `\0`.
pub fn derive_e2ee_psk(pairing_secret: &str) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    if pairing_secret.trim().is_empty()
        || pairing_secret
            .bytes()
            .any(|b| matches!(b, b'\r' | b'\n' | b'\0'))
    {
        return Err(CryptoError::InvalidPairingSecret);
    }
    let mut hasher = Sha256::new();
    hasher.update(CLIENT_E2EE_PSK_DOMAIN);
    hasher.update(pairing_secret.as_bytes());
    let mut psk = Zeroizing::new([0u8; 32]);
    psk.copy_from_slice(&hasher.finalize());
    Ok(psk)
}

/// Static X25519 identity keypair owned by each Orbit Daemon host (Devbox, Workstation, Mac mini).
#[derive(Clone, Serialize, Deserialize)]
pub struct HostIdentityKeypair {
    pub public_key_b64: String,
    pub private_key_b64: String,
}

impl HostIdentityKeypair {
    pub fn generate() -> Result<Self, CryptoError> {
        let params = NOISE_PATTERN.parse()?;
        let builder = Builder::new(params);
        let kp = builder.generate_keypair()?;
        Ok(Self {
            public_key_b64: URL_SAFE_NO_PAD.encode(&kp.public),
            private_key_b64: URL_SAFE_NO_PAD.encode(&kp.private),
        })
    }

    pub fn public_bytes(&self) -> Result<[u8; 32], CryptoError> {
        decode_key_32(&self.public_key_b64)
    }

    pub fn private_bytes(&self) -> Result<[u8; 32], CryptoError> {
        decode_key_32(&self.private_key_b64)
    }

    /// Short deterministic host fingerprint used as stable `host_id`.
    pub fn host_id(&self) -> Result<String, CryptoError> {
        let pub_bytes = self.public_bytes()?;
        let digest = Sha256::digest(pub_bytes);
        Ok(format!("host_{}", URL_SAFE_NO_PAD.encode(&digest[..12])))
    }
}

pub fn decode_key_32(b64: &str) -> Result<[u8; 32], CryptoError> {
    let raw = URL_SAFE_NO_PAD.decode(b64.trim())?;
    let len = raw.len();
    raw.try_into()
        .map_err(|_| CryptoError::InvalidKeyLength(len))
}

/// Encode a 32-byte key/secret into canonical `URL_SAFE_NO_PAD` base64.
pub fn encode_key_32(key: &[u8; 32]) -> String {
    URL_SAFE_NO_PAD.encode(key)
}

/// Compute RFC 2104 `HMAC-SHA256(attach_secret, attach_nonce || tunnel_id)` to authenticate
/// `/v1/daemon/tunnel` before the relay consumes a pending tunnel slot.
pub fn compute_tunnel_attach_mac(
    attach_secret: &[u8; 32],
    attach_nonce: &[u8; TUNNEL_ATTACH_NONCE_LEN],
    tunnel_id: &str,
) -> [u8; TUNNEL_ATTACH_MAC_LEN] {
    let mut ipad = Zeroizing::new([0x36u8; 64]);
    let mut opad = Zeroizing::new([0x5cu8; 64]);
    for i in 0..32 {
        ipad[i] ^= attach_secret[i];
        opad[i] ^= attach_secret[i];
    }
    let mut inner = Sha256::new();
    inner.update(*ipad);
    inner.update(attach_nonce);
    inner.update(tunnel_id.as_bytes());
    let inner_digest = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(*opad);
    outer.update(inner_digest);
    outer.finalize().into()
}

/// Constant-time verification of `HMAC-SHA256(attach_secret, attach_nonce || tunnel_id)`.
pub fn verify_tunnel_attach_mac(
    attach_secret: &[u8; 32],
    attach_nonce: &[u8; TUNNEL_ATTACH_NONCE_LEN],
    tunnel_id: &str,
    candidate_mac: &[u8],
) -> bool {
    if candidate_mac.len() != TUNNEL_ATTACH_MAC_LEN {
        return false;
    }
    let expected = compute_tunnel_attach_mac(attach_secret, attach_nonce, tunnel_id);
    constant_time_eq(&expected, candidate_mac)
}

/// Generate a 256-bit (32-byte CSPRNG) URL-safe base64 pairing secret (`43` chars, no padding).
pub fn generate_pairing_secret() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Constant-time byte-slice comparison (fixed-time XOR fold with no early exit on mismatch).
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

/// Pairing payload encoded into QR codes for instant mobile onboarding across multiple hosts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingOffer {
    pub host_id: String,
    pub hostname: String,
    pub daemon_pubkey_b64: String,
    /// Direct endpoints (e.g. LAN IP, Tailscale IP, or public Devbox WS/QUIC endpoint).
    pub direct_endpoints: Vec<String>,
    /// Optional private self-hosted Relay URL (e.g. `wss://aws-box:7878`).
    pub relay_url: Option<String>,
    pub pairing_secret: String,
}

/// Normalize a direct WebSocket endpoint (`host:port` -> `ws://host:port/ws`, `[v6]:port` -> `ws://[v6]:port/ws`),
/// ensuring a `ws://` / `wss://` scheme and `/ws` path when omitted.
pub fn normalize_ws_endpoint(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return String::new();
    }
    let has_ws_scheme = trimmed.len() >= 5
        && (trimmed[..5].eq_ignore_ascii_case("ws://")
            || (trimmed.len() >= 6 && trimmed[..6].eq_ignore_ascii_case("wss://")));
    let with_scheme = if has_ws_scheme || trimmed.contains("://") {
        trimmed.to_string()
    } else if let Some(rest) = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("HTTP://"))
    {
        format!("ws://{rest}")
    } else if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("HTTPS://"))
    {
        format!("wss://{rest}")
    } else {
        format!("ws://{trimmed}")
    };

    if with_scheme.ends_with("/ws") || with_scheme.ends_with("/ws/e2ee") {
        with_scheme
    } else {
        let after_scheme = with_scheme
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&with_scheme);
        if after_scheme.contains('/') {
            with_scheme
        } else {
            format!("{with_scheme}/ws")
        }
    }
}

/// Normalize a peer direct endpoint to its Noise_NK `/ws/e2ee` WebSocket URL.
pub fn normalize_e2ee_ws_endpoint(raw: &str) -> String {
    let normalized = normalize_ws_endpoint(raw);
    if normalized.is_empty() {
        return normalized;
    }
    if let Some(prefix) = normalized.strip_suffix("/ws") {
        format!("{prefix}/ws/e2ee")
    } else if normalized.ends_with("/ws/e2ee") {
        normalized
    } else {
        format!("{normalized}/ws/e2ee")
    }
}

impl PairingOffer {
    pub fn to_qr_uri(&self) -> Result<String, CryptoError> {
        let json = serde_json::to_vec(self)?;
        Ok(format!("orbit://pair/{}", URL_SAFE_NO_PAD.encode(json)))
    }

    pub fn from_qr_uri(uri: &str) -> Result<Self, CryptoError> {
        let encoded = uri.strip_prefix("orbit://pair/").unwrap_or(uri);
        let raw = URL_SAFE_NO_PAD.decode(encoded.trim())?;
        let mut offer: Self = serde_json::from_slice(&raw)?;
        for ep in &mut offer.direct_endpoints {
            let norm = normalize_ws_endpoint(ep);
            if !norm.is_empty() {
                *ep = norm;
            }
        }
        Ok(offer)
    }
}

/// Initiator (Mobile / CLI Client) handshake state knowing the Daemon's pinned static public key.
pub struct ClientHandshake {
    state: HandshakeState,
}

/// Responder (Daemon) handshake helper holding its static private key.
pub struct DaemonHandshake;

/// Established authenticated E2EE session with replay-protected counters and automatic chunking.
pub struct SecureChannel {
    transport: TransportState,
}

impl ClientHandshake {
    pub fn new(
        daemon_pubkey: &[u8; 32],
        pairing_secret: &str,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let psk = derive_e2ee_psk(pairing_secret)?;
        let mut state = client_e2ee_noise_builder()?
            .prologue(NOISE_PROLOGUE)
            .remote_public_key(daemon_pubkey)
            .psk(0, &*psk)
            .build_initiator()?;
        let mut msg1 = vec![0u8; CLIENT_E2EE_MSG_LEN];
        let n = state.write_message(&[], &mut msg1)?;
        if n != CLIENT_E2EE_MSG_LEN {
            return Err(CryptoError::AuthenticationFailed);
        }
        Ok((Self { state }, msg1))
    }

    pub fn finalize(mut self, msg2: &[u8]) -> Result<SecureChannel, CryptoError> {
        if msg2.len() != CLIENT_E2EE_MSG_LEN {
            return Err(CryptoError::AuthenticationFailed);
        }
        let mut payload = [0u8; CLIENT_E2EE_MSG_LEN];
        let n = self
            .state
            .read_message(msg2, &mut payload)
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        if n != 0 {
            return Err(CryptoError::AuthenticationFailed);
        }
        Ok(SecureChannel {
            transport: self.state.into_transport_mode()?,
        })
    }
}

impl DaemonHandshake {
    pub fn accept(
        daemon_privkey: &[u8; 32],
        expected_pairing_secret: &str,
        msg1: &[u8],
    ) -> Result<(SecureChannel, Vec<u8>), CryptoError> {
        let psk = derive_e2ee_psk(expected_pairing_secret)?;
        if msg1.len() != CLIENT_E2EE_MSG_LEN {
            return Err(CryptoError::AuthenticationFailed);
        }
        let mut state: HandshakeState = client_e2ee_noise_builder()?
            .prologue(NOISE_PROLOGUE)
            .local_private_key(daemon_privkey)
            .psk(0, &*psk)
            .build_responder()?;
        let mut payload = [0u8; CLIENT_E2EE_MSG_LEN];
        let n = state
            .read_message(msg1, &mut payload)
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        if n != 0 {
            return Err(CryptoError::AuthenticationFailed);
        }

        let mut msg2 = vec![0u8; CLIENT_E2EE_MSG_LEN];
        let out_len = state
            .write_message(&[], &mut msg2)
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        if out_len != CLIENT_E2EE_MSG_LEN {
            return Err(CryptoError::AuthenticationFailed);
        }

        Ok((
            SecureChannel {
                transport: state.into_transport_mode()?,
            },
            msg2,
        ))
    }
}

impl SecureChannel {
    /// Encrypt an arbitrarily sized frame into length-prefixed Noise transport chunks.
    pub fn seal_record(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let chunks: Vec<&[u8]> = if plaintext.is_empty() {
            vec![&[]]
        } else {
            plaintext.chunks(MAX_NOISE_CHUNK_PLAINTEXT).collect()
        };

        let mut out = Vec::with_capacity(plaintext.len() + chunks.len() * 24 + 4);
        let count = u32::try_from(chunks.len()).map_err(|_| CryptoError::MalformedRecord)?;
        out.extend_from_slice(&count.to_be_bytes());

        let mut buf = vec![0u8; MAX_NOISE_CHUNK_PLAINTEXT + 64];
        for chunk in chunks {
            let n = self.transport.write_message(chunk, &mut buf)?;
            let len_u16 = u16::try_from(n).map_err(|_| CryptoError::MalformedRecord)?;
            out.extend_from_slice(&len_u16.to_be_bytes());
            out.extend_from_slice(&buf[..n]);
        }
        Ok(out)
    }

    /// Decrypt and verify a length-prefixed Noise record, rejecting tampering or replays.
    pub fn open_record(&mut self, record: &[u8]) -> Result<Vec<u8>, CryptoError> {
        if record.len() < 4 {
            return Err(CryptoError::MalformedRecord);
        }
        let chunk_count = u32::from_be_bytes([record[0], record[1], record[2], record[3]]) as usize;
        if chunk_count == 0 || chunk_count > 1024 {
            return Err(CryptoError::MalformedRecord);
        }

        let mut cursor = 4;
        let mut plaintext = Vec::with_capacity(record.len());
        let mut buf = vec![0u8; MAX_NOISE_CHUNK_PLAINTEXT + 64];

        for _ in 0..chunk_count {
            if cursor + 2 > record.len() {
                return Err(CryptoError::MalformedRecord);
            }
            let ct_len = u16::from_be_bytes([record[cursor], record[cursor + 1]]) as usize;
            cursor += 2;
            if cursor + ct_len > record.len() {
                return Err(CryptoError::MalformedRecord);
            }
            let n = self
                .transport
                .read_message(&record[cursor..cursor + ct_len], &mut buf)?;
            plaintext.extend_from_slice(&buf[..n]);
            cursor += ct_len;
        }

        if cursor != record.len() {
            return Err(CryptoError::MalformedRecord);
        }
        Ok(plaintext)
    }
}

/// Initiator (Orbit Relay) side of the 1-RTT `Noise_NK` Proof-of-Possession handshake.
///
/// With contributory X25519 DH enforced and empty NK payloads under [`RELAY_POP_PROLOGUE`],
/// authenticating message 2 cryptographically proves the peer possesses the static private key
/// matching `daemon_pubkey` and yields an encrypted [`SecureChannel`] for the control stream.
pub struct RelayPopInitiator {
    state: HandshakeState,
}

/// Responder (Orbit Daemon) side of the 1-RTT `Noise_NK` Proof-of-Possession handshake.
pub struct RelayPopResponder;

impl RelayPopInitiator {
    /// Initiate a 1-RTT `Noise_NK` PoP handshake to `daemon_pubkey`, returning
    /// `(initiator_state, msg1)`. Rejects all low-order X25519 public keys via contributory DH.
    pub fn new(daemon_pubkey: &[u8; 32]) -> Result<(Self, Vec<u8>), CryptoError> {
        let mut state = noise_builder()?
            .prologue(RELAY_POP_PROLOGUE)
            .remote_public_key(daemon_pubkey)
            .build_initiator()?;

        let mut msg1 = vec![0u8; RELAY_POP_MSG_LEN];
        let n = state
            .write_message(&[], &mut msg1)
            .map_err(|_| CryptoError::PopVerificationFailed)?;
        if n != RELAY_POP_MSG_LEN {
            return Err(CryptoError::PopVerificationFailed);
        }
        Ok((Self { state }, msg1))
    }

    /// Authenticate the daemon's PoP response (`msg2`) and transition into an encrypted
    /// [`SecureChannel`] protecting subsequent Relay control messages (`registered`, `open_tunnel`).
    pub fn finalize(mut self, msg2: &[u8]) -> Result<SecureChannel, CryptoError> {
        if msg2.len() != RELAY_POP_MSG_LEN {
            return Err(CryptoError::PopVerificationFailed);
        }
        let mut payload = [0u8; RELAY_POP_MSG_LEN];
        let n = self
            .state
            .read_message(msg2, &mut payload)
            .map_err(|_| CryptoError::PopVerificationFailed)?;
        if n != 0 {
            return Err(CryptoError::PopVerificationFailed);
        }
        Ok(SecureChannel {
            transport: self.state.into_transport_mode()?,
        })
    }
}

impl RelayPopResponder {
    /// Authenticate the relay's `Noise_NK` PoP message 1 using `daemon_privkey`, returning
    /// `(control_secure_channel, msg2)`.
    pub fn accept(
        daemon_privkey: &[u8; 32],
        msg1: &[u8],
    ) -> Result<(SecureChannel, Vec<u8>), CryptoError> {
        if msg1.len() != RELAY_POP_MSG_LEN {
            return Err(CryptoError::PopVerificationFailed);
        }
        let mut state: HandshakeState = noise_builder()?
            .prologue(RELAY_POP_PROLOGUE)
            .local_private_key(daemon_privkey)
            .build_responder()?;

        let mut payload = [0u8; RELAY_POP_MSG_LEN];
        let n = state
            .read_message(msg1, &mut payload)
            .map_err(|_| CryptoError::PopVerificationFailed)?;
        if n != 0 {
            return Err(CryptoError::PopVerificationFailed);
        }

        let mut msg2 = vec![0u8; RELAY_POP_MSG_LEN];
        let out_len = state
            .write_message(&[], &mut msg2)
            .map_err(|_| CryptoError::PopVerificationFailed)?;
        if out_len != RELAY_POP_MSG_LEN {
            return Err(CryptoError::PopVerificationFailed);
        }

        Ok((
            SecureChannel {
                transport: state.into_transport_mode()?,
            },
            msg2,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_nkpsk0_handshake_and_bidirectional_large_record_encryption() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");
        let secret = generate_pairing_secret();

        let (client_hs, msg1) = ClientHandshake::new(&pub_bytes, &secret).expect("client init");
        assert_eq!(msg1.len(), CLIENT_E2EE_MSG_LEN);

        let (mut daemon_chan, msg2) =
            DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("daemon accept");
        assert_eq!(msg2.len(), CLIENT_E2EE_MSG_LEN);

        let mut client_chan = client_hs.finalize(&msg2).expect("client finalize");

        // Test multi-chunk large payload (> 65KB) client -> daemon
        let big_payload = vec![0x42u8; 150_000];
        let ciphertext = client_chan.seal_record(&big_payload).expect("seal");
        let decrypted = daemon_chan.open_record(&ciphertext).expect("open");
        assert_eq!(big_payload, decrypted);

        // Replay protection: replaying the exact same ciphertext must fail!
        assert!(daemon_chan.open_record(&ciphertext).is_err());

        // Test bidirectional daemon -> client transport
        let reply_payload = b"daemon-welcome-authenticated";
        let reply_ct = daemon_chan.seal_record(reply_payload).expect("daemon seal");
        let reply_pt = client_chan.open_record(&reply_ct).expect("client open");
        assert_eq!(reply_pt, reply_payload);
        assert!(client_chan.open_record(&reply_ct).is_err());
    }

    #[test]
    fn client_e2ee_rejects_wrong_empty_whitespace_and_control_char_secrets() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");
        let real_secret = generate_pairing_secret();
        let wrong_secret = generate_pairing_secret();

        // Empty, whitespace-only, and control-char secrets fail closed on derivation & handshake
        for bad in ["", " ", "\t\t", "   \n", "abc\rdef", "abc\ndef", "abc\0def"] {
            assert!(
                matches!(derive_e2ee_psk(bad), Err(CryptoError::InvalidPairingSecret)),
                "derive_e2ee_psk must reject {bad:?}"
            );
            assert!(
                matches!(
                    ClientHandshake::new(&pub_bytes, bad),
                    Err(CryptoError::InvalidPairingSecret)
                ),
                "ClientHandshake::new must reject {bad:?}"
            );
            assert!(
                matches!(
                    DaemonHandshake::accept(&priv_bytes, bad, &[0u8; CLIENT_E2EE_MSG_LEN]),
                    Err(CryptoError::InvalidPairingSecret)
                ),
                "DaemonHandshake::accept must reject {bad:?}"
            );
        }

        // Wrong pairing_secret fails AEAD verification with AuthenticationFailed (no msg2 produced)
        let (_client_hs, msg1_wrong) =
            ClientHandshake::new(&pub_bytes, &wrong_secret).expect("client init wrong");
        assert!(matches!(
            DaemonHandshake::accept(&priv_bytes, &real_secret, &msg1_wrong),
            Err(CryptoError::AuthenticationFailed)
        ));

        // Wrong-length msg1 and msg2 fail closed with AuthenticationFailed
        assert!(matches!(
            DaemonHandshake::accept(&priv_bytes, &real_secret, &msg1_wrong[..47]),
            Err(CryptoError::AuthenticationFailed)
        ));
        let oversized = vec![0u8; CLIENT_E2EE_MSG_LEN + 1];
        assert!(matches!(
            DaemonHandshake::accept(&priv_bytes, &real_secret, &oversized),
            Err(CryptoError::AuthenticationFailed)
        ));

        let (client_hs2, _msg1_valid) =
            ClientHandshake::new(&pub_bytes, &real_secret).expect("client init valid");
        assert!(matches!(
            client_hs2.finalize(&oversized),
            Err(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn client_e2ee_replayed_msg1_is_unusable_without_client_ephemeral_key() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");
        let secret = generate_pairing_secret();

        // Legitimate session 1
        let (client_hs1, msg1) = ClientHandshake::new(&pub_bytes, &secret).expect("init 1");
        let (mut daemon_chan1, msg2_1) =
            DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("accept 1");
        let mut client_chan1 = client_hs1.finalize(&msg2_1).expect("finalize 1");
        let legit_ct = client_chan1
            .seal_record(b"legit-frame-1")
            .expect("seal legit");
        assert_eq!(
            daemon_chan1.open_record(&legit_ct).expect("open legit"),
            b"legit-frame-1"
        );

        // Eavesdropper replays captured msg1 to daemon; daemon mints a fresh ephemeral key (msg2_replay)
        let (mut daemon_chan_replayed, msg2_replay) =
            DaemonHandshake::accept(&priv_bytes, &secret, &msg1).expect("replayed accept");
        assert_ne!(msg2_replay, msg2_1);

        // Without client 1's ephemeral private key:
        // 1. A fresh client handshake cannot finalize with msg2_replay
        let (attacker_hs, _attacker_msg1) =
            ClientHandshake::new(&pub_bytes, &secret).expect("attacker fresh init");
        assert!(matches!(
            attacker_hs.finalize(&msg2_replay),
            Err(CryptoError::AuthenticationFailed)
        ));

        // 2. Replaying captured transport ciphertext from session 1 fails on daemon_chan_replayed
        assert!(daemon_chan_replayed.open_record(&legit_ct).is_err());
    }

    #[test]
    fn relay_pop_handshake_establishes_encrypted_control_channel() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");

        let (initiator, msg1) = RelayPopInitiator::new(&pub_bytes).expect("pop init");
        assert_eq!(msg1.len(), RELAY_POP_MSG_LEN);

        let (mut daemon_ctrl, msg2) =
            RelayPopResponder::accept(&priv_bytes, &msg1).expect("pop accept");
        assert_eq!(msg2.len(), RELAY_POP_MSG_LEN);

        let mut relay_ctrl = initiator.finalize(&msg2).expect("pop finalize");

        // Verify bidirectional encrypted transport over the PoP channel + replay rejection
        let control_msg = br#"{"op":"open_tunnel","tunnel_id":"tun_secret"}"#;
        let ct = relay_ctrl.seal_record(control_msg).expect("seal control");
        assert!(
            !String::from_utf8_lossy(&ct).contains("tun_secret"),
            "control plaintext must be encrypted"
        );
        let opened = daemon_ctrl.open_record(&ct).expect("open control");
        assert_eq!(opened, control_msg);
        assert!(daemon_ctrl.open_record(&ct).is_err(), "replay must fail");
    }

    #[test]
    fn relay_pop_rejects_all_low_order_x25519_points_wrong_key_tampering_and_replay() {
        let victim_kp = HostIdentityKeypair::generate().expect("victim");
        let attacker_kp = HostIdentityKeypair::generate().expect("attacker");
        let victim_pub = victim_kp.public_bytes().expect("victim pub");
        let victim_priv = victim_kp.private_bytes().expect("victim priv");
        let attacker_priv = attacker_kp.private_bytes().expect("attacker priv");

        // Reject all-zero AND non-zero low-order Curve25519 points (contributory DH check)
        let mut order4_point = [0u8; 32];
        order4_point[0] = 1;
        let order8_point: [u8; 32] = [
            0xe0, 0xeb, 0x7a, 0x7c, 0x3b, 0x41, 0xb8, 0xae, 0x16, 0x56, 0xe3, 0xfa, 0xf1, 0x9f,
            0xc4, 0x6a, 0xda, 0x09, 0x8d, 0xeb, 0x9c, 0x32, 0xb1, 0xfd, 0x86, 0x62, 0x05, 0x16,
            0x5f, 0x49, 0xb8, 0x00,
        ];
        let pairing_secret = generate_pairing_secret();
        for low_order_pk in [[0u8; 32], order4_point, order8_point] {
            assert!(
                RelayPopInitiator::new(&low_order_pk).is_err(),
                "low-order X25519 pubkey must be rejected by contributory DH"
            );
            assert!(
                ClientHandshake::new(&low_order_pk, &pairing_secret).is_err(),
                "low-order X25519 pubkey must also be rejected on client E2EE handshake"
            );
        }

        // Wrong private key cannot decrypt msg1
        let (initiator1, msg1) = RelayPopInitiator::new(&victim_pub).expect("pop init 1");
        assert!(RelayPopResponder::accept(&attacker_priv, &msg1).is_err());

        // Valid response for session 1
        let (_daemon_chan, valid_msg2) =
            RelayPopResponder::accept(&victim_priv, &msg1).expect("valid msg2");

        // Tampered response fails finalize
        let mut tampered = valid_msg2.clone();
        tampered[0] ^= 0x01;
        assert!(initiator1.finalize(&tampered).is_err());

        // Replaying session 1's valid msg2 into a fresh session 2 fails finalize
        let (initiator2, _msg1_b) = RelayPopInitiator::new(&victim_pub).expect("pop init 2");
        assert!(initiator2.finalize(&valid_msg2).is_err());

        // Wrong-length frames (including oversized) are rejected on both sides
        let (initiator3, _msg1_c) = RelayPopInitiator::new(&victim_pub).expect("pop init 3");
        let oversized = vec![0u8; RELAY_POP_MSG_LEN + 1];
        assert!(RelayPopResponder::accept(&victim_priv, &oversized).is_err());
        assert!(initiator3.finalize(&oversized).is_err());
    }

    #[test]
    fn relay_pop_is_domain_separated_from_client_e2ee_handshake() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");
        let secret = generate_pairing_secret();

        // A Relay PoP msg1 cannot be decrypted by the client E2EE DaemonHandshake (different pattern/prologue/PSK)
        let (_initiator, pop_msg1) = RelayPopInitiator::new(&pub_bytes).expect("pop init");
        assert!(matches!(
            DaemonHandshake::accept(&priv_bytes, &secret, &pop_msg1),
            Err(CryptoError::AuthenticationFailed)
        ));

        // Conversely, a ClientHandshake msg1 cannot be answered by RelayPopResponder
        let (_client_hs, client_msg1) =
            ClientHandshake::new(&pub_bytes, &secret).expect("client init");
        assert!(matches!(
            RelayPopResponder::accept(&priv_bytes, &client_msg1),
            Err(CryptoError::PopVerificationFailed)
        ));
    }

    #[test]
    fn tunnel_attach_hmac_sha256_verifies_and_rejects_forgery() {
        let secret = [0x42u8; 32];
        let other_secret = [0x43u8; 32];
        let nonce = [0x11u8; TUNNEL_ATTACH_NONCE_LEN];
        let other_nonce = [0x12u8; TUNNEL_ATTACH_NONCE_LEN];
        let tid = "tun_0123456789abcdef";

        let encoded = encode_key_32(&secret);
        assert_eq!(decode_key_32(&encoded).expect("decode secret"), secret);

        let mac = compute_tunnel_attach_mac(&secret, &nonce, tid);
        assert!(verify_tunnel_attach_mac(&secret, &nonce, tid, &mac));

        // Wrong secret, wrong nonce, wrong tunnel_id, tampered MAC, and wrong length must fail
        assert!(!verify_tunnel_attach_mac(&other_secret, &nonce, tid, &mac));
        assert!(!verify_tunnel_attach_mac(&secret, &other_nonce, tid, &mac));
        assert!(!verify_tunnel_attach_mac(
            &secret,
            &nonce,
            "tun_other",
            &mac
        ));
        let mut tampered = mac;
        tampered[31] ^= 0x01;
        assert!(!verify_tunnel_attach_mac(&secret, &nonce, tid, &tampered));
        assert!(!verify_tunnel_attach_mac(&secret, &nonce, tid, &mac[..31]));
    }

    #[test]
    fn pairing_qr_uri_roundtrip() {
        let kp = HostIdentityKeypair::generate().expect("keygen");
        let offer = PairingOffer {
            host_id: kp.host_id().expect("id"),
            hostname: "mac-mini.local".into(),
            daemon_pubkey_b64: kp.public_key_b64,
            direct_endpoints: vec!["ws://192.168.1.50:6868/ws".into()],
            relay_url: Some("wss://aws-relay.example.com:7878".into()),
            pairing_secret: "sec_123456".into(),
        };
        let uri = offer.to_qr_uri().expect("encode uri");
        assert!(uri.starts_with("orbit://pair/"));
        let parsed = PairingOffer::from_qr_uri(&uri).expect("decode uri");
        assert_eq!(parsed.hostname, "mac-mini.local");
        assert_eq!(parsed.relay_url, offer.relay_url);
    }

    #[test]
    fn generate_pairing_secret_is_32_byte_url_safe_base64_and_unique() {
        let s1 = generate_pairing_secret();
        let s2 = generate_pairing_secret();
        assert_eq!(s1.len(), 43);
        assert_eq!(s2.len(), 43);
        assert_ne!(s1, s2);
        let decoded = URL_SAFE_NO_PAD.decode(&s1).expect("valid base64url");
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn constant_time_eq_handles_equal_differing_and_mismatched_lengths() {
        assert!(constant_time_eq(b"secret-token-123", b"secret-token-123"));
        assert!(!constant_time_eq(b"secret-token-123", b"secret-token-124"));
        assert!(!constant_time_eq(b"secret-token-123", b"secret-token-12"));
        assert!(!constant_time_eq(b"short", b"longer-value"));
        assert!(constant_time_eq(b"", b""));
    }
}
