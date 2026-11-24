//! Shared Agent Client Protocol (ACP) client-side callback host (`fs/*` & `terminal/*`).
//!
//! Shared across all ACP drivers (`acp_generic.rs`, `acp_providers.rs`, `acp_registry.rs`)
//! so every ACP agent gets identical hardened filesystem and terminal execution semantics
//! with zero duplicated callback code.
//!
//! Security boundaries:
//! - Workspace confinement: `fs/read_text_file`, `fs/write_text_file`, and `terminal/create` `cwd`
//!   are strictly confined inside the canonical workspace root; symlink escapes and `..` traversals
//!   are rejected using ARM64-safe `O_NOFOLLOW` + `(dev, ino)` identity checks.
//! - `.git/` writes are unconditionally rejected; all writes use atomic `create_new(true)` +
//!   `O_NOFOLLOW` temp file + `rename` inside a verified non-symlink parent directory.
//! - Bounded payload sizes (`4 MiB` max file read/write, `1 MiB` terminal output ring buffer)
//!   and bounded concurrency (`8` max active terminals per session).
//! - Subprocess isolation: every terminal runs in its own isolated process group (`process_group(0)`
//!   on Unix, `CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW` on Windows), strips all `ORBIT_*`
//!   secrets and dangerous dynamic-linker/shell-hook variables, enforces `pgid > 1` with explicit
//!   `--` option terminator before `-<pgid>` on Unix `/bin/kill`, and automatically cleans up
//!   every active process group on `Drop` so zero orphan processes can leak.

use std::collections::{HashMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tracing::debug;
use uuid::Uuid;

/// Maximum UTF-8 byte length permitted for `fs/read_text_file` and `fs/write_text_file` (4 MiB).
pub const MAX_ACP_FS_BYTES: usize = 4 * 1024 * 1024;

/// Maximum concurrent active terminals allowed per ACP agent session.
pub const MAX_ACP_TERMINALS: usize = 8;

/// Maximum concurrent deferred `terminal/wait_for_exit` requests per ACP agent session.
pub const MAX_ACP_TERMINAL_WAITERS: usize = 32;

/// Bounded timeout for draining stdout/stderr pipes after the terminal leader process exits
/// so background descendants holding inherited pipe FDs never hang `terminal/wait_for_exit`.
const ACP_TERMINAL_PIPE_DRAIN_TIMEOUT: Duration = Duration::from_millis(250);

/// Default and maximum ring-buffer byte capacity per ACP terminal (1 MiB).
pub const MAX_ACP_TERMINAL_OUTPUT_BYTES: usize = 1024 * 1024;

/// JSON-RPC 2.0 error codes used by ACP client callbacks.
pub const JSONRPC_INVALID_PARAMS: i64 = -32602;
pub const JSONRPC_METHOD_NOT_FOUND: i64 = -32601;
pub const JSONRPC_INTERNAL_ERROR: i64 = -32603;
pub const JSONRPC_PERMISSION_DENIED: i64 = -32001;
pub const JSONRPC_RESOURCE_NOT_FOUND: i64 = -32002;
pub const JSONRPC_LIMIT_EXCEEDED: i64 = -32003;

/// Canonical `clientCapabilities` advertised by Orbit during ACP `initialize`.
pub fn acp_client_capabilities() -> Value {
    json!({
        "fs": {
            "readTextFile": true,
            "writeTextFile": true
        },
        "terminal": true
    })
}

/// Outcome returned when dispatching an incoming ACP server->client JSON-RPC request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpCallbackOutcome {
    /// Immediate JSON-RPC response line ready to write back to the ACP server's stdin.
    Reply(String),
    /// Asynchronous operation spawned (e.g. `terminal/wait_for_exit`); its JSON-RPC response
    /// line will be pushed to the host's deferred reply channel when ready.
    Deferred,
    /// Method is not a recognized `fs/*` or `terminal/*` client callback.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalExitStatus {
    pub exit_code: Option<i32>,
    pub signal: Option<String>,
}

impl TerminalExitStatus {
    fn to_json(&self) -> Value {
        json!({
            "exitCode": self.exit_code,
            "signal": self.signal,
        })
    }
}

struct TerminalOutputRing {
    bytes: VecDeque<u8>,
    max_bytes: usize,
    truncated: bool,
}

impl TerminalOutputRing {
    fn new(max_bytes: usize) -> Self {
        let cap = max_bytes.clamp(1, MAX_ACP_TERMINAL_OUTPUT_BYTES);
        Self {
            bytes: VecDeque::with_capacity(cap.min(64 * 1024)),
            max_bytes: cap,
            truncated: false,
        }
    }

    fn push_slice(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if chunk.len() >= self.max_bytes {
            self.bytes.clear();
            let start = chunk.len() - self.max_bytes;
            self.bytes.extend(&chunk[start..]);
            self.truncated = true;
            return;
        }
        let next_len = self.bytes.len() + chunk.len();
        if next_len > self.max_bytes {
            let drop_count = next_len - self.max_bytes;
            self.bytes.drain(..drop_count);
            self.truncated = true;
        }
        self.bytes.extend(chunk);
    }

    fn snapshot(&self) -> (String, bool) {
        let raw: Vec<u8> = self.bytes.iter().copied().collect();
        (String::from_utf8_lossy(&raw).into_owned(), self.truncated)
    }
}

struct AbortOnDrop(Option<tokio::task::AbortHandle>);

impl AbortOnDrop {
    fn new(handle: tokio::task::AbortHandle) -> Self {
        Self(Some(handle))
    }

    fn abort_now(&mut self) {
        if let Some(h) = self.0.take() {
            h.abort();
        }
    }
}

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.abort_now();
    }
}

struct ManagedAcpTerminal {
    /// Live, unreaped process group ID (`0` once retired or reaped so `kill`/`Drop` can
    /// NEVER signal a recycled OS PID).
    live_pgid: Arc<AtomicU32>,
    output: Arc<Mutex<TerminalOutputRing>>,
    exit_rx: watch::Receiver<Option<TerminalExitStatus>>,
    /// Deferred JSON-RPC request IDs waiting for this terminal's single exit notification.
    waiters: Arc<Mutex<Vec<Value>>>,
    task_abort: tokio::task::AbortHandle,
}

impl ManagedAcpTerminal {
    fn kill_sync(&self) {
        // Retire and terminate the process group BEFORE aborting/reaping so descendants are
        // cleaned up while the PGID is guaranteed unrecycled.
        let pgid = self.live_pgid.swap(0, Ordering::AcqRel);
        if pgid > 1 {
            terminate_process_group_sync(pgid, "KILL");
        }
        self.task_abort.abort();
    }

    fn pending_waiter_count(&self) -> usize {
        self.waiters
            .lock()
            .map(|w| w.len())
            .unwrap_or_else(|poisoned| poisoned.into_inner().len())
    }
}

/// Per-session ACP client callback host managing `fs/*` and `terminal/*` requests.
///
/// Dropping `AcpCallbackHost` immediately terminates and cleans up every remaining terminal
/// process group spawned by the session so zero orphan processes can leak.
pub struct AcpCallbackHost {
    pub(crate) policy: Option<orbit_protocol::SandboxPolicy>,
    pub(crate) proxy: Option<String>,
    workspace_root: PathBuf,
    terminals: HashMap<String, Arc<ManagedAcpTerminal>>,
    reply_tx: mpsc::Sender<String>,
}

impl Drop for AcpCallbackHost {
    fn drop(&mut self) {
        for (_, term) in self.terminals.drain() {
            term.kill_sync();
        }
    }
}

impl AcpCallbackHost {
    /// Create a new callback host confined to `workspace_root`, returning both the host and
    /// a receiver carrying asynchronous JSON-RPC reply lines (for `terminal/wait_for_exit`).
    pub fn new(workspace_root: PathBuf) -> (Self, mpsc::Receiver<String>) {
        let (reply_tx, reply_rx) = mpsc::channel(128);
        (
            Self {
                policy: None,
                proxy: None,
                workspace_root,
                terminals: HashMap::new(),
                reply_tx,
            },
            reply_rx,
        )
    }

    /// Number of currently tracked (unreleased) terminals in this session.
    pub fn active_terminal_count(&self) -> usize {
        self.terminals.len()
    }

    /// Total queued `terminal/wait_for_exit` requests across all active terminals.
    pub fn pending_waiter_count(&self) -> usize {
        self.terminals
            .values()
            .map(|t| t.pending_waiter_count())
            .sum()
    }

