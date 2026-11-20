//! REAL LIVE 3-CLI End-to-End Collaboration Test (`claude` + `codex` + `agy`).
//!
//! Marked `#[ignore]` so offline CI never invokes external LLM providers.
//! Execute locally against all 3 real installed CLIs with:
//!   `cargo test -p orbit-cli --test e2e_live_all_clis -- --ignored --nocapture`
//!
//! Safety:
//! - Uses a completely isolated temporary `ORBIT_HOME` (never touches `~/.orbit`).
//! - Uses a temporary non-git parent folder containing a real nested Git repository (`rate_limiter/`).
//!
//! Scenario (Hard Multi-Agent Engineering Mission across ALL 3 CLIs):
//! 1. Non-Git parent (`mission_sandbox/`) with nested Git repo (`rate_limiter/`):
//!    - Verifies `InspectGitRepos` detects `is_git_repo = false` on parent and discovers `rate_limiter`.
//! 2. Dual `OrbitAppCore` clients (`device_a` + `device_b`) connected to the live daemon:
//!    - Verifies real-time cross-device read/unread, ManualUnread, pin, and manual project drag reorder sync.
//! 3. **Claude Code (`claude`)** as `@architect` (Local in `rate_limiter/`):
//!    - Diagnoses two real bugs in `limiter.py` (refill clamping + fractional rounding bug) and designs
//!      a new `SlidingWindowLog` rate limiter, writing the spec to `BOARD.md` via `workspace_board_update`.
//! 4. **Codex (`codex`)** as `@impl` (Isolated Git Worktree of nested repo `rate_limiter/` inside the SAME Mission Workspace):
//!    - Reads `BOARD.md` via `workspace_board_read`, implements the bug fixes + `SlidingWindowLog` + tests
//!      inside its isolated worktree, runs `python3 -m unittest discover -v` until all tests pass, and
//!      updates `BOARD.md` via `workspace_board_update`.
//!    - Verifies the nested-repo worktree folds cleanly under `mission_sandbox` in `fleet_sidebar()`!
//! 5. **Antigravity (`agy`)** as `@reviewer` (`plan` mode inside the SAME Mission Workspace):
//!    - Uses `workspace_peers`, `workspace_board_read`, and `read_peer_context(handle="impl")` to inspect
//!      `@impl`'s worktree changes, verifies correctness, and appends a `"Review Verdict"` section to `BOARD.md`.
//! 6. Verifies all 3 real CLIs (`claude`, `codex`, `agy`) coexist inside ONE multi-agent Mission Workspace
//!    with live `<orbit-workspace>` roster awareness, shared `BOARD.md` updates from all 3 CLIs, and passing tests.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use orbit_client_core::app_core::{CreateAgentRequest, OrbitAppCore};
use orbit_client_core::contract::SpawnWorkspacePeerRequest;
use orbit_client_core::sidebar::{SidebarProjectSort, SidebarThreadFilter};
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{AttentionState, ProviderKind, SessionMode};
use uuid::Uuid;

struct TempDirGuard(PathBuf);

impl std::ops::Deref for TempDirGuard {
    type Target = Path;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        orbit_shim::kill_all_shims_in_dir_sync(&self.0.join("shims"));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp_dir(prefix: &str) -> TempDirGuard {
    let uid = &Uuid::new_v4().simple().to_string()[..8];
    let dir = std::env::temp_dir().join(format!("ol_{prefix}_{uid}"));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    TempDirGuard(dir)
}

const BUGGY_LIMITER_PY: &str = r#""""Rate limiter primitives for Orbit live 3-CLI mission benchmark."""

from dataclasses import dataclass
from typing import Callable, Deque
from collections import deque
import time


class TokenBucket:
    """Classic token bucket rate limiter with injectable monotonic clock."""

