//! AI Risk-Assessment Auto-Approval (`Auto` / Guardian Classifier) wire types and policy configuration.

use serde::{Deserialize, Serialize};

use crate::ProviderKind;

fn guardian_default_true() -> bool {
    true
}

fn guardian_default_timeout_ms() -> u64 {
    4_000
}

fn guardian_default_llm_timeout_ms() -> u64 {
    GUARDIAN_DEFAULT_LLM_TIMEOUT_MS
}

pub const GUARDIAN_MIN_TIMEOUT_MS: u64 = 250;
pub const GUARDIAN_MAX_TIMEOUT_MS: u64 = 30_000;
pub const GUARDIAN_DEFAULT_LLM_TIMEOUT_MS: u64 = 8_000;
pub const GUARDIAN_MIN_LLM_TIMEOUT_MS: u64 = 250;
pub const GUARDIAN_MAX_LLM_TIMEOUT_MS: u64 = 20_000;
pub const GUARDIAN_MAX_POLICY_PATTERNS: usize = 128;
pub const GUARDIAN_MAX_MODEL_NAME_CHARS: usize = 128;

/// Risk classification produced by the hybrid deterministic + rubric/AI Guardian classifier.
/// Ordered from lowest (`Low`) to highest (`Critical`) risk.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GuardianRiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

impl GuardianRiskLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub const fn display_label(self) -> &'static str {
        match self {
            Self::Low => "Low risk",
            Self::Medium => "Medium risk",
            Self::High => "High risk",
            Self::Critical => "Critical risk",
        }
    }
}

/// Final enforcement action emitted by Guardian for a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianDecision {
    AutoAllow,
    RequireHuman,
    AutoDeny,
}

impl GuardianDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoAllow => "auto_allow",
            Self::RequireHuman => "require_human",
            Self::AutoDeny => "auto_deny",
        }
    }
}

/// Host-local Guardian auto-approval policy persisted in `$ORBIT_HOME/guardian.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardianPolicyConfig {
    #[serde(default = "guardian_default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_approve_max_risk: GuardianRiskLevel,
    #[serde(default = "guardian_default_true")]
    pub use_ai_classifier_for_medium: bool,
    #[serde(default = "guardian_default_timeout_ms")]
    pub classifier_timeout_ms: u64,
    #[serde(default = "guardian_default_true")]
    pub allow_read_only_tools: bool,
    #[serde(default = "guardian_default_true")]
    pub allow_workspace_tests: bool,
    #[serde(default)]
    pub blocked_command_patterns: Vec<String>,
    #[serde(default)]
    pub trusted_command_prefixes: Vec<String>,
    /// Enable fast-model LLM Tier-2 risk classification for Medium/ambiguous actions.
    #[serde(default = "guardian_default_true")]
    pub llm_tier2_enabled: bool,
    /// Optional provider override for LLM Tier-2 classification (defaults to agent provider / auto-select).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_provider: Option<ProviderKind>,
    /// Optional fast model override for LLM Tier-2 classification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_model: Option<String>,
    /// Bounded timeout for LLM Tier-2 classification (default 8000ms, capped <= 20000ms).
    #[serde(default = "guardian_default_llm_timeout_ms")]
    pub llm_timeout_ms: u64,
}

impl Default for GuardianPolicyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_approve_max_risk: GuardianRiskLevel::Low,
            use_ai_classifier_for_medium: true,
            classifier_timeout_ms: guardian_default_timeout_ms(),
            allow_read_only_tools: true,
            allow_workspace_tests: true,
            blocked_command_patterns: Vec::new(),
            trusted_command_prefixes: Vec::new(),
            llm_tier2_enabled: true,
            llm_provider: None,
            llm_model: None,
            llm_timeout_ms: guardian_default_llm_timeout_ms(),
        }
    }
}

impl GuardianPolicyConfig {
    /// Normalize bounds and deduplicate custom policy lists so persisted configs stay safe and clean.
    pub fn sanitized(mut self) -> Self {
        // Security invariant: never allow auto-approving High or Critical risk actions even if a
        // tampered config requests it; cap `auto_approve_max_risk` at `Medium`.
        if self.auto_approve_max_risk > GuardianRiskLevel::Medium {
            self.auto_approve_max_risk = GuardianRiskLevel::Medium;
        }
        self.classifier_timeout_ms = self
            .classifier_timeout_ms
            .clamp(GUARDIAN_MIN_TIMEOUT_MS, GUARDIAN_MAX_TIMEOUT_MS);
        self.llm_timeout_ms = self
            .llm_timeout_ms
            .clamp(GUARDIAN_MIN_LLM_TIMEOUT_MS, GUARDIAN_MAX_LLM_TIMEOUT_MS);
        self.llm_model = self
            .llm_model
            .take()
            .map(|m| m.trim().to_string())
            .filter(|m| {
                !m.is_empty()
                    && m.chars().count() <= GUARDIAN_MAX_MODEL_NAME_CHARS
                    && !m.chars().any(|c| c == '\0' || c.is_control())
            });
        self.blocked_command_patterns = clean_pattern_list(self.blocked_command_patterns);
        self.trusted_command_prefixes = clean_pattern_list(self.trusted_command_prefixes);
        self
    }
}

fn clean_pattern_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in raw {
        let trimmed = item.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            out.push(trimmed.to_string());
            if out.len() >= GUARDIAN_MAX_POLICY_PATTERNS {
                break;
            }
        }
    }
    out
}

