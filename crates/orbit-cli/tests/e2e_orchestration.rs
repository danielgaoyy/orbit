//! Comprehensive 10-scenario End-to-End Orchestration Integration Test Suite.
//!
//! Uses a deterministic Python3 mock Claude Code stream-json provider (injected via safe
//! `orbit_providers::set_provider_executable_override`) that speaks the exact Claude control
//! protocol and makes REAL HTTP JSON-RPC 2.0 requests to Orbit Daemon's `/mcp/agents` endpoint.
//!
//! Covers:
//! 1. Single-host delegation: `list_profiles` -> `create_workspace(worktree)` -> `create_agent` -> auto finish wakeup
//! 2. Permission checkpoint forwarding to parent -> parent `respond_to_permission` -> continued watch -> final finish
//! 3. 2s quiet-window coalescing (`send_agent_prompt` + finish -> single parent wakeup)
//! 4. Soft archive + cascade same-workspace child archive + detached & cross-workspace preservation + unarchive
//! 5. Daemon restart + surviving Agent Shim reattach + durable `finish_subscriptions` recovery
//! 6. Idle Runtime Cleanup veto (active heartbeat) + collection + transparent session resume
//! 7. Worktree lifecycle: `.worktreeinclude` copy, `orbit.json` setup, service script + `*.localhost` reverse proxy, teardown
//! 8. Cron Schedules (`auto_archive`) + Heartbeat busy-skip + idle delivery
//! 9. Cross-host Fleet Mesh (`Daemon A` <-> `Daemon B`), remote `create_agent(host_id=B)`, remote finish wakeup, idempotency
//! 10. Real `orbit` CLI subprocesses + `orbit mcp` stdio bridge + `McpToolPolicy` disabled-tool enforcement

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use orbit_client_core::{HostSessionHandle, connect_direct};
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{
    AgentMessageKind, AgentProfile, AgentStatus, ConnectedHostStatus, McpToolPolicy, ProviderKind,
    ScheduleNewAgentConfig, ScheduleRunStatus, ScheduleTarget, SessionMode, TimelinePayload,
    WireMessage, WorkspaceIsolation, WorktreeCreateMode,
};
use serde_json::{Value, json};
use uuid::Uuid;

static MOCK_CLAUDE_PATH: OnceLock<PathBuf> = OnceLock::new();

const MOCK_CLAUDE_SCRIPT: &str = r#"#!/usr/bin/env python3
import json
import os
import sys
import time
import urllib.request
import urllib.error

if "--version" in sys.argv[1:]:
    sys.stdout.write("2.1.0 (mock-claude)\n")
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
            try:
                mcp_cfg = json.loads(args[i + 1])
            except Exception:
                mcp_cfg = None
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
    return mcp_cfg, resume_id or session_id or "mock-session-1", resume_id is not None

MCP_CFG, SESSION_ID, WAS_RESUMED = parse_args()

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
        raw = resp.read().decode("utf-8")
        return json.loads(raw)

def extract_text(frame):
    content = frame.get("message", {}).get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = []
        for b in content:
            if isinstance(b, dict) and b.get("type") == "text":
                parts.append(b.get("text", ""))
        return "\n".join(parts)
    return ""

def parse_kv_tokens(text):
    out = {}
    for tok in text.split():
        if "=" in tok:
            k, v = tok.split("=", 1)
            out[k.strip()] = v.strip()
    return out

pending_perm_resp = None
turn_counter = 0

def read_next_frame():
    line = sys.stdin.readline()
    if not line:
        return None
    try:
        return json.loads(line)
    except Exception:
        return {}