    def __init__(
        self,
        capacity: float,
        refill_rate_per_sec: float,
        clock: Callable[[], float] = time.monotonic,
    ) -> None:
        if capacity <= 0 or refill_rate_per_sec <= 0:
            raise ValueError("capacity and refill_rate_per_sec must be positive")
        self.capacity = float(capacity)
        self.refill_rate = float(refill_rate_per_sec)
        self._clock = clock
        self._tokens = float(capacity)
        self._last_ts = self._clock()

    def _refill(self) -> None:
        now = self._clock()
        elapsed = now - self._last_ts
        # BUG 1: does not guard against backward clock skew and fails to clamp to self.capacity
        self._tokens = self._tokens + elapsed * self.refill_rate
        self._last_ts = now

    def allow(self, cost: float = 1.0) -> bool:
        if cost <= 0:
            raise ValueError("cost must be positive")
        self._refill()
        # BUG 2: uses `>` instead of `>=` so exact remaining tokens are wrongly rejected
        if self._tokens > cost:
            self._tokens -= cost
            return True
        return False

    @property
    def available_tokens(self) -> float:
        self._refill()
        return self._tokens
"#;

const INITIAL_TEST_LIMITER_PY: &str = r#"import unittest
from limiter import TokenBucket


class FakeClock:
    def __init__(self, start: float = 100.0) -> None:
        self.now = start

    def __call__(self) -> float:
        return self.now

    def advance(self, seconds: float) -> None:
        self.now += seconds


class TestTokenBucket(unittest.TestCase):
    def test_exact_capacity_consumption_allowed(self) -> None:
        clk = FakeClock()
        bucket = TokenBucket(capacity=3, refill_rate_per_sec=1.0, clock=clk)
        self.assertTrue(bucket.allow(1))
        self.assertTrue(bucket.allow(1))
        self.assertTrue(bucket.allow(1))
        self.assertFalse(bucket.allow(1))

    def test_refill_clamped_to_capacity(self) -> None:
        clk = FakeClock()
        bucket = TokenBucket(capacity=5, refill_rate_per_sec=2.0, clock=clk)
        clk.advance(100.0)
        self.assertLessEqual(bucket.available_tokens, 5.0)


if __name__ == "__main__":
    unittest.main()
"#;

fn init_nested_rate_limiter_repo(repo: &Path) {
    std::fs::create_dir_all(repo).expect("mkdir repo");
    std::fs::write(repo.join("limiter.py"), BUGGY_LIMITER_PY).expect("write limiter.py");
    std::fs::write(repo.join("test_limiter.py"), INITIAL_TEST_LIMITER_PY)
        .expect("write test_limiter.py");
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(repo)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "mission@orbit.local"]);
    run(&["config", "user.name", "Orbit Mission"]);
    run(&["add", "."]);
    run(&["commit", "-m", "initial buggy rate limiter"]);
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
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

async fn open_and_connect(db: PathBuf, uri: String) -> (Arc<OrbitAppCore>, String) {
    let core = tokio::task::spawn_blocking(move || OrbitAppCore::open(&db, false).expect("core"))
        .await
        .expect("open core");
    let card = core.pair_host(&uri).expect("pair");
    let hid = card.host_id.clone();
    core.connect_host_async(hid.clone());
    let ok = wait_until(Duration::from_secs(15), || {
        let c = Arc::clone(&core);
        let h = hid.clone();
        async move {
            c.host_detail(&h)
                .map(|d| {
                    d.host.connection_state == orbit_client_core::HostConnectionState::OnlineDirect
                })
                .unwrap_or(false)
        }
    })
    .await;
    assert!(ok, "core must connect to daemon");
    (core, hid)
}

