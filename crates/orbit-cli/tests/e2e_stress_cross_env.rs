#![cfg(unix)]
//! High-pressure isolated multi-process & cross-environment stress suite for the compiled `orbit` binary.
//!
//! Exercises 4 real standalone `orbit` OS processes (`1 orbit relay` + `3 orbit daemon` hosts across
//! Direct WS and Relay-Only Noise NK E2EE) plus `OrbitAppCore` multi-host client projection:
//! 1. 42+ concurrent `Conversation` + `Subagent` sessions across Local, Direct Mesh, E2EE Relay WAN,
//!    worktrees, MCP `fork`, CLI `orbit run`, and CLI `orbit fork` — asserting zero sidebar leakage
//!    and accurate cross-host permission approval bubbling.
//! 2. `PersonalRouter` persistent `--append-system-prompt` subprocess verification, automatic Fleet
//!    Mesh sync on `set_primary_router_selection`, cross-host Workspace/Goal Atlas, finish wakeups,
//!    and manual-only `reclaim_conversation` / `harvest_conversation`.
//! 3. 24-way concurrent `BOARD.md` + `update_goal` CAS storm and 5-epoch `reset_board` rollover with
//!    `BOARD.history.md` persistence and `board_reset_seq` conflict watermark isolation.
//! 4. Daemon `SIGTERM` hot-restart survival with live detached `orbit shim` reattachment.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use orbit_client_core::app_core::OrbitAppCore;
use orbit_client_core::sidebar::{SidebarProjectSort, SidebarThreadFilter};
use orbit_client_core::{HostConnectionState, HostRouteMode};
use orbit_crypto::PairingOffer;
use orbit_protocol::{
    ConnectedHostStatus, PersonalRouterConfig, PrimaryRouterSelection, ProviderKind, SessionTier,
};
use serde_json::{Value, json};
use uuid::Uuid;

const STRESS_MOCK_CLAUDE_PY: &str = r#"#!/usr/bin/env python3
import json
import os
import sys
import time
import urllib.request

if "--version" in sys.argv[1:]:
    sys.stdout.write("2.1.0 (stress-mock-claude)\n")
    sys.stdout.flush()
    sys.exit(0)

def parse_args():
    args = sys.argv[1:]
    mcp_cfg = None
    session_id = None
    resume_id = None
    sys_prompt_append = ""
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--mcp-config" and i + 1 < len(args):
            with open(args[i + 1], "r", encoding="utf-8") as fh:
                mcp_cfg = json.load(fh)
            i += 2
        elif a == "--append-system-prompt" and i + 1 < len(args):
            sys_prompt_append = args[i + 1]
            i += 2
        elif a.startswith("--append-system-prompt="):
            sys_prompt_append = a.split("=", 1)[1]
            i += 1
        elif a == "--session-id" and i + 1 < len(args):
            session_id = args[i + 1]
            i += 2
        elif a.startswith("--session-id="):
            session_id = a.split("=", 1)[1]
            i += 1
        elif a == "--resume" and i + 1 < len(args):
            resume_id = args[i + 1]
            i += 2
        elif a.startswith("--resume="):
            resume_id = a.split("=", 1)[1]
            i += 1
        else:
            i += 1
    return mcp_cfg, resume_id or session_id or "stress-sess", resume_id is not None, sys_prompt_append

MCP_CFG, SESSION_ID, WAS_RESUMED, SYS_PROMPT_APPEND = parse_args()

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def extract_text(frame):
    content = frame.get("message", {}).get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text")
    return ""

turn_idx = 0
while True:
    line = sys.stdin.readline()
    if not line:
        break
    try:
        frame = json.loads(line)
    except Exception:
        continue
    ftype = frame.get("type")
    if ftype == "control_request":
        req_id = frame.get("request_id", "ctrl_1")
        sub = frame.get("request", {}).get("subtype", "")
        emit({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": req_id, "response": {"ok": True}}
        })
        if sub == "initialize":
            emit({"type": "system", "subtype": "init", "session_id": SESSION_ID, "model": "stress-claude"})
        continue
    if ftype != "user":
        continue

    turn_idx += 1
    text = extract_text(frame)

    if "[STRESS:SLEEP=" in text:
        seg = text.split("[STRESS:SLEEP=", 1)[1].split("]", 1)[0]
        time.sleep(float(seg))

    if "[STRESS:PERM_GATE]" in text:
        perm_id = f"perm_{os.getpid()}_{turn_idx}"
        emit({
            "type": "control_request",
            "request_id": perm_id,
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "input": {"command": "echo gated-stress-op"}
            }
        })
        while True:
            pline = sys.stdin.readline()
            if not pline:
                sys.exit(0)
            pframe = json.loads(pline)
            if pframe.get("type") == "control_response" and pframe.get("response", {}).get("request_id") == perm_id:
                break

    if "[STRESS:EDIT=" in text:
        edit_path = text.split("[STRESS:EDIT=", 1)[1].split("]", 1)[0]
        tu_id = f"tu_edit_{os.getpid()}_{turn_idx}"
        emit({
            "type": "assistant",
            "message": {
                "id": f"msg_tu_{turn_idx}",
                "role": "assistant",
                "content": [{
                    "type": "tool_use",
                    "id": tu_id,
                    "name": "Edit",
                    "input": {"file_path": edit_path, "old_string": "42", "new_string": "43"}
                }]
            }
        })
        emit({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tu_id,
                    "content": "File updated"
                }]
            }
        })

    has_constitution = (
        "dispatch_conversation" in SYS_PROMPT_APPEND
        and "reclaim_conversation" in SYS_PROMPT_APPEND
    )
    reply = (
        f"TURN={turn_idx} RESUMED={WAS_RESUMED} "
        f"HAS_ROUTER_SYS_PROMPT={has_constitution} "
        f"ACK={text[:120]}"
    )
    emit({
        "type": "assistant",
        "message": {
            "id": f"msg_{turn_idx}",
            "role": "assistant",
            "content": [{"type": "text", "text": reply}]
        }
    })
    emit({
        "type": "result",
        "subtype": "success",
        "session_id": SESSION_ID,
        "result": reply,
        "is_error": False,
        "num_turns": turn_idx
    })
"#;

const STRESS_MOCK_HERMES_PY: &str = r#"#!/usr/bin/env python3
import json
import sys

if "--version" in sys.argv[1:] or "version" in sys.argv[1:]:
    sys.stdout.write("Hermes Agent 0.9.0-stress\n")
    sys.stdout.flush()
    sys.exit(0)

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for raw in sys.stdin:
    raw = raw.strip()
    if not raw:
        continue
    try:
        msg = json.loads(raw)
    except Exception:
        continue
    method = msg.get("method")
    rid = msg.get("id")
    if method == "initialize":
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "protocolVersion": 1,
                "agentInfo": {"name": "Hermes", "version": "0.9.0-stress"},
                "agentCapabilities": {"loadSession": True}
            }
        })
    elif method in ("session/new", "session/load"):
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {
                "sessionId": "hermes_router_sess_1",
                "models": {"currentModelId": "hermes-router-v1"},
                "modes": {"availableModes": [{"id": "default"}, {"id": "accept-edits"}]}
            }
        })
    elif method == "session/prompt":
        params = msg.get("params") or {}
        blocks = params.get("prompt") or []
        full_text = "\n".join(
            b.get("text", "") for b in blocks if isinstance(b, dict)
        )
        has_sys = (
            "[System Instructions]" in full_text
            and "dispatch_conversation" in full_text
            and "reclaim_conversation" in full_text
        )
        has_remote_goal = "Epoch 5: Relay Stress Milestone" in full_text
        reply = f"HERMES_ACP_OK SYS_CONSTITUTION={has_sys} REMOTE_ATLAS_GOAL={has_remote_goal}"
        emit({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": "hermes_router_sess_1",
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": reply}
                }
            }
        })
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {"stopReason": "end_turn"}
        })
    elif rid is not None:
        emit({
            "jsonrpc": "2.0",
            "id": rid,
            "result": {}
        })