    /// Dispatch a server->client JSON-RPC request if it belongs to `fs/*` or `terminal/*`.
    pub async fn handle_request(
        &mut self,
        id_val: &Value,
        method: &str,
        params: &Value,
    ) -> AcpCallbackOutcome {
        match method {
            "fs/read_text_file" => {
                let root = self.workspace_root.clone();
                let params_owned = params.clone();
                let policy = self.policy.clone();
                let res = tokio::task::spawn_blocking(move || {
                    read_with_policy(&root, &params_owned, policy.as_ref())
                })
                .await
                .unwrap_or_else(|e| {
                    Err((
                        JSONRPC_INTERNAL_ERROR,
                        format!("fs/read_text_file worker failed: {e}"),
                    ))
                });
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            "fs/write_text_file" => {
                let root = self.workspace_root.clone();
                let params_owned = params.clone();
                let policy = self.policy.clone();
                let res = tokio::task::spawn_blocking(move || {
                    write_with_policy(&root, &params_owned, policy.as_ref())
                })
                .await
                .unwrap_or_else(|e| {
                    Err((
                        JSONRPC_INTERNAL_ERROR,
                        format!("fs/write_text_file worker failed: {e}"),
                    ))
                });
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            "terminal/create" => {
                let res = self.handle_terminal_create(params).await;
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            "terminal/output" => {
                let res = self.handle_terminal_output(params);
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            "terminal/wait_for_exit" => self.handle_terminal_wait_for_exit(id_val, params),
            "terminal/kill" => {
                let res = self.handle_terminal_kill(params).await;
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            "terminal/release" => {
                let res = self.handle_terminal_release(params).await;
                AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, res))
            }
            _ => AcpCallbackOutcome::Unsupported,
        }
    }

    async fn handle_terminal_create(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        if self.terminals.len() >= MAX_ACP_TERMINALS {
            return Err((
                JSONRPC_LIMIT_EXCEEDED,
                format!(
                    "Maximum active ACP terminals ({MAX_ACP_TERMINALS}) reached; release an existing terminal first"
                ),
            ));
        }

        let command = params
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                (
                    JSONRPC_INVALID_PARAMS,
                    "terminal/create requires a non-empty string `command`".to_string(),
                )
            })?;
        if command.chars().any(|c| c == '\0' || c == '\r' || c == '\n') {
            return Err((
                JSONRPC_INVALID_PARAMS,
                "terminal/create `command` contains invalid control characters".to_string(),
            ));
        }

        let explicit_args: Option<Vec<String>> = match params.get("args") {
            None | Some(Value::Null) => None,
            Some(Value::Array(arr)) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    let s = item.as_str().ok_or_else(|| {
                        (
                            JSONRPC_INVALID_PARAMS,
                            "terminal/create `args` entries must be strings".to_string(),
                        )
                    })?;
                    if s.contains('\0') {
                        return Err((
                            JSONRPC_INVALID_PARAMS,
                            "terminal/create `args` must not contain NUL bytes".to_string(),
                        ));
                    }
                    out.push(s.to_string());
                }
                Some(out)
            }
            Some(_) => {
                return Err((
                    JSONRPC_INVALID_PARAMS,
                    "terminal/create `args` must be an array of strings".to_string(),
                ));
            }
        };

        let raw_cwd = params
            .get("cwd")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let root_for_cwd = self.workspace_root.clone();
        let resolved_cwd = tokio::task::spawn_blocking(move || {
            resolve_confined_dir(&root_for_cwd, raw_cwd.as_deref())
        })
        .await
        .map_err(|e| {
            (
                JSONRPC_INTERNAL_ERROR,
                format!("failed to resolve terminal cwd: {e}"),
            )
        })??;

        let output_limit = params
            .get("outputByteLimit")
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .map(|v| v.clamp(1, MAX_ACP_TERMINAL_OUTPUT_BYTES))
            .unwrap_or(MAX_ACP_TERMINAL_OUTPUT_BYTES);

        let user_env = parse_terminal_env(params.get("env"))?;

