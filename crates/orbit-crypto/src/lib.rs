//! End-to-end encryption (`Noise_NK_25519_ChaChaPoly_SHA256`) and Multi-Host pairing credentials.
//!
//! Ensures the Relay is strictly zero-knowledge and allows strict `--allow-pubkey` admission control.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::{Builder, HandshakeState, TransportState};

pub const NOISE_PATTERN: &str = "Noise_NK_25519_ChaChaPoly_SHA256";
pub const NOISE_PROLOGUE: &[u8] = b"orbit-relay-e2ee/1";
const MAX_NOISE_CHUNK_PLAINTEXT: usize = 65_000;

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
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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

impl PairingOffer {
    pub fn to_qr_uri(&self) -> Result<String, CryptoError> {
        let json = serde_json::to_vec(self)?;
        Ok(format!("orbit://pair/{}", URL_SAFE_NO_PAD.encode(json)))
    }

    pub fn from_qr_uri(uri: &str) -> Result<Self, CryptoError> {
        let encoded = uri.strip_prefix("orbit://pair/").unwrap_or(uri);
        let raw = URL_SAFE_NO_PAD.decode(encoded.trim())?;
        Ok(serde_json::from_slice(&raw)?)
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
    pub fn new(daemon_pubkey: &[u8; 32]) -> Result<(Self, Vec<u8>), CryptoError> {
        let params = NOISE_PATTERN.parse()?;
        let mut state = Builder::new(params)
            .prologue(NOISE_PROLOGUE)
            .remote_public_key(daemon_pubkey)
            .build_initiator()?;
        let mut msg1 = vec![0u8; 128];
        let n = state.write_message(&[0x01, 0x01], &mut msg1)?;
        msg1.truncate(n);
        Ok((Self { state }, msg1))
    }

    pub fn finalize(mut self, msg2: &[u8]) -> Result<SecureChannel, CryptoError> {
        let mut payload = vec![0u8; 128];
        let _ = self.state.read_message(msg2, &mut payload)?;
        Ok(SecureChannel {
            transport: self.state.into_transport_mode()?,
        })
    }
}

impl DaemonHandshake {
    pub fn accept(
        daemon_privkey: &[u8; 32],
        msg1: &[u8],
    ) -> Result<(SecureChannel, Vec<u8>), CryptoError> {
        let params = NOISE_PATTERN.parse()?;
        let mut state: HandshakeState = Builder::new(params)
            .prologue(NOISE_PROLOGUE)
            .local_private_key(daemon_privkey)
            .build_responder()?;
        let mut offer = vec![0u8; 128];
        let _ = state.read_message(msg1, &mut offer)?;

        let mut msg2 = vec![0u8; 128];
        let n = state.write_message(&[0x01, 0x01], &mut msg2)?;
        msg2.truncate(n);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_nk_handshake_and_large_record_encryption() {
        let host_kp = HostIdentityKeypair::generate().expect("keygen");
        let pub_bytes = host_kp.public_bytes().expect("pub");
        let priv_bytes = host_kp.private_bytes().expect("priv");

        let (client_hs, msg1) = ClientHandshake::new(&pub_bytes).expect("client init");
        let (mut daemon_chan, msg2) =
            DaemonHandshake::accept(&priv_bytes, &msg1).expect("daemon accept");
        let mut client_chan = client_hs.finalize(&msg2).expect("client finalize");

        // Test multi-chunk large payload (> 65KB)
        let big_payload = vec![0x42u8; 150_000];
        let ciphertext = client_chan.seal_record(&big_payload).expect("seal");
        let decrypted = daemon_chan.open_record(&ciphertext).expect("open");
        assert_eq!(big_payload, decrypted);

        // Replay protection: replaying the exact same ciphertext must fail!
        assert!(daemon_chan.open_record(&ciphertext).is_err());
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
}