while True:
    frame = read_next_frame()
    if frame is None:
        break
    ftype = frame.get("type")

    if ftype == "control_request":
        req_id = frame.get("request_id", "ctrl_1")
        sub = frame.get("request", {}).get("subtype", "")
        emit({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": req_id, "response": {"ok": True}}
        })
        if sub == "initialize":
            emit({"type": "system", "subtype": "init", "session_id": SESSION_ID, "model": "mock-claude"})
        continue

    if ftype != "user":
        continue

    turn_counter += 1
    text = extract_text(frame)
    kv = parse_kv_tokens(text)
    reply_parts = []

    if "[MOCK:RESUMED_CHECK]" in text:
        reply_parts.append(f"RESUMED={WAS_RESUMED} TURN={turn_counter}")

    if "[MOCK:SLEEP=" in text:
        sec_s = text.split("[MOCK:SLEEP=", 1)[1].split("]", 1)[0]
        time.sleep(float(sec_s))
        reply_parts.append(f"SLEPT={sec_s}")

    if "[MOCK:WRITE_FILE]" in text:
        fname = kv.get("FILE", "child_artifact.txt")
        content = kv.get("CONTENT", "hello-from-child")
        with open(os.path.join(os.getcwd(), fname), "w") as f:
            f.write(content)
        reply_parts.append(f"WROTE={fname} CWD={os.getcwd()}")

    if "[MOCK:TASK_SUBAGENT]" in text:
        emit({
            "type": "system",
            "subtype": "task_started",
            "task_id": "task_sub_1",
            "tool_use_id": "tu_sub_1",
            "task_type": "local_agent",
            "description": "Inspect architecture"
        })
        emit({
            "type": "assistant",
            "parent_tool_use_id": "tu_sub_1",
            "message": {"id": "sub_msg_1", "content": [{"type": "text", "text": "Subagent inspected repo."}]}
        })
        emit({
            "type": "system",
            "subtype": "task_notification",
            "task_id": "task_sub_1",
            "status": "completed",
            "summary": "Done inspecting"
        })
        reply_parts.append("EMITTED_TASK_SUBAGENT")

    if "[MOCK:DELEGATE_WORKTREE]" in text:
        profs = mcp_call("list_profiles", {})
        prof_id = kv.get("PROFILE_ID")
        repo = kv.get("REPO", os.getcwd())
        ws = mcp_call("create_workspace", {
            "source_cwd": repo,
            "isolation": "worktree",
            "title": "impl-wt"
        })
        ws_id = ws["result"]["structuredContent"]["workspace"]["workspace_id"]
        child_prompt = "[MOCK:WRITE_FILE] FILE=wt_proof.txt CONTENT=delegated-ok"
        if "[MOCK:CHILD_NEEDS_PERM]" in text:
            child_prompt = "[MOCK:NEED_PERMISSION] " + child_prompt
        child = mcp_call("create_agent", {
            "workspace_id": ws_id,
            "profile_id": prof_id,
            "mode": "default" if "[MOCK:CHILD_NEEDS_PERM]" in text else "accept_edits",
            "title": "Delegated Worker",
            "prompt": child_prompt,
            "notify_on_finish": True
        })
        child_id = child["result"]["structuredContent"]["agent_id"]
        reply_parts.append(f"DELEGATED child_id={child_id} ws_id={ws_id} profiles_ok={not profs['result']['isError']}")

    if "[MOCK:DELEGATE_REMOTE]" in text:
        target_host = kv.get("TARGET_HOST")
        remote_cwd = kv.get("REMOTE_CWD", os.getcwd())
        hosts_res = mcp_call("list_hosts", {})
        child = mcp_call("create_agent", {
            "host_id": target_host,
            "cwd": remote_cwd,
            "provider": "claude_code",
            "title": "Remote Mesh Worker",
            "prompt": "[MOCK:ECHO] remote-mesh-success",
            "notify_on_finish": True
        })
        child_id = child["result"]["structuredContent"]["agent_id"]
        reply_parts.append(f"DELEGATED_REMOTE child_id={child_id} host={target_host} hosts_ok={not hosts_res['result']['isError']}")

    if "[MOCK:DELEGATE_COALESCE]" in text:
        child = mcp_call("create_agent", {
            "cwd": os.getcwd(),
            "provider": "claude_code",
            "title": "Coalesce Child",
            "prompt": "[MOCK:MSG_PARENT_AND_FINISH]",
            "notify_on_finish": True
        })
        child_id = child["result"]["structuredContent"]["agent_id"]
        reply_parts.append(f"SPAWNED_COALESCE child_id={child_id}")

    if "[MOCK:MSG_PARENT_AND_FINISH]" in text:
        parent_id = os.environ.get("ORBIT_PARENT_AGENT_ID")
        if parent_id:
            mcp_call("send_agent_prompt", {
                "agent_id": parent_id,
                "prompt": "PRE_FINISH_REPORT: progress 100%",
                "notify_on_finish": False
            })
        time.sleep(0.15)
        reply_parts.append("FINAL_CHILD_SUMMARY: all done")

    if "[MOCK:NEED_PERMISSION]" in text:
        perm_req_id = "perm_ctrl_1"
        emit({
            "type": "control_request",
            "request_id": perm_req_id,
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "tool_use_id": "tu_bash_perm_1",
                "input": {"command": "echo approved-tool"}
            }
        })
        # Block waiting for control_response from Orbit
        decision = "deny"
        while True:
            rf = read_next_frame()
            if rf is None:
                sys.exit(0)
            if rf.get("type") == "control_response":
                resp_obj = rf.get("response", {}).get("response", {})
                decision = resp_obj.get("behavior", "deny")
                break
        reply_parts.append(f"PERM_DECISION={decision}")

    if "permission-request" in text and "respond_to_permission" in text:
        # Parent received permission checkpoint envelope — approve child via MCP!
        perms = mcp_call("list_pending_permissions", {})
        plist = perms["result"]["structuredContent"].get("permissions", [])
        for p in plist:
            mcp_call("respond_to_permission", {
                "agent_id": p["agent_id"],
                "request_id": p["request_id"],
                "decision": "allow"
            })
        reply_parts.append(f"PARENT_APPROVED_PERMS={len(plist)}")

    if not reply_parts:
        reply_parts.append(f"ACK_TURN={turn_counter} ECHO={text[:160]}")

    final_text = " | ".join(reply_parts)
    emit({
        "type": "assistant",
        "message": {
            "id": f"msg_{turn_counter}",
            "content": [{"type": "text", "text": final_text}]
        }
    })
    emit({
        "type": "result",
        "subtype": "success",
        "is_error": False,
        "result": final_text
    })
"#;

