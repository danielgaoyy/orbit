//! Early-wakeup one-shot conditional timer domain models (`DurationSeconds` + `TimerCondition`).

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Minimum allowed countdown duration in seconds (1 second).
pub const MIN_CONDITIONAL_TIMER_SECONDS: u64 = 1;
/// Maximum allowed countdown duration in seconds (24 hours).
pub const MAX_CONDITIONAL_TIMER_SECONDS: u64 = 86_400;
/// Maximum allowed wakeup prompt UTF-8 byte length (8 KiB).
pub const MAX_CONDITIONAL_TIMER_PROMPT_BYTES: usize = 8 * 1024;
/// Maximum number of concurrently armed conditional timers per agent.
pub const MAX_ARMED_TIMERS_PER_AGENT: usize = 16;
/// Maximum number of concurrently armed conditional timers across the entire host daemon.
pub const MAX_ARMED_TIMERS_GLOBAL: usize = 256;

/// Error returned when parsing a shorthand [`TimerCondition`] string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseTimerConditionError {
    #[error("timer condition `sender:<id>` requires a non-empty sender_id")]
    EmptySenderId,
    #[error("timer condition `file:<path>` requires a non-empty path")]
    EmptyFilePath,
    #[error(
        "invalid timer condition `{0}`; expected `never`, `any`, `all_subagents`, `all_peers`, `sender:<id>`, or `file:<path>`"
    )]
    InvalidCondition(String),
}

/// Condition under which an armed one-shot timer is canceled or woken early.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimerCondition {
    /// Never cancel early; always fire after `duration_seconds` unless manually canceled.
    #[default]
    Never,
    /// Cancel early when any peer, child, finish-notification, or user message arrives for the agent.
    AnyMessage,
    /// Cancel early when a message arrives from a specific `sender_id` (e.g. an agent ID).
    Sender { sender_id: String },
    /// Cancel early once all active managed and provider-native subagents of the agent settle.
    AllSubagentsSettled,
    /// Cancel early once all sibling peers in the agent's Mission Workspace settle.
    AllPeersSettled,
    /// Watch a file inside the agent's workspace; wake immediately / resolve early on change.
    FileChanged { path: String },
}

impl FromStr for TimerCondition {
    type Err = ParseTimerConditionError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("never") {
            return Ok(Self::Never);
        }
        if trimmed.eq_ignore_ascii_case("any") || trimmed.eq_ignore_ascii_case("any_message") {
            return Ok(Self::AnyMessage);
        }
        if trimmed.eq_ignore_ascii_case("all_subagents")
            || trimmed.eq_ignore_ascii_case("all_subagents_settled")
        {
            return Ok(Self::AllSubagentsSettled);
        }
        if trimmed.eq_ignore_ascii_case("all_peers")
            || trimmed.eq_ignore_ascii_case("all_peers_settled")
        {
            return Ok(Self::AllPeersSettled);
        }
        if let Some((prefix, value)) = trimmed.split_once(':') {
            let val = value.trim();
            if prefix.eq_ignore_ascii_case("sender") {
                if val.is_empty() {
                    return Err(ParseTimerConditionError::EmptySenderId);
                }
                return Ok(Self::Sender {
                    sender_id: val.to_string(),
                });
            }
            if prefix.eq_ignore_ascii_case("file") {
                if val.is_empty() {
                    return Err(ParseTimerConditionError::EmptyFilePath);
                }
                return Ok(Self::FileChanged {
                    path: val.to_string(),
                });
            }
        }
        Err(ParseTimerConditionError::InvalidCondition(
            trimmed.to_string(),
        ))
    }
}

impl TimerCondition {
    /// Parse a compact string shorthand used by MCP tools and CLI flags:
    /// `"never"` | `"any"` | `"all_subagents"` | `"all_peers"` | `"sender:<id>"` | `"file:<path>"`.
    pub fn parse_shorthand(raw: &str) -> Result<Self, String> {
        raw.parse::<Self>().map_err(|e| e.to_string())
    }

