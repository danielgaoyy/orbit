#![cfg(unix)]
//! LIVE 3-CLI Multi-Agent Parallel Collaboration & Prompt Cache Benchmark (`claude` + `codex` + `agy`).
//!
//! Marked `#[ignore]` so offline CI never invokes external LLM providers.
//! Execute locally against all 3 real installed CLIs sequentially:
//!   `TMPDIR=/tmp cargo test -p orbit-cli --test e2e_workspace_collab_benchmark -- --ignored --nocapture --test-threads=1`
//!
//! Safety:
//! - Uses isolated temporary `ORBIT_HOME` and temporary git repositories (never touches `~/.orbit`).
//! - Uses `orbit_shim::set_test_parent_pid_watchdog` and `Drop` shim cleanup so zero child processes leak.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use orbit_client_core::app_core::{CreateAgentRequest, OrbitAppCore};
use orbit_client_core::contract::SpawnWorkspacePeerRequest;
use orbit_daemon::DaemonRuntime;
use orbit_daemon::spawn_embedded_daemon;
use orbit_daemon::tools::list_tool_definitions_for_caller;
use orbit_daemon::workspace_board::build_workspace_roster_context_incremental;
use orbit_protocol::{
    AgentStatus, AgentSummary, McpToolPolicy, ProviderKind, ReasoningEffort, SessionMode,
    WorkspaceEventKind, WorkspaceIsolation,
};
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
    let dir = std::env::temp_dir().join(format!("ob_{prefix}_{uid}"));
    std::fs::create_dir_all(&dir).expect("mkdir temp");
    TempDirGuard(dir)
}

fn ensure_all_three_clis_installed() {
    let installed = orbit_providers::discover_installed_providers();
    let kinds: Vec<ProviderKind> = installed.iter().map(|p| p.kind).collect();
    assert!(
        kinds.contains(&ProviderKind::ClaudeCode),
        "claude CLI must be installed: {kinds:?}"
    );
    assert!(
        kinds.contains(&ProviderKind::Codex),
        "codex CLI must be installed: {kinds:?}"
    );
    assert!(
        kinds.contains(&ProviderKind::Antigravity),
        "agy CLI must be installed: {kinds:?}"
    );
}

fn git_ok(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {:?} failed in {}: {}",
        args,
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_git_repo(repo: &Path, files: &[(&str, &str)], initial_commit: &str) {
    std::fs::create_dir_all(repo).expect("mkdir repo");
    for (rel, content) in files {
        let target = repo.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("mkdir parent");
        }
        std::fs::write(&target, content).expect("write initial repo file");
    }
    git_ok(repo, &["init", "-b", "main"]);
    git_ok(repo, &["config", "core.autocrlf", "false"]);
    git_ok(repo, &["config", "user.email", "benchmark@orbit.local"]);
    git_ok(repo, &["config", "user.name", "Orbit Benchmark"]);
    git_ok(repo, &["add", "-A"]);
    git_ok(repo, &["commit", "-m", initial_commit]);
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
    assert!(ok, "core must connect to embedded daemon");
    (core, hid)
}

async fn create_curated_workspace_and_wait(
    rt: &DaemonRuntime,
    core: &Arc<OrbitAppCore>,
    host_id: &str,
    cwd: &Path,
    title: &str,
    goal: &str,
) -> String {
    let cwd_str = cwd.to_string_lossy().to_string();
    core.create_mission_workspace(
        host_id,
        &cwd_str,
        title.to_string(),
        Some(goal.to_string()),
        WorkspaceIsolation::Local,
        None,
        None,
    );
    let title_owned = title.to_string();
    let ok = wait_until(Duration::from_secs(15), || {
        let store = rt.store.clone();
        let t = title_owned.clone();
        async move {
            store
                .list_workspaces_filtered(false, false)
                .ok()
                .is_some_and(|list| list.iter().any(|w| w.title == t && w.is_curated))
        }
    })
    .await;
    assert!(ok, "curated workspace `{title}` must be created");
    rt.store
        .list_workspaces_filtered(false, false)
        .expect("list workspaces")
        .into_iter()
        .find(|w| w.title == title && w.is_curated)
        .expect("find created workspace")
        .workspace_id
}

