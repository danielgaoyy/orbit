//! Audio endpoint construction, socket pinning and host-authorized credential binding.
use super::audio::MAX_AUDIO_BYTES;
use anyhow::{Context, Result, anyhow, ensure};
use orbit_protocol::VoiceConfig;
use reqwest::{Client, RequestBuilder, Url, multipart};
use serde::Deserialize;
use std::{net::IpAddr, time::Duration};

fn loopback(url: &Url) -> bool {
    url.host_str().is_some_and(|s| {
        s == "localhost"
            || s.trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    })
}

pub(super) fn base(raw: &str) -> Result<Url> {
    let mut url = crate::mcp_servers::validate_mcp_http_url(raw).map_err(anyhow::Error::msg)?;
    ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "audio endpoint must be a base URL without query or fragment"
    );
    ensure!(
        loopback(&url) || url.scheme() == "https",
        "remote audio endpoints require HTTPS"
    );
    if !loopback(&url) {
        let host_norm = url
            .host_str()
            .unwrap_or_default()
            .trim_end_matches('.')
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        ensure!(
            !host_norm.ends_with(".localhost") && !host_norm.ends_with(".internal"),
            "remote audio endpoint host is not allowed"
        );
        if let Ok(ip) = host_norm.parse::<IpAddr>() {
            ensure!(
                !crate::goals::is_blocked_health_ssrf_ip(ip),
                "remote audio endpoint IP is blocked by SSRF policy"
            );
        }
    }
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/v1") {
        format!("{path}/")
    } else {
        format!("{path}/v1/")
    };
    url.set_path(&path);
    Ok(url)
}

fn key_for(config: &VoiceConfig, base: &Url) -> Result<Option<String>> {
    let Some(name) = &config.openai_api_key_env else {
        return Ok(None);
    };
    // A paired client may select an endpoint, but cannot redirect an unrelated daemon secret.
    let official = name == "OPENAI_API_KEY" && base.as_str() == "https://api.openai.com/v1/";
    let bound = std::env::var("ORBIT_VOICE_API_KEY_ENV").as_ref() == Ok(name)
        && std::env::var("ORBIT_VOICE_API_BASE")
            .ok()
            .and_then(|v| self::base(&v).ok())
            .as_ref()
            == Some(base);
    ensure!(
        official || bound,
        "audio credential is not bound to this endpoint on the host"
    );
    Ok(Some(
        std::env::var(name).context("voice API key is missing")?,
    ))
}

async fn request(config: &VoiceConfig, route: &str) -> Result<RequestBuilder> {
    let base = base(
        config
            .openai_endpoint_url
            .as_deref()
            .context("audio endpoint is not configured")?,
    )?;
    let mut builder = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(55));
    if loopback(&base) {
        let (host, addrs) = crate::mcp_servers::resolve_and_validate_http_addrs(&base)
            .await
            .map_err(anyhow::Error::msg)?;
        ensure!(
            addrs.iter().all(|a| a.ip().is_loopback()),
            "invalid loopback resolution"
        );
        builder = builder.resolve_to_addrs(&host, &addrs);
    } else {
        let (_, host, addrs) = crate::goals::validate_and_pin_http_health_url(base.as_str())
            .await
            .map_err(anyhow::Error::msg)?;
        builder = builder.resolve_to_addrs(&host, &addrs);
    }
    let mut req = builder.build()?.post(base.join(route)?);
    if let Some(key) = key_for(config, &base)? {
        req = req.bearer_auth(key);
    }
    Ok(req)
}

async fn body(request: RequestBuilder, cap: usize) -> Result<Vec<u8>> {
    let mut response = request
        .send()
        .await
        .map_err(|_| anyhow!("audio endpoint connection failed"))?;
    ensure!(
        response.status().is_success(),
        "audio endpoint returned {}",
        response.status()
    );
    ensure!(
        response.content_length().is_none_or(|n| n <= cap as u64),
        "oversized audio endpoint response"
    );
    let mut buf = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            chunk.len() <= cap - buf.len(),
            "oversized audio endpoint response"
        );
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

#[derive(Deserialize)]
pub(super) struct Transcript {
    pub text: String,
    #[serde(alias = "lang")]
    pub language: Option<String>,
}

pub(super) async fn transcribe(
    config: &VoiceConfig,
    wav: Vec<u8>,
    language: Option<&str>,
) -> Result<Transcript> {
    let model = match config.stt_model_id.as_str() {
        "whisper-base-q5" | "sensevoice-small-int8" => "whisper-1",
        other => other,
    };
    let mut form = multipart::Form::new().text("model", model.to_owned()).part(
        "file",
        multipart::Part::bytes(wav)
            .file_name("capture.wav")
            .mime_str("audio/wav")?,
    );
    if let Some(lang) = language {
        form = form.text("language", lang.to_owned());
    }
    let data = body(
        request(config, "audio/transcriptions")
            .await?
            .multipart(form),
        64 * 1024,
    )
    .await?;
    Ok(serde_json::from_slice(&data)?)
}

