//! Zero-Knowledge End-to-End Encrypted Cloud Vault (`orbit-cloud-vault/v1`).
//!
//! Uses Argon2id (OWASP interactive baseline: 19 MiB, 2 iterations, 1 lane) to derive a
//! 256-bit master key from the user's Sync Passphrase / Recovery Key, Zstd compression, and
//! AES-256-GCM authenticated encryption with the format string bound as AAD.
//!
//! Remote storage providers (e.g. GitHub Secret Gist) only ever store opaque ciphertext.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub const CLOUD_VAULT_ENVELOPE_FORMAT: &str = "orbit-cloud-vault/v1";
pub const CLOUD_VAULT_KDF: &str = "argon2id";
pub const CLOUD_VAULT_CIPHER: &str = "aes-256-gcm";

/// OWASP 2024+ interactive Argon2id baseline (~19 MiB RAM, 2 iterations, 1 thread).
pub const DEFAULT_ARGON2_M_COST_KIB: u32 = 19_456;
pub const DEFAULT_ARGON2_T_COST: u32 = 2;
pub const DEFAULT_ARGON2_P_COST: u32 = 1;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// Hard cap on decompressed plaintext size (16 MiB) to defend against zip-bomb payloads.
const MAX_VAULT_PLAINTEXT_BYTES: usize = 16 * 1024 * 1024;
/// Upper bound on Argon2 memory cost accepted from remote envelopes (128 MiB) to prevent DoS.
const MAX_ARGON2_M_COST_KIB: u32 = 131_072;
const MAX_ARGON2_T_COST: u32 = 16;
const MAX_ARGON2_P_COST: u32 = 8;

#[derive(Debug, thiserror::Error)]
pub enum VaultCryptoError {
    #[error("passphrase must not be empty")]
    EmptyPassphrase,
    #[error("unsupported vault envelope format: {0}")]
    UnsupportedFormat(String),
    #[error("unsupported vault KDF: {0}")]
    UnsupportedKdf(String),
    #[error("unsupported vault cipher: {0}")]
    UnsupportedCipher(String),
    #[error("invalid KDF parameters: {0}")]
    InvalidKdfParams(String),
    #[error("invalid base64 encoding in vault envelope: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("invalid {field} length: expected {expected} bytes, got {actual}")]
    InvalidFieldLength {
        field: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("key derivation failed: {0}")]
    KeyDerivation(String),
    #[error("failed to encrypt vault payload")]
    EncryptionFailed,
    #[error("failed to decrypt vault payload (wrong sync passphrase or tampered data)")]
    DecryptionFailed,
    #[error("compression error: {0}")]
    Compression(String),
    #[error("vault payload exceeds safety size limit")]
    PayloadTooLarge,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultKdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for VaultKdfParams {
    fn default() -> Self {
        Self {
            m_cost: DEFAULT_ARGON2_M_COST_KIB,
            t_cost: DEFAULT_ARGON2_T_COST,
            p_cost: DEFAULT_ARGON2_P_COST,
        }
    }
}

impl VaultKdfParams {
    pub fn validate(self) -> Result<(), VaultCryptoError> {
        if self.m_cost < 8 || self.m_cost > MAX_ARGON2_M_COST_KIB {
            return Err(VaultCryptoError::InvalidKdfParams(format!(
                "m_cost {} outside [8, {}]",
                self.m_cost, MAX_ARGON2_M_COST_KIB
            )));
        }
        if self.t_cost == 0 || self.t_cost > MAX_ARGON2_T_COST {
            return Err(VaultCryptoError::InvalidKdfParams(format!(
                "t_cost {} outside [1, {}]",
                self.t_cost, MAX_ARGON2_T_COST
            )));
        }
        if self.p_cost == 0 || self.p_cost > MAX_ARGON2_P_COST {
            return Err(VaultCryptoError::InvalidKdfParams(format!(
                "p_cost {} outside [1, {}]",
                self.p_cost, MAX_ARGON2_P_COST
            )));
        }
        Ok(())
    }
}

/// Self-describing encrypted envelope stored in the user's remote cloud storage (e.g. GitHub Secret Gist).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedVaultEnvelope {
    pub format: String,
    pub kdf: String,
    pub kdf_params: VaultKdfParams,
    pub salt_b64: String,
    pub nonce_b64: String,
    pub cipher: String,
    pub ciphertext_b64: String,
    pub updated_at_ms: i64,
}

