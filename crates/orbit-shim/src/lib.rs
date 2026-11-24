//! Detached process & PTY supervisor (`orbit shim`).
//!
//! Keeps `agy`, `codex`, `claude`, and interactive PTY shells alive across `orbit daemon`
//! restarts or binary hot-upgrades via Unix Domain Sockets on Unix and authenticated
//! loopback TCP endpoints (`127.0.0.1` + per-shim 256-bit token descriptor) on Windows
//! (`$ORBIT_HOME/shims/<id>.sock`).

pub mod sandbox;
pub use sandbox::{SandboxProbeStatus, probe_sandbox_support};
use std::collections::{HashMap, VecDeque};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
#[cfg(unix)]
use std::sync::OnceLock;
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
            use std::os::unix::fs::PermissionsExt;
            validate_unix_socket_path_len(socket_path)?;
            if let Some(parent) = socket_path.parent() {
                ensure_shim_parent_dir(parent)?;
            }
            reject_if_symlink(socket_path)?;
            let inner = tokio::net::UnixListener::bind(socket_path)?;
            std::fs::set_permissions(socket_path, std::fs::Permissions::from_mode(0o600))?;
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
        let path = pid_file_path(socket_path);
        if std::fs::write(&path, json).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

fn read_pid_file(socket_path: &Path) -> Option<ShimPidRecord> {
    let raw = std::fs::read_to_string(pid_file_path(socket_path)).ok()?;
    serde_json::from_str::<ShimPidRecord>(&raw).ok()
}

#[cfg(unix)]
const UNIX_KILL_BINS: [&str; 3] = ["/bin/kill", "/usr/bin/kill", "kill"];

#[cfg(unix)]
fn build_kill_pid_args(pid: u32, signal: &str) -> Option<[String; 4]> {
    if pid <= 1 {
        return None;
    }
    let sig = signal.trim().trim_start_matches('-');
    if sig.is_empty() {
        return None;
    }
    Some([
        "-s".to_string(),
        sig.to_string(),
        "--".to_string(),
        pid.to_string(),
    ])
}

#[cfg(unix)]
fn build_kill_pgid_args(pgid: u32, signal: &str) -> Option<[String; 4]> {
    if pgid <= 1 {
        return None;
    }
    let sig = signal.trim().trim_start_matches('-');
    if sig.is_empty() {
        return None;
    }
    Some([
        "-s".to_string(),
        sig.to_string(),
        "--".to_string(),
        format!("-{pgid}"),
    ])
}

#[cfg(unix)]
fn run_unix_kill(args: [String; 4]) {
    for bin in UNIX_KILL_BINS {
        let status = std::process::Command::new(bin)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.is_ok() {
            break;
        }
    }
}

#[cfg(unix)]
fn signal_pid(pid: u32, signal: &str) {
    if let Some(args) = build_kill_pid_args(pid, signal) {
        run_unix_kill(args);
    }
}

#[cfg(unix)]
fn signal_pid_group(pgid: u32, signal: &str) {
    if let Some(args) = build_kill_pgid_args(pgid, signal) {
        run_unix_kill(args);
    }
}

#[cfg(unix)]
fn is_orbit_shim_process(pid: u32, socket_path: &Path) -> bool {
    if pid <= 1 || pid == std::process::id() {
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

/// Terminate a child and its process group, returning `(reaped, exit_code)` where `reaped` is
/// `true` only once `child.wait()` has confirmed the child actually exited.
async fn terminate_child_process_group(child: &mut tokio::process::Child) -> (bool, Option<i32>) {
    #[cfg(unix)]
    let child_pid = child.id();
    #[cfg(unix)]
    if let Some(pid) = child_pid {
        // Child was spawned with `process_group(0)` so its PGID equals its PID.
        signal_pid_group(pid, "-TERM");
        if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_millis(150), child.wait()).await
        {
            signal_pid_group(pid, "-KILL");
            return (true, status.code());
        }
        signal_pid_group(pid, "-KILL");
    }
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        kill_process_tree_async(pid).await;
    }
    let _ = child.kill().await;
    match tokio::time::timeout(Duration::from_secs(1), child.wait()).await {
        Ok(Ok(status)) => (true, status.code()),
        _ => (false, None),
    }
}

async fn reap_recorded_shim_pid(rec: &ShimPidRecord, socket_path: &Path) {
    #[cfg(unix)]
    {
        if rec.shim_pid <= 1 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        let safe_child_pgid = rec
            .child_pgid
            .filter(|&pgid| pgid > 1 && pgid != std::process::id() && pgid != rec.shim_pid);
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
        if rec.shim_pid <= 1 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        if let Some(child_pid) = rec
            .child_pid
            .filter(|&p| p > 1 && p != std::process::id() && p != rec.shim_pid)
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
        if rec.shim_pid <= 1 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        let safe_child_pgid = rec
            .child_pgid
            .filter(|&pgid| pgid > 1 && pgid != std::process::id() && pgid != rec.shim_pid);
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
        if rec.shim_pid <= 1 || rec.shim_pid == std::process::id() {
            return;
        }
        if !is_pid_alive(rec.shim_pid) || !is_orbit_shim_process(rec.shim_pid, socket_path) {
            return;
        }
        if let Some(child_pid) = rec
            .child_pid
            .filter(|&p| p > 1 && p != std::process::id() && p != rec.shim_pid)
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShimMode {
    Stdio,
    Pty { cols: u16, rows: u16 },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShimSpawnSpec {
    #[serde(default)]
    pub sandbox: Option<orbit_protocol::SandboxPolicy>,
    #[serde(default)]
    pub egress_proxy_url: Option<String>,
    pub shim_id: String,
    pub mode: ShimMode,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    /// Optional private ephemeral files (such as `0600` Claude MCP configs) owned by this shim
    /// generation and unlinked right after the managed child process actually exits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cleanup_paths: Vec<PathBuf>,
}

impl std::fmt::Debug for ShimSpawnSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShimSpawnSpec")
            .field("shim_id", &self.shim_id)
            .field("mode", &self.mode)
            .field("sandbox", &self.sandbox.as_ref().map(|p| p.mode))
            .finish_non_exhaustive()
    }
}
fn path_contains_nul(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().contains(&0)
    }
    #[cfg(not(unix))]
    {
        path.to_string_lossy().as_bytes().contains(&0)
    }
}

fn validate_spawn_spec(spec: &ShimSpawnSpec) -> Result<(), ShimError> {
    if spec.program.trim().is_empty() {
        return Err(ShimError::InvalidSpec(
            "shim program must not be empty".into(),
        ));
    }
    if spec.program.as_bytes().contains(&0) {
        return Err(ShimError::InvalidSpec(
            "shim program must not contain NUL bytes".into(),
        ));
    }
    if path_contains_nul(&spec.cwd) {
        return Err(ShimError::InvalidSpec(
            "shim cwd must not contain NUL bytes".into(),
        ));
    }
    for arg in &spec.args {
        if arg.as_bytes().contains(&0) {
            return Err(ShimError::InvalidSpec(
                "shim args must not contain NUL bytes".into(),
            ));
        }
    }
    for (k, v) in &spec.env {
        if k.is_empty() || k.as_bytes().contains(&0) || k.contains('=') {
            return Err(ShimError::InvalidSpec(
                "shim env key must be non-empty and must not contain NUL or '='".into(),
            ));
        }
        if v.as_bytes().contains(&0) {
            return Err(ShimError::InvalidSpec(
                "shim env value must not contain NUL bytes".into(),
            ));
        }
    }
    for path in &spec.cleanup_paths {
        if path_contains_nul(path) {
            return Err(ShimError::InvalidSpec(
                "shim cleanup_paths must not contain NUL bytes".into(),
            ));
        }
    }
    Ok(())
}

