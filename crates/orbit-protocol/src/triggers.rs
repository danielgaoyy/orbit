//! Wire protocol models for External Event Triggers (GitHub, Slack, Linear, Generic Webhook)
//! and GitHub PR/Issue/CI Polling Fallback.

use serde::{Deserialize, Serialize};

use crate::{ProviderKind, SessionMode};

/// Minimum allowed polling interval for GitHub PR/Issue/CI fallback (30 seconds).
pub const MIN_GITHUB_POLL_INTERVAL_SECS: u64 = 30;
/// Maximum allowed polling interval for GitHub PR/Issue/CI fallback (1 hour).
pub const MAX_GITHUB_POLL_INTERVAL_SECS: u64 = 3600;
/// Default polling interval for GitHub PR/Issue/CI fallback (2 minutes).
pub const DEFAULT_GITHUB_POLL_INTERVAL_SECS: u64 = 120;

/// Supported external event trigger source providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSourceKind {
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "slack")]
    Slack,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "generic_webhook")]
    GenericWebhook,
}

impl TriggerSourceKind {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Slack => "slack",
            Self::Linear => "linear",
            Self::GenericWebhook => "generic_webhook",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "github" => Some(Self::GitHub),
            "slack" => Some(Self::Slack),
            "linear" => Some(Self::Linear),
            "generic_webhook" | "webhook" | "generic" => Some(Self::GenericWebhook),
            _ => None,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::Slack => "Slack",
            Self::Linear => "Linear",
            Self::GenericWebhook => "Webhook",
        }
    }
}

/// Dispatch target when an external event trigger matches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDispatchTarget {
    ExistingAgent {
        agent_id: String,
    },
    SpawnWorkspaceAgent {
        cwd: String,
        provider: ProviderKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        mode: SessionMode,
    },
}

/// Optional GitHub REST API polling fallback configuration for private/NAT-bound hosts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitHubPollConfig {
    /// Repository slug in `owner/repo` format.
    pub repo: String,
    /// Polling cadence in seconds (clamped to 30..=3600).
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub watch_prs: bool,
    #[serde(default)]
    pub watch_issues: bool,
    #[serde(default)]
    pub watch_ci_failures: bool,
    /// Whether a GitHub personal access token is stored server-side (plaintext is never returned).
    #[serde(default)]
    pub has_token: bool,
}

impl GitHubPollConfig {
    /// Return a normalized copy with `poll_interval_secs` clamped into `[30, 3600]`.
    pub fn clamped(mut self) -> Self {
        self.repo = self.repo.trim().to_string();
        self.poll_interval_secs = self
            .poll_interval_secs
            .clamp(MIN_GITHUB_POLL_INTERVAL_SECS, MAX_GITHUB_POLL_INTERVAL_SECS);
        self
    }
}

/// Sanitized external event trigger record surfaced over the wire (secrets are masked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTriggerRecord {
    pub trigger_id: String,
    pub name: String,
    pub source: TriggerSourceKind,
    #[serde(default)]
    pub event_filters: Vec<String>,
    pub prompt_template: String,
    pub target: TriggerDispatchTarget,
    pub webhook_path: String,
    #[serde(default)]
    pub has_secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_poll: Option<GitHubPollConfig>,
    pub enabled: bool,
    #[serde(default)]
    pub trigger_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_triggered_at_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Recorded inbound webhook or polling delivery audit entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventTriggerDelivery {
    pub delivery_id: String,
    pub trigger_id: String,
    pub source: TriggerSourceKind,
    pub event_type: String,
    /// `"dispatched" | "filtered" | "rejected_signature" | "rate_limited" | "duplicate" | "error"`
    pub status: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_agent_id: Option<String>,
    pub received_at_ms: u64,
}
