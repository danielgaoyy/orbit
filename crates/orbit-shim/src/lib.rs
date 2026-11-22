//! Detached process & PTY supervisor (`orbit shim`).
//!
//! Keeps `agy`, `codex`, `claude`, and interactive PTY shells alive across `orbit daemon`
//! restarts or binary hot-upgrades via Unix Domain Sockets on Unix and authenticated
//! loopback TCP endpoints (`127.0.0.1` + per-shim 256-bit token descriptor) on Windows
//! (`$ORBIT_HOME/shims/<id>.sock`).

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
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, mpsc, oneshot};

#[cfg(unix)]
pub type ShimStream = tokio::net::UnixStream;
#[cfg(unix)]
pub type ShimReadHalf = tokio::net::unix::OwnedReadHalf;
#[cfg(unix)]
pub type ShimWriteHalf = tokio::net::unix::OwnedWriteHalf;

#[cfg(not(unix))]
pub type ShimStream = tokio::net::TcpStream;
#[cfg(not(unix))]
pub type ShimReadHalf = tokio::net::tcp::OwnedReadHalf;
#[cfg(not(unix))]
pub type ShimWriteHalf = tokio::net::tcp::OwnedWriteHalf;

#[cfg(windows)]
const WIN_CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(windows)]
const WIN_CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Descriptor written to `<id>.sock` on Windows (and tested cross-platform) for loopback IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimTcpEndpoint {
    pub port: u16,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShimAuthHandshake {
    auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShimAuthAck {
    auth_ok: bool,
}

#[allow(dead_code)]
static ENDPOINT_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[allow(dead_code)]
const MAX_CONCURRENT_AUTH_HANDSHAKES: usize = 16;

#[allow(dead_code)]
fn generate_shim_auth_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let mut hex = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

#[allow(dead_code)]
fn encode_shim_auth_line(token: &str) -> String {
    let mut line = serde_json::to_string(&ShimAuthHandshake {
        auth: token.to_string(),
    })
    .unwrap_or_else(|_| String::from("{\"auth\":\"\"}"));
    line.push('\n');
    line
}

#[allow(dead_code)]
fn verify_shim_auth_line(raw_line: &str, expected_token: &str) -> bool {
    let Ok(handshake) = serde_json::from_str::<ShimAuthHandshake>(raw_line.trim()) else {
        return false;
    };
    constant_time_eq(handshake.auth.as_bytes(), expected_token.as_bytes())
}

#[allow(dead_code)]
fn encode_shim_auth_ack_line() -> &'static str {
    "{\"auth_ok\":true}\n"
}

#[allow(dead_code)]
fn verify_shim_auth_ack_line(raw_line: &str) -> bool {
    serde_json::from_str::<ShimAuthAck>(raw_line.trim()).is_ok_and(|ack| ack.auth_ok)
}

#[allow(dead_code)]
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[allow(dead_code)]
fn read_tcp_endpoint(socket_path: &Path) -> std::io::Result<ShimTcpEndpoint> {
    let raw = std::fs::read_to_string(socket_path)?;
    serde_json::from_str::<ShimTcpEndpoint>(raw.trim()).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "stale or invalid shim endpoint descriptor",
        )
    })
}

#[allow(dead_code)]
fn write_tcp_endpoint_atomic(
    socket_path: &Path,
    endpoint: &ShimTcpEndpoint,
) -> std::io::Result<()> {
    let json = serde_json::to_string(endpoint).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("encode endpoint: {e}"),
        )
    })?;
    let seq = ENDPOINT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = socket_path.with_extension(format!("sock.tmp.{}.{seq}", std::process::id()));
    std::fs::write(&tmp, &json)?;
    if let Err(err) = std::fs::rename(&tmp, socket_path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

/// Cross-platform Shim IPC listener (Unix Domain Socket on Unix, authenticated loopback TCP on Windows).
pub struct ShimListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(not(unix))]
    inner: tokio::net::TcpListener,
    #[cfg(not(unix))]
    auth_token: Arc<str>,
    #[cfg(not(unix))]
    handshake_sem: Arc<tokio::sync::Semaphore>,
    #[cfg(not(unix))]
    ready_tx: mpsc::Sender<ShimStream>,
    #[cfg(not(unix))]
    ready_rx: tokio::sync::Mutex<mpsc::Receiver<ShimStream>>,
}

