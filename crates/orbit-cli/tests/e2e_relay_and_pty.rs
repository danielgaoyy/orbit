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
    let offer =
        runtime.build_pairing_offer(vec!["ws://127.0.0.1:1/ws".into()], Some(ssh_relay_uri));
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
            },
            Duration::from_secs(5),
            |w| matches!(w, WireMessage::AgentsSnapshot { request_id, .. } if *request_id == req_id),
        )
        .await
        .expect("list agents over ssh+relay");
    assert!(matches!(resp, WireMessage::AgentsSnapshot { .. }));

    let _ = std::fs::remove_dir_all(&temp_home);
}
