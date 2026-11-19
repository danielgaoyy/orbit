//! Chunked remote attachment staging (`WireMessage::StageAttachmentChunk`) into `$ORBIT_HOME/attachments/`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use tokio::sync::Mutex;
use uuid::Uuid;

pub const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_ATTACHMENT_CHUNKS: u32 = 64;
pub const MAX_CHUNK_RAW_BYTES: usize = 1024 * 1024;
const PENDING_UPLOAD_TTL: Duration = Duration::from_secs(10 * 60);
const ORPHAN_PART_TTL: Duration = Duration::from_secs(60 * 60);
const STAGED_FILE_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedChunkResult {
    pub upload_id: String,
    pub chunk_index: u32,
    pub completed: bool,
    pub path: Option<String>,
    pub filename: Option<String>,
    pub size_bytes: Option<u64>,
}

struct PendingUpload {
    next_chunk: u32,
    total_chunks: u32,
    total_bytes: usize,
    clean_name: String,
    temp_path: PathBuf,
    updated_at: Instant,
}

pub struct AttachmentStagingManager {
    attachments_dir: PathBuf,
    pending: Arc<Mutex<HashMap<String, PendingUpload>>>,
}

impl AttachmentStagingManager {
    pub fn new(home_dir: &Path) -> Self {
        Self {
            attachments_dir: home_dir.join("attachments"),
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn attachments_dir(&self) -> &Path {
        &self.attachments_dir
    }

    pub async fn ingest_chunk(
        &self,
        upload_id: String,
        filename: String,
        chunk_index: u32,
        total_chunks: u32,
        data_b64: String,
    ) -> Result<StagedChunkResult, String> {
        let upload_id = sanitize_upload_id(&upload_id)?;
        if total_chunks == 0 || total_chunks > MAX_ATTACHMENT_CHUNKS {
            return Err(format!(
                "invalid total_chunks {total_chunks} (must be 1..={MAX_ATTACHMENT_CHUNKS})"
            ));
        }
        if chunk_index >= total_chunks {
            return Err(format!(
                "chunk_index {chunk_index} out of bounds for total_chunks {total_chunks}"
            ));
        }

        let decoded = B64
            .decode(data_b64.trim())
            .map_err(|e| format!("invalid base64 attachment chunk: {e}"))?;
        if decoded.len() > MAX_CHUNK_RAW_BYTES {
            return Err(format!(
                "chunk size {} exceeds {MAX_CHUNK_RAW_BYTES} byte limit",
                decoded.len()
            ));
        }

        let clean_name = sanitize_filename(&filename);
        let attachments_dir = self.attachments_dir.clone();
        let mut pending = self.pending.lock().await;

        // Purge expired in-flight uploads before checking this upload
        let mut expired_paths = Vec::new();
        pending.retain(|_, entry| {
            if entry.updated_at.elapsed() >= PENDING_UPLOAD_TTL {
                expired_paths.push(entry.temp_path.clone());
                false
            } else {
                true
            }
        });

        if chunk_index == 0 {
            if let Some(prev) = pending.remove(&upload_id) {
                expired_paths.push(prev.temp_path);
            }
            if decoded.len() > MAX_ATTACHMENT_BYTES {
                return Err("Attachment exceeds 25 MiB limit".into());
            }

            let temp_path = attachments_dir
                .join(".tmp")
                .join(format!("{upload_id}.part"));
            let is_single_chunk = total_chunks == 1;
            let dir_clone = attachments_dir.clone();
            let temp_clone = temp_path.clone();
            let name_clone = clean_name.clone();
            let bytes_len = decoded.len();

            let finalized =
                tokio::task::spawn_blocking(move || -> Result<Option<PathBuf>, String> {
                    for old in expired_paths {
                        let _ = fs::remove_file(old);
                    }
                    ensure_private_dirs(&dir_clone)?;
                    write_part_file(&temp_clone, &decoded, true)?;
                    if is_single_chunk {
                        let dest = finalize_part_file(&dir_clone, &temp_clone, &name_clone)?;
                        cleanup_expired_files(&dir_clone);
                        Ok(Some(dest))
                    } else {
                        Ok(None)
                    }
                })
                .await
                .map_err(|e| format!("attachment staging task error: {e}"))??;

            if let Some(final_path) = finalized {
                return Ok(StagedChunkResult {
                    upload_id,
                    chunk_index,
                    completed: true,
                    path: Some(final_path.to_string_lossy().to_string()),
                    filename: Some(clean_name),
                    size_bytes: Some(bytes_len as u64),
                });
            }

            pending.insert(
                upload_id.clone(),
                PendingUpload {
                    next_chunk: 1,
                    total_chunks,
                    total_bytes: bytes_len,
                    clean_name: clean_name.clone(),
                    temp_path,
                    updated_at: Instant::now(),
                },
            );

            Ok(StagedChunkResult {
                upload_id,
                chunk_index,
                completed: false,
                path: None,
                filename: None,
                size_bytes: None,
            })
        } else {
            let Some(mut entry) = pending.remove(&upload_id) else {
                cleanup_paths_async(expired_paths).await;
                return Err(format!(
                    "unknown or expired attachment upload `{upload_id}`"
                ));
            };

            if chunk_index != entry.next_chunk || total_chunks != entry.total_chunks {
                expired_paths.push(entry.temp_path);
                cleanup_paths_async(expired_paths).await;
                return Err(format!(
                    "out-of-order chunk {chunk_index}/{total_chunks} (expected {}/{})",
                    entry.next_chunk, entry.total_chunks
                ));
            }

            let new_total = entry.total_bytes.saturating_add(decoded.len());
            if new_total > MAX_ATTACHMENT_BYTES {
                expired_paths.push(entry.temp_path);
                cleanup_paths_async(expired_paths).await;
                return Err("Attachment exceeds 25 MiB limit".into());
            }

            let is_last = chunk_index + 1 == entry.total_chunks;
            let dir_clone = attachments_dir.clone();
            let temp_clone = entry.temp_path.clone();
            let name_clone = entry.clean_name.clone();

            let io_res = tokio::task::spawn_blocking(move || -> Result<Option<PathBuf>, String> {
                for old in expired_paths {
                    let _ = fs::remove_file(old);
                }
                write_part_file(&temp_clone, &decoded, false)?;
                if is_last {
                    let dest = finalize_part_file(&dir_clone, &temp_clone, &name_clone)?;
                    cleanup_expired_files(&dir_clone);
                    Ok(Some(dest))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| format!("attachment staging task error: {e}"));

            match io_res {
                Ok(Ok(Some(final_path))) => Ok(StagedChunkResult {
                    upload_id,
                    chunk_index,
                    completed: true,
                    path: Some(final_path.to_string_lossy().to_string()),
                    filename: Some(entry.clean_name),
                    size_bytes: Some(new_total as u64),
                }),
                Ok(Ok(None)) => {
                    entry.next_chunk += 1;
                    entry.total_bytes = new_total;
                    entry.updated_at = Instant::now();
                    pending.insert(upload_id.clone(), entry);
                    Ok(StagedChunkResult {
                        upload_id,
                        chunk_index,
                        completed: false,
                        path: None,
                        filename: None,
                        size_bytes: None,
                    })
                }
                Ok(Err(e)) => {
                    let _ = fs::remove_file(&entry.temp_path);
                    Err(e)
                }
                Err(e) => {
                    let _ = fs::remove_file(&entry.temp_path);
                    Err(e)
                }
            }
        }
    }
}

async fn cleanup_paths_async(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        for p in paths {
            let _ = fs::remove_file(p);
        }
    })
    .await;
}

fn sanitize_upload_id(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err("invalid upload_id length".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err("invalid characters in upload_id".into());
    }
    Ok(trimmed.to_string())
}

pub fn sanitize_filename(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
    let mut mapped: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();

    if mapped.len() > 120 {
        if let Some(dot_idx) = mapped.rfind('.') {
            let ext = &mapped[dot_idx..];
            if ext.len() <= 16 {
                let prefix_len = 120usize.saturating_sub(ext.len());
                mapped = format!("{}{}", &mapped[..prefix_len], ext);
            } else {
                mapped.truncate(120);
            }
        } else {
            mapped.truncate(120);
        }
    }

    let trimmed_underscores = mapped.trim_matches('_');
    if trimmed_underscores.is_empty() || trimmed_underscores == "." || trimmed_underscores == ".." {
        return "attachment.bin".to_string();
    }
    if trimmed_underscores.starts_with('.') {
        return format!("file{trimmed_underscores}");
    }
    trimmed_underscores.to_string()
}

fn ensure_private_dirs(attachments_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(attachments_dir)
        .map_err(|e| format!("failed to create attachments directory: {e}"))?;
    let tmp_dir = attachments_dir.join(".tmp");
    fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("failed to create attachments temp directory: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(attachments_dir, fs::Permissions::from_mode(0o700));
        let _ = fs::set_permissions(&tmp_dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

fn write_part_file(path: &Path, bytes: &[u8], create_new: bool) -> Result<(), String> {
    let mut opts = OpenOptions::new();
    if create_new {
        opts.create(true).write(true).truncate(true);
    } else {
        opts.append(true);
    }
    let mut file = opts
        .open(path)
        .map_err(|e| format!("failed to open attachment part file: {e}"))?;
    #[cfg(unix)]
    if create_new {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    file.write_all(bytes)
        .map_err(|e| format!("failed to write attachment chunk: {e}"))?;
    file.flush()
        .map_err(|e| format!("failed to flush attachment chunk: {e}"))?;
    Ok(())
}

fn finalize_part_file(
    attachments_dir: &Path,
    temp_path: &Path,
    clean_name: &str,
) -> Result<PathBuf, String> {
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let short_uuid = &Uuid::new_v4().simple().to_string()[..8];
    let dest = attachments_dir.join(format!("{ts_ms}_{short_uuid}_{clean_name}"));
    fs::rename(temp_path, &dest)
        .map_err(|e| format!("failed to finalize staged attachment: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dest, fs::Permissions::from_mode(0o600));
    }
    Ok(dest)
}

fn cleanup_expired_files(attachments_dir: &Path) {
    let now = SystemTime::now();
    if let Ok(entries) = fs::read_dir(attachments_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(meta) = entry.metadata()
                && let Ok(modified) = meta.modified()
                && now.duration_since(modified).unwrap_or_default() >= STAGED_FILE_TTL
            {
                let _ = fs::remove_file(path);
            }
        }
    }
    let tmp_dir = attachments_dir.join(".tmp");
    if let Ok(entries) = fs::read_dir(&tmp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Ok(meta) = entry.metadata()
                && let Ok(modified) = meta.modified()
                && now.duration_since(modified).unwrap_or_default() >= ORPHAN_PART_TTL
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_att_test_{tag}_{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[tokio::test]
    async fn single_and_multichunk_attachment_staging_roundtrip() {
        let home = temp_home("multi");
        let mgr = AttachmentStagingManager::new(&home);

        // Single-chunk PNG
        let png = b"\x89PNG\r\n\x1a\nsmall-png-bytes";
        let res1 = mgr
            .ingest_chunk(
                "up_single".into(),
                "../../etc/shot.png".into(),
                0,
                1,
                B64.encode(png),
            )
            .await
            .expect("single chunk");
        assert!(res1.completed);
        let p1 = PathBuf::from(res1.path.expect("path"));
        assert!(p1.starts_with(home.join("attachments")));
        assert_eq!(res1.filename.as_deref(), Some("shot.png"));
        assert_eq!(fs::read(&p1).expect("read p1"), png);

        // 1.2 MiB split across 3 chunks (512 KiB each)
        let payload: Vec<u8> = (0..(1_200 * 1024)).map(|i| (i % 251) as u8).collect();
        let chunk_size = 512 * 1024;
        let slices: Vec<&[u8]> = payload.chunks(chunk_size).collect();
        assert_eq!(slices.len(), 3);

        for (idx, slice) in slices.iter().enumerate() {
            let res = mgr
                .ingest_chunk(
                    "up_multi_1".into(),
                    "archive/测试报告.bin".into(),
                    idx as u32,
                    slices.len() as u32,
                    B64.encode(slice),
                )
                .await
                .expect("ingest chunk");
            if idx + 1 < slices.len() {
                assert!(!res.completed);
                assert!(res.path.is_none());
            } else {
                assert!(res.completed);
                let final_path = PathBuf::from(res.path.expect("final path"));
                assert_eq!(res.size_bytes, Some(payload.len() as u64));
                assert_eq!(fs::read(&final_path).expect("read final"), payload);
            }
        }

        // Verify no leftover `.part` files
        let leftover: Vec<_> = fs::read_dir(home.join("attachments/.tmp"))
            .expect("read tmp")
            .flatten()
            .collect();
        assert!(leftover.is_empty());

        let _ = fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn rejects_out_of_order_and_oversized_and_traversal_names() {
        let home = temp_home("guards");
        let mgr = AttachmentStagingManager::new(&home);

        assert_eq!(sanitize_filename("../../.ssh/id_rsa"), "id_rsa");
        assert_eq!(sanitize_filename("../.."), "attachment.bin");
        assert_eq!(sanitize_filename("截图.png"), "file.png");

        // Out-of-order chunk 1 before chunk 0
        let err = mgr
            .ingest_chunk("up_bad".into(), "a.png".into(), 1, 2, B64.encode(b"hi"))
            .await
            .expect_err("must reject missing chunk 0");
        assert!(err.contains("unknown or expired"));

        // Path traversal upload_id rejected
        let bad_id = mgr
            .ingest_chunk("../bad".into(), "a.png".into(), 0, 1, B64.encode(b"hi"))
            .await
            .expect_err("must reject bad upload_id");
        assert!(bad_id.contains("upload_id"));

        // Over 25 MiB across chunks rejected and cleans temp file
        let big_chunk = vec![7u8; 1024 * 1024];
        let b64_big = B64.encode(&big_chunk);
        for idx in 0..25u32 {
            mgr.ingest_chunk("up_25m".into(), "huge.bin".into(), idx, 26, b64_big.clone())
                .await
                .expect("within 25 MiB");
        }
        let over_err = mgr
            .ingest_chunk("up_25m".into(), "huge.bin".into(), 25, 26, b64_big)
            .await
            .expect_err("26 MiB must fail");
        assert!(over_err.contains("25 MiB"));
        assert!(!home.join("attachments/.tmp/up_25m.part").exists());

        let _ = fs::remove_dir_all(&home);
    }
}