fn unlink_cleanup_paths(paths: &[PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

/// RAII guard that unlinks `cleanup_paths` only if shim startup fails BEFORE the managed child
/// process is spawned. Once the child is launched, call [`Self::disarm`] so cleanup is performed
/// strictly after the child actually exits.
struct PreSpawnCleanupGuard<'a> {
    paths: &'a [PathBuf],
    armed: bool,
}

impl<'a> PreSpawnCleanupGuard<'a> {
    fn new(paths: &'a [PathBuf]) -> Self {
        Self { paths, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PreSpawnCleanupGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            unlink_cleanup_paths(self.paths);
        }
    }
}

/// Hard upper bound (4 MiB) for a serialized [`ShimSpawnSpec`] read from stdin.
pub const MAX_SHIM_SPEC_BYTES: usize = 4 * 1024 * 1024;

/// Read and parse a [`ShimSpawnSpec`] JSON payload from an arbitrary reader (capped at [`MAX_SHIM_SPEC_BYTES`]).
pub fn read_spawn_spec_from_reader<R: Read>(reader: R) -> Result<ShimSpawnSpec, ShimError> {
    let mut buf = Vec::new();
    reader
        .take(MAX_SHIM_SPEC_BYTES as u64 + 1)
        .read_to_end(&mut buf)?;
    if buf.len() > MAX_SHIM_SPEC_BYTES {
        return Err(ShimError::InvalidSpec(format!(
            "shim spawn spec exceeds {MAX_SHIM_SPEC_BYTES} byte limit"
        )));
    }
    serde_json::from_slice::<ShimSpawnSpec>(&buf).map_err(ShimError::Json)
}

/// Read and parse a [`ShimSpawnSpec`] JSON payload piped over `stdin` into `orbit shim`.
pub fn read_spawn_spec_from_stdin() -> Result<ShimSpawnSpec, ShimError> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(ShimError::InvalidSpec(
            "shim spawn spec must be piped via stdin".into(),
        ));
    }
    read_spawn_spec_from_reader(stdin.lock())
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
    #[error("invalid shim spawn spec: {0}")]
    InvalidSpec(String),
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
    attach_ack_seq: Option<u64>,
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
        let mut attach_ack_seq: Option<u64> = None;
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
                        ShimFrame::AttachAck { latest_seq } => {
                            // Modern cursor-aware shim confirmed our cursor; every subsequent
                            // frame is strictly unseen (`seq > after_seq`).
                            attach_ack_seq = Some(latest_seq);
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
            attach_ack_seq,
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
            attach_ack_seq: None,
        }
    }

    /// Returns the shim's latest frame sequence reported in `AttachAck` upon cursor-aware attach.
    pub fn attach_ack_seq(&self) -> Option<u64> {
        self.attach_ack_seq
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
pub async fn shim_socket_reachable(path: &Path) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(2), connect_shim_stream(path)).await,
        Ok(Ok(_))
    )
}

pub async fn probe_existing_shim(socket_path: &Path) -> bool {
    if !socket_path.exists() {
        return false;
    }
    // On Windows loopback TCP, a closed port takes ~1-2s of SYN retries before WSAECONNREFUSED.
    let timeout = if cfg!(unix) {
        Duration::from_millis(300)
    } else {
        Duration::from_millis(3000)
    };
    match tokio::time::timeout(timeout, connect_shim_stream(socket_path)).await {
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
    let timeout = if cfg!(unix) {
        Duration::from_millis(300)
    } else {
        Duration::from_millis(3000)
    };
    match tokio::time::timeout(timeout, ShimClient::connect(socket_path)).await {
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
    let timeout = if cfg!(unix) {
        Duration::from_millis(500)
    } else {
        Duration::from_millis(3000)
    };
    match tokio::time::timeout(
        timeout,
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

/// Maximum usable Unix Domain Socket path length in bytes (excluding trailing NUL):
/// Darwin/BSD `sun_path` is 104 bytes (`103` usable); Linux `sun_path` is 108 bytes (`107` usable).
#[cfg(unix)]
pub const MAX_UNIX_SOCKET_PATH_BYTES: usize = if cfg!(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd",
    target_os = "dragonfly"
)) {
    103
} else {
    107
};

#[cfg(unix)]
pub fn validate_unix_socket_path_len(socket_path: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let len = socket_path.as_os_str().as_bytes().len();
    if len > MAX_UNIX_SOCKET_PATH_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "unix socket path ({len} bytes) exceeds platform sun_path limit ({MAX_UNIX_SOCKET_PATH_BYTES} bytes): {}",
                socket_path.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn current_unix_uid() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    static CACHED_UID: OnceLock<u32> = OnceLock::new();
    if let Some(&uid) = CACHED_UID.get() {
        return Ok(uid);
    }
    #[cfg(target_os = "linux")]
    if let Ok(meta) = std::fs::metadata("/proc/self") {
        let uid = meta.uid();
        let _ = CACHED_UID.set(uid);
        return Ok(uid);
    }
    if let Ok(out) = std::process::Command::new("id")
        .arg("-u")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        && out.status.success()
        && let Ok(uid) = String::from_utf8_lossy(&out.stdout).trim().parse::<u32>()
    {
        let _ = CACHED_UID.set(uid);
        return Ok(uid);
    }
    let probe = std::env::temp_dir().join(format!(
        ".orbit-uid-probe-{}-{}",
        std::process::id(),
        ENDPOINT_TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let meta = file.metadata()?;
    drop(file);
    let _ = std::fs::remove_file(&probe);
    let uid = meta.uid();
    let _ = CACHED_UID.set(uid);
    Ok(uid)
}

#[cfg(unix)]
fn reject_if_symlink(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("refusing to use symlink path: {}", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Create (with mode `0700` on Unix) and verify a private directory: real directory (never a symlink),
/// owned by the current UID, and strictly mode `0700`.
pub fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        let my_uid = current_unix_uid()?;
        match std::fs::symlink_metadata(dir) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "refusing to use symlinked private directory: {}",
                            dir.display()
                        ),
                    ));
                }
                if !meta.is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "private directory path is not a directory: {}",
                            dir.display()
                        ),
                    ));
                }
                if meta.uid() != my_uid {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "private directory {} is owned by uid {}, expected {}",
                            dir.display(),
                            meta.uid(),
                            my_uid
                        ),
                    ));
                }
                if meta.permissions().mode() & 0o777 != 0o700 {
                    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = std::fs::DirBuilder::new();
                builder.recursive(true).mode(0o700);
                if let Err(create_err) = builder.create(dir)
                    && create_err.kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(create_err);
                }
            }
            Err(err) => return Err(err),
        }

        let meta = std::fs::symlink_metadata(dir)?;
        if meta.file_type().is_symlink()
            || !meta.is_dir()
            || meta.uid() != my_uid
            || meta.permissions().mode() & 0o777 != 0o700
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "private directory {} failed security verification (symlink={}, dir={}, uid={}, mode={:o})",
                    dir.display(),
                    meta.file_type().is_symlink(),
                    meta.is_dir(),
                    meta.uid(),
                    meta.permissions().mode() & 0o777
                ),
            ));
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir)
    }
}

