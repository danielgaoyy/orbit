#![cfg(unix)]
//! Comprehensive End-to-End Integration Tests for Orbit v0.6.0:
//! 1. Non-Git Worktree Guard + Nested Git Repo Discovery & Sidebar Worktree Folding
//! 2. Dual-Client Real-Time Multi-Device Sync (Read/Unread, ManualUnread, Pin, Rename, Projects, Manual Reorder, Offline Queue)
//! 3. Mission Workspace Collaboration (`<orbit-workspace>` roster XML, `BOARD.md`, MCP Peer Tools, `@all` Broadcast, File Conflict Radar)
//! 4. Two-Stage Auto-Clustering (High-Confidence Auto-Group + Reversible Undo, Medium-Confidence Suggestion, Merge, Unrelated Isolation)

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use orbit_client_core::app_core::{CreateAgentRequest, OrbitAppCore};
use orbit_client_core::connect_direct;
use orbit_client_core::contract::SpawnWorkspacePeerRequest;
use orbit_client_core::sidebar::{SidebarProjectSort, SidebarThreadFilter};
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{
    AttentionState, ProviderKind, SessionMode, WireMessage, WorkspaceEventKind, WorktreeCreateMode,
};
use uuid::Uuid;

static MOCK_CLAUDE_WS_PATH: OnceLock<PathBuf> = OnceLock::new();

const MOCK_CLAUDE_WS_SCRIPT: &str = r#"#!/usr/bin/env python3
import json
import os
import sys
import time
import urllib.request

if "--version" in sys.argv[1:]:
    sys.stdout.write("2.1.0 (mock-claude-ws)\n")
    sys.stdout.flush()
    sys.exit(0)

def parse_args():
    args = sys.argv[1:]
    mcp_cfg = None
    session_id = None
    resume_id = None
    i = 0
    while i < len(args):
        if args[i] == "--mcp-config" and i + 1 < len(args):
            raw_mcp = args[i + 1]
            if not os.path.isfile(raw_mcp):
                sys.stderr.write(f"expected --mcp-config to be a readable file path, got: {raw_mcp}\n")
                sys.exit(2)
            with open(raw_mcp, "r", encoding="utf-8") as fh:
                mcp_cfg = json.load(fh)
            i += 2
        elif args[i].startswith("--session-id="):
            session_id = args[i].split("=", 1)[1]
            i += 1
        elif args[i] == "--session-id" and i + 1 < len(args):
            session_id = args[i + 1]
            i += 2
        elif args[i].startswith("--resume="):
            resume_id = args[i].split("=", 1)[1]
            i += 1
        elif args[i] == "--resume" and i + 1 < len(args):
            resume_id = args[i + 1]
            i += 2
        else:
            i += 1
    return mcp_cfg, resume_id or session_id or "mock-ws-session"

MCP_CFG, SESSION_ID = parse_args()

def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def mcp_call(tool_name, arguments):
    srv = (MCP_CFG or {}).get("mcpServers", {}).get("orbit", {})
    url = srv.get("url") or os.environ.get("ORBIT_MCP_URL")
    headers = dict(srv.get("headers") or {})
    if "Authorization" not in headers and os.environ.get("ORBIT_MCP_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["ORBIT_MCP_TOKEN"]
    headers["Content-Type"] = "application/json"
    body = json.dumps({
        "jsonrpc": "2.0",
        "id": int(time.time() * 1000) % 1000000,
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments}
    }).encode("utf-8")
    req = urllib.request.Request(url, data=body, headers=headers, method="POST")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))

def extract_text(frame):
    content = frame.get("message", {}).get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(
            b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text"
        )
    return ""

def parse_kv(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k.strip()] = v.strip()
    return out

turn_counter = 0

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
            emit({"type": "system", "subtype": "init", "session_id": SESSION_ID, "model": "mock-claude-ws"})
        continue

    if ftype != "user":
        continue

    turn_counter += 1
    text = extract_text(frame)
    kv = parse_kv(text)
    parts = []

    if "[MOCK:CHECK_WS_XML]" in text:
        has_xml = "<orbit-workspace" in text and "</orbit-workspace>" in text
        parts.append(f"HAS_WS_XML={has_xml}")

    if "[MOCK:BOARD_WRITE]" in text:
        sec = kv.get("SECTION", "Spec")
        body = kv.get("BODY", "Initial architecture decisions recorded.").replace("_", " ")
        res = mcp_call("workspace_board_update", {"section": sec, "content": body})
        sc = res.get("result", {}).get("structuredContent", {})
        rev = sc.get("board", {}).get("revision", -1)
        parts.append(f"BOARD_WROTE_REV={rev}")

    if "[MOCK:PEER_COLLAB]" in text:
        peers_res = mcp_call("workspace_peers", {})
        peers_sc = peers_res.get("result", {}).get("structuredContent", {})
        peer_handles = [p.get("handle") for p in peers_sc.get("peers", [])]

        board_res = mcp_call("workspace_board_read", {})
        board_sc = board_res.get("result", {}).get("structuredContent", {})
        board_txt = board_sc.get("board", {}).get("content", "")

        target = kv.get("TARGET", "lead")
        ctx_res = mcp_call("read_peer_context", {"handle": target, "limit": 10})
        ctx_ok = not ctx_res.get("result", {}).get("isError", True)

        msg_res = mcp_call("message_peer", {
            "handle": target,
            "message": "PEER_PING_FROM_WORKER: board verified",
            "notify_on_finish": False
        })
        msg_ok = not msg_res.get("result", {}).get("isError", True)
        parts.append(
            f"PEERS={','.join(sorted(str(h) for h in peer_handles))} "
            f"BOARD_LEN={len(board_txt)} CTX_OK={ctx_ok} MSG_OK={msg_ok}"
        )

    if "[MOCK:EDIT_FILE]" in text:
        fpath = kv.get("FILE", "src/shared.rs")
        tool_id = f"tu_edit_{turn_counter}_{os.getpid()}"
        emit({
            "type": "assistant",
            "message": {
                "id": f"msg_tool_{turn_counter}",
                "content": [{
                    "type": "tool_use",
                    "id": tool_id,
                    "name": "Edit",
                    "input": {
                        "file_path": fpath,
                        "old_string": "a",
                        "new_string": "b"
                    }
                }]
            }
        })
        emit({
            "type": "user",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": tool_id,
                    "content": "File edited successfully",
                    "is_error": False
                }]
            }
        })
        parts.append(f"EDITED={fpath}")

    if not parts:
        parts.append(f"ACK_TURN={turn_counter} ECHO={text[:140]}")

    reply = " | ".join(parts)
    emit({
        "type": "assistant",
        "message": {
            "id": f"msg_final_{turn_counter}",
            "content": [{"type": "text", "text": reply}]
        }
    })
    emit({
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "result": reply
    })
