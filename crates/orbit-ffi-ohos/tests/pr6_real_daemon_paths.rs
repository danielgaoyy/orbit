#![cfg(unix)]

use orbit_daemon::{DaemonConfig, DaemonRuntime, run_daemon_until};
use orbit_ohos::OhosCoreFacade;
use orbit_protocol::{
    ExternalMcpServerRecord, HostProjectEntry, ManagedSkillRecord, ProjectMemoryItem,
};
use orbit_store::OrbitStore;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let seq = NEXT_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("{prefix}_{pid}_{seq}"));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn write_memory_md(project_dir: &Path, id: &str, title: &str, status: &str) -> PathBuf {
    let mem_dir = project_dir.join(".orbit").join("memory");
    std::fs::create_dir_all(&mem_dir).expect("mkdir .orbit/memory");
    let file_path = mem_dir.join(format!("{id}.md"));
    let body = format!(
        "---\nid: {id}\ntitle: {title}\ncategory: convention\nstatus: {status}\nconfidence: 0.9\nfile_paths: []\ntags: []\ncreated_at_ms: 1760000000000\nupdated_at_ms: 1760000000000\n---\nBody for {title}.\n"
    );
    std::fs::write(&file_path, body).expect("write memory md");
    file_path
}

fn write_dot_mcp(project_dir: &Path, server_name: &str) {
    let file_path = project_dir.join(".mcp.json");
    let content = format!(
        r#"{{"mcpServers":{{"{server_name}":{{"command":"echo","args":["{server_name}"]}}}}}}"#
    );
    std::fs::write(&file_path, content).expect("write .mcp.json");
}

fn write_project_skill(project_dir: &Path, slug: &str, name: &str) {
    let skill_dir = project_dir.join(".orbit").join("skills").join(slug);
    std::fs::create_dir_all(&skill_dir).expect("mkdir skill");
    let content =
        format!("---\nname: {name}\ndescription: Skill {name}\n---\nInstructions for {name}.\n");
    std::fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
}

struct LiveTestDaemon {
    host_id: String,
    facade: OhosCoreFacade,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    _base: PathBuf,
}