/// Risk evaluation verdict produced by Guardian for a single permission request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardianAssessment {
    pub risk_level: GuardianRiskLevel,
    pub decision: GuardianDecision,
    pub confidence: f32,
    /// Classifier tier that produced this decision:
    /// `"deterministic_tier1" | "llm_tier2" | "heuristic_fallback" | "fail_closed"`.
    pub classifier_tier: String,
    pub reason: String,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub latency_ms: u64,
}

/// Returns `true` whenever a Guardian assessment MUST be resolved strictly by a human operator
/// and NEVER delegated to Personal Router or approved by an agent:
/// - `High` or `Critical` risk level
/// - `fail_closed` classifier tier
/// - Fail-closed / fallback LLM errors, hook blocks, or explicit policy-disabled signals
pub fn assessment_requires_human_only(assessment: &GuardianAssessment) -> bool {
    if assessment.risk_level >= GuardianRiskLevel::High {
        return true;
    }
    if assessment.classifier_tier == "fail_closed" {
        return true;
    }
    if assessment.classifier_tier == "heuristic_fallback"
        && assessment.decision == GuardianDecision::RequireHuman
        && assessment.confidence <= 0.0
    {
        return true;
    }
    assessment.signals.iter().any(|s| {
        matches!(
            s.as_str(),
            "fail_closed"
                | "pre_tool_use_hook_blocked"
                | "read_only_disabled"
                | "workspace_tests_disabled"
                | "llm_timeout"
                | "llm_malformed_json"
                | "llm_unavailable"
                | "llm_error"
                | "llm_provider_unavailable"
                | "llm_provider_unsupported"
                | "invalid_classifier_response"
        )
    })
}

/// Persisted / in-memory audit record of a Guardian decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuardianAuditEntry {
    pub id: String,
    pub agent_id: String,
    pub request_id: String,
    pub tool_name: String,
    pub target_summary: String,
    pub assessment: GuardianAssessment,
    pub decided_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guardian_policy_defaults_and_sanitization() {
        let cfg = GuardianPolicyConfig::default();
        assert!(cfg.enabled);
        assert_eq!(cfg.auto_approve_max_risk, GuardianRiskLevel::Low);
        assert!(cfg.use_ai_classifier_for_medium);
        assert_eq!(cfg.classifier_timeout_ms, 4_000);
        assert!(cfg.allow_read_only_tools);
        assert!(cfg.allow_workspace_tests);
        assert!(cfg.llm_tier2_enabled);
        assert_eq!(cfg.llm_provider, None);
        assert_eq!(cfg.llm_model, None);
        assert_eq!(cfg.llm_timeout_ms, GUARDIAN_DEFAULT_LLM_TIMEOUT_MS);

        // Tampered config attempting to raise max risk to Critical or exceed 20s LLM timeout is clamped.
        let clamped = GuardianPolicyConfig {
            auto_approve_max_risk: GuardianRiskLevel::Critical,
            classifier_timeout_ms: 10,
            llm_timeout_ms: 99_999,
            llm_provider: Some(ProviderKind::ClaudeCode),
            llm_model: Some("  claude-haiku-4-5  ".into()),
            blocked_command_patterns: vec![
                "  rm -rf ".into(),
                "RM -RF".into(),
                "".into(),
                "sudo".into(),
            ],
            trusted_command_prefixes: vec!["just test".into(), "  ".into()],
            ..GuardianPolicyConfig::default()
        }
        .sanitized();
        assert_eq!(clamped.auto_approve_max_risk, GuardianRiskLevel::Medium);
        assert_eq!(clamped.classifier_timeout_ms, GUARDIAN_MIN_TIMEOUT_MS);
        assert_eq!(clamped.llm_timeout_ms, GUARDIAN_MAX_LLM_TIMEOUT_MS);
        assert_eq!(clamped.llm_provider, Some(ProviderKind::ClaudeCode));
        assert_eq!(clamped.llm_model.as_deref(), Some("claude-haiku-4-5"));
        assert_eq!(clamped.blocked_command_patterns, vec!["rm -rf", "sudo"]);
        assert_eq!(clamped.trusted_command_prefixes, vec!["just test"]);

        // Backward compatibility: legacy JSON lacking new LLM fields deserializes cleanly with defaults.
        let legacy: GuardianPolicyConfig = serde_json::from_str(
            r#"{"enabled":true,"auto_approve_max_risk":"low","use_ai_classifier_for_medium":true}"#,
        )
        .expect("deserialize legacy guardian config");
        assert!(legacy.llm_tier2_enabled);
        assert_eq!(legacy.llm_provider, None);
        assert_eq!(legacy.llm_model, None);
        assert_eq!(legacy.llm_timeout_ms, GUARDIAN_DEFAULT_LLM_TIMEOUT_MS);
    }

    #[test]
    fn guardian_risk_ordering_and_serde() {
        assert!(GuardianRiskLevel::Low < GuardianRiskLevel::Medium);
        assert!(GuardianRiskLevel::Medium < GuardianRiskLevel::High);
        assert!(GuardianRiskLevel::High < GuardianRiskLevel::Critical);

        let assessment = GuardianAssessment {
            risk_level: GuardianRiskLevel::High,
            decision: GuardianDecision::RequireHuman,
            confidence: 0.98,
            classifier_tier: "deterministic".into(),
            reason: "Outside workspace write".into(),
            signals: vec!["outside_workspace".into()],
            latency_ms: 1,
        };
        let raw = serde_json::to_string(&assessment).expect("serialize assessment");
        let parsed: GuardianAssessment = serde_json::from_str(&raw).expect("deserialize");
        assert_eq!(parsed, assessment);
    }
}
