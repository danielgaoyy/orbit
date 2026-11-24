use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Subcommand;
use orbit_client_core::HostSessionHandle;
use orbit_daemon::voice::audio;
use orbit_protocol::WireMessage;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Subcommand)]
pub enum VoiceAction {
    Status,
    Models,
    Download { id: String },
    Transcribe { wav: PathBuf },
}

pub async fn run(session: &HostSessionHandle, action: VoiceAction, json: bool) -> Result<()> {
    let request_id = Uuid::new_v4();
    let models_only = matches!(action, VoiceAction::Models);
    let req = match action {
        VoiceAction::Status | VoiceAction::Models => WireMessage::GetVoiceConfig { request_id },
        VoiceAction::Download { id } => WireMessage::DownloadVoiceModel {
            request_id,
            model_id: id,
        },
        VoiceAction::Transcribe { wav } => {
            let (rate, samples) = audio::decode_wav(&audio::read_audio_file(&wav)?)?;
            let pcm = audio::resample_to_16k(&samples, rate)?;
            let bytes: Vec<_> = pcm.into_iter().flat_map(i16::to_le_bytes).collect();
            WireMessage::TranscribeVoiceAudio {
                request_id,
                pcm16_le_base64: STANDARD.encode(bytes),
                sample_rate_hz: audio::SAMPLE_RATE,
                agent_id: None,
                language: None,
            }
        }
    };
    match session.request_voice(req).await? {
        WireMessage::VoiceConfigState { state, .. } if models_only => {
            if json {
                println!("{}", serde_json::to_string_pretty(&state.models)?)
            } else {
                for m in state.models {
                    println!(
                        "{}\t{}\t{}",
                        m.id,
                        if m.installed {
                            "installed"
                        } else {
                            "available"
                        },
                        m.name
                    );
                }
            }
        }
        WireMessage::VoiceConfigState { state, .. } => {
            println!("{}", serde_json::to_string_pretty(&state)?)
        }
        WireMessage::VoiceTranscriptionResult { result, .. } => {
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if result.backend_used == orbit_protocol::VoiceSttBackend::SystemClientFallback {
                anyhow::bail!("host STT unavailable; use speech recognition on the client")
            } else {
                println!("{}", result.text);
            }
        }
        _ => anyhow::bail!("unexpected voice response"),
    }
    Ok(())
}