impl LiveTestDaemon {
    async fn start(base: PathBuf, projects: &[(&str, &Path)]) -> Self {
        let home = base.join("orbit_home");
        std::fs::create_dir_all(&home).expect("mkdir orbit_home");

        let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("probe bind");
        let addr: SocketAddr = probe.local_addr().expect("local addr");
        drop(probe);

        let rt = DaemonRuntime::bootstrap(&home)
            .await
            .expect("bootstrap daemon");
        let host_id = rt.host_info.host_id.clone();
        let pubkey = rt.keypair.public_key_b64.clone();
        let secret = rt.pairing_secret.clone();
        drop(rt);

        // Register projects in daemon SQLite store so authorization allows each exact path
        let store = OrbitStore::open(home.join("orbit.db")).expect("open daemon store");
        for (idx, (name, dir)) in projects.iter().enumerate() {
            store
                .upsert_host_project(&HostProjectEntry {
                    project_id: format!("proj_{idx}"),
                    name: (*name).to_string(),
                    directories: vec![dir.to_string_lossy().to_string()],
                    sort_order: idx as i64,
                    hidden: false,
                    updated_at_ms: 1,
                })
                .expect("upsert host project");
        }
        drop(store);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let home_bg = home.clone();
        tokio::spawn(async move {
            let _ = run_daemon_until(
                DaemonConfig {
                    home_dir: home_bg,
                    listen_addr: addr,
                    relay_url: None,
                },
                None,
                async move {
                    let _ = shutdown_rx.await;
                },
            )
            .await;
        });

        // Wait until TCP port accepts connections
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut ready = false;
        while tokio::time::Instant::now() < deadline {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        assert!(ready, "test daemon failed to bind on {addr}");

        let facade = OhosCoreFacade::open_in_memory().expect("open facade");
        let offer = orbit_crypto::PairingOffer {
            host_id: host_id.clone(),
            hostname: "TestHost".into(),
            daemon_pubkey_b64: pubkey,
            direct_endpoints: vec![format!("ws://{addr}/ws")],
            relay_url: None,
            pairing_secret: secret,
        };
        let uri = offer.to_qr_uri().expect("qr uri");
        facade.pair_host_json(&uri).expect("pair host");
        facade.connect_host(&host_id);

        // Wait until facade reports online_direct
        let connect_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        let mut connected = false;
        while tokio::time::Instant::now() < connect_deadline {
            let overview = facade.fleet_overview_json().unwrap_or_default();
            if overview.contains("online_direct") {
                connected = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(connected, "facade did not connect to local test daemon");

        Self {
            host_id,
            facade,
            shutdown_tx: Some(shutdown_tx),
            _base: base,
        }
    }
}

impl Drop for LiveTestDaemon {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        let _ = std::fs::remove_dir_all(&self._base);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr6_trailing_space_project_isolates_memory_mcp_and_skills_on_real_daemon() {
    let base = unique_temp_dir("orbit_pr6_space_dir");
    let plain_dir = base.join("project");
    let space_dir = base.join("project ");
    std::fs::create_dir_all(&plain_dir).expect("mkdir plain");
    std::fs::create_dir_all(&space_dir).expect("mkdir space");

    let plain_mem = write_memory_md(&plain_dir, "alpha-mem", "alpha-plain", "hypothesis");
    let space_mem = write_memory_md(&space_dir, "alpha-mem", "alpha-space", "hypothesis");
    write_dot_mcp(&plain_dir, "mcp-plain");
    write_dot_mcp(&space_dir, "mcp-space");
    write_project_skill(&plain_dir, "skill-plain", "Skill Plain");
    write_project_skill(&space_dir, "skill-space", "Skill Space");

    let plain_str = plain_dir.to_string_lossy().to_string();
    let space_str = space_dir.to_string_lossy().to_string();

    let daemon = LiveTestDaemon::start(
        base.clone(),
        &[("plain", &plain_dir), ("space", &space_dir)],
    )
    .await;
    let host_id = daemon.host_id.clone();
    let facade = daemon.facade.clone();

    // Run blocking facade RPC calls on spawn_blocking so Tokio worker stays free
    let (
        plain_mem_out,
        space_mem_out,
        plain_mcp_out,
        space_mcp_out,
        plain_skills_out,
        space_skills_out,
    ) = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let plain_str = plain_str.clone();
        let space_str = space_str.clone();
        move || {
            let p_mem: Vec<ProjectMemoryItem> = serde_json::from_str(
                &facade
                    .list_project_memories_json(&host_id, &plain_str, 50)
                    .expect("list plain mem"),
            )
            .expect("parse plain mem");
            let s_mem: Vec<ProjectMemoryItem> = serde_json::from_str(
                &facade
                    .list_project_memories_json(&host_id, &space_str, 50)
                    .expect("list space mem"),
            )
            .expect("parse space mem");
            let p_mcp: Vec<ExternalMcpServerRecord> = serde_json::from_str(
                &facade
                    .list_external_mcp_servers_json(&host_id, Some(plain_str.clone()))
                    .expect("list plain mcp"),
            )
            .expect("parse plain mcp");
            let s_mcp: Vec<ExternalMcpServerRecord> = serde_json::from_str(
                &facade
                    .list_external_mcp_servers_json(&host_id, Some(space_str.clone()))
                    .expect("list space mcp"),
            )
            .expect("parse space mcp");
            let p_sk: Vec<ManagedSkillRecord> = serde_json::from_str(
                &facade
                    .list_managed_skills_json(&host_id, Some(plain_str.clone()))
                    .expect("list plain skills"),
            )
            .expect("parse plain skills");
            let s_sk: Vec<ManagedSkillRecord> = serde_json::from_str(
                &facade
                    .list_managed_skills_json(&host_id, Some(space_str.clone()))
                    .expect("list space skills"),
            )
            .expect("parse space skills");
            (p_mem, s_mem, p_mcp, s_mcp, p_sk, s_sk)
        }
    })
    .await
    .expect("join list calls");

    assert_eq!(plain_mem_out.len(), 1);
    assert_eq!(plain_mem_out[0].title, "alpha-plain");
    assert_eq!(space_mem_out.len(), 1);
    assert_eq!(space_mem_out[0].title, "alpha-space");

    assert!(plain_mcp_out.iter().any(|m| m.name == "mcp-plain"));
    assert!(!plain_mcp_out.iter().any(|m| m.name == "mcp-space"));
    assert!(space_mcp_out.iter().any(|m| m.name == "mcp-space"));
    assert!(!space_mcp_out.iter().any(|m| m.name == "mcp-plain"));

    assert!(plain_skills_out.iter().any(|s| s.slug == "skill-plain"));
    assert!(!plain_skills_out.iter().any(|s| s.slug == "skill-space"));
    assert!(space_skills_out.iter().any(|s| s.slug == "skill-space"));
    assert!(!space_skills_out.iter().any(|s| s.slug == "skill-plain"));

    // Confirm memory strictly in `project ` — MUST NOT touch `project` on disk!
    tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let space_str = space_str.clone();
        move || {
            facade
                .confirm_project_memory_json(&host_id, "alpha-mem", Some(space_str))
                .expect("confirm space mem");
        }
    })
    .await
    .expect("join confirm");