impl ShimListener {
    pub async fn bind(socket_path: &Path) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            let inner = tokio::net::UnixListener::bind(socket_path)?;
            Ok(Self { inner })
        }
        #[cfg(not(unix))]
        {
            let inner = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let port = inner.local_addr()?.port();
            let auth_token = generate_shim_auth_token();
            write_tcp_endpoint_atomic(
                socket_path,
                &ShimTcpEndpoint {
                    port,
                    token: auth_token.clone(),
                },
            )?;
            let (ready_tx, ready_rx) = mpsc::channel(64);
            Ok(Self {
                inner,
                auth_token: Arc::from(auth_token),
                handshake_sem: Arc::new(tokio::sync::Semaphore::new(
                    MAX_CONCURRENT_AUTH_HANDSHAKES,
                )),
                ready_tx,
                ready_rx: tokio::sync::Mutex::new(ready_rx),
            })
        }
    }

    pub async fn accept(&self) -> std::io::Result<ShimStream> {
        #[cfg(unix)]
        {
            let (stream, _) = self.inner.accept().await?;
            Ok(stream)
        }
        #[cfg(not(unix))]
        {
            use tokio::io::AsyncReadExt;
            let mut rx = self.ready_rx.lock().await;
            loop {
                tokio::select! {
                    biased;
                    maybe_ready = rx.recv() => {
                        if let Some(stream) = maybe_ready {
                            return Ok(stream);
                        }
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "shim listener channel closed",
                        ));
                    }
                    accept_res = self.inner.accept() => {
                        let (mut stream, _) = accept_res?;
                        let _ = stream.set_nodelay(true);
                        let Ok(permit) = self.handshake_sem.clone().try_acquire_owned() else {
                            drop(stream);
                            continue;
                        };
                        let expected_token = Arc::clone(&self.auth_token);
                        let ready_tx = self.ready_tx.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            // Read the single auth line byte-by-byte (max 256 bytes) so zero
                            // bytes of the subsequent `ShimFrame` stream are ever over-read.
                            let mut buf = Vec::with_capacity(128);
                            let mut byte = [0u8; 1];
                            let auth_ok = tokio::time::timeout(Duration::from_secs(1), async {
                                while buf.len() < 256 {
                                    match stream.read(&mut byte).await {
                                        Ok(0) | Err(_) => return false,
                                        Ok(_) => {
                                            if byte[0] == b'\n' {
                                                break;
                                            }
                                            buf.push(byte[0]);
                                        }
                                    }
                                }
                                let line = String::from_utf8_lossy(&buf);
                                verify_shim_auth_line(&line, &expected_token)
                            })
                            .await
                            .unwrap_or(false);

                            if !auth_ok {
                                drop(stream);
                                return;
                            }
                            if tokio::time::timeout(Duration::from_millis(500), async {
                                stream.write_all(encode_shim_auth_ack_line().as_bytes()).await?;
                                stream.flush().await
                            })
                            .await
                            .ok()
                            .and_then(Result::ok)
                            .is_some()
                            {
                                let _ = ready_tx.send(stream).await;
                            }
                        });
                    }
                }
            }
        }
    }
}

async fn connect_shim_stream(socket_path: &Path) -> std::io::Result<ShimStream> {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(socket_path).await
    }
    #[cfg(not(unix))]
    {
        use tokio::io::AsyncReadExt;
        let endpoint = read_tcp_endpoint(socket_path)?;
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], endpoint.port));
        let mut stream = tokio::net::TcpStream::connect(addr).await?;
        let _ = stream.set_nodelay(true);
        let auth_line = encode_shim_auth_line(&endpoint.token);
        stream.write_all(auth_line.as_bytes()).await?;
        stream.flush().await?;

        // Wait for the shim's `{"auth_ok":true}\n` ACK line byte-by-byte so:
        // - Zero bytes of subsequent `ShimFrame` payloads are over-read.
        // - A recycled ephemeral port owned by another local service that closes or sends
        //   non-ACK data returns `ConnectionRefused` (allowing stale socket cleanup).
        // - A transiently busy shim still in kernel backlog returns `TimedOut` (never unlinked).
        let mut buf = Vec::with_capacity(64);
        let mut byte = [0u8; 1];
        let ack_res = tokio::time::timeout(Duration::from_millis(1000), async {
            while buf.len() < 128 {
                match stream.read(&mut byte).await {
                    Ok(0) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::ConnectionRefused,
                            "shim peer closed before auth ack",
                        ));
                    }
                    Err(e) => return Err(e),
                    Ok(_) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                }
            }
            let line = String::from_utf8_lossy(&buf);
            if verify_shim_auth_ack_line(&line) {
                Ok(())
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionRefused,
                    "unexpected shim auth ack (recycled port or token mismatch)",
                ))
            }
        })
        .await;

        match ack_res {
            Ok(Ok(())) => Ok(stream),
            Ok(Err(err)) => Err(err),
            Err(_) => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "timed out waiting for shim auth ack",
            )),
        }
    }
}