"#;

fn ensure_mock_claude_installed() -> PathBuf {
    MOCK_CLAUDE_WS_PATH
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("orbit_mock_ws_{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create mock dir");
            let script = dir.join("mock_claude_ws.py");
            std::fs::write(&script, MOCK_CLAUDE_WS_SCRIPT).expect("write mock script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod mock");
            }
            orbit_providers::set_provider_executable_override(
                ProviderKind::ClaudeCode,
                script.clone(),
            );
            orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));
            script
        })
        .clone()
}

struct TempTestDir(PathBuf);

impl std::ops::Deref for TempTestDir {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        orbit_shim::kill_all_shims_in_dir_sync(&self.0.join("shims"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_temp_dir(tag: &str) -> TempTestDir {
    let uid = &Uuid::new_v4().simple().to_string()[..8];
    let dir = std::env::temp_dir().join(format!("ow_{tag}_{uid}"));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    TempTestDir(dir)
}

fn init_git_repo(dir: &Path, branch: &str) {
    std::fs::create_dir_all(dir).expect("mkdir repo");
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git cmd");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-b", branch]);
    run(&["config", "user.email", "test@orbit.local"]);
    run(&["config", "user.name", "Orbit Test"]);
    std::fs::write(dir.join("README.md"), "# Test Repo\n").expect("write readme");
    run(&["add", "."]);
    run(&["commit", "-m", "initial commit"]);
}

async fn wait_until<F, Fut>(timeout: Duration, mut pred: F) -> bool
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if pred().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }
    false
}

