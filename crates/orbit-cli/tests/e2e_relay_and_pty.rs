use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use orbit_client_core::{
    ClientCoreError, ClientInboundEvent, HostConnectionState, HostRouteMode, MultiHostFleet,
    SavedHostProfile, app_core::OrbitAppCore, connect_smart, connect_via_relay,
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
    let pairing_secret = pre_rt.pairing_secret.clone();
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
    let stranger_res = connect_via_relay(
        &relay_url,
        &stranger_kp.public_key_b64,
        &pairing_secret,
        None,
    )
    .await;
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
    let pairing_secret = pre_rt.pairing_secret.clone();
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
        connect_via_relay(&wrong_fp, &allowed_pubkey, &pairing_secret, None)
            .await
            .is_err(),
        "wrong SSH host key fingerprint must be rejected"
    );

    // 4. Unlisted daemon pubkey must still be rejected by Relay allowlist through SSH tunnel
    let stranger_kp = HostIdentityKeypair::generate().expect("stranger");
    assert!(
        connect_via_relay(
            &ssh_relay_uri,
            &stranger_kp.public_key_b64,
            &pairing_secret,
            None,
        )
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
    let pairing_secret = pre_rt.pairing_secret.clone();
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
    let session = connect_via_relay(&relay_url, &allowed_pubkey, &pairing_secret, None)
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

#[cfg(unix)]
#[tokio::test]
async fn detached_orbit_shim_isolates_process_group_hides_argv_secrets_and_survives_reconnect() {
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;

    let orbit_exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_orbit"));
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_sh_{short_id}"));
    let shims_dir = temp_home.join("shims");
    let sock_path = shims_dir.join("cat.sock");

    let secret = "orbit-live-shim-secret-token-4f3e2d1c";
    let mut env = HashMap::new();
    env.insert("ORBIT_SECRET_TEST".into(), secret.into());
    let runtime_dir = temp_home.join("runtime");
    std::fs::create_dir_all(&runtime_dir).expect("mkdir runtime");
    let cleanup_file = runtime_dir.join("claude-mcp-detached-test.json");
    std::fs::write(&cleanup_file, "{\"secret\":true}").expect("write cleanup file");
    let spec = orbit_shim::ShimSpawnSpec {
        shim_id: "detached_survival_test".into(),
        mode: orbit_shim::ShimMode::Stdio,
        program: "/bin/cat".into(),
        args: vec![],
        cwd: base_tmp.clone(),
        env,
        cleanup_paths: vec![cleanup_file.clone()],
    };

    let mut client1 = orbit_shim::spawn_detached_shim(&orbit_exe, &sock_path, &spec)
        .await
        .expect("spawn real detached orbit shim");

    // Verify hardened Unix permissions on shims/ (0700) and .sock (0600)
    assert_eq!(
        std::fs::metadata(&shims_dir)
            .expect("shims meta")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&sock_path)
            .expect("sock meta")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    // Read recorded shim PID from `.pid` sidecar and verify:
    // 1) Process group is detached (`pgid == shim_pid`, separate from parent process group).
    // 2) Process argv contains ZERO secrets and ZERO `--spec-json`.
    let pid_raw =
        std::fs::read_to_string(orbit_shim::pid_file_path(&sock_path)).expect("read pid sidecar");
    let pid_json: serde_json::Value = serde_json::from_str(&pid_raw).expect("parse pid sidecar");
    let shim_pid = pid_json["shim_pid"].as_u64().expect("shim_pid") as u32;

    let ps_pgid = std::process::Command::new("ps")
        .args(["-p", &shim_pid.to_string(), "-o", "pgid="])
        .output()
        .expect("ps pgid");
    let shim_pgid: u32 = String::from_utf8_lossy(&ps_pgid.stdout)
        .trim()
        .parse()
        .expect("parse shim pgid");
    assert_eq!(
        shim_pgid, shim_pid,
        "detached orbit shim must lead its own process group (pgid == shim_pid)"
    );

    let ps_cmd = std::process::Command::new("ps")
        .args(["-p", &shim_pid.to_string(), "-o", "command="])
        .output()
        .expect("ps command");
    let shim_cmdline = String::from_utf8_lossy(&ps_cmd.stdout);
    assert!(
        !shim_cmdline.contains(secret),
        "live shim argv must NEVER expose secrets: {shim_cmdline}"
    );
    assert!(
        !shim_cmdline.contains("--spec-json"),
        "live shim argv must NEVER contain --spec-json: {shim_cmdline}"
    );

    // Exchange frame 1 and disconnect client1 (simulating daemon restart/exit)
    client1
        .write_stdin_line("before-reconnect")
        .await
        .expect("write 1");
    let seq1 = match tokio::time::timeout(Duration::from_secs(3), client1.rx.recv())
        .await
        .expect("timeout 1")
        .expect("frame 1")
    {
        orbit_shim::ShimFrame::StdoutLine { line, seq } => {
            assert_eq!(line, "before-reconnect");
            seq.expect("seq 1")
        }
        other => panic!("unexpected frame 1: {other:?}"),
    };
    drop(client1);

    // Verify the exact same shim PID is still alive after client disconnect
    let alive_check = std::process::Command::new("kill")
        .args(["-0", &shim_pid.to_string()])
        .status()
        .expect("kill -0");
    assert!(
        alive_check.success(),
        "detached shim PID {shim_pid} must stay alive after client disconnect"
    );

    // Reconnect with cursor to the same surviving shim PID and continue live I/O
    let mut client2 = orbit_shim::connect_existing_shim_after(&sock_path, Some(seq1))
        .await
        .expect("reconnect to surviving shim");
    client2
        .write_stdin_line("after-reconnect")
        .await
        .expect("write 2");
    match tokio::time::timeout(Duration::from_secs(3), client2.rx.recv())
        .await
        .expect("timeout 2")
        .expect("frame 2")
    {
        orbit_shim::ShimFrame::StdoutLine { line, seq } => {
            assert_eq!(line, "after-reconnect");
            assert_eq!(seq, Some(seq1 + 1));
        }
        other => panic!("unexpected frame 2: {other:?}"),
    }

    // Confirm PID sidecar still belongs to the exact same shim PID and cleanup file is still
    // intact while the child process is running across reconnect.
    let pid_raw_after =
        std::fs::read_to_string(orbit_shim::pid_file_path(&sock_path)).expect("read pid after");
    let pid_json_after: serde_json::Value =
        serde_json::from_str(&pid_raw_after).expect("parse pid after");
    assert_eq!(
        pid_json_after["shim_pid"].as_u64(),
        Some(u64::from(shim_pid))
    );
    assert!(
        cleanup_file.exists(),
        "shim must keep cleanup_paths intact across client disconnect/reconnect while child lives"
    );

    client2
        .kill_and_wait(Duration::from_secs(2))
        .await
        .expect("kill surviving shim");
    assert!(
        !cleanup_file.exists(),
        "shim must unlink cleanup_paths right after its child exits (even after reconnect)"
    );
    let _ = std::fs::remove_dir_all(&temp_home);
}

#[tokio::test]
async fn e2e_relay_e2ee_rejects_wrong_and_empty_pairing_secret() {
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_e2ee_auth_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    let real_secret = pre_rt.pairing_secret.clone();
    let host_id = pre_rt.host_info.host_id.clone();
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

    // 1. Empty pairing_secret fails closed before handshake (even against an unreachable relay URL)
    let empty_err = connect_via_relay("ws://127.0.0.1:1", &allowed_pubkey, "", None)
        .await
        .err()
        .expect("empty pairing_secret must fail closed before handshake");
    assert!(
        matches!(empty_err, ClientCoreError::Unauthorized(_)) && empty_err.is_auth_error(),
        "expected Unauthorized for empty pairing_secret, got {empty_err:?}"
    );

    // 2. Wrong pairing_secret over live Relay E2EE fails authentication cleanly
    let wrong_secret = orbit_crypto::generate_pairing_secret();
    assert_ne!(wrong_secret, real_secret);
    let wrong_err = connect_via_relay(&relay_url, &allowed_pubkey, &wrong_secret, None)
        .await
        .err()
        .expect("wrong pairing_secret must be rejected over Relay E2EE");
    assert!(
        matches!(wrong_err, ClientCoreError::Unauthorized(_)) && wrong_err.is_auth_error(),
        "expected Unauthorized for wrong pairing_secret over relay, got {wrong_err:?}"
    );

    // 3. Auto route mode preserves Unauthorized when direct endpoint is unreachable and relay rejects auth
    let bad_profile = SavedHostProfile {
        host_id,
        hostname: "relay-auth-host".into(),
        custom_name: None,
        ssh_alias: None,
        route_mode: HostRouteMode::Auto,
        remote_daemon_port: 1,
        custom_direct_endpoint: None,
        daemon_pubkey_b64: allowed_pubkey.clone(),
        direct_endpoints: vec!["ws://127.0.0.1:1/ws".into()],
        relay_url: Some(relay_url.clone()),
        pairing_secret: wrong_secret,
        pairing_secret_updated_at_ms: 0,
        state: HostConnectionState::Disconnected,
    };
    let smart_err = connect_smart(&bad_profile, None)
        .await
        .err()
        .expect("connect_smart Auto must fail with Unauthorized");
    assert!(
        matches!(smart_err, ClientCoreError::Unauthorized(_)) && smart_err.is_auth_error(),
        "expected Unauthorized from connect_smart Auto, got {smart_err:?}"
    );

    // 4. Legitimate pairing_secret succeeds and serves RPCs
    let session = connect_via_relay(&relay_url, &allowed_pubkey, &real_secret, None)
        .await
        .expect("valid pairing_secret must connect over Relay E2EE");
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
        .expect("list agents with valid pairing_secret");
    assert!(matches!(resp, WireMessage::AgentsSnapshot { .. }));

    relay_handle.abort();
    let _ = std::fs::remove_dir_all(&temp_home);
}

#[tokio::test]
async fn e2e_relay_e2ee_disconnects_on_pairing_secret_rotation() {
    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_e2ee_rot_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    let old_secret = pre_rt.pairing_secret.clone();
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
    let (_daemon_addr, runtime) = spawn_embedded_daemon(temp_home.clone(), Some(relay_url.clone()))
        .await
        .expect("spawn daemon");
    tokio::time::sleep(Duration::from_millis(180)).await;

    // 1. Connect over Relay E2EE with the initial pairing_secret and verify live RPC
    let session = connect_via_relay(&relay_url, &allowed_pubkey, &old_secret, None)
        .await
        .expect("initial relay E2EE connection");
    let mut events = session.subscribe();
    let req1 = Uuid::new_v4();
    let resp1 = session
        .request(
            WireMessage::ListAgents {
                request_id: req1,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::AgentsSnapshot { request_id, .. } if *request_id == req1),
        )
        .await
        .expect("initial RPC over relay E2EE");
    assert!(matches!(resp1, WireMessage::AgentsSnapshot { .. }));

    // 2. Rotate pairing_secret: active Relay E2EE session MUST disconnect promptly
    let rotated_secret = runtime
        .rotate_pairing_secret()
        .expect("rotate pairing_secret");
    assert_ne!(rotated_secret, old_secret);

    let disconnected = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match events.recv().await {
                Ok(ClientInboundEvent::Disconnected) | Err(_) => return true,
                Ok(_) => continue,
            }
        }
    })
    .await
    .unwrap_or(false);
    assert!(
        disconnected,
        "active Relay E2EE session must disconnect promptly after pairing_secret rotation"
    );

    // 3. Reconnecting with the old secret fails with Unauthorized
    let stale_err = connect_via_relay(&relay_url, &allowed_pubkey, &old_secret, None)
        .await
        .err()
        .expect("old pairing_secret must fail after rotation");
    assert!(
        matches!(stale_err, ClientCoreError::Unauthorized(_)) && stale_err.is_auth_error(),
        "expected Unauthorized with rotated-out secret, got {stale_err:?}"
    );

    // 4. Reconnecting with the rotated secret succeeds and completes an RPC
    let fresh_session = connect_via_relay(&relay_url, &allowed_pubkey, &rotated_secret, None)
        .await
        .expect("rotated pairing_secret must reconnect over Relay E2EE");
    let req2 = Uuid::new_v4();
    let resp2 = fresh_session
        .request(
            WireMessage::ListAgents {
                request_id: req2,
                workspace_id: None,
                include_archived: false,
                include_background: false,
                parent_agent_id: None,
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::AgentsSnapshot { request_id, .. } if *request_id == req2),
        )
        .await
        .expect("RPC over rotated relay E2EE session");
    assert!(matches!(resp2, WireMessage::AgentsSnapshot { .. }));

    relay_handle.abort();
    let _ = std::fs::remove_dir_all(&temp_home);
}

