//! Host-local voice capture processing, optional inference and permission-aware intent routing.
pub mod audio;
mod endpoint;
mod intent;
mod local;
mod models;
pub use audio::{PreRollAudioBuffer, VoiceActivityDetector};
pub use intent::{parse_voice_intent, sanitize_for_speech};
pub use models::VoiceModelManager;

use crate::{DaemonRuntime, manager::AgentOrchestrator};
use anyhow::{Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use orbit_protocol::{
    VoiceApprovalSafety, VoiceConfig, VoiceConfigState, VoiceIntent, VoiceSpeechResult,
    VoiceSttBackend, VoiceTranscriptionResult, VoiceTtsBackend, WireMessage,
    evaluate_voice_approval_safety,
};
use std::{
    io::{ErrorKind, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use uuid::Uuid;

pub(crate) struct VoiceService {
    home: PathBuf,
    models: VoiceModelManager,
    pub(super) slots: Arc<tokio::sync::Semaphore>,
}

fn language(raw: Option<&str>) -> Result<Option<&str>> {
    match raw {
        None | Some("auto") => Ok(None),
        Some(s @ ("en" | "zh" | "ja")) => Ok(Some(s)),
        _ => anyhow::bail!("unsupported voice language"),
    }
}

fn validate_voice_id(id: Option<&str>) -> Result<()> {
    ensure!(
        id.is_none_or(|s| !s.is_empty()
            && s.len() <= 80
            && !s.starts_with('-')
            && s.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' ' | '.'))),
        "invalid voice ID"
    );
    Ok(())
}

fn validate_config(cfg: &VoiceConfig) -> Result<()> {
    language(cfg.language.as_deref())?;
    validate_voice_id(cfg.voice_id.as_deref())?;
    ensure!(
        cfg.vad_threshold.is_finite()
            && (0.0..=1.0).contains(&cfg.vad_threshold)
            && cfg.preroll_ms <= 2000
            && cfg.silence_hangover_ms <= 5000,
        "invalid VAD settings"
    );
    for id in [&cfg.stt_model_id, &cfg.tts_model_id] {
        ensure!(
            !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control),
            "invalid voice model name"
        );
    }
    if let Some(url) = &cfg.openai_endpoint_url {
        endpoint::base(url)?;
    }
    if let Some(name) = &cfg.openai_api_key_env {
        ensure!(
            !name.is_empty()
                && name.len() <= 128
                && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'),
            "invalid audio API key environment name"
        );
    }
    Ok(())
}

fn bounded_text(text: &str) -> Result<()> {
    ensure!(text.len() <= 64 * 1024, "voice text exceeds 64 KiB");
    Ok(())
}