/// Resolve a private shim socket path under `home_dir/shims/<filename>` or, if that exceeds the
/// platform's Unix `sun_path` limit, under a verified per-UID `0700` fallback directory (`os-<uid>`).
pub fn resolve_private_shim_socket_path(
    home_dir: &Path,
    filename: &str,
    home_tag: &str,
) -> std::io::Result<PathBuf> {
    let shims_dir = home_dir.join("shims");
    let primary = shims_dir.join(filename);
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if primary.as_os_str().as_bytes().len() <= MAX_UNIX_SOCKET_PATH_BYTES {
            ensure_private_dir(&shims_dir)?;
            return Ok(primary);
        }
        let uid = current_unix_uid()?;
        let fallback_name = format!("{home_tag}_{filename}");
        let temp_candidate_dir = std::env::temp_dir().join(format!("os-{uid}"));
        let fallback_dir = if temp_candidate_dir
            .join(&fallback_name)
            .as_os_str()
            .as_bytes()
            .len()
            <= MAX_UNIX_SOCKET_PATH_BYTES
        {
            temp_candidate_dir
        } else {
            PathBuf::from("/tmp").join(format!("os-{uid}"))
        };
        ensure_private_dir(&fallback_dir)?;
        let final_path = fallback_dir.join(&fallback_name);
        validate_unix_socket_path_len(&final_path)?;
        Ok(final_path)
    }
    #[cfg(not(unix))]
    {
        let _ = home_tag;
        std::fs::create_dir_all(&shims_dir)?;
        Ok(primary)
    }
}

fn ensure_shim_parent_dir(parent: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let is_private_named = parent
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == "shims" || n.starts_with("os-"));
        if is_private_named {
            return ensure_private_dir(parent);
        }
        match std::fs::metadata(parent) {
            Ok(meta) if meta.is_dir() => Ok(()),
            Ok(_) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("shim parent is not a directory: {}", parent.display()),
            )),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => ensure_private_dir(parent),
            Err(err) => Err(err),
        }
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(parent)
    }
}

fn recorded_provider_targets(socket_path: &Path, shim_pid: u32) -> (Option<u32>, Option<u32>) {
    match read_pid_file(socket_path) {
        Some(rec) if rec.shim_pid == shim_pid => {
            let safe_child_pid = rec
                .child_pid
                .filter(|&p| p > 1 && p != std::process::id() && p != shim_pid);
            let safe_child_pgid = rec
                .child_pgid
                .filter(|&pg| pg > 1 && pg != std::process::id() && pg != shim_pid);
            (safe_child_pid, safe_child_pgid)
        }
        _ => (None, None),
    }
}

struct SpawnedChildGuard {
    child: Option<tokio::process::Child>,
    socket_path: PathBuf,
}

impl SpawnedChildGuard {
    fn new(child: tokio::process::Child, socket_path: PathBuf) -> Self {
        Self {
            child: Some(child),
            socket_path,
        }
    }

    fn take_stdin(&mut self) -> Option<tokio::process::ChildStdin> {
        self.child.as_mut().and_then(|c| c.stdin.take())
    }

    fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        match self.child.as_mut() {
            Some(c) => c.try_wait(),
            None => Ok(None),
        }
    }

    async fn wait_exit(&mut self) -> std::io::Result<std::process::ExitStatus> {
        match self.child.as_mut() {
            Some(c) => c.wait().await,
            None => Err(std::io::Error::other("missing spawned shim child")),
        }
    }

    fn disarm(&mut self) {
        self.child = None;
    }

    async fn kill_and_reap(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };
        let shim_pid = child.id();
        // If the shim already recorded a spawned provider process group in its `.pid`
        // sidecar, signal the provider group alongside the shim group so startup aborts
        // never leave an orphaned provider process group behind.
        #[cfg(unix)]
        if let Some(pid) = shim_pid {
            let (_, child_pgid) = recorded_provider_targets(&self.socket_path, pid);
            if let Some(pgid) = child_pgid {
                signal_pid_group(pgid, "-TERM");
            }
        }
        #[cfg(windows)]
        if let Some(pid) = shim_pid {
            let (child_pid, _) = recorded_provider_targets(&self.socket_path, pid);
            if let Some(cpid) = child_pid {
                kill_process_tree_async(cpid).await;
            }
        }

        let (reaped, _) = terminate_child_process_group(child).await;

        // Re-read the `.pid` sidecar after terminating the shim in case it finished
        // spawning and recording its provider child right before exiting.
        if let Some(pid) = shim_pid {
            #[cfg(unix)]
            {
                let (_, child_pgid) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(pgid) = child_pgid {
                    signal_pid_group(pgid, "-KILL");
                }
            }
            #[cfg(windows)]
            {
                let (child_pid, _) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(cpid) = child_pid {
                    kill_process_tree_async(cpid).await;
                }
            }

            if read_pid_file(&self.socket_path).is_some_and(|rec| rec.shim_pid == pid) {
                let _ = std::fs::remove_file(&self.socket_path);
                let _ = std::fs::remove_file(pid_file_path(&self.socket_path));
            }
        }

        if reaped {
            self.child = None;
        }
    }
}

impl Drop for SpawnedChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let shim_pid = child.id();
            #[cfg(unix)]
            if let Some(pid) = shim_pid {
                let (_, child_pgid) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(pgid) = child_pgid {
                    signal_pid_group(pgid, "-KILL");
                }
                signal_pid_group(pid, "-KILL");
                // Re-check in case the shim wrote child_pgid right before SIGKILL landed.
                let (_, post_pgid) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(pgid) = post_pgid
                    && post_pgid != child_pgid
                {
                    signal_pid_group(pgid, "-KILL");
                }
            }
            #[cfg(windows)]
            if let Some(pid) = shim_pid {
                let (child_pid, _) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(cpid) = child_pid {
                    kill_process_tree_sync(cpid);
                }
                kill_process_tree_sync(pid);
                let (post_cpid, _) = recorded_provider_targets(&self.socket_path, pid);
                if let Some(cpid) = post_cpid
                    && post_cpid != child_pid
                {
                    kill_process_tree_sync(cpid);
                }
            }
            let _ = child.start_kill();
            if let Some(pid) = shim_pid
                && read_pid_file(&self.socket_path).is_some_and(|rec| rec.shim_pid == pid)
            {
                let _ = std::fs::remove_file(&self.socket_path);
                let _ = std::fs::remove_file(pid_file_path(&self.socket_path));
            }
        }
    }
}

fn build_detached_shim_command(orbit_exe: &Path, socket_path: &Path) -> std::process::Command {
    let mut std_cmd = std::process::Command::new(orbit_exe);
    std_cmd
        .arg("shim")
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::piped())
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

    std_cmd
}