/// 256-bit derived master key paired with the salt & KDF params used to derive it.
/// Kept locally on unlocked devices so subsequent syncs do not re-prompt for the passphrase.
#[derive(Clone)]
pub struct DerivedVaultKey {
    key: Zeroizing<[u8; KEY_LEN]>,
    pub salt_b64: String,
    pub kdf_params: VaultKdfParams,
}

impl DerivedVaultKey {
    pub fn from_raw(
        key_bytes: [u8; KEY_LEN],
        salt_b64: String,
        kdf_params: VaultKdfParams,
    ) -> Result<Self, VaultCryptoError> {
        kdf_params.validate()?;
        let _ = decode_fixed::<SALT_LEN>(&salt_b64, "salt")?;
        Ok(Self {
            key: Zeroizing::new(key_bytes),
            salt_b64,
            kdf_params,
        })
    }

    pub fn from_b64(
        key_b64: &str,
        salt_b64: String,
        kdf_params: VaultKdfParams,
    ) -> Result<Self, VaultCryptoError> {
        let key_bytes = decode_fixed::<KEY_LEN>(key_b64, "key")?;
        Self::from_raw(key_bytes, salt_b64, kdf_params)
    }

    pub fn key_b64(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(*self.key))
    }

    pub fn matches_envelope(&self, env: &EncryptedVaultEnvelope) -> bool {
        self.salt_b64 == env.salt_b64 && self.kdf_params == env.kdf_params
    }
}

/// Generate a fresh random 16-byte salt encoded as URL-safe base64.
pub fn generate_vault_salt_b64() -> String {
    let mut salt = [0u8; SALT_LEN];
    rand::rng().fill_bytes(&mut salt);
    URL_SAFE_NO_PAD.encode(salt)
}

/// Generate a human-readable high-entropy Recovery Key (`orbit-XXXX-XXXX-XXXX-XXXX-XXXX`, 100+ bits).
pub fn generate_recovery_passphrase() -> String {
    // Crockford-inspired unambiguous uppercase alphabet (no 0/O/1/I/L)
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let mut raw = [0u8; 20];
    rand::rng().fill_bytes(&mut raw);
    let chars: Vec<char> = raw
        .iter()
        .map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char)
        .collect();
    format!(
        "orbit-{}-{}-{}-{}-{}",
        chars[0..4].iter().collect::<String>(),
        chars[4..8].iter().collect::<String>(),
        chars[8..12].iter().collect::<String>(),
        chars[12..16].iter().collect::<String>(),
        chars[16..20].iter().collect::<String>(),
    )
}

/// Derive a 256-bit master key from `passphrase`, `salt_b64`, and Argon2id parameters.
pub fn derive_vault_key(
    passphrase: &str,
    salt_b64: &str,
    kdf_params: VaultKdfParams,
) -> Result<DerivedVaultKey, VaultCryptoError> {
    let trimmed = passphrase.trim();
    if trimmed.is_empty() {
        return Err(VaultCryptoError::EmptyPassphrase);
    }
    kdf_params.validate()?;
    let salt = decode_fixed::<SALT_LEN>(salt_b64, "salt")?;
    let params = Params::new(
        kdf_params.m_cost,
        kdf_params.t_cost,
        kdf_params.p_cost,
        Some(KEY_LEN),
    )
    .map_err(|e| VaultCryptoError::InvalidKdfParams(e.to_string()))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; KEY_LEN]);
    argon2
        .hash_password_into(trimmed.as_bytes(), &salt, key.as_mut())
        .map_err(|e| VaultCryptoError::KeyDerivation(e.to_string()))?;

    Ok(DerivedVaultKey {
        key,
        salt_b64: salt_b64.trim().to_string(),
        kdf_params,
    })
}

/// Derive a brand-new vault key with a freshly generated random salt and default Argon2id parameters.
pub fn derive_new_vault_key(passphrase: &str) -> Result<DerivedVaultKey, VaultCryptoError> {
    let salt_b64 = generate_vault_salt_b64();
    derive_vault_key(passphrase, &salt_b64, VaultKdfParams::default())
}