impl VoiceService {
    pub(crate) fn new(home: &Path) -> Self {
        Self {
            home: home.into(),
            models: VoiceModelManager::new(home),
            slots: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }
    async fn config(&self) -> Result<VoiceConfig> {
        let path = self.home.join("voice.json");
        tokio::task::spawn_blocking(move || {
            match std::fs::symlink_metadata(&path) {
                Ok(m) => ensure!(
                    m.is_file() && m.len() <= 16384,
                    "invalid voice configuration file"
                ),
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(VoiceConfig::default()),
                Err(e) => return Err(e.into()),
            }
            let mut raw = Vec::new();
            std::fs::File::open(path)?
                .take(16385)
                .read_to_end(&mut raw)?;
            ensure!(raw.len() <= 16384, "oversized voice configuration");
            let cfg = serde_json::from_slice(&raw)?;
            validate_config(&cfg)?;
            Ok(cfg)
        })
        .await?
    }
    async fn enabled_config(&self) -> Result<VoiceConfig> {
        let cfg = self.config().await?;
        ensure!(cfg.enabled, "voice is disabled on this host");
        Ok(cfg)
    }
    async fn save(&self, cfg: &VoiceConfig) -> Result<()> {
        validate_config(cfg)?;
        let home = self.home.clone();
        let bytes = serde_json::to_vec_pretty(cfg)?;
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(&home)?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            file.persist(home.join("voice.json"))?;
            Ok(())
        })
        .await?
    }

    async fn snapshot(&self, request_id: Uuid, config: VoiceConfig) -> Result<WireMessage> {
        let models = self.models.list().await?;
        let capabilities = tokio::task::spawn_blocking(local::capabilities).await?;
        Ok(WireMessage::VoiceConfigState {
            request_id,
            state: VoiceConfigState {
                config,
                models,
                capabilities,
            },
        })
    }

    async fn transcribe(
        &self,
        cfg: &VoiceConfig,
        pcm: String,
        rate: u32,
        lang: Option<&str>,
    ) -> Result<VoiceTranscriptionResult> {
        ensure!(
            pcm.len() <= audio::MAX_AUDIO_BYTES.div_ceil(3) * 4,
            "encoded audio exceeds limit"
        );
        let framing = cfg.clone();
        let (duration_ms, wav) = tokio::task::spawn_blocking(move || {
            let samples = audio::decode_pcm(&STANDARD.decode(pcm)?, rate)?;
            let duration = samples.len() as u64 * 1000 / u64::from(rate);
            if framing.stt_backend == VoiceSttBackend::SystemClientFallback {
                return anyhow::Ok((duration, None));
            }
            let samples = audio::resample_to_16k(&samples, rate)?;
            let speech = audio::speech_frames(&samples, &framing);
            let wav = (!speech.is_empty())
                .then(|| audio::encode_wav(&speech, audio::SAMPLE_RATE))
                .transpose()?;
            Ok((duration, wav))
        })
        .await??;
        let mut result = VoiceTranscriptionResult {
            text: String::new(),
            language: lang.map(str::to_owned),
            duration_ms,
            backend_used: cfg.stt_backend,
            parsed_intent: None,
            approval_safety: None,
        };
        let Some(wav) = wav else { return Ok(result) };
        let expected_model = match cfg.stt_backend {
            VoiceSttBackend::LocalWhisper => Some("whisper-base-q5"),
            VoiceSttBackend::LocalSenseVoice => Some("sensevoice-small-int8"),
            _ => None,
        };
        if expected_model == Some(cfg.stt_model_id.as_str())
            && let Some(model) = self.models.open_verified(&cfg.stt_model_id).await?
        {
            match local::transcribe(cfg.stt_backend, &model.path, &wav, lang).await {
                Ok(t) => {
                    result.text = t.text;
                    result.language = t.language.or(result.language);
                    return Ok(result);
                }
                Err(error) => {
                    tracing::warn!(%error, backend = ?cfg.stt_backend, "local voice STT failed; falling back");
                }
            }
        }
        if cfg.openai_endpoint_url.is_some() {
            match tokio::time::timeout(
                Duration::from_secs(60),
                endpoint::transcribe(cfg, wav, lang),
            )
            .await
            {
                Ok(Ok(t)) => {
                    result.backend_used = VoiceSttBackend::OpenAiCompatible;
                    result.text = t.text;
                    result.language = t.language.or(result.language);
                    return Ok(result);
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "OpenAI-compatible voice STT failed; falling back to client");
                }
                Err(_) => {
                    tracing::warn!("OpenAI-compatible voice STT timed out; falling back to client");
                }
            }
        }
        result.backend_used = VoiceSttBackend::SystemClientFallback;
        Ok(result)
    }

    async fn synthesize(
        &self,
        cfg: &VoiceConfig,
        text: &str,
        lang: Option<&str>,
        voice: Option<&str>,
    ) -> Result<Option<VoiceSpeechResult>> {
        bounded_text(text)?;
        let text = sanitize_for_speech(text);
        ensure!(!text.is_empty(), "nothing to speak");
        validate_voice_id(voice)?;
        if cfg.tts_backend == VoiceTtsBackend::LocalKokoro
            && cfg.tts_model_id == "kokoro-82m-q8"
            && let Some(model) = self.models.open_verified(&cfg.tts_model_id).await?
        {
            match local::kokoro(&model.path, &text, lang, voice).await {
                Ok(wav) => match speech_result_async(wav, VoiceTtsBackend::LocalKokoro).await {
                    Ok(result) => return Ok(Some(result)),
                    Err(error) => tracing::warn!(%error, "failed to encode local Kokoro WAV"),
                },
                Err(error) => {
                    tracing::warn!(%error, "local Kokoro TTS failed; falling back");
                }
            }
        }
        if matches!(
            cfg.tts_backend,
            VoiceTtsBackend::LocalKokoro | VoiceTtsBackend::OpenAiCompatible
        ) && cfg.openai_endpoint_url.is_some()
        {
            match tokio::time::timeout(
                Duration::from_secs(60),
                endpoint::synthesize(cfg, &text, voice),
            )
            .await
            {
                Ok(Ok(wav)) => {
                    match speech_result_async(wav, VoiceTtsBackend::OpenAiCompatible).await {
                        Ok(result) => return Ok(Some(result)),
                        Err(error) => {
                            tracing::warn!(%error, "failed to encode endpoint TTS WAV")
                        }
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(%error, "OpenAI-compatible TTS failed; falling back");
                }
                Err(_) => {
                    tracing::warn!("OpenAI-compatible TTS timed out; falling back");
                }
            }
        }
        match local::native(&text, lang, voice).await {
            Ok(wav) => match speech_result_async(wav, VoiceTtsBackend::SystemNative).await {
                Ok(result) => return Ok(Some(result)),
                Err(error) => tracing::warn!(%error, "failed to encode native TTS WAV"),
            },
            Err(error) => {
                tracing::warn!(%error, "native OS TTS unavailable; delegating to client");
            }
        }
        Ok(None)
    }
}

async fn speech_result_async(
    wav: Vec<u8>,
    backend_used: VoiceTtsBackend,
) -> Result<VoiceSpeechResult> {
    tokio::task::spawn_blocking(move || speech_result(wav, backend_used)).await?
}