"#;

fn pick_free_addr() -> SocketAddr {
    // Allocate outside the OS ephemeral outbound port range (49152..65535) so concurrent
    // outbound HTTP/WS connections never steal the port during process spawn or hot-restart.
    let seed = (Uuid::new_v4().as_u128() % 18_000) as u16 + 24_000;
    for offset in 0..2_000u16 {
        let port = 24_000 + ((seed - 24_000 + offset) % 20_000);
        if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            return listener.local_addr().expect("local_addr");
        }
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local_addr")
}

fn resolve_orbit_binary() -> PathBuf {
    if let Ok(explicit) = std::env::var("ORBIT_STRESS_BIN") {
        let p = PathBuf::from(explicit);
        if p.is_file() {
            return p;
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let release_bin = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .map(|root| root.join("target").join("release").join("orbit"));
    if let Some(p) = release_bin
        && p.is_file()
    {
        return p;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_orbit"))
}

fn init_git_repo(dir: &Path, project_title: &str) {
    std::fs::create_dir_all(dir.join("src")).expect("mkdir src");
    std::fs::write(
        dir.join("AGENTS.md"),
        format!("# {project_title} Rules\nZero unsafe. PR-first.\n"),
    )
    .expect("write AGENTS.md");
    std::fs::write(
        dir.join("README.md"),
        format!("# {project_title}\nIsolated stress-test repository.\n"),
    )
    .expect("write README.md");
    std::fs::write(dir.join("src/lib.rs"), "pub fn compute() -> u64 { 42 }\n")
        .expect("write src/lib.rs");

    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "stress@orbit.test"],
        vec!["config", "user.name", "Orbit Stress"],
        vec!["add", "."],
        vec!["commit", "-m", "initial commit"],
    ] {
        let status = Command::new("git")
            .args(&args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }
}

struct IsolatedCluster {
    root: PathBuf,
    relay_child: Option<Child>,
    daemon_a: Option<Child>,
    daemon_b: Option<Child>,
    daemon_c: Option<Child>,
}

impl IsolatedCluster {
    fn kill_shims_under(orbit_home: &Path) {
        let shims_dir = orbit_home.join("shims");
        if let Ok(entries) = std::fs::read_dir(shims_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("pid")
                    && let Ok(raw) = std::fs::read_to_string(&path)
                {
                    if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                        for key in ["child_pgid", "child_pid", "shim_pid"] {
                            if let Some(pid) = val.get(key).and_then(Value::as_u64)
                                && pid > 1
                            {
                                let target = if key == "child_pgid" {
                                    format!("-{pid}")
                                } else {
                                    pid.to_string()
                                };
                                let _ = Command::new("kill")
                                    .args(["-9", &target])
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null())
                                    .status();
                            }
                        }
                    } else if let Ok(pid) = raw.trim().parse::<i32>()
                        && pid > 1
                    {
                        let _ = Command::new("kill")
                            .args(["-9", &format!("-{pid}"), &pid.to_string()])
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                }
            }
        }
    }
}