        let original = build_terminal_command(command, explicit_args.as_deref());
        let plan = orbit_shim::sandbox::SandboxCommand::prepare(
            original.as_std().get_program(),
            original.as_std().get_args().map(Into::into).collect(),
            &resolved_cwd,
            self.policy.as_ref(),
            self.proxy.as_deref(),
        )
        .map_err(|e| (JSONRPC_INTERNAL_ERROR, e.to_string()))?;
        let mut cmd = if self.policy.is_some() {
            plan.command()
        } else {
            original
        };
        cmd.current_dir(&resolved_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        configure_isolated_process_group(&mut cmd);

        // Strip Orbit internal secret env vars and dynamic-linker/shell-hook injection vectors
        // from inherited environment BEFORE applying caller-supplied environment overrides.
        for (k_os, _) in std::env::vars_os() {
            if let Some(k_str) = k_os.to_str()
                && is_blocked_terminal_env_key(k_str)
            {
                cmd.env_remove(&k_os);
            }
        }
        cmd.env("NO_COLOR", "1");
        for (k, v) in user_env {
            if !is_blocked_terminal_env_key(&k) {
                cmd.env(k, v);
            }
        }

        plan.apply_env(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| {
            (
                JSONRPC_INTERNAL_ERROR,
                format!("Failed to spawn terminal command `{command}`: {e}"),
            )
        })?;

        let pid = child.id();
        let live_pgid = Arc::new(AtomicU32::new(pid.filter(|&p| p > 1).unwrap_or(0)));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let output = Arc::new(Mutex::new(TerminalOutputRing::new(output_limit)));
        let (exit_tx, exit_rx) = watch::channel::<Option<TerminalExitStatus>>(None);
        let waiters: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));

        let output_bg = Arc::clone(&output);
        let live_pgid_bg = Arc::clone(&live_pgid);
        let waiters_bg = Arc::clone(&waiters);
        let reply_tx_bg = self.reply_tx.clone();
        let sandbox_profile_bg = plan.profile;
        let task = tokio::spawn(async move {
            let _sandbox_profile = sandbox_profile_bg;
            let out_ring_1 = Arc::clone(&output_bg);
            let mut stdout_reader = tokio::spawn(async move {
                if let Some(mut s) = stdout {
                    drain_pipe_into_ring(&mut s, &out_ring_1).await;
                }
            });
            let mut stdout_guard = AbortOnDrop::new(stdout_reader.abort_handle());

            let out_ring_2 = Arc::clone(&output_bg);
            let mut stderr_reader = tokio::spawn(async move {
                if let Some(mut s) = stderr {
                    drain_pipe_into_ring(&mut s, &out_ring_2).await;
                }
            });
            let mut stderr_guard = AbortOnDrop::new(stderr_reader.abort_handle());

            let status_res = child.wait().await;
            // Immediately retire the PGID as soon as the leader is reaped so concurrent
            // `terminal/kill` or `Drop` can NEVER signal a recycled OS PID.
            live_pgid_bg.store(0, Ordering::Release);

            // Allow a short bounded drain window for buffered stdout/stderr from the exited
            // leader, then abort readers so descendants holding pipe FDs never hang wait_for_exit.
            let drain_deadline = tokio::time::Instant::now() + ACP_TERMINAL_PIPE_DRAIN_TIMEOUT;
            let _ = tokio::time::timeout_at(drain_deadline, &mut stdout_reader).await;
            stdout_guard.abort_now();
            let _ = tokio::time::timeout_at(drain_deadline, &mut stderr_reader).await;
            stderr_guard.abort_now();

            let exit_status = match status_res {
                Ok(st) => {
                    #[cfg(unix)]
                    let signal = {
                        use std::os::unix::process::ExitStatusExt;
                        st.signal().map(unix_signal_name)
                    };
                    #[cfg(not(unix))]
                    let signal = None;
                    TerminalExitStatus {
                        exit_code: st.code(),
                        signal,
                    }
                }
                Err(_) => TerminalExitStatus {
                    exit_code: None,
                    signal: None,
                },
            };

            // Publish status AND drain all registered waiters atomically with respect to
            // `handle_terminal_wait_for_exit` (which checks `exit_rx` while holding `waiters`).
            let pending_ids = {
                let mut guard = waiters_bg
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let _ = exit_tx.send(Some(exit_status.clone()));
                std::mem::take(&mut *guard)
            };
            let result_json = exit_status.to_json();
            for req_id in pending_ids {
                let line = format_jsonrpc_outcome(&req_id, Ok(result_json.clone()));
                let _ = reply_tx_bg.send(line).await;
            }
        });

        let terminal_id = format!("acp_term_{}", &Uuid::new_v4().simple().to_string()[..12]);
        self.terminals.insert(
            terminal_id.clone(),
            Arc::new(ManagedAcpTerminal {
                live_pgid,
                output,
                exit_rx,
                waiters,
                task_abort: task.abort_handle(),
            }),
        );

        debug!(terminal_id = %terminal_id, pid = ?pid, "Spawned workspace-confined ACP terminal");
        Ok(json!({ "terminalId": terminal_id }))
    }

    fn handle_terminal_output(&self, params: &Value) -> Result<Value, (i64, String)> {
        let tid = extract_terminal_id(params)?;
        let term = self.terminals.get(tid).ok_or_else(|| {
            (
                JSONRPC_RESOURCE_NOT_FOUND,
                format!("ACP terminal `{tid}` not found"),
            )
        })?;
        let (text, truncated) = term
            .output
            .lock()
            .map(|ring| ring.snapshot())
            .unwrap_or_else(|poisoned| poisoned.into_inner().snapshot());
        let exit_status = term.exit_rx.borrow().clone();
        let mut out = json!({
            "output": text,
            "truncated": truncated,
        });
        if let Some(status) = exit_status {
            out["exitStatus"] = status.to_json();
        }
        Ok(out)
    }

    fn handle_terminal_wait_for_exit(
        &mut self,
        id_val: &Value,
        params: &Value,
    ) -> AcpCallbackOutcome {
        let tid = match extract_terminal_id(params) {
            Ok(t) => t,
            Err(err) => return AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, Err(err))),
        };
        let Some(term) = self.terminals.get(tid).cloned() else {
            return AcpCallbackOutcome::Reply(format_jsonrpc_outcome(
                id_val,
                Err((
                    JSONRPC_RESOURCE_NOT_FOUND,
                    format!("ACP terminal `{tid}` not found"),
                )),
            ));
        };

        if let Some(status) = term.exit_rx.borrow().clone() {
            return AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, Ok(status.to_json())));
        }

        if self.pending_waiter_count() >= MAX_ACP_TERMINAL_WAITERS {
            return AcpCallbackOutcome::Reply(format_jsonrpc_outcome(
                id_val,
                Err((
                    JSONRPC_LIMIT_EXCEEDED,
                    format!(
                        "Maximum concurrent ACP terminal/wait_for_exit requests ({MAX_ACP_TERMINAL_WAITERS}) reached"
                    ),
                )),
            ));
        }

        // Lock `waiters` and re-check `exit_rx` under the same lock so there is zero lost-wakeup
        // race and zero extra Tokio task allocation per waiter.
        let mut guard = term
            .waiters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(status) = term.exit_rx.borrow().clone() {
            return AcpCallbackOutcome::Reply(format_jsonrpc_outcome(id_val, Ok(status.to_json())));
        }
        guard.push(id_val.clone());
        AcpCallbackOutcome::Deferred
    }

    async fn handle_terminal_kill(&self, params: &Value) -> Result<Value, (i64, String)> {
        let tid = extract_terminal_id(params)?;
        let term = self.terminals.get(tid).ok_or_else(|| {
            (
                JSONRPC_RESOURCE_NOT_FOUND,
                format!("ACP terminal `{tid}` not found"),
            )
        })?;
        let pgid = term.live_pgid.load(Ordering::Acquire);
        if pgid > 1 {
            terminate_process_group_async(pgid, "TERM").await;
            let mut rx = term.exit_rx.clone();
            let exited =
                tokio::time::timeout(Duration::from_millis(250), rx.wait_for(|opt| opt.is_some()))
                    .await
                    .is_ok();
            // Re-check `live_pgid` before sending KILL so we NEVER signal a PID that was reaped
            // and potentially recycled during the grace window.
            let still_live = term.live_pgid.load(Ordering::Acquire);
            if !exited && still_live > 1 {
                terminate_process_group_async(still_live, "KILL").await;
            }
        }
        Ok(json!({}))
    }

    async fn handle_terminal_release(&mut self, params: &Value) -> Result<Value, (i64, String)> {
        let tid = extract_terminal_id(params)?;
        let Some(term) = self.terminals.remove(tid) else {
            return Err((
                JSONRPC_RESOURCE_NOT_FOUND,
                format!("ACP terminal `{tid}` not found"),
            ));
        };
        // Always terminate the live/unreaped process group BEFORE aborting/reaping so
        // background descendants that closed pipes are killed cleanly without PID reuse.
        let pgid = term.live_pgid.swap(0, Ordering::AcqRel);
        if pgid > 1 {
            terminate_process_group_async(pgid, "KILL").await;
        }
        term.task_abort.abort();

        // Unblock any pending `terminal/wait_for_exit` waiters on release.
        let pending_ids = {
            let mut guard = term
                .waiters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *guard)
        };
        if !pending_ids.is_empty() {
            let status = term.exit_rx.borrow().clone().unwrap_or(TerminalExitStatus {
                exit_code: None,
                signal: Some("SIGKILL".to_string()),
            });
            let result_json = status.to_json();
            for req_id in pending_ids {
                let line = format_jsonrpc_outcome(&req_id, Ok(result_json.clone()));
                let _ = self.reply_tx.send(line).await;
            }
        }
        Ok(json!({}))
    }
}

fn format_jsonrpc_outcome(id_val: &Value, outcome: Result<Value, (i64, String)>) -> String {
    match outcome {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "result": result,
        })
        .to_string(),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id_val,
            "error": {
                "code": code,
                "message": message,
            }
        })
        .to_string(),
    }
}

fn extract_terminal_id(params: &Value) -> Result<&str, (i64, String)> {
    params
        .get("terminalId")
        .or_else(|| params.get("terminal_id"))
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                JSONRPC_INVALID_PARAMS,
                "terminal request requires a non-empty `terminalId`".to_string(),
            )
        })
}

fn parse_terminal_env(raw: Option<&Value>) -> Result<Vec<(String, String)>, (i64, String)> {
    let mut out = Vec::new();
    let Some(val) = raw else {
        return Ok(out);
    };
    if val.is_null() {
        return Ok(out);
    }
    if let Some(arr) = val.as_array() {
        for item in arr {
            let name = item
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    (
                        JSONRPC_INVALID_PARAMS,
                        "terminal/create `env` items must have a non-empty string `name`"
                            .to_string(),
                    )
                })?;
            let value = item.get("value").and_then(Value::as_str).unwrap_or("");
            if name.contains('\0') || name.contains('=') || value.contains('\0') {
                return Err((
                    JSONRPC_INVALID_PARAMS,
                    format!("terminal/create env var `{name}` contains invalid characters"),
                ));
            }
            out.push((name.to_string(), value.to_string()));
        }
        return Ok(out);
    }
    if let Some(obj) = val.as_object() {
        for (name, v) in obj {
            let val_str = v.as_str().ok_or_else(|| {
                (
                    JSONRPC_INVALID_PARAMS,
                    format!("terminal/create env `{name}` value must be a string"),
                )
            })?;
            if name.trim().is_empty()
                || name.contains('\0')
                || name.contains('=')
                || val_str.contains('\0')
            {
                return Err((
                    JSONRPC_INVALID_PARAMS,
                    format!("terminal/create env var `{name}` contains invalid characters"),
                ));
            }
            out.push((name.clone(), val_str.to_string()));
        }
        return Ok(out);
    }
    Err((
        JSONRPC_INVALID_PARAMS,
        "terminal/create `env` must be an array of {name, value} objects or a string map"
            .to_string(),
    ))
}

/// Returns `true` for Orbit internal secret env vars and dynamic-linker / shell-hook injection keys
/// that must never be exposed to or overridden by ACP terminal child processes.
pub fn is_blocked_terminal_env_key(key: &str) -> bool {
    let upper = key.trim().to_ascii_uppercase();
    if upper.is_empty() {
        return true;
    }
    if upper.starts_with("ORBIT_")
        || upper.starts_with("DYLD_")
        || upper.starts_with("LD_")
        || upper.starts_with("BASH_FUNC_")
    {
        return true;
    }
    matches!(
        upper.as_str(),
        "BASH_ENV"
            | "ENV"
            | "ZDOTDIR"
            | "PROMPT_COMMAND"
            | "IFS"
            | "CDPATH"
            | "NODE_OPTIONS"
            | "PYTHONSTARTUP"
            | "PYTHONPATH"
            | "RUBYOPT"
            | "PERL5OPT"
            | "GIT_SSH_COMMAND"
            | "GIT_EXEC_PATH"
            | "SSLKEYLOGFILE"
    )
}