/// Parse `tasklist /FI "PID eq <pid>" /NH /FO CSV` stdout to verify PID presence (and optional image prefix).
#[allow(dead_code)]
fn parse_windows_tasklist_csv_match(
    stdout: &str,
    pid: u32,
    required_image_prefix: Option<&str>,
) -> bool {
    let pid_str = pid.to_string();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.starts_with('"') {
            continue;
        }
        let mut cols = trimmed.split("\",\"").map(|c| c.trim_matches('"'));
        let Some(image) = cols.next() else {
            continue;
        };
        let Some(found_pid) = cols.next() else {
            continue;
        };
        if found_pid.trim() == pid_str {
            if let Some(prefix) = required_image_prefix {
                return image
                    .to_ascii_lowercase()
                    .starts_with(&prefix.to_ascii_lowercase());
            }
            return true;
        }
    }
    false
}

const STDIO_RING_CAPACITY: usize = 256;
const PTY_SNAPSHOT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShimPidRecord {
    shim_pid: u32,
    #[serde(default)]
    child_pid: Option<u32>,
    #[serde(default)]
    child_pgid: Option<u32>,
}

/// Derive the `.pid` sidecar path corresponding to a shim `.sock` path.
pub fn pid_file_path(socket_path: &Path) -> PathBuf {
    socket_path.with_extension("pid")
}

fn write_pid_file(
    socket_path: &Path,
    shim_pid: u32,
    child_pid: Option<u32>,
    child_pgid: Option<u32>,
) {
    let rec = ShimPidRecord {
        shim_pid,
        child_pid,
        child_pgid,
    };
    if let Ok(json) = serde_json::to_string(&rec) {
        let _ = std::fs::write(pid_file_path(socket_path), json);
    }
}

fn read_pid_file(socket_path: &Path) -> Option<ShimPidRecord> {
    let raw = std::fs::read_to_string(pid_file_path(socket_path)).ok()?;
    serde_json::from_str::<ShimPidRecord>(&raw).ok()
}