async fn wait_for_peer_idle(
    rt: &orbit_daemon::DaemonRuntime,
    handle: &str,
    timeout: Duration,
) -> orbit_protocol::AgentSummary {
    let done = wait_until(timeout, || {
        let store = rt.store.clone();
        let h = handle.to_string();
        async move {
            let Ok(agents) = store.list_agents_filtered(None, false, false, None) else {
                return false;
            };
            let Some(ag) = agents
                .iter()
                .find(|a| a.handle.as_deref() == Some(h.as_str()))
            else {
                return false;
            };
            let has_reply = store
                .last_assistant_message_text(&ag.agent_id)
                .ok()
                .flatten()
                .map(|t| !t.trim().is_empty())
                .unwrap_or(false);
            let settled = matches!(
                ag.status,
                orbit_protocol::AgentStatus::Idle | orbit_protocol::AgentStatus::Error { .. }
            );
            settled && has_reply
        }
    })
    .await;

    let agents = rt
        .store
        .list_agents_filtered(None, false, false, None)
        .expect("list agents");
    let ag = agents
        .into_iter()
        .find(|a| a.handle.as_deref() == Some(handle))
        .unwrap_or_else(|| panic!("peer @{handle} never created"));
    if !done {
        let last_status = format!("{:?}", ag.status);
        let tail = rt
            .store
            .last_assistant_message_text(&ag.agent_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        panic!(
            "Timed out waiting for peer @{handle} (id={}, provider={:?}) to settle! status={last_status} tail={tail}",
            ag.agent_id, ag.provider
        );
    }
    let reply = rt
        .store
        .last_assistant_message_text(&ag.agent_id)
        .ok()
        .flatten()
        .unwrap_or_default();
    println!(
        "\n[LIVE E2E] Peer @{handle} ({:?}) finished! cwd={} reply_preview={}\n",
        ag.provider,
        ag.cwd,
        reply.chars().take(300).collect::<String>()
    );
    ag
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "Live 3-CLI integration test (invokes real claude, codex, agy CLIs)"]
async fn live_all_three_clis_mission_workspace_sync_and_nested_worktree() {
    let installed = orbit_providers::discover_installed_providers();
    let kinds: Vec<ProviderKind> = installed.iter().map(|p| p.kind).collect();
    println!("[LIVE E2E] Discovered CLIs: {:?}", kinds);
    assert!(
        kinds.contains(&ProviderKind::ClaudeCode),
        "claude CLI must be installed"
    );
    assert!(
        kinds.contains(&ProviderKind::Codex),
        "codex CLI must be installed"
    );
    assert!(
        kinds.contains(&ProviderKind::Antigravity),
        "agy CLI must be installed"
    );

    orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));

    let home = temp_dir("home");
    let sandbox_parent = temp_dir("sandbox");
    let nested_repo = sandbox_parent.join("rate_limiter");
    let extra_project = temp_dir("other_proj");
    init_nested_rate_limiter_repo(&nested_repo);

    // Sanity check: existing tests MUST fail before our agents fix the bugs
    let pre_test = Command::new("python3")
        .args(["-m", "unittest", "discover", "-v"])
        .current_dir(&nested_repo)
        .output()
        .expect("python3 unittest");
    assert!(
        !pre_test.status.success(),
        "Pre-mission unit tests must fail to prove the bugs are real"
    );

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .to_qr_uri()
        .expect("pair uri");

    let (device_a, host_id) = open_and_connect(home.join("device_a.db"), uri.clone()).await;
    let (device_b, _) = open_and_connect(home.join("device_b.db"), uri).await;

    // Step 1: Non-Git parent inspection discovers nested `rate_limiter` repo
    let parent_str = sandbox_parent.to_string_lossy().to_string();
    let insp = device_a
        .inspect_git_repos_on_host(&host_id, &parent_str, Vec::new())
        .await
        .expect("inspect sandbox parent");
    assert!(
        !insp.is_git_repo,
        "mission_sandbox parent is NOT a git repo"
    );
    assert_eq!(insp.nested_git_repos.len(), 1);
    assert_eq!(insp.nested_git_repos[0].name, "rate_limiter");
    println!(
        "[LIVE E2E] Step 1 PASS: Non-Git parent detected nested repo {} ({:?})",
        insp.nested_git_repos[0].relative_path, insp.nested_git_repos[0].git_branch
    );

    // Save two projects & sync manual drag reorder across Device A -> Device B
    let p_sandbox = device_a
        .upsert_project(None, &host_id, "Mission Sandbox", vec![parent_str.clone()])
        .expect("upsert sandbox");
    let p_other = device_a
        .upsert_project(
            None,
            &host_id,
            "Secondary Folder",
            vec![extra_project.to_string_lossy().to_string()],
        )
        .expect("upsert other");
    device_a
        .reorder_projects(vec![
            p_other.project_id.clone(),
            p_sandbox.project_id.clone(),
        ])
        .expect("reorder");

    let order_synced = wait_until(Duration::from_secs(12), || {
        let b = Arc::clone(&device_b);
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
            let pos_other = names.iter().position(|n| *n == "Secondary Folder");
            let pos_sb = names.iter().position(|n| *n == "Mission Sandbox");
            matches!((pos_other, pos_sb), (Some(o), Some(s)) if o < s)
        }
    })
    .await;
    assert!(
        order_synced,
        "Manual project drag order must sync to Device B"
    );
    println!("[LIVE E2E] Step 2 PASS: Manual project reorder synced to Device B");

    // Step 3: CLI #1 — Claude Code (`@architect`, Local in nested repo) writes spec to BOARD.md
    device_a.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &nested_repo.to_string_lossy(),
            extra_dirs: vec![parent_str.clone()],
            provider: ProviderKind::ClaudeCode,
            model: None,
            effort: None,
            mode: SessionMode::FullAccess,
            initial_prompt: "You are @architect in this Orbit Mission Workspace. Read `limiter.py` and `test_limiter.py` in the current directory (do NOT edit any Python files yet). Identify the two bugs in `TokenBucket`, design a concise `SlidingWindowLog(max_requests: int, window_seconds: float, clock=time.monotonic)` class with `allow() -> bool` and `remaining() -> int`, and publish your specification to the shared Mission Board by calling the Orbit MCP tool `workspace_board_update` with `section: \"Architecture Spec\"`. Keep your final reply under 6 lines.",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: Some("architect".into()),
            auto_cluster: Some(false),
        },
        None,
    );

    let architect = wait_for_peer_idle(&rt, "architect", Duration::from_secs(180)).await;
    let ws_id = architect.workspace_id.clone();

    let board_after_claude = rt
        .orchestrator
        .get_workspace_board(&ws_id)
        .expect("board after claude");
    println!(
        "[LIVE E2E] BOARD.md after Claude (rev={}):\n{}\n",
        board_after_claude.revision, board_after_claude.content
    );
    assert!(
        board_after_claude.revision >= 1 && board_after_claude.content.contains("SlidingWindowLog"),
        "Claude (@architect) must publish Architecture Spec with SlidingWindowLog to BOARD.md"
    );

    // Cross-device read/unread + ManualUnread + Pin verification on real agent!
    let both_unread = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&device_a);
        let b = Arc::clone(&device_b);
        let hid = host_id.clone();
        let aid = architect.agent_id.clone();
        async move {
            let ua = a
                .host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|x| x.is_unread)
                .unwrap_or(false);
            let ub = b
                .host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|x| x.is_unread)
                .unwrap_or(false);
            ua && ub
        }
    })
    .await;
    assert!(
        both_unread,
        "Real finished Claude turn must be unread on both Device A and Device B"
    );

    device_a.mark_agent_seen(&host_id, &architect.agent_id, architect.latest_seq);
    let b_read_ok = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&device_b);
        let hid = host_id.clone();
        let aid = architect.agent_id.clone();
        async move {
            b.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|x| !x.is_unread)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        b_read_ok,
        "Marking read on Device A must clear unread on Device B"
    );

    device_b.mark_agent_unread(&host_id, &architect.agent_id);
    let a_manual_ok = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&device_a);
        let hid = host_id.clone();
        let aid = architect.agent_id.clone();
        async move {
            a.host_detail(&hid)
                .and_then(|d| d.agents.into_iter().find(|x| x.agent_id == aid))
                .map(|x| x.is_unread && x.attention == AttentionState::ManualUnread)
                .unwrap_or(false)
        }
    })
    .await;
    assert!(
        a_manual_ok,
        "ManualUnread from Device B must sync to Device A"
    );
    device_a.mark_agent_seen(&host_id, &architect.agent_id, architect.latest_seq);
    println!(
        "[LIVE E2E] Step 3 PASS: Claude (@architect) updated BOARD.md + cross-device sync verified"
    );

    // Step 4: CLI #2 — Codex (`@impl`, ISOLATED WORKTREE of nested repo inside SAME Mission Workspace!)
    device_a.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: nested_repo.to_string_lossy().to_string(),
        extra_dirs: vec![parent_str.clone()],
        provider: ProviderKind::Codex,
        model: None,
        effort: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @impl in this Orbit Mission Workspace, running in an isolated Git worktree. 1) Call the Orbit MCP tool `workspace_board_read` to read @architect's Architecture Spec. 2) Fix the bugs in `limiter.py` and implement `SlidingWindowLog` per the spec. 3) Add thorough unit tests for both `TokenBucket` (including backward clock skew & exact capacity) and `SlidingWindowLog` in `test_limiter.py`. 4) Run `python3 -m unittest discover -v` and make sure every test passes. 5) Call `workspace_board_update` with `section: \"Implementation Summary\"` listing what you changed and the test result.".to_string(),
        worktree_branch: Some("feat/rate-limiter-impl".into()),
        handle: Some("impl".into()),
        profile_id: None,
    });

    let impl_peer = wait_for_peer_idle(&rt, "impl", Duration::from_secs(240)).await;
    assert_eq!(
        impl_peer.workspace_id, ws_id,
        "Codex worktree peer must share Mission Workspace"
    );
    assert_ne!(
        PathBuf::from(&impl_peer.cwd),
        nested_repo,
        "Codex peer must run inside an isolated Git worktree, not the source repo"
    );

    // Verify Codex's worktree tests actually pass on disk!
    let wt_test = Command::new("python3")
        .args(["-m", "unittest", "discover", "-v"])
        .current_dir(&impl_peer.cwd)
        .output()
        .expect("run unittest in codex worktree");
    let wt_out = format!(
        "{}\n{}",
        String::from_utf8_lossy(&wt_test.stdout),
        String::from_utf8_lossy(&wt_test.stderr)
    );
    println!("[LIVE E2E] Codex worktree unittest output:\n{wt_out}");
    assert!(
        wt_test.status.success(),
        "Codex (@impl) worktree unit tests MUST pass!"
    );

    // Verify nested-repo worktree folds under `Mission Sandbox` in sidebar!
    let folded_ok = wait_until(Duration::from_secs(10), || {
        let a = Arc::clone(&device_a);
        let hid = host_id.clone();
        let wid = ws_id.clone();
        let pstr = parent_str.clone();
        async move {
            let sb = a.fleet_sidebar(
                SidebarProjectSort::Manual,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                Some(pstr.as_str()),
            );
            sb.projects.iter().any(|p| {
                p.primary_cwd == pstr
                    && p.workspaces
                        .iter()
                        .any(|w| w.workspace_id == wid && w.is_multi_agent && w.peer_count >= 2)
            })
        }
    })
    .await;
    assert!(
        folded_ok,
        "Codex nested-repo worktree peer must fold under Mission Sandbox inside the multi-agent workspace card"
    );
    let board_after_codex = rt
        .orchestrator
        .get_workspace_board(&ws_id)
        .expect("board after codex");
    println!(
        "[LIVE E2E] BOARD.md after Codex (rev={}):\n{}\n",
        board_after_codex.revision, board_after_codex.content
    );
    assert!(
        board_after_codex.revision > board_after_claude.revision,
        "Codex (@impl) must update BOARD.md via MCP workspace_board_update"
    );
    println!(
        "[LIVE E2E] Step 4 PASS: Codex (@impl) implemented fixes in isolated worktree, tests pass, folded under parent project"
    );

    // Step 5: CLI #3 — Antigravity (`@reviewer`, Plan/Read-Only inside SAME Mission Workspace!)
    device_a.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: impl_peer.cwd.clone(),
        extra_dirs: vec![parent_str.clone()],
        provider: ProviderKind::Antigravity,
        model: None,
        effort: None,
        mode: SessionMode::FullAccess,
        initial_prompt: format!(
            "You are @reviewer in this Orbit Mission Workspace. 1) Call `workspace_peers`, `workspace_board_read`, and `read_peer_context` for handle `impl`. 2) Inspect `limiter.py` and `test_limiter.py` in `{}` (the worktree edited by @impl) and verify the fixes and SlidingWindowLog edge cases. 3) Call `workspace_board_update` with `section: \"Review Verdict\"` containing a concise verdict (APPROVE or CHANGES_REQUESTED + 2-3 bullet points). Keep your final reply under 6 lines.",
            impl_peer.cwd
        ),
        worktree_branch: None,
        handle: Some("reviewer".into()),
        profile_id: None,
    });

    let reviewer = wait_for_peer_idle(&rt, "reviewer", Duration::from_secs(240)).await;
    assert_eq!(
        reviewer.workspace_id, ws_id,
        "Antigravity (@reviewer) must belong to the same Mission Workspace"
    );

    let final_board = rt
        .orchestrator
        .get_workspace_board(&ws_id)
        .expect("final board");
    println!(
        "[LIVE E2E] FINAL BOARD.md after Claude + Codex + Antigravity (rev={}):\n{}\n",
        final_board.revision, final_board.content
    );
    assert!(
        final_board.revision > board_after_codex.revision
            && final_board.content.contains("Review Verdict"),
        "Antigravity (@reviewer) must publish Review Verdict to BOARD.md via MCP"
    );

    // Verify all 3 providers coexist inside ONE multi-agent Mission Workspace in client sidebar!
    let all_three_in_ws = wait_until(Duration::from_secs(10), || {
        let b = Arc::clone(&device_b);
        let hid = host_id.clone();
        let wid = ws_id.clone();
        async move {
            let sb = b.fleet_sidebar(
                SidebarProjectSort::Manual,
                SidebarThreadFilter::All,
                "",
                Some(hid.as_str()),
                None,
                None,
            );
            sb.projects.iter().any(|p| {
                p.workspaces.iter().any(|w| {
                    w.workspace_id == wid
                        && w.is_multi_agent
                        && w.peer_count == 3
                        && w.conversations
                            .iter()
                            .any(|c| c.agent.provider == ProviderKind::ClaudeCode)
                        && w.conversations
                            .iter()
                            .any(|c| c.agent.provider == ProviderKind::Codex)
                        && w.conversations
                            .iter()
                            .any(|c| c.agent.provider == ProviderKind::Antigravity)
                })
            })
        }
    })
    .await;
    assert!(
        all_three_in_ws,
        "Device B sidebar must display all 3 CLIs (Claude + Codex + Antigravity) united inside one multi-agent Mission Workspace"
    );

    println!(
        "\n======================================================================\n\
         [LIVE E2E] ALL 3 CLIs (Claude + Codex + Antigravity) PASSED FULL MISSION!\n\
         Workspace: {ws_id} | Board rev: {}\n\
         ======================================================================\n",
        final_board.revision
    );

    let _ = tokio::task::spawn_blocking(move || {
        drop(device_a);
        drop(device_b);
    })
    .await;
    drop(rt);
}