fn format_workspace_agents_diag(rt: &DaemonRuntime, ws_id: Option<&str>) -> String {
    let agents = rt
        .store
        .list_agents_filtered(ws_id, true, true, None)
        .unwrap_or_default();
    if agents.is_empty() {
        return format!("(no agents in ws={ws_id:?})");
    }
    agents
        .into_iter()
        .map(|a| {
            format!(
                "{}(handle={:?}, provider={:?}, status={:?}, seq={}, cwd={})",
                a.agent_id, a.handle, a.provider, a.status, a.latest_seq, a.cwd
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

async fn wait_for_agent_settled(
    rt: &DaemonRuntime,
    agent_id: Option<&str>,
    ws_id: Option<&str>,
    handle: Option<&str>,
    min_seq_gt: u64,
    timeout: Duration,
) -> AgentSummary {
    let ok = wait_until(timeout, || {
        let store = rt.store.clone();
        let aid = agent_id.map(ToOwned::to_owned);
        let wid = ws_id.map(ToOwned::to_owned);
        let h = handle.map(ToOwned::to_owned);
        async move {
            let found = if let Some(ref id) = aid {
                store.get_agent(id).ok().flatten()
            } else {
                store
                    .list_agents_filtered(wid.as_deref(), false, false, None)
                    .ok()
                    .and_then(|list| {
                        list.into_iter().find(|a| {
                            h.as_deref()
                                .is_none_or(|hh| a.handle.as_deref() == Some(hh))
                        })
                    })
            };
            let Some(ag) = found else {
                return false;
            };
            if ag.latest_seq <= min_seq_gt {
                return false;
            }
            let settled = matches!(
                ag.status,
                AgentStatus::Idle | AgentStatus::Error { .. } | AgentStatus::Closed
            );
            if !settled {
                return false;
            }
            if matches!(ag.status, AgentStatus::Error { .. }) {
                return true;
            }
            store
                .last_assistant_message_text(&ag.agent_id)
                .ok()
                .flatten()
                .is_some_and(|t| !t.trim().is_empty())
        }
    })
    .await;

    let ag = if let Some(id) = agent_id {
        rt.store
            .get_agent(id)
            .expect("get agent")
            .unwrap_or_else(|| {
                panic!(
                    "agent `{id}` never appeared; workspace_agents=[{}]",
                    format_workspace_agents_diag(rt, ws_id)
                )
            })
    } else {
        rt.store
            .list_agents_filtered(ws_id, false, false, None)
            .expect("list agents")
            .into_iter()
            .find(|a| handle.is_none_or(|hh| a.handle.as_deref() == Some(hh)))
            .unwrap_or_else(|| {
                panic!(
                    "peer @{handle:?} never appeared; workspace_agents=[{}]",
                    format_workspace_agents_diag(rt, ws_id)
                )
            })
    };
    if !ok {
        let tail = rt
            .store
            .last_assistant_message_text(&ag.agent_id)
            .ok()
            .flatten()
            .unwrap_or_default();
        panic!(
            "Timed out waiting for agent {} (handle={:?}, provider={:?})! status={:?}, latest_seq={}, tail={} | workspace_agents=[{}]",
            ag.agent_id,
            ag.handle,
            ag.provider,
            ag.status,
            ag.latest_seq,
            tail,
            format_workspace_agents_diag(rt, ws_id),
        );
    }
    if let AgentStatus::Error { ref message, .. } = ag.status {
        panic!(
            "Agent {} (handle={:?}, provider={:?}) ended in Error: {message}",
            ag.agent_id, ag.handle, ag.provider
        );
    }
    let reply = rt
        .store
        .last_assistant_message_text(&ag.agent_id)
        .ok()
        .flatten()
        .unwrap_or_default();
    println!(
        "[LIVE SETTLED] @{} ({}, {:?}) seq={} cwd={} reply_preview={}",
        ag.handle.as_deref().unwrap_or("peer"),
        ag.agent_id,
        ag.provider,
        ag.latest_seq,
        ag.cwd,
        reply
            .chars()
            .take(180)
            .collect::<String>()
            .replace('\n', " ")
    );
    ag
}

async fn wait_for_agents_registered(
    rt: &DaemonRuntime,
    ws_id: &str,
    handles: &[&str],
    timeout: Duration,
) {
    let expected: Vec<String> = handles.iter().map(|s| (*s).to_string()).collect();
    let ok = wait_until(timeout, || {
        let store = rt.store.clone();
        let wid = ws_id.to_string();
        let exp = expected.clone();
        async move {
            let Ok(list) = store.list_agents_filtered(Some(&wid), false, false, None) else {
                return false;
            };
            exp.iter()
                .all(|h| list.iter().any(|a| a.handle.as_deref() == Some(h.as_str())))
        }
    })
    .await;
    if !ok {
        panic!(
            "Expected peers {handles:?} to register in workspace {ws_id}; actual=[{}]",
            format_workspace_agents_diag(rt, Some(ws_id))
        );
    }
}

#[derive(Default)]
struct AgentCacheTotals {
    provider: String,
    handle: String,
    model: String,
    turns: usize,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    cost_usd: f64,
    savings_usd: f64,
}

async fn print_collab_and_cache_scorecard(
    title: &str,
    rt: &DaemonRuntime,
    ws_id: &str,
    since_ms: i64,
    elapsed: Duration,
) {
    // Allow async background usage ledger writes a brief moment to flush to SQLite.
    tokio::time::sleep(Duration::from_millis(800)).await;

    let agents = rt
        .store
        .list_agents_filtered(Some(ws_id), true, true, None)
        .unwrap_or_default();
    let ws_agent_ids: HashSet<String> = agents.iter().map(|a| a.agent_id.clone()).collect();
    let handle_by_id: BTreeMap<String, String> = agents
        .iter()
        .map(|a| {
            (
                a.agent_id.clone(),
                a.handle.clone().unwrap_or_else(|| a.title.clone()),
            )
        })
        .collect();

    let entries: Vec<_> = rt
        .store
        .list_usage_entries_since(since_ms)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| ws_agent_ids.contains(&e.agent_id))
        .collect();

    let mut per_agent: BTreeMap<String, AgentCacheTotals> = BTreeMap::new();
    println!(
        "\n==================== SCORECARD: {title} ({:.1}s) ====================",
        elapsed.as_secs_f64()
    );
    println!("[Per-Turn Usage & Prompt Cache Ledger]");
    for (idx, e) in entries.iter().enumerate() {
        let prompt_total = e.input_tokens + e.cache_read_tokens + e.cache_write_tokens;
        let hit_pct = if prompt_total > 0 {
            (e.cache_read_tokens as f64 / prompt_total as f64) * 100.0
        } else {
            0.0
        };
        let handle = handle_by_id
            .get(&e.agent_id)
            .map(String::as_str)
            .unwrap_or(&e.agent_id);
        println!(
            "  #{idx:>2} @{handle:<12} provider={:<11} model={:<22} in={:<7} out={:<6} cache_read={:<8} cache_write={:<7} hit={:>5.1}% saved=${:.4} cost=${:.4}",
            e.provider,
            e.model,
            e.input_tokens,
            e.output_tokens,
            e.cache_read_tokens,
            e.cache_write_tokens,
            hit_pct,
            e.cache_savings_usd,
            e.estimated_cost_usd
        );

        let agg = per_agent
            .entry(e.agent_id.clone())
            .or_insert_with(|| AgentCacheTotals {
                provider: e.provider.clone(),
                handle: handle.to_string(),
                model: e.model.clone(),
                ..Default::default()
            });
        agg.turns += 1;
        agg.input_tokens += e.input_tokens;
        agg.output_tokens += e.output_tokens;
        agg.cache_read_tokens += e.cache_read_tokens;
        agg.cache_write_tokens += e.cache_write_tokens;
        agg.cost_usd += e.estimated_cost_usd;
        agg.savings_usd += e.cache_savings_usd;
    }

    println!("[Per-Agent Totals]");
    for (aid, tot) in &per_agent {
        let prompt_total = tot.input_tokens + tot.cache_read_tokens + tot.cache_write_tokens;
        let hit_pct = if prompt_total > 0 {
            (tot.cache_read_tokens as f64 / prompt_total as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "  @{:<12} ({aid}) provider={:<11} model={:<20} turns={} in={} out={} cache_read={} cache_write={} hit={:.1}% saved=${:.4} cost=${:.4}",
            tot.handle,
            tot.provider,
            tot.model,
            tot.turns,
            tot.input_tokens,
            tot.output_tokens,
            tot.cache_read_tokens,
            tot.cache_write_tokens,
            hit_pct,
            tot.savings_usd,
            tot.cost_usd
        );
    }

    let events = rt
        .store
        .list_workspace_events(ws_id, 200)
        .unwrap_or_default();
    let count_kind =
        |kind: WorkspaceEventKind| -> usize { events.iter().filter(|e| e.kind == kind).count() };
    let board = rt.orchestrator.get_workspace_board(ws_id).ok();
    println!(
        "[Workspace Events & Board] ws={ws_id} | BoardUpdated={} PeerMessage={} ConflictWarning={} FileEdited={} WorkspaceMerged={} | board_rev={}",
        count_kind(WorkspaceEventKind::BoardUpdated),
        count_kind(WorkspaceEventKind::PeerMessage),
        count_kind(WorkspaceEventKind::ConflictWarning),
        count_kind(WorkspaceEventKind::FileEdited),
        count_kind(WorkspaceEventKind::WorkspaceMerged),
        board.as_ref().map(|b| b.revision).unwrap_or(0),
    );
    if let Some(b) = board
        && !b.content.trim().is_empty()
    {
        println!("[BOARD.md snapshot]\n{}\n", b.content.trim());
    }
    println!("========================================================================\n");
}

fn run_python_unittest(repo: &Path) -> (bool, String) {
    let out = Command::new("python3")
        .args(["-m", "unittest", "discover", "-v"])
        .current_dir(repo)
        .output()
        .expect("run python3 unittest");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), combined)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "Live benchmark test invoking real claude, codex, agy CLIs"]
async fn live_00_normal_chat_unblocked_and_cached() {
    ensure_all_three_clis_installed();
    orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));

    let started = Instant::now();
    let since_ms = chrono::Utc::now().timestamp_millis();
    let home = temp_dir("home_chat");
    let chat_dir = temp_dir("chat_dir");

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn embedded daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .to_qr_uri()
        .expect("pair uri");
    let (core, host_id) = open_and_connect(home.join("client.db"), uri).await;

    core.create_agent_with_profile(
        CreateAgentRequest {
            host_id: &host_id,
            cwd: &chat_dir.to_string_lossy(),
            extra_dirs: Vec::new(),
            provider: ProviderKind::ClaudeCode,
            acp_agent_id: None,
            model: None,
            effort: None,
            fast_mode: None,
            mode: SessionMode::FullAccess,
            initial_prompt: "Explain in 2 short sentences why Rust prevents data races at compile time. Plain Q&A only; do not read/edit files or call any tools.",
            initial_display_prompt: None,
            initial_badges: Vec::new(),
            worktree_branch: None,
            workspace_id: None,
            handle: None,
            auto_cluster: Some(false),
        },
        None,
    );

    let t1 = wait_for_agent_settled(&rt, None, None, None, 0, Duration::from_secs(120)).await;
    assert_eq!(
        t1.labels.get("orbit.mcp_packs").map(String::as_str),
        Some("core"),
        "Normal standalone chat must lock only the `core` capability pack"
    );
    let defs = list_tool_definitions_for_caller(&rt, Some(&t1.agent_id), &McpToolPolicy::default());
    assert_eq!(
        defs.len(),
        11,
        "Normal standalone chat MCP catalog must stay strictly at 11 Core tools"
    );

    let ws = rt
        .store
        .get_workspace(&t1.workspace_id)
        .expect("get ws")
        .expect("ws exists");
    assert!(
        !ws.is_curated,
        "Standalone chat workspace must remain uncurated"
    );
    assert!(
        build_workspace_roster_context_incremental(
            &rt.store,
            &rt.home_dir,
            &ws,
            &t1.agent_id,
            std::slice::from_ref(&t1),
            &[],
            None,
            None
        )
        .is_none(),
        "Standalone chat must inject zero <orbit-workspace> XML"
    );

    // Turn 2 & Turn 3: follow-up Q&A turns without stalls
    core.send_prompt(
        &host_id,
        &t1.agent_id,
        "Give one 1-sentence example contrasting Send vs Sync. Plain Q&A only; no tools.",
    );
    let t2 = wait_for_agent_settled(
        &rt,
        Some(&t1.agent_id),
        None,
        None,
        t1.latest_seq,
        Duration::from_secs(120),
    )
    .await;

    core.send_prompt(
        &host_id,
        &t1.agent_id,
        "Respond with a 1-sentence takeaway starting with `Summary:`. Plain Q&A only; no tools.",
    );
    let t3 = wait_for_agent_settled(
        &rt,
        Some(&t1.agent_id),
        None,
        None,
        t2.latest_seq,
        Duration::from_secs(120),
    )
    .await;
    let reply3 = rt
        .store
        .last_assistant_message_text(&t3.agent_id)
        .expect("read t3")
        .unwrap_or_default();
    assert!(
        !reply3.trim().is_empty(),
        "Turn 3 reply must be non-empty: {reply3}"
    );

    let ws_events = rt
        .store
        .list_workspace_events(&t1.workspace_id, 50)
        .expect("ws events");
    assert!(
        ws_events.is_empty(),
        "Standalone chat must emit zero unsolicited workspace/conflict events: {ws_events:?}"
    );

    print_collab_and_cache_scorecard(
        "live_00_normal_chat_unblocked_and_cached",
        &rt,
        &t1.workspace_id,
        since_ms,
        started.elapsed(),
    )
    .await;

    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
    drop(rt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "Live benchmark test invoking real claude, codex, agy CLIs"]
async fn live_01_three_agent_contract_pipeline() {
    ensure_all_three_clis_installed();
    orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));

    let started = Instant::now();
    let since_ms = chrono::Utc::now().timestamp_millis();
    let home = temp_dir("home_p1");
    let repo = temp_dir("repo_p1");
    init_git_repo(
        &repo,
        &[("README.md", "# TaskRunner Benchmark\n")],
        "initial taskrunner scaffold",
    );

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .to_qr_uri()
        .expect("pair uri");
    let (core, host_id) = open_and_connect(home.join("client.db"), uri).await;

    let repo_str = repo.to_string_lossy().to_string();
    let ws_id = create_curated_workspace_and_wait(
        &rt,
        &core,
        &host_id,
        &repo,
        "TaskRunner 3-CLI Pipeline",
        "Build models.py, engine.py, formatter.py, and test_app.py cleanly via MCP coordination",
    )
    .await;

    // Launch @backend (Claude) & @cli (Codex) first, then @reviewer (Antigravity)
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::ClaudeCode,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @backend in this Orbit Mission Workspace (owning ONLY `models.py` and `engine.py` — do NOT edit `formatter.py` or `test_app.py`). 1) Implement `@dataclass class Task(id: int, title: str, priority: int = 1, done: bool = False)` in `models.py` and `class TaskStore` in `engine.py` (`add(title: str, priority: int = 1) -> Task`, `complete(task_id: int) -> Task`, `pending() -> list[Task]` sorted by priority asc then id asc). 2) Call Orbit MCP `workspace_board` (`action=\"read\"`) once to get the current revision, then call `workspace_board` with `action=\"update\"`, `section=\"Decisions & Contracts\"`, `expected_revision=<rev>` (if conflict, read `current_board.revision` from the error and retry once without extra loops) documenting the exact signatures. 3) Send ONE informational ping via `message_peer(handle=\"cli\", message=\"models.py & engine.py contract ready\", notify_on_finish=false)` and finish your turn immediately.".into(),
        worktree_branch: None,
        handle: Some("backend".into()),
        profile_id: None,
    });

    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Codex,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @cli in this Orbit Mission Workspace (owning ONLY `formatter.py` — do NOT edit `models.py`, `engine.py`, or `test_app.py`). 1) Inspect `workspace_peers` and `workspace_board(action=\"read\")`. Implement `formatter.py` with `format_task(task) -> str` (using `.id`, `.title`, `.priority`, `.done` from `Task`) and `render_summary(tasks: list) -> str` returning multi-line text with header + counts. 2) Call Orbit MCP `workspace_board(action=\"update\", section=\"Tasks\", content=...)` documenting `formatter.py` API. 3) Send ONE brief ping via `message_peer(handle=\"reviewer\", message=\"formatter.py ready\", notify_on_finish=false)` and keep final reply under 5 lines.".into(),
        worktree_branch: None,
        handle: Some("cli".into()),
        profile_id: None,
    });

    wait_for_agents_registered(&rt, &ws_id, &["backend", "cli"], Duration::from_secs(20)).await;

    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Antigravity,
        acp_agent_id: None,
        model: None,
        effort: None,
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @reviewer in this Orbit Mission Workspace (owning `test_app.py` integration tests & final QA). 1) Inspect `workspace_peers` and `workspace_board(action=\"read\")`. Check if `@backend` (`models.py`, `engine.py`) and `@cli` (`formatter.py`) have both settled and written all three modules. If either coding peer is still running or any file is missing, call `orbit_timer(action=\"create\", duration_seconds=90, condition=\"all_peers\", prompt=\"All coding peers settled. Inspect models.py, engine.py, formatter.py, write unittest coverage in test_app.py, run python3 -m unittest discover -v, and update workspace_board(action=\\\"update\\\", section=\\\"Findings\\\").\")` and end your turn cleanly. 2) Once all three files exist, write `test_app.py`, run `python3 -m unittest discover -v` until all tests pass, and update `workspace_board(action=\"update\", section=\"Findings\", content=...)`.".into(),
        worktree_branch: None,
        handle: Some("reviewer".into()),
        profile_id: None,
    });

    let _backend = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("backend"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let _cli = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("cli"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let reviewer = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("reviewer"),
        0,
        Duration::from_secs(360),
    )
    .await;

    // Wait up to 120s for any armed `all_peers` barrier wakeup turn on `@reviewer` if tests aren't written yet.
    if !repo.join("test_app.py").is_file() || !run_python_unittest(&repo).0 {
        let ready = wait_until(Duration::from_secs(120), || {
            let r = repo.to_path_buf();
            let store = rt.store.clone();
            let rev_id = reviewer.agent_id.clone();
            async move {
                let settled = store
                    .get_agent(&rev_id)
                    .ok()
                    .flatten()
                    .is_some_and(|a| matches!(a.status, AgentStatus::Idle));
                settled && r.join("test_app.py").is_file() && run_python_unittest(&r).0
            }
        })
        .await;
        if !ready {
            let prev_seq = rt
                .store
                .get_agent(&reviewer.agent_id)
                .ok()
                .flatten()
                .map(|a| a.latest_seq)
                .unwrap_or(reviewer.latest_seq);
            core.send_prompt(
                &host_id,
                &reviewer.agent_id,
                "All peers have settled. Write/fix `test_app.py`, ensure `python3 -m unittest discover -v` passes all tests, and update `workspace_board(action=\"update\", section=\"Findings\")`.",
            );
            let _ = wait_for_agent_settled(
                &rt,
                Some(&reviewer.agent_id),
                None,
                None,
                prev_seq,
                Duration::from_secs(240),
            )
            .await;
        }
    }

    assert!(repo.join("models.py").is_file(), "models.py must exist");
    assert!(repo.join("engine.py").is_file(), "engine.py must exist");
    assert!(
        repo.join("formatter.py").is_file(),
        "formatter.py must exist"
    );
    assert!(repo.join("test_app.py").is_file(), "test_app.py must exist");

    let (tests_ok, test_out) = run_python_unittest(&repo);
    println!("[LIVE P1] python unittest output:\n{test_out}");
    assert!(tests_ok, "Pipeline unit tests must pass:\n{test_out}");

    let board = rt
        .orchestrator
        .get_workspace_board(&ws_id)
        .expect("get board");
    assert!(
        board.revision >= 3,
        "Expected all 3 peers to update BOARD.md (rev={})",
        board.revision
    );

    print_collab_and_cache_scorecard(
        "live_01_three_agent_contract_pipeline",
        &rt,
        &ws_id,
        since_ms,
        started.elapsed(),
    )
    .await;

    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
    drop(rt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "Live benchmark test invoking real claude, codex, agy CLIs"]
async fn live_02_three_agent_worktree_conflict_merge() {
    ensure_all_three_clis_installed();
    orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));

    let started = Instant::now();
    let since_ms = chrono::Utc::now().timestamp_millis();
    let home = temp_dir("home_wt");
    let repo = temp_dir("repo_wt");

    let init_calc = r#""""Simple calculator module for Orbit multi-worktree merge test."""


