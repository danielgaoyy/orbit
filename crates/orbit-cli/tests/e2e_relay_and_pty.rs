use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use orbit_client_core::{
    ClientInboundEvent, HostConnectionState, MultiHostFleet, app_core::OrbitAppCore,
    connect_via_relay,
};
use orbit_crypto::HostIdentityKeypair;
use orbit_daemon::spawn_embedded_daemon;
use orbit_protocol::{TerminalBinaryFrame, WireMessage};
use orbit_relay::{RelayConfig, run_relay};
use orbit_relay_transport::{
    SshRelaySpec, compute_openssh_pubkey_sha256_b64, generate_ssh_tunnel_key,
};
use russh::keys::key::safe_rng;
use russh::keys::ssh_key::{Algorithm, PrivateKey, PublicKey};
use russh::server::{
    Auth, ChannelOpenHandle, Config as SshServerConfig, Handler, Msg, Server, Session,
    VerifiedPublicKey,
};
use russh::{Channel, ChannelId};
use uuid::Uuid;

#[tokio::test]
async fn full_stack_e2ee_relay_allowlist_git_and_pty_integration() {
    orbit_daemon::manager::set_orbit_exe_override(std::path::PathBuf::from(env!(
        "CARGO_BIN_EXE_orbit"
    )));
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
    let offer = runtime
        .build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(relay_url.clone()))
        .expect("pairing offer");
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
            Duration::from_secs(20),
            |w| matches!(w, WireMessage::TerminalAttached { request_id, .. } if *request_id == term_req),
        )
        .await
        .expect("create detached PTY over e2ee");

    let (term_id, slot) = match term_resp {
        WireMessage::TerminalAttached { terminal, slot, .. } => (terminal.terminal_id, slot),
        other => panic!("unexpected terminal response: {other:?}"),
    };

    let pty_cmd: Vec<u8> = if cfg!(windows) {
        b"echo ORBIT_E2EE_PTY_OK\r\n".to_vec()
    } else {
        b"printf 'ORBIT_E2EE_PTY_OK\\n'\n".to_vec()
    };
    session
        .send_terminal(TerminalBinaryFrame::Input {
            slot,
            data: pty_cmd.clone(),
        })
        .await
        .expect("send pty input");

    let mut collected = String::new();
    let pty_deadline =
        tokio::time::Instant::now() + Duration::from_secs(if cfg!(windows) { 25 } else { 10 });
    let mut last_resend = tokio::time::Instant::now();
    let mut dsr_answered = false;
    while tokio::time::Instant::now() < pty_deadline {
        if let Ok(Ok(ev)) =
            tokio::time::timeout(Duration::from_millis(250), term_events.recv()).await
            && let ClientInboundEvent::Terminal(
                TerminalBinaryFrame::Output { data, .. }
                | TerminalBinaryFrame::Snapshot { data, .. },
            ) = ev
        {
            let chunk = String::from_utf8_lossy(&data);
            collected.push_str(&chunk);
            if !dsr_answered && collected.contains("\x1b[6n") {
                dsr_answered = true;
                let _ = session
                    .send_terminal(TerminalBinaryFrame::Input {
                        slot,
                        data: b"\x1b[1;1R".to_vec(),
                    })
                    .await;
            }
            if collected.contains("ORBIT_E2EE_PTY_OK") {
                break;
            }
        }
        if cfg!(windows) && last_resend.elapsed() >= Duration::from_secs(2) {
            last_resend = tokio::time::Instant::now();
            let _ = session
                .send_terminal(TerminalBinaryFrame::Input {
                    slot,
                    data: pty_cmd.clone(),
                })
                .await;
        }
    }

    let kill_req = Uuid::new_v4();
    let kill_res = session
        .request(
            WireMessage::KillTerminal {
                request_id: kill_req,
                terminal_id: term_id,
            },
            Duration::from_secs(10),
            |w| matches!(w, WireMessage::Ack { request_id } if *request_id == kill_req),
        )
        .await;
    assert!(
        collected.contains("ORBIT_E2EE_PTY_OK"),
        "expected PTY output over E2EE relay (bytes={}), got: {:?}",
        collected.len(),
        collected
    );
    kill_res.expect("kill terminal");

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[derive(Clone)]
struct MockRelaySshServer {
    allowed_client_pk: PublicKey,
    relay_port: u16,
}