fn speech_result(wav: Vec<u8>, backend_used: VoiceTtsBackend) -> Result<VoiceSpeechResult> {
    let (rate, samples) =
        audio::decode_wav_output(&wav, backend_used == VoiceTtsBackend::OpenAiCompatible)?;
    Ok(VoiceSpeechResult {
        audio_wav_base64: STANDARD.encode(audio::encode_wav(&samples, rate)?),
        mime_type: "audio/wav".into(),
        duration_ms: samples.len() as u64 * 1000 / u64::from(rate),
        backend_used,
    })
}

async fn route(
    orchestrator: &AgentOrchestrator,
    result: &mut VoiceTranscriptionResult,
    agent: Option<&str>,
    locale: Option<&str>,
) -> Result<()> {
    bounded_text(&result.text)?;
    result.parsed_intent = parse_voice_intent(&result.text, locale);
    if let Some(VoiceIntent::ApprovePermission {
        request_id,
        confirmed,
    }) = &result.parsed_intent
    {
        let pending = match agent {
            Some(agent) => orchestrator.get_pending_permission(agent, request_id).await,
            None => None,
        };
        let safety = evaluate_voice_approval_safety(pending.as_ref(), *confirmed);
        result.approval_safety = Some(safety);
        if safety == VoiceApprovalSafety::RequiresVisualConfirmation {
            result.parsed_intent = None;
        }
    }
    Ok(())
}

pub(super) async fn handle(
    runtime: &Arc<DaemonRuntime>,
    request_id: Uuid,
    msg: WireMessage,
) -> Result<WireMessage> {
    let service = &runtime.voice;
    match msg {
        WireMessage::GetVoiceConfig { .. } => {
            let cfg = service.config().await?;
            service.snapshot(request_id, cfg).await
        }
        WireMessage::UpdateVoiceConfig { config, .. } => {
            service.save(&config).await?;
            service.snapshot(request_id, config).await
        }
        WireMessage::DownloadVoiceModel { model_id, .. } => {
            service.models.download(&model_id).await?;
            let cfg = service.config().await?;
            service.snapshot(request_id, cfg).await
        }
        WireMessage::DeleteVoiceModel { model_id, .. } => {
            service.models.delete(&model_id).await?;
            let cfg = service.config().await?;
            service.snapshot(request_id, cfg).await
        }
        WireMessage::TranscribeVoiceAudio {
            pcm16_le_base64,
            sample_rate_hz,
            language: lang,
            agent_id,
            ..
        } => {
            let cfg = service.enabled_config().await?;
            let lang = language(lang.as_deref().or(cfg.language.as_deref()))?;
            let mut result = service
                .transcribe(&cfg, pcm16_le_base64, sample_rate_hz, lang)
                .await?;
            route(
                &runtime.orchestrator,
                &mut result,
                agent_id.as_deref(),
                lang,
            )
            .await?;
            Ok(WireMessage::VoiceTranscriptionResult { request_id, result })
        }
        WireMessage::ParseVoiceTranscript {
            text,
            language: lang,
            agent_id,
            ..
        } => {
            let cfg = service.enabled_config().await?;
            let lang = language(lang.as_deref().or(cfg.language.as_deref()))?;
            let mut result = VoiceTranscriptionResult {
                text,
                language: lang.map(str::to_owned),
                duration_ms: 0,
                backend_used: VoiceSttBackend::SystemClientFallback,
                parsed_intent: None,
                approval_safety: None,
            };
            route(
                &runtime.orchestrator,
                &mut result,
                agent_id.as_deref(),
                lang,
            )
            .await?;
            Ok(WireMessage::VoiceTranscriptionResult { request_id, result })
        }
        WireMessage::SynthesizeVoiceSpeech {
            text,
            language: lang,
            voice_id,
            ..
        } => {
            let cfg = service.enabled_config().await?;
            if cfg.tts_backend == VoiceTtsBackend::Disabled {
                return Ok(crate::rpc_err(
                    request_id,
                    "voice_tts_disabled",
                    "speech synthesis is disabled",
                ));
            }
            let lang = language(lang.as_deref().or(cfg.language.as_deref()))?;
            match service
                .synthesize(
                    &cfg,
                    &text,
                    lang,
                    voice_id.as_deref().or(cfg.voice_id.as_deref()),
                )
                .await?
            {
                Some(result) => Ok(WireMessage::VoiceSpeechResult { request_id, result }),
                None => Ok(crate::rpc_err(
                    request_id,
                    "voice_client_tts_required",
                    "use client speech synthesis",
                )),
            }
        }
        WireMessage::ApproveVoicePermission {
            agent_id,
            permission_request_id,
            confirmed,
            ..
        } => {
            service.enabled_config().await?;
            let pending = runtime
                .orchestrator
                .get_pending_permission(&agent_id, &permission_request_id)
                .await;
            let mut safety = evaluate_voice_approval_safety(pending.as_ref(), confirmed);
            if let Some(pending) = pending
                && safety == VoiceApprovalSafety::Allowed
                && !runtime
                    .orchestrator
                    .approve_voice_permission(pending)
                    .await?
            {
                safety = VoiceApprovalSafety::RequiresVisualConfirmation;
            }
            Ok(WireMessage::VoicePermissionResult { request_id, safety })
        }
        _ => anyhow::bail!("unknown voice operation"),
    }
}