fn build_terminal_command(command: &str, explicit_args: Option<&[String]>) -> Command {
    if let Some(args) = explicit_args {
        let mut cmd = Command::new(command);
        cmd.args(args);
        return cmd;
    }
    // If `args` was omitted and `command` looks like a shell pipeline/command string (not a bare
    // executable path), run it via the platform shell so agents sending `command: "echo hi"` work.
    let looks_like_shell = command.chars().any(|c| {
        c.is_whitespace() || matches!(c, '|' | '&' | ';' | '>' | '<' | '$' | '`' | '*' | '?')
    }) && !Path::new(command).is_file();

    if looks_like_shell {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let comspec = std::env::var("COMSPEC")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "cmd.exe".to_string());
            let mut std_cmd = std::process::Command::new(comspec);
            std_cmd
                .arg("/C")
                .raw_arg(format!("\"{command}\""))
                .creation_flags(0x0800_0000);
            Command::from(std_cmd)
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("/bin/sh");
            cmd.args(["-c", command]);
            cmd
        }
    } else {
        Command::new(command)
    }
}

async fn drain_pipe_into_ring<R: tokio::io::AsyncRead + Unpin>(
    pipe: &mut R,
    ring: &Arc<Mutex<TerminalOutputRing>>,
) {
    let mut buf = [0u8; 8192];
    loop {
        match pipe.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Ok(mut guard) = ring.lock() {
                    guard.push_slice(&buf[..n]);
                }
            }
        }
    }
}

// --- Filesystem Confinement & Atomic I/O ---

#[cfg(unix)]
fn unix_o_nofollow_flag() -> Option<i32> {
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        Some(0x0100)
    }
    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "x86", target_arch = "riscv64")
    ))]
    {
        Some(0x20000)
    }
    #[cfg(all(target_os = "linux", any(target_arch = "aarch64", target_arch = "arm")))]
    {
        // Linux arm/aarch64 `O_NOFOLLOW` is octal 0100000 (0x8000); 0x10000 is `O_DIRECT`.
        Some(0o100000)
    }
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
        all(
            target_os = "linux",
            any(
                target_arch = "x86_64",
                target_arch = "x86",
                target_arch = "riscv64",
                target_arch = "aarch64",
                target_arch = "arm"
            )
        )
    )))]
    {
        None
    }
}

fn canonicalize_root(root: &Path) -> Result<PathBuf, (i64, String)> {
    let canon = fs::canonicalize(root).map_err(|e| {
        (
            JSONRPC_INTERNAL_ERROR,
            format!("Workspace root unavailable: {e}"),
        )
    })?;
    if !canon.is_dir() {
        return Err((
            JSONRPC_INTERNAL_ERROR,
            "Workspace root is not a directory".to_string(),
        ));
    }
    Ok(canon)
}

fn path_has_git_component(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c,
            Component::Normal(seg) if seg.to_string_lossy().eq_ignore_ascii_case(".git")
        )
    })
}

fn validate_raw_path(raw: &str) -> Result<&str, (i64, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err((JSONRPC_INVALID_PARAMS, "Path must not be empty".to_string()));
    }
    if trimmed.chars().any(|c| c == '\0' || c.is_control()) {
        return Err((
            JSONRPC_INVALID_PARAMS,
            "Path contains invalid control characters".to_string(),
        ));
    }
    Ok(trimmed)
}

/// Resolve a caller-provided relative or absolute path strictly inside `canonical_root`,
/// returning normalized relative path segments (never containing `..`, root, or empty segments).
fn extract_confined_relative_segments(
    canonical_root: &Path,
    raw_path: &str,
    allow_root_itself: bool,
) -> Result<Vec<String>, (i64, String)> {
    let trimmed = validate_raw_path(raw_path)?;
    let p = Path::new(trimmed);

    let mut rel_segments: Vec<String> = Vec::new();
    if p.is_absolute() {
        // Normalize lexical `..` BEFORE checking prefix against `canonical_root` (or the raw
        // non-canonical root if e.g. `/var` vs `/private/var` on macOS).
        let mut lex = PathBuf::new();
        for comp in p.components() {
            match comp {
                Component::Prefix(pref) => lex.push(pref.as_os_str()),
                Component::RootDir => lex.push(comp.as_os_str()),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !lex.pop() {
                        return Err((
                            JSONRPC_PERMISSION_DENIED,
                            format!("Path `{trimmed}` escapes workspace root"),
                        ));
                    }
                }
                Component::Normal(seg) => lex.push(seg),
            }
        }
        let stripped = if let Ok(s) = lex.strip_prefix(canonical_root) {
            s.to_path_buf()
        } else if let Ok(canon_existing) = fs::canonicalize(&lex)
            && let Ok(s) = canon_existing.strip_prefix(canonical_root)
        {
            s.to_path_buf()
        } else {
            // Check canonicalized longest existing ancestor (handles macOS `/var` -> `/private/var`
            // when creating a brand-new file inside an existing workspace directory).
            let mut probe = lex.as_path();
            let mut tail: Vec<std::ffi::OsString> = Vec::new();
            let mut matched: Option<PathBuf> = None;
            while let Some(parent) = probe.parent() {
                if let Some(name) = probe.file_name() {
                    tail.push(name.to_os_string());
                }
                if let Ok(canon_parent) = fs::canonicalize(parent)
                    && let Ok(prefix_rel) = canon_parent.strip_prefix(canonical_root)
                {
                    let mut rebuilt = prefix_rel.to_path_buf();
                    for seg in tail.into_iter().rev() {
                        rebuilt.push(seg);
                    }
                    matched = Some(rebuilt);
                    break;
                }
                probe = parent;
            }
            matched.ok_or_else(|| {
                (
                    JSONRPC_PERMISSION_DENIED,
                    format!("Path `{trimmed}` is outside workspace root"),
                )
            })?
        };
        for comp in stripped.components() {
            match comp {
                Component::Normal(os) => {
                    let s = os.to_str().ok_or_else(|| {
                        (
                            JSONRPC_INVALID_PARAMS,
                            "Path contains non-UTF-8 characters".to_string(),
                        )
                    })?;
                    rel_segments.push(s.to_string());
                }
                Component::CurDir => {}
                _ => {
                    return Err((
                        JSONRPC_PERMISSION_DENIED,
                        format!("Path `{trimmed}` escapes workspace root"),
                    ));
                }
            }
        }
    } else {
        for comp in p.components() {
            match comp {
                Component::Normal(os) => {
                    let s = os.to_str().ok_or_else(|| {
                        (
                            JSONRPC_INVALID_PARAMS,
                            "Path contains non-UTF-8 characters".to_string(),
                        )
                    })?;
                    rel_segments.push(s.to_string());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if rel_segments.pop().is_none() {
                        return Err((
                            JSONRPC_PERMISSION_DENIED,
                            format!("Path `{trimmed}` escapes workspace root"),
                        ));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err((
                        JSONRPC_PERMISSION_DENIED,
                        format!("Path `{trimmed}` escapes workspace root"),
                    ));
                }
            }
        }
    }

    if rel_segments.is_empty() && !allow_root_itself {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            "Refusing to operate directly on workspace root directory".to_string(),
        ));
    }
    Ok(rel_segments)
}

fn resolve_confined_dir(
    workspace_root: &Path,
    raw_cwd: Option<&str>,
) -> Result<PathBuf, (i64, String)> {
    let canonical_root = canonicalize_root(workspace_root)?;
    let Some(raw) = raw_cwd else {
        return Ok(canonical_root);
    };
    let segments = extract_confined_relative_segments(&canonical_root, raw, true)?;
    let mut current = canonical_root.clone();
    for seg in &segments {
        let next = current.join(seg);
        let canon = fs::canonicalize(&next).map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                JSONRPC_RESOURCE_NOT_FOUND
            } else {
                JSONRPC_PERMISSION_DENIED
            };
            (code, format!("Cannot resolve terminal cwd `{raw}`: {e}"))
        })?;
        if !canon.starts_with(&canonical_root) {
            return Err((
                JSONRPC_PERMISSION_DENIED,
                format!("Terminal cwd `{raw}` escapes workspace root"),
            ));
        }
        if !canon.is_dir() {
            return Err((
                JSONRPC_INVALID_PARAMS,
                format!("Terminal cwd `{raw}` is not a directory"),
            ));
        }
        current = canon;
    }
    Ok(current)
}

