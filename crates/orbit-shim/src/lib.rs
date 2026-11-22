//! Detached process & PTY supervisor (`orbit shim`).
//!
//! Keeps `agy`, `codex`, `claude`, and interactive PTY shells alive across `orbit daemon`
//! restarts or binary hot-upgrades via Unix Domain Sockets (`$ORBIT_HOME/shims/<id>.sock`).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static TEST_PARENT_PID_WATCHDOG: AtomicU32 = AtomicU32::new(0);

/// Enable an opt-in test watchdog so detached shims spawned by integration tests automatically
/// self-terminate if the test runner process exits or crashes (never enabled in production daemons).
pub fn set_test_parent_pid_watchdog(pid: Option<u32>) {
    TEST_PARENT_PID_WATCHDOG.store(pid.unwrap_or(0), Ordering::Relaxed);
}

use base64::{Engine as _, engine::general_purpose::STANDARD as B64};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc};

const STDIO_RING_CAPACITY: usize = 256;
const PTY_SNAPSHOT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShimMode {
    Stdio,
    Pty { cols: u16, rows: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShimSpawnSpec {
    pub shim_id: String,
    pub mode: ShimMode,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
}

/// Frame exchanged over the Unix Domain Socket between `orbit daemon` and `orbit shim`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ShimFrame {
    // Daemon -> Shim (Stdio)
    WriteStdin {
        line: String,
    },
    // Daemon -> Shim (PTY)
    PtyInput {
        data_b64: String,
    },
    PtyResize {
        cols: u16,
        rows: u16,
    },
    // Daemon -> Shim (Lifecycle)
    Kill,
    // Daemon -> Shim (Cursor-aware Stdio attach handshake)
    Attach {
        #[serde(default)]
        after_seq: Option<u64>,
    },

    // Shim -> Daemon (Cursor-aware Stdio attach acknowledgement)
    AttachAck {
        latest_seq: u64,
    },
    // Shim -> Daemon (Stdio)
    StdoutLine {
        line: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    StderrLine {
        line: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
    },
    // Shim -> Daemon (PTY)
    PtySnapshot {
        data_b64: String,
    },
    PtyOutput {
        data_b64: String,
    },
    // Shim -> Daemon (Lifecycle)
    Exited {
        code: Option<i32>,
    },
}

impl ShimFrame {
    pub fn seq(&self) -> Option<u64> {
        match self {
            ShimFrame::StdoutLine { seq, .. } | ShimFrame::StderrLine { seq, .. } => *seq,
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ShimError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("pty error: {0}")]
    Pty(String),
    #[error("shim socket did not become ready in time: {0}")]
    ReadyTimeout(PathBuf),
    #[error("shim channel closed")]
    ChannelClosed,
}

/// Client handle held by `orbit daemon` to interact with a running `orbit shim` over UDS.
/// Dropping `ShimClient` only disconnects the UDS socket — the supervised child keeps running!
pub struct ShimClient {
    tx: mpsc::Sender<ShimFrame>,
    pub rx: mpsc::Receiver<ShimFrame>,
}

impl ShimClient {
    pub async fn connect(socket_path: &Path) -> Result<Self, ShimError> {
        let stream = UnixStream::connect(socket_path).await?;
        Ok(Self::from_stream(stream, Vec::new()))
    }

    /// Connect to a Stdio `orbit shim` with an optional replay cursor (`after_seq`).
    ///
    /// - Modern shims immediately reply with `AttachAck` and stream only frames with `seq > after_seq`.
    /// - Legacy pre-cursor shims ignore `Attach` and immediately dump their unsequenced (`seq: None`)
    ///   ring buffer; when `drain_legacy_unsequenced` is true, that synchronous burst is drained so
    ///   hot-upgrading from an older shim never duplicates historical output into the timeline.
    pub async fn connect_with_cursor(
        socket_path: &Path,
        after_seq: Option<u64>,
        drain_legacy_unsequenced: bool,
    ) -> Result<Self, ShimError> {
        let stream = UnixStream::connect(socket_path).await?;
        let (read_half, mut write_half) = stream.into_split();

        let mut attach_line = serde_json::to_string(&ShimFrame::Attach { after_seq })?;
        attach_line.push('\n');
        write_half.write_all(attach_line.as_bytes()).await?;
        let _ = write_half.flush().await;

        let mut lines = BufReader::new(read_half).lines();
        let mut preloaded: Vec<ShimFrame> = Vec::new();
        let handshake_deadline = tokio::time::Instant::now() + Duration::from_millis(180);
        let mut quiet_window = Duration::from_millis(50);
        let mut legacy_drained: usize = 0;

        loop {
            let remaining =
                handshake_deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let wait = remaining.min(quiet_window);
            match tokio::time::timeout(wait, lines.next_line()).await {
                Ok(Ok(Some(raw))) => {
                    let Ok(frame) = serde_json::from_str::<ShimFrame>(&raw) else {
                        continue;
                    };
                    match frame {
                        ShimFrame::AttachAck { .. } => {
                            // Modern cursor-aware shim confirmed our cursor; every subsequent
                            // frame is strictly unseen (`seq > after_seq`).
                            break;
                        }
                        ShimFrame::StdoutLine { seq: None, .. }
                        | ShimFrame::StderrLine { seq: None, .. }
                            if drain_legacy_unsequenced =>
                        {
                            // Legacy shim dumping its unsequenced ring buffer — discard burst.
                            legacy_drained += 1;
                            quiet_window = Duration::from_millis(25);
                            if legacy_drained >= STDIO_RING_CAPACITY {
                                break;
                            }
                        }
                        other => {
                            preloaded.push(other);
                            break;
                        }
                    }
                }
                Ok(Ok(None)) => break,
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        let (in_tx, mut in_rx) = mpsc::channel::<ShimFrame>(256);
        let (out_tx, out_rx) = mpsc::channel::<ShimFrame>(512);

        tokio::spawn(async move {
            while let Some(frame) = in_rx.recv().await {
                let Ok(mut line) = serde_json::to_string(&frame) else {
                    continue;
                };
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            for frame in preloaded {
                if out_tx.send(frame).await.is_err() {
                    return;
                }
            }
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(frame) = serde_json::from_str::<ShimFrame>(&line) {
                    if matches!(frame, ShimFrame::AttachAck { .. }) {
                        continue;
                    }
                    if out_tx.send(frame).await.is_err() {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            tx: in_tx,
            rx: out_rx,
        })
    }

    fn from_stream(stream: UnixStream, preloaded: Vec<ShimFrame>) -> Self {
        let (read_half, mut write_half) = stream.into_split();
        let (in_tx, mut in_rx) = mpsc::channel::<ShimFrame>(256);
        let (out_tx, out_rx) = mpsc::channel::<ShimFrame>(512);

        tokio::spawn(async move {
            while let Some(frame) = in_rx.recv().await {
                let Ok(mut line) = serde_json::to_string(&frame) else {
                    continue;
                };
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
            }
        });

        tokio::spawn(async move {
            for frame in preloaded {
                if out_tx.send(frame).await.is_err() {
                    return;
                }
            }
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(frame) = serde_json::from_str::<ShimFrame>(&line) {
                    if matches!(frame, ShimFrame::AttachAck { .. }) {
                        continue;
                    }
                    if out_tx.send(frame).await.is_err() {
                        break;
                    }
                }
            }
        });

        Self {
            tx: in_tx,
            rx: out_rx,
        }
    }

    pub async fn send(&self, frame: ShimFrame) -> Result<(), ShimError> {
        self.tx
            .send(frame)
            .await
            .map_err(|_| ShimError::ChannelClosed)
    }

    pub async fn write_stdin_line(&self, line: impl Into<String>) -> Result<(), ShimError> {
        self.send(ShimFrame::WriteStdin { line: line.into() }).await
    }

    pub async fn write_pty_input(&self, bytes: &[u8]) -> Result<(), ShimError> {
        self.send(ShimFrame::PtyInput {
            data_b64: B64.encode(bytes),
        })
        .await
    }

    pub async fn resize_pty(&self, cols: u16, rows: u16) -> Result<(), ShimError> {
        self.send(ShimFrame::PtyResize { cols, rows }).await
    }

    pub async fn kill(&self) -> Result<(), ShimError> {
        self.send(ShimFrame::Kill).await
    }
}

/// Probe whether a shim UDS socket is alive without triggering ring-buffer replay.
/// Removes stale socket files whose listener is gone (`ECONNREFUSED`).
pub async fn probe_existing_shim(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    match tokio::time::timeout(Duration::from_millis(300), UnixStream::connect(socket_path)).await {
        Ok(Ok(_stream)) => true,
        _ => {
            let _ = std::fs::remove_file(socket_path);
            false
        }
    }
}

/// Attempt to reconnect to an already-running `orbit shim` UDS socket (e.g. after Daemon hot-restart).
/// If the socket file exists on disk but the shim process is gone (`ECONNREFUSED`), removes the stale file.
pub async fn connect_existing_shim(socket_path: &Path) -> Option<ShimClient> {
    if !socket_path.exists() {
        return None;
    }
    match tokio::time::timeout(Duration::from_millis(300), ShimClient::connect(socket_path)).await {
        Ok(Ok(client)) => Some(client),
        _ => {
            let _ = std::fs::remove_file(socket_path);
            None
        }
    }
}

/// Reconnect to an already-running Stdio `orbit shim`, requesting only frames with `seq > after_seq`.
/// When `after_seq` is `None` (e.g. upgrading from a daemon that hadn't persisted cursors yet),
/// sends `Some(u64::MAX)` so a modern shim skips historical ring frames and only delivers new live
/// frames, while legacy unsequenced ring dumps are drained automatically.
pub async fn connect_existing_shim_after(
    socket_path: &Path,
    after_seq: Option<u64>,
) -> Option<ShimClient> {
    if !socket_path.exists() {
        return None;
    }
    let effective_cursor = Some(after_seq.unwrap_or(u64::MAX));
    match tokio::time::timeout(
        Duration::from_millis(500),
        ShimClient::connect_with_cursor(socket_path, effective_cursor, true),
    )
    .await
    {
        Ok(Ok(client)) => Some(client),
        _ => {
            let _ = std::fs::remove_file(socket_path);
            None
        }
    }
}

/// Best-effort kill for a surviving shim socket that should be closed before replacement.
pub async fn kill_existing_shim(socket_path: &Path) {
    if let Some(client) = connect_existing_shim(socket_path).await {
        let _ = client.kill().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let _ = std::fs::remove_file(socket_path);
}

/// Synchronous best-effort kill for a shim socket (safe to call from RAII `Drop` guards in tests).
pub fn kill_existing_shim_sync(socket_path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;
        if let Ok(mut stream) = UnixStream::connect(socket_path) {
            let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
            let _ = stream.write_all(b"{\"type\":\"kill\"}\n");
            let _ = stream.flush();
            drop(stream);
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    let _ = std::fs::remove_file(socket_path);
}

/// Synchronously kill every active shim whose `.sock` lives in `shims_dir`.
pub fn kill_all_shims_in_dir_sync(shims_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(shims_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sock") {
            kill_existing_shim_sync(&path);
        }
    }
}

/// Spawn a detached `orbit shim` process in its own OS process group and connect to its UDS.
pub async fn spawn_detached_shim(
    orbit_exe: &Path,
    socket_path: &Path,
    spec: &ShimSpawnSpec,
) -> Result<ShimClient, ShimError> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let spec_json = serde_json::to_string(spec)?;
    let mut std_cmd = std::process::Command::new(orbit_exe);
    std_cmd
        .arg("shim")
        .arg("--socket")
        .arg(socket_path)
        .arg("--spec-json")
        .arg(spec_json)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let watchdog_pid = TEST_PARENT_PID_WATCHDOG.load(Ordering::Relaxed);
    if watchdog_pid > 0 {
        std_cmd.env("ORBIT_SHIM_WATCHDOG_PID", watchdog_pid.to_string());
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        std_cmd.process_group(0);
    }

    let _child = std_cmd.spawn()?;

    // Poll up to 3s for UDS listener readiness
    for _ in 0..150 {
        if socket_path.exists()
            && let Ok(client) = ShimClient::connect(socket_path).await
        {
            return Ok(client);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    Err(ShimError::ReadyTimeout(socket_path.to_path_buf()))
}

/// Entrypoint invoked by `orbit shim --socket <path> --spec-json <json>`.
pub async fn run_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    match spec.mode {
        ShimMode::Stdio => run_stdio_shim(socket_path, spec).await,
        ShimMode::Pty { cols, rows } => run_pty_shim(socket_path, spec, cols, rows).await,
    }
}

/// RAII guard that guarantees the shim Unix socket file is unlinked even on early error returns.
struct SocketCleanupGuard(PathBuf);

impl Drop for SocketCleanupGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Probe every `*.sock` entry under `shims_dir` and unlink orphaned sockets whose listener has
/// already exited (`ConnectionRefused` / `NotFound`). Active shims survive untouched.
pub async fn cleanup_stale_shim_sockets(shims_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(shims_dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("sock") {
            continue;
        }
        if let Ok(Err(err)) =
            tokio::time::timeout(Duration::from_millis(150), UnixStream::connect(&path)).await
            && matches!(
                err.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            )
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

async fn run_stdio_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    prepare_socket_path(socket_path)?;
    let listener = UnixListener::bind(socket_path)?;
    let _socket_guard = SocketCleanupGuard(socket_path.to_path_buf());

    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .envs(&spec.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    let mut child = cmd.spawn()?;
    let mut child_stdin = child.stdin.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing child stdin")
    })?;
    let child_stdout = child.stdout.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing child stdout")
    })?;
    let child_stderr = child.stderr.take().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing child stderr")
    })?;

    let ring: Arc<Mutex<VecDeque<ShimFrame>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(STDIO_RING_CAPACITY)));
    let next_seq = Arc::new(AtomicU64::new(1));
    let (out_tx, _) = broadcast::channel::<ShimFrame>(1024);
    let (in_tx, mut in_rx) = mpsc::channel::<ShimFrame>(256);

    let out_tx_stdout = out_tx.clone();
    let ring_stdout = ring.clone();
    let seq_stdout = next_seq.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(child_stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let seq = seq_stdout.fetch_add(1, Ordering::Relaxed);
            let frame = ShimFrame::StdoutLine {
                line,
                seq: Some(seq),
            };
            push_ring(&ring_stdout, frame.clone());
            let _ = out_tx_stdout.send(frame);
        }
    });

    let out_tx_stderr = out_tx.clone();
    let ring_stderr = ring.clone();
    let seq_stderr = next_seq.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(child_stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let seq = seq_stderr.fetch_add(1, Ordering::Relaxed);
            let frame = ShimFrame::StderrLine {
                line,
                seq: Some(seq),
            };
            push_ring(&ring_stderr, frame.clone());
            let _ = out_tx_stderr.send(frame);
        }
    });

    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    spawn_optional_parent_watchdog(&spec, kill_tx.clone());
    tokio::spawn(async move {
        while let Some(frame) = in_rx.recv().await {
            match frame {
                ShimFrame::WriteStdin { mut line } => {
                    if !line.ends_with('\n') {
                        line.push('\n');
                    }
                    if child_stdin.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = child_stdin.flush().await;
                }
                ShimFrame::Kill => {
                    let _ = kill_tx.send(()).await;
                    break;
                }
                _ => {}
            }
        }
    });

    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                if let Ok((stream, _)) = accept_res {
                    handle_stdio_daemon_attach(
                        stream,
                        ring.clone(),
                        next_seq.clone(),
                        out_tx.subscribe(),
                        in_tx.clone(),
                    );
                }
            }
            _ = kill_rx.recv() => {
                let _ = child.kill().await;
                let status = child.wait().await.ok();
                let _ = out_tx.send(ShimFrame::Exited { code: status.and_then(|s| s.code()) });
                break;
            }
            wait_res = child.wait() => {
                let code = wait_res.ok().and_then(|s| s.code());
                let _ = out_tx.send(ShimFrame::Exited { code });
                break;
            }
        }
    }

    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