    let plain_disk = std::fs::read_to_string(&plain_mem).expect("read plain md");
    let space_disk = std::fs::read_to_string(&space_mem).expect("read space md");
    assert!(
        plain_disk.contains("status: hypothesis"),
        "plain md must remain hypothesis: {plain_disk}"
    );
    assert!(
        space_disk.contains("status: confirmed"),
        "space md must be confirmed: {space_disk}"
    );

    // Delete memory strictly in `project ` — `project` file MUST remain intact, `project ` deleted!
    tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let space_str = space_str.clone();
        move || {
            facade
                .delete_project_memory_json(&host_id, "alpha-mem", Some(space_str))
                .expect("delete space mem");
        }
    })
    .await
    .expect("join delete");

    assert!(
        plain_mem.exists(),
        "plain directory memory file must still exist"
    );
    assert!(
        !space_mem.exists(),
        "space directory memory file must be deleted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr6_two_arg_delete_rejects_ambiguous_same_id_across_east_and_west_preserving_disk() {
    let base = unique_temp_dir("orbit_pr6_east_west");
    let east_dir = base.join("east");
    let west_dir = base.join("west");
    std::fs::create_dir_all(&east_dir).expect("mkdir east");
    std::fs::create_dir_all(&west_dir).expect("mkdir west");

    let east_mem = write_memory_md(&east_dir, "shared", "East Shared", "hypothesis");
    let west_mem = write_memory_md(&west_dir, "shared", "West Shared", "hypothesis");

    let east_str = east_dir.to_string_lossy().to_string();
    let west_str = west_dir.to_string_lossy().to_string();

    let daemon =
        LiveTestDaemon::start(base.clone(), &[("east", &east_dir), ("west", &west_dir)]).await;
    let host_id = daemon.host_id.clone();
    let facade = daemon.facade.clone();

    let ambig_err = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_str = east_str.clone();
        let west_str = west_str.clone();
        move || {
            let _ = facade
                .list_project_memories_json(&host_id, &east_str, 50)
                .expect("list east");
            let _ = facade
                .list_project_memories_json(&host_id, &west_str, 50)
                .expect("list west");
            // 2-arg delete without explicit cwd MUST fail closed as ambiguous
            facade
                .delete_project_memory_json(&host_id, "shared", None)
                .expect_err("ambiguous 2-arg delete must fail")
        }
    })
    .await
    .expect("join ambig delete");

    assert!(
        ambig_err.contains("ambiguous"),
        "expected ambiguity error, got: {ambig_err}"
    );
    assert!(
        east_mem.exists(),
        "east memory file must remain intact on disk"
    );
    assert!(
        west_mem.exists(),
        "west memory file must remain intact on disk"
    );

    // Explicit cwd delete removes ONLY west's file while preserving east's file
    tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let west_str = west_str.clone();
        move || {
            facade
                .delete_project_memory_json(&host_id, "shared", Some(west_str))
                .expect("explicit west delete");
        }
    })
    .await
    .expect("join explicit delete");

    assert!(
        east_mem.exists(),
        "east memory file must still exist after deleting west"
    );
    assert!(!west_mem.exists(), "west memory file must be deleted");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr6_evicted_ambiguous_memory_and_single_side_relist_stay_fail_closed_on_real_daemon() {
    // Part A: east + west `original-pair` -> 9 filler projects × 240 (2160) evict it ->
    // re-list ONLY west -> 2-arg confirm & delete MUST fail closed with both disk files byte-identical.
    let base = unique_temp_dir("orbit_pr6_evict_ambig");
    let east_dir = base.join("east");
    let west_dir = base.join("west");
    std::fs::create_dir_all(&east_dir).expect("mkdir east");
    std::fs::create_dir_all(&west_dir).expect("mkdir west");

    let east_mem = write_memory_md(&east_dir, "original-pair", "East Pair", "hypothesis");
    let west_mem = write_memory_md(&west_dir, "original-pair", "West Pair", "hypothesis");
    let east_orig_bytes = std::fs::read(&east_mem).expect("read east bytes");
    let west_orig_bytes = std::fs::read(&west_mem).expect("read west bytes");

    let mut filler_dirs: Vec<(String, PathBuf)> = Vec::new();
    for p in 0..9 {
        let name = format!("fill_{p}");
        let dir = base.join(&name);
        std::fs::create_dir_all(&dir).expect("mkdir filler");
        for i in 0..240 {
            let _ = write_memory_md(
                &dir,
                &format!("f-{p}-{i}"),
                &format!("Fill {p}-{i}"),
                "hypothesis",
            );
        }
        filler_dirs.push((name, dir));
    }

    let mut reg_projects: Vec<(&str, &Path)> = vec![("east", &east_dir), ("west", &west_dir)];
    for (name, dir) in &filler_dirs {
        reg_projects.push((name.as_str(), dir.as_path()));
    }

    let daemon = LiveTestDaemon::start(base.clone(), &reg_projects).await;
    let host_id = daemon.host_id.clone();
    let facade = daemon.facade.clone();
    let east_str = east_dir.to_string_lossy().to_string();
    let west_str = west_dir.to_string_lossy().to_string();
    let filler_strs: Vec<String> = filler_dirs
        .iter()
        .map(|(_, d)| d.to_string_lossy().to_string())
        .collect();

    let (confirm_err, delete_err) = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_str = east_str.clone();
        let west_str = west_str.clone();
        let filler_strs = filler_strs.clone();
        move || {
            let _ = facade
                .list_project_memories_json(&host_id, &east_str, 250)
                .expect("list east");
            let _ = facade
                .list_project_memories_json(&host_id, &west_str, 250)
                .expect("list west");
            // Initial 2-arg calls must fail as ambiguous
            assert!(
                facade
                    .confirm_project_memory_json(&host_id, "original-pair", None)
                    .is_err()
            );
            assert!(
                facade
                    .delete_project_memory_json(&host_id, "original-pair", None)
                    .is_err()
            );

            // Push 2160 distinct keys through public list RPC to evict `original-pair` from main LRU
            for f_cwd in &filler_strs {
                let _ = facade
                    .list_project_memories_json(&host_id, f_cwd, 250)
                    .expect("list filler");
            }

            // Re-list ONLY west — 2-arg confirm & delete MUST STILL fail closed!
            let _ = facade
                .list_project_memories_json(&host_id, &west_str, 250)
                .expect("relist west");
            let c_err = facade
                .confirm_project_memory_json(&host_id, "original-pair", None)
                .expect_err("2-arg confirm must fail closed after eviction + single-side relist");
            let d_err = facade
                .delete_project_memory_json(&host_id, "original-pair", None)
                .expect_err("2-arg delete must fail closed after eviction + single-side relist");
            (c_err, d_err)
        }
    })
    .await
    .expect("join part A");

    assert!(confirm_err.contains("ambiguous"), "got: {confirm_err}");
    assert!(delete_err.contains("ambiguous"), "got: {delete_err}");
    assert_eq!(
        std::fs::read(&east_mem).expect("east bytes"),
        east_orig_bytes
    );
    assert_eq!(
        std::fs::read(&west_mem).expect("west bytes"),
        west_orig_bytes
    );

    // Explicit 3-arg confirm & delete on west still succeed and leave east byte-identical
    tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let west_str = west_str.clone();
        move || {
            facade
                .confirm_project_memory_json(&host_id, "original-pair", Some(west_str.clone()))
                .expect("explicit west confirm");
            facade
                .delete_project_memory_json(&host_id, "original-pair", Some(west_str))
                .expect("explicit west delete");
        }
    })
    .await
    .expect("join explicit part A");

    assert_eq!(
        std::fs::read(&east_mem).expect("east intact"),
        east_orig_bytes
    );
    assert!(!west_mem.exists(), "west must be deleted via explicit cwd");

    // Part B: fresh daemon -> list ONLY east -> evict via 2160 entries -> list ONLY west ->
    // 2-arg delete MUST STILL fail closed and keep both files byte-identical on disk!
    let base2 = unique_temp_dir("orbit_pr6_evict_late_west");
    let east2 = base2.join("east");
    let west2 = base2.join("west");
    std::fs::create_dir_all(&east2).expect("mkdir east2");
    std::fs::create_dir_all(&west2).expect("mkdir west2");
    let east2_mem = write_memory_md(&east2, "late-pair", "East Late", "hypothesis");
    let west2_mem = write_memory_md(&west2, "late-pair", "West Late", "hypothesis");
    let east2_bytes = std::fs::read(&east2_mem).expect("east2 bytes");
    let west2_bytes = std::fs::read(&west2_mem).expect("west2 bytes");
    let mut fill2: Vec<(String, PathBuf)> = Vec::new();
    for p in 0..9 {
        let name = format!("fill_{p}");
        let dir = base2.join(&name);
        std::fs::create_dir_all(&dir).expect("mkdir fill2");
        for i in 0..240 {
            let _ = write_memory_md(
                &dir,
                &format!("g-{p}-{i}"),
                &format!("G {p}-{i}"),
                "hypothesis",
            );
        }
        fill2.push((name, dir));
    }
    let mut reg2: Vec<(&str, &Path)> = vec![("east", &east2), ("west", &west2)];
    for (name, dir) in &fill2 {
        reg2.push((name.as_str(), dir.as_path()));
    }
    let daemon2 = LiveTestDaemon::start(base2.clone(), &reg2).await;
    let host2 = daemon2.host_id.clone();
    let facade2 = daemon2.facade.clone();
    let east2_str = east2.to_string_lossy().to_string();
    let west2_str = west2.to_string_lossy().to_string();
    let fill2_strs: Vec<String> = fill2
        .iter()
        .map(|(_, d)| d.to_string_lossy().to_string())
        .collect();

    let late_err = tokio::task::spawn_blocking(move || {
        let _ = facade2
            .list_project_memories_json(&host2, &east2_str, 250)
            .expect("list east2 only");
        for f_cwd in &fill2_strs {
            let _ = facade2
                .list_project_memories_json(&host2, f_cwd, 250)
                .expect("list fill2");
        }
        let _ = facade2
            .list_project_memories_json(&host2, &west2_str, 250)
            .expect("list west2 only");
        facade2
            .delete_project_memory_json(&host2, "late-pair", None)
            .expect_err("2-arg delete after prior eviction + second workspace must fail closed")
    })
    .await
    .expect("join part B");

    assert!(late_err.contains("ambiguous"), "got: {late_err}");
    assert_eq!(std::fs::read(&east2_mem).expect("east2 bytes"), east2_bytes);
    assert_eq!(std::fs::read(&west2_mem).expect("west2 bytes"), west2_bytes);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr6_replayed_two_arg_delete_after_listing_other_workspace_preserves_west_on_real_daemon() {
    let base = unique_temp_dir("orbit_pr6_delete_replay");
    let east_dir = base.join("east");
    let west_dir = base.join("west");
    std::fs::create_dir_all(&east_dir).expect("mkdir east");
    std::fs::create_dir_all(&west_dir).expect("mkdir west");

    let east_mem = write_memory_md(&east_dir, "replay-id", "East Replay", "hypothesis");
    let west_mem = write_memory_md(&west_dir, "replay-id", "West Replay", "hypothesis");
    let west_orig_bytes = std::fs::read(&west_mem).expect("read west bytes");

    let daemon =
        LiveTestDaemon::start(base.clone(), &[("east", &east_dir), ("west", &west_dir)]).await;
    let host_id = daemon.host_id.clone();
    let facade = daemon.facade.clone();
    let east_str = east_dir.to_string_lossy().to_string();
    let west_str = west_dir.to_string_lossy().to_string();

    let (immediate_retry_err, after_west_list_err) = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_str = east_str.clone();
        let west_str = west_str.clone();
        move || {
            // 1. List ONLY east, then 2-arg delete succeeds on east
            let _ = facade
                .list_project_memories_json(&host_id, &east_str, 50)
                .expect("list east");
            facade
                .delete_project_memory_json(&host_id, "replay-id", None)
                .expect("initial 2-arg delete on east");

            // 2. Immediate replay of 2-arg delete fails (cwd unknown)
            let err1 = facade
                .delete_project_memory_json(&host_id, "replay-id", None)
                .expect_err("immediate replay must fail");

            // 3. List west containing same ID `replay-id`
            let west_json = facade
                .list_project_memories_json(&host_id, &west_str, 50)
                .expect("list west");
            assert!(west_json.contains("West Replay"), "got: {west_json}");

            // 4. Replaying the exact same 2-arg delete MUST fail closed as ambiguous!
            let err2 = facade
                .delete_project_memory_json(&host_id, "replay-id", None)
                .expect_err("replayed 2-arg delete after listing west must fail closed");
            (err1, err2)
        }
    })
    .await
    .expect("join replay test");

    assert!(
        immediate_retry_err.contains("unknown"),
        "got: {immediate_retry_err}"
    );
    assert!(
        after_west_list_err.contains("ambiguous"),
        "got: {after_west_list_err}"
    );
    assert!(!east_mem.exists(), "east file must have been deleted");
    assert!(west_mem.exists(), "west file must still exist on disk");
    assert_eq!(
        std::fs::read(&west_mem).expect("west bytes intact"),
        west_orig_bytes
    );

    // 5. Explicit 3-arg delete targeting west still succeeds
    tokio::task::spawn_blocking(move || {
        facade
            .delete_project_memory_json(&host_id, "replay-id", Some(west_str))
            .expect("explicit west delete must succeed");
    })
    .await
    .expect("join explicit west delete");

    assert!(!west_mem.exists(), "explicit west delete removes west file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pr6_explicit_slash_delete_revokes_cache_and_same_dir_relist_succeeds_on_real_daemon() {
    let base = unique_temp_dir("orbit_pr6_slash_and_backslash");
    let east_dir = base.join("east");
    let east_bs_dir = base.join("east\\");
    std::fs::create_dir_all(&east_dir).expect("mkdir east");
    std::fs::create_dir_all(&east_bs_dir).expect("mkdir east backslash");

    let slash_a_path = write_memory_md(&east_dir, "slash-a", "Slash A v1", "hypothesis");
    let slash_b_path = write_memory_md(&east_dir, "slash-b", "Slash B v1", "hypothesis");
    let bs_plain_path = write_memory_md(&east_dir, "bs-shared", "Plain East", "hypothesis");
    let bs_slash_path = write_memory_md(&east_bs_dir, "bs-shared", "Backslash East", "hypothesis");
    let bs_plain_orig = std::fs::read(&bs_plain_path).expect("read plain bs bytes");
    let bs_slash_orig = std::fs::read(&bs_slash_path).expect("read backslash bs bytes");

    let daemon = LiveTestDaemon::start(
        base.clone(),
        &[("east", &east_dir), ("east_bs", &east_bs_dir)],
    )
    .await;
    let host_id = daemon.host_id.clone();
    let facade = daemon.facade.clone();
    let east_slash_str = format!("{}/", east_dir.to_string_lossy());
    let east_plain_str = east_dir.to_string_lossy().to_string();
    let east_bs_str = east_bs_dir.to_string_lossy().to_string();

    // Part A (R4-P1-2): list `east/`, explicit delete `slash-a` via `east/` ->
    // restore `slash-a` WITHOUT re-list -> 2-arg delete MUST fail (cwd unknown) & preserve disk!
    tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_slash_str = east_slash_str.clone();
        move || {
            let listed = facade
                .list_project_memories_json(&host_id, &east_slash_str, 50)
                .expect("list east/");
            assert!(listed.contains("Slash A v1"), "got: {listed}");
            assert!(listed.contains("Slash B v1"), "got: {listed}");
            facade
                .delete_project_memory_json(&host_id, "slash-a", Some(east_slash_str.clone()))
                .expect("explicit slash-a delete via east/");
            facade
                .delete_project_memory_json(&host_id, "slash-b", Some(east_slash_str))
                .expect("explicit slash-b delete via east/");
        }
    })
    .await
    .expect("join initial slash deletes");

    assert!(!slash_a_path.exists(), "slash-a deleted");
    assert!(!slash_b_path.exists(), "slash-b deleted");

    // Recreate BOTH files on disk with new v2 content
    let slash_a_v2_path = write_memory_md(&east_dir, "slash-a", "Slash A v2", "hypothesis");
    let slash_a_v2_bytes = std::fs::read(&slash_a_v2_path).expect("read slash-a v2 bytes");
    let slash_b_v2_path = write_memory_md(&east_dir, "slash-b", "Slash B v2", "hypothesis");

    let (stale_two_arg_err, relist_json) = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_slash_str = east_slash_str.clone();
        move || {
            // Without re-listing, 2-arg delete on restored `slash-a` MUST fail closed (cwd unknown)!
            let err_a = facade
                .delete_project_memory_json(&host_id, "slash-a", None)
                .expect_err("restored slash-a without re-list must NOT be deleted via stale 2-arg");

            // Re-list `east/` -> `slash-b` MUST NOT be falsely marked ambiguous; 2-arg confirm succeeds!
            let listed2 = facade
                .list_project_memories_json(&host_id, &east_slash_str, 50)
                .expect("relist east/");
            facade
                .confirm_project_memory_json(&host_id, "slash-b", None)
                .expect("same-dir relist after explicit slash delete must allow 2-arg confirm");
            (err_a, listed2)
        }
    })
    .await
    .expect("join relist verification");

    assert!(
        stale_two_arg_err.contains("unknown"),
        "expected cwd unknown after explicit slash delete revocation, got: {stale_two_arg_err}"
    );
    assert!(relist_json.contains("Slash B v2"), "got: {relist_json}");
    assert_eq!(
        std::fs::read(&slash_a_v2_path).expect("slash-a v2 intact"),
        slash_a_v2_bytes
    );
    let slash_b_disk = std::fs::read_to_string(&slash_b_v2_path).expect("read slash-b v2");
    assert!(
        slash_b_disk.contains("status: confirmed"),
        "restored slash-b must be confirmed after same-dir relist: {slash_b_disk}"
    );

    // Part B (R4-P0-1 POSIX backslash parity): `east` vs `east\` are strictly distinct directories
    let bs_ambig_err = tokio::task::spawn_blocking({
        let facade = facade.clone();
        let host_id = host_id.clone();
        let east_plain_str = east_plain_str.clone();
        let east_bs_str = east_bs_str.clone();
        move || {
            let plain_list = facade
                .list_project_memories_json(&host_id, &east_plain_str, 50)
                .expect("list plain east");
            let bs_list = facade
                .list_project_memories_json(&host_id, &east_bs_str, 50)
                .expect("list east\\");
            assert!(plain_list.contains("Plain East"), "got: {plain_list}");
            assert!(bs_list.contains("Backslash East"), "got: {bs_list}");
            facade
                .delete_project_memory_json(&host_id, "bs-shared", None)
                .expect_err("same ID across east and east\\ must fail closed as ambiguous")
        }
    })
    .await
    .expect("join backslash ambiguity check");

    assert!(bs_ambig_err.contains("ambiguous"), "got: {bs_ambig_err}");
    assert_eq!(
        std::fs::read(&bs_plain_path).expect("plain east intact"),
        bs_plain_orig
    );
    assert_eq!(
        std::fs::read(&bs_slash_path).expect("backslash east intact"),
        bs_slash_orig
    );

    // Explicit delete targeting `east\` removes ONLY `east\` while keeping `east` byte-identical
    tokio::task::spawn_blocking(move || {
        facade
            .delete_project_memory_json(&host_id, "bs-shared", Some(east_bs_str))
            .expect("explicit delete on east\\ must succeed");
    })
    .await
    .expect("join explicit east\\ delete");

    assert_eq!(
        std::fs::read(&bs_plain_path).expect("plain east still byte-identical"),
        bs_plain_orig
    );
    assert!(!bs_slash_path.exists(), "east\\ memory must be deleted");
}