async fn open_and_connect_core(
    db_path: PathBuf,
    pairing_uri: String,
) -> (Arc<OrbitAppCore>, String) {
    let core = tokio::task::spawn_blocking(move || {
        OrbitAppCore::open(&db_path, false).expect("open client core")
    })
    .await
    .expect("spawn_blocking open");

    let card = core.pair_host(&pairing_uri).expect("pair host");
    let host_id = card.host_id.clone();

    let online = wait_until(Duration::from_secs(12), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        async move {
            c.host_detail(&hid)
                .map(|d| {
                    d.host.connection_state == orbit_client_core::HostConnectionState::OnlineDirect
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(online, "OrbitAppCore must connect to embedded daemon");
    (core, host_id)
}

async fn drop_core(core: Arc<OrbitAppCore>) {
    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
}

// ============================================================================
// TEST 1: Non-Git Worktree Guard + Nested Git Repo Discovery & Sidebar Folding
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_01_non_git_worktree_guard_and_nested_git_repo_folding() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("ng_home");
    let plain_dir = new_temp_dir("ng_plain");
    let parent_dir = new_temp_dir("ng_parent");
    let backend_repo = parent_dir.join("backend-api");
    let web_repo = parent_dir.join("web-app");
    init_git_repo(&backend_repo, "main");
    init_git_repo(&web_repo, "feat/ui");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let offer = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("pairing offer");
    let uri = offer.to_qr_uri().expect("pair uri");

    let (core, host_id) = open_and_connect_core(home.join("client1.db"), uri).await;

    // 1. Pure non-git directory -> InspectGitRepos returns is_git_repo=false & 0 nested repos
    let plain_insp = core
        .inspect_git_repos_on_host(&host_id, &plain_dir.to_string_lossy(), Vec::new())
        .await
        .expect("inspect plain dir");
    assert!(!plain_insp.is_git_repo, "plain_dir must not be a git repo");
    assert!(
        plain_insp.nested_git_repos.is_empty(),
        "plain_dir must have 0 nested git repos"
    );

    // 2. Defense-in-depth: daemon rejects worktree=true on pure non-git directory with structured error
    let raw_client = connect_direct(&format!("ws://{addr}/ws"), &rt.pairing_secret, None)
        .await
        .expect("raw ws");
    let req_id = Uuid::new_v4();
    let err_resp = raw_client
        .request(
            WireMessage::CreateAgent {
                request_id: req_id,
                cwd: plain_dir.to_string_lossy().to_string(),
                extra_dirs: Vec::new(),
                title: Some("Should Fail WT".into()),
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                fast_mode: None,
                mode: SessionMode::AcceptEdits,
                initial_prompt: "hello".into(),
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree: true,
                workspace_id: None,
                parent_agent_id: None,
                parent_host_id: None,
                profile_id: None,
                background: false,
                auto_archive: false,
                notify_on_finish: None,
                idempotency_key: None,
                base_ref: None,
                worktree_mode: WorktreeCreateMode::BranchOff,
                branch: Some("feat/bad".into()),
                handle: None,
                auto_cluster: Some(false),
                acp_agent_id: None,
            },
            Duration::from_secs(10),
            |w| matches!(w, WireMessage::RpcError { request_id, .. } if *request_id == Some(req_id)),
        )
        .await
        .expect("expected RpcError for worktree on non-git dir");
    match err_resp {
        WireMessage::RpcError { message, .. } => {
            assert!(
                message.contains("not_a_git_repo"),
                "expected structured not_a_git_repo error, got: {message}"
            );
        }
        other => panic!("unexpected response: {other:?}"),
    }

    // 3. Non-git parent containing 2 nested Git repos -> discovers both with branches
    let parent_str = parent_dir.to_string_lossy().to_string();
    let parent_insp = core
        .inspect_git_repos_on_host(&host_id, &parent_str, Vec::new())
        .await
        .expect("inspect parent");
    assert!(
        !parent_insp.is_git_repo,
        "parent_dir itself is not a git repo"
    );
    assert_eq!(
        parent_insp.nested_git_repos.len(),
        2,
        "must discover 2 nested repos"
    );
    let names: Vec<&str> = parent_insp
        .nested_git_repos
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(names.contains(&"backend-api"));
    assert!(names.contains(&"web-app"));

    // Register parent_dir as a saved project in OrbitAppCore
    core.upsert_project(None, &host_id, "Monorepo Parent", vec![parent_str.clone()])
        .expect("upsert parent project");

    // 4. Create a worktree agent from nested repo `backend-api`, passing parent_dir in extra_dirs
    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &backend_repo.to_string_lossy(),
            extra_dirs: vec![parent_str.clone()],
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Check nested repo worktree folding",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: Some("feat/nested-wt"),
            workspace_id: None,
            handle: Some("nested-worker".into()),
            auto_cluster: Some(false),
        },
        None,
    );

    // Wait for real worktree agent to finish its turn and appear in client core
    let folded_ok = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        let pstr = parent_str.clone();
        async move {
            let sb = c.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                Some(pstr.as_str()),
            );
            sb.projects.iter().any(|p| {
                p.primary_cwd == pstr
                    && !p.is_git_repo
                    && p.nested_git_repos.len() == 2
                    && p.conversations
                        .iter()
                        .any(|conv| !conv.agent.agent_id.starts_with("opt_") && conv.is_worktree)
            })
        }
    })
    .await;
    assert!(
        folded_ok,
        "Worktree created from nested Git repo must fold cleanly under parent non-git project in sidebar"
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// TEST 2: Dual-Client Real-Time Multi-Device Sync (Read/Unread/Pin/Rename/Projects/Manual Order)
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_02_dual_client_realtime_multi_device_sync() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("sync_home");
    let repo_alpha = new_temp_dir("sync_alpha");
    let repo_beta = new_temp_dir("sync_beta");
    init_git_repo(&repo_alpha, "main");
    init_git_repo(&repo_beta, "main");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let offer = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("pairing offer");
    let uri = offer.to_qr_uri().expect("pair uri");

    // Two completely independent client devices (separate SQLite DBs) connected to the same daemon
    let (client_a, host_id) = open_and_connect_core(home.join("device_a.db"), uri.clone()).await;
    let (client_b, _) = open_and_connect_core(home.join("device_b.db"), uri.clone()).await;

    // Create a conversation from Client A
    client_a.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo_alpha.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Test cross-device sync reply",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: None,
            auto_cluster: Some(false),
        },
        None,
    );

    // Wait for real agent to finish its turn on daemon and appear unread on BOTH Client A and Client B
    let both_unread = wait_until(Duration::from_secs(15), || {
        let a = Arc::clone(&client_a);
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        async move {
            let da = a.host_detail(&hid);
            let db = b.host_detail(&hid);
            match (da, db) {
                (Some(da), Some(db)) => {
                    let ag_a = da.agents.iter().find(|x| !x.agent_id.starts_with("opt_"));
                    let ag_b = db.agents.iter().find(|x| !x.agent_id.starts_with("opt_"));
                    match (ag_a, ag_b) {
                        (Some(x), Some(y)) => {
                            !x.is_running
                                && !y.is_running
                                && x.latest_seq > 0
                                && x.latest_seq == y.latest_seq
                                && x.attention == AttentionState::Finished
                                && y.attention == AttentionState::Finished
                                && x.is_unread
                                && y.is_unread
                        }
                        _ => false,
                    }
                }
                _ => false,
            }
        }
    })
    .await;
    assert!(
        both_unread,
        "Both Device A and Device B must observe finished thread as unread"
    );

    let (agent_id, latest_seq) = {
        let d = client_a.host_detail(&host_id).expect("detail A");
        let ag = d
            .agents
            .iter()
            .find(|x| !x.agent_id.starts_with("opt_"))
            .expect("agent");
        (ag.agent_id.clone(), ag.latest_seq)
    };

    // 1. Device A reads the thread -> Device B must clear unread in real time!
    client_a.mark_agent_seen(&host_id, &agent_id, latest_seq);
    let b_read_cleared = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| !ag.is_unread && ag.read_seq >= latest_seq)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        b_read_cleared,
        "Reading on Device A must clear unread on Device B in real time"
    );

    // 2. Device B marks thread ManualUnread -> Device A must show unread + ManualUnread in real time!
    client_b.mark_agent_unread(&host_id, &agent_id);
    let a_manual_unread = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&client_a);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            a.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| ag.is_unread && ag.attention == AttentionState::ManualUnread)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        a_manual_unread,
        "Mark Unread on Device B must sync ManualUnread to Device A in real time"
    );

    // 3. Device A marks read again -> clears ManualUnread on BOTH devices!
    client_a.mark_agent_seen(&host_id, &agent_id, latest_seq);
    let both_cleared = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&client_a);
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            let ca = a
                .host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| !ag.is_unread && ag.attention == AttentionState::None)
                .unwrap_or(false);
            let cb = b
                .host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| !ag.is_unread && ag.attention == AttentionState::None)
                .unwrap_or(false);
            ca && cb
        }
    })
    .await;
    assert!(
        both_cleared,
        "Clearing ManualUnread on Device A must sync to Device B"
    );

    // 4. Pin on Device A -> synced to Device B; Unpin on Device B -> synced to Device A
    client_a.set_agent_pinned(&host_id, &agent_id, true);
    let b_pinned = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| ag.is_pinned)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(b_pinned, "Pin on Device A must sync to Device B");

    client_b.set_agent_pinned(&host_id, &agent_id, false);
    let a_unpinned = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&client_a);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            a.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| !ag.is_pinned)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(a_unpinned, "Unpin on Device B must sync to Device A");

    // 5. Thread Rename on Device A -> synced to Device B
    client_a.update_agent(
        &host_id,
        &agent_id,
        Some("Cross-Device Renamed Thread".into()),
        None,
        None,
        None,
        None,
    );
    let b_renamed = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| ag.title == "Cross-Device Renamed Thread")
                .unwrap_or(false)
        }
    })
    .await;
    assert!(b_renamed, "Thread rename on Device A must sync to Device B");

    // 6. Projects & Manual Drag-and-Drop Reorder sync from Device A -> Device B
    let p_alpha = client_a
        .upsert_project(
            None,
            &host_id,
            "Alpha Project",
            vec![repo_alpha.to_string_lossy().to_string()],
        )
        .expect("upsert alpha");
    let p_beta = client_a
        .upsert_project(
            None,
            &host_id,
            "Beta Project",
            vec![repo_beta.to_string_lossy().to_string()],
        )
        .expect("upsert beta");
    client_a
        .reorder_projects(vec![p_beta.project_id.clone(), p_alpha.project_id.clone()])
        .expect("reorder projects");

    let b_order_synced = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        async move {
            let sb = b.fleet_sidebar(
                SidebarProjectSort::Manual,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            let names: Vec<&str> = sb.projects.iter().map(|p| p.name.as_str()).collect();
            let pos_beta = names.iter().position(|n| *n == "Beta Project");
            let pos_alpha = names.iter().position(|n| *n == "Alpha Project");
            matches!((pos_beta, pos_alpha), (Some(pb), Some(pa)) if pb < pa)
        }
    })
    .await;
    assert!(
        b_order_synced,
        "Saved projects and Manual drag-and-drop sort order must sync from Device A to Device B"
    );

    // 7. Offline Device C boots fresh, connects later -> inherits full daemon SSOT state!
    let (client_c, _) = open_and_connect_core(home.join("device_c.db"), uri).await;
    let c_converged = wait_until(Duration::from_secs(10), || {
        let c = Arc::clone(&client_c);
        let hid = host_id.clone();
        let aid = agent_id.clone();
        async move {
            let detail_ok = c
                .host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|ag| !ag.is_unread && ag.title == "Cross-Device Renamed Thread")
                .unwrap_or(false);
            let sb = c.fleet_sidebar(
                SidebarProjectSort::Manual,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            let names: Vec<&str> = sb.projects.iter().map(|p| p.name.as_str()).collect();
            let pos_beta = names.iter().position(|n| *n == "Beta Project");
            let pos_alpha = names.iter().position(|n| *n == "Alpha Project");
            detail_ok && matches!((pos_beta, pos_alpha), (Some(pb), Some(pa)) if pb < pa)
        }
    })
    .await;
    assert!(
        c_converged,
        "Freshly connected Device C must converge to daemon read state, title, and project order"
    );

    drop_core(client_a).await;
    drop_core(client_b).await;
    drop_core(client_c).await;
    drop(rt);
}