/// Spawn a detached `orbit shim` process in its own OS process group and connect to its UDS.
/// Validates spec size & socket path before reaping any surviving shim for `socket_path` so an
/// invalid replacement never destroys a healthy live session.
///
/// Passes `spec` JSON exclusively over the child's stdin pipe (never in `argv`) so secrets in
/// `spec.env` / `spec.args` never appear in `ps aux` or `/proc/<pid>/cmdline`.
pub async fn spawn_detached_shim(
    orbit_exe: &Path,
    socket_path: &Path,
    spec: &ShimSpawnSpec,
) -> Result<ShimClient, ShimError> {
    validate_spawn_spec(spec)?;
    let spec_json = serde_json::to_string(spec)?;
    if spec_json.len() > MAX_SHIM_SPEC_BYTES {
        return Err(ShimError::InvalidSpec(format!(
            "shim spawn spec exceeds {MAX_SHIM_SPEC_BYTES} byte limit"
        )));
    }
    #[cfg(unix)]
    validate_unix_socket_path_len(socket_path)?;
    if let Some(parent) = socket_path.parent() {
        ensure_shim_parent_dir(parent)?;
    }
    let err_sidecar = socket_path.with_extension("err");
    #[cfg(unix)]
    {
        reject_if_symlink(socket_path)?;
        reject_if_symlink(&pid_file_path(socket_path))?;
        reject_if_symlink(&err_sidecar)?;
    }

    if socket_path.exists() || pid_file_path(socket_path).exists() {
        kill_existing_shim(socket_path).await;
    }

    let _ = std::fs::remove_file(&err_sidecar);

    // Windows cold process + ConPTY startup can exceed 3s under AV scanning.
    let startup_timeout = if cfg!(windows) {
        Duration::from_secs(10)
    } else {
        Duration::from_secs(5)
    };
    let deadline = tokio::time::Instant::now() + startup_timeout;

    let mut tokio_cmd = Command::from(build_detached_shim_command(orbit_exe, socket_path));
    tokio_cmd.kill_on_drop(false);
    let mut child_guard = SpawnedChildGuard::new(
        spawn_command_retrying_busy(&mut tokio_cmd).await?,
        socket_path.to_path_buf(),
    );

    let mut child_stdin = child_guard.take_stdin().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::BrokenPipe, "missing shim stdin pipe")
    })?;
    let write_outcome = tokio::time::timeout_at(deadline, async {
        child_stdin.write_all(spec_json.as_bytes()).await?;
        child_stdin.flush().await?;
        child_stdin.shutdown().await?;
        Ok::<(), std::io::Error>(())
    })
    .await;
    // Close write end immediately so the child sees EOF.
    drop(child_stdin);

    match write_outcome {
        Ok(Ok(())) => {}
        Ok(Err(write_err)) => {
            // Treat any failed spec write (including BrokenPipe) as failed startup while
            // preserving child exit status and `.err` sidecar diagnostics.
            let wait_until = deadline.min(tokio::time::Instant::now() + Duration::from_millis(300));
            let exited_status = tokio::time::timeout_at(wait_until, child_guard.wait_exit())
                .await
                .ok()
                .and_then(Result::ok);
            let detail = std::fs::read_to_string(&err_sidecar).unwrap_or_default();
            let _ = std::fs::remove_file(&err_sidecar);
            let detail = detail.trim();
            if exited_status.is_some() {
                child_guard.disarm();
            } else {
                child_guard.kill_and_reap().await;
            }
            if let Some(status) = exited_status {
                return Err(ShimError::Pty(format!(
                    "shim process ({}) exited early ({status}) during spec delivery before {} became ready{}",
                    orbit_exe.display(),
                    socket_path.display(),
                    if detail.is_empty() {
                        String::new()
                    } else {
                        format!(": {detail}")
                    }
                )));
            }
            if !detail.is_empty() {
                return Err(ShimError::Pty(format!(
                    "shim spec delivery failed ({write_err}): {detail}"
                )));
            }
            return Err(ShimError::Io(write_err));
        }
        Err(_) => {
            child_guard.kill_and_reap().await;
            let _ = std::fs::remove_file(&err_sidecar);
            return Err(ShimError::ReadyTimeout(socket_path.to_path_buf()));
        }
    }

    let mut last_connect_err: Option<String> = None;
    while tokio::time::Instant::now() < deadline {
        if socket_path.exists() {
            match tokio::time::timeout_at(deadline, ShimClient::connect(socket_path)).await {
                Ok(Ok(client)) => {
                    let _ = std::fs::remove_file(&err_sidecar);
                    child_guard.disarm();
                    return Ok(client);
                }
                Ok(Err(err)) => {
                    last_connect_err = Some(err.to_string());
                }
                Err(_) => break,
            }
        }
        if let Ok(Some(status)) = child_guard.try_wait() {
            let detail = std::fs::read_to_string(&err_sidecar).unwrap_or_default();
            let _ = std::fs::remove_file(&err_sidecar);
            child_guard.disarm();
            let detail = detail.trim();
            return Err(ShimError::Pty(format!(
                "shim process ({}) exited early ({status}) before {} became ready{}",
                orbit_exe.display(),
                socket_path.display(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(": {detail}")
                }
            )));
        }
        let sleep_until = deadline.min(tokio::time::Instant::now() + Duration::from_millis(20));
        tokio::time::sleep_until(sleep_until).await;
    }

    child_guard.kill_and_reap().await;
    let _ = std::fs::remove_file(&err_sidecar);

    if let Some(err) = last_connect_err {
        return Err(ShimError::Pty(format!(
            "shim socket {} existed (exe={}) but connect failed: {err}",
            socket_path.display(),
            orbit_exe.display()
        )));
    }

    Err(ShimError::ReadyTimeout(socket_path.to_path_buf()))
}

/// Entrypoint invoked by `orbit shim --socket <path>` (with `ShimSpawnSpec` JSON piped over stdin).
pub async fn run_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    run_shim_with_bridge(socket_path, spec, None, None).await
}