/// Compress and encrypt arbitrary plaintext bytes into an `EncryptedVaultEnvelope` using a fresh 96-bit nonce.
pub fn encrypt_vault_bytes(
    key: &DerivedVaultKey,
    plaintext: &[u8],
    updated_at_ms: i64,
) -> Result<EncryptedVaultEnvelope, VaultCryptoError> {
    if plaintext.len() > MAX_VAULT_PLAINTEXT_BYTES {
        return Err(VaultCryptoError::PayloadTooLarge);
    }
    let compressed = zstd::bulk::compress(plaintext, 3)
        .map_err(|e| VaultCryptoError::Compression(e.to_string()))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(key.key.as_ref())
        .map_err(|_| VaultCryptoError::EncryptionFailed)?;
    let aad = build_aad(CLOUD_VAULT_ENVELOPE_FORMAT, &key.salt_b64, key.kdf_params);
    let ciphertext = cipher
        .encrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &compressed,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| VaultCryptoError::EncryptionFailed)?;

    Ok(EncryptedVaultEnvelope {
        format: CLOUD_VAULT_ENVELOPE_FORMAT.to_string(),
        kdf: CLOUD_VAULT_KDF.to_string(),
        kdf_params: key.kdf_params,
        salt_b64: key.salt_b64.clone(),
        nonce_b64: URL_SAFE_NO_PAD.encode(nonce_bytes),
        cipher: CLOUD_VAULT_CIPHER.to_string(),
        ciphertext_b64: URL_SAFE_NO_PAD.encode(ciphertext),
        updated_at_ms,
    })
}