def add(a: int, b: int) -> int:
    return a + b


def sub(a: int, b: int) -> int:
    return a - b
"#;
    let init_test = r#"import unittest
from calculator import add, sub


class TestCalculator(unittest.TestCase):
    def test_add_and_sub(self) -> None:
        self.assertEqual(add(2, 3), 5)
        self.assertEqual(sub(5, 2), 3)


if __name__ == "__main__":
    unittest.main()
"#;
    init_git_repo(
        &repo,
        &[
            ("calculator.py", init_calc),
            ("test_calculator.py", init_test),
        ],
        "initial calculator",
    );

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .to_qr_uri()
        .expect("pair uri");
    let (core, host_id) = open_and_connect(home.join("client.db"), uri).await;

    let repo_str = repo.to_string_lossy().to_string();
    let ws_id = create_curated_workspace_and_wait(
        &rt,
        &core,
        &host_id,
        &repo,
        "Worktree Conflict Merge Arena",
        "Merge two parallel overlapping worktree features cleanly via handoff_worktree --3way",
    )
    .await;

    // Two parallel Worktree peers inside the SAME Mission Workspace editing overlapping tails!
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Codex,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @feat-a running in an isolated Git worktree. 1) Append `mul(a: int, b: int) -> int` and `div(a: int, b: int) -> float` (raising `ZeroDivisionError` on `b == 0`) to `calculator.py`, and add unit tests `test_mul` and `test_div` to `test_calculator.py`. 2) Run `python3 -m unittest discover -v` inside your worktree until all tests pass. 3) Record completion via `workspace_board(action=\"update\", section=\"Tasks\")`. Keep final reply under 5 lines.".into(),
        worktree_branch: Some("feat/calc-mul-div".into()),
        handle: Some("feat-a".into()),
        profile_id: None,
    });

    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Antigravity,
        acp_agent_id: None,
        model: None,
        effort: None,
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @feat-b running in an isolated Git worktree. 1) Append `pow_int(base: int, exp: int) -> int` and `mod_int(a: int, b: int) -> int` to `calculator.py`, and add unit tests `test_pow_and_mod` to `test_calculator.py`. 2) Run `python3 -m unittest discover -v` inside your worktree until all tests pass. 3) Record completion via `workspace_board(action=\"update\", section=\"Findings\")`. Keep final reply under 5 lines.".into(),
        worktree_branch: Some("feat/calc-pow-mod".into()),
        handle: Some("feat-b".into()),
        profile_id: None,
    });

    wait_for_agents_registered(&rt, &ws_id, &["feat-a", "feat-b"], Duration::from_secs(30)).await;

    // Spawn @integrator in the local checkout to wait on all_peers and merge both worktrees!
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::ClaudeCode,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @integrator in the local checkout (do NOT edit Python files before sibling worktrees finish). 1) Inspect `workspace_peers`. If `@feat-a` or `@feat-b` is still running, call `orbit_timer(action=\"create\", duration_seconds=150, condition=\"all_peers\", prompt=\"Both @feat-a and @feat-b worktree peers have settled! Merge both worktrees into this local checkout: call `handoff_worktree(agent_id=\\\"feat-a\\\", direction=\\\"worktree_to_local\\\", mode=\\\"apply\\\", allow_three_way_conflicts=true)` and then `handoff_worktree(agent_id=\\\"feat-b\\\", direction=\\\"worktree_to_local\\\", mode=\\\"apply\\\", allow_three_way_conflicts=true)`. Resolve any `<<<<<<<` / `=======` / `>>>>>>>` 3-way conflict markers in `calculator.py` and `test_calculator.py` so ALL functions (`add`, `sub`, `mul`, `div`, `pow_int`, `mod_int`) and tests exist, run `python3 -m unittest discover -v`, call `workspace_board(action=\\\"update\\\", section=\\\"Findings\\\", content=\\\"Merged @feat-a and @feat-b cleanly; all unittest cases pass.\\\")` ONCE without rewriting other sections, and finish immediately.\")` and end this turn immediately without waiting in bash. 2) If both `@feat-a` and `@feat-b` are already idle right now, merge both immediately as instructed above.".into(),
        worktree_branch: None,
        handle: Some("integrator".into()),
        profile_id: None,
    });

    let fa = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("feat-a"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let fb = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("feat-b"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let repo_canon = std::fs::canonicalize(&*repo).unwrap_or_else(|_| repo.to_path_buf());
    let fa_canon = std::fs::canonicalize(&fa.cwd).unwrap_or_else(|_| PathBuf::from(&fa.cwd));
    let fb_canon = std::fs::canonicalize(&fb.cwd).unwrap_or_else(|_| PathBuf::from(&fb.cwd));
    assert_ne!(
        fa_canon, repo_canon,
        "@feat-a must run in its own isolated git worktree"
    );
    assert_ne!(
        fb_canon, repo_canon,
        "@feat-b must run in its own isolated git worktree"
    );
    assert_ne!(
        fa_canon, fb_canon,
        "@feat-a and @feat-b must have distinct isolated worktree directories"
    );

    let integrator = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("integrator"),
        0,
        Duration::from_secs(540),
    )
    .await;

    let merged_ok = wait_until(Duration::from_secs(150), || {
        let r = repo.to_path_buf();
        let store = rt.store.clone();
        let iid = integrator.agent_id.clone();
        async move {
            let settled = store
                .get_agent(&iid)
                .ok()
                .flatten()
                .is_some_and(|a| matches!(a.status, AgentStatus::Idle));
            if !settled {
                return false;
            }
            let calc = std::fs::read_to_string(r.join("calculator.py")).unwrap_or_default();
            calc.contains("def mul")
                && calc.contains("def div")
                && calc.contains("def pow_int")
                && calc.contains("def mod_int")
                && !calc.contains("<<<<<<<")
                && run_python_unittest(&r).0
        }
    })
    .await;

    if !merged_ok {
        let prev_seq = rt
            .store
            .get_agent(&integrator.agent_id)
            .ok()
            .flatten()
            .map(|a| a.latest_seq)
            .unwrap_or(integrator.latest_seq);
        core.send_prompt(
            &host_id,
            &integrator.agent_id,
            "Both @feat-a and @feat-b have settled. Call `handoff_worktree(agent_id=\"feat-a\", direction=\"worktree_to_local\", mode=\"apply\", allow_three_way_conflicts=true)` and `handoff_worktree(agent_id=\"feat-b\", direction=\"worktree_to_local\", mode=\"apply\", allow_three_way_conflicts=true)`, resolve all conflict markers in `calculator.py` and `test_calculator.py`, run `python3 -m unittest discover -v`, and update `workspace_board`.",
        );
        let _ = wait_for_agent_settled(
            &rt,
            Some(&integrator.agent_id),
            None,
            None,
            prev_seq,
            Duration::from_secs(240),
        )
        .await;
    }

    let final_calc = std::fs::read_to_string(repo.join("calculator.py")).expect("read calculator");
    let final_test =
        std::fs::read_to_string(repo.join("test_calculator.py")).expect("read test_calculator");
    println!("[LIVE P2] Merged calculator.py:\n{final_calc}");
    assert!(
        !final_calc.contains("<<<<<<<") && !final_test.contains("<<<<<<<"),
        "All 3-way conflict markers must be cleanly resolved"
    );
    for sym in [
        "def add",
        "def sub",
        "def mul",
        "def div",
        "def pow_int",
        "def mod_int",
    ] {
        assert!(
            final_calc.contains(sym),
            "Merged calculator.py missing `{sym}`:\n{final_calc}"
        );
    }

    let (tests_ok, test_out) = run_python_unittest(&repo);
    println!("[LIVE P2] python unittest output:\n{test_out}");
    assert!(
        tests_ok,
        "Merged local calculator unit tests must pass:\n{test_out}"
    );

    print_collab_and_cache_scorecard(
        "live_02_three_agent_worktree_conflict_merge",
        &rt,
        &ws_id,
        since_ms,
        started.elapsed(),
    )
    .await;

    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
    drop(rt);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "Live benchmark test invoking real claude, codex, agy CLIs"]