fn ensure_mock_claude_installed() -> PathBuf {
    MOCK_CLAUDE_PATH
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("orbit_mock_cli_{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("create mock cli dir");
            let script = dir.join("mock_claude.py");
            std::fs::write(&script, MOCK_CLAUDE_SCRIPT).expect("write mock claude");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                    .expect("chmod mock claude");
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

impl AsRef<Path> for TempTestDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        orbit_shim::kill_all_shims_in_dir_sync(&self.0.join("shims"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_temp_dir(prefix: &str) -> TempTestDir {
    let short_uid = &Uuid::new_v4().simple().to_string()[..8];
    let short_pfx = prefix.strip_prefix("orbit_e2e_").unwrap_or(prefix);
    let dir = std::env::temp_dir().join(format!("oe_{short_pfx}_{short_uid}"));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    TempTestDir(dir)
}

fn init_temp_git_repo(dir: &Path) {
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
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@orbit.local"]);
    run(&["config", "user.name", "Orbit Test"]);
    std::fs::write(dir.join("README.md"), "# Orbit E2E Repo\n").expect("write readme");
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

async fn create_agent_rpc(
    client: &HostSessionHandle,
    cwd: &str,
    prompt: &str,
    mode: SessionMode,
    profile_id: Option<String>,
    parent_agent_id: Option<String>,
    worktree: bool,
) -> orbit_protocol::AgentSummary {
    let req_id = Uuid::new_v4();
    let resp = client
        .request(
            WireMessage::CreateAgent {
                request_id: req_id,
                cwd: cwd.to_string(),
                extra_dirs: Vec::new(),
                title: None,
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                mode,
                initial_prompt: prompt.to_string(),
                initial_display_prompt: None,
                initial_badges: Vec::new(),
                worktree,
                workspace_id: None,
                parent_agent_id,
                parent_host_id: None,
                profile_id,
                background: false,
                auto_archive: false,
                notify_on_finish: None,
                idempotency_key: None,
                base_ref: None,
                worktree_mode: WorktreeCreateMode::BranchOff,
                branch: None,
                handle: None,
                auto_cluster: Some(false),
            },
            Duration::from_secs(20),
            |w| matches!(w, WireMessage::AgentCreated { request_id, .. } if *request_id == req_id),
        )
        .await
        .expect("create_agent rpc");
    match resp {
        WireMessage::AgentCreated { agent, .. } => agent,
        other => panic!("unexpected create_agent resp: {other:?}"),
    }
}

// ============================================================================
// Scenario 1: Single-host delegation -> worktree -> auto finish wakeup + provider subagent
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_01_single_host_delegation_worktree_and_auto_finish_wakeup() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s1_home");
    let repo = new_temp_dir("orbit_e2e_s1_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("connect ws");

    // Register a worker Profile
    let now = Utc::now();
    let worker_profile = AgentProfile {
        profile_id: "prof_worker".into(),
        name: "Specialist Worker".into(),
        provider: ProviderKind::ClaudeCode,
        model: Some("mock-claude".into()),
        effort: None,
        mode: SessionMode::AcceptEdits,
        when_to_use: "Use for isolated worktree implementation".into(),
        system_prompt_append: Some("Be concise.".into()),
        mcp_policy: McpToolPolicy::default(),
        created_at: now,
        updated_at: now,
    };
    runtime
        .store
        .upsert_profile(&worker_profile)
        .expect("upsert profile");

    // Spawn parent that delegates via REAL HTTP MCP `/mcp/agents`
    let prompt = format!(
        "[MOCK:DELEGATE_WORKTREE] [MOCK:TASK_SUBAGENT] PROFILE_ID=prof_worker REPO={}",
        repo.display()
    );
    let parent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        &prompt,
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;

    // Wait for parent to receive automatic FinishNotification AgentMessage from child
    let got_finish = wait_until(Duration::from_secs(20), || async {
        let Ok((events, _)) = runtime.store.fetch_timeline_since(&parent.agent_id, 0, 500) else {
            return false;
        };
        let has_finish_msg = events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::FinishNotification,
                    text,
                    ..
                } if text.contains("WROTE=wt_proof.txt")
            )
        });
        let parent_idle = runtime
            .store
            .get_agent(&parent.agent_id)
            .ok()
            .flatten()
            .is_some_and(|a| matches!(a.status, AgentStatus::Idle));
        has_finish_msg && parent_idle
    })
    .await;
    assert!(
        got_finish,
        "Parent must automatically wake up with child finish report"
    );

    // Verify child ran inside an isolated git worktree and wrote wt_proof.txt there (NOT in source repo!)
    assert!(
        !repo.join("wt_proof.txt").exists(),
        "Source repo must remain untouched when child edits worktree"
    );
    let children = runtime
        .store
        .list_agents_filtered(None, true, true, Some(&parent.agent_id))
        .expect("list children");
    assert_eq!(children.len(), 1);
    let child_cwd = PathBuf::from(&children[0].cwd);
    assert!(child_cwd.join("wt_proof.txt").is_file());
    assert_eq!(
        std::fs::read_to_string(child_cwd.join("wt_proof.txt")).expect("read artifact"),
        "delegated-ok"
    );

    // Verify Claude native Task subagent was persisted for parent
    let native_subs = runtime
        .store
        .list_provider_subagents(&parent.agent_id)
        .expect("list provider subagents");
    assert!(
        native_subs.iter().any(|s| s.subagent_id == "task_sub_1"),
        "Native provider subagent task_sub_1 should be tracked"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 2: Permission checkpoint forwarding -> parent resolves -> continued watch -> finish
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_02_permission_checkpoint_forwarding_and_continued_watch() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s2_home");
    let repo = new_temp_dir("orbit_e2e_s2_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("connect ws");

    let now = Utc::now();
    runtime
        .store
        .upsert_profile(&AgentProfile {
            profile_id: "prof_perm".into(),
            name: "Default Perm Worker".into(),
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            mode: SessionMode::Default,
            when_to_use: "Needs approval".into(),
            system_prompt_append: None,
            mcp_policy: McpToolPolicy::default(),
            created_at: now,
            updated_at: now,
        })
        .expect("upsert profile");

    let prompt = format!(
        "[MOCK:DELEGATE_WORKTREE] [MOCK:CHILD_NEEDS_PERM] PROFILE_ID=prof_perm REPO={}",
        repo.display()
    );
    let parent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        &prompt,
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;

    // Wait for BOTH permission checkpoint AND final finish notification after parent approves!
    let completed = wait_until(Duration::from_secs(25), || async {
        let Ok((events, _)) = runtime.store.fetch_timeline_since(&parent.agent_id, 0, 500) else {
            return false;
        };
        let saw_checkpoint = events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    permission_context: Some(_),
                    ..
                }
            )
        });
        let saw_final_finish = events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::FinishNotification,
                    permission_context: None,
                    text,
                    ..
                } if text.contains("PERM_DECISION=allow")
            )
        });
        saw_checkpoint && saw_final_finish
    })
    .await;
    assert!(
        completed,
        "Parent must receive permission checkpoint, approve child via MCP, and then receive final finish wakeup"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 3: 2s Quiet-Window Coalescing (send_agent_prompt + finish => 1 parent wakeup)
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_03_quiet_window_coalescing_single_parent_wakeup() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s3_home");
    let repo = new_temp_dir("orbit_e2e_s3_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("connect ws");

    let parent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:DELEGATE_COALESCE]",
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;

    let coalesced_ok = wait_until(Duration::from_secs(20), || async {
        let Ok((events, _)) = runtime.store.fetch_timeline_since(&parent.agent_id, 0, 500) else {
            return false;
        };
        let has_pre = events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage { text, .. } if text.contains("PRE_FINISH_REPORT")
            )
        });
        let has_fin = events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::FinishNotification,
                    text,
                    ..
                } if text.contains("FINAL_CHILD_SUMMARY")
            )
        });
        let assistant_turns: Vec<_> = events
            .iter()
            .filter(|e| {
                matches!(
                    &e.payload,
                    TimelinePayload::AssistantMessage {
                        streaming: false,
                        ..
                    }
                )
            })
            .collect();
        // Turn 1 = initial spawn, Turn 2 = SINGLE coalesced wakeup containing both messages!
        has_pre && has_fin && assistant_turns.len() == 2
    })
    .await;
    assert!(
        coalesced_ok,
        "Child direct message + immediate finish within 2s must coalesce into exactly 1 parent wakeup turn"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 4: Soft archive + cascade same-ws child + detached & cross-ws survival + unarchive
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_04_soft_archive_cascade_detach_and_unarchive() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s4_home");
    let repo = new_temp_dir("orbit_e2e_s4_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("connect ws");

    let parent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] parent",
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;
    let child_same_ws = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] child-same",
        SessionMode::AcceptEdits,
        None,
        Some(parent.agent_id.clone()),
        false,
    )
    .await;
    let child_detached = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] child-detached",
        SessionMode::AcceptEdits,
        None,
        Some(parent.agent_id.clone()),
        false,
    )
    .await;
    let child_other_ws = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] child-wt",
        SessionMode::AcceptEdits,
        None,
        Some(parent.agent_id.clone()),
        true,
    )
    .await;

    // Detach `child_detached` before archiving parent
    runtime
        .orchestrator
        .detach_agent(&child_detached.agent_id)
        .await
        .expect("detach");

    // Soft-archive parent -> cascades ONLY to `child_same_ws`
    runtime
        .orchestrator
        .archive_agent(&parent.agent_id)
        .await
        .expect("archive parent");

    assert!(
        runtime
            .store
            .get_agent(&parent.agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_some()
    );
    assert!(
        runtime
            .store
            .get_agent(&child_same_ws.agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_some(),
        "Same-workspace child must cascade-archive with parent"
    );
    assert!(
        runtime
            .store
            .get_agent(&child_detached.agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_none(),
        "Detached child must NOT be archived when parent archives"
    );
    assert!(
        runtime
            .store
            .get_agent(&child_other_ws.agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_none(),
        "Child in its own worktree workspace must NOT cascade-archive"
    );

    // Unarchive parent restores it cleanly
    let restored = runtime
        .orchestrator
        .unarchive_agent(&parent.agent_id)
        .await
        .expect("unarchive");
    assert!(restored.archived_at.is_none());

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 5: Daemon restart + surviving Agent Shim reattach + durable finish_subscriptions
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_05_daemon_restart_shim_reattach_and_durable_finish_subscription() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s5_home");
    let repo = new_temp_dir("orbit_e2e_s5_repo");
    init_temp_git_repo(&repo);

    // Phase 1: Start Daemon 1, spawn Parent + slow sleeping Child + register durable finish sub
    let (addr1, rt1) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon 1");
    let client1 = connect_direct(&format!("ws://{addr1}/ws"), None)
        .await
        .expect("connect 1");

    let parent = create_agent_rpc(
        &client1,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] parent-ready",
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;
    assert!(
        wait_until(Duration::from_secs(10), || async {
            rt1.store
                .get_agent(&parent.agent_id)
                .ok()
                .flatten()
                .is_some_and(|a| matches!(a.status, AgentStatus::Idle))
        })
        .await
    );

    let child = create_agent_rpc(
        &client1,
        &repo.to_string_lossy(),
        "[MOCK:SLEEP=2.5] surviving-child-ok",
        SessionMode::AcceptEdits,
        None,
        Some(parent.agent_id.clone()),
        false,
    )
    .await;

    rt1.finish_subs
        .register(parent.agent_id.clone(), child.agent_id.clone(), None, true)
        .await
        .expect("register durable sub");
    assert_eq!(
        rt1.store.list_finish_subscriptions().expect("subs").len(),
        1
    );

    // Simulate Daemon 1 stopping while child's `orbit shim` survives!
    drop(client1);
    drop(rt1);
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Phase 2: Start fresh Daemon 2 on the SAME `$ORBIT_HOME`
    let (_addr2, rt2) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon 2");

    // Wait for surviving child to finish and Daemon 2's recovered FinishSubscriptionManager to wake Parent!
    let recovered = wait_until(Duration::from_secs(15), || async {
        let Ok((events, _)) = rt2.store.fetch_timeline_since(&parent.agent_id, 0, 500) else {
            return false;
        };
        events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::FinishNotification,
                    text,
                    ..
                } if text.contains("SLEPT=2.5")
            )
        })
    })
    .await;
    assert!(
        recovered,
        "Daemon 2 must reattach surviving shim and deliver durable finish notification to parent"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 6: Idle Runtime Cleanup veto (active heartbeat) + collection + transparent resume
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_06_idle_runtime_cleanup_veto_and_transparent_resume() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s6_home");
    let repo = new_temp_dir("orbit_e2e_s6_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    {
        let mut cfg = runtime.orch_config.write().await;
        cfg.idle_cleanup_enabled = true;
        cfg.idle_timeout_secs = 1;
    }

    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("ws");
    let agent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:ECHO] initial-turn",
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;
    assert!(
        wait_until(Duration::from_secs(10), || async {
            runtime
                .store
                .get_agent(&agent.agent_id)
                .ok()
                .flatten()
                .is_some_and(|a| matches!(a.status, AgentStatus::Idle))
        })
        .await
    );

    // Attach an active heartbeat -> idle cleanup MUST VETO collecting this agent!
    let hb = runtime
        .scheduler
        .create_schedule(
            "Keep-alive HB".into(),
            "5m",
            None,
            "ping".into(),
            ScheduleTarget::ExistingAgent {
                agent_id: agent.agent_id.clone(),
            },
            None,
            None,
            None,
        )
        .expect("create hb");

    let future_now = Utc::now() + chrono::Duration::seconds(30);
    let collected_while_hb = runtime.idle_cleanup.sweep_once(future_now).await;
    assert!(
        collected_while_hb.is_empty(),
        "Idle cleanup must veto agents with an active heartbeat"
    );

    // Delete heartbeat -> idle cleanup collects runtime cleanly!
    runtime
        .scheduler
        .delete_schedule(&hb.schedule_id)
        .expect("delete hb");
    let collected = runtime.idle_cleanup.sweep_once(future_now).await;
    assert_eq!(collected, vec![agent.agent_id.clone()]);
    assert!(
        !runtime
            .orchestrator
            .list_live_agent_ids()
            .await
            .contains(&agent.agent_id)
    );

    // Next prompt transparently resumes the collected agent with `--resume <session_id>`!
    runtime
        .orchestrator
        .send_prompt(&agent.agent_id, "[MOCK:RESUMED_CHECK]".into())
        .await
        .expect("resume send");

    let resumed_ok = wait_until(Duration::from_secs(10), || async {
        let Ok((events, _)) = runtime.store.fetch_timeline_since(&agent.agent_id, 0, 200) else {
            return false;
        };
        events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AssistantMessage { text, .. } if text.contains("RESUMED=True")
            )
        })
    })
    .await;
    assert!(
        resumed_ok,
        "Collected idle agent must transparently resume with --resume"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 7: Worktree lifecycle (.worktreeinclude, setup, service + *.localhost proxy, teardown)
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_07_worktree_lifecycle_includes_setup_service_proxy_and_teardown() {
    let home = new_temp_dir("orbit_e2e_s7_home");
    let repo = new_temp_dir("orbit_e2e_s7_repo");
    init_temp_git_repo(&repo);

    // Create untracked .env.local + .worktreeinclude + orbit.json
    std::fs::write(repo.join(".env.local"), "SECRET_KEY=orbit_123\n").expect("write env");
    std::fs::write(repo.join(".worktreeinclude"), ".env.local\n").expect("write include");

    let teardown_marker = home.join("teardown_ran.txt");
    let orbit_json = json!({
        "worktree": {
            "setup": "echo setup-ok > .setup_done",
            "teardown": format!("echo torn-down > {}", teardown_marker.display())
        },
        "scripts": {
            "web": {
                "command": "python3 -c \"import http.server, os; s=http.server.HTTPServer(('127.0.0.1', int(os.environ['PORT'])), http.server.SimpleHTTPRequestHandler); s.serve_forever()\"",
                "type": "service"
            }
        }
    });
    std::fs::write(
        repo.join("orbit.json"),
        serde_json::to_string_pretty(&orbit_json).expect("json"),
    )
    .expect("write orbit.json");

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");

    let ws = runtime
        .orchestrator
        .create_workspace_v2(orbit_daemon::manager::CreateWorkspaceSpec {
            source_cwd: repo.to_string_lossy().to_string(),
            title: Some("Proxy Test WT".into()),
            worktree_branch: Some("feat/proxy-test".into()),
            base_ref: None,
            isolation: WorkspaceIsolation::Worktree,
            worktree_mode: WorktreeCreateMode::BranchOff,
            pr_number: None,
            parent_workspace_id: None,
            background: false,
            mission_goal: None,
            project_root: None,
            allow_reuse_existing: false,
            is_curated: true,
        })
        .await
        .expect("create worktree");

    let wt_path = PathBuf::from(&ws.cwd);
    assert!(
        wt_path.join(".env.local").is_file(),
        ".worktreeinclude must copy .env.local"
    );

    // Wait for async setup hook to finish
    let setup_done = wait_until(Duration::from_secs(10), || async {
        wt_path.join(".setup_done").is_file()
    })
    .await;
    assert!(setup_done, "worktree.setup hook must run automatically");

    // Start `web` service script and access it through Orbit Daemon's `*.localhost` reverse proxy!
    let script_info = runtime
        .orchestrator
        .start_workspace_script(&ws.workspace_id, "web")
        .await
        .expect("start service script");
    let proxy_url = script_info.proxy_url.expect("proxy url");
    let proxy_host = proxy_url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .expect("proxy host")
        .to_string();

    let http_client = reqwest::Client::new();
    let proxied_ok = wait_until(Duration::from_secs(10), || {
        let c = http_client.clone();
        let h = proxy_host.clone();
        async move {
            let Ok(resp) = c
                .get(format!("http://127.0.0.1:{}/README.md", addr.port()))
                .header("Host", h)
                .send()
                .await
            else {
                return false;
            };
            resp.status().is_success()
                && resp
                    .text()
                    .await
                    .is_ok_and(|t| t.contains("Orbit E2E Repo"))
        }
    })
    .await;
    assert!(
        proxied_ok,
        "Daemon *.localhost reverse proxy must forward HTTP traffic to running service"
    );

    // Archive worktree workspace -> stops service, runs teardown hook, physically removes worktree!
    runtime
        .orchestrator
        .archive_workspace(&ws.workspace_id)
        .await
        .expect("archive worktree");
    assert!(
        teardown_marker.is_file(),
        "worktree.teardown hook must execute on archive"
    );
    assert!(
        !wt_path.exists(),
        "worktree directory must be removed after final archive"
    );

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 8: Cron Schedules (`auto_archive`) + Heartbeat busy-skip & idle wakeup
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_08_cron_schedules_auto_archive_and_heartbeat_busy_skip() {
    ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s8_home");
    let repo = new_temp_dir("orbit_e2e_s8_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let client = connect_direct(&format!("ws://{addr}/ws"), None)
        .await
        .expect("ws");

    // 1. Cron Schedule with `archive_on_finish = true`
    let sched = runtime
        .scheduler
        .create_schedule(
            "Nightly Check".into(),
            "daily 09:00",
            None,
            "[MOCK:ECHO] nightly-ok".into(),
            ScheduleTarget::NewAgent(ScheduleNewAgentConfig {
                cwd: repo.to_string_lossy().to_string(),
                workspace_id: None,
                provider: ProviderKind::ClaudeCode,
                model: None,
                effort: None,
                mode: SessionMode::AcceptEdits,
                profile_id: None,
                worktree: false,
                base_ref: None,
                archive_on_finish: true,
                background: true,
            }),
            None,
            None,
            None,
        )
        .expect("create schedule");

    let run = runtime
        .scheduler
        .run_once(&sched.schedule_id)
        .await
        .expect("run_once");
    let run_agent_id = run.agent_id.expect("run spawned agent");

    // Wait for scheduled agent to finish and auto-archive!
    let auto_archived = wait_until(Duration::from_secs(12), || async {
        let ag = runtime.store.get_agent(&run_agent_id).ok().flatten();
        let runs = runtime
            .store
            .list_schedule_runs(&sched.schedule_id, 10)
            .unwrap_or_default();
        ag.is_some_and(|a| a.archived_at.is_some())
            && runs
                .iter()
                .any(|r| r.run_id == run.run_id && r.status == ScheduleRunStatus::Succeeded)
    })
    .await;
    assert!(
        auto_archived,
        "Cron run agent must record Succeeded and auto-archive on finish"
    );

    // 2. Heartbeat busy-skip while target is running, then idle delivery
    let busy_agent = create_agent_rpc(
        &client,
        &repo.to_string_lossy(),
        "[MOCK:SLEEP=2.0] busy-turn",
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;
    assert!(
        wait_until(Duration::from_secs(5), || async {
            runtime
                .store
                .get_agent(&busy_agent.agent_id)
                .ok()
                .flatten()
                .is_some_and(|a| matches!(a.status, AgentStatus::Running { .. }))
        })
        .await
    );

    let hb = runtime
        .scheduler
        .create_schedule(
            "Monitor HB".into(),
            "5m",
            None,
            "[MOCK:ECHO] heartbeat-ping".into(),
            ScheduleTarget::ExistingAgent {
                agent_id: busy_agent.agent_id.clone(),
            },
            None,
            None,
            None,
        )
        .expect("create hb");

    // Trigger while busy -> must Skip cleanly!
    let skipped = runtime
        .scheduler
        .run_once(&hb.schedule_id)
        .await
        .expect("hb while busy");
    assert_eq!(skipped.status, ScheduleRunStatus::Skipped);

    // Wait until idle, then trigger -> delivers Heartbeat AgentMessage!
    assert!(
        wait_until(Duration::from_secs(10), || async {
            runtime
                .store
                .get_agent(&busy_agent.agent_id)
                .ok()
                .flatten()
                .is_some_and(|a| matches!(a.status, AgentStatus::Idle))
        })
        .await
    );
    let delivered = runtime
        .scheduler
        .run_once(&hb.schedule_id)
        .await
        .expect("hb while idle");
    assert_eq!(delivered.status, ScheduleRunStatus::Succeeded);

    let got_hb_msg = wait_until(Duration::from_secs(10), || async {
        let Ok((events, _)) = runtime
            .store
            .fetch_timeline_since(&busy_agent.agent_id, 0, 200)
        else {
            return false;
        };
        events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::Heartbeat,
                    ..
                }
            )
        })
    })
    .await;
    assert!(got_hb_msg, "Idle agent must receive Heartbeat AgentMessage");

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 9: Cross-Host Fleet Mesh (Daemon A <-> Daemon B) + Idempotency Receipts
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_09_cross_host_fleet_mesh_delegation_and_idempotency() {
    ensure_mock_claude_installed();
    let home_a = new_temp_dir("orbit_e2e_s9_a");
    let home_b = new_temp_dir("orbit_e2e_s9_b");
    let repo_a = new_temp_dir("orbit_e2e_s9_repo_a");
    let repo_b = new_temp_dir("orbit_e2e_s9_repo_b");
    init_temp_git_repo(&repo_a);
    init_temp_git_repo(&repo_b);

    let (addr_a, rt_a) = spawn_embedded_daemon(home_a.to_path_buf(), None)
        .await
        .expect("daemon A");
    let (addr_b, rt_b) = spawn_embedded_daemon(home_b.to_path_buf(), None)
        .await
        .expect("daemon B");

    // Register Daemon B onto Daemon A via `orbit://pair/...` URI
    let offer_b = rt_b.build_pairing_offer(vec![format!("ws://{addr_b}/ws")], None);
    let uri_b = offer_b.to_qr_uri().expect("pair uri b");
    rt_a.connected_hosts
        .add_from_pairing_uri(Some("Worker-B".into()), &uri_b)
        .await
        .expect("add peer B");

    // Wait for Daemon A <-> Daemon B persistent link to become Connected
    let mesh_connected = wait_until(Duration::from_secs(10), || async {
        rt_a.connected_hosts.list_summaries().await.iter().any(|h| {
            h.host_id.as_deref() == Some(rt_b.host_info.host_id.as_str())
                && h.status == ConnectedHostStatus::Online
        })
    })
    .await;
    assert!(
        mesh_connected,
        "Daemon A must connect to Daemon B via pairing URI"
    );

    // Spawn Parent on Daemon A that delegates a Child to Daemon B via MCP `create_agent(host_id=B)`
    let client_a = connect_direct(&format!("ws://{addr_a}/ws"), None)
        .await
        .expect("ws A");
    let prompt = format!(
        "[MOCK:DELEGATE_REMOTE] TARGET_HOST={} REMOTE_CWD={}",
        rt_b.host_info.host_id,
        repo_b.display()
    );
    let parent_a = create_agent_rpc(
        &client_a,
        &repo_a.to_string_lossy(),
        &prompt,
        SessionMode::AcceptEdits,
        None,
        None,
        false,
    )
    .await;

    // Wait for Child on Daemon B to finish AND Daemon A to wake Parent A with cross-host FinishNotification!
    let remote_wakeup = wait_until(Duration::from_secs(20), || async {
        let Ok((events, _)) = rt_a.store.fetch_timeline_since(&parent_a.agent_id, 0, 500) else {
            return false;
        };
        events.iter().any(|e| {
            matches!(
                &e.payload,
                TimelinePayload::AgentMessage {
                    kind: AgentMessageKind::FinishNotification,
                    source_host_id: Some(src_host),
                    text,
                    ..
                } if src_host == &rt_b.host_info.host_id && text.contains("remote-mesh-success")
            )
        })
    })
    .await;
    assert!(
        remote_wakeup,
        "Parent on Daemon A must receive cross-host finish wakeup from worker on Daemon B"
    );

    // Verify Idempotency Receipt replay & conflict on `CreateWorkspace`
    let idem_req1 = Uuid::new_v4();
    let ws1 = client_a
        .request(
            WireMessage::CreateWorkspace {
                request_id: idem_req1,
                source_cwd: repo_a.to_string_lossy().to_string(),
                title: Some("Idem WS".into()),
                worktree_branch: None,
                base_ref: None,
                isolation: WorkspaceIsolation::Local,
                worktree_mode: WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: false,
                idempotency_key: Some("idem-key-1".into()),
                is_curated: None,
                mission_goal: None,
                project_root: None,
            },
            Duration::from_secs(10),
            |w| matches!(w, WireMessage::WorkspaceCreated { request_id, .. } if *request_id == idem_req1),
        )
        .await
        .expect("idem 1");
    let ws1_id = match ws1 {
        WireMessage::WorkspaceCreated { workspace, .. } => workspace.workspace_id,
        other => panic!("unexpected: {other:?}"),
    };

    let idem_req2 = Uuid::new_v4();
    let ws2 = client_a
        .request(
            WireMessage::CreateWorkspace {
                request_id: idem_req2,
                source_cwd: repo_a.to_string_lossy().to_string(),
                title: Some("Idem WS".into()),
                worktree_branch: None,
                base_ref: None,
                isolation: WorkspaceIsolation::Local,
                worktree_mode: WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: false,
                idempotency_key: Some("idem-key-1".into()),
                is_curated: None,
                mission_goal: None,
                project_root: None,
            },
            Duration::from_secs(10),
            |w| matches!(w, WireMessage::WorkspaceCreated { request_id, .. } if *request_id == idem_req2),
        )
        .await
        .expect("idem 2 replay");
    match ws2 {
        WireMessage::WorkspaceCreated { workspace, .. } => {
            assert_eq!(
                workspace.workspace_id, ws1_id,
                "Idempotency replay must return exact same workspace"
            );
        }
        other => panic!("unexpected: {other:?}"),
    }

    let conflict_req = Uuid::new_v4();
    let conflict_resp = client_a
        .request(
            WireMessage::CreateWorkspace {
                request_id: conflict_req,
                source_cwd: repo_a.to_string_lossy().to_string(),
                title: Some("DIFFERENT TITLE".into()),
                worktree_branch: None,
                base_ref: None,
                isolation: WorkspaceIsolation::Local,
                worktree_mode: WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: false,
                idempotency_key: Some("idem-key-1".into()),
                is_curated: None,
                mission_goal: None,
                project_root: None,
            },
            Duration::from_secs(10),
            |w| matches!(w, WireMessage::RpcError { request_id: Some(r), .. } if *r == conflict_req),
        )
        .await
        .expect("idem conflict");
    assert!(matches!(
        conflict_resp,
        WireMessage::RpcError { ref code, .. } if code == "idempotency_conflict"
    ));

    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 10: Real `orbit` CLI subprocesses + `orbit mcp` stdio bridge + McpToolPolicy
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_10_cli_orchestration_commands_mcp_stdio_bridge_and_policy_gating() {
    let mock_bin = ensure_mock_claude_installed();
    let home = new_temp_dir("orbit_e2e_s10_home");
    let repo = new_temp_dir("orbit_e2e_s10_repo");
    init_temp_git_repo(&repo);

    let (addr, runtime) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("daemon");
    let orbit_bin = PathBuf::from(env!("CARGO_BIN_EXE_orbit"));
    let ws_url = format!("ws://{addr}/ws");

    // 1. CLI `orbit profile create` with disabled tool `delete_agent`
    let prof_out = tokio::process::Command::new(&orbit_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "--server",
            &ws_url,
            "--json",
            "profile",
            "create",
            "Restricted Reader",
            "--provider",
            "claude",
            "--id",
            "prof_restricted",
            "--disable-tool",
            "delete_agent",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .expect("orbit profile create");
    assert!(
        prof_out.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&prof_out.stderr)
    );

    // 2. CLI `orbit run --no-wait --json --profile prof_restricted`
    let run_out = tokio::process::Command::new(&orbit_bin)
        .env("ORBIT_CLAUDE_BIN", &mock_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "--server",
            &ws_url,
            "--json",
            "run",
            "--provider",
            "claude",
            "--profile",
            "prof_restricted",
            "--cwd",
            &repo.to_string_lossy(),
            "--no-wait",
            "[MOCK:ECHO] cli-hello",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .expect("orbit run");
    assert!(
        run_out.status.success(),
        "orbit run failed: {}",
        String::from_utf8_lossy(&run_out.stderr)
    );
    let created: Value = serde_json::from_slice(&run_out.stdout).expect("parse run json");
    let agent_id = created["agent_id"].as_str().expect("agent_id").to_string();

    // 3. CLI `orbit wait --json`
    let wait_out = tokio::process::Command::new(&orbit_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "--server",
            &ws_url,
            "--json",
            "wait",
            &agent_id,
            "--timeout",
            "15",
        ])
        .kill_on_drop(true)
        .output()
        .await
        .expect("orbit wait");
    assert!(wait_out.status.success());
    let wait_json: Value = serde_json::from_slice(&wait_out.stdout).expect("parse wait json");
    assert!(
        wait_json["last_message"]
            .as_str()
            .unwrap_or("")
            .contains("cli-hello")
    );

    // 4. Test `orbit mcp` stdio bridge with caller using `prof_restricted` (`delete_agent` disabled!)
    let mut mcp_child = tokio::process::Command::new(&orbit_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "mcp",
            "--caller-agent-id",
            &agent_id,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn orbit mcp");

    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = mcp_child.stdin.take().expect("stdin");
        for req in [
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "delete_agent", "arguments": {"agent_id": agent_id}}}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "list_profiles", "arguments": {}}}),
        ] {
            let line = format!("{req}\n");
            stdin
                .write_all(line.as_bytes())
                .await
                .expect("write mcp req");
        }
        stdin.shutdown().await.expect("close stdin");
    }
    let mcp_out = tokio::time::timeout(Duration::from_secs(20), mcp_child.wait_with_output())
        .await
        .expect("mcp bridge timeout")
        .expect("wait orbit mcp");
    assert!(
        mcp_out.status.success(),
        "orbit mcp failed: {}",
        String::from_utf8_lossy(&mcp_out.stderr)
    );
    let lines: Vec<Value> = String::from_utf8_lossy(&mcp_out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("valid json-rpc response"))
        .collect();
    assert_eq!(
        lines.len(),
        4,
        "Expected 4 JSON-RPC responses from orbit mcp bridge"
    );
    assert_eq!(
        lines[0]["result"]["serverInfo"]["name"],
        "orbit-orchestrator"
    );

    let listed_tools = lines[1]["result"]["tools"].as_array().expect("tools array");
    assert!(
        !listed_tools.iter().any(|t| t["name"] == "delete_agent"),
        "Disabled tool `delete_agent` must be hidden from tools/list"
    );
    assert!(listed_tools.iter().any(|t| t["name"] == "create_agent"));

    assert_eq!(
        lines[2]["result"]["isError"], true,
        "Calling policy-disabled tool `delete_agent` must return isError=true"
    );
    assert_eq!(lines[3]["result"]["isError"], false);

    // 5. CLI archive & unarchive roundtrip
    let arch = tokio::process::Command::new(&orbit_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "--server",
            &ws_url,
            "archive",
            &agent_id,
        ])
        .kill_on_drop(true)
        .output()
        .await
        .expect("orbit archive");
    assert!(arch.status.success());
    assert!(
        runtime
            .store
            .get_agent(&agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_some()
    );

    let unarch = tokio::process::Command::new(&orbit_bin)
        .args([
            "--home",
            &home.to_string_lossy(),
            "--server",
            &ws_url,
            "unarchive",
            &agent_id,
        ])
        .kill_on_drop(true)
        .output()
        .await
        .expect("orbit unarchive");
    assert!(unarch.status.success());
    assert!(
        runtime
            .store
            .get_agent(&agent_id)
            .expect("get")
            .expect("some")
            .archived_at
            .is_none()
    );

    let _ = runtime.orchestrator.kill_agent(&agent_id).await;
    // TempTestDir RAII Drop kills all shims and removes temp dirs.
}