pub async fn run_shim_with_bridge(
    socket_path: &Path,
    mut spec: ShimSpawnSpec,
    bridge_socket: Option<&Path>,
    bridge_exe: Option<&Path>,
) -> Result<(), ShimError> {
    struct EarlyCleanupGuard(Vec<PathBuf>);
    impl Drop for EarlyCleanupGuard {
        fn drop(&mut self) {
            if !self.0.is_empty() {
                unlink_cleanup_paths(&self.0);
            }
        }
    }
    let mut early_cleanup = EarlyCleanupGuard(spec.cleanup_paths.clone());
    validate_spawn_spec(&spec)?;
    let plan = sandbox::SandboxCommand::prepare_with_bridge(
        spec.program.as_ref(),
        spec.args.iter().map(Into::into).collect(),
        &spec.cwd,
        spec.sandbox.as_ref(),
        spec.egress_proxy_url.as_deref(),
        bridge_socket,
        bridge_exe,
    )?;
    spec.program = plan
        .program
        .into_string()
        .map_err(|_| ShimError::InvalidSpec("non-UTF8 program".into()))?;
    spec.args = plan
        .args
        .into_iter()
        .map(|v| {
            v.into_string()
                .map_err(|_| ShimError::InvalidSpec("non-UTF8 argument".into()))
        })
        .collect::<Result<_, _>>()?;
    #[cfg(windows)]
    if spec
        .sandbox
        .as_ref()
        .is_some_and(|p| p.mode != orbit_protocol::SandboxMode::Off)
    {
        spec.env.retain(|k, _| !sandbox::sensitive_env(k));
    }
    spec.env.extend(plan.env);
    if let Some(path) = plan.profile {
        spec.cleanup_paths.push(path.keep().map_err(|e| e.error)?);
    }
    // Ownership of `cleanup_paths` now passes to run_stdio_shim / run_pty_shim.
    early_cleanup.0.clear();
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
            let probe_timeout = if cfg!(unix) {
                Duration::from_millis(150)
            } else {
                // Windows loopback TCP SYN retries take ~1-2s before returning WSAECONNREFUSED.
                Duration::from_millis(3000)
            };
            if let Ok(Err(err)) =
                tokio::time::timeout(probe_timeout, connect_shim_stream(&path)).await
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

/// Retry `tokio::process::Command::spawn` on transient `ExecutableFileBusy` (`ETXTBSY`) errors
/// caused on Linux when another thread in a multi-threaded process forks while a freshly written
/// executable or script file descriptor is still closing.
async fn spawn_command_retrying_busy(cmd: &mut Command) -> std::io::Result<tokio::process::Child> {
    const MAX_ATTEMPTS: usize = 10;
    let mut attempt = 0usize;
    loop {
        match cmd.spawn() {
            Ok(child) => return Ok(child),
            Err(err)
                if err.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && attempt + 1 < MAX_ATTEMPTS =>
            {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20 * attempt as u64)).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn run_stdio_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    let mut pre_spawn_cleanup = PreSpawnCleanupGuard::new(&spec.cleanup_paths);
    prepare_socket_path(socket_path)?;
    let listener = ShimListener::bind(socket_path).await?;
    let _socket_guard = SocketCleanupGuard::new(socket_path.to_path_buf());
    write_pid_file(socket_path, std::process::id(), None, None);

    let mut cmd = Command::new(&spec.program);
    #[cfg(windows)]
    if spec
        .sandbox
        .as_ref()
        .is_some_and(|p| p.mode != orbit_protocol::SandboxMode::Off)
    {
        for (k, _) in std::env::vars_os() {
            if sandbox::sensitive_env(&k.to_string_lossy()) {
                cmd.env_remove(k);
            }
        }
    }
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

    let mut child = spawn_command_retrying_busy(&mut cmd).await?;
    pre_spawn_cleanup.disarm();
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
                let (reaped, code) = terminate_child_process_group(&mut child).await;
                if reaped {
                    unlink_cleanup_paths(&spec.cleanup_paths);
                }
                let _ = out_tx.send(ShimFrame::Exited { code });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
            wait_res = child.wait() => {
                let code = match wait_res {
                    Ok(status) => {
                        unlink_cleanup_paths(&spec.cleanup_paths);
                        status.code()
                    }
                    Err(_) => None,
                };
                let _ = out_tx.send(ShimFrame::Exited { code });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
        }
    }

    Ok(())
}

fn is_zsh_program(program: &str) -> bool {
    let trimmed = program.trim();
    if trimmed.is_empty() {
        return false;
    }
    let file_name = Path::new(trimmed)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(trimmed);
    let basename = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    basename.eq_ignore_ascii_case("zsh")
}

/// Build the environment variables applied to a spawned PTY shell.
/// Defaults (`TERM`, `COLORTERM`, `TERM_PROGRAM`, `ORBIT_SHELL_INTEGRATION`, and `PROMPT_EOL_MARK=""`
/// for `zsh` to prevent the reverse-video `%` partial-line artifact on fresh PTY launch) are injected
/// before `spec_env` so explicit caller overrides always win and user prompts stay untouched.
fn build_pty_env_pairs(program: &str, spec_env: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut env = Vec::with_capacity(spec_env.len() + 5);
    if !spec_env.contains_key("TERM") {
        env.push(("TERM".to_string(), "xterm-256color".to_string()));
    }
    if !spec_env.contains_key("COLORTERM") {
        env.push(("COLORTERM".to_string(), "truecolor".to_string()));
    }
    if !spec_env.contains_key("TERM_PROGRAM") {
        env.push(("TERM_PROGRAM".to_string(), "Orbit".to_string()));
    }
    if !spec_env.contains_key("ORBIT_SHELL_INTEGRATION") {
        env.push(("ORBIT_SHELL_INTEGRATION".to_string(), "1".to_string()));
    }
    if is_zsh_program(program) && !spec_env.contains_key("PROMPT_EOL_MARK") {
        env.push(("PROMPT_EOL_MARK".to_string(), String::new()));
    }
    for (k, v) in spec_env {
        env.push((k.clone(), v.clone()));
    }
    env
}

async fn run_pty_shim(
    socket_path: &Path,
    spec: ShimSpawnSpec,
    cols: u16,
    rows: u16,
) -> Result<(), ShimError> {
    let mut pre_spawn_cleanup = PreSpawnCleanupGuard::new(&spec.cleanup_paths);
    prepare_socket_path(socket_path)?;
    let listener = ShimListener::bind(socket_path).await?;
    let _socket_guard = SocketCleanupGuard::new(socket_path.to_path_buf());
    write_pid_file(socket_path, std::process::id(), None, None);

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
    #[cfg(windows)]
    if spec
        .sandbox
        .as_ref()
        .is_some_and(|p| p.mode != orbit_protocol::SandboxMode::Off)
    {
        for (k, _) in std::env::vars_os() {
            if sandbox::sensitive_env(&k.to_string_lossy()) {
                cmd.env_remove(k);
            }
        }
    }
    for (k, v) in build_pty_env_pairs(&spec.program, &spec.env) {
        cmd.env(k, v);
    }

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| ShimError::Pty(e.to_string()))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| ShimError::Pty(e.to_string()))?;

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| ShimError::Pty(e.to_string()))?;
    pre_spawn_cleanup.disarm();
    let pty_child_pid = child.process_id();
    write_pid_file(
        socket_path,
        std::process::id(),
        pty_child_pid,
        pty_child_pid,
    );
    let mut child_killer = child.clone_killer();
    drop(pair.slave);
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
                if matches!(
                    tokio::time::timeout(Duration::from_secs(1), exit_rx.recv()).await,
                    Ok(Some(_))
                ) {
                    unlink_cleanup_paths(&spec.cleanup_paths);
                }
                let _ = out_tx.send(ShimFrame::Exited { code: None });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
            maybe_code = exit_rx.recv() => {
                if maybe_code.is_some() {
                    unlink_cleanup_paths(&spec.cleanup_paths);
                }
                let _ = out_tx.send(ShimFrame::Exited { code: maybe_code.flatten() });
                tokio::time::sleep(Duration::from_millis(20)).await;
                break;
            }
        }
    }

    Ok(())
}

