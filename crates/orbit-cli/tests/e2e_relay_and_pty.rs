use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use orbit_client_core::{
    ClientInboundEvent, HostConnectionState, MultiHostFleet, connect_via_relay,
};
use orbit_crypto::HostIdentityKeypair;
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{TerminalBinaryFrame, WireMessage};
use orbit_relay::{RelayConfig, run_relay};
use uuid::Uuid;

#[tokio::test]
async fn full_stack_e2ee_relay_allowlist_git_and_pty_integration() {
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_e2e_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    // 1. Bootstrap Daemon first to know its static X25519 public key for Relay allowlisting
    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    drop(pre_rt);

    // 2. Bind a free port for the private allowlisted Relay
    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind probe");
    let relay_addr = relay_listener.local_addr().expect("addr");
    drop(relay_listener);

    let mut allow_set = HashSet::new();
    allow_set.insert(allowed_pubkey.clone());
    let relay_cfg = RelayConfig {
        listen_addr: relay_addr,
        allowed_pubkeys: Arc::new(allow_set),
    };
    tokio::spawn(async move {
        let _ = run_relay(relay_cfg).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

    let relay_url = format!("ws://{relay_addr}");

    // 3. Verify stranger pubkey is strictly rejected (zero memory/bandwidth abuse!)
    let stranger_kp = HostIdentityKeypair::generate().expect("stranger keygen");
    let stranger_res = connect_via_relay(&relay_url, &stranger_kp.public_key_b64, None).await;
    assert!(
        stranger_res.is_err(),
        "unlisted public key must be rejected by Relay allowlist"
    );

    // 4. Start embedded Daemon connected outbound to the Relay
    let (_daemon_addr, runtime) = spawn_embedded_daemon(temp_home.clone(), Some(relay_url.clone()))
        .await
        .expect("spawn daemon");

    // Wait briefly for outbound registration on Relay
    tokio::time::sleep(Duration::from_millis(150)).await;

    // 5. Build pairing QR URI with an unreachable LAN IP so Happy-Eyeballs falls back to E2EE Relay!
    let offer =
        runtime.build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(relay_url.clone()));
    let qr_uri = offer.to_qr_uri().expect("qr uri");

    let fleet = MultiHostFleet::open_in_memory().expect("fleet");
    let profile = fleet.import_pairing_uri(&qr_uri).expect("import qr");
    let session = fleet
        .connect_host(&profile.host_id)
        .await
        .expect("connect e2ee");
    assert_eq!(session.mode, HostConnectionState::OnlineRelay);

    // 6. Test Workspace + Git Status over Noise NK E2EE + Zstd
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_string_lossy()
        .to_string();

    let ws_req = Uuid::new_v4();
    let ws_resp = session
        .request(
            WireMessage::CreateWorkspace {
                request_id: ws_req,
                source_cwd: repo_root.clone(),
                title: Some("Orbit Repo".into()),
                worktree_branch: None,
                base_ref: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::WorkspaceCreated { request_id, .. } if *request_id == ws_req),
        )
        .await
        .expect("create workspace over e2ee");

    match ws_resp {
        WireMessage::WorkspaceCreated { workspace, .. } => {
            assert_eq!(workspace.title, "Orbit Repo");
        }
        other => panic!("unexpected workspace response: {other:?}"),
    }

    // Verify local mobile SQLite replica automatically cached the workspace!
    let cached_ws = fleet
        .replica_store()
        .list_workspaces()
        .expect("replica list");
    assert_eq!(cached_ws.len(), 1);

    let git_req = Uuid::new_v4();
    let git_resp = session
        .request(
            WireMessage::GetGitStatus {
                request_id: git_req,
                cwd: repo_root.clone(),
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::GitStatusResponse { request_id, .. } if *request_id == git_req),
        )
        .await
        .expect("git status over e2ee");

    if let WireMessage::GitStatusResponse { status, .. } = git_resp {
        assert!(status.is_git_repo);
    } else {
        panic!("expected GitStatusResponse");
    }

    // 7. Test Detached PTY Terminal creation & binary I/O over Noise NK E2EE!
    let mut term_events = session.subscribe();
    let term_req = Uuid::new_v4();
    let term_resp = session
        .request(
            WireMessage::CreateTerminal {
                request_id: term_req,
                cwd: repo_root,
                title: Some("E2EE Shell".into()),
                cols: 80,
                rows: 24,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::TerminalAttached { request_id, .. } if *request_id == term_req),
        )
        .await
        .expect("create detached PTY over e2ee");

    let (term_id, slot) = match term_resp {
        WireMessage::TerminalAttached { terminal, slot, .. } => (terminal.terminal_id, slot),
        other => panic!("unexpected terminal response: {other:?}"),
    };

    session
        .send_terminal(TerminalBinaryFrame::Input {
            slot,
            data: b"printf 'ORBIT_E2EE_PTY_OK\\n'\n".to_vec(),
        })
        .await
        .expect("send pty input");

    let mut collected = String::new();
    let pty_deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < pty_deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(Duration::from_millis(250), term_events.recv()).await
            && let ClientInboundEvent::Terminal(
                TerminalBinaryFrame::Output { data, .. }
                | TerminalBinaryFrame::Snapshot { data, .. },
            ) = ev
        {
            collected.push_str(&String::from_utf8_lossy(&data));
            if collected.contains("ORBIT_E2EE_PTY_OK") {
                break;
            }
        }
    }
    assert!(
        collected.contains("ORBIT_E2EE_PTY_OK"),
        "expected PTY output over E2EE relay, got: {collected}"
    );

    let kill_req = Uuid::new_v4();
    let _ = session
        .request(
            WireMessage::KillTerminal {
                request_id: kill_req,
                terminal_id: term_id,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::Ack { request_id } if *request_id == kill_req),
        )
        .await
        .expect("kill terminal");

    let _ = std::fs::remove_dir_all(&temp_home);
}