    /// Format into the canonical shorthand accepted by [`Self::parse_shorthand`].
    pub fn to_shorthand(&self) -> String {
        match self {
            Self::Never => "never".to_string(),
            Self::AnyMessage => "any".to_string(),
            Self::Sender { sender_id } => format!("sender:{sender_id}"),
            Self::AllSubagentsSettled => "all_subagents".to_string(),
            Self::AllPeersSettled => "all_peers".to_string(),
            Self::FileChanged { path } => format!("file:{path}"),
        }
    }

    /// Concise human-readable description suitable for compact UI pills.
    pub fn short_label(&self) -> String {
        match self {
            Self::Never => "fixed timer".to_string(),
            Self::AnyMessage => "until any message".to_string(),
            Self::Sender { sender_id } => format!("until {sender_id}"),
            Self::AllSubagentsSettled => "until subagents settle".to_string(),
            Self::AllPeersSettled => "until workspace peers settle".to_string(),
            Self::FileChanged { path } => {
                let leaf = path.rsplit(['/', '\\']).next().unwrap_or(path);
                format!("until {leaf} changes")
            }
        }
    }
}

/// Terminal or armed lifecycle state of a one-shot conditional timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionalTimerState {
    Armed,
    Fired,
    CanceledEarly,
    CanceledManual,
}

impl ConditionalTimerState {
    pub const fn is_armed(self) -> bool {
        matches!(self, Self::Armed)
    }

    pub const fn is_resolved(self) -> bool {
        !self.is_armed()
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::Armed => "armed",
            Self::Fired => "fired",
            Self::CanceledEarly => "canceled_early",
            Self::CanceledManual => "canceled_manual",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "armed" => Some(Self::Armed),
            "fired" => Some(Self::Fired),
            "canceled_early" => Some(Self::CanceledEarly),
            "canceled_manual" => Some(Self::CanceledManual),
            _ => None,
        }
    }
}

/// Persisted and wire-synced record for a one-shot conditional timer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionalTimerRecord {
    pub timer_id: String,
    pub agent_id: String,
    pub workspace_id: String,
    pub prompt: String,
    pub duration_seconds: u64,
    pub condition: TimerCondition,
    pub state: ConditionalTimerState,
    pub created_at_ms: u64,
    pub fire_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_condition_shorthand_roundtrip() {
        let cases = [
            ("never", TimerCondition::Never),
            ("any", TimerCondition::AnyMessage),
            ("all_subagents", TimerCondition::AllSubagentsSettled),
            ("all_peers", TimerCondition::AllPeersSettled),
            (
                "sender:ag_child_1",
                TimerCondition::Sender {
                    sender_id: "ag_child_1".into(),
                },
            ),
            (
                "file:src/main.rs",
                TimerCondition::FileChanged {
                    path: "src/main.rs".into(),
                },
            ),
        ];
        for (raw, expected) in cases {
            let parsed_from_str: TimerCondition = raw.parse().expect("FromStr parse");
            assert_eq!(parsed_from_str, expected);
            let parsed = TimerCondition::parse_shorthand(raw).expect("parse shorthand");
            assert_eq!(parsed, expected);
            assert_eq!(parsed.to_shorthand(), raw);
        }

        assert_eq!(
            "sender:".parse::<TimerCondition>(),
            Err(ParseTimerConditionError::EmptySenderId)
        );
        assert_eq!(
            "file:   ".parse::<TimerCondition>(),
            Err(ParseTimerConditionError::EmptyFilePath)
        );
        assert_eq!(
            "bogus".parse::<TimerCondition>(),
            Err(ParseTimerConditionError::InvalidCondition("bogus".into()))
        );
    }

    #[test]
    fn conditional_timer_state_slug_roundtrip() {
        for state in [
            ConditionalTimerState::Armed,
            ConditionalTimerState::Fired,
            ConditionalTimerState::CanceledEarly,
            ConditionalTimerState::CanceledManual,
        ] {
            assert_eq!(ConditionalTimerState::from_slug(state.slug()), Some(state));
        }
        assert!(ConditionalTimerState::Armed.is_armed());
        assert!(ConditionalTimerState::Fired.is_resolved());
        assert!(ConditionalTimerState::CanceledEarly.is_resolved());
        assert!(ConditionalTimerState::CanceledManual.is_resolved());
    }
}
