//! The relay forwards opaque binary records. The client asserts the negotiated format after opening each one.
use futures_util::{SinkExt, StreamExt};
use orbit_crypto::ClientHandshake;
use orbit_protocol::{E2EE_FRAME_WIRE_JSON, PROTOCOL_VERSION, WireMessage};
use orbit_relay::{RelayConfig, run_relay};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[tokio::test]
async fn browser_json_rpc_over_direct_and_relay() -> anyhow::Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().to_path_buf();
    let key = orbit_daemon::DaemonRuntime::bootstrap(&home)
        .await?
        .keypair
        .clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    drop(listener);
    let relay = tokio::spawn(run_relay(RelayConfig {
        listen_addr: addr,
        allowed_pubkeys: Arc::new([key.public_key_b64.clone()].into()),
        fleet_token: None,
    }));
    let (direct, runtime) =
        orbit_daemon::spawn_embedded_daemon(home.clone(), Some(format!("ws://{addr}"))).await?;
    for url in [
        format!("ws://{direct}/ws/e2ee"),
        format!("ws://{addr}/v1/client/{}", key.public_key_b64),
    ] {
        tokio::time::timeout(Duration::from_secs(15), async {
            let mut ws = loop {
                if let Ok((ws, _)) = connect_async(&url).await {
                    break ws;
                }
                tokio::time::sleep(Duration::from_millis(75)).await;
            };
            let (hs, e) = ClientHandshake::new(&key.public_bytes()?, &runtime.pairing_secret)?;
            ws.send(Message::Binary(e.into())).await?;
            let reply = ws
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("closed during handshake"))??;
            let mut channel = hs.finalize(&reply.into_data())?;
            let hello = WireMessage::Hello {
                client_id: "browser".into(),
                client_name: "web".into(),
                protocol_version: PROTOCOL_VERSION,
                supports_zstd: false,
            };
            for msg in [hello, WireMessage::Ping { ts_ms: 123 }] {
                ws.send(Message::Binary(
                    channel.seal_record(&msg.encode_e2ee(false)?)?.into(),
                ))
                .await?;
                let record = ws
                    .next()
                    .await
                    .ok_or_else(|| anyhow::anyhow!("closed during RPC"))??;
                let Message::Binary(bytes) = record else {
                    anyhow::bail!("expected ciphertext")
                };
                let plain = channel.open_record(&bytes)?;
                assert_eq!(plain.first(), Some(&E2EE_FRAME_WIRE_JSON));
                match (msg, WireMessage::decode_e2ee(&plain)?) {
                    (WireMessage::Hello { .. }, WireMessage::Welcome { host }) => {
                        assert_eq!(host.host_id, key.host_id()?)
                    }
                    (WireMessage::Ping { ts_ms }, WireMessage::Pong { ts_ms: echo }) => {
                        assert_eq!(ts_ms, echo)
                    }
                    _ => anyhow::bail!("unexpected encrypted response"),
                }
            }
            ws.close(None).await?;
            anyhow::Ok(())
        })
        .await??;
    }
    relay.abort();
    drop(runtime);
    Ok(())
}

#[tokio::test]
async fn foreground_pwa_preserves_api_routes_and_confines_asset_reads() -> anyhow::Result<()> {
    use tokio::process::Command;
    let tmp_dir = tempfile::tempdir()?;
    let tmp = tmp_dir.path();
    let assets = tmp.join("dist");
    std::fs::create_dir_all(&assets)?;
    std::fs::write(assets.join("index.html"), "Orbit shell")?;
    std::fs::write(tmp.join("outside.js"), "private")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.join("outside.js"), assets.join("linked.js"))?;
    let client = reqwest::Client::new();
    for role in ["daemon", "relay"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        drop(listener);
        let mut child = Command::new(env!("CARGO_BIN_EXE_orbit"))
            .env("ORBIT_HOME", tmp.join(role))
            .arg(role)
            .arg("--listen")
            .arg(addr.to_string())
            .arg("--serve-web")
            .arg(&assets)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        let base = format!("http://{addr}");
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    anyhow::bail!("foreground {role} exited: {status}");
                }
                if client
                    .get(format!("{base}/healthz"))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            anyhow::Ok(())
        })
        .await??;
        let shell = client.get(&base).send().await?;
        assert_eq!(shell.headers()["x-content-type-options"], "nosniff");
        assert_eq!(shell.headers()["referrer-policy"], "no-referrer");
        assert!(
            shell.headers()["content-security-policy"]
                .to_str()?
                .contains("object-src 'none'")
        );
        assert_eq!(shell.text().await?, "Orbit shell");
        if role == "daemon" {
            for ws_path in ["/ws/e2ee", "/ws"] {
                let evil = client
                    .get(format!("{base}{ws_path}"))
                    .header("origin", "https://evil.example")
                    .header("connection", "Upgrade")
                    .header("upgrade", "websocket")
                    .header("sec-websocket-version", "13")
                    .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                    .send()
                    .await?;
                assert_eq!(
                    evil.status(),
                    403,
                    "cross-origin {ws_path} must be rejected with 403"
                );
            }
            let mut same_origin_req = format!("ws://{addr}/ws/e2ee").into_client_request()?;
            same_origin_req
                .headers_mut()
                .insert("origin", format!("http://{addr}").parse()?);
            let (mut ws, _) = tokio_tungstenite::connect_async(same_origin_req).await?;
            let _ = ws.close(None).await;
        }
        for path in ["/linked.js", "/%2e%2e%2foutside.js", "/outside.js"] {
            assert_eq!(
                client.get(format!("{base}{path}")).send().await?.status(),
                404
            );
        }
        child.kill().await?;
        let _ = child.wait().await;
    }
    Ok(())
}
