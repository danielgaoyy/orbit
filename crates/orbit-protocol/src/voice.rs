//! Host-scoped voice settings, model inventory and advisory command intents.
use serde::{Deserialize, Serialize};

use crate::{GuardianDecision, GuardianRiskLevel, PendingPermissionSummary};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceSttBackend {
    #[default]
    LocalWhisper,
    LocalSenseVoice,
    OpenAiCompatible,
    SystemClientFallback,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceTtsBackend {
    LocalKokoro,
    OpenAiCompatible,
    #[default]
    SystemNative,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub stt_backend: VoiceSttBackend,
    pub tts_backend: VoiceTtsBackend,
    pub language: Option<String>,
    pub vad_threshold: f32,
    pub preroll_ms: u32,
    pub silence_hangover_ms: u32,
    pub auto_speak_responses: bool,
    pub voice_id: Option<String>,
    pub openai_endpoint_url: Option<String>,
    pub openai_api_key_env: Option<String>,
    pub stt_model_id: String,
    pub tts_model_id: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            stt_backend: VoiceSttBackend::LocalWhisper,
            tts_backend: VoiceTtsBackend::SystemNative,
            language: None,
            vad_threshold: 0.012,
            preroll_ms: 300,
            silence_hangover_ms: 750,
            auto_speak_responses: false,
            voice_id: None,
            openai_endpoint_url: None,
            openai_api_key_env: None,
            stt_model_id: "whisper-base-q5".into(),
            tts_model_id: "kokoro-82m-q8".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceModelKind {
    Vad,
    Stt,
    Tts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceModelDescriptor {
    pub id: String,
    pub kind: VoiceModelKind,
    pub name: String,
    /// Size and digest identify the primary weight file at `download_url`.
    pub size_bytes: u64,
    pub sha256: String,
    pub download_url: String,
    /// True only when all required files verify, including voice tables or tokens.
    pub installed: bool,
    pub local_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceHostCapabilities {
    pub local_whisper: bool,
    pub local_sense_voice: bool,
    pub local_kokoro: bool,
    pub system_native_tts: bool,
    pub system_client_fallback: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VoiceIntent {
    SendPrompt { text: String },
    SwitchAgent { query: String },
    ReadLastReply,
    QueueStatus,
    ApprovePermission { request_id: String, confirmed: bool },
    DenyPermission { request_id: String },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceApprovalSafety {
    Allowed,
    RequiresSpokenConfirmation,
    RequiresVisualConfirmation,
}

/// Fail closed on stale or unassessed requests. Evaluate only host-owned pending state.
pub fn evaluate_voice_approval_safety(
    pending: Option<&PendingPermissionSummary>,
    confirmed: bool,
) -> VoiceApprovalSafety {
    let Some(request) = pending else {
        return VoiceApprovalSafety::RequiresVisualConfirmation;
    };
    let standard = !request.human_only
        && crate::permits_allow_decision(&request.allowed_decisions)
        && request.plan_approval.is_none()
        && request.guardian_assessment.as_ref().is_some_and(|a| {
            matches!(
                a.risk_level,
                GuardianRiskLevel::Low | GuardianRiskLevel::Medium
            ) && a.decision != GuardianDecision::AutoDeny
                && !crate::assessment_requires_human_only(a)
                && !a.signals.iter().any(|s| s == "guardian_disabled")
        });
    if !standard {
        VoiceApprovalSafety::RequiresVisualConfirmation
    } else if confirmed {
        VoiceApprovalSafety::Allowed
    } else {
        VoiceApprovalSafety::RequiresSpokenConfirmation
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceConfigState {
    pub config: VoiceConfig,
    pub models: Vec<VoiceModelDescriptor>,
    pub capabilities: VoiceHostCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceTranscriptionResult {
    pub text: String,
    pub language: Option<String>,
    pub duration_ms: u64,
    pub backend_used: VoiceSttBackend,
    pub parsed_intent: Option<VoiceIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_safety: Option<VoiceApprovalSafety>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSpeechResult {
    pub audio_wav_base64: String,
    pub mime_type: String,
    pub duration_ms: u64,
    pub backend_used: VoiceTtsBackend,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GuardianAssessment, PlanApprovalPayload, ToolCallDetail, WireMessage};

    #[test]
    fn approval_requires_exact_assessed_standard_permission() -> Result<(), serde_json::Error> {
        use VoiceApprovalSafety::*;
        let mut pending = PendingPermissionSummary {
            agent_id: "agent".into(),
            agent_title: String::new(),
            workspace_id: "ws".into(),
            request_id: "request".into(),
            title: String::new(),
            detail: ToolCallDetail::FileRead {
                path: "a".into(),
                start_line: None,
                end_line: None,
            },
            allowed_decisions: vec!["allow".into()],
            guardian_assessment: None,
            plan_approval: None,
            human_only: false,
            router_recommendation: None,
            created_at: chrono::Utc::now(),
        };
        assert_eq!(
            evaluate_voice_approval_safety(None, true),
            RequiresVisualConfirmation
        );
        assert_eq!(
            evaluate_voice_approval_safety(Some(&pending), true),
            RequiresVisualConfirmation
        );
        for risk in [
            GuardianRiskLevel::Low,
            GuardianRiskLevel::Medium,
            GuardianRiskLevel::High,
            GuardianRiskLevel::Critical,
        ] {
            pending.guardian_assessment = Some(GuardianAssessment {
                risk_level: risk,
                decision: GuardianDecision::RequireHuman,
                confidence: 1.,
                classifier_tier: "deterministic_tier1".into(),
                reason: String::new(),
                signals: vec![],
                latency_ms: 0,
            });
            let plan: PlanApprovalPayload =
                serde_json::from_value(serde_json::json!({"plan_markdown":"review"}))?;
            for human_only in [false, true] {
                pending.human_only = human_only;
                for is_plan in [false, true] {
                    pending.plan_approval = is_plan.then(|| plan.clone());
                    for confirmed in [false, true] {
                        let expected = if human_only || is_plan || risk >= GuardianRiskLevel::High {
                            RequiresVisualConfirmation
                        } else if confirmed {
                            Allowed
                        } else {
                            RequiresSpokenConfirmation
                        };
                        assert_eq!(
                            evaluate_voice_approval_safety(Some(&pending), confirmed),
                            expected,
                            "{risk:?} human_only={human_only} plan={is_plan} confirmed={confirmed}"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn state_result_wire_is_flat_and_config_defaults_are_compatible()
    -> Result<(), serde_json::Error> {
        let cfg: VoiceConfig = serde_json::from_str(r#"{"enabled":true}"#)?;
        assert_eq!((cfg.preroll_ms, cfg.silence_hangover_ms), (300, 750));
        let msg = WireMessage::VoiceTranscriptionResult {
            request_id: uuid::Uuid::nil(),
            result: VoiceTranscriptionResult {
                text: "hello".into(),
                duration_ms: 200,
                language: Some("en".into()),
                backend_used: VoiceSttBackend::LocalWhisper,
                parsed_intent: None,
                approval_safety: None,
            },
        };
        let json = serde_json::to_value(msg)?;
        assert_eq!(json["op"], "voice_transcription_result");
        assert_eq!(json["text"], "hello");
        assert_eq!(json["backend_used"], "local_whisper");
        assert!(matches!(
            serde_json::from_value(json)?,
            WireMessage::VoiceTranscriptionResult { .. }
        ));
        Ok(())
    }
}