fn check_policy_path(
    policy: Option<&orbit_protocol::SandboxPolicy>,
    path: &Path,
    write: bool,
) -> Result<(), (i64, String)> {
    if let Some(p) = policy.filter(|p| p.mode != orbit_protocol::SandboxMode::Off)
        && ((write && p.mode == orbit_protocol::SandboxMode::StrictReadOnly)
            || orbit_shim::sandbox::denied_paths(p)
                .map_err(|e| (JSONRPC_PERMISSION_DENIED, e.to_string()))?
                .iter()
                .any(|d| path.starts_with(d)))
    {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            "host sandbox policy denied filesystem access".into(),
        ));
    }
    Ok(())
}
/// Handle ACP `fs/read_text_file` with strict workspace confinement, ARM64-safe `O_NOFOLLOW` +
/// `(dev, ino)` symlink-swap defense, 4 MiB read cap, and optional 1-based `line` & `limit`.
pub fn handle_fs_read_text_file(
    workspace_root: &Path,
    params: &Value,
) -> Result<Value, (i64, String)> {
    read_with_policy(workspace_root, params, None)
}
fn read_with_policy(
    workspace_root: &Path,
    params: &Value,
    policy: Option<&orbit_protocol::SandboxPolicy>,
) -> Result<Value, (i64, String)> {
    let raw_path = params.get("path").and_then(Value::as_str).ok_or_else(|| {
        (
            JSONRPC_INVALID_PARAMS,
            "fs/read_text_file requires string `path`".to_string(),
        )
    })?;
    let canonical_root = canonicalize_root(workspace_root)?;
    let segments = extract_confined_relative_segments(&canonical_root, raw_path, false)?;

    // Walk every path segment checking symlink confinement before opening the final leaf.
    let mut current = canonical_root.clone();
    for (idx, seg) in segments.iter().enumerate() {
        let next = current.join(seg);
        let lmeta = fs::symlink_metadata(&next).map_err(|e| {
            let code = if e.kind() == std::io::ErrorKind::NotFound {
                JSONRPC_RESOURCE_NOT_FOUND
            } else {
                JSONRPC_PERMISSION_DENIED
            };
            (code, format!("Cannot access `{raw_path}`: {e}"))
        })?;
        let is_last = idx + 1 == segments.len();
        if lmeta.file_type().is_symlink() {
            let canon = fs::canonicalize(&next).map_err(|e| {
                (
                    JSONRPC_PERMISSION_DENIED,
                    format!("Refusing to follow invalid symlink `{seg}`: {e}"),
                )
            })?;
            if !canon.starts_with(&canonical_root) {
                return Err((
                    JSONRPC_PERMISSION_DENIED,
                    format!("Refusing to follow symlink `{seg}` outside workspace root"),
                ));
            }
            if is_last {
                // Reject symlinks on the final target leaf so `O_NOFOLLOW` open matches verified inode.
                return Err((
                    JSONRPC_PERMISSION_DENIED,
                    format!("Refusing to read symlink target `{raw_path}` directly"),
                ));
            }
            current = canon;
        } else {
            let canon = fs::canonicalize(&next).map_err(|e| {
                (
                    JSONRPC_PERMISSION_DENIED,
                    format!("Cannot resolve `{raw_path}`: {e}"),
                )
            })?;
            if !canon.starts_with(&canonical_root) {
                return Err((
                    JSONRPC_PERMISSION_DENIED,
                    format!("Path `{raw_path}` escapes workspace root"),
                ));
            }
            current = canon;
        }
    }

    check_policy_path(policy, &current, false)?;
    let lmeta_before = fs::symlink_metadata(&current).map_err(|e| {
        (
            JSONRPC_RESOURCE_NOT_FOUND,
            format!("Cannot stat `{raw_path}`: {e}"),
        )
    })?;
    if lmeta_before.file_type().is_symlink() || !lmeta_before.is_file() {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            format!("Target `{raw_path}` is not a regular file"),
        ));
    }

    let mut opts = OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    if let Some(flag) = unix_o_nofollow_flag() {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(flag);
    }
    let file = opts.open(&current).map_err(|e| {
        (
            JSONRPC_PERMISSION_DENIED,
            format!("Failed to open `{raw_path}`: {e}"),
        )
    })?;
    let fmeta = file.metadata().map_err(|e| {
        (
            JSONRPC_INTERNAL_ERROR,
            format!("Failed to inspect `{raw_path}`: {e}"),
        )
    })?;
    if !fmeta.is_file() {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            format!("Target `{raw_path}` is not a regular file"),
        ));
    }
    if fmeta.len() > MAX_ACP_FS_BYTES as u64 {
        return Err((
            JSONRPC_LIMIT_EXCEEDED,
            format!(
                "File `{raw_path}` ({} bytes) exceeds 4 MiB ACP read limit",
                fmeta.len()
            ),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let lmeta_after = fs::symlink_metadata(&current).map_err(|e| {
            (
                JSONRPC_PERMISSION_DENIED,
                format!("Failed to re-verify `{raw_path}`: {e}"),
            )
        })?;
        if lmeta_after.file_type().is_symlink()
            || fmeta.dev() != lmeta_before.dev()
            || fmeta.ino() != lmeta_before.ino()
            || fmeta.dev() != lmeta_after.dev()
            || fmeta.ino() != lmeta_after.ino()
        {
            return Err((
                JSONRPC_PERMISSION_DENIED,
                format!("Target `{raw_path}` changed during open; refusing read"),
            ));
        }
    }

    let mut buf = Vec::new();
    file.take((MAX_ACP_FS_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| {
            (
                JSONRPC_INTERNAL_ERROR,
                format!("Failed reading `{raw_path}`: {e}"),
            )
        })?;
    if buf.len() > MAX_ACP_FS_BYTES {
        return Err((
            JSONRPC_LIMIT_EXCEEDED,
            format!("File `{raw_path}` exceeds 4 MiB ACP read limit"),
        ));
    }
    let full_text = String::from_utf8(buf).map_err(|_| {
        (
            JSONRPC_INVALID_PARAMS,
            format!("File `{raw_path}` is not valid UTF-8 text"),
        )
    })?;

    let start_line = params
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());
    let limit = params
        .get("limit")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok());

    let content = slice_text_lines(&full_text, start_line, limit);
    Ok(json!({ "content": content }))
}

fn slice_text_lines(text: &str, start_line: Option<usize>, limit: Option<usize>) -> String {
    if start_line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let skip = start_line.unwrap_or(1).saturating_sub(1);
    let iter = text.split_inclusive('\n').skip(skip);
    match limit {
        Some(0) => String::new(),
        Some(n) => iter.take(n).collect(),
        None => iter.collect(),
    }
}

struct AtomicTempGuard {
    path: PathBuf,
    armed: bool,
}

