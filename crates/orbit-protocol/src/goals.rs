//! Wire types for Unattended Long-Running `/goal` Mode & `Monitor` Streaming Background Engine.

use serde::{Deserialize, Serialize};

fn default_expect_status() -> u16 {
    200
}

fn default_http_interval_secs() -> u64 {
    30
}

/// Lifecycle state of an unattended autonomous Goal run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Running,
    Verifying,
    Paused,
    Succeeded,
    Failed,
    Canceled,
}

impl GoalStatus {
    /// Canonical snake_case representation for persistence.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Paused => "paused",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }

    /// Parse from persisted snake_case string.
    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "running" => Some(Self::Running),
            "verifying" => Some(Self::Verifying),
            "paused" => Some(Self::Paused),
            "succeeded" => Some(Self::Succeeded),
            "failed" => Some(Self::Failed),
            "canceled" | "cancelled" => Some(Self::Canceled),
            _ => None,
        }
    }

    /// Returns `true` when this goal is actively executing or verifying (holds Keep-Awake).
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Verifying)
    }

    /// Returns `true` when this goal has reached a terminal state.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

/// Control action sent by a client or tool to manage an active Goal run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalControlAction {
    Pause,
    Resume,
    Cancel,
    VerifyNow,
}

/// Individual acceptance criterion tracked on a Goal run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalCriterion {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub passed: bool,
}

/// Persisted & wire-synced state of an unattended autonomous Goal run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalRunRecord {
    pub goal_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub objective: String,
    #[serde(default)]
    pub criteria: Vec<GoalCriterion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_command: Option<String>,
    pub max_iterations: u32,
    #[serde(default)]
    pub current_iteration: u32,
    pub max_duration_secs: u64,
    pub status: GoalStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verify_exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_verify_output: Option<String>,
    #[serde(default)]
    pub consecutive_identical_failures: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
    #[serde(default)]
    pub paused_by_user: bool,
    #[serde(default)]
    pub active_duration_secs: u64,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
}

/// Source specification for a streaming background Monitor watch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonitorKind {
    CommandStream {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
    },
    LogFileTail {
        path: String,
        pattern: String,
    },
    HttpHealth {
        url: String,
        #[serde(default = "default_expect_status")]
        expect_status: u16,
        #[serde(default = "default_http_interval_secs")]
        interval_secs: u64,
    },
}

/// Lifecycle state of a streaming Monitor watch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorStatus {
    Active,
    Paused,
    Triggered,
    Stopped,
    Error,
}

impl MonitorStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Triggered => "triggered",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }

    pub fn from_str_opt(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "triggered" => Some(Self::Triggered),
            "stopped" => Some(Self::Stopped),
            "error" => Some(Self::Error),
            _ => None,
        }
    }

    pub const fn is_running(self) -> bool {
        matches!(self, Self::Active | Self::Triggered)
    }
}

/// Persisted & wire-synced state of a streaming background Monitor watch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorWatchRecord {
    pub monitor_id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: MonitorKind,
    pub status: MonitorStatus,
    #[serde(default)]
    pub match_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_triggered_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    pub created_at_ms: u64,
}
