//! Pure-Rust SHA-256 release checksum parsing and verification (`SHA256SUMS.txt`).
//!
//! Shared across `orbit-daemon`, `orbit-cli`, and `orbit-desktop` so every self-upgrade and
//! release-download path enforces identical cryptographic integrity checks with zero fallbacks.

use std::collections::HashMap;

use sha2::{Digest, Sha256};

/// Standard checksum manifest filename published alongside GitHub Release assets.
pub const SHA256SUMS_FILENAME: &str = "SHA256SUMS.txt";

#[derive(Debug, thiserror::Error)]
pub enum ChecksumError {
    #[error("SHA-256 entry for asset `{asset}` not found in {SHA256SUMS_FILENAME}")]
    MissingAsset { asset: String },
    #[error("malformed {SHA256SUMS_FILENAME} line {line}: {detail}")]
    InvalidLine { line: usize, detail: String },
    #[error("conflicting duplicate SHA-256 entry for `{asset}` on line {line}")]
    DuplicateConflictingAsset { asset: String, line: usize },
    #[error("invalid SHA-256 hex digest: `{raw}`")]
    InvalidDigest { raw: String },
    #[error("SHA-256 checksum mismatch for `{asset}`: expected {expected}, got {actual}")]
    Mismatch {
        asset: String,
        expected: String,
        actual: String,
    },
}

/// Incremental streaming SHA-256 hasher for verifying large release downloads in a single pass.
#[derive(Debug, Clone, Default)]
pub struct Sha256Hasher {
    inner: Sha256,
}

impl Sha256Hasher {
    pub fn new() -> Self {
        Self {
            inner: Sha256::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    pub fn finalize_bytes(self) -> [u8; 32] {
        self.inner.finalize().into()
    }

    pub fn finalize_hex(self) -> String {
        format!("{:x}", self.inner.finalize())
    }
}

/// Compute lowercase hex SHA-256 digest of an in-memory slice.
pub fn sha256_hex_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256Hasher::new();
    hasher.update(data);
    hasher.finalize_hex()
}

/// Parse a GNU `sha256sum` / `shasum -a 256` manifest (`<hex>  <filename>`, `<hex> *<filename>`,
/// or `<hex> <filename>`) into a map of normalized asset filename -> lowercase 64-char hex digest.
pub fn parse_sha256sums(content: &str) -> Result<HashMap<String, String>, ChecksumError> {
    let mut entries: HashMap<String, String> = HashMap::new();

    for (idx, raw_line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some(ws_pos) = trimmed.find(|c: char| c.is_ascii_whitespace()) else {
            return Err(ChecksumError::InvalidLine {
                line: line_no,
                detail: "missing filename column".to_string(),
            });
        };

        let raw_hash = &trimmed[..ws_pos];
        let raw_rest = trimmed[ws_pos..].trim_start();
        if raw_rest.is_empty() {
            return Err(ChecksumError::InvalidLine {
                line: line_no,
                detail: "empty filename column".to_string(),
            });
        }

        let valid_hash = validate_sha256_hex(raw_hash).map_err(|_| ChecksumError::InvalidLine {
            line: line_no,
            detail: format!("invalid 64-char hex digest `{raw_hash}`"),
        })?;

        let filename = normalize_manifest_filename(raw_rest);
        if filename.is_empty() {
            return Err(ChecksumError::InvalidLine {
                line: line_no,
                detail: "empty normalized filename".to_string(),
            });
        }

        if let Some(existing) = entries.get(filename) {
            if !existing.eq_ignore_ascii_case(valid_hash) {
                return Err(ChecksumError::DuplicateConflictingAsset {
                    asset: filename.to_string(),
                    line: line_no,
                });
            }
        } else {
            entries.insert(filename.to_string(), valid_hash.to_ascii_lowercase());
        }
    }

    Ok(entries)
}

/// Look up the expected lowercase 64-char hex SHA-256 digest for `asset_name` in `sums_content`.
pub fn find_expected_sha256(sums_content: &str, asset_name: &str) -> Result<String, ChecksumError> {
    let mut entries = parse_sha256sums(sums_content)?;
    let target = normalize_manifest_filename(asset_name.trim());
    entries
        .remove(target)
        .ok_or_else(|| ChecksumError::MissingAsset {
            asset: target.to_string(),
        })
}

/// Validate and compare expected and actual 64-char hex SHA-256 digests.
pub fn verify_sha256_hex(
    asset_name: &str,
    expected_hex: &str,
    actual_hex: &str,
) -> Result<(), ChecksumError> {
    let expected = validate_sha256_hex(expected_hex)?;
    let actual = validate_sha256_hex(actual_hex)?;
    if expected.eq_ignore_ascii_case(actual) {
        Ok(())
    } else {
        Err(ChecksumError::Mismatch {
            asset: asset_name.to_string(),
            expected: expected.to_ascii_lowercase(),
            actual: actual.to_ascii_lowercase(),
        })
    }
}

fn normalize_manifest_filename(raw: &str) -> &str {
    let stripped_binary = raw.strip_prefix('*').unwrap_or(raw);
    stripped_binary
        .strip_prefix("./")
        .unwrap_or(stripped_binary)
        .trim()
}

fn validate_sha256_hex(raw: &str) -> Result<&str, ChecksumError> {
    let trimmed = raw.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ChecksumError::InvalidDigest {
            raw: trimmed.to_string(),
        });
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_verify_standard_gnu_and_bsd_formats() {
        let payload = b"orbit release binary payload";
        let expected = sha256_hex_bytes(payload);
        let upper = expected.to_ascii_uppercase();
        let other_hash = sha256_hex_bytes(b"other asset");

        let manifest = format!(
            "# Orbit release checksums\r\n\
             \r\n\
             {expected}  orbit-cli-linux-x64\r\n\
             {upper} *Orbit-macOS-AppleSilicon.dmg\n\
             {other_hash} ./Orbit-Linux-arm64.deb\n"
        );

        assert_eq!(
            find_expected_sha256(&manifest, "orbit-cli-linux-x64").expect("linux cli"),
            expected
        );
        assert_eq!(
            find_expected_sha256(&manifest, "Orbit-macOS-AppleSilicon.dmg").expect("macos dmg"),
            expected
        );
        assert_eq!(
            find_expected_sha256(&manifest, "Orbit-Linux-arm64.deb").expect("linux deb"),
            other_hash
        );

        verify_sha256_hex("orbit-cli-linux-x64", &upper, &expected)
            .expect("case-insensitive hex match must succeed");
    }