pub(super) async fn synthesize(
    config: &VoiceConfig,
    text: &str,
    voice: Option<&str>,
) -> Result<Vec<u8>> {
    let model = if config.tts_model_id == "kokoro-82m-q8" {
        "tts-1"
    } else {
        &config.tts_model_id
    };
    let req = request(config,"audio/speech").await?.json(&serde_json::json!({"model":model,"input":text,"voice":voice.unwrap_or("alloy"),"response_format":"wav"}));
    body(req, MAX_AUDIO_BYTES).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::audio;
    use axum::{Router, body::Bytes, routing::post};
    #[tokio::test]
    async fn endpoint_policy_pins_loopback_rejects_internal_and_binds_credentials() -> Result<()> {
        let cfg = |url: &str| VoiceConfig {
            openai_endpoint_url: Some(url.to_owned()),
            ..Default::default()
        };
        for url in [
            "http://example.com",
            "https://169.254.169.254",
            "https://10.0.0.2",
            "https://[::ffff:127.0.0.1]",
            "file:///tmp/voice",
            "https://x:x@api.openai.com",
            "https://example.com?key=abc",
        ] {
            assert!(request(&cfg(url), "audio/speech").await.is_err(), "{url}");
        }
        assert_eq!(base("http://[::1]:9000/v1")?.path(), "/v1/");
        assert!(
            request(&cfg("http://localhost:9000"), "audio/speech")
                .await
                .is_ok()
        );
        let mut forbidden = cfg("http://127.0.0.1");
        forbidden.openai_api_key_env = Some("PATH".to_owned());
        assert!(request(&forbidden, "audio/speech").await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn compatible_endpoint_receives_multipart_pcm_and_returns_valid_wav() -> Result<()> {
        let wav = audio::encode_wav(&[1, 2, 3], 16000)?;
        let mut out = wav.clone();
        out[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        out[40..44].copy_from_slice(&u32::MAX.to_le_bytes());
        let app = Router::new()
            .route(
                "/v1/audio/transcriptions",
                post(|headers: axum::http::HeaderMap, body: Bytes| async move {
                    assert!(
                        headers["content-type"]
                            .to_str()
                            .is_ok_and(|v| v.starts_with("multipart/form-data; boundary="))
                    );
                    let parts = String::from_utf8_lossy(&body);
                    for item in [
                        "name=\"file\"",
                        "capture.wav",
                        "audio/wav",
                        "RIFF",
                        "whisper-1",
                        "name=\"language\"",
                        "ja",
                    ] {
                        assert!(parts.contains(item), "{item}");
                    }
                    axum::Json(serde_json::json!({"text":"キューの状態","language":"ja"}))
                }),
            )
            .route(
                "/v1/audio/speech",
                post(
                    move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                        assert_eq!(body["response_format"], "wav");
                        assert_eq!(body["input"], "hello");
                        out
                    },
                ),
            );
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let cfg = VoiceConfig {
            tts_backend: orbit_protocol::VoiceTtsBackend::LocalKokoro,
            openai_endpoint_url: Some(format!("http://{}", socket.local_addr()?)),
            ..Default::default()
        };
        let server = tokio::spawn(async move { axum::serve(socket, app).await });
        use base64::{Engine, engine::general_purpose::STANDARD};
        let home = tempfile::tempdir()?;
        let service = crate::voice::VoiceService::new(home.path());
        let pcm: Vec<_> = (0..3200)
            .map(|n| if n % 64 < 32 { 4000i16 } else { -4000i16 })
            .flat_map(i16::to_le_bytes)
            .collect();
        let result = service
            .transcribe(&cfg, STANDARD.encode(pcm), 16000, Some("ja"))
            .await?;
        assert_eq!(result.text, "キューの状態");
        assert_eq!(
            result.backend_used,
            orbit_protocol::VoiceSttBackend::OpenAiCompatible
        );
        let spoken = service
            .synthesize(&cfg, "**hello**\n\n```\ncode\n```", None, None)
            .await?
            .context("expected speech")?;
        assert_eq!(
            spoken.backend_used,
            orbit_protocol::VoiceTtsBackend::OpenAiCompatible
        );
        assert_eq!(STANDARD.decode(spoken.audio_wav_base64)?, wav);
        server.abort();
        Ok(())
    }
}