/// Decrypt and decompress an `EncryptedVaultEnvelope` using a previously derived key.
pub fn decrypt_vault_bytes(
    key: &DerivedVaultKey,
    envelope: &EncryptedVaultEnvelope,
) -> Result<Vec<u8>, VaultCryptoError> {
    validate_envelope_header(envelope)?;
    let nonce_bytes = decode_fixed::<NONCE_LEN>(&envelope.nonce_b64, "nonce")?;
    let ciphertext = URL_SAFE_NO_PAD.decode(envelope.ciphertext_b64.trim())?;

    let cipher = Aes256Gcm::new_from_slice(key.key.as_ref())
        .map_err(|_| VaultCryptoError::DecryptionFailed)?;
    let aad = build_aad(&envelope.format, &envelope.salt_b64, envelope.kdf_params);
    let compressed = cipher
        .decrypt(
            Nonce::from_slice(&nonce_bytes),
            Payload {
                msg: &ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| VaultCryptoError::DecryptionFailed)?;

    let plaintext = zstd::bulk::decompress(&compressed, MAX_VAULT_PLAINTEXT_BYTES)
        .map_err(|e| VaultCryptoError::Compression(e.to_string()))?;
    Ok(plaintext)
}

/// Serialize `value` as JSON and encrypt it into an `EncryptedVaultEnvelope`.
pub fn encrypt_vault_json<T: Serialize>(
    key: &DerivedVaultKey,
    value: &T,
    updated_at_ms: i64,
) -> Result<EncryptedVaultEnvelope, VaultCryptoError> {
    let raw = serde_json::to_vec(value)?;
    encrypt_vault_bytes(key, &raw, updated_at_ms)
}

/// Decrypt an `EncryptedVaultEnvelope` and deserialize its JSON body into `T`.
pub fn decrypt_vault_json<T: for<'de> Deserialize<'de>>(
    key: &DerivedVaultKey,
    envelope: &EncryptedVaultEnvelope,
) -> Result<T, VaultCryptoError> {
    let raw = decrypt_vault_bytes(key, envelope)?;
    Ok(serde_json::from_slice(&raw)?)
}

/// Validate header invariants of a remote `EncryptedVaultEnvelope` before deriving keys.
pub fn validate_envelope_header(envelope: &EncryptedVaultEnvelope) -> Result<(), VaultCryptoError> {
    if envelope.format != CLOUD_VAULT_ENVELOPE_FORMAT {
        return Err(VaultCryptoError::UnsupportedFormat(envelope.format.clone()));
    }
    if envelope.kdf != CLOUD_VAULT_KDF {
        return Err(VaultCryptoError::UnsupportedKdf(envelope.kdf.clone()));
    }
    if envelope.cipher != CLOUD_VAULT_CIPHER {
        return Err(VaultCryptoError::UnsupportedCipher(envelope.cipher.clone()));
    }
    envelope.kdf_params.validate()?;
    let _ = decode_fixed::<SALT_LEN>(&envelope.salt_b64, "salt")?;
    let _ = decode_fixed::<NONCE_LEN>(&envelope.nonce_b64, "nonce")?;
    Ok(())
}

fn build_aad(format: &str, salt_b64: &str, kdf_params: VaultKdfParams) -> String {
    format!(
        "{format}|{}|{}|{}|{}",
        salt_b64.trim(),
        kdf_params.m_cost,
        kdf_params.t_cost,
        kdf_params.p_cost
    )
}

fn decode_fixed<const N: usize>(
    b64: &str,
    field: &'static str,
) -> Result<[u8; N], VaultCryptoError> {
    let raw = URL_SAFE_NO_PAD.decode(b64.trim())?;
    let actual = raw.len();
    raw.try_into()
        .map_err(|_| VaultCryptoError::InvalidFieldLength {
            field,
            expected: N,
            actual,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct SamplePayload {
        hosts: Vec<String>,
        revision: u64,
    }

    #[test]
    fn encrypt_decrypt_roundtrip_and_wrong_passphrase_rejected() {
        let key = derive_new_vault_key("correct-horse-battery-staple").expect("derive key");
        let payload = SamplePayload {
            hosts: vec!["host_a".into(), "host_b".into()],
            revision: 7,
        };
        let env = encrypt_vault_json(&key, &payload, 1_763_700_000_000).expect("encrypt");
        assert_eq!(env.format, CLOUD_VAULT_ENVELOPE_FORMAT);
        assert_eq!(env.cipher, CLOUD_VAULT_CIPHER);

        // Same passphrase + envelope salt recovers identical key & plaintext
        let rederived = derive_vault_key(
            "correct-horse-battery-staple",
            &env.salt_b64,
            env.kdf_params,
        )
        .expect("rederive");
        let decoded: SamplePayload = decrypt_vault_json(&rederived, &env).expect("decrypt");
        assert_eq!(decoded, payload);

        // Wrong passphrase must fail authenticated decryption
        let wrong =
            derive_vault_key("wrong-passphrase", &env.salt_b64, env.kdf_params).expect("wrong key");
        assert!(matches!(
            decrypt_vault_json::<SamplePayload>(&wrong, &env),
            Err(VaultCryptoError::DecryptionFailed)
        ));
    }

    #[test]
    fn tampered_ciphertext_or_aad_metadata_fails() {
        let key = derive_new_vault_key("my-sync-secret").expect("derive");
        let env = encrypt_vault_bytes(&key, b"{\"secret\":true}", 42).expect("encrypt");

        // Tamper ciphertext
        let mut bad_ct = env.clone();
        let mut raw = URL_SAFE_NO_PAD
            .decode(&bad_ct.ciphertext_b64)
            .expect("decode ct");
        raw[0] ^= 0xFF;
        bad_ct.ciphertext_b64 = URL_SAFE_NO_PAD.encode(raw);
        assert!(matches!(
            decrypt_vault_bytes(&key, &bad_ct),
            Err(VaultCryptoError::DecryptionFailed)
        ));

        // Tamper salt (bound in AAD)
        let mut bad_salt = env.clone();
        bad_salt.salt_b64 = generate_vault_salt_b64();
        assert!(matches!(
            decrypt_vault_bytes(&key, &bad_salt),
            Err(VaultCryptoError::DecryptionFailed)
        ));
    }

    #[test]
    fn fresh_nonce_on_every_encryption_and_recovery_passphrase_format() {
        let key = derive_new_vault_key("same-passphrase").expect("derive");
        let e1 = encrypt_vault_bytes(&key, b"hello world", 1).expect("e1");
        let e2 = encrypt_vault_bytes(&key, b"hello world", 2).expect("e2");
        assert_eq!(e1.salt_b64, e2.salt_b64);
        assert_ne!(e1.nonce_b64, e2.nonce_b64);
        assert_ne!(e1.ciphertext_b64, e2.ciphertext_b64);

        let rec1 = generate_recovery_passphrase();
        let rec2 = generate_recovery_passphrase();
        assert!(rec1.starts_with("orbit-"));
        assert_eq!(rec1.split('-').count(), 6);
        assert_ne!(rec1, rec2);
    }
}
