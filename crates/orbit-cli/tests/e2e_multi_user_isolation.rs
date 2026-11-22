use std::net::SocketAddr;
use std::time::Duration;

use orbit_client_core::{
    ClientCoreError, HostConnectionState, HostRouteMode, SavedHostProfile, app_core::OrbitAppCore,
    connect_smart,
};
use orbit_daemon::{DaemonConfig, DaemonPersistedConfig, DaemonRuntime, run_daemon};
use orbit_protocol::WireMessage;
use uuid::Uuid;

#[tokio::test]
async fn same_machine_multi_user_daemons_auto_isolate_ports_and_block_cross_talk() {
    let base_tmp = std::env::temp_dir().join(format!("orbit_multi_user_iso_{}", Uuid::new_v4()));
    let home_a = base_tmp.join("home_ec2_user");
    let home_b = base_tmp.join("home_gj");
    std::fs::create_dir_all(&home_a).expect("mkdir a");
    std::fs::create_dir_all(&home_b).expect("mkdir b");

    // Pick an available port P for user A (`ec2-user`)
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("probe bind");
    let shared_addr: SocketAddr = probe.local_addr().expect("local addr");
    drop(probe);

    let rt_a = DaemonRuntime::bootstrap(&home_a).await.expect("rt a");
    let host_id_a = rt_a.host_info.host_id.clone();
    let pubkey_a = rt_a.keypair.public_key_b64.clone();
    drop(rt_a);

    let rt_b = DaemonRuntime::bootstrap(&home_b).await.expect("rt b");
    let host_id_b = rt_b.host_info.host_id.clone();
    let pubkey_b = rt_b.keypair.public_key_b64.clone();
    drop(rt_b);
    assert_ne!(host_id_a, host_id_b);

    // 1. Start user A's daemon on `shared_addr`
    let home_a_bg = home_a.clone();
    tokio::spawn(async move {
        let _ = run_daemon(DaemonConfig {
            home_dir: home_a_bg,
            listen_addr: shared_addr,
            relay_url: None,
        })
        .await;
    });
    let http = reqwest::Client::new();
    let deadline_a = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut daemon_a_ready = false;
    while tokio::time::Instant::now() < deadline_a {
        if let Ok(resp) = http
            .get(format!("http://{shared_addr}/healthz"))
            .send()
            .await
            && resp.status().is_success()
        {
            daemon_a_ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        daemon_a_ready,
        "user A daemon must become ready on {shared_addr}"
    );

    // 2. Start user B's daemon ALSO requesting the EXACT SAME `shared_addr`!
    //    `run_daemon` must automatically isolate user B onto a conflict-free port in `6868..=6999`
    //    and persist it to `home_b/config.json`!
    let home_b_bg = home_b.clone();
    tokio::spawn(async move {
        let _ = run_daemon(DaemonConfig {
            home_dir: home_b_bg,
            listen_addr: shared_addr,
            relay_url: None,
        })
        .await;
    });
    let deadline_b = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut isolated_addr_b = None;
    while tokio::time::Instant::now() < deadline_b {
        if let Some(addr) = DaemonPersistedConfig::load(&home_b).listen_addr
            && let Ok(resp) = http.get(format!("http://{addr}/healthz")).send().await
            && resp.status().is_success()
        {
            isolated_addr_b = Some(addr);
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let isolated_addr_b =
        isolated_addr_b.expect("user B must persist its auto-isolated listen_addr");
    assert_ne!(
        isolated_addr_b.port(),
        shared_addr.port(),
        "user B must never collide with user A's port"
    );

    // Verify `/healthz` returns distinct cryptographic `host_id`s for user A and user B!
    let hz_a: serde_json::Value = http
        .get(format!("http://{shared_addr}/healthz"))
        .send()
        .await
        .expect("healthz a")
        .json()
        .await
        .expect("json a");
    let hz_b: serde_json::Value = http
        .get(format!("http://{isolated_addr_b}/healthz"))
        .send()
        .await
        .expect("healthz b")
        .json()
        .await
        .expect("json b");
    assert_eq!(hz_a["host_id"].as_str(), Some(host_id_a.as_str()));
    assert_eq!(hz_b["host_id"].as_str(), Some(host_id_b.as_str()));

    // 3. Strict Identity Enforcement: if user B's profile accidentally points at user A's port,
    //    `connect_smart` MUST reject immediately with `HostIdentityMismatch`!
    let misconfigured_b = SavedHostProfile {
        host_id: host_id_b.clone(),
        hostname: "same-ec2-host".into(),
        custom_name: Some("gj".into()),
        ssh_alias: None,
        route_mode: HostRouteMode::DirectOnly,
        remote_daemon_port: shared_addr.port(),
        custom_direct_endpoint: None,
        daemon_pubkey_b64: pubkey_b.clone(),
        direct_endpoints: vec![format!("ws://{shared_addr}/ws")],
        relay_url: None,
        state: HostConnectionState::Disconnected,
    };
    let err = connect_smart(&misconfigured_b, None)
        .await
        .err()
        .expect("must reject mismatched daemon identity");
    assert!(
        matches!(err, ClientCoreError::HostIdentityMismatch { .. }),
        "expected HostIdentityMismatch, got {err:?}"
    );

    // 4. Simultaneous Multi-User Operation in a single OrbitAppCore:
    //    Create a workspace on user A and verify user B has 0 leaked workspaces!
    let client_a = orbit_client_core::connect_direct(&format!("ws://{shared_addr}/ws"), None)
        .await
        .expect("connect a");
    let ws_req = Uuid::new_v4();
    let _ = client_a
        .request(
            WireMessage::CreateWorkspace {
                request_id: ws_req,
                source_cwd: home_a.to_string_lossy().to_string(),
                title: Some("ec2_user_only_repo".into()),
                worktree_branch: None,
                base_ref: None,
                isolation: orbit_protocol::WorkspaceIsolation::Local,
                worktree_mode: orbit_protocol::WorktreeCreateMode::BranchOff,
                pr_number: None,
                parent_workspace_id: None,
                background: false,
                idempotency_key: None,
                is_curated: None,
                mission_goal: None,
                project_root: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::WorkspaceCreated { request_id, .. } if *request_id == ws_req),
        )
        .await
        .expect("create workspace on user A");

    let offer_a = orbit_crypto::PairingOffer {
        host_id: host_id_a.clone(),
        hostname: "ec2-shared".into(),
        daemon_pubkey_b64: pubkey_a,
        direct_endpoints: vec![format!("ws://{shared_addr}/ws")],
        relay_url: None,
        pairing_secret: "sec-a".into(),
    };
    let offer_b = orbit_crypto::PairingOffer {
        host_id: host_id_b.clone(),
        hostname: "ec2-shared".into(),
        daemon_pubkey_b64: pubkey_b,
        direct_endpoints: vec![format!("ws://{isolated_addr_b}/ws")],
        relay_url: None,
        pairing_secret: "sec-b".into(),
    };

    let core = OrbitAppCore::open_in_memory().expect("core");
    core.pair_host(&offer_a.to_qr_uri().expect("qr a"))
        .expect("pair a");
    core.pair_host(&offer_b.to_qr_uri().expect("qr b"))
        .expect("pair b");

    for _ in 0..25 {
        let da = core.host_detail(&host_id_a).expect("detail a");
        let db = core.host_detail(&host_id_b).expect("detail b");
        if da.host.connection_state == HostConnectionState::OnlineDirect
            && db.host.connection_state == HostConnectionState::OnlineDirect
            && !da.workspaces.is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    let detail_a = core.host_detail(&host_id_a).expect("detail a");
    let detail_b = core.host_detail(&host_id_b).expect("detail b");
    assert_eq!(
        detail_a.host.connection_state,
        HostConnectionState::OnlineDirect
    );
    assert_eq!(
        detail_b.host.connection_state,
        HostConnectionState::OnlineDirect
    );
    assert_eq!(detail_a.host.remote_daemon_port, shared_addr.port());
    assert_eq!(detail_b.host.remote_daemon_port, isolated_addr_b.port());
    assert_eq!(
        detail_a.workspaces.len(),
        1,
        "user A should see its own workspace"
    );
    assert_eq!(detail_a.workspaces[0].title, "ec2_user_only_repo");
    assert!(
        detail_b.workspaces.is_empty() && detail_b.agents.is_empty(),
        "user B must NEVER see user A's workspaces or sessions!"
    );

    let _ = std::fs::remove_dir_all(&base_tmp);
}
