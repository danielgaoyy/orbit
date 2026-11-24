//! Real host sessions and CLI commands over the authenticated voice RPC transport.
use anyhow::{Context, Result};
use orbit_client_core::{HostConnectionState, app_core::OrbitAppCore};
use orbit_daemon::{spawn_embedded_daemon, voice::audio};
use orbit_protocol::{
    VoiceApprovalSafety, VoiceConfig, VoiceIntent, VoiceSttBackend, VoiceTtsBackend,
};
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::process::Command;

async fn cli(home: &Path, server: &str, args: &[&str]) -> Result<Value> {
    let out = Command::new(env!("CARGO_BIN_EXE_orbit"))
        .arg("--home")
        .arg(home)
        .args(["--server", server, "--json", "voice"])
        .args(args)
        .output()
        .await?;
    anyhow::ensure!(
        out.status.success(),
        "voice CLI failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(serde_json::from_slice(&out.stdout)?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn voice_rpc_is_host_scoped_persisted_and_fail_closed() -> Result<()> {
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("host");
    let (addr, rt) = spawn_embedded_daemon(home.clone(), None).await?;
    let server = format!("ws://{addr}/ws");
    let uri = rt
        .build_pairing_offer(vec![server.clone()], None)?
        .to_qr_uri()?;
    let db = tmp.path().join("client.db");
    let core = tokio::task::spawn_blocking(move || OrbitAppCore::open(db, false)).await??;
    let host = core.pair_host(&uri)?.host_id;
    core.connect_host_async(host.clone());
    tokio::time::timeout(Duration::from_secs(10), async {
        while !core
            .host_detail(&host)
            .is_some_and(|d| d.host.connection_state == HostConnectionState::OnlineDirect)
        {
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await?;
    assert!(!core.get_voice_config(&host).await?.config.enabled);
    assert!(core.get_voice_config("other-host").await.is_err());
    let cfg = VoiceConfig {
        enabled: true,
        tts_backend: VoiceTtsBackend::Disabled,
        ..Default::default()
    };
    core.update_voice_config(&host, cfg.clone()).await?;
    assert_eq!(
        serde_json::from_slice::<VoiceConfig>(&std::fs::read(home.join("voice.json"))?)?,
        cfg
    );
    assert_eq!(
        cli(&home, &server, &["status"]).await?["config"]["enabled"],
        true
    );
    assert_eq!(
        cli(&home, &server, &["models"])
            .await?
            .as_array()
            .context("model list")?
            .len(),
        4
    );
    assert!(
        core.download_voice_model(&host, "../outside".into())
            .await
            .is_err()
    );
    assert!(
        core.delete_voice_model(&host, "silero-vad-v5".into())
            .await?
            .models
            .iter()
            .all(|m| !m.installed)
    );

    let samples: Vec<_> = (0..48000)
        .map(|n| ((n as f32 * std::f32::consts::TAU / 180.).sin() * 9000.) as i16)
        .collect();
    let capture = tmp.path().join("clip.wav");
    std::fs::write(&capture, audio::encode_wav(&samples, 48000)?)?;
    let result = cli(
        &home,
        &server,
        &["transcribe", capture.to_str().context("WAV path")?],
    )
    .await?;
    assert_eq!(result["backend_used"], "system_client_fallback");
    assert_eq!(result["duration_ms"], 1000);

    let parsed = core
        .parse_voice_transcript(&host, "队列状态".into(), Some("zh".into()), None)
        .await?;
    assert_eq!(parsed.backend_used, VoiceSttBackend::SystemClientFallback);
    assert_eq!(parsed.parsed_intent, Some(VoiceIntent::QueueStatus));
    let missing = core
        .parse_voice_transcript(
            &host,
            "confirm approve Req_5".into(),
            None,
            Some("agent".into()),
        )
        .await?;
    assert_eq!(
        missing.approval_safety,
        Some(VoiceApprovalSafety::RequiresVisualConfirmation)
    );
    assert!(missing.parsed_intent.is_none());
    assert_eq!(
        core.approve_voice_permission(&host, "agent".into(), "Req_5".into(), true)
            .await?,
        VoiceApprovalSafety::RequiresVisualConfirmation
    );
    assert_eq!(
        core.synthesize_voice_speech(&host, "hello".into(), None, None)
            .await
            .expect_err("TTS disabled")
            .code,
        "voice_tts_disabled"
    );
    tokio::task::spawn_blocking(move || drop(core)).await?;
    Ok(())
}
