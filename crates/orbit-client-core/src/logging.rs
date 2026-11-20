//! Lightweight, zero-dependency size-rotating file log writer shared by `orbit daemon` and
//! `orbit-desktop` so diagnostic logs stay bounded on disk (`<= max_bytes * (max_backups + 1)`).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const DEFAULT_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const DEFAULT_LOG_MAX_BACKUPS: usize = 5;

struct RotatingState {
    path: PathBuf,
    max_bytes: u64,
    max_backups: usize,
    file: Option<File>,
    written_bytes: u64,
}

impl RotatingState {
    fn open(path: PathBuf, max_bytes: u64, max_backups: usize) -> io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        let written_bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_bytes: max_bytes.max(4096),
            max_backups,
            file: Some(file),
            written_bytes,
        })
    }

    fn rotate_if_needed(&mut self, incoming: usize) -> io::Result<()> {
        if self.written_bytes.saturating_add(incoming as u64) <= self.max_bytes {
            return Ok(());
        }
        // Close active handle before renaming on all platforms
        self.file = None;

        if self.max_backups > 0 {
            for idx in (1..self.max_backups).rev() {
                let src = rotated_path(&self.path, idx);
                let dst = rotated_path(&self.path, idx + 1);
                if src.exists() {
                    let _ = fs::rename(&src, &dst);
                }
            }
            let first = rotated_path(&self.path, 1);
            if self.path.exists() {
                let _ = fs::rename(&self.path, &first);
            }
        } else if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }

        let fresh = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)?;
        self.file = Some(fresh);
        self.written_bytes = 0;
        Ok(())
    }

    fn write_bytes(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.rotate_if_needed(buf.len());
        if self.file.is_none()
            && let Ok(reopened) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
        {
            self.written_bytes = reopened.metadata().map(|m| m.len()).unwrap_or(0);
            self.file = Some(reopened);
        }
        if let Some(ref mut f) = self.file {
            f.write_all(buf)?;
            self.written_bytes = self.written_bytes.saturating_add(buf.len() as u64);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(ref mut f) = self.file {
            f.flush()?;
        }
        Ok(())
    }
}

fn rotated_path(base: &Path, index: usize) -> PathBuf {
    let mut os = base.as_os_str().to_os_string();
    os.push(format!(".{index}"));
    PathBuf::from(os)
}

/// Thread-safe, cloneable rotating log writer suitable for `tracing_subscriber::fmt::MakeWriter`.
#[derive(Clone)]
pub struct RotatingLogWriter {
    inner: Arc<Mutex<RotatingState>>,
    tee_stderr: bool,
}

impl RotatingLogWriter {
    pub fn open(
        path: impl Into<PathBuf>,
        max_bytes: u64,
        max_backups: usize,
        tee_stderr: bool,
    ) -> io::Result<Self> {
        let state = RotatingState::open(path.into(), max_bytes, max_backups)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(state)),
            tee_stderr,
        })
    }
}

impl Write for RotatingLogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.tee_stderr {
            let _ = io::stderr().write_all(buf);
        }
        if let Ok(mut guard) = self.inner.lock() {
            let _ = guard.write_bytes(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.tee_stderr {
            let _ = io::stderr().flush();
        }
        if let Ok(mut guard) = self.inner.lock() {
            let _ = guard.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_file_when_exceeding_max_bytes() {
        let dir = std::env::temp_dir().join(format!("orbit_log_rot_{}", uuid::Uuid::new_v4()));
        let log_path = dir.join("test.log");
        let mut writer = RotatingLogWriter::open(&log_path, 4096, 2, false).expect("open");

        let chunk = vec![b'a'; 2500];
        writer.write_all(&chunk).expect("write 1");
        writer.write_all(&chunk).expect("write 2 triggers rotation");
        writer
            .write_all(&chunk)
            .expect("write 3 triggers second rotation");
        writer.flush().expect("flush");

        assert!(log_path.exists());
        assert!(rotated_path(&log_path, 1).exists());
        assert!(rotated_path(&log_path, 2).exists());
        assert!(!rotated_path(&log_path, 3).exists());

        let _ = fs::remove_dir_all(&dir);
    }
}