struct MockRelaySshHandler {
    allowed_client_pk: PublicKey,
    relay_port: u16,
}

impl Server for MockRelaySshServer {
    type Handler = MockRelaySshHandler;

    fn new_client(&mut self, _peer_addr: Option<std::net::SocketAddr>) -> Self::Handler {
        MockRelaySshHandler {
            allowed_client_pk: self.allowed_client_pk.clone(),
            relay_port: self.relay_port,
        }
    }
}

impl Handler for MockRelaySshHandler {
    type Error = russh::Error;

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: VerifiedPublicKey<'_>,
    ) -> Result<Auth, Self::Error> {
        if user == "testuser" && key.public_key.key_data() == self.allowed_client_pk.key_data() {
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if host_to_connect != "127.0.0.1" || port_to_connect != u32::from(self.relay_port) {
            return Ok(());
        }
        let Ok(mut upstream) = tokio::net::TcpStream::connect(("127.0.0.1", self.relay_port)).await
        else {
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let mut ssh_stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut ssh_stream, &mut upstream).await;
        });
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn ssh_tunneled_e2ee_relay_integration() {
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_ssh_e2e_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    drop(pre_rt);

    // 1. Start localhost-only orbit-relay
    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay");
    let relay_addr = relay_listener.local_addr().expect("relay addr");
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

    // 2. Start in-process mock SSH server permitting direct-tcpip only to 127.0.0.1:<relay_port>
    let host_priv = PrivateKey::random(&mut safe_rng(), Algorithm::Ed25519).expect("host key");
    let host_openssh = host_priv.public_key().to_openssh().expect("host openssh");
    let server_fp = compute_openssh_pubkey_sha256_b64(&host_openssh).expect("server fp");

    let tunnel_key = generate_ssh_tunnel_key().expect("tunnel key");
    let allowed_client_pk = PublicKey::from_openssh(&tunnel_key.openssh_pubkey).expect("client pk");

    let ssh_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ssh");
    let ssh_port = ssh_listener.local_addr().expect("ssh addr").port();

    let ssh_cfg = Arc::new(SshServerConfig {
        keys: vec![host_priv],
        auth_rejection_time: Duration::from_millis(5),
        auth_rejection_time_initial: Some(Duration::from_millis(0)),
        ..Default::default()
    });
    let mut ssh_server = MockRelaySshServer {
        allowed_client_pk,
        relay_port: relay_addr.port(),
    };
    tokio::spawn(async move {
        let _ = ssh_server.run_on_socket(ssh_cfg, &ssh_listener).await;
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let ssh_relay_uri = SshRelaySpec {
        user: "testuser".into(),
        ssh_host: "127.0.0.1".into(),
        ssh_port,
        relay_port: relay_addr.port(),
        key_seed_b64: tunnel_key.seed_b64.clone(),
        host_key_fps_b64: vec![server_fp],
    }
    .to_uri();

    // 3. Wrong SSH server fingerprint must fail
    let wrong_fp = SshRelaySpec {
        user: "testuser".into(),
        ssh_host: "127.0.0.1".into(),
        ssh_port,
        relay_port: relay_addr.port(),
        key_seed_b64: tunnel_key.seed_b64.clone(),
        host_key_fps_b64: vec!["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into()],
    }
    .to_uri();
    assert!(
        connect_via_relay(&wrong_fp, &allowed_pubkey, None)
            .await
            .is_err(),
        "wrong SSH host key fingerprint must be rejected"
    );

    // 4. Unlisted daemon pubkey must still be rejected by Relay allowlist through SSH tunnel
    let stranger_kp = HostIdentityKeypair::generate().expect("stranger");
    assert!(
        connect_via_relay(&ssh_relay_uri, &stranger_kp.public_key_b64, None)
            .await
            .is_err(),
        "stranger pubkey must be rejected by Relay allowlist over SSH tunnel"
    );

    // 5. Start embedded Daemon connected outbound over ssh+relay://
    let (_daemon_addr, runtime) =
        spawn_embedded_daemon(temp_home.clone(), Some(ssh_relay_uri.clone()))
            .await
            .expect("spawn daemon");
    tokio::time::sleep(Duration::from_millis(250)).await;

    // 6. Client imports PairingOffer with unreachable LAN endpoint -> connects via SSH Relay + Noise NK E2EE
    let offer = runtime
        .build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(ssh_relay_uri))
        .expect("pairing offer");
    let qr_uri = offer.to_qr_uri().expect("qr uri");

    let fleet = MultiHostFleet::open_in_memory().expect("fleet");
    let profile = fleet.import_pairing_uri(&qr_uri).expect("import qr");
    let session = fleet
        .connect_host(&profile.host_id)
        .await
        .expect("connect via ssh+relay");
    assert_eq!(session.mode, HostConnectionState::OnlineRelay);

    // 7. Execute a real RPC over SSH -> Relay -> Noise NK E2EE -> Daemon
    let req_id = Uuid::new_v4();
    let resp = session
        .request(
            WireMessage::ListAgents {
                request_id: req_id,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::AgentsSnapshot { request_id, .. } if *request_id == req_id),
        )
        .await
        .expect("list agents over ssh+relay");
    assert!(matches!(resp, WireMessage::AgentsSnapshot { .. }));

    let _ = std::fs::remove_dir_all(&temp_home);
}

#[tokio::test]
async fn e2ee_relay_chunked_attachment_staging_integration() {
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_att_e2e_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    drop(pre_rt);

    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay");
    let relay_addr = relay_listener.local_addr().expect("relay addr");
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
    let (_daemon_addr, runtime) = spawn_embedded_daemon(temp_home.clone(), Some(relay_url.clone()))
        .await
        .expect("spawn daemon");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let offer = runtime
        .build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(relay_url))
        .expect("pairing offer");
    let qr_uri = offer.to_qr_uri().expect("qr uri");
    let host_id = offer.host_id.clone();

    let core = OrbitAppCore::open_in_memory().expect("open core");
    core.pair_host(&qr_uri).expect("pair host");

    let connect_deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    let mut connected_online = false;
    while tokio::time::Instant::now() < connect_deadline {
        if let Some(card) = core
            .fleet_overview()
            .hosts
            .into_iter()
            .find(|h| h.host_id == host_id)
            && card.connection_state == HostConnectionState::OnlineRelay
            && card.daemon_version.is_some()
        {
            connected_online = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
    assert!(connected_online, "core should connect via E2EE relay");

    // 1. Single-chunk PNG upload
    let png_bytes = b"\x89PNG\r\n\x1a\ne2ee-relay-png-test".to_vec();
    let png_info = core
        .stage_attachment_on_host(&host_id, "screenshot.png", png_bytes.clone())
        .await
        .expect("stage png over relay");
    let png_path = std::path::PathBuf::from(&png_info.path);
    assert!(png_path.starts_with(temp_home.join("attachments")));
    assert_eq!(png_info.size_bytes, png_bytes.len());
    assert_eq!(std::fs::read(&png_path).expect("read png"), png_bytes);

    // 2. Multi-chunk 1.2 MiB upload (3 chunks of 512 KiB)
    let big_bytes: Vec<u8> = (0..(1_200 * 1024)).map(|i| ((i * 7) % 251) as u8).collect();
    let big_info = core
        .stage_attachment_on_host(&host_id, "trace_dump.bin", big_bytes.clone())
        .await
        .expect("stage 1.2 MiB over relay");
    let big_path = std::path::PathBuf::from(&big_info.path);
    assert!(big_path.starts_with(temp_home.join("attachments")));
    assert_eq!(big_info.size_bytes, big_bytes.len());
    assert_eq!(std::fs::read(&big_path).expect("read big"), big_bytes);

    let leftover_tmp: Vec<_> = std::fs::read_dir(temp_home.join("attachments/.tmp"))
        .expect("read tmp")
        .flatten()
        .collect();
    assert!(leftover_tmp.is_empty(), "expected clean .tmp dir");

    // 3. Reject > 25 MiB
    let oversized = vec![0u8; 25 * 1024 * 1024 + 1];
    let over_err = core
        .stage_attachment_on_host(&host_id, "too_big.bin", oversized)
        .await
        .expect_err("oversized must fail");
    assert!(over_err.contains("25 MiB"));

    // 4. Removing/disconnecting host fails upload immediately
    core.remove_host(&host_id).expect("remove host");
    let missing_err = core
        .stage_attachment_on_host(&host_id, "after_remove.png", png_bytes)
        .await
        .expect_err("removed host must fail");
    assert!(missing_err.contains("not found"));

    let _ = tokio::task::spawn_blocking(move || drop(core)).await;
    let _ = std::fs::remove_dir_all(&temp_home);
}

#[tokio::test]
async fn e2e_relay_pop_blocks_daemon_hijack_while_legitimate_client_succeeds() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message as TungMsg;

    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_pop_e2e_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    drop(pre_rt);

    let relay_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind relay");
    let relay_addr = relay_listener.local_addr().expect("relay addr");
    drop(relay_listener);

    let mut allow_set = HashSet::new();
    allow_set.insert(allowed_pubkey.clone());
    let relay_cfg = RelayConfig {
        listen_addr: relay_addr,
        allowed_pubkeys: Arc::new(allow_set),
    };
    let relay_handle = tokio::spawn(async move {
        let _ = run_relay(relay_cfg).await;
    });
    tokio::time::sleep(Duration::from_millis(80)).await;

    let relay_url = format!("ws://{relay_addr}");
    let (_daemon_addr, _runtime) =
        spawn_embedded_daemon(temp_home.clone(), Some(relay_url.clone()))
            .await
            .expect("spawn daemon");
    tokio::time::sleep(Duration::from_millis(180)).await;

    // Attacker knows the allowlisted pubkey and tries to hijack /v1/daemon/control with forged PoP bytes
    let hijack_url = format!("{relay_url}/v1/daemon/control?pubkey={allowed_pubkey}");
    let (mut attacker_ws, _) = tokio_tungstenite::connect_async(&hijack_url)
        .await
        .expect("attacker ws upgrade");
    let challenge = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next())
        .await
        .expect("challenge timeout")
        .expect("challenge frame")
        .expect("challenge ok");
    assert!(
        matches!(challenge, TungMsg::Binary(ref b) if b.len() == orbit_crypto::RELAY_POP_MSG_LEN)
    );

    attacker_ws
        .send(TungMsg::Binary(
            vec![0xDEu8; orbit_crypto::RELAY_POP_MSG_LEN].into(),
        ))
        .await
        .expect("send forged pop");
    let closed = tokio::time::timeout(Duration::from_secs(3), attacker_ws.next())
        .await
        .expect("close timeout");
    assert!(
        matches!(closed, None | Some(Ok(TungMsg::Close(_))) | Some(Err(_))),
        "relay must reject forged PoP and close attacker socket, got: {closed:?}"
    );

    // Legitimate E2EE client still reaches the real daemon unimpeded!
    let session = connect_via_relay(&relay_url, &allowed_pubkey, None)
        .await
        .expect("legitimate client must connect after failed hijack");
    let req_id = Uuid::new_v4();
    let resp = session
        .request(
            WireMessage::ListAgents {
                request_id: req_id,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::AgentsSnapshot { request_id, .. } if *request_id == req_id),
        )
        .await
        .expect("list agents over E2EE relay after hijack attempt");
    assert!(matches!(resp, WireMessage::AgentsSnapshot { .. }));

    relay_handle.abort();
    let _ = std::fs::remove_dir_all(&temp_home);
}