// ============================================================================
// Scenario 11: Relay-Routed Fleet Mesh (Allowlisted Relay + Direct Mismatch Fallback)
// ============================================================================
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn scenario_11_relay_routed_fleet_mesh_and_direct_mismatch_fallback() {
    let home_a = new_temp_dir("oe_s11_a");
    let home_b = new_temp_dir("oe_s11_b");
    let home_decoy = new_temp_dir("oe_s11_c");

    // 1. Bootstrap Daemon B to obtain its X25519 static public key for strict Relay allowlisting
    let pre_b = orbit_daemon::DaemonRuntime::bootstrap(&home_b)
        .await
        .expect("bootstrap B");
    let pubkey_b = pre_b.keypair.public_key_b64.clone();
    drop(pre_b);

    // 2. Start a real allowlisted Orbit Relay
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay probe");
    let relay_addr = probe.local_addr().expect("relay addr");
    drop(probe);

    let mut allow = std::collections::HashSet::new();
    allow.insert(pubkey_b);
    let relay_cfg = orbit_relay::RelayConfig {
        listen_addr: relay_addr,
        allowed_pubkeys: std::sync::Arc::new(allow),
    };
    let relay_handle = tokio::spawn(async move {
        let _ = orbit_relay::run_relay(relay_cfg).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;
    let relay_url = format!("ws://{relay_addr}");

    // 3. Start Daemon B attached outbound to the Relay, a Decoy Daemon C, and Daemon A
    let (_addr_b, rt_b) = spawn_embedded_daemon(home_b.to_path_buf(), Some(relay_url.clone()))
        .await
        .expect("daemon B");
    let (addr_decoy, _rt_decoy) = spawn_embedded_daemon(home_decoy.to_path_buf(), None)
        .await
        .expect("daemon decoy");
    let (_addr_a, rt_a) = spawn_embedded_daemon(home_a.to_path_buf(), None)
        .await
        .expect("daemon A");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // 4. Register Daemon B onto Daemon A with Decoy C's direct endpoint + real Relay URL.
    //    Daemon A must reject the mismatched direct host_id on Decoy C and fall back over
    //    `/v1/client/{pubkey}` Noise NK E2EE Relay to reach Daemon B cleanly.
    let offer_b = rt_b.build_pairing_offer(vec![format!("ws://{addr_decoy}/ws")], Some(relay_url));
    let uri_b = offer_b.to_qr_uri().expect("pair uri b");
    rt_a.connected_hosts
        .add_from_pairing_uri(Some("Relay-Worker-B".into()), &uri_b)
        .await
        .expect("add relay peer B");

    let mesh_online = wait_until(Duration::from_secs(10), || async {
        rt_a.connected_hosts.list_summaries().await.iter().any(|h| {
            h.host_id.as_deref() == Some(rt_b.host_info.host_id.as_str())
                && h.status == ConnectedHostStatus::Online
        })
    })
    .await;
    assert!(
        mesh_online,
        "Daemon A must skip mismatched direct endpoint and connect to Daemon B via allowlisted E2EE Relay"
    );

    // 5. Execute a correlated WireMessage RPC over the Noise NK Relay mesh link
    let req_id = Uuid::new_v4();
    let reply = rt_a
        .connected_hosts
        .send_rpc(
            &rt_b.host_info.host_id,
            req_id,
            WireMessage::ListAgents {
                request_id: req_id,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(5),
        )
        .await
        .expect("ListAgents over relay mesh");
    assert!(
        matches!(reply, WireMessage::AgentsSnapshot { request_id, .. } if request_id == req_id),
        "Expected correlated AgentsSnapshot over relay mesh, got: {reply:?}"
    );

    relay_handle.abort();
}