async fn live_03_same_dir_conflict_radar_and_cache_endurance() {
    ensure_all_three_clis_installed();
    orbit_shim::set_test_parent_pid_watchdog(Some(std::process::id()));

    let started = Instant::now();
    let since_ms = chrono::Utc::now().timestamp_millis();
    let home = temp_dir("home_cr");
    let repo = temp_dir("repo_cr");

    let init_cfg = "TIMEOUT_SECS = 5\nRETRIES = 2\n";
    let init_test = r#"import unittest
import config


class TestConfig(unittest.TestCase):
    def test_defaults(self) -> None:
        self.assertGreater(config.TIMEOUT_SECS, 0)
        self.assertGreaterEqual(config.RETRIES, 1)


if __name__ == "__main__":
    unittest.main()
"#;
    init_git_repo(
        &repo,
        &[("config.py", init_cfg), ("test_config.py", init_test)],
        "initial config",
    );

    let (addr, rt) = spawn_embedded_daemon(home.to_path_buf(), None)
        .await
        .expect("spawn daemon");
    let uri = rt
        .build_pairing_offer(vec![format!("ws://{addr}/ws")], None)
        .to_qr_uri()
        .expect("pair uri");
    let (core, host_id) = open_and_connect(home.join("client.db"), uri).await;

    let repo_str = repo.to_string_lossy().to_string();
    let ws_id = create_curated_workspace_and_wait(
        &rt,
        &core,
        &host_id,
        &repo,
        "Same-Dir Radar & Multi-Turn Cache",
        "Exercise same-directory edits, @all broadcast, Conflict Radar, and multi-turn cache hit rate",
    )
    .await;

    // Turn 1: Same-dir devs + reviewer in ONE local repo
    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::ClaudeCode,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @dev-a in this local Mission Workspace. Edit `config.py` to add `BASE_URL = \"https://api.orbit.local\"` and `LOG_LEVEL = \"INFO\"` while preserving existing constants. Run `python3 -m unittest discover -v` and update `workspace_board(action=\"update\", section=\"Tasks\")`. Keep final reply under 4 lines.".into(),
        worktree_branch: None,
        handle: Some("dev-a".into()),
        profile_id: None,
    });

    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Codex,
        acp_agent_id: None,
        model: None,
        effort: Some(ReasoningEffort::Low),
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @dev-b in this local Mission Workspace. Edit `config.py` carefully (preserve lines added by `@dev-a`) to add `MAX_CONNECTIONS = 16`, update `test_config.py` to assert `MAX_CONNECTIONS >= 1`, run `python3 -m unittest discover -v`, and message `@reviewer` via `message_peer(handle=\"reviewer\", message=\"...\", notify_on_finish=false)`. Keep final reply under 4 lines.".into(),
        worktree_branch: None,
        handle: Some("dev-b".into()),
        profile_id: None,
    });

    core.spawn_workspace_peer(SpawnWorkspacePeerRequest {
        host_id: host_id.clone(),
        workspace_id: ws_id.clone(),
        cwd: repo_str.clone(),
        extra_dirs: Vec::new(),
        provider: ProviderKind::Antigravity,
        acp_agent_id: None,
        model: None,
        effort: None,
        fast_mode: None,
        mode: SessionMode::FullAccess,
        initial_prompt: "You are @reviewer in this local Mission Workspace. Check `workspace_peers` and `config.py`, and report the current constants in `workspace_board(action=\"update\", section=\"Findings\")`. Keep final reply under 4 lines.".into(),
        worktree_branch: None,
        handle: Some("reviewer".into()),
        profile_id: None,
    });

    wait_for_agents_registered(
        &rt,
        &ws_id,
        &["dev-a", "dev-b", "reviewer"],
        Duration::from_secs(30),
    )
    .await;

    let dev_a_1 = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("dev-a"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let dev_b_1 = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("dev-b"),
        0,
        Duration::from_secs(360),
    )
    .await;
    let rev_1 = wait_for_agent_settled(
        &rt,
        None,
        Some(&ws_id),
        Some("reviewer"),
        0,
        Duration::from_secs(360),
    )
    .await;

    // Turn 2: `@all` broadcast + multi-turn prompt cache test!
    core.send_prompt(
        &host_id,
        &dev_a_1.agent_id,
        "@all Spec update: @dev-a rename `RETRIES` to `MAX_RETRIES = 3` in `config.py`; @dev-b update `test_config.py` to use `MAX_RETRIES` (keeping tests passing); @reviewer verify once both finish.",
    );

    let dev_a_2 = wait_for_agent_settled(
        &rt,
        Some(&dev_a_1.agent_id),
        None,
        Some("dev-a"),
        dev_a_1.latest_seq,
        Duration::from_secs(360),
    )
    .await;
    let dev_b_2 = wait_for_agent_settled(
        &rt,
        Some(&dev_b_1.agent_id),
        None,
        Some("dev-b"),
        dev_b_1.latest_seq,
        Duration::from_secs(360),
    )
    .await;
    let rev_2 = wait_for_agent_settled(
        &rt,
        Some(&rev_1.agent_id),
        None,
        Some("reviewer"),
        rev_1.latest_seq,
        Duration::from_secs(360),
    )
    .await;

    // Turn 3: Final cache-hit endurance turn & verification
    core.send_prompt(
        &host_id,
        &dev_a_2.agent_id,
        "Run `python3 -m unittest discover -v` right now, ensure every constant and test is consistent, and reply with 2 concise bullet points.",
    );
    core.send_prompt(
        &host_id,
        &dev_b_2.agent_id,
        "Verify `config.py` and `test_config.py` with `python3 -m unittest discover -v` and reply in 2 lines.",
    );
    core.send_prompt(
        &host_id,
        &rev_2.agent_id,
        "Read `config.py` and `test_config.py`, run `python3 -m unittest discover -v`, and post final APPROVE/FAIL status to `workspace_board`.",
    );

    let _dev_a_3 = wait_for_agent_settled(
        &rt,
        Some(&dev_a_2.agent_id),
        None,
        Some("dev-a"),
        dev_a_2.latest_seq,
        Duration::from_secs(240),
    )
    .await;
    let _dev_b_3 = wait_for_agent_settled(
        &rt,
        Some(&dev_b_2.agent_id),
        None,
        Some("dev-b"),
        dev_b_2.latest_seq,
        Duration::from_secs(240),
    )
    .await;
    let _rev_3 = wait_for_agent_settled(
        &rt,
        Some(&rev_2.agent_id),
        None,
        Some("reviewer"),
        rev_2.latest_seq,
        Duration::from_secs(240),
    )
    .await;

    let (tests_ok, test_out) = run_python_unittest(&repo);
    println!("[LIVE P3] python unittest output:\n{test_out}");
    assert!(
        tests_ok,
        "Same-dir config unit tests must pass:\n{test_out}"
    );

    let events = rt.store.list_workspace_events(&ws_id, 200).expect("events");
    let conflict_warnings = events
        .iter()
        .filter(|e| e.kind == WorkspaceEventKind::ConflictWarning)
        .count();
    let file_edits = events
        .iter()
        .filter(|e| e.kind == WorkspaceEventKind::FileEdited)
        .count();
    println!(
        "[LIVE P3] Observed workspace telemetry: file_edits={file_edits}, conflict_warnings={conflict_warnings}"
    );
    assert!(
        conflict_warnings >= 1 || file_edits >= 2,
        "Same-dir edits to config.py/test_config.py must be tracked by File Conflict Radar: {events:?}"
    );

    print_collab_and_cache_scorecard(
        "live_03_same_dir_conflict_radar_and_cache_endurance",
        &rt,
        &ws_id,
        since_ms,
        started.elapsed(),
    )
    .await;

    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
    drop(rt);
}