impl AtomicTempGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for AtomicTempGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Handle ACP `fs/write_text_file` with strict workspace confinement, `.git/` & symlink rejection,
/// 4 MiB size cap, and atomic temp-file (`create_new` + `O_NOFOLLOW`) + `rename`.
pub fn handle_fs_write_text_file(
    workspace_root: &Path,
    params: &Value,
) -> Result<Value, (i64, String)> {
    write_with_policy(workspace_root, params, None)
}
fn write_with_policy(
    workspace_root: &Path,
    params: &Value,
    policy: Option<&orbit_protocol::SandboxPolicy>,
) -> Result<Value, (i64, String)> {
    let raw_path = params.get("path").and_then(Value::as_str).ok_or_else(|| {
        (
            JSONRPC_INVALID_PARAMS,
            "fs/write_text_file requires string `path`".to_string(),
        )
    })?;
    let content = params
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            (
                JSONRPC_INVALID_PARAMS,
                "fs/write_text_file requires string `content`".to_string(),
            )
        })?;
    if content.len() > MAX_ACP_FS_BYTES {
        return Err((
            JSONRPC_LIMIT_EXCEEDED,
            format!(
                "Content ({} bytes) exceeds 4 MiB ACP write limit",
                content.len()
            ),
        ));
    }

    let canonical_root = canonicalize_root(workspace_root)?;
    if path_has_git_component(&canonical_root) {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            "Writing inside `.git` internals is forbidden".to_string(),
        ));
    }

    let segments = extract_confined_relative_segments(&canonical_root, raw_path, false)?;
    if segments.iter().any(|s| s.eq_ignore_ascii_case(".git")) {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            "Writing inside `.git` internals is forbidden".to_string(),
        ));
    }

    let (leaf, parent_segments) = segments.split_last().ok_or_else(|| {
        (
            JSONRPC_INVALID_PARAMS,
            "Target path has no file name".to_string(),
        )
    })?;

    check_policy_path(
        policy,
        &segments
            .iter()
            .fold(canonical_root.clone(), |p, s| p.join(s)),
        true,
    )?;
    // Walk/create parent directories strictly inside `canonical_root`, rejecting escaping symlinks & `.git`.
    let mut parent_dir = canonical_root.clone();
    for seg in parent_segments {
        let next = parent_dir.join(seg);
        match fs::symlink_metadata(&next) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    return Err((
                        JSONRPC_PERMISSION_DENIED,
                        format!("Refusing to traverse symlink directory `{seg}` during write"),
                    ));
                }
                if !meta.is_dir() {
                    return Err((
                        JSONRPC_INVALID_PARAMS,
                        format!("Parent `{seg}` is not a directory"),
                    ));
                }
                let canon = fs::canonicalize(&next).map_err(|e| {
                    (
                        JSONRPC_PERMISSION_DENIED,
                        format!("Cannot resolve parent `{seg}`: {e}"),
                    )
                })?;
                if !canon.starts_with(&canonical_root) || path_has_git_component(&canon) {
                    return Err((
                        JSONRPC_PERMISSION_DENIED,
                        "Parent directory escapes workspace boundary".to_string(),
                    ));
                }
                parent_dir = canon;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&next).map_err(|ce| {
                    (
                        JSONRPC_INTERNAL_ERROR,
                        format!("Failed creating parent directory `{seg}`: {ce}"),
                    )
                })?;
                let canon = fs::canonicalize(&next).map_err(|ce| {
                    (
                        JSONRPC_PERMISSION_DENIED,
                        format!("Cannot resolve created directory `{seg}`: {ce}"),
                    )
                })?;
                if !canon.starts_with(&canonical_root) || path_has_git_component(&canon) {
                    return Err((
                        JSONRPC_PERMISSION_DENIED,
                        "Created directory escapes workspace boundary".to_string(),
                    ));
                }
                parent_dir = canon;
            }
            Err(e) => {
                return Err((
                    JSONRPC_PERMISSION_DENIED,
                    format!("Cannot inspect parent `{seg}`: {e}"),
                ));
            }
        }
    }

    let target_path = parent_dir.join(leaf);
    let existing_perms = match fs::symlink_metadata(&target_path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err((
                    JSONRPC_PERMISSION_DENIED,
                    format!("Refusing to overwrite symlink `{raw_path}`"),
                ));
            }
            if !meta.is_file() {
                return Err((
                    JSONRPC_INVALID_PARAMS,
                    format!("Target `{raw_path}` exists and is not a regular file"),
                ));
            }
            Some(meta.permissions())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err((
                JSONRPC_PERMISSION_DENIED,
                format!("Cannot inspect `{raw_path}`: {e}"),
            ));
        }
    };

    // Re-verify every ancestor component immediately before opening the temp file.
    verify_non_symlink_parent_chain(&canonical_root, parent_segments)?;

    let tmp_name = format!(".orbit-acp-write-{}.tmp", Uuid::new_v4().simple());
    let tmp_path = parent_dir.join(tmp_name);
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Always create temp files private (0600) BEFORE writing any bytes so secrets/edits
        // are never momentarily world-readable prior to applying target permissions.
        opts.mode(0o600);
        if let Some(flag) = unix_o_nofollow_flag() {
            opts.custom_flags(flag);
        }
    }

    let mut tmp_file = opts.open(&tmp_path).map_err(|e| {
        (
            JSONRPC_INTERNAL_ERROR,
            format!("Failed creating atomic temp file for `{raw_path}`: {e}"),
        )
    })?;
    let mut guard = AtomicTempGuard::new(tmp_path.clone());

    tmp_file
        .write_all(content.as_bytes())
        .and_then(|()| tmp_file.flush())
        .and_then(|()| tmp_file.sync_all())
        .map_err(|e| {
            (
                JSONRPC_INTERNAL_ERROR,
                format!("Failed writing temp file for `{raw_path}`: {e}"),
            )
        })?;

    if let Some(perms) = existing_perms {
        tmp_file.set_permissions(perms).map_err(|e| {
            (
                JSONRPC_INTERNAL_ERROR,
                format!("Failed preserving permissions on temp file for `{raw_path}`: {e}"),
            )
        })?;
    }
    drop(tmp_file);

    // Component-by-component non-symlink & `.git`-free re-verification immediately before atomic rename.
    verify_non_symlink_parent_chain(&canonical_root, parent_segments)?;
    if let Ok(target_after) = fs::symlink_metadata(&target_path)
        && (target_after.file_type().is_symlink() || !target_after.is_file())
    {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            format!("Target `{raw_path}` changed to symlink during write"),
        ));
    }

    fs::rename(&tmp_path, &target_path).map_err(|e| {
        (
            JSONRPC_INTERNAL_ERROR,
            format!("Failed atomic rename for `{raw_path}`: {e}"),
        )
    })?;
    guard.disarm();

    Ok(json!({}))
}

fn verify_non_symlink_parent_chain(
    canonical_root: &Path,
    parent_segments: &[String],
) -> Result<(), (i64, String)> {
    let mut cur = canonical_root.to_path_buf();
    for seg in parent_segments {
        if seg.eq_ignore_ascii_case(".git") {
            return Err((
                JSONRPC_PERMISSION_DENIED,
                "Writing inside `.git` internals is forbidden".to_string(),
            ));
        }
        cur.push(seg);
        let meta = fs::symlink_metadata(&cur).map_err(|e| {
            (
                JSONRPC_PERMISSION_DENIED,
                format!("Failed re-verifying parent `{seg}`: {e}"),
            )
        })?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err((
                JSONRPC_PERMISSION_DENIED,
                format!("Parent `{seg}` changed to symlink or non-directory during write"),
            ));
        }
    }
    let canon = fs::canonicalize(&cur).map_err(|e| {
        (
            JSONRPC_PERMISSION_DENIED,
            format!("Failed canonicalizing parent directory during write: {e}"),
        )
    })?;
    if !canon.starts_with(canonical_root) || path_has_git_component(&canon) {
        return Err((
            JSONRPC_PERMISSION_DENIED,
            "Parent directory escapes workspace boundary".to_string(),
        ));
    }
    Ok(())
}

// --- Zero-`unsafe` Process Group Isolation & Termination ---

fn configure_isolated_process_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        // CREATE_NEW_PROCESS_GROUP (0x200) | CREATE_NO_WINDOW (0x0800_0000)
        cmd.creation_flags(0x0000_0200 | 0x0800_0000);
    }
}

#[cfg(unix)]
const UNIX_KILL_BINS: [&str; 3] = ["/bin/kill", "/usr/bin/kill", "kill"];

#[cfg(unix)]
fn build_unix_kill_pgid_args(pgid: u32, signal: &str) -> Option<[String; 4]> {
    if pgid <= 1 {
        return None;
    }
    let sig = signal.trim().trim_start_matches('-');
    if sig.is_empty() {
        return None;
    }
    // Always pass `--` before `-<pgid>` so GNU/procps-ng `getopt_long` never misparses
    // `-1xxxxxx` as `kill(-1, ...)`!
    Some([
        "-s".to_string(),
        sig.to_string(),
        "--".to_string(),
        format!("-{pgid}"),
    ])
}

#[cfg(unix)]
fn unix_signal_name(sig: i32) -> String {
    match sig {
        1 => "SIGHUP".to_string(),
        2 => "SIGINT".to_string(),
        3 => "SIGQUIT".to_string(),
        9 => "SIGKILL".to_string(),
        13 => "SIGPIPE".to_string(),
        15 => "SIGTERM".to_string(),
        other => format!("SIG{other}"),
    }
}