// ============================================================================
// TEST 3: Mission Workspace Peers, Roster XML, BOARD.md, @all Broadcast & Conflict Radar
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_03_mission_workspace_peers_board_mentions_and_conflict_radar() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("mw_home");
    let repo = new_temp_dir("mw_repo");
    init_git_repo(&repo, "main");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let offer = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("pairing offer");
    let uri = offer.to_qr_uri().expect("pair uri");
    let (core, host_id) = open_and_connect_core(home.join("mw_client.db"), uri).await;

    // 1. Create @lead in a Mission Workspace & write initial spec to BOARD.md via MCP
    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "[MOCK:BOARD_WRITE] SECTION=Spec BODY=Shared_Blackboard_Spec_Ready",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("lead".into()),
            auto_cluster: Some(false),
        },
        None,
    );

    let lead_ready = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        async move {
            c.host_detail(&hid)
                .and_then(|d| {
                    d.agents.into_iter().find(|a| {
                        !a.agent_id.starts_with("opt_") && a.handle.as_deref() == Some("lead")
                    })
                })
                .map(|a| !a.is_running && a.latest_seq > 0)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(lead_ready, "@lead must finish initial turn");

    let (lead_id, ws_id) = {
        let d = core.host_detail(&host_id).expect("detail");
        let lead = d
            .agents
            .iter()
            .find(|a| a.handle.as_deref() == Some("lead"))
            .expect("lead");
        (lead.agent_id.clone(), lead.workspace_id.clone())
    };

    // Verify BOARD.md on disk + synced to client core
    core.fetch_workspace_board(&host_id, &ws_id);
    let board_synced = wait_until(Duration::from_secs(10), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        let wid = ws_id.clone();
        async move {
            c.workspace_board(&hid, &wid)
                .map(|b| b.revision >= 1 && b.content.contains("Shared Blackboard Spec Ready"))
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        board_synced,
        "BOARD.md update via MCP must persist and sync to client"
    );
    let board_file = home.join("workspaces").join(&ws_id).join("BOARD.md");
    assert!(
        board_file.is_file(),
        "BOARD.md must be written to disk at {}",
        board_file.display()
    );

    // 2. Spawn @worker inside the SAME Mission Workspace (in an isolated Git worktree!)
    //    and have it verify `<orbit-workspace>` XML + call workspace_peers / workspace_board_read / read_peer_context / message_peer
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo.to_string_lossy().to_string(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::ClaudeCode,
        acp_agent_id: None,
        model: None,
        effort: None,
        fast_mode: None,
        mode: SessionMode::AcceptEdits,
        initial_prompt: "[MOCK:CHECK_WS_XML] [MOCK:PEER_COLLAB] TARGET=lead".into(),
        worktree_branch: Some("feat/worker-peer".into()),
        handle: Some("worker".into()),
        profile_id: None,
    });

    let collab_done = wait_until(Duration::from_secs(20), || {
        let store = rt.store.clone();
        let wid = ws_id.clone();
        let lid = lead_id.clone();
        async move {
            let Ok(peers) = store.list_agents_filtered(Some(&wid), false, false, None) else {
                return false;
            };
            let Some(worker) = peers.iter().find(|p| p.handle.as_deref() == Some("worker")) else {
                return false;
            };
            let worker_reply = store
                .last_assistant_message_text(&worker.agent_id)
                .ok()
                .flatten()
                .unwrap_or_default();
            let lead_received_ping = store
                .fetch_timeline_since(&lid, 0, 200)
                .ok()
                .map(|(evs, _)| {
                    evs.iter().any(|e| {
                        matches!(
                            &e.payload,
                            orbit_protocol::TimelinePayload::AgentMessage { text, .. }
                                if text.contains("PEER_PING_FROM_WORKER")
                        )
                    })
                })
                .unwrap_or(false);
            worker_reply.contains("HAS_WS_XML=True")
                && worker_reply.contains("CTX_OK=True")
                && worker_reply.contains("MSG_OK=True")
                && worker_reply.contains("lead,worker")
                && lead_received_ping
        }
    })
    .await;
    assert!(
        collab_done,
        "Worktree peer in Mission Workspace must receive <orbit-workspace> XML, read board & peer context, and deliver message_peer to @lead"
    );

    // 3. Human `@all` broadcast from @lead must reach @worker too!
    core.send_prompt(&host_id, &lead_id, "@all BROADCAST_MARKER_42 sync check");
    let broadcast_ok = wait_until(Duration::from_secs(15), || {
        let store = rt.store.clone();
        let wid = ws_id.clone();
        async move {
            let Ok(peers) = store.list_agents_filtered(Some(&wid), false, false, None) else {
                return false;
            };
            let Some(worker) = peers.iter().find(|p| p.handle.as_deref() == Some("worker")) else {
                return false;
            };
            store
                .fetch_timeline_since(&worker.agent_id, 0, 200)
                .ok()
                .map(|(evs, _)| {
                    evs.iter().any(|e| {
                        matches!(
                            &e.payload,
                            orbit_protocol::TimelinePayload::AgentMessage { text, .. }
                                if text.contains("BROADCAST_MARKER_42")
                        )
                    })
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        broadcast_ok,
        "Human @all message must broadcast to sibling peers in workspace"
    );

    // 4. Live File Conflict Radar: spawn @editor2 in the SAME cwd as @lead, both edit `src/shared.rs`
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo.to_string_lossy().to_string(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::ClaudeCode,
        acp_agent_id: None,
        model: None,
        effort: None,
        fast_mode: None,
        mode: SessionMode::AcceptEdits,
        initial_prompt: "[MOCK:EDIT_FILE] FILE=src/shared.rs".into(),
        worktree_branch: None,
        handle: Some("editor2".into()),
        profile_id: None,
    });
    // Wait briefly so @editor2 records FileEdited on src/shared.rs, then have @lead edit the same file
    let ed2_edited = wait_until(Duration::from_secs(12), || {
        let store = rt.store.clone();
        let wid = ws_id.clone();
        async move {
            store
                .list_workspace_events(&wid, 50)
                .ok()
                .map(|evs| {
                    evs.iter().any(|e| {
                        e.kind == WorkspaceEventKind::FileEdited
                            && e.file_path.as_deref() == Some("src/shared.rs")
                    })
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(ed2_edited, "@editor2 edit must record FileEdited event");

    core.send_prompt(&host_id, &lead_id, "[MOCK:EDIT_FILE] FILE=src/shared.rs");

    let conflict_detected = wait_until(Duration::from_secs(12), || {
        let c = Arc::clone(&core);
        let hid = host_id.clone();
        let wid = ws_id.clone();
        async move {
            c.fetch_workspace_events(&hid, &wid, Some(50));
            let evs = c.workspace_events(&hid, &wid);
            let has_warn = evs.iter().any(|e| {
                e.kind == WorkspaceEventKind::ConflictWarning
                    && e.file_path.as_deref() == Some("src/shared.rs")
            });
            let sb = c.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            let ws_promoted = sb.projects.iter().any(|p| {
                p.workspaces.iter().any(|w| {
                    w.workspace_id == wid && w.is_multi_agent && w.peer_count >= 3 && w.has_conflict
                })
            });
            has_warn && ws_promoted
        }
    })
    .await;
    assert!(
        conflict_detected,
        "Conflict Radar must emit ConflictWarning and surface has_conflict=true on multi-agent sidebar workspace card"
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// TEST 4: Auto-Clustering (High Auto-Group + Reversible Undo, Medium Suggest, Merge, Unrelated)
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_04_auto_clustering_high_medium_unrelated_and_reversible_undo() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("ac_home");
    let repo = new_temp_dir("ac_repo");
    init_git_repo(&repo, "main");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let offer = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("pairing offer");
    let uri = offer.to_qr_uri().expect("pair uri");
    let (core, host_id) = open_and_connect_core(home.join("ac_client.db"), uri).await;

    // 1. Seed initial workspace with clear topic + edited file `crates/orbit-client-core/src/app_core.rs`
    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "[MOCK:EDIT_FILE] FILE=crates/orbit-client-core/src/app_core.rs Refactor Noise handshake and cross-device read_seq sync in app_core.rs",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("seed".into()),
            auto_cluster: Some(false),
        },
        None,
    );

    let seed_ready = wait_until(Duration::from_secs(15), || {
        let store = rt.store.clone();
        async move {
            let Ok(agents) = store.list_agents_filtered(None, false, false, None) else {
                return false;
            };
            let Some(seed) = agents.iter().find(|a| a.handle.as_deref() == Some("seed")) else {
                return false;
            };
            store
                .list_workspace_events(&seed.workspace_id, 20)
                .ok()
                .map(|evs| evs.iter().any(|e| e.kind == WorkspaceEventKind::FileEdited))
                .unwrap_or(false)
        }
    })
    .await;
    assert!(seed_ready, "seed agent must finish and record FileEdited");

    let ws1_id = rt
        .store
        .list_agents_filtered(None, false, false, None)
        .expect("agents")
        .into_iter()
        .find(|a| a.handle.as_deref() == Some("seed"))
        .expect("seed")
        .workspace_id;
    rt.orchestrator
        .update_workspace_goal(
            &ws1_id,
            Some("Noise Handshake & ReadSeq Sync".to_string()),
            Some(
                "Refactor Noise handshake and cross-device read_seq sync in app_core.rs"
                    .to_string(),
            ),
        )
        .expect("set mission goal");

    // 2. High-confidence follow-up mentioning `@seed` & same file -> AUTO-CLUSTERS into ws1 with reversible Undo!
    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Coordinate with @seed on crates/orbit-client-core/src/app_core.rs read_seq sync tests",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("auto-joined".into()),
            auto_cluster: Some(true),
        },
        None,
    );

    let auto_clustered = wait_until(Duration::from_secs(15), || {
        let store = rt.store.clone();
        let target_ws = ws1_id.clone();
        async move {
            store
                .list_agents_filtered(None, false, false, None)
                .ok()
                .and_then(|list| {
                    list.into_iter()
                        .find(|a| a.handle.as_deref() == Some("auto-joined"))
                })
                .map(|a| {
                    a.workspace_id == target_ws
                        && a.cluster_suggestion
                            .as_ref()
                            .map(|s| s.auto_applied)
                            .unwrap_or(false)
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        auto_clustered,
        "High-confidence related prompt must auto-cluster into existing Mission Workspace with auto_applied=true"
    );

    let auto_agent_id = rt
        .store
        .list_agents_filtered(None, false, false, None)
        .expect("agents")
        .into_iter()
        .find(|a| a.handle.as_deref() == Some("auto-joined"))
        .expect("auto-joined")
        .agent_id;

    // 3. Reversible Undo via move_agent_to_workspace(None) -> splits cleanly into standalone workspace!
    core.move_agent_to_workspace(&host_id, &auto_agent_id, None);
    let undone = wait_until(Duration::from_secs(10), || {
        let store = rt.store.clone();
        let aid = auto_agent_id.clone();
        let old_ws = ws1_id.clone();
        async move {
            store
                .get_agent(&aid)
                .ok()
                .flatten()
                .map(|a| a.workspace_id != old_ws)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        undone,
        "Undo must split auto-clustered agent into its own workspace"
    );

    let split_ws_id = rt
        .store
        .get_agent(&auto_agent_id)
        .expect("get")
        .expect("agent")
        .workspace_id;

    // 4. Unrelated prompt -> stays completely separate with NO cluster_suggestion
    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &repo.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::AcceptEdits,
            initial_prompt: "Adjust marketing footer button color and typography padding",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("unrelated".into()),
            auto_cluster: Some(true),
        },
        None,
    );
    let unrelated_ok = wait_until(Duration::from_secs(15), || {
        let store = rt.store.clone();
        let target_ws = ws1_id.clone();
        async move {
            store
                .list_agents_filtered(None, false, false, None)
                .ok()
                .and_then(|list| {
                    list.into_iter()
                        .find(|a| a.handle.as_deref() == Some("unrelated"))
                })
                .map(|a| a.workspace_id != target_ws && a.cluster_suggestion.is_none())
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        unrelated_ok,
        "Unrelated prompt must remain standalone without spurious cluster suggestion"
    );

    // 5. Merge workspaces (`split_ws_id` -> `ws1_id`) consolidates peers and logs WorkspaceMerged
    core.merge_workspaces(&host_id, &split_ws_id, &ws1_id);
    let merged_ok = wait_until(Duration::from_secs(10), || {
        let store = rt.store.clone();
        let aid = auto_agent_id.clone();
        let dst = ws1_id.clone();
        async move {
            let moved = store
                .get_agent(&aid)
                .ok()
                .flatten()
                .map(|a| a.workspace_id == dst)
                .unwrap_or(false);
            let logged = store
                .list_workspace_events(&dst, 40)
                .ok()
                .map(|evs| {
                    evs.iter()
                        .any(|e| e.kind == WorkspaceEventKind::WorkspaceMerged)
                })
                .unwrap_or(false);
            moved && logged
        }
    })
    .await;
    assert!(
        merged_ok,
        "merge_workspaces must move peers and record WorkspaceMerged event"
    );

    drop_core(core).await;
    drop(rt);
}

// ============================================================================
// TEST 5: First-Class Workspace CRUD, Unclassified (`待归类`) Inbox, AI Auto-Triage,
//         Disband-to-Inbox, and Semantic Editable/Locked Tab Handles
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_05_workspace_management_unclassified_inbox_ai_triage_and_tab_rename() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("ux_home");
    let repo = new_temp_dir("ux_repo");
    init_git_repo(&repo, "main");
    let repo_str = repo.to_string_lossy().to_string();

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .expect("pairing offer")
        .to_qr_uri()
        .expect("pair uri");

    let (client_a, host_id) = open_and_connect_core(home.join("ux_a.db"), uri.clone()).await;
    let (client_b, _) = open_and_connect_core(home.join("ux_b.db"), uri).await;

    // 1. Manually create an EMPTY curated Mission Workspace -> visible on BOTH clients with peer_count == 0
    client_a.create_mission_workspace(
        &host_id,
        &repo_str,
        "支付网关重构".into(),
        Some("统一 Stripe 与 Alipay 回调幂等逻辑".into()),
        orbit_protocol::WorkspaceIsolation::Local,
        None,
        None,
    );

    let empty_ws_synced = wait_until(Duration::from_secs(12), || {
        let a = Arc::clone(&client_a);
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        async move {
            let check = |core: &OrbitAppCore| {
                let sb = core.fleet_sidebar(
                    SidebarProjectSort::Recent,
                    SidebarThreadFilter::All,
                    "",
                    Some(hid.as_str()),
                    None,
                    None,
                );
                sb.projects.iter().any(|p| {
                    p.workspaces
                        .iter()
                        .any(|w| w.title == "支付网关重构" && w.is_curated && w.peer_count == 0)
                })
            };
            check(&a) && check(&b)
        }
    })
    .await;
    assert!(
        empty_ws_synced,
        "Manually created empty curated Workspace must sync and appear in both Client A & Client B sidebars"
    );

    let manual_ws_id = rt
        .store
        .list_workspaces_filtered(false, false)
        .expect("ws list")
        .into_iter()
        .find(|w| w.title == "支付网关重构")
        .expect("manual ws")
        .workspace_id;

    // 2. Rename Workspace -> syncs across devices
    client_a.rename_workspace(&host_id, &manual_ws_id, "支付中心 v2");
    let renamed_ok = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let wid = manual_ws_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.workspaces.into_iter().find(|w| w.workspace_id == wid))
                .is_some_and(|w| w.title == "支付中心 v2" && w.is_curated && w.title_locked)
        }
    })
    .await;
    assert!(renamed_ok, "Renamed workspace must sync to Client B");

    // 3. Create 2 unclassified conversations sharing a distinct topic ("剪贴板图片上传") with auto_cluster=false
    for prompt in [
        "请帮我排查剪贴板图片上传失败导致 clipboard_upload.rs 超时的问题",
        "继续补齐 clipboard_upload.rs 剪贴板图片上传的单元测试与重试逻辑",
    ] {
        client_a.create_agent_with_profile(
            CreateAgentRequest {
                host_id: &host_id,
                cwd: &repo_str,
                extra_dirs: Vec::new(),
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                fast_mode: None,
                mode: SessionMode::AcceptEdits,
                initial_prompt: prompt,
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree_branch: None,
                workspace_id: None,
                handle: None,
                auto_cluster: Some(false),
                acp_agent_id: None,
            },
            None,
        );
    }

    // Both must settle in `unclassified_conversations` (`待归类`), NOT inside curated `workspaces`,
    // and their initial handles must be semantic (NOT generic `agy`/`claude`/`codex`/`lead`/`peer`)!
    let unclassified_ready = wait_until(Duration::from_secs(15), || {
        let a = Arc::clone(&client_a);
        let hid = host_id.clone();
        async move {
            let sb = a.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            sb.projects.iter().any(|p| {
                p.unclassified_conversations.len() == 2
                    && p.unclassified_conversations
                        .iter()
                        .all(|c| !c.agent.is_running)
            })
        }
    })
    .await;
    assert!(
        unclassified_ready,
        "New standalone conversations must appear in `unclassified_conversations` (`待归类`)"
    );

    let unclassified_agents = rt
        .store
        .list_agents_filtered(None, false, false, None)
        .expect("agents");
    assert_eq!(unclassified_agents.len(), 2);
    for ag in &unclassified_agents {
        let h = ag.handle.as_deref().unwrap_or("");
        assert!(
            !orbit_daemon::workspace_cluster::is_generic_harness_handle(h),
            "New session handle must be semantic, not generic harness name: got `{h}`"
        );
        // Verify unclassified container has is_curated == false and did NOT inject <orbit-workspace>
        let ws = rt
            .store
            .get_workspace(&ag.workspace_id)
            .expect("get ws")
            .expect("ws");
        assert!(
            !ws.is_curated,
            "Unclassified session container must have is_curated=false"
        );
        let reply = rt
            .store
            .last_assistant_message_text(&ag.agent_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        assert!(
            !reply.contains("<orbit-workspace"),
            "Unclassified conversations must NEVER inject <orbit-workspace> XML"
        );
    }

    // 4. Trigger AI Auto-Triage (`organize_project_workspaces`) on the project
    client_a.organize_project_workspaces(&host_id, &repo_str, Vec::new(), true);
    let organized_ok = wait_until(Duration::from_secs(15), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let root = repo_str.clone();
        async move {
            let Some(detail) = b.host_detail(&hid) else {
                return false;
            };
            let phase_done = detail
                .workspace_organize_status
                .get(&root)
                .is_some_and(|ev| ev.phase == orbit_protocol::WorkspaceOrganizePhase::Completed);
            let sb = b.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            let grouped = sb.projects.iter().any(|p| {
                p.unclassified_conversations.is_empty()
                    && p.workspaces
                        .iter()
                        .any(|w| w.is_curated && w.peer_count == 2)
            });
            phase_done && grouped
        }
    })
    .await;
    assert!(
        organized_ok,
        "AI Auto-Triage must group both related unclassified conversations into one curated Workspace and clear `待归类`"
    );

    let ag1_id = unclassified_agents[0].agent_id.clone();
    let ag2_id = unclassified_agents[1].agent_id.clone();
    let ag1_after_org = rt.store.get_agent(&ag1_id).expect("get").expect("ag1");
    let grouped_ws_id = ag1_after_org.workspace_id.clone();
    assert!(
        ag1_after_org
            .task_summary
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        "AI Organizer must populate 1-line `task_summary`"
    );

    // 5. Manually switch ag1 into the manually created Workspace (`支付中心 v2`)
    client_a.move_agent_to_workspace_v2(&host_id, &ag1_id, Some(manual_ws_id.clone()), None, false);
    let moved_ok = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = ag1_id.clone();
        let dst = manual_ws_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|a| a.agent_id == aid))
                .is_some_and(|a| a.workspace_id == dst)
        }
    })
    .await;
    assert!(moved_ok, "Manual workspace switch must sync to Client B");

    // 6. Move ag1 back to `待归类` (`make_unclassified = true`)
    client_a.move_agent_to_workspace_v2(&host_id, &ag1_id, None, None, true);
    let back_to_inbox = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid = ag1_id.clone();
        async move {
            let sb = b.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            sb.projects.iter().any(|p| {
                p.unclassified_conversations
                    .iter()
                    .any(|c| c.agent.agent_id == aid)
            })
        }
    })
    .await;
    assert!(
        back_to_inbox,
        "Moving agent with make_unclassified=true must return it cleanly to `待归类`"
    );

    // 7. Disband `grouped_ws_id` (`keep_agents_unclassified = true`) -> ag2 returns to `待归类` without archiving!
    client_a.archive_workspace_v2(&host_id, &grouped_ws_id, true);
    let disbanded_ok = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&client_b);
        let hid = host_id.clone();
        let aid2 = ag2_id.clone();
        let disbanded = grouped_ws_id.clone();
        async move {
            let sb = b.fleet_sidebar(
                SidebarProjectSort::Recent,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            sb.projects.iter().any(|p| {
                !p.workspaces.iter().any(|w| w.workspace_id == disbanded)
                    && p.unclassified_conversations
                        .iter()
                        .any(|c| c.agent.agent_id == aid2)
            })
        }
    })
    .await;
    assert!(
        disbanded_ok,
        "Disbanding a Workspace must archive the Workspace while keeping its sessions alive in `待归类`"
    );

    // 8. Manual Tab Handle Rename + Lock (`orbit.handle_locked=1`) must survive subsequent `summarize_agent`!
    client_a.update_agent_identity(
        &host_id,
        &ag1_id,
        Some("剪贴板修复".into()),
        None,
        None,
        true,
        false,
    );
    let locked_ok = wait_until(Duration::from_secs(10), || {
        let store = rt.store.clone();
        let aid = ag1_id.clone();
        async move {
            store.get_agent(&aid).ok().flatten().is_some_and(|a| {
                a.handle.as_deref() == Some("剪贴板修复")
                    && a.labels.get("orbit.handle_locked").map(String::as_str) == Some("1")
            })
        }
    })
    .await;
    assert!(
        locked_ok,
        "Manual CJK tab handle rename must persist with lock label"
    );

    client_a.summarize_agent(&host_id, &ag1_id);
    tokio::time::sleep(Duration::from_millis(600)).await;
    let after_resum = rt.store.get_agent(&ag1_id).expect("get").expect("ag1");
    assert_eq!(
        after_resum.handle.as_deref(),
        Some("剪贴板修复"),
        "User-locked tab handle must NEVER be overwritten by AI summarize"
    );
    assert!(
        after_resum
            .task_summary
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty()),
        "AI summarize must maintain live task_summary"
    );

    drop_core(client_a).await;
    drop_core(client_b).await;
    drop(rt);
}