async fn run_pty_shim(
    socket_path: &Path,
    spec: ShimSpawnSpec,
    cols: u16,
    rows: u16,
) -> Result<(), ShimError> {
    prepare_socket_path(socket_path)?;
    let listener = UnixListener::bind(socket_path)?;
    let _socket_guard = SocketCleanupGuard(socket_path.to_path_buf());

    let pty_system = NativePtySystem::default();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| ShimError::Pty(e.to_string()))?;

    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    cmd.env("TERM", "xterm-256color");
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| ShimError::Pty(e.to_string()))?;
    let mut child_killer = child.clone_killer();
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| ShimError::Pty(e.to_string()))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| ShimError::Pty(e.to_string()))?;
    let master = Arc::new(Mutex::new(pair.master));

    let scrollback: Arc<Mutex<VecDeque<u8>>> =
        Arc::new(Mutex::new(VecDeque::with_capacity(PTY_SNAPSHOT_BYTES)));
    let (out_tx, _) = broadcast::channel::<ShimFrame>(1024);
    let (in_tx, mut in_rx) = mpsc::channel::<ShimFrame>(256);
    let (exit_tx, mut exit_rx) = mpsc::channel::<Option<i32>>(1);

    let out_tx_pty = out_tx.clone();
    let scrollback_reader = scrollback.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let chunk = &buf[..n];
                    if let Ok(mut sb) = scrollback_reader.lock() {
                        for &b in chunk {
                            if sb.len() >= PTY_SNAPSHOT_BYTES {
                                sb.pop_front();
                            }
                            sb.push_back(b);
                        }
                    }
                    let _ = out_tx_pty.send(ShimFrame::PtyOutput {
                        data_b64: B64.encode(chunk),
                    });
                }
            }
        }
        let code = child
            .wait()
            .ok()
            .and_then(|s| i32::try_from(s.exit_code()).ok());
        let _ = exit_tx.blocking_send(code);
    });

    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
    spawn_optional_parent_watchdog(&spec, kill_tx.clone());
    let master_ctrl = master.clone();
    tokio::spawn(async move {
        while let Some(frame) = in_rx.recv().await {
            match frame {
                ShimFrame::PtyInput { data_b64 } => {
                    if let Ok(bytes) = B64.decode(data_b64) {
                        let _ = writer.write_all(&bytes);
                        let _ = writer.flush();
                    }
                }
                ShimFrame::PtyResize { cols, rows } => {
                    if let Ok(m) = master_ctrl.lock() {
                        let _ = m.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
                ShimFrame::Kill => {
                    let _ = kill_tx.send(()).await;
                    break;
                }
                _ => {}
            }
        }
    });

    loop {
        tokio::select! {
            accept_res = listener.accept() => {
                if let Ok((stream, _)) = accept_res {
                    let snap_bytes: Vec<u8> = scrollback
                        .lock()
                        .map(|sb| sb.iter().copied().collect())
                        .unwrap_or_default();
                    let initial = if snap_bytes.is_empty() {
                        Vec::new()
                    } else {
                        vec![ShimFrame::PtySnapshot { data_b64: B64.encode(&snap_bytes) }]
                    };
                    handle_daemon_attach(
                        stream,
                        initial,
                        out_tx.subscribe(),
                        in_tx.clone(),
                        Some(scrollback.clone()),
                    );
                }
            }
            _ = kill_rx.recv() => {
                let _ = child_killer.kill();
                let _ = out_tx.send(ShimFrame::Exited { code: None });
                break;
            }
            maybe_code = exit_rx.recv() => {
                let _ = out_tx.send(ShimFrame::Exited { code: maybe_code.flatten() });
                break;
            }
        }
    }

    let _ = std::fs::remove_file(socket_path);
    Ok(())
}

fn prepare_socket_path(socket_path: &Path) -> Result<(), std::io::Error> {
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
    }
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn push_ring(ring: &Arc<Mutex<VecDeque<ShimFrame>>>, frame: ShimFrame) {
    if let Ok(mut guard) = ring.lock() {
        if guard.len() >= STDIO_RING_CAPACITY {
            guard.pop_front();
        }
        guard.push_back(frame);
    }
}

fn handle_stdio_daemon_attach(
    stream: UnixStream,
    ring: Arc<Mutex<VecDeque<ShimFrame>>>,
    next_seq: Arc<AtomicU64>,
    mut out_rx: broadcast::Receiver<ShimFrame>,
    in_tx: mpsc::Sender<ShimFrame>,
) {
    tokio::spawn(async move {
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        // Briefly inspect the first incoming frame to check for a cursor-aware `Attach` handshake.
        // - If `probe_existing_shim` connects and immediately closes (EOF), return without writing.
        // - If a modern daemon sends `Attach { after_seq }`, acknowledge with `AttachAck` and send
        //   only frames strictly newer than `after_seq`.
        // - If a legacy client sends `WriteStdin`/`Kill` or doesn't send `Attach` within 80ms,
        //   fall back to replaying the full ring buffer for backward compatibility.
        let mut send_ack = false;
        let mut after_seq: Option<u64> = None;
        let mut first_non_attach: Option<ShimFrame> = None;

        match tokio::time::timeout(Duration::from_millis(80), lines.next_line()).await {
            Ok(Ok(None)) => return,
            Ok(Ok(Some(raw))) => {
                if let Ok(frame) = serde_json::from_str::<ShimFrame>(&raw) {
                    match frame {
                        ShimFrame::Attach { after_seq: cursor } => {
                            send_ack = true;
                            after_seq = cursor;
                        }
                        other => {
                            first_non_attach = Some(other);
                        }
                    }
                }
            }
            Ok(Err(_)) => return,
            Err(_) => {}
        }

        let latest_seq = next_seq.load(Ordering::Relaxed).saturating_sub(1);
        let initial_frames: Vec<ShimFrame> = ring
            .lock()
            .map(|g| {
                g.iter()
                    .filter(|f| match after_seq {
                        Some(cursor) => f.seq().is_none_or(|s| s > cursor),
                        None => true,
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Spawn reader loop for remaining inbound frames from the daemon.
        let in_tx_reader = in_tx.clone();
        tokio::spawn(async move {
            if let Some(frame) = first_non_attach
                && in_tx_reader.send(frame).await.is_err()
            {
                return;
            }
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(frame) = serde_json::from_str::<ShimFrame>(&line) {
                    if matches!(frame, ShimFrame::Attach { .. }) {
                        continue;
                    }
                    if in_tx_reader.send(frame).await.is_err() {
                        break;
                    }
                }
            }
        });

        let mut max_sent_seq = after_seq.unwrap_or(0).min(latest_seq);
        if send_ack
            && let Ok(mut encoded) = serde_json::to_string(&ShimFrame::AttachAck { latest_seq })
        {
            encoded.push('\n');
            if write_half.write_all(encoded.as_bytes()).await.is_err() {
                return;
            }
        }
        for frame in initial_frames {
            if let Some(s) = frame.seq() {
                max_sent_seq = max_sent_seq.max(s);
            }
            if let Ok(mut encoded) = serde_json::to_string(&frame) {
                encoded.push('\n');
                if write_half.write_all(encoded.as_bytes()).await.is_err() {
                    return;
                }
            }
        }
        loop {
            match out_rx.recv().await {
                Ok(frame) => {
                    if let Some(s) = frame.seq() {
                        if s <= max_sent_seq {
                            continue;
                        }
                        max_sent_seq = s;
                    }
                    if let Ok(mut encoded) = serde_json::to_string(&frame) {
                        encoded.push('\n');
                        if write_half.write_all(encoded.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn handle_daemon_attach(
    stream: UnixStream,
    initial_frames: Vec<ShimFrame>,
    mut out_rx: broadcast::Receiver<ShimFrame>,
    in_tx: mpsc::Sender<ShimFrame>,
    pty_scrollback: Option<Arc<Mutex<VecDeque<u8>>>>,
) {
    let (read_half, mut write_half) = stream.into_split();
    tokio::spawn(async move {
        let mut lines = BufReader::new(read_half).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Ok(frame) = serde_json::from_str::<ShimFrame>(&line)
                && in_tx.send(frame).await.is_err()
            {
                break;
            }
        }
    });

    tokio::spawn(async move {
        for frame in initial_frames {
            if let Ok(mut encoded) = serde_json::to_string(&frame) {
                encoded.push('\n');
                if write_half.write_all(encoded.as_bytes()).await.is_err() {
                    return;
                }
            }
        }
        loop {
            match out_rx.recv().await {
                Ok(frame) => {
                    if let Ok(mut encoded) = serde_json::to_string(&frame) {
                        encoded.push('\n');
                        if write_half.write_all(encoded.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    if let Some(ref sb_arc) = pty_scrollback {
                        let snap_bytes: Vec<u8> = sb_arc
                            .lock()
                            .map(|sb| sb.iter().copied().collect())
                            .unwrap_or_default();
                        if !snap_bytes.is_empty() {
                            let snap = ShimFrame::PtySnapshot {
                                data_b64: B64.encode(&snap_bytes),
                            };
                            if let Ok(mut encoded) = serde_json::to_string(&snap) {
                                encoded.push('\n');
                                if write_half.write_all(encoded.as_bytes()).await.is_err() {
                                    break;
                                }
                            }
                        }
                    } else {
                        let _ = skipped;
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        true
    }
}

fn spawn_optional_parent_watchdog(spec: &ShimSpawnSpec, kill_tx: mpsc::Sender<()>) {
    let pid_opt = spec
        .env
        .get("ORBIT_SHIM_WATCHDOG_PID")
        .cloned()
        .or_else(|| std::env::var("ORBIT_SHIM_WATCHDOG_PID").ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|&p| p > 0);
    let Some(pid) = pid_opt else {
        return;
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        loop {
            tick.tick().await;
            if !is_pid_alive(pid) {
                let _ = kill_tx.send(()).await;
                break;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stdio_shim_survives_daemon_disconnect_and_replays_ring() {
        let sock_path =
            std::env::temp_dir().join(format!("orbit_test_shim_{}.sock", std::process::id()));
        let spec = ShimSpawnSpec {
            shim_id: "test_cat".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
        };

        let sock_bg = sock_path.clone();
        let shim_task = tokio::spawn(async move {
            run_shim(&sock_bg, spec).await.expect("shim run");
        });

        for _ in 0..50 {
            if sock_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // First Daemon connection writes a line and disconnects (simulating daemon restart)
        {
            let mut client1 = ShimClient::connect(&sock_path).await.expect("connect 1");
            client1
                .write_stdin_line("hello-across-restart")
                .await
                .expect("write");
            let frame = tokio::time::timeout(Duration::from_secs(2), client1.rx.recv())
                .await
                .expect("timeout")
                .expect("frame");
            match frame {
                ShimFrame::StdoutLine { line, seq } => {
                    assert_eq!(line, "hello-across-restart");
                    assert_eq!(seq, Some(1));
                }
                other => panic!("unexpected frame: {other:?}"),
            }
        } // client1 dropped here!

        // Second Daemon connection attaches to the SAME running shim without a cursor and gets replay + live I/O
        let mut client2 = ShimClient::connect(&sock_path).await.expect("connect 2");
        let replayed = tokio::time::timeout(Duration::from_secs(2), client2.rx.recv())
            .await
            .expect("timeout")
            .expect("replay");
        match replayed {
            ShimFrame::StdoutLine { line, seq } => {
                assert_eq!(line, "hello-across-restart");
                assert_eq!(seq, Some(1));
            }
            other => panic!("unexpected replay: {other:?}"),
        }

        client2
            .write_stdin_line("second-turn")
            .await
            .expect("write 2");
        let live = tokio::time::timeout(Duration::from_secs(2), client2.rx.recv())
            .await
            .expect("timeout")
            .expect("live");
        match live {
            ShimFrame::StdoutLine { line, seq } => {
                assert_eq!(line, "second-turn");
                assert_eq!(seq, Some(2));
            }
            other => panic!("unexpected live: {other:?}"),
        }

        client2.kill().await.expect("kill");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
    }

    #[tokio::test]
    async fn stdio_shim_cursor_attach_skips_consumed_frames_and_delivers_only_new_ones() {
        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_shim_cursor_{}.sock",
            std::process::id()
        ));
        let spec = ShimSpawnSpec {
            shim_id: "test_cat_cursor".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
        };

        let sock_bg = sock_path.clone();
        let shim_task = tokio::spawn(async move {
            run_shim(&sock_bg, spec).await.expect("shim run");
        });

        for _ in 0..50 {
            if sock_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        // First daemon consumes seq 1 & 2, then disconnects.
        let last_seq = {
            let mut client1 = ShimClient::connect(&sock_path).await.expect("connect 1");
            client1.write_stdin_line("old-turn-1").await.expect("w1");
            client1.write_stdin_line("old-turn-2").await.expect("w2");
            let f1 = tokio::time::timeout(Duration::from_secs(2), client1.rx.recv())
                .await
                .expect("t1")
                .expect("f1");
            let f2 = tokio::time::timeout(Duration::from_secs(2), client1.rx.recv())
                .await
                .expect("t2")
                .expect("f2");
            assert_eq!(f1.seq(), Some(1));
            assert_eq!(f2.seq(), Some(2));
            // Also verify probing does not disturb the shim
            assert!(probe_existing_shim(&sock_path).await);
            f2.seq().expect("f2 seq")
        };

        // Reconnect with `after_seq = Some(2)` -> must NOT replay old-turn-1 or old-turn-2!
        let mut client2 = connect_existing_shim_after(&sock_path, Some(last_seq))
            .await
            .expect("reattach with cursor");
        assert!(
            tokio::time::timeout(Duration::from_millis(80), client2.rx.recv())
                .await
                .is_err(),
            "already-consumed frames must NOT be replayed across reattach"
        );

        // Live output after reattach must still arrive cleanly with seq = 3.
        client2.write_stdin_line("new-turn-3").await.expect("w3");
        let f3 = tokio::time::timeout(Duration::from_secs(2), client2.rx.recv())
            .await
            .expect("t3")
            .expect("f3");
        match f3 {
            ShimFrame::StdoutLine { line, seq } => {
                assert_eq!(line, "new-turn-3");
                assert_eq!(seq, Some(3));
            }
            other => panic!("unexpected live frame: {other:?}"),
        }

        // Reconnect with `after_seq = Some(1)` -> must replay ONLY seq 2 & 3 (not seq 1).
        drop(client2);
        let mut client3 = connect_existing_shim_after(&sock_path, Some(1))
            .await
            .expect("reattach gap");
        let r2 = tokio::time::timeout(Duration::from_secs(2), client3.rx.recv())
            .await
            .expect("tr2")
            .expect("r2");
        let r3 = tokio::time::timeout(Duration::from_secs(2), client3.rx.recv())
            .await
            .expect("tr3")
            .expect("r3");
        assert!(
            matches!(r2, ShimFrame::StdoutLine { ref line, seq: Some(2) } if line == "old-turn-2")
        );
        assert!(
            matches!(r3, ShimFrame::StdoutLine { ref line, seq: Some(3) } if line == "new-turn-3")
        );

        client3.kill().await.expect("kill");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
    }

    #[tokio::test]
    async fn cursor_attach_drains_legacy_unsequenced_ring_dump() {
        let sock_path = std::env::temp_dir().join(format!(
            "orbit_test_legacy_shim_{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let listener = UnixListener::bind(&sock_path).expect("bind fake legacy shim");

        // Simulate a legacy v0.8.0 shim that ignores `Attach` and dumps unsequenced ring frames.
        let server = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                let legacy_dump = concat!(
                    "{\"type\":\"stdout_line\",\"line\":\"stale-truncated-chunk-1\"}\n",
                    "{\"type\":\"stdout_line\",\"line\":\"stale-truncated-chunk-2\"}\n"
                );
                let _ = stream.write_all(legacy_dump.as_bytes()).await;
                let _ = stream.flush().await;
                tokio::time::sleep(Duration::from_millis(80)).await;
                let live = "{\"type\":\"stdout_line\",\"line\":\"fresh-after-attach\"}\n";
                let _ = stream.write_all(live.as_bytes()).await;
                let _ = stream.flush().await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        let mut client = connect_existing_shim_after(&sock_path, None)
            .await
            .expect("connect legacy");
        let frame = tokio::time::timeout(Duration::from_secs(2), client.rx.recv())
            .await
            .expect("timeout")
            .expect("frame");
        match frame {
            ShimFrame::StdoutLine { line, seq } => {
                assert_eq!(line, "fresh-after-attach");
                assert_eq!(seq, None);
            }
            other => panic!("unexpected frame: {other:?}"),
        }

        let _ = server.await;
        let _ = std::fs::remove_file(&sock_path);
    }

    #[tokio::test]
    async fn cleanup_stale_shim_sockets_removes_dead_and_keeps_live_sockets() {
        let dir = std::env::temp_dir().join(format!("orbit_stale_sock_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");

        let live_sock = dir.join("live.sock");
        let _live_listener = UnixListener::bind(&live_sock).expect("bind live");

        let dead_sock = dir.join("dead.sock");
        {
            let _dead_listener = UnixListener::bind(&dead_sock).expect("bind dead");
        } // listener dropped -> stale socket file remains on disk

        assert!(dead_sock.exists());
        let removed = cleanup_stale_shim_sockets(&dir).await;
        assert_eq!(removed, 1);
        assert!(!dead_sock.exists(), "dead socket should be unlinked");
        assert!(live_sock.exists(), "live socket must remain untouched");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn parent_watchdog_terminates_shim_when_watched_pid_is_gone() {
        let sock_path =
            std::env::temp_dir().join(format!("orbit_watchdog_shim_{}.sock", std::process::id()));
        let mut env = HashMap::new();
        // Guaranteed non-existent PID
        env.insert("ORBIT_SHIM_WATCHDOG_PID".into(), "4194300".into());
        let spec = ShimSpawnSpec {
            shim_id: "test_watchdog".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env,
        };
        let sock_bg = sock_path.clone();
        let res = tokio::time::timeout(Duration::from_secs(3), run_shim(&sock_bg, spec)).await;
        assert!(
            res.is_ok(),
            "shim with dead watchdog PID must self-terminate promptly"
        );
        assert!(!sock_path.exists(), "socket file must be unlinked on exit");
    }
}