    #[test]
    fn streaming_hasher_detects_tampering() {
        let payload = b"genuine orbit installer bytes 0123456789";
        let mut hasher = Sha256Hasher::new();
        hasher.update(&payload[..12]);
        hasher.update(&payload[12..]);
        let expected = hasher.finalize_hex();

        verify_sha256_hex(
            "Orbit-Windows-x64-setup.exe",
            &expected,
            &sha256_hex_bytes(payload),
        )
        .expect("matching digest must succeed");

        let mut tampered = payload.to_vec();
        tampered[0] ^= 0x01;
        let err = verify_sha256_hex(
            "Orbit-Windows-x64-setup.exe",
            &expected,
            &sha256_hex_bytes(&tampered),
        )
        .expect_err("tampered payload must fail verification");
        assert!(matches!(err, ChecksumError::Mismatch { .. }));
    }

    #[test]
    fn rejects_missing_asset_malformed_lines_and_conflicting_duplicates() {
        let valid_hash = sha256_hex_bytes(b"abc");
        let other_hash = sha256_hex_bytes(b"def");

        let missing = find_expected_sha256(&format!("{valid_hash}  other.bin\n"), "target.bin")
            .expect_err("missing asset must fail");
        assert!(matches!(missing, ChecksumError::MissingAsset { .. }));

        let malformed = parse_sha256sums("not-a-hex-digest  orbit-cli-linux-x64\n")
            .expect_err("malformed digest must fail");
        assert!(matches!(
            malformed,
            ChecksumError::InvalidLine { line: 1, .. }
        ));

        let conflict = parse_sha256sums(&format!(
            "{valid_hash}  orbit-cli-linux-x64\n{other_hash}  orbit-cli-linux-x64\n"
        ))
        .expect_err("conflicting duplicate entries must fail");
        assert!(matches!(
            conflict,
            ChecksumError::DuplicateConflictingAsset { line: 2, .. }
        ));
    }
}
