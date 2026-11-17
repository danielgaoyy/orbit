//! Detached process & PTY supervisor (`orbit shim`).
//!
//! Keeps `agy`, `codex`, `claude`, and interactive PTY shells alive across `orbit daemon`
//! restarts or binary hot-upgrades via Unix Domain Sockets (`$ORBIT_HOME/shims/<id>.sock`).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    WriteStdin { line: String },
    // Daemon -> Shim (PTY)
    PtyInput { data_b64: String },
    PtyResize { cols: u16, rows: u16 },
    // Daemon -> Shim (Lifecycle)
    Kill,

    // Shim -> Daemon (Stdio)
    StdoutLine { line: String },
    StderrLine { line: String },
    // Shim -> Daemon (PTY)
    PtySnapshot { data_b64: String },
    PtyOutput { data_b64: String },
    // Shim -> Daemon (Lifecycle)
    Exited { code: Option<i32> },
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
            let mut lines = BufReader::new(read_half).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Ok(frame) = serde_json::from_str::<ShimFrame>(&line)
                    && out_tx.send(frame).await.is_err()
                {
                    break;
                }
            }
        });

        Ok(Self {
            tx: in_tx,
            rx: out_rx,
        })
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

async fn run_stdio_shim(socket_path: &Path, spec: ShimSpawnSpec) -> Result<(), ShimError> {
    prepare_socket_path(socket_path)?;
    let listener = UnixListener::bind(socket_path)?;

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
    let (out_tx, _) = broadcast::channel::<ShimFrame>(1024);
    let (in_tx, mut in_rx) = mpsc::channel::<ShimFrame>(256);

    let out_tx_stdout = out_tx.clone();
    let ring_stdout = ring.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(child_stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame = ShimFrame::StdoutLine { line };
            push_ring(&ring_stdout, frame.clone());
            let _ = out_tx_stdout.send(frame);
        }
    });

    let out_tx_stderr = out_tx.clone();
    let ring_stderr = ring.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(child_stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame = ShimFrame::StderrLine { line };
            push_ring(&ring_stderr, frame.clone());
            let _ = out_tx_stderr.send(frame);
        }
    });

    let (kill_tx, mut kill_rx) = mpsc::channel::<()>(1);
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
                    let initial_frames: Vec<ShimFrame> = ring
                        .lock()
                        .map(|g| g.iter().cloned().collect())
                        .unwrap_or_default();
                    handle_daemon_attach(stream, initial_frames, out_tx.subscribe(), in_tx.clone());
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
                    handle_daemon_attach(stream, initial, out_tx.subscribe(), in_tx.clone());
                }
            }
            _ = kill_rx.recv() => {
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

fn handle_daemon_attach(
    stream: UnixStream,
    initial_frames: Vec<ShimFrame>,
    mut out_rx: broadcast::Receiver<ShimFrame>,
    in_tx: mpsc::Sender<ShimFrame>,
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
        while let Ok(frame) = out_rx.recv().await {
            if let Ok(mut encoded) = serde_json::to_string(&frame) {
                encoded.push('\n');
                if write_half.write_all(encoded.as_bytes()).await.is_err() {
                    break;
                }
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
                ShimFrame::StdoutLine { line } => assert_eq!(line, "hello-across-restart"),
                other => panic!("unexpected frame: {other:?}"),
            }
        } // client1 dropped here!

        // Second Daemon connection attaches to the SAME running shim and gets replay + live I/O
        let mut client2 = ShimClient::connect(&sock_path).await.expect("connect 2");
        let replayed = tokio::time::timeout(Duration::from_secs(2), client2.rx.recv())
            .await
            .expect("timeout")
            .expect("replay");
        match replayed {
            ShimFrame::StdoutLine { line } => assert_eq!(line, "hello-across-restart"),
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
            ShimFrame::StdoutLine { line } => assert_eq!(line, "second-turn"),
            other => panic!("unexpected live: {other:?}"),
        }

        client2.kill().await.expect("kill");
        let _ = tokio::time::timeout(Duration::from_secs(2), shim_task).await;
    }
}