#[cfg(unix)]
fn signal_pid(pid: u32, signal: &str) {
    if pid == 0 {
        return;
    }
    let _ = std::process::Command::new("/bin/kill")
        .args([signal, &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn signal_pid_group(pgid: u32, signal: &str) {
    if pgid == 0 {
        return;
    }
    let _ = std::process::Command::new("/bin/kill")
        .args([signal, &format!("-{pgid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn is_orbit_shim_process(pid: u32, socket_path: &Path) -> bool {
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    let expected_sock = socket_path.to_string_lossy();
    if expected_sock.is_empty() {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = std::fs::read(format!("/proc/{pid}/cmdline")) {
            let args: Vec<String> = raw
                .split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect();
            let has_shim = args.iter().any(|a| a == "shim");
            let has_matching_sock = args
                .windows(2)
                .any(|w| w[0] == "--socket" && w[1] == expected_sock.as_ref());
            return has_shim && has_matching_sock;
        }
    }
    std::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let cmd = String::from_utf8_lossy(&o.stdout);
            cmd.contains("shim") && cmd.contains("--socket") && cmd.contains(expected_sock.as_ref())
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn kill_process_tree_sync(pid: u32) {
    if pid == 0 || pid == std::process::id() {
        return;
    }
    use std::os::windows::process::CommandExt;
    let pid_str = pid.to_string();
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid_str, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(WIN_CREATE_NO_WINDOW)
        .status();
}

#[cfg(windows)]
async fn kill_process_tree_async(pid: u32) {
    if pid == 0 || pid == std::process::id() {
        return;
    }
    let pid_str = pid.to_string();
    let mut cmd = Command::new("taskkill");
    cmd.args(["/PID", &pid_str, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(WIN_CREATE_NO_WINDOW);
    let _ = cmd.status().await;
}

#[cfg(windows)]
fn is_orbit_shim_process(pid: u32, _socket_path: &Path) -> bool {
    if pid == 0 || pid == std::process::id() {
        return false;
    }
    use std::os::windows::process::CommandExt;
    let filter = format!("PID eq {pid}");
    std::process::Command::new("tasklist")
        .args(["/FI", &filter, "/NH", "/FO", "CSV"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(WIN_CREATE_NO_WINDOW)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            parse_windows_tasklist_csv_match(&stdout, pid, Some("orbit"))
        })
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn is_orbit_shim_process(_pid: u32, _socket_path: &Path) -> bool {
    false
}

async fn terminate_child_process_group(child: &mut tokio::process::Child) -> Option<i32> {
    #[cfg(unix)]
    let child_pid = child.id();
    #[cfg(unix)]
    if let Some(pid) = child_pid {
        // Child was spawned with `process_group(0)` so its PGID equals its PID.
        signal_pid_group(pid, "-TERM");
        if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_millis(150), child.wait()).await
        {
            signal_pid_group(pid, "-KILL");
            return status.code();
        }
        signal_pid_group(pid, "-KILL");
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        kill_process_tree_async(pid).await;
    }
    let _ = child.kill().await;
    tokio::time::timeout(Duration::from_millis(200), child.wait())
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|s| s.code())
}

async fn reap_recorded_shim_pid(rec: &ShimPidRecord, socket_path: &Path) {
    #[cfg(unix)]
    {
        if rec.shim_pid == 0 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        let safe_child_pgid = rec
            .child_pgid
            .filter(|&pgid| pgid > 0 && pgid != std::process::id() && pgid != rec.shim_pid);
        // Give a recently signalled shim a brief grace window to exit on its own.
        for _ in 0..6 {
            if !is_pid_alive(rec.shim_pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if let Some(pgid) = safe_child_pgid {
            signal_pid_group(pgid, "-TERM");
        }
        signal_pid(rec.shim_pid, "-TERM");
        for _ in 0..8 {
            if !is_pid_alive(rec.shim_pid) {
                if let Some(pgid) = safe_child_pgid {
                    signal_pid_group(pgid, "-KILL");
                }
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        if let Some(pgid) = safe_child_pgid {
            signal_pid_group(pgid, "-KILL");
        }
        signal_pid(rec.shim_pid, "-KILL");
        for _ in 0..4 {
            if !is_pid_alive(rec.shim_pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    #[cfg(windows)]
    {
        if rec.shim_pid == 0 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        if let Some(child_pid) = rec
            .child_pid
            .filter(|&p| p > 0 && p != std::process::id() && p != rec.shim_pid)
        {
            kill_process_tree_async(child_pid).await;
        }
        kill_process_tree_async(rec.shim_pid).await;
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (rec, socket_path);
    }
}

fn reap_recorded_shim_pid_sync(rec: &ShimPidRecord, socket_path: &Path) {
    #[cfg(unix)]
    {
        if rec.shim_pid == 0 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        let safe_child_pgid = rec
            .child_pgid
            .filter(|&pgid| pgid > 0 && pgid != std::process::id() && pgid != rec.shim_pid);
        if let Some(pgid) = safe_child_pgid {
            signal_pid_group(pgid, "-TERM");
        }
        signal_pid(rec.shim_pid, "-TERM");
        std::thread::sleep(Duration::from_millis(50));
        if is_pid_alive(rec.shim_pid) {
            if let Some(pgid) = safe_child_pgid {
                signal_pid_group(pgid, "-KILL");
            }
            signal_pid(rec.shim_pid, "-KILL");
        }
    }
    #[cfg(windows)]
    {
        if rec.shim_pid == 0 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        if let Some(child_pid) = rec
            .child_pid
            .filter(|&p| p > 0 && p != std::process::id() && p != rec.shim_pid)
        {
            kill_process_tree_sync(child_pid);
        }
        kill_process_tree_sync(rec.shim_pid);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (rec, socket_path);
    }
}

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
    tx: mpsc::Sender<(ShimFrame, Option<oneshot::Sender<()>>)>,
    pub rx: mpsc::Receiver<ShimFrame>,
}

impl ShimClient {
    pub async fn connect(socket_path: &Path) -> Result<Self, ShimError> {
        let stream = connect_shim_stream(socket_path).await?;
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
        let stream = connect_shim_stream(socket_path).await?;
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

        let (in_tx, mut in_rx) = mpsc::channel::<(ShimFrame, Option<oneshot::Sender<()>>)>(256);
        let (out_tx, out_rx) = mpsc::channel::<ShimFrame>(512);

        tokio::spawn(async move {
            while let Some((frame, ack)) = in_rx.recv().await {
                let is_kill = matches!(frame, ShimFrame::Kill);
                let Ok(mut line) = serde_json::to_string(&frame) else {
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                    continue;
                };
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                    break;
                }
                if is_kill || ack.is_some() {
                    let _ = write_half.flush().await;
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
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

    fn from_stream(stream: ShimStream, preloaded: Vec<ShimFrame>) -> Self {
        let (read_half, mut write_half) = stream.into_split();
        let (in_tx, mut in_rx) = mpsc::channel::<(ShimFrame, Option<oneshot::Sender<()>>)>(256);
        let (out_tx, out_rx) = mpsc::channel::<ShimFrame>(512);

        tokio::spawn(async move {
            while let Some((frame, ack)) = in_rx.recv().await {
                let is_kill = matches!(frame, ShimFrame::Kill);
                let Ok(mut line) = serde_json::to_string(&frame) else {
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                    continue;
                };
                line.push('\n');
                if write_half.write_all(line.as_bytes()).await.is_err() {
                    if let Some(tx) = ack {
                        let _ = tx.send(());
                    }
                    break;
                }
                if is_kill || ack.is_some() {
                    let _ = write_half.flush().await;
                }
                if let Some(tx) = ack {
                    let _ = tx.send(());
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
            .send((frame, None))
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

    /// Send `ShimFrame::Kill` and wait until the writer task flushes it to the kernel socket so
    /// dropping `ShimClient` immediately after `kill()` never drops the frame before delivery.
    pub async fn kill(&self) -> Result<(), ShimError> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.tx
            .send((ShimFrame::Kill, Some(ack_tx)))
            .await
            .map_err(|_| ShimError::ChannelClosed)?;
        let _ = tokio::time::timeout(Duration::from_millis(200), ack_rx).await;
        Ok(())
    }

    /// Send `ShimFrame::Kill`, flush it cleanly, and wait up to `timeout` for `ShimFrame::Exited`
    /// or socket EOF confirming the shim terminated.
    pub async fn kill_and_wait(&mut self, timeout: Duration) -> Result<(), ShimError> {
        self.kill().await?;
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, self.rx.recv()).await {
                Ok(Some(ShimFrame::Exited { .. })) | Ok(None) => break,
                Ok(Some(_)) => continue,
                Err(_) => break,
            }
        }
        Ok(())
    }
}

/// Probe whether a shim UDS socket is alive without triggering ring-buffer replay.
/// Removes stale socket files ONLY when the listener is definitively gone (`ConnectionRefused`/`NotFound`).
pub async fn probe_existing_shim(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    match tokio::time::timeout(Duration::from_millis(300), connect_shim_stream(socket_path)).await {
        Ok(Ok(_stream)) => true,
        Ok(Err(err))
            if matches!(
                err.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            let _ = std::fs::remove_file(socket_path);
            let _ = std::fs::remove_file(pid_file_path(socket_path));
            false
        }
        _ => false,
    }
}

/// Attempt to reconnect to an already-running `orbit shim` UDS socket (e.g. after Daemon hot-restart).
/// Only unlinks stale socket files on `ConnectionRefused` / `NotFound` — NEVER on timeout or transient error.
pub async fn connect_existing_shim(socket_path: &Path) -> Option<ShimClient> {
    if !socket_path.exists() {
        return None;
    }
    match tokio::time::timeout(Duration::from_millis(300), ShimClient::connect(socket_path)).await {
        Ok(Ok(client)) => Some(client),
        Ok(Err(ShimError::Io(ref err)))
            if matches!(
                err.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            let _ = std::fs::remove_file(socket_path);
            let _ = std::fs::remove_file(pid_file_path(socket_path));
            None
        }
        _ => None,
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
        Ok(Err(ShimError::Io(ref err)))
            if matches!(
                err.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            let _ = std::fs::remove_file(socket_path);
            let _ = std::fs::remove_file(pid_file_path(socket_path));
            None
        }
        _ => None,
    }
}

/// Best-effort kill for a surviving shim socket (and/or socketless `.pid` sidecar) before replacement.
pub async fn kill_existing_shim(socket_path: &Path) {
    let pid_rec = read_pid_file(socket_path);
    if let Some(mut client) = connect_existing_shim(socket_path).await {
        let _ = client.kill_and_wait(Duration::from_millis(400)).await;
    }
    if let Some(rec) = pid_rec {
        reap_recorded_shim_pid(&rec, socket_path).await;
    }
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_file_path(socket_path));
}

/// Synchronous best-effort kill for a shim socket (safe to call from RAII `Drop` guards in tests).
pub fn kill_existing_shim_sync(socket_path: &Path) {
    let pid_rec = read_pid_file(socket_path);
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
    #[cfg(not(unix))]
    {
        if let Ok(endpoint) = read_tcp_endpoint(socket_path) {
            let addr = std::net::SocketAddr::from(([127, 0, 0, 1], endpoint.port));
            if let Ok(mut stream) =
                std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200))
            {
                let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                let auth_line = encode_shim_auth_line(&endpoint.token);
                let _ = stream.write_all(auth_line.as_bytes());
                let _ = stream.write_all(b"{\"type\":\"kill\"}\n");
                let _ = stream.flush();
                drop(stream);
                std::thread::sleep(Duration::from_millis(30));
            }
        }
    }
    if let Some(rec) = pid_rec {
        reap_recorded_shim_pid_sync(&rec, socket_path);
    }
    let _ = std::fs::remove_file(socket_path);
    let _ = std::fs::remove_file(pid_file_path(socket_path));
}

/// Synchronously kill every active shim whose `.sock` or `.pid` lives in `shims_dir`.
pub fn kill_all_shims_in_dir_sync(shims_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(shims_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("sock") | Some("pid")
        ) {
            kill_existing_shim_sync(&path.with_extension("sock"));
        }
    }
}

/// Spawn a detached `orbit shim` process in its own OS process group and connect to its UDS.
/// Reaps any surviving shim for the same `socket_path` (via UDS or `.pid` sidecar) first so two
/// shims for the same agent/PTY can never coexist.
pub async fn spawn_detached_shim(
    orbit_exe: &Path,
    socket_path: &Path,
    spec: &ShimSpawnSpec,
) -> Result<ShimClient, ShimError> {
    if socket_path.exists() || pid_file_path(socket_path).exists() {
        kill_existing_shim(socket_path).await;
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        std_cmd.creation_flags(WIN_CREATE_NEW_PROCESS_GROUP | WIN_CREATE_NO_WINDOW);
    }

    let _child = std_cmd.spawn()?;

    // Poll for shim listener readiness (Windows cold process + ConPTY startup can exceed 3s under AV scanning).
    let max_polls = if cfg!(windows) { 500 } else { 250 };
    for _ in 0..max_polls {
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

/// RAII guard that guarantees the shim Unix socket and `.pid` sidecar are unlinked on exit
/// without accidentally clobbering a newer replacement shim's files.
struct SocketCleanupGuard {
    socket_path: PathBuf,
    owner_pid: u32,
}

impl SocketCleanupGuard {
    fn new(socket_path: PathBuf) -> Self {
        Self {
            socket_path,
            owner_pid: std::process::id(),
        }
    }
}

impl Drop for SocketCleanupGuard {
    fn drop(&mut self) {
        let should_clean = match read_pid_file(&self.socket_path) {
            Some(rec) => rec.shim_pid == self.owner_pid,
            None => false,
        };
        if should_clean {
            let _ = std::fs::remove_file(&self.socket_path);
            let _ = std::fs::remove_file(pid_file_path(&self.socket_path));
        }
    }
}

/// Probe every `*.sock` entry under `shims_dir` and unlink orphaned sockets whose listener has
/// already exited (`ConnectionRefused` / `NotFound`), along with stale `.pid` sidecars.
/// Active shims survive untouched.
pub async fn cleanup_stale_shim_sockets(shims_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(shims_dir) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("sock") {
            if let Ok(Err(err)) =
                tokio::time::timeout(Duration::from_millis(150), connect_shim_stream(&path)).await
                && matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                )
                && std::fs::remove_file(&path).is_ok()
            {
                let pid_path = pid_file_path(&path);
                if read_pid_file(&path).is_none_or(|rec| !is_pid_alive(rec.shim_pid)) {
                    let _ = std::fs::remove_file(pid_path);
                }
                removed += 1;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("pid") {
            let sock_path = path.with_extension("sock");
            if !sock_path.exists()
                && read_pid_file(&sock_path).is_none_or(|rec| !is_pid_alive(rec.shim_pid))
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    removed
}

async fn run_stdio_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    prepare_socket_path(socket_path)?;
    let listener = ShimListener::bind(socket_path).await?;
    let _socket_guard = SocketCleanupGuard::new(socket_path.to_path_buf());
    write_pid_file(socket_path, std::process::id(), None, None);

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
    #[cfg(windows)]
    {
        cmd.creation_flags(WIN_CREATE_NEW_PROCESS_GROUP | WIN_CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn()?;
    let child_pid = child.id();
    #[cfg(unix)]
    let child_pgid = child_pid;
    #[cfg(not(unix))]
    let child_pgid = None;
    write_pid_file(socket_path, std::process::id(), child_pid, child_pgid);
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
                if let Ok(stream) = accept_res {
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
                let code = terminate_child_process_group(&mut child).await;
                let _ = out_tx.send(ShimFrame::Exited { code });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
            wait_res = child.wait() => {
                let code = wait_res.ok().and_then(|s| s.code());
                let _ = out_tx.send(ShimFrame::Exited { code });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
        }
    }

    Ok(())
}

async fn run_pty_shim(
    socket_path: &Path,
    spec: ShimSpawnSpec,
    cols: u16,
    rows: u16,
) -> Result<(), ShimError> {
    prepare_socket_path(socket_path)?;

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
    let pty_child_pid = child.process_id();
    let listener = ShimListener::bind(socket_path).await?;
    let _socket_guard = SocketCleanupGuard::new(socket_path.to_path_buf());
    write_pid_file(
        socket_path,
        std::process::id(),
        pty_child_pid,
        pty_child_pid,
    );
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
                if let Ok(stream) = accept_res {
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
                #[cfg(unix)]
                if let Some(pid) = pty_child_pid {
                    signal_pid_group(pid, "-TERM");
                }
                #[cfg(windows)]
                if let Some(pid) = pty_child_pid {
                    kill_process_tree_async(pid).await;
                }
                let _ = child_killer.kill();
                #[cfg(unix)]
                if let Some(pid) = pty_child_pid {
                    signal_pid_group(pid, "-KILL");
                }
                let _ = out_tx.send(ShimFrame::Exited { code: None });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
            maybe_code = exit_rx.recv() => {
                let _ = out_tx.send(ShimFrame::Exited { code: maybe_code.flatten() });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
        }
    }

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
    stream: ShimStream,
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
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    eprintln!(
                        "[orbit-shim] stdio daemon attach lagged by {skipped} frame(s); continuing from broadcast channel"
                    );
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn handle_daemon_attach(
    stream: ShimStream,
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let filter = format!("PID eq {pid}");
        std::process::Command::new("tasklist")
            .args(["/FI", &filter, "/NH", "/FO", "CSV"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(WIN_CREATE_NO_WINDOW)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                parse_windows_tasklist_csv_match(&stdout, pid, None)
            })
    }
    #[cfg(not(any(unix, windows)))]
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

    // On macOS, `UnixListener::bind` lacks atomic `SOCK_CLOEXEC`, so concurrent `fork()` calls
    // from sibling tests in the same binary can momentarily inherit a test listener FD before
    // `fcntl(FD_CLOEXEC)` runs. Serialize fork-spawning / socket-cleanup tests cleanly.
    static SHIM_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_shim_survives_daemon_disconnect_and_replays_ring() {
        let _guard = SHIM_TEST_LOCK.lock().await;
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

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_shim_cursor_attach_skips_consumed_frames_and_delivers_only_new_ones() {
        let _guard = SHIM_TEST_LOCK.lock().await;
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
        let listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind fake legacy shim");

        // Simulate a legacy v0.8.0 shim that ignores `Attach` and dumps unsequenced ring frames.
        let server = tokio::spawn(async move {
            if let Ok(mut stream) = listener.accept().await {
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
        let _guard = SHIM_TEST_LOCK.lock().await;
        let dir = std::env::temp_dir().join(format!("orbit_stale_sock_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");

        let live_sock = dir.join("live.sock");
        let _live_listener = ShimListener::bind(&live_sock).await.expect("bind live");

        let dead_sock = dir.join("dead.sock");
        {
            let _dead_listener = ShimListener::bind(&dead_sock).await.expect("bind dead");
        } // listener dropped -> stale socket file remains on disk

        assert!(dead_sock.exists());
        let removed = cleanup_stale_shim_sockets(&dir).await;
        assert_eq!(removed, 1);
        assert!(!dead_sock.exists(), "dead socket should be unlinked");
        assert!(live_sock.exists(), "live socket must remain untouched");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn parent_watchdog_terminates_shim_when_watched_pid_is_gone() {
        let _guard = SHIM_TEST_LOCK.lock().await;
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
        assert!(
            !pid_file_path(&sock_path).exists(),
            "pid sidecar must be unlinked on exit"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn probe_and_connect_keep_live_socket_and_pid_sidecar_cleans_on_exit() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let sock_path =
            std::env::temp_dir().join(format!("orbit_pid_sidecar_{}.sock", std::process::id()));
        let pid_path = pid_file_path(&sock_path);
        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_file(&pid_path);

        let spec = ShimSpawnSpec {
            shim_id: "test_pid_sidecar".into(),
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
            if sock_path.exists() && pid_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert!(pid_path.exists(), "shim must write .pid sidecar");
        let rec = read_pid_file(&sock_path).expect("valid pid record");
        assert_eq!(rec.shim_pid, std::process::id());
        assert!(rec.child_pid.is_some());
        assert!(probe_existing_shim(&sock_path).await);

        let mut client = connect_existing_shim(&sock_path)
            .await
            .expect("connect existing");
        client
            .kill_and_wait(Duration::from_millis(500))
            .await
            .expect("kill_and_wait");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        assert!(!sock_path.exists(), "socket cleaned after kill");
        assert!(!pid_path.exists(), "pid sidecar cleaned after kill");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_shim_lagged_broadcast_resyncs_without_detaching() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let sock_path =
            std::env::temp_dir().join(format!("orbit_lag_resync_{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock_path);
        let spec = ShimSpawnSpec {
            shim_id: "test_lag".into(),
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

        let mut client = ShimClient::connect(&sock_path).await.expect("connect");
        // Write > 1100 lines quickly without draining `client.rx` immediately so broadcast lags
        let writer = ShimClient::connect(&sock_path).await.expect("writer");
        for i in 0..1200 {
            writer
                .write_stdin_line(format!("burst-{i}"))
                .await
                .expect("write burst");
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
        writer
            .write_stdin_line("sentinel-after-lag")
            .await
            .expect("write sentinel");

        let mut received_count = 0usize;
        let mut saw_sentinel = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(300), client.rx.recv()).await {
                Ok(Some(ShimFrame::StdoutLine { line, .. })) => {
                    received_count += 1;
                    if line == "sentinel-after-lag" {
                        saw_sentinel = true;
                        break;
                    }
                }
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => break,
            }
        }
        assert!(
            saw_sentinel,
            "daemon attach must resync across broadcast lag instead of detaching"
        );
        assert!(
            received_count > STDIO_RING_CAPACITY,
            "lagged attach must preserve frames buffered in 1024-slot broadcast channel (> {STDIO_RING_CAPACITY}), got {received_count}"
        );

        client
            .kill_and_wait(Duration::from_millis(500))
            .await
            .expect("kill");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_shim_kill_terminates_child_process_group_grandchildren() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let sock_path =
            std::env::temp_dir().join(format!("orbit_pgid_kill_{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&sock_path);
        let spec = ShimSpawnSpec {
            shim_id: "test_pgid".into(),
            mode: ShimMode::Stdio,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 30 & echo $!; wait".into()],
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

        let mut client = ShimClient::connect(&sock_path).await.expect("connect");
        let grandchild_pid: u32 = loop {
            let frame = tokio::time::timeout(Duration::from_secs(2), client.rx.recv())
                .await
                .expect("grandchild pid timeout")
                .expect("frame");
            if let ShimFrame::StdoutLine { line, .. } = frame
                && let Ok(pid) = line.trim().parse::<u32>()
            {
                break pid;
            }
        };
        assert!(
            is_pid_alive(grandchild_pid),
            "grandchild sleep must be alive before kill"
        );

        client
            .kill_and_wait(Duration::from_millis(600))
            .await
            .expect("kill_and_wait");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;

        let mut died = false;
        for _ in 0..20 {
            if !is_pid_alive(grandchild_pid) {
                died = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            died,
            "grandchild process {grandchild_pid} must be terminated when shim kills process group"
        );
    }

    #[tokio::test]
    async fn connect_existing_shim_does_not_unlink_on_non_refused_timeout() {
        // Bind a listening socket that never accepts connections, and connect via a cursor handshake
        // that cannot fail with ConnectionRefused. Wait: on macOS UnixStream::connect succeeds into
        // the kernel backlog immediately, so `connect_with_cursor` will time out waiting for AttachAck
        // (returning Ok(client) after 180ms) — verify the socket is NEVER unlinked!
        let sock_path = std::env::temp_dir().join(format!(
            "orbit_no_unlink_timeout_{}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&sock_path);
        let _listener = ShimListener::bind(&sock_path).await.expect("bind listener");

        #[cfg(unix)]
        assert!(probe_existing_shim(&sock_path).await);
        #[cfg(not(unix))]
        let _ = probe_existing_shim(&sock_path).await;
        assert!(
            sock_path.exists(),
            "live socket must remain on disk after non-refused timeout"
        );

        let _ = std::fs::remove_file(&sock_path);
    }

    #[test]
    fn windows_tcp_endpoint_descriptor_and_auth_handshake_roundtrip() {
        let dir = std::env::temp_dir().join(format!("orbit_win_ep_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let ep_path = dir.join("agent.sock");

        let token = generate_shim_auth_token();
        assert_eq!(token.len(), 64);
        let ep = ShimTcpEndpoint {
            port: 49152,
            token: token.clone(),
        };
        write_tcp_endpoint_atomic(&ep_path, &ep).expect("write endpoint");
        let loaded = read_tcp_endpoint(&ep_path).expect("read endpoint");
        assert_eq!(loaded, ep);

        let line = encode_shim_auth_line(&token);
        assert!(verify_shim_auth_line(&line, &token));
        assert!(!verify_shim_auth_line(&line, "wrong-token"));
        assert!(!verify_shim_auth_line("{\"type\":\"kill\"}", &token));

        let ack = encode_shim_auth_ack_line();
        assert!(verify_shim_auth_ack_line(ack));
        assert!(!verify_shim_auth_ack_line("{\"auth_ok\":false}\n"));
        assert!(!verify_shim_auth_ack_line("HTTP/1.1 400 Bad Request\r\n"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_tasklist_csv_parser_matches_pid_and_image_prefix() {
        let csv = "\"orbit.exe\",\"4242\",\"Console\",\"1\",\"18,432 K\"\r\n\"notepad.exe\",\"9999\",\"Console\",\"1\",\"4,000 K\"\r\n";
        assert!(parse_windows_tasklist_csv_match(csv, 4242, None));
        assert!(parse_windows_tasklist_csv_match(csv, 4242, Some("orbit")));
        assert!(!parse_windows_tasklist_csv_match(csv, 9999, Some("orbit")));
        assert!(!parse_windows_tasklist_csv_match(csv, 1234, None));
        assert!(!parse_windows_tasklist_csv_match(
            "INFO: No tasks are running which match the specified criteria.\r\n",
            4242,
            None
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reap_recorded_shim_pid_requires_matching_socket_and_guards_child_pgid() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        // Spawn a harmless sleep process whose cmdline does NOT match `shim --socket <path>`
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();
        let sock_path = std::env::temp_dir().join("orbit_expected_match.sock");

        assert!(
            !is_orbit_shim_process(pid, &sock_path),
            "non-shim process must not match is_orbit_shim_process"
        );
        // Even if a stale `.pid` file pointed at this recycled PID, reap MUST leave it untouched!
        let stale_rec = ShimPidRecord {
            shim_pid: pid,
            child_pid: Some(pid),
            child_pgid: Some(pid),
        };
        reap_recorded_shim_pid(&stale_rec, &sock_path).await;
        assert!(
            is_pid_alive(pid),
            "reap_recorded_shim_pid must NEVER kill a process whose cmdline doesn't match socket_path"
        );

        let _ = child.kill();
        let _ = child.wait();
    }
}