fn prepare_socket_path(socket_path: &Path) -> Result<(), std::io::Error> {
    #[cfg(unix)]
    validate_unix_socket_path_len(socket_path)?;
    if let Some(parent) = socket_path.parent() {
        ensure_shim_parent_dir(parent)?;
    }
    #[cfg(unix)]
    reject_if_symlink(socket_path)?;
    if socket_path.exists() {
        let _ = std::fs::remove_file(socket_path);
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
            .args(["-0", "--", &pid.to_string()])
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

    // Serialize fork-spawning / socket-cleanup tests cleanly across Unix platforms:
    // on macOS `UnixListener::bind` lacks atomic `SOCK_CLOEXEC`, and on Linux a concurrent
    // sibling test's `fork()` holds a transient reference to a newly dropped listener FD until
    // `execve()` closes `SOCK_CLOEXEC` descriptors.
    pub(crate) static SHIM_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[cfg(unix)]
    #[tokio::test]
    async fn stdio_shim_survives_daemon_disconnect_and_replays_ring() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let sock_path =
            std::env::temp_dir().join(format!("orbit_test_shim_{}.sock", std::process::id()));
        let spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_cat".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
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
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_cat_cursor".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
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
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_watchdog".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env,
            cleanup_paths: Vec::new(),
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
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_pid_sidecar".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
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
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_lag".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
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
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "test_pgid".into(),
            mode: ShimMode::Stdio,
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 30 & echo $!; wait".into()],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
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

    #[tokio::test]
    async fn pty_shim_echoes_output_over_shim_ipc() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let tmp = std::env::temp_dir().join(format!("orbit_pty_shim_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).expect("mkdir tmp");
        let sock_path = tmp.join("pty_echo.sock");

        let (program, args) = if cfg!(windows) {
            (
                "cmd.exe".to_string(),
                vec![
                    "/C".to_string(),
                    "echo ORBIT_SHIM_PTY_OK & ping -n 4 127.0.0.1 >NUL".to_string(),
                ],
            )
        } else {
            (
                "/bin/sh".to_string(),
                vec![
                    "-c".to_string(),
                    "echo ORBIT_SHIM_PTY_OK; sleep 3".to_string(),
                ],
            )
        };
        let spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "pty_echo_test".into(),
            mode: ShimMode::Pty { cols: 80, rows: 24 },
            program,
            args,
            cwd: tmp.clone(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
        };

        let sock_bg = sock_path.clone();
        let shim_task = tokio::spawn(async move { run_shim(&sock_bg, spec).await });

        let mut client = None;
        for _ in 0..250 {
            if sock_path.exists()
                && let Ok(c) = ShimClient::connect(&sock_path).await
            {
                client = Some(c);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut client = client.expect("connect to in-process pty shim");

        let mut collected = String::new();
        let mut answered_dsr = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(250), client.rx.recv()).await {
                Ok(Some(ShimFrame::PtyOutput { data_b64 }))
                | Ok(Some(ShimFrame::PtySnapshot { data_b64 })) => {
                    if let Ok(bytes) = B64.decode(data_b64) {
                        let chunk = String::from_utf8_lossy(&bytes);
                        if !answered_dsr && chunk.contains("\x1b[6n") {
                            // Windows ConPTY waits for a CPR response to its initial DSR query.
                            let _ = client.write_pty_input(b"\x1b[1;1R").await;
                            answered_dsr = true;
                        }
                        collected.push_str(&chunk);
                        if collected.contains("ORBIT_SHIM_PTY_OK") {
                            break;
                        }
                    }
                }
                Ok(Some(ShimFrame::Exited { .. })) | Ok(None) => break,
                _ => {}
            }
        }
        let _ = client.kill().await;
        let _ = tokio::time::timeout(Duration::from_secs(3), shim_task).await;
        let _ = std::fs::remove_dir_all(&tmp);

        assert!(
            collected.contains("ORBIT_SHIM_PTY_OK"),
            "expected PTY shim output, got: {:?}",
            collected
        );
    }

    #[test]
    fn stdin_spec_roundtrip_and_bounds_checks() {
        let mut env = HashMap::new();
        env.insert("ANTHROPIC_API_KEY".into(), "sk-ant-secret-12345".into());
        env.insert("ORBIT_MCP_TOKEN".into(), "tok-super-secret-xyz".into());
        let spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "claude_ag_1".into(),
            mode: ShimMode::Stdio,
            program: "/usr/bin/claude".into(),
            args: vec!["--output-format".into(), "stream-json".into()],
            cwd: PathBuf::from("/tmp/repo"),
            env,
            cleanup_paths: Vec::new(),
        };

        let raw = serde_json::to_vec(&spec).expect("serialize spec");
        let parsed = read_spawn_spec_from_reader(raw.as_slice()).expect("parse valid spec");
        assert_eq!(parsed, spec);

        // Empty, whitespace-only, and malformed stdin must fail cleanly via serde_json.
        assert!(matches!(
            read_spawn_spec_from_reader(&[][..]),
            Err(ShimError::Json(_))
        ));
        assert!(matches!(
            read_spawn_spec_from_reader(&b"   \n\t  "[..]),
            Err(ShimError::Json(_))
        ));
        assert!(matches!(
            read_spawn_spec_from_reader(&b"{not valid json"[..]),
            Err(ShimError::Json(_))
        ));

        // Oversized payload (> MAX_SHIM_SPEC_BYTES) must be rejected without reading unbounded memory.
        let oversized = vec![b' '; MAX_SHIM_SPEC_BYTES + 16];
        assert!(matches!(
            read_spawn_spec_from_reader(oversized.as_slice()),
            Err(ShimError::InvalidSpec(_))
        ));
    }

    #[tokio::test]
    async fn spawn_detached_shim_passes_spec_via_stdin_without_argv_secrets() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let sock_path =
            std::env::temp_dir().join(format!("orbit_no_argv_secret_{}.sock", std::process::id()));
        let cmd = build_detached_shim_command(Path::new("/usr/local/bin/orbit"), &sock_path);
        let argv: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            argv,
            vec![
                "shim".to_string(),
                "--socket".to_string(),
                sock_path.to_string_lossy().into_owned()
            ]
        );
        assert!(!argv.iter().any(|a| a.contains("spec-json")));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let base = std::env::temp_dir().join(format!("os_in_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            let shims_dir = base.join("shims");
            std::fs::create_dir_all(&shims_dir).expect("mkdir shims");
            let test_sock = shims_dir.join("s.sock");
            let argv_dump = base.join("argv.txt");
            let stdin_dump = base.join("stdin.json");
            let fake_orbit = base.join("fake-orbit.sh");
            let script = format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\ncat > \"{}\"\nexit 7\n",
                argv_dump.display(),
                stdin_dump.display()
            );
            std::fs::write(&fake_orbit, script).expect("write fake orbit");
            std::fs::set_permissions(&fake_orbit, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake orbit");

            let secret = "super-secret-bearer-token-9f8e7d6c";
            let mut env = HashMap::new();
            env.insert("ORBIT_SECRET".into(), secret.into());
            let spec = ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "secret_shim".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![format!("--arg={secret}")],
                cwd: base.clone(),
                env,
                cleanup_paths: Vec::new(),
            };

            let res = spawn_detached_shim(&fake_orbit, &test_sock, &spec).await;
            assert!(res.is_err(), "fake orbit exits with status 7");

            let recorded_argv = std::fs::read_to_string(&argv_dump).expect("read argv dump");
            assert!(recorded_argv.contains("shim"));
            assert!(recorded_argv.contains("--socket"));
            assert!(recorded_argv.contains(&test_sock.to_string_lossy().to_string()));
            assert!(
                !recorded_argv.contains("--spec-json"),
                "argv must never include --spec-json: {recorded_argv}"
            );
            assert!(
                !recorded_argv.contains(secret),
                "argv must NEVER leak secrets: {recorded_argv}"
            );

            let recorded_stdin = std::fs::read(&stdin_dump).expect("read stdin dump");
            let decoded =
                read_spawn_spec_from_reader(recorded_stdin.as_slice()).expect("decode piped spec");
            assert_eq!(decoded, spec);

            let _ = std::fs::remove_dir_all(&base);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unix_shim_listener_hardens_shims_dir_0700_and_socket_0600() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = SHIM_TEST_LOCK.lock().await;
        let base = std::env::temp_dir().join(format!("os_pm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let shims_dir = base.join("shims");
        std::fs::create_dir_all(&shims_dir).expect("mkdir shims");
        std::fs::set_permissions(&shims_dir, std::fs::Permissions::from_mode(0o755))
            .expect("set loose shims perm");

        let sock_path = shims_dir.join("p.sock");
        let _listener = ShimListener::bind(&sock_path)
            .await
            .expect("bind hardened listener");

        let dir_mode = std::fs::metadata(&shims_dir)
            .expect("shims dir meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            dir_mode, 0o700,
            "shims directory must be hardened to 0700, got {:o}",
            dir_mode
        );

        let sock_mode = std::fs::metadata(&sock_path)
            .expect("socket meta")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            sock_mode, 0o600,
            "unix socket must be hardened to 0600, got {:o}",
            sock_mode
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn oversized_spec_rejected_without_killing_existing_live_shim() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let base = std::env::temp_dir().join(format!("os_ov_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let shims_dir = base.join("shims");
        let sock_path = shims_dir.join("live.sock");

        let live_spec = ShimSpawnSpec {
            sandbox: None,
            egress_proxy_url: None,
            shim_id: "live_keep".into(),
            mode: ShimMode::Stdio,
            program: "/bin/cat".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            env: HashMap::new(),
            cleanup_paths: Vec::new(),
        };
        let sock_bg = sock_path.clone();
        let shim_task = tokio::spawn(async move {
            run_shim(&sock_bg, live_spec).await.expect("run live shim");
        });
        for _ in 0..50 {
            if sock_path.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(probe_existing_shim(&sock_path).await);

        let mut huge_env = HashMap::new();
        huge_env.insert("HUGE".into(), "x".repeat(MAX_SHIM_SPEC_BYTES + 64));
        let mut nul_env_val = HashMap::new();
        nul_env_val.insert("BAD".into(), "val\0ue".into());
        let mut nul_env_key = HashMap::new();
        nul_env_key.insert("BA\0D".into(), "ok".into());
        let mut empty_env_key = HashMap::new();
        empty_env_key.insert(String::new(), "ok".into());
        let mut eq_env_key = HashMap::new();
        eq_env_key.insert("A=B".into(), "ok".into());
        let bad_specs = [
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "oversized".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: huge_env,
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "empty_program".into(),
                mode: ShimMode::Stdio,
                program: "   ".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: HashMap::new(),
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "nul_program".into(),
                mode: ShimMode::Stdio,
                program: "/bin/tr\0ue".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: HashMap::new(),
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "nul_arg".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec!["--bad\0arg".into()],
                cwd: std::env::temp_dir(),
                env: HashMap::new(),
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "nul_env_val".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: nul_env_val,
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "nul_env_key".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: nul_env_key,
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "empty_env_key".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: empty_env_key,
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "eq_env_key".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: std::env::temp_dir(),
                env: eq_env_key,
                cleanup_paths: Vec::new(),
            },
            ShimSpawnSpec {
                sandbox: None,
                egress_proxy_url: None,
                shim_id: "nul_cwd".into(),
                mode: ShimMode::Stdio,
                program: "/bin/true".into(),
                args: vec![],
                cwd: PathBuf::from("/tmp/bad\0cwd"),
                env: HashMap::new(),
                cleanup_paths: Vec::new(),
            },
        ];
        for bad_spec in &bad_specs {
            let err = spawn_detached_shim(Path::new("/bin/true"), &sock_path, bad_spec).await;
            assert!(
                matches!(err, Err(ShimError::InvalidSpec(_))),
                "invalid spec `{}` must return InvalidSpec",
                bad_spec.shim_id
            );
            assert!(
                probe_existing_shim(&sock_path).await,
                "rejecting invalid spec `{}` must NOT kill an existing healthy shim",
                bad_spec.shim_id
            );
        }

        let mut client = ShimClient::connect(&sock_path).await.expect("connect live");
        client
            .kill_and_wait(Duration::from_millis(500))
            .await
            .expect("kill live");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[test]
    fn private_dir_and_long_home_fallback_reject_symlinks_and_fit_sun_path() {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::PermissionsExt;

        let base = std::env::temp_dir().join(format!("os_fb_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("mkdir base");

        // 1. Symlinked private directory must be rejected.
        let real_target = base.join("real_target");
        std::fs::create_dir_all(&real_target).expect("mkdir target");
        let symlink_dir = base.join("shims");
        std::os::unix::fs::symlink(&real_target, &symlink_dir).expect("create symlink");
        let err = ensure_private_dir(&symlink_dir).expect_err("symlink dir must fail");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);

        // 2. Deeply nested home_dir falls back to per-UID `0700` dir and fits `MAX_UNIX_SOCKET_PATH_BYTES`.
        let deep_home = base
            .join("very_long_segment_1_aaaaaaaaaaaaaaaa")
            .join("very_long_segment_2_bbbbbbbbbbbbbbbb")
            .join("very_long_segment_3_cccccccccccccccc");
        let resolved =
            resolve_private_shim_socket_path(&deep_home, "a_0123456789abcdef.sock", "deadbeef")
                .expect("resolve fallback socket path");
        assert!(
            resolved.as_os_str().as_bytes().len() <= MAX_UNIX_SOCKET_PATH_BYTES,
            "fallback socket path ({} bytes) must fit sun_path limit ({MAX_UNIX_SOCKET_PATH_BYTES})",
            resolved.as_os_str().as_bytes().len()
        );
        let fallback_parent = resolved.parent().expect("fallback parent");
        let parent_name = fallback_parent
            .file_name()
            .and_then(|n| n.to_str())
            .expect("utf8 parent name");
        let uid = current_unix_uid().expect("uid");
        assert_eq!(parent_name, format!("os-{uid}"));
        let meta = std::fs::symlink_metadata(fallback_parent).expect("lstat fallback dir");
        assert!(!meta.file_type().is_symlink());
        assert!(meta.is_dir());
        assert_eq!(meta.permissions().mode() & 0o777, 0o700);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn spawned_child_guard_cancellation_kills_shim_and_recorded_provider_process_groups() {
        let _guard = SHIM_TEST_LOCK.lock().await;
        let base = std::env::temp_dir().join(format!("os_cg_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("mkdir base");
        let sock_path = base.join("cancel.sock");
        std::fs::write(&sock_path, b"").expect("touch sock");

        // Spawn a SIGTERM-ignoring "shim" process in its own process group so `kill_and_reap`
        // waits during its SIGTERM grace period, allowing us to cancel the future mid-await.
        let mut shim_cmd = Command::new("/bin/sh");
        shim_cmd
            .args(["-c", "trap '' TERM; sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(false);
        let shim_child = shim_cmd.spawn().expect("spawn shim");
        let shim_pid = shim_child.id().expect("shim pid");

        // Spawn a separate SIGTERM-ignoring "provider" process in its own distinct process group,
        // mirroring `run_stdio_shim`'s `cmd.process_group(0)` child, and record it in `.pid`.
        let mut provider_cmd = Command::new("/bin/sh");
        provider_cmd
            .args(["-c", "trap '' TERM; sleep 30"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(false);
        let mut provider_child = provider_cmd.spawn().expect("spawn provider");
        let provider_pid = provider_child.id().expect("provider pid");
        write_pid_file(&sock_path, shim_pid, Some(provider_pid), Some(provider_pid));

        // Give shells a moment to install their TERM traps.
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(is_pid_alive(shim_pid));
        assert!(is_pid_alive(provider_pid));

        {
            let mut guard = SpawnedChildGuard::new(shim_child, sock_path.clone());
            // Cancel `kill_and_reap` mid-wait; `guard` still owns `self.child` so `Drop` MUST
            // synchronously SIGKILL both shim and provider process groups!
            let timed_out = tokio::time::timeout(Duration::from_millis(30), guard.kill_and_reap())
                .await
                .is_err();
            assert!(
                timed_out,
                "kill_and_reap should still be awaiting SIGTERM grace window before cancellation"
            );
            assert!(
                guard.child.is_some(),
                "SpawnedChildGuard must retain child ownership if kill_and_reap is cancelled mid-await"
            );
        } // Drop runs here

        // Reap our own test-spawned provider_child handle so it doesn't stay a zombie in `kill -0`.
        let _ = tokio::time::timeout(Duration::from_secs(2), provider_child.wait()).await;

        let mut both_dead = false;
        for _ in 0..40 {
            // Note: shim_child is a direct child of this test process, so after SIGKILL it may
            // be a zombie until reaped or `is_pid_alive` may report it only while alive; check
            // provider_pid (already waited) and verify sidecars were cleaned up.
            if !is_pid_alive(provider_pid) {
                both_dead = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            both_dead,
            "provider process group {provider_pid} must be killed when SpawnedChildGuard drops after cancellation"
        );
        assert!(
            !sock_path.exists() && !pid_file_path(&sock_path).exists(),
            "SpawnedChildGuard drop must remove socket and pid sidecar owned by cancelled shim"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn pty_env_injects_empty_prompt_eol_mark_for_zsh_only_and_respects_overrides() {
        let to_map = |pairs: Vec<(String, String)>| -> HashMap<String, String> {
            pairs.into_iter().collect()
        };

        // 1. `/bin/zsh`, `zsh`, and case-insensitive `ZSH` receive `PROMPT_EOL_MARK=""` and `TERM_PROGRAM="Orbit"`
        for prog in ["/bin/zsh", "zsh", "/opt/homebrew/bin/ZSH"] {
            assert!(is_zsh_program(prog), "{prog} must be recognized as zsh");
            let env = to_map(build_pty_env_pairs(prog, &HashMap::new()));
            assert_eq!(env.get("PROMPT_EOL_MARK").map(String::as_str), Some(""));
            assert_eq!(env.get("TERM_PROGRAM").map(String::as_str), Some("Orbit"));
            assert_eq!(env.get("TERM").map(String::as_str), Some("xterm-256color"));
            assert_eq!(env.get("COLORTERM").map(String::as_str), Some("truecolor"));
            assert_eq!(
                env.get("ORBIT_SHELL_INTEGRATION").map(String::as_str),
                Some("1")
            );
        }

        // 2. Non-zsh shells must NOT receive `PROMPT_EOL_MARK`
        for prog in [
            "/bin/bash",
            "bash",
            "/bin/sh",
            "/usr/bin/fish",
            "/bin/zsh-5.9",
        ] {
            assert!(!is_zsh_program(prog), "{prog} must not match zsh");
            let env = to_map(build_pty_env_pairs(prog, &HashMap::new()));
            assert!(
                !env.contains_key("PROMPT_EOL_MARK"),
                "PROMPT_EOL_MARK must not be injected for {prog}"
            );
            assert_eq!(env.get("TERM_PROGRAM").map(String::as_str), Some("Orbit"));
        }

        // 3. Explicit caller overrides in `spec.env` always win
        let mut custom = HashMap::new();
        custom.insert("PROMPT_EOL_MARK".to_string(), "%B%S#%s%b".to_string());
        custom.insert("TERM_PROGRAM".to_string(), "CustomTerm".to_string());
        custom.insert("TERM".to_string(), "screen-256color".to_string());
        let env = to_map(build_pty_env_pairs("/bin/zsh", &custom));
        assert_eq!(
            env.get("PROMPT_EOL_MARK").map(String::as_str),
            Some("%B%S#%s%b")
        );
        assert_eq!(
            env.get("TERM_PROGRAM").map(String::as_str),
            Some("CustomTerm")
        );
        assert_eq!(env.get("TERM").map(String::as_str), Some("screen-256color"));
    }

    #[cfg(unix)]
    #[test]
    fn unix_kill_args_always_include_option_terminator_and_reject_pid_zero_or_one() {
        assert_eq!(build_kill_pid_args(0, "-TERM"), None);
        assert_eq!(build_kill_pid_args(1, "-TERM"), None);
        assert_eq!(build_kill_pgid_args(0, "-TERM"), None);
        assert_eq!(build_kill_pgid_args(1, "-TERM"), None);
        assert_eq!(build_kill_pid_args(1042769, "  "), None);
        assert_eq!(build_kill_pgid_args(1042769, "---"), None);

        assert_eq!(
            build_kill_pid_args(1042769, "-TERM"),
            Some([
                "-s".to_string(),
                "TERM".to_string(),
                "--".to_string(),
                "1042769".to_string(),
            ])
        );
        assert_eq!(
            build_kill_pgid_args(1042769, "-TERM"),
            Some([
                "-s".to_string(),
                "TERM".to_string(),
                "--".to_string(),
                "-1042769".to_string(),
            ])
        );
        assert_eq!(
            build_kill_pgid_args(1042769, "KILL"),
            Some([
                "-s".to_string(),
                "KILL".to_string(),
                "--".to_string(),
                "-1042769".to_string(),
            ])
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn signal_pid_group_kills_only_target_group_and_leaves_unrelated_sibling_alive() {
        let _guard = SHIM_TEST_LOCK.lock().await;

        let mut target = tokio::process::Command::new("sh")
            .args(["-c", "sleep 30 & echo $!; wait"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn target pgid leader");
        let target_pid = target.id().expect("target pid");
        assert!(target_pid > 1);

        let stdout = target.stdout.take().expect("target stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
            .await
            .expect("read grandchild pid timed out")
            .expect("read grandchild pid io error");
        let grandchild_pid: u32 = line.trim().parse().expect("parse grandchild pid");
        assert!(grandchild_pid > 1);

        let mut canary = tokio::process::Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn sibling canary");

        // Regression test: without `--` before `-<pgid>`, procps-ng /bin/kill misparsed `-1...`
        // as `kill(-1, SIGTERM)` and wiped out unrelated processes in the user session.
        signal_pid_group(target_pid, "-TERM");

        let target_wait = tokio::time::timeout(Duration::from_secs(3), target.wait()).await;
        if target_wait.is_err() {
            signal_pid_group(target_pid, "-KILL");
            let _ = tokio::time::timeout(Duration::from_secs(3), target.wait()).await;
        }

        let mut grandchild_dead = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while tokio::time::Instant::now() < deadline {
            if !is_pid_alive(grandchild_pid) {
                grandchild_dead = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let canary_status = canary.try_wait();

        // Always clean up before asserting so zero orphan sleep survives on failure.
        let _ = canary.kill().await;
        let _ = canary.wait().await;
        if !grandchild_dead {
            signal_pid_group(grandchild_pid, "-KILL");
        }

        assert!(
            grandchild_dead,
            "target process group's grandchild {grandchild_pid} should be dead"
        );
        assert!(
            matches!(canary_status, Ok(None)),
            "unrelated sibling canary process should still be alive, got {canary_status:?}"
        );
    }
}