#[tokio::test]
async fn e2e_relay_e2ee_replayed_msg1_rejected_before_session_start() {
    use futures_util::{SinkExt, StreamExt};
    use orbit_crypto::{CLIENT_E2EE_MSG_LEN, ClientHandshake, decode_key_32};
    use tokio_tungstenite::tungstenite::{Message as TungMsg, protocol::frame::coding::CloseCode};

    let base_tmp = if std::path::Path::new("/tmp").is_dir() {
        std::path::PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    let short_id = &Uuid::new_v4().simple().to_string()[..8];
    let temp_home = base_tmp.join(format!("orb_e2ee_replay_{short_id}"));
    std::fs::create_dir_all(&temp_home).expect("mkdir");

    let pre_rt = orbit_daemon::DaemonRuntime::bootstrap(&temp_home)
        .await
        .expect("bootstrap");
    let allowed_pubkey = pre_rt.keypair.public_key_b64.clone();
    let pubkey_bytes = decode_key_32(&allowed_pubkey).expect("decode pubkey");
    let pairing_secret = pre_rt.pairing_secret.clone();
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

    let client_ws_url = format!("{relay_url}/v1/client/{allowed_pubkey}");

    // 1. Legitimate raw E2EE session: capture `msg1` and the first encrypted `Hello` ciphertext.
    let (client_hs, captured_msg1) =
        ClientHandshake::new(&pubkey_bytes, &pairing_secret).expect("client init");
    let (mut legit_ws, _) = tokio_tungstenite::connect_async(&client_ws_url)
        .await
        .expect("connect legit");
    legit_ws
        .send(TungMsg::Binary(captured_msg1.clone().into()))
        .await
        .expect("send msg1");
    let msg2_1 = match tokio::time::timeout(Duration::from_secs(5), legit_ws.next())
        .await
        .expect("msg2 timeout")
        .expect("msg2 frame")
        .expect("msg2 ok")
    {
        TungMsg::Binary(b) => b,
        other => panic!("expected Binary msg2, got {other:?}"),
    };
    assert_eq!(msg2_1.len(), CLIENT_E2EE_MSG_LEN);

    let mut legit_chan = client_hs.finalize(&msg2_1).expect("finalize legit");
    let hello = WireMessage::Hello {
        client_id: "legit-client".into(),
        client_name: "legit".into(),
        protocol_version: orbit_protocol::PROTOCOL_VERSION,
        supports_zstd: true,
    };
    let hello_z = hello.encode_zstd(3).expect("zstd");
    let mut plain = Vec::with_capacity(1 + hello_z.len());
    plain.push(orbit_client_core::E2EE_FRAME_WIRE);
    plain.extend_from_slice(&hello_z);
    let captured_hello_ct = legit_chan.seal_record(&plain).expect("seal hello");
    legit_ws
        .send(TungMsg::Binary(captured_hello_ct.clone().into()))
        .await
        .expect("send hello");

    // Verify legitimate client receives encrypted Welcome after sending first record.
    let welcome_ct = match tokio::time::timeout(Duration::from_secs(5), legit_ws.next())
        .await
        .expect("welcome timeout")
        .expect("welcome frame")
        .expect("welcome ok")
    {
        TungMsg::Binary(b) => b,
        other => panic!("expected Binary Welcome ciphertext, got {other:?}"),
    };
    let welcome_plain = legit_chan.open_record(&welcome_ct).expect("open welcome");
    assert_eq!(welcome_plain[0], orbit_client_core::E2EE_FRAME_WIRE);
    let welcome = WireMessage::decode_zstd(&welcome_plain[1..]).expect("decode welcome");
    assert!(matches!(welcome, WireMessage::Welcome { .. }));
    let _ = legit_ws.close(None).await;

    // 2. Untrusted relay / attacker replays `captured_msg1`, receives fresh `msg2_replay`,
    //    and replays `captured_hello_ct` without possessing the client's ephemeral private key.
    //    Daemon MUST reject and close with Policy (1008) before starting a session or sending Welcome.
    let (mut replay_ws, _) = tokio_tungstenite::connect_async(&client_ws_url)
        .await
        .expect("connect attacker");
    replay_ws
        .send(TungMsg::Binary(captured_msg1.into()))
        .await
        .expect("send replayed msg1");
    let msg2_replay = match tokio::time::timeout(Duration::from_secs(5), replay_ws.next())
        .await
        .expect("msg2_replay timeout")
        .expect("msg2_replay frame")
        .expect("msg2_replay ok")
    {
        TungMsg::Binary(b) => b,
        other => panic!("expected Binary msg2_replay, got {other:?}"),
    };
    assert_ne!(msg2_replay, msg2_1);

    replay_ws
        .send(TungMsg::Binary(captured_hello_ct.into()))
        .await
        .expect("send replayed hello ciphertext");
    let reply = tokio::time::timeout(Duration::from_secs(3), replay_ws.next())
        .await
        .expect("daemon must reject replayed ciphertext promptly");
    assert!(
        matches!(
            reply,
            Some(Ok(TungMsg::Close(Some(ref cf)))) if cf.code == CloseCode::Policy
        ) || matches!(reply, None | Some(Ok(TungMsg::Close(None))) | Some(Err(_))),
        "replayed msg1 + ciphertext must be closed with Policy without sending Welcome, got {reply:?}"
    );

    relay_handle.abort();
    let _ = std::fs::remove_dir_all(&temp_home);
}