impl Drop for IsolatedCluster {
    fn drop(&mut self) {
        for ch in [
            &mut self.daemon_a,
            &mut self.daemon_b,
            &mut self.daemon_c,
            &mut self.relay_child,
        ]
        .into_iter()
        .flatten()
        {
            let _ = ch.kill();
            let _ = ch.wait();
        }
        for sub in ["orbit_a", "orbit_b", "orbit_c"] {
            Self::kill_shims_under(&self.root.join(sub));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

async fn wait_until<F, Fut>(timeout: Duration, mut check: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    false
}

async fn mcp_call(
    http: &reqwest::Client,
    addr: SocketAddr,
    mcp_token: &str,
    caller_agent_id: Option<&str>,
    tool_name: &str,
    arguments: Value,
) -> Value {
    let url = match caller_agent_id {
        Some(cid) => format!("http://{addr}/mcp/agents?caller_agent_id={cid}"),
        None => format!("http://{addr}/mcp/agents"),
    };
    let resp = http
        .post(&url)
        .header("Authorization", format!("Bearer {mcp_token}"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": Uuid::new_v4().to_string(),
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments,
            }
        }))
        .send()
        .await
        .expect("POST /mcp/agents");
    assert!(
        resp.status().is_success(),
        "MCP HTTP status {} for {tool_name}",
        resp.status()
    );
    let body: Value = resp.json().await.expect("parse MCP JSON");
    assert!(
        body.get("error").is_none(),
        "JSON-RPC protocol error for {tool_name}: {body}"
    );
    let result = body.get("result").cloned().expect("MCP result");
    let is_error = result
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(!is_error, "MCP tool `{tool_name}` returned error: {result}");
    result.get("structuredContent").cloned().unwrap_or_else(|| {
        result
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .and_then(|s| serde_json::from_str::<Value>(s).ok())
            .unwrap_or(result)
    })
}

fn spawn_daemon_proc(
    orbit_bin: &Path,
    sys_home: &Path,
    orbit_home: &Path,
    path_env: &str,
    addr: SocketAddr,
    relay_url: Option<&str>,
) -> Child {
    std::fs::create_dir_all(sys_home).expect("mkdir sys_home");
    std::fs::create_dir_all(orbit_home).expect("mkdir orbit_home");
    let mut cmd = Command::new(orbit_bin);
    cmd.arg("--home")
        .arg(orbit_home)
        .arg("daemon")
        .arg("--listen")
        .arg(addr.to_string());
    if let Some(r) = relay_url {
        cmd.arg("--relay").arg(r);
    }
    cmd.env("HOME", sys_home)
        .env("ORBIT_HOME", orbit_home)
        .env("PATH", path_env)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn orbit daemon process")
}

async fn wait_daemon_ready(orbit_home: &Path, addr: SocketAddr) -> String {
    let token_path = orbit_home.join("mcp-token");
    let ok = wait_until(Duration::from_secs(15), || async {
        token_path.is_file() && tokio::net::TcpStream::connect(addr).await.is_ok()
    })
    .await;
    assert!(ok, "daemon at {addr} ({orbit_home:?}) did not become ready");
    std::fs::read_to_string(&token_path)
        .expect("read mcp-token")
        .trim()
        .to_string()
}

fn run_orbit_cli_json(
    orbit_bin: &Path,
    sys_home: &Path,
    orbit_home: &Path,
    path_env: &str,
    extra_env: &[(&str, &str)],
    args: &[&str],
) -> Value {
    let mut cmd = Command::new(orbit_bin);
    cmd.arg("--home").arg(orbit_home).arg("--json").args(args);
    cmd.env("HOME", sys_home)
        .env("ORBIT_HOME", orbit_home)
        .env("PATH", path_env);
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("run orbit CLI");
    assert!(
        out.status.success(),
        "orbit {:?} failed:\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap_or_else(
        |_| json!({ "raw": String::from_utf8_lossy(&out.stdout).trim().to_string() }),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "high-pressure multi-process release binary cross-environment stress suite"]
async fn stress_release_multi_env_sovereignty_router_workspace_and_hot_restart() {
    let orbit_bin = resolve_orbit_binary();
    println!(
        "[STRESS] Using compiled orbit binary: {}",
        orbit_bin.display()
    );

    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let root = PathBuf::from("/tmp").join(format!("orbit_stress_{short_id}"));
    let mock_bin_dir = root.join("mock_bin");
    std::fs::create_dir_all(&mock_bin_dir).expect("mkdir mock_bin");
    let claude_script = mock_bin_dir.join("claude");
    std::fs::write(&claude_script, STRESS_MOCK_CLAUDE_PY).expect("write mock claude");
    let hermes_script = mock_bin_dir.join("hermes");
    std::fs::write(&hermes_script, STRESS_MOCK_HERMES_PY).expect("write mock hermes");
    let agy_script = mock_bin_dir.join("agy");
    std::fs::write(
        &agy_script,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \"agy 2.0.0-stress\"; exit 0; fi\necho '{\"handle\":\"stress-conv\",\"title\":\"Stress Conversation\",\"task_summary\":\"Verified isolated summary\"}'\n",
    )
    .expect("write mock agy");
    {
        use std::os::unix::fs::PermissionsExt;
        for s in [&claude_script, &hermes_script, &agy_script] {
            std::fs::set_permissions(s, std::fs::Permissions::from_mode(0o755))
                .expect("chmod +x mock script");
        }
    }
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let path_env = format!("{}:{orig_path}", mock_bin_dir.display());

    let mut cluster = IsolatedCluster {
        root: root.clone(),
        relay_child: None,
        daemon_a: None,
        daemon_b: None,
        daemon_c: None,
    };

    // 1. Initialize 3 isolated Git repositories across 3 isolated OS environments
    let repo_a = root.join("home_a/projects/core-gateway");
    let repo_b = root.join("home_b/projects/gpu-trainer");
    let repo_c = root.join("home_c/projects/edge-vault");
    init_git_repo(&repo_a, "Core Gateway (Env-A)");
    init_git_repo(&repo_b, "GPU Trainer (Env-B Direct)");
    init_git_repo(&repo_c, "Edge Vault (Env-C E2EE Relay)");

    // 2. Bootstrap host identities & pairing URIs via real CLI `orbit pair` first so the
    //    zero-knowledge `orbit relay` allowlists Host A and Host C's exact static public keys!
    let relay_addr = pick_free_addr();
    let relay_url = format!("ws://{relay_addr}");
    let relay_home = root.join("relay_home");
    std::fs::create_dir_all(&relay_home).expect("mkdir relay_home");

    let addr_a = pick_free_addr();
    let addr_b = pick_free_addr();
    let addr_c = pick_free_addr();
    let home_a = root.join("home_a");
    let home_b = root.join("home_b");
    let home_c = root.join("home_c");
    let orbit_a = root.join("orbit_a");
    let orbit_b = root.join("orbit_b");
    let orbit_c = root.join("orbit_c");
    for d in [&home_a, &home_b, &home_c, &orbit_a, &orbit_b, &orbit_c] {
        std::fs::create_dir_all(d).expect("mkdir env dir");
    }

    let pair_a = run_orbit_cli_json(
        &orbit_bin,
        &home_a,
        &orbit_a,
        &path_env,
        &[],
        &["pair", "--endpoint", &format!("ws://{addr_a}/ws")],
    );
    let pair_b = run_orbit_cli_json(
        &orbit_bin,
        &home_b,
        &orbit_b,
        &path_env,
        &[],
        &["pair", "--endpoint", &format!("ws://{addr_b}/ws")],
    );
    let pair_c_raw = run_orbit_cli_json(
        &orbit_bin,
        &home_c,
        &orbit_c,
        &path_env,
        &[],
        &[
            "pair",
            "--endpoint",
            "ws://127.0.0.1:1/ws",
            "--relay",
            &relay_url,
        ],
    );
    let uri_a = pair_a["pairing_uri"]
        .as_str()
        .or_else(|| pair_a["uri"].as_str())
        .expect("uri_a")
        .to_string();
    let uri_b = pair_b["pairing_uri"]
        .as_str()
        .or_else(|| pair_b["uri"].as_str())
        .expect("uri_b")
        .to_string();
    let uri_c = pair_c_raw["pairing_uri"]
        .as_str()
        .or_else(|| pair_c_raw["uri"].as_str())
        .expect("uri_c")
        .to_string();
    let offer_a = PairingOffer::from_qr_uri(&uri_a).expect("parse offer_a");
    let offer_c = PairingOffer::from_qr_uri(&uri_c).expect("parse offer_c");
    assert_eq!(offer_c.direct_endpoints, vec!["ws://127.0.0.1:1/ws"]);
    assert_eq!(offer_c.relay_url.as_deref(), Some(relay_url.as_str()));

    // 3. Start allowlisted standalone `orbit relay` + 3 standalone `orbit daemon` processes
    cluster.relay_child = Some(
        Command::new(&orbit_bin)
            .arg("--home")
            .arg(&relay_home)
            .arg("relay")
            .arg("--listen")
            .arg(relay_addr.to_string())
            .arg("--allow-pubkey")
            .arg(&offer_a.daemon_pubkey_b64)
            .arg("--allow-pubkey")
            .arg(&offer_c.daemon_pubkey_b64)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn orbit relay"),
    );
    assert!(
        wait_until(Duration::from_secs(10), || async {
            tokio::net::TcpStream::connect(relay_addr).await.is_ok()
        })
        .await,
        "orbit relay failed to listen on {relay_addr}"
    );

    cluster.daemon_a = Some(spawn_daemon_proc(
        &orbit_bin,
        &home_a,
        &orbit_a,
        &path_env,
        addr_a,
        Some(&relay_url),
    ));
    cluster.daemon_b = Some(spawn_daemon_proc(
        &orbit_bin, &home_b, &orbit_b, &path_env, addr_b, None,
    ));
    cluster.daemon_c = Some(spawn_daemon_proc(
        &orbit_bin,
        &home_c,
        &orbit_c,
        &path_env,
        addr_c,
        Some(&relay_url),
    ));

    let (tok_a, tok_b, tok_c) = tokio::join!(
        wait_daemon_ready(&orbit_a, addr_a),
        wait_daemon_ready(&orbit_b, addr_b),
        wait_daemon_ready(&orbit_c, addr_c),
    );
    println!("[STRESS] All 4 standalone OS processes online (Relay + Env-A + Env-B + Env-C)");

    // Verify `orbit doctor` in isolated Env-A sees isolated Orbit Home & mock Claude
    let doc_out = Command::new(&orbit_bin)
        .arg("--home")
        .arg(&orbit_a)
        .arg("doctor")
        .env("HOME", &home_a)
        .env("ORBIT_HOME", &orbit_a)
        .env("PATH", &path_env)
        .output()
        .expect("orbit doctor");
    assert!(doc_out.status.success(), "orbit doctor failed");
    let doc_text = String::from_utf8_lossy(&doc_out.stdout);
    assert!(
        doc_text.contains(&orbit_a.to_string_lossy().to_string()),
        "orbit doctor must report isolated ORBIT_HOME: {doc_text}"
    );

    // Wait briefly for Daemon C's outbound E2EE registration on `orbit relay`
    tokio::time::sleep(Duration::from_millis(500)).await;

    // 4. Connect `OrbitAppCore` simultaneously to Env-A (Direct), Env-B (Direct), and Env-C (RelayOnly!)
    let client_db = root.join("client_replica.sqlite");
    let core = tokio::task::spawn_blocking(move || {
        OrbitAppCore::open(&client_db, false).expect("open OrbitAppCore")
    })
    .await
    .expect("join open");

    let host_a_id = core.pair_host(&uri_a).expect("pair A").host_id;
    let host_b_id = core.pair_host(&uri_b).expect("pair B").host_id;
    let host_c_id = core.pair_host(&uri_c).expect("pair C").host_id;
    core.update_host_config(
        &host_c_id,
        Some("Cloud-WAN-Relay".into()),
        Some(HostRouteMode::RelayOnly),
        None,
        None,
        None,
        Some(relay_url.clone()),
        true,
    )
    .expect("force RelayOnly on Host C");

    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let (ha, hb, hc) = (host_a_id.clone(), host_b_id.clone(), host_c_id.clone());
            async move {
                let da = c
                    .host_detail(&ha)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineDirect);
                let db = c
                    .host_detail(&hb)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineDirect);
                let dc = c
                    .host_detail(&hc)
                    .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineRelay);
                da && db && dc
            }
        })
        .await,
        "OrbitAppCore failed to connect A(Direct), B(Direct), C(OnlineRelay)"
    );
    println!(
        "[STRESS] OrbitAppCore connected: A=OnlineDirect, B=OnlineDirect, C=OnlineRelay (E2EE)"
    );

    // 5. Activate PersonalRouter on Env-A + select as Primary Router -> verify automatic mesh sync!
    let router_snap = core
        .configure_personal_router_on_host(
            &host_a_id,
            PersonalRouterConfig {
                enabled: true,
                provider: ProviderKind::ClaudeCode,
                cwd: Some(repo_a.to_string_lossy().to_string()),
                ..Default::default()
            },
            None,
            true,
        )
        .await
        .expect("configure PersonalRouter on A");
    let router_agent = router_snap.router_agent.expect("router_agent created");
    let router_id = router_agent.agent_id.clone();
    assert_eq!(router_agent.session_tier(), SessionTier::PersonalRouter);

    core.set_primary_router_selection(Some(PrimaryRouterSelection {
        host_id: host_a_id.clone(),
        agent_id: Some(router_id.clone()),
        updated_at_ms: 0,
    }))
    .expect("set_primary_router_selection");
    let server_a = format!("ws://{addr_a}/ws");
    // Host C (Relay-backed) is synced automatically by `set_primary_router_selection` (which intentionally
    // strips `127.0.0.1` loopback direct endpoints); register loopback Direct Host B via real `orbit host add` CLI!
    run_orbit_cli_json(
        &orbit_bin,
        &home_a,
        &orbit_a,
        &path_env,
        &[("ORBIT_SERVER", server_a.as_str())],
        &["host", "add", &uri_b, "--label", "Linux-GPU-Direct"],
    );

    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let (ha, hb, hc) = (host_a_id.clone(), host_b_id.clone(), host_c_id.clone());
            async move {
                let Some(detail_a) = c.host_detail(&ha) else {
                    return false;
                };
                let b_online = detail_a.connected_hosts.iter().any(|peer| {
                    peer.host_id.as_deref() == Some(hb.as_str())
                        && peer.status == ConnectedHostStatus::Online
                });
                let c_online = detail_a.connected_hosts.iter().any(|peer| {
                    peer.host_id.as_deref() == Some(hc.as_str())
                        && peer.status == ConnectedHostStatus::Online
                });
                b_online && c_online
            }
        })
        .await,
        "Primary Router Host A must auto-connect to BOTH Host B (Direct) and Host C (E2EE Relay)"
    );
    println!(
        "[STRESS] Primary Router Mesh synced automatically: Host A <-> Host B (Direct) & Host C (Relay)"
    );

    // Verify persistent `--append-system-prompt` reached the real `claude` OS process via `orbit shim`!
    core.send_prompt(&host_a_id, &router_id, "ping-router-constitution");
    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let (ha, rid) = (host_a_id.clone(), router_id.clone());
            async move {
                let tl = c.agent_timeline(&ha, &rid);
                tl.items
                    .iter()
                    .any(|it| format!("{it:?}").contains("HAS_ROUTER_SYS_PROMPT=True"))
            }
        })
        .await,
        "PersonalRouter shim must inject persistent Router Constitution via --append-system-prompt"
    );
    println!(
        "[STRESS] Verified persistent `--append-system-prompt` on live PersonalRouter subprocess"
    );

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client");

    // 6. Create curated Mission Workspaces across all 3 environments + verify cross-env context inspection
    let ws_a = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "orbit_workspace",
        json!({
            "action": "create",
            "source_cwd": repo_a.to_string_lossy(),
            "title": "Gateway Routing",
            "goal": "Milestone 1: Zero-Trust Token Gate"
        }),
    )
    .await["workspace"]["workspace_id"]
        .as_str()
        .expect("ws_a")
        .to_string();

    let ws_b = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "orbit_workspace",
        json!({
            "action": "create",
            "host_id": host_b_id,
            "source_cwd": repo_b.to_string_lossy(),
            "title": "CUDA Kernel Pipeline",
            "goal": "Milestone 1: FP8 Tensor Core Fusion"
        }),
    )
    .await["workspace"]["workspace_id"]
        .as_str()
        .expect("ws_b")
        .to_string();

    let ws_c = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "orbit_workspace",
        json!({
            "action": "create",
            "host_id": host_c_id,
            "source_cwd": repo_c.to_string_lossy(),
            "title": "WAN Vault Sync",
            "goal": "Milestone 1: Noise NK Relay Framing"
        }),
    )
    .await["workspace"]["workspace_id"]
        .as_str()
        .expect("ws_c")
        .to_string();

    // Router inspects remote project context on Relay-isolated Env-C without SSH!
    let ctx_c = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "orbit_control",
        json!({
            "action": "inspect_project_context",
            "host_id": host_c_id,
            "cwd": repo_c.to_string_lossy()
        }),
    )
    .await;
    assert!(
        format!("{ctx_c}").contains("Edge Vault (Env-C E2EE Relay)"),
        "Router must inspect remote project context over Noise E2EE Relay: {ctx_c}"
    );

    // 7. HIGH-PRESSURE FAN-OUT: 12 concurrent Router `dispatch_conversation`s across A / B / C
    let mut dispatch_futs = Vec::new();
    for idx in 0..12usize {
        let http_c = http.clone();
        let tok_a_c = tok_a.clone();
        let rid_c = router_id.clone();
        let (target_host, cwd, ws_id, env_tag) = match idx % 3 {
            0 => (
                None,
                repo_a.to_string_lossy().to_string(),
                ws_a.clone(),
                "EnvA",
            ),
            1 => (
                Some(host_b_id.clone()),
                repo_b.to_string_lossy().to_string(),
                ws_b.clone(),
                "EnvB-Direct",
            ),
            _ => (
                Some(host_c_id.clone()),
                repo_c.to_string_lossy().to_string(),
                ws_c.clone(),
                "EnvC-Relay",
            ),
        };
        dispatch_futs.push(tokio::spawn(async move {
            let mut args = json!({
                "workspace_id": ws_id,
                "cwd": cwd,
                "provider": "claude_code",
                "mode": "accept_edits",
                "title": format!("Routed-{env_tag}-{idx}"),
                "initial_prompt": format!("routed-worker-{idx}-done"),
                "notify_on_finish": true
            });
            if let Some(h) = target_host.clone() {
                args["host_id"] = Value::String(h);
            }
            let res = mcp_call(
                &http_c,
                addr_a,
                &tok_a_c,
                Some(&rid_c),
                "dispatch_conversation",
                args,
            )
            .await;
            let aid = res["agent_id"]
                .as_str()
                .expect("routed agent_id")
                .to_string();
            (idx, target_host, aid)
        }));
    }

    let mut routed_convs: Vec<(String, String)> = Vec::new();
    for f in dispatch_futs {
        let (_idx, target_host, aid) = f.await.expect("join dispatch");
        let hid = target_host.unwrap_or_else(|| host_a_id.clone());
        routed_convs.push((hid, aid));
    }
    assert_eq!(routed_convs.len(), 12);
    println!(
        "[STRESS] Dispatched 12 concurrent primary Conversations across Local, Direct & E2EE Relay"
    );

    let primary_a = routed_convs
        .iter()
        .find(|(h, _)| h == &host_a_id)
        .expect("primary_a")
        .1
        .clone();
    let primary_b = routed_convs
        .iter()
        .find(|(h, _)| h == &host_b_id)
        .expect("primary_b")
        .1
        .clone();
    let primary_c = routed_convs
        .iter()
        .find(|(h, _)| h == &host_c_id)
        .expect("primary_c")
        .1
        .clone();

    // 8. SUBAGENT FLOOD: Spawn 24+ Subagents across EVERY possible vector (same-host, worktree peer,
    //    MCP fork, cross-host Direct, cross-host E2EE Relay, CLI `orbit run`, CLI `orbit fork`, and Permission Gates!)
    let mut subagent_futs = Vec::new();

    // 8a. 6 local + worktree subagents on A, B, C
    for i in 0..6usize {
        let http_c = http.clone();
        let (addr, tok, caller_id, cwd, use_wt) = match i % 3 {
            0 => (
                addr_a,
                tok_a.clone(),
                primary_a.clone(),
                repo_a.to_string_lossy().to_string(),
                i % 2 == 0,
            ),
            1 => (
                addr_b,
                tok_b.clone(),
                primary_b.clone(),
                repo_b.to_string_lossy().to_string(),
                i % 2 == 0,
            ),
            _ => (
                addr_c,
                tok_c.clone(),
                primary_c.clone(),
                repo_c.to_string_lossy().to_string(),
                i % 2 == 0,
            ),
        };
        subagent_futs.push(tokio::spawn(async move {
            let res = mcp_call(
                &http_c,
                addr,
                &tok,
                Some(&caller_id),
                "create_agent",
                json!({
                    "cwd": cwd,
                    "provider": "claude_code",
                    "mode": "accept_edits",
                    "worktree": use_wt,
                    "title": format!("LocalSub-{i}"),
                    "initial_prompt": format!("local-sub-{i}")
                }),
            )
            .await;
            res["agent_id"].as_str().expect("sub id").to_string()
        }));
    }

    // 8b. 6 `spawn_workspace_peer` helpers inside curated Workspaces on A, B, C
    for i in 0..6usize {
        let http_c = http.clone();
        let (addr, tok, caller_id, ws_id) = match i % 3 {
            0 => (addr_a, tok_a.clone(), primary_a.clone(), ws_a.clone()),
            1 => (addr_b, tok_b.clone(), primary_b.clone(), ws_b.clone()),
            _ => (addr_c, tok_c.clone(), primary_c.clone(), ws_c.clone()),
        };
        subagent_futs.push(tokio::spawn(async move {
            let res = mcp_call(
                &http_c,
                addr,
                &tok,
                Some(&caller_id),
                "spawn_workspace_peer",
                json!({
                    "workspace_id": ws_id,
                    "handle": format!("helper-{i}"),
                    "provider": "claude_code",
                    "mode": "accept_edits",
                    "worktree": i % 2 == 1,
                    "title": format!("PeerHelper-{i}"),
                    "initial_prompt": format!("peer-helper-{i}")
                }),
            )
            .await;
            res["agent_id"].as_str().expect("peer sub id").to_string()
        }));
    }

    // 8c. 6 Cross-Host Subagents (Env-A worker -> Env-B Direct & Env-C E2EE Relay!)
    for i in 0..6usize {
        let http_c = http.clone();
        let tok_a_c = tok_a.clone();
        let caller_a = primary_a.clone();
        let (remote_hid, remote_cwd, remote_ws) = if i % 2 == 0 {
            (
                host_b_id.clone(),
                repo_b.to_string_lossy().to_string(),
                ws_b.clone(),
            )
        } else {
            (
                host_c_id.clone(),
                repo_c.to_string_lossy().to_string(),
                ws_c.clone(),
            )
        };
        subagent_futs.push(tokio::spawn(async move {
            let res = mcp_call(
                &http_c,
                addr_a,
                &tok_a_c,
                Some(&caller_a),
                "create_agent",
                json!({
                    "host_id": remote_hid,
                    "workspace_id": remote_ws,
                    "cwd": remote_cwd,
                    "provider": "claude_code",
                    "mode": "accept_edits",
                    "title": format!("CrossHostSub-{i}"),
                    "initial_prompt": format!("cross-sub-{i}")
                }),
            )
            .await;
            res["agent_id"].as_str().expect("cross sub id").to_string()
        }));
    }

    // 8d. 2 MCP-forked subagents (1 local on A, 1 cross-host on C over E2EE Relay!)
    {
        let http_c = http.clone();
        let tok_a_c = tok_a.clone();
        let caller_a = primary_a.clone();
        subagent_futs.push(tokio::spawn(async move {
            let res = mcp_call(
                &http_c,
                addr_a,
                &tok_a_c,
                Some(&caller_a),
                "orbit_agent",
                json!({
                    "action": "fork",
                    "agent_id": caller_a,
                    "worktree": true,
                    "title": "LocalForkSub",
                    "initial_prompt": "fork-sub-local"
                }),
            )
            .await;
            res["agent"]["agent_id"]
                .as_str()
                .expect("local fork sub")
                .to_string()
        }));
    }
    {
        let http_c = http.clone();
        let tok_a_c = tok_a.clone();
        let caller_a = primary_a.clone();
        let hc = host_c_id.clone();
        let src_c = primary_c.clone();
        subagent_futs.push(tokio::spawn(async move {
            let res = mcp_call(
                &http_c,
                addr_a,
                &tok_a_c,
                Some(&caller_a),
                "orbit_agent",
                json!({
                    "action": "fork",
                    "host_id": hc,
                    "agent_id": src_c,
                    "worktree": true,
                    "title": "RelayForkSub",
                    "initial_prompt": "fork-sub-relay"
                }),
            )
            .await;
            res["agent"]["agent_id"]
                .as_str()
                .expect("relay fork sub")
                .to_string()
        }));
    }

    let mut all_subagent_ids: HashSet<String> = HashSet::new();
    for f in subagent_futs {
        all_subagent_ids.insert(f.await.expect("join subagent"));
    }

    // 8e. Real CLI `orbit run` AND CLI `orbit fork` invoked with `ORBIT_AGENT_ID` + `ORBIT_HOST_ID`!
    let cli_run_sub = run_orbit_cli_json(
        &orbit_bin,
        &home_a,
        &orbit_a,
        &path_env,
        &[
            ("ORBIT_SERVER", server_a.as_str()),
            ("ORBIT_AGENT_ID", primary_a.as_str()),
            ("ORBIT_HOST_ID", host_a_id.as_str()),
        ],
        &[
            "run",
            "--provider",
            "claude-code",
            "--mode",
            "accept-edits",
            "--cwd",
            &repo_a.to_string_lossy(),
            "--title",
            "CliRunSubagent",
            "--no-wait",
            "cli-run-subagent-ok",
        ],
    );
    let cli_run_sub_id = cli_run_sub["agent_id"]
        .as_str()
        .or_else(|| cli_run_sub["agent"]["agent_id"].as_str())
        .expect("cli_run_sub_id")
        .to_string();
    all_subagent_ids.insert(cli_run_sub_id.clone());

    let cli_fork_sub = run_orbit_cli_json(
        &orbit_bin,
        &home_a,
        &orbit_a,
        &path_env,
        &[
            ("ORBIT_SERVER", server_a.as_str()),
            ("ORBIT_AGENT_ID", primary_a.as_str()),
            ("ORBIT_HOST_ID", host_a_id.as_str()),
        ],
        &[
            "fork",
            &primary_a,
            "--worktree",
            "--title",
            "CliForkSubagent",
            "--prompt",
            "cli-fork-subagent-ok",
        ],
    );
    let cli_fork_sub_id = cli_fork_sub["agent"]["agent_id"]
        .as_str()
        .expect("cli_fork_sub_id")
        .to_string();
    all_subagent_ids.insert(cli_fork_sub_id.clone());

    // 8f. Cross-host Relay Subagent hitting a live Permission Gate (`[STRESS:PERM_GATE]`)!
    let perm_sub_res = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&primary_a),
        "create_agent",
        json!({
            "host_id": host_c_id,
            "workspace_id": ws_c,
            "cwd": repo_c.to_string_lossy(),
            "provider": "claude_code",
            "mode": "default",
            "title": "RelayGatedSubagent",
            "initial_prompt": "[STRESS:PERM_GATE] need-human-approval"
        }),
    )
    .await;
    let perm_sub_id = perm_sub_res["agent_id"]
        .as_str()
        .expect("perm_sub_id")
        .to_string();
    all_subagent_ids.insert(perm_sub_id.clone());

    println!(
        "[STRESS] Spawned {} Subagents across all local/worktree/peer/fork/CLI/Direct/Relay vectors",
        all_subagent_ids.len()
    );

    // Wait for `perm_sub_id` on Relay Host C to enter `AwaitingPermission` AND bubble onto `primary_a` in `OrbitAppCore::fleet_sidebar`!
    assert!(
        wait_until(Duration::from_secs(20), || {
            let c = Arc::clone(&core);
            let root_parent = primary_a.clone();
            let ha = host_a_id.clone();
            async move {
                let sb = c.fleet_sidebar(
                    SidebarProjectSort::Recent,
                    SidebarThreadFilter::All,
                    "",
                    None,
                    None,
                    None,
                );
                sb.needs_attention
                    .iter()
                    .any(|item| item.host_id == ha && item.agent.agent_id == root_parent)
            }
        })
        .await,
        "Relay-isolated cross-host Subagent permission gate on Host C MUST bubble onto root Conversation `primary_a` on Host A in Needs Attention!"
    );
    println!(
        "[STRESS] Verified E2EE Relay Subagent permission gate bubbled onto root Conversation on Host A!"
    );

    // 9. STRICT SIDEBAR SOVEREIGNTY AUDIT across all 3 connected hosts!
    let sb = core.fleet_sidebar(
        SidebarProjectSort::Recent,
        SidebarThreadFilter::All,
        "",
        None,
        None,
        None,
    );
    let mut sidebar_ids: HashSet<String> = HashSet::new();
    for item in &sb.needs_attention {
        sidebar_ids.insert(item.agent.agent_id.clone());
    }
    for item in &sb.pinned_conversations {
        sidebar_ids.insert(item.agent.agent_id.clone());
    }
    for proj in &sb.projects {
        for conv in &proj.conversations {
            sidebar_ids.insert(conv.agent.agent_id.clone());
        }
        for ws in &proj.workspaces {
            for conv in &ws.conversations {
                sidebar_ids.insert(conv.agent.agent_id.clone());
            }
        }
        for conv in &proj.unclassified_conversations {
            sidebar_ids.insert(conv.agent.agent_id.clone());
        }
    }

    let leaked: Vec<&String> = all_subagent_ids
        .iter()
        .filter(|sid| sidebar_ids.contains(*sid))
        .collect();
    assert!(
        leaked.is_empty(),
        "ZERO subagents may ever appear in Left Sidebar, but found leaked IDs: {leaked:?}"
    );
    for (_, rid) in &routed_convs {
        assert!(
            sidebar_ids.contains(rid),
            "Router-dispatched primary Conversation `{rid}` MUST be visible in Left Sidebar"
        );
    }
    let pa_briefing = core.personal_agent_briefing(Some(host_a_id.clone()));
    assert_eq!(
        pa_briefing
            .personal_agent
            .as_ref()
            .map(|pa| pa.agent_id.as_str()),
        Some(router_id.as_str()),
        "PersonalRouter must be resolved as the canonical Personal Agent top-level entry"
    );
    assert!(
        pa_briefing.missions.len() >= 12,
        "Personal Agent Briefing must track all 12 Router-dispatched Conversations across Local/Direct/Relay, got {}",
        pa_briefing.missions.len()
    );
    println!(
        "[STRESS] Sidebar & Briefing Sovereignty 100% PASS: 0/{} subagents leaked; 12/12 routed Conversations + Personal Agent verified!",
        all_subagent_ids.len()
    );

    // Approve the gated Relay subagent via `OrbitAppCore` / MCP and verify `needs_attention` clears!
    let perms_c = mcp_call(
        &http,
        addr_c,
        &tok_c,
        None,
        "orbit_control",
        json!({ "action": "list_pending_permissions", "agent_id": perm_sub_id }),
    )
    .await;
    let perm_req_id = perms_c["permissions"][0]["request_id"]
        .as_str()
        .expect("perm request_id")
        .to_string();
    mcp_call(
        &http,
        addr_c,
        &tok_c,
        None,
        "orbit_control",
        json!({
            "action": "respond_permission",
            "agent_id": perm_sub_id,
            "request_id": perm_req_id,
            "decision": "allow"
        }),
    )
    .await;

    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            async move {
                c.fleet_sidebar(
                    SidebarProjectSort::Recent,
                    SidebarThreadFilter::All,
                    "",
                    None,
                    None,
                    None,
                )
                .needs_attention
                .is_empty()
            }
        })
        .await,
        "Needs Attention must clear once gated Relay subagent permission is resolved"
    );

    // 10. Explicit `detach` promotes ONLY the detached subagent; archiving a parent NEVER promotes worktree/remote subagents!
    core.detach_agent(&host_a_id, &cli_run_sub_id);
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let target = cli_run_sub_id.clone();
            async move {
                c.fleet_sidebar(
                    SidebarProjectSort::Recent,
                    SidebarThreadFilter::All,
                    "",
                    None,
                    None,
                    None,
                )
                .projects
                .iter()
                .flat_map(|p| p.conversations.iter())
                .any(|conv| conv.agent.agent_id == target)
            }
        })
        .await,
        "Explicitly human-detached subagent must promote into a visible Conversation"
    );

    // Archive `primary_a` (which owns worktree, CLI-forked, and cross-host subagents) and verify none of its other subagents surface!
    core.archive_agent(&host_a_id, &primary_a);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let sb_after_archive = core.fleet_sidebar(
        SidebarProjectSort::Recent,
        SidebarThreadFilter::All,
        "",
        None,
        None,
        None,
    );
    for proj in &sb_after_archive.projects {
        for conv in &proj.conversations {
            if conv.agent.agent_id != cli_run_sub_id {
                assert!(
                    !all_subagent_ids.contains(&conv.agent.agent_id),
                    "Archiving parent leaked orphaned subagent `{}` into Sidebar!",
                    conv.agent.agent_id
                );
            }
        }
    }
    println!("[STRESS] Parent archive & explicit Detach sovereignty verified!");

    // 11. 24-WAY CONCURRENT WHITEBOARD CAS + GOAL SYNC STORM + 5-EPOCH `reset_board` ROLLOVER
    let mut board_storm = Vec::new();
    for i in 0..24usize {
        let http_c = http.clone();
        let tok_c_clone = tok_c.clone();
        let wsc = ws_c.clone();
        board_storm.push(tokio::spawn(async move {
            if i % 4 == 0 {
                mcp_call(
                    &http_c,
                    addr_c,
                    &tok_c_clone,
                    None,
                    "orbit_workspace",
                    json!({
                        "action": "update_goal",
                        "workspace_id": wsc,
                        "goal": format!("Concurrent Goal Update #{i}")
                    }),
                )
                .await
            } else {
                let sec = match i % 3 {
                    0 => "Decisions & Contracts",
                    1 => "Tasks",
                    _ => "Findings",
                };
                mcp_call(
                    &http_c,
                    addr_c,
                    &tok_c_clone,
                    None,
                    "workspace_board",
                    json!({
                        "action": "update",
                        "workspace_id": wsc,
                        "section": sec,
                        "content": format!("- Concurrent entry #{i} verified under storm")
                    }),
                )
                .await
            }
        }));
    }
    for f in board_storm {
        f.await.expect("board storm task");
    }

    // Run 5 sequential Milestone Rollovers (`reset_board`) across Host C (via MCP & via OrbitAppCore over E2EE Relay!)
    for epoch in 1..=5usize {
        let next_goal = format!("Epoch {epoch}: Relay Stress Milestone");
        let standing = format!("- Invariant from epoch {epoch}: strict zero-knowledge E2EE");
        if epoch % 2 == 1 {
            mcp_call(
                &http,
                addr_a,
                &tok_a,
                Some(&router_id),
                "orbit_workspace",
                json!({
                    "action": "reset_board",
                    "host_id": host_c_id,
                    "workspace_id": ws_c,
                    "goal": next_goal,
                    "keep_notes": standing
                }),
            )
            .await;
        } else {
            core.reset_workspace_board(
                &host_c_id,
                &ws_c,
                Some(next_goal.clone()),
                Some(standing.clone()),
            );
            assert!(
                wait_until(Duration::from_secs(10), || {
                    let c = Arc::clone(&core);
                    let hc = host_c_id.clone();
                    let wsc = ws_c.clone();
                    let expected = next_goal.clone();
                    async move {
                        c.host_detail(&hc).is_some_and(|d| {
                            d.workspace_boards
                                .get(&wsc)
                                .is_some_and(|b| b.content.contains(&expected))
                        })
                    }
                })
                .await,
                "OrbitAppCore reset_workspace_board over E2EE Relay must round-trip WorkspaceBoardSnapshot"
            );
        }
        // Add a milestone note so the next reset archives substantive content to `BOARD.history.md`
        mcp_call(
            &http,
            addr_c,
            &tok_c,
            None,
            "workspace_board",
            json!({
                "action": "update",
                "workspace_id": ws_c,
                "section": "Findings",
                "content": format!("- Completed epoch {epoch} verification")
            }),
        )
        .await;
    }

    let history_md = std::fs::read_to_string(
        orbit_c
            .join("workspaces")
            .join(&ws_c)
            .join("BOARD.history.md"),
    )
    .expect("BOARD.history.md must exist on Env-C disk");
    for ep in 1..=4usize {
        assert!(
            history_md.contains(&format!("Completed epoch {ep} verification")),
            "BOARD.history.md must preserve epoch {ep} notes: {history_md}"
        );
    }
    // Verify Env-C sidebar Conversations inside `ws_c` were NOT archived by 5 board resets!
    let c_convs_alive = routed_convs
        .iter()
        .filter(|(h, _)| h == &host_c_id)
        .all(|(_, aid)| {
            core.host_detail(&host_c_id).is_some_and(|d| {
                d.agents
                    .iter()
                    .any(|a| a.agent_id == *aid && a.archived_at_rfc3339.is_none())
            })
        });
    assert!(
        c_convs_alive,
        "Sidebar Conversations in Workspace C must stay 100% untouched across 5 board resets"
    );
    println!("[STRESS] 24-way CAS storm + 5-epoch `reset_board` + `BOARD.history.md` verified!");

    // 12. Manual `reclaim_conversation` / `harvest_conversation` across Direct & E2EE Relay
    //     (a) Attempting to harvest PersonalRouter itself MUST be rejected
    let bad_reclaim = http
        .post(format!("http://{addr_a}/mcp/agents"))
        .header("Authorization", format!("Bearer {tok_a}"))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": "bad_harvest",
            "method": "tools/call",
            "params": {
                "name": "reclaim_conversation",
                "arguments": { "agent_id": router_id }
            }
        }))
        .send()
        .await
        .expect("bad reclaim http")
        .json::<Value>()
        .await
        .expect("bad reclaim json");
    assert_eq!(
        bad_reclaim
            .pointer("/result/isError")
            .and_then(Value::as_bool),
        Some(true),
        "Harvesting the canonical PersonalRouter must be rejected!"
    );

    //     (b) Manually reclaim `primary_b` via `OrbitAppCore::harvest_conversation` and `primary_c` via Router MCP over E2EE Relay!
    core.harvest_conversation(
        &host_b_id,
        &primary_b,
        Some("Distilled CUDA FP8 fusion benchmark (+38% throughput)".into()),
        true,
    );
    mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "reclaim_conversation",
        json!({
            "host_id": host_c_id,
            "agent_id": primary_c,
            "summary_note": "Distilled WAN E2EE framing validation",
            "append_to_workspace_board": true
        }),
    )
    .await;

    assert!(
        wait_until(Duration::from_secs(15), || {
            let c = Arc::clone(&core);
            let (hb, pb, hc, pc) = (
                host_b_id.clone(),
                primary_b.clone(),
                host_c_id.clone(),
                primary_c.clone(),
            );
            async move {
                let b_arch = c.host_detail(&hb).is_some_and(|d| {
                    d.agents
                        .iter()
                        .any(|a| a.agent_id == pb && a.archived_at_rfc3339.is_some())
                });
                let c_arch = c.host_detail(&hc).is_some_and(|d| {
                    d.agents
                        .iter()
                        .any(|a| a.agent_id == pc && a.archived_at_rfc3339.is_some())
                });
                b_arch && c_arch
            }
        })
        .await,
        "Manual harvest_conversation & cross-relay reclaim_conversation must archive ONLY the targeted threads"
    );
    let board_b_md =
        std::fs::read_to_string(orbit_b.join("workspaces").join(&ws_b).join("BOARD.md"))
            .expect("read Env-B BOARD.md");
    assert!(
        board_b_md.contains("Distilled CUDA FP8 fusion benchmark"),
        "Harvested takeaway must append to Workspace B BOARD.md: {board_b_md}"
    );
    println!("[STRESS] Manual Reclaim across Direct & E2EE Relay verified!");

    // 13. DAEMON HOT-RESTART / SHIM ISOLATION SURVIVAL (`AGENTS.md` Rule #2)
    //     Launch a mid-turn 3.0s job on Env-B, kill `daemon_b` mid-flight, relaunch `daemon_b` on the
    //     same port/ORBIT_HOME, and verify the detached `orbit shim` survives & finishes cleanly!
    let survivor_res = mcp_call(
        &http,
        addr_a,
        &tok_a,
        Some(&router_id),
        "dispatch_conversation",
        json!({
            "host_id": host_b_id,
            "workspace_id": ws_b,
            "cwd": repo_b.to_string_lossy(),
            "provider": "claude_code",
            "mode": "accept_edits",
            "title": "HotRestartSurvivor",
            "initial_prompt": "[STRESS:SLEEP=3.5] survived-daemon-restart-ok",
            "notify_on_finish": true
        }),
    )
    .await;
    let survivor_id = survivor_res["agent_id"]
        .as_str()
        .expect("survivor_id")
        .to_string();
    assert!(
        wait_until(Duration::from_secs(10), || {
            let http_c = http.clone();
            let tok_c = tok_b.clone();
            let sid = survivor_id.clone();
            async move {
                let st = mcp_call(
                    &http_c,
                    addr_b,
                    &tok_c,
                    None,
                    "orbit_agent",
                    json!({ "action": "status", "agent_id": sid }),
                )
                .await;
                let s = format!("{st}");
                s.contains("\"running\"")
            }
        })
        .await,
        "HotRestartSurvivor shim must enter `running` state before killing Daemon B"
    );
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Kill Daemon B while `survivor_id`'s `orbit shim` is actively sleeping mid-turn!
    if let Some(mut db_proc) = cluster.daemon_b.take() {
        let _ = db_proc.kill();
        let _ = db_proc.wait();
    }
    println!("[STRESS] Killed Daemon B mid-turn; relaunching Daemon B on {addr_b}...");
    cluster.daemon_b = Some(spawn_daemon_proc(
        &orbit_bin, &home_b, &orbit_b, &path_env, addr_b, None,
    ));
    let tok_b_restarted = wait_daemon_ready(&orbit_b, addr_b).await;

    // Wait for `survivor_id` shim to complete its turn and transition back to `Idle` on the restarted Daemon B!
    assert!(
        wait_until(Duration::from_secs(20), || {
            let http_c = http.clone();
            let tok_c = tok_b_restarted.clone();
            let sid = survivor_id.clone();
            async move {
                let st = mcp_call(
                    &http_c,
                    addr_b,
                    &tok_c,
                    None,
                    "orbit_agent",
                    json!({ "action": "status", "agent_id": sid }),
                )
                .await;
                st["agent"]["status"] == json!("idle")
                    || st["status"] == json!("idle")
                    || format!("{st}").contains("\"idle\"")
            }
        })
        .await,
        "Surviving `orbit shim` on Env-B must reattach to restarted Daemon B and finish cleanly!"
    );

    // Re-verify zero subagent leakage after Daemon B hot-restart + SQLite reload!
    core.connect_host_async(host_b_id.clone());
    tokio::time::sleep(Duration::from_millis(600)).await;
    let sb_final = core.fleet_sidebar(
        SidebarProjectSort::Recent,
        SidebarThreadFilter::All,
        "",
        None,
        None,
        None,
    );
    for proj in &sb_final.projects {
        for conv in &proj.conversations {
            if conv.agent.agent_id != cli_run_sub_id {
                assert!(
                    !all_subagent_ids.contains(&conv.agent.agent_id),
                    "Post-restart Sidebar leaked subagent `{}`!",
                    conv.agent.agent_id
                );
            }
        }
    }

    // 14. HERMES ACP (`ProviderKind::Acp` / `hermes`) PERSONAL ROUTER ENGINE SWITCH + FLEET ATLAS CHECK
    //     Ensure Host A is reconnected to restarted Host B + Relay Host C, switch Router engine to
    //     Hermes ACP, and verify BOTH `[System Instructions]` Constitution AND remote Relay Workspace Goal
    //     arrive inside Hermes ACP's `session/prompt`!
    assert!(
        wait_until(Duration::from_secs(15), || {
            let http_c = http.clone();
            let tok_c = tok_a.clone();
            let hc = host_c_id.clone();
            async move {
                let hosts = mcp_call(
                    &http_c,
                    addr_a,
                    &tok_c,
                    None,
                    "orbit_control",
                    json!({ "action": "list_hosts" }),
                )
                .await;
                format!("{hosts}").contains(&hc) && format!("{hosts}").contains("\"online\"")
            }
        })
        .await,
        "Host A must maintain Online connection to Relay Host C"
    );

    let hermes_snap = core
        .configure_personal_router_on_host(
            &host_a_id,
            PersonalRouterConfig {
                enabled: true,
                provider: ProviderKind::Acp,
                acp_agent_id: Some("hermes".into()),
                cwd: Some(repo_a.to_string_lossy().to_string()),
                ..Default::default()
            },
            None,
            true,
        )
        .await
        .expect("switch PersonalRouter on Host A to Hermes ACP");
    let hermes_router = hermes_snap.router_agent.expect("hermes router created");
    let hermes_router_id = hermes_router.agent_id.clone();
    assert_ne!(
        hermes_router_id, router_id,
        "Switching engine to Hermes ACP must retire old Claude router and spawn canonical Hermes router"
    );
    assert_eq!(hermes_router.provider, ProviderKind::Acp);
    assert_eq!(hermes_router.session_tier(), SessionTier::PersonalRouter);

    core.send_prompt(&host_a_id, &hermes_router_id, "ping-hermes-atlas");
    let hermes_ok = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let (ha, hrid) = (host_a_id.clone(), hermes_router_id.clone());
        async move {
            let tl = c.agent_timeline(&ha, &hrid);
            tl.items.iter().any(|it| {
                let dbg = format!("{it:?}");
                dbg.contains("HERMES_ACP_OK")
                    && dbg.contains("SYS_CONSTITUTION=True")
                    && dbg.contains("REMOTE_ATLAS_GOAL=True")
            })
        }
    })
    .await;
    assert!(
        hermes_ok,
        "Hermes ACP PersonalRouter must receive BOTH persistent [System Instructions] Constitution AND remote E2EE Relay Workspace Goal in <orbit-fleet-state>! timeline={:?}",
        core.agent_timeline(&host_a_id, &hermes_router_id).items
    );
    println!(
        "[STRESS] Hermes ACP PersonalRouter verified: SYS_CONSTITUTION=True & REMOTE_ATLAS_GOAL=True!"
    );

    // 15. `board_reset_seq` FILE-CONFLICT WATERMARK ISOLATION ACROSS `reset_board`
    let editor_1 = mcp_call(
        &http,
        addr_b,
        &tok_b_restarted,
        None,
        "create_agent",
        json!({
            "workspace_id": ws_b,
            "cwd": repo_b.to_string_lossy(),
            "provider": "claude_code",
            "mode": "accept_edits",
            "title": "Epoch1EditorA",
            "initial_prompt": "[STRESS:EDIT=src/lib.rs] edit-1"
        }),
    )
    .await["agent_id"]
        .as_str()
        .expect("editor_1")
        .to_string();
    let editor_2 = mcp_call(
        &http,
        addr_b,
        &tok_b_restarted,
        None,
        "create_agent",
        json!({
            "workspace_id": ws_b,
            "cwd": repo_b.to_string_lossy(),
            "provider": "claude_code",
            "mode": "accept_edits",
            "title": "Epoch1EditorB",
            "initial_prompt": "[STRESS:EDIT=src/lib.rs] edit-2"
        }),
    )
    .await["agent_id"]
        .as_str()
        .expect("editor_2")
        .to_string();

    assert!(
        wait_until(Duration::from_secs(15), || {
            let http_c = http.clone();
            let tok_c = tok_b_restarted.clone();
            let wsb = ws_b.clone();
            async move {
                let peers = mcp_call(
                    &http_c,
                    addr_b,
                    &tok_c,
                    None,
                    "workspace_peers",
                    json!({ "workspace_id": wsb }),
                )
                .await;
                let s = format!("{peers}");
                s.contains("src/lib.rs") && s.contains("conflict_warning")
            }
        })
        .await,
        "Overlapping file edits by `{editor_1}` and `{editor_2}` before `reset_board` must record `src/lib.rs` & `conflict_warning`"
    );

    // Reset Workspace B's board -> advances `board_reset_seq` watermark cleanly!
    mcp_call(
        &http,
        addr_b,
        &tok_b_restarted,
        None,
        "orbit_workspace",
        json!({
            "action": "reset_board",
            "workspace_id": ws_b,
            "goal": "Milestone 2: Clean Conflict Watermark"
        }),
    )
    .await;

    let peers_after_reset = mcp_call(
        &http,
        addr_b,
        &tok_b_restarted,
        None,
        "workspace_peers",
        json!({ "workspace_id": ws_b }),
    )
    .await;
    assert!(
        peers_after_reset["workspace_edited_files"]
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "workspace_edited_files must be empty immediately after reset_board watermark advance: {peers_after_reset}"
    );
    assert!(
        !format!("{}", peers_after_reset["recent_events"]).contains("conflict_warning"),
        "Pre-reset conflict_warning noise must be fenced off after reset_board: {peers_after_reset}"
    );
    assert!(
        format!("{}", peers_after_reset["recent_events"]).contains("Reset Shared Board"),
        "Post-reset recent_events must retain the Reset Shared Board milestone event: {peers_after_reset}"
    );
    // Also verify OrbitAppCore Left Sidebar `has_conflict` badge is immediately cleared on Workspace B!
    assert!(
        wait_until(Duration::from_secs(10), || {
            let c = Arc::clone(&core);
            let wsb = ws_b.clone();
            async move {
                let sb = c.fleet_sidebar(
                    SidebarProjectSort::Recent,
                    SidebarThreadFilter::All,
                    "",
                    None,
                    None,
                    None,
                );
                sb.projects
                    .iter()
                    .flat_map(|p| p.workspaces.iter())
                    .find(|w| w.workspace_id == wsb)
                    .is_some_and(|w| {
                        !w.has_conflict
                            && w.mission_goal.as_deref()
                                == Some("Milestone 2: Clean Conflict Watermark")
                    })
            }
        })
        .await,
        "OrbitAppCore Left Sidebar workspace card must clear has_conflict and sync new Goal immediately after reset_board!"
    );
    println!(
        "[STRESS] `board_reset_seq` file-conflict, prompt-injection & sidebar watermark isolation verified!"
    );

    println!("[STRESS] ALL 15 HIGH-PRESSURE CROSS-ENVIRONMENT & HOT-RESTART PHASES PASSED 100%!");
}