async fn terminate_process_group_async(pid: u32, signal: &str) {
    #[cfg(unix)]
    if let Some(args) = build_unix_kill_pgid_args(pid, signal) {
        for bin in UNIX_KILL_BINS {
            let res = Command::new(bin)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            if res.is_ok() {
                break;
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = signal;
        if pid <= 1 {
            return;
        }
        let pid_str = pid.to_string();
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid_str, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000);
        let _ = cmd.status().await;
    }
}

fn terminate_process_group_sync(pid: u32, signal: &str) {
    #[cfg(unix)]
    if let Some(args) = build_unix_kill_pgid_args(pid, signal) {
        for bin in UNIX_KILL_BINS {
            let res = std::process::Command::new(bin)
                .args(&args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            if res.is_ok() {
                break;
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = signal;
        if pid <= 1 {
            return;
        }
        use std::os::windows::process::CommandExt;
        let pid_str = pid.to_string();
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid_str, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "orbit_acp_cb_{tag}_{}",
            &Uuid::new_v4().simple().to_string()[..8]
        ));
        fs::create_dir_all(&dir).expect("mkdir temp ws");
        fs::canonicalize(&dir).unwrap_or(dir)
    }

    #[test]
    fn client_capabilities_advertise_fs_and_terminal() {
        let caps = acp_client_capabilities();
        assert_eq!(caps.pointer("/fs/readTextFile"), Some(&json!(true)));
        assert_eq!(caps.pointer("/fs/writeTextFile"), Some(&json!(true)));
        assert_eq!(caps.pointer("/terminal"), Some(&json!(true)));
    }

    #[test]
    fn fs_read_and_write_enforce_workspace_confinement_git_block_and_symlink_rejection() {
        let ws = temp_ws("fs_sec");
        let outside = temp_ws("fs_outside");
        let outside_file = outside.join("secret.txt");
        fs::write(&outside_file, "TOP_SECRET").expect("write outside");
        fs::create_dir_all(ws.join(".git")).expect("mkdir .git");

        // 1. Atomic write inside workspace (creating nested parent dirs) + read with line & limit
        let write_ok = handle_fs_write_text_file(
            &ws,
            &json!({
                "path": "src/nested/hello.txt",
                "content": "line1\nline2\nline3\nline4\n"
            }),
        )
        .expect("write inside ws");
        assert_eq!(write_ok, json!({}));

        let full_read = handle_fs_read_text_file(&ws, &json!({ "path": "src/nested/hello.txt" }))
            .expect("read full");
        assert_eq!(full_read["content"], json!("line1\nline2\nline3\nline4\n"));

        let sliced_read = handle_fs_read_text_file(
            &ws,
            &json!({ "path": ws.join("src/nested/hello.txt").to_string_lossy(), "line": 2, "limit": 2 }),
        )
        .expect("read sliced");
        assert_eq!(sliced_read["content"], json!("line2\nline3\n"));

        // 2. Reject `..` escape and absolute path outside workspace
        assert!(
            handle_fs_read_text_file(&ws, &json!({ "path": "../secret.txt" })).is_err(),
            "must reject .. traversal"
        );
        assert!(
            handle_fs_read_text_file(&ws, &json!({ "path": outside_file.to_string_lossy() }))
                .is_err(),
            "must reject outside absolute path"
        );
        assert!(
            handle_fs_write_text_file(&ws, &json!({ "path": "../escape.txt", "content": "nope" }))
                .is_err(),
            "must reject write traversal"
        );

        // 3. Reject writing into `.git/`
        assert!(
            handle_fs_write_text_file(
                &ws,
                &json!({ "path": ".git/config", "content": "[core]\n" })
            )
            .is_err(),
            "must reject .git write"
        );
        assert!(
            handle_fs_write_text_file(
                &ws,
                &json!({ "path": ".GIT/hooks/pre-commit", "content": "#!/bin/sh\n" })
            )
            .is_err(),
            "must reject case-insensitive .GIT write"
        );

        // 4. Reject escaping symlinks for both read and write
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let escape_link = ws.join("escape_link.txt");
            symlink(&outside_file, &escape_link).expect("symlink file");
            assert!(
                handle_fs_read_text_file(&ws, &json!({ "path": "escape_link.txt" })).is_err(),
                "must reject reading escaping symlink"
            );
            assert!(
                handle_fs_write_text_file(
                    &ws,
                    &json!({ "path": "escape_link.txt", "content": "pwned" })
                )
                .is_err(),
                "must reject overwriting escaping symlink"
            );

            let escape_dir_link = ws.join("dir_escape");
            symlink(&outside, &escape_dir_link).expect("symlink dir");
            assert!(
                handle_fs_write_text_file(
                    &ws,
                    &json!({ "path": "dir_escape/pwn.txt", "content": "pwned" })
                )
                .is_err(),
                "must reject writing through escaping dir symlink"
            );
            assert_eq!(
                fs::read_to_string(&outside_file).expect("read outside"),
                "TOP_SECRET"
            );
        }

        // 5. Reject payloads exceeding 4 MiB cap
        let oversized = "a".repeat(MAX_ACP_FS_BYTES + 1);
        let err =
            handle_fs_write_text_file(&ws, &json!({ "path": "huge.txt", "content": oversized }))
                .expect_err("must reject >4 MiB write");
        assert_eq!(err.0, JSONRPC_LIMIT_EXCEEDED);

        let _ = fs::remove_dir_all(&ws);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_create_output_wait_kill_release_env_strip_and_drop_cleanup() {
        let ws = temp_ws("term_ops");
        let outside = temp_ws("term_outside");

        let (mut host, mut reply_rx) = AcpCallbackHost::new(ws.clone());

        // 1. Reject cwd outside workspace
        let bad_cwd = host
            .handle_request(
                &json!(1),
                "terminal/create",
                &json!({
                    "command": "/bin/sh",
                    "args": ["-c", "pwd"],
                    "cwd": outside.to_string_lossy()
                }),
            )
            .await;
        match bad_cwd {
            AcpCallbackOutcome::Reply(line) => {
                let v: Value = serde_json::from_str(&line).expect("json");
                assert!(v.get("error").is_some(), "outside cwd must fail: {line}");
            }
            other => panic!("expected Reply, got {other:?}"),
        }

        // 2. Create terminal that echoes output & verifies blocked ORBIT_MCP_TOKEN isn't injected
        let created_reply = host
            .handle_request(
                &json!(2),
                "terminal/create",
                &json!({
                    "command": "/bin/sh",
                    "args": ["-c", "printf 'hello_acp:%s:%s' \"$CUSTOM_OK\" \"$ORBIT_MCP_TOKEN\""],
                    "env": [
                        { "name": "CUSTOM_OK", "value": "yes42" },
                        { "name": "ORBIT_MCP_TOKEN", "value": "LEAKED_SECRET" }
                    ]
                }),
            )
            .await;
        let tid = match created_reply {
            AcpCallbackOutcome::Reply(line) => {
                let v: Value = serde_json::from_str(&line).expect("json");
                v.pointer("/result/terminalId")
                    .and_then(Value::as_str)
                    .expect("terminalId")
                    .to_string()
            }
            other => panic!("expected Reply, got {other:?}"),
        };

        // 3. Wait for exit (either immediate Reply or Deferred pushed to reply_rx)
        let wait_outcome = host
            .handle_request(
                &json!(3),
                "terminal/wait_for_exit",
                &json!({ "terminalId": tid }),
            )
            .await;
        let wait_line = match wait_outcome {
            AcpCallbackOutcome::Reply(l) => l,
            AcpCallbackOutcome::Deferred => {
                tokio::time::timeout(std::time::Duration::from_secs(3), reply_rx.recv())
                    .await
                    .expect("wait timeout")
                    .expect("wait reply")
            }
            AcpCallbackOutcome::Unsupported => panic!("wait_for_exit must be supported"),
        };
        let wait_val: Value = serde_json::from_str(&wait_line).expect("wait json");
        assert_eq!(wait_val.pointer("/result/exitCode"), Some(&json!(0)));

        // 4. Read terminal/output and verify stripped secret env + exact output
        let out_reply = host
            .handle_request(&json!(4), "terminal/output", &json!({ "terminalId": tid }))
            .await;
        match out_reply {
            AcpCallbackOutcome::Reply(line) => {
                let v: Value = serde_json::from_str(&line).expect("out json");
                assert_eq!(
                    v.pointer("/result/output"),
                    Some(&json!("hello_acp:yes42:"))
                );
                assert_eq!(v.pointer("/result/truncated"), Some(&json!(false)));
                assert_eq!(v.pointer("/result/exitStatus/exitCode"), Some(&json!(0)));
            }
            other => panic!("expected Reply, got {other:?}"),
        }

        // 5. Release terminal
        let rel_reply = host
            .handle_request(&json!(5), "terminal/release", &json!({ "terminalId": tid }))
            .await;
        assert!(matches!(rel_reply, AcpCallbackOutcome::Reply(_)));
        assert_eq!(host.active_terminal_count(), 0);

        // 6. Spawn long-running terminal, verify `terminal/kill` terminates it cleanly
        let sleep_reply = host
            .handle_request(
                &json!(6),
                "terminal/create",
                &json!({ "command": "/bin/sleep", "args": ["30"] }),
            )
            .await;
        let sleep_tid = match sleep_reply {
            AcpCallbackOutcome::Reply(line) => {
                let v: Value = serde_json::from_str(&line).expect("json");
                v.pointer("/result/terminalId")
                    .and_then(Value::as_str)
                    .expect("sleep tid")
                    .to_string()
            }
            other => panic!("expected Reply, got {other:?}"),
        };
        let kill_reply = host
            .handle_request(
                &json!(7),
                "terminal/kill",
                &json!({ "terminalId": sleep_tid }),
            )
            .await;
        assert!(matches!(kill_reply, AcpCallbackOutcome::Reply(_)));

        // 7. Spawn another long-running terminal and verify Drop kills its process group cleanly
        let leak_pid_file = ws.join("orphan.pid");
        let drop_reply = host
            .handle_request(
                &json!(8),
                "terminal/create",
                &json!({
                    "command": "/bin/sh",
                    "args": ["-c", format!("echo $$ > '{}' && exec /bin/sleep 60", leak_pid_file.display())]
                }),
            )
            .await;
        assert!(matches!(drop_reply, AcpCallbackOutcome::Reply(_)));
        for _ in 0..40 {
            if leak_pid_file.exists()
                && fs::read_to_string(&leak_pid_file)
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        let child_pid: u32 = fs::read_to_string(&leak_pid_file)
            .expect("read orphan pid")
            .trim()
            .parse()
            .expect("parse pid");

        drop(host);
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;

        let alive = std::process::Command::new("/bin/kill")
            .args(["-0", &child_pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            !alive,
            "AcpCallbackHost::drop must kill all spawned process groups (pid {child_pid} survived!)"
        );

        let _ = fs::remove_dir_all(&ws);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn unix_kill_pgid_args_enforce_pgid_gt_1_and_option_terminator() {
        assert!(build_unix_kill_pgid_args(0, "KILL").is_none());
        assert!(build_unix_kill_pgid_args(1, "KILL").is_none());
        assert_eq!(
            build_unix_kill_pgid_args(123456, "TERM"),
            Some([
                "-s".to_string(),
                "TERM".to_string(),
                "--".to_string(),
                "-123456".to_string(),
            ])
        );
    }

    #[cfg(unix)]
    #[test]
    fn fs_write_creates_0600_temp_preserves_existing_mode_and_blocks_case_insensitive_git() {
        use std::os::unix::fs::PermissionsExt;

        let ws = temp_ws("fs-perms-git");
        let new_file = ws.join("secret.env");
        handle_fs_write_text_file(
            &ws,
            &json!({ "path": "secret.env", "content": "TOKEN=1\n" }),
        )
        .expect("write new file");
        let mode_new = fs::metadata(&new_file).expect("meta").permissions().mode() & 0o777;
        assert_eq!(
            mode_new, 0o600,
            "newly written file must inherit 0600 temp mode"
        );

        // Existing 0640 file preserves its mode across atomic rewrite
        let existing = ws.join("preserved.txt");
        fs::write(&existing, "v1").expect("seed");
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o640)).expect("chmod 0640");
        handle_fs_write_text_file(&ws, &json!({ "path": "preserved.txt", "content": "v2" }))
            .expect("rewrite existing");
        let mode_after = fs::metadata(&existing).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode_after, 0o640, "existing file mode must be preserved");

        // Case-insensitive .GIT component is rejected
        for bad in [".GIT/config", "sub/.Git/HEAD", ".gIt/hooks/pre-commit"] {
            let err = handle_fs_write_text_file(&ws, &json!({ "path": bad, "content": "pwned" }))
                .expect_err("case-insensitive .git must be rejected");
            assert_eq!(err.0, JSONRPC_PERMISSION_DENIED);
        }

        let _ = fs::remove_dir_all(&ws);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn terminal_wait_for_exit_does_not_hang_on_descendant_pipes_caps_waiters_and_retires_pgid()
     {
        let ws = temp_ws("term-wait-drain");
        let (mut host, mut reply_rx) = AcpCallbackHost::new(ws.clone());

        // 1. Descendant inherits stdout/stderr pipe, leader exits immediately -> wait_for_exit
        //    must return promptly after the bounded 250ms drain timeout, and live_pgid must retire to 0.
        let desc_pid_file = ws.join("desc_pipe.pid");
        let create = host
            .handle_request(
                &json!(1),
                "terminal/create",
                &json!({
                    "command": "/bin/sh",
                    "args": [
                        "-c",
                        format!("sleep 20 & echo $! > {}; echo fast-leader", desc_pid_file.display())
                    ]
                }),
            )
            .await;
        let AcpCallbackOutcome::Reply(create_line) = create else {
            panic!("expected reply");
        };
        let tid = serde_json::from_str::<Value>(&create_line)
            .expect("json")
            .pointer("/result/terminalId")
            .and_then(Value::as_str)
            .expect("terminalId")
            .to_string();

        let wait_outcome = host
            .handle_request(
                &json!(2),
                "terminal/wait_for_exit",
                &json!({ "terminalId": tid }),
            )
            .await;
        let wait_resp: Value = match wait_outcome {
            AcpCallbackOutcome::Reply(line) => serde_json::from_str(&line).expect("json"),
            AcpCallbackOutcome::Deferred => {
                let line = tokio::time::timeout(Duration::from_secs(2), reply_rx.recv())
                    .await
                    .expect("wait_for_exit must not hang on descendant-held pipe")
                    .expect("reply");
                serde_json::from_str(&line).expect("json")
            }
            AcpCallbackOutcome::Unsupported => panic!("unexpected unsupported"),
        };
        assert_eq!(
            wait_resp
                .pointer("/result/exitCode")
                .and_then(Value::as_i64),
            Some(0)
        );

        // Once reaped, live_pgid must be retired to 0 so PID reuse can never occur.
        let live_after_reap = host
            .terminals
            .get(&tid)
            .expect("term tracked")
            .live_pgid
            .load(Ordering::Acquire);
        assert_eq!(
            live_after_reap, 0,
            "live_pgid must retire to 0 once leader is reaped"
        );

        // Clean up the descendant spawned in the pipe test.
        if let Ok(raw_pid) = fs::read_to_string(&desc_pid_file)
            && let Ok(dpid) = raw_pid.trim().parse::<u32>()
            && dpid > 1
        {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-s", "KILL", "--", &dpid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = host
            .handle_request(&json!(3), "terminal/release", &json!({ "terminalId": tid }))
            .await;

        // 2. Bounded waiter cap & release kills background descendants that closed pipes.
        let bg_pid_file = ws.join("bg_closed_pipes.pid");
        let create2 = host
            .handle_request(
                &json!(10),
                "terminal/create",
                &json!({
                    "command": "/bin/sh",
                    "args": [
                        "-c",
                        format!(
                            "sleep 30 </dev/null >/dev/null 2>&1 & echo $! > {}; sleep 30",
                            bg_pid_file.display()
                        )
                    ]
                }),
            )
            .await;
        let AcpCallbackOutcome::Reply(create2_line) = create2 else {
            panic!("expected reply");
        };
        let tid2 = serde_json::from_str::<Value>(&create2_line)
            .expect("json")
            .pointer("/result/terminalId")
            .and_then(Value::as_str)
            .expect("terminalId")
            .to_string();

        for _ in 0..40 {
            if bg_pid_file.exists()
                && fs::read_to_string(&bg_pid_file)
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        let bg_pid: u32 = fs::read_to_string(&bg_pid_file)
            .expect("read bg pid")
            .trim()
            .parse()
            .expect("parse bg pid");

        for i in 0..MAX_ACP_TERMINAL_WAITERS {
            let res = host
                .handle_request(
                    &json!(100 + i),
                    "terminal/wait_for_exit",
                    &json!({ "terminalId": tid2 }),
                )
                .await;
            assert!(matches!(res, AcpCallbackOutcome::Deferred));
        }
        let capped = host
            .handle_request(
                &json!(999),
                "terminal/wait_for_exit",
                &json!({ "terminalId": tid2 }),
            )
            .await;
        let AcpCallbackOutcome::Reply(capped_line) = capped else {
            panic!("expected cap error reply");
        };
        let capped_json: Value = serde_json::from_str(&capped_line).expect("json");
        assert_eq!(
            capped_json.pointer("/error/code").and_then(Value::as_i64),
            Some(JSONRPC_LIMIT_EXCEEDED)
        );

        // Releasing tid2 must terminate the entire live PGID (killing bg_pid too!).
        let _ = host
            .handle_request(
                &json!(1000),
                "terminal/release",
                &json!({ "terminalId": tid2 }),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(120)).await;
        let bg_alive = std::process::Command::new("/bin/kill")
            .args(["-0", &bg_pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(
            !bg_alive,
            "terminal/release must kill background descendants in the live process group (pid {bg_pid} survived!)"
        );

        let _ = fs::remove_dir_all(&ws);
    }
}
