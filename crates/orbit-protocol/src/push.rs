//! Wire types for Enhanced Desktop Notifications & Native Mobile Background Push
//! (APNs / FCM / HarmonyOS Push Kit).
//!
//! Privacy boundary: by default (`include_message_preview: false`), mobile push payloads
//! contain only minimal metadata (`event_kind`, `host_id`, `agent_id`, generic title/body,
//! and deep link) so plaintext prompts and source code never traverse third-party push relays.

use serde::{Deserialize, Serialize};

fn push_default_true() -> bool {
    true
}

fn push_default_quiet_start() -> u8 {
    23
}

fn push_default_quiet_end() -> u8 {
    7
}

/// Supported native mobile background push delivery platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushPlatform {
    Apns,
    Fcm,
    HarmonyPushKit,
}

impl PushPlatform {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Apns => "apns",
            Self::Fcm => "fcm",
            Self::HarmonyPushKit => "harmony_push_kit",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "apns" | "ios" | "apple" => Some(Self::Apns),
            "fcm" | "android" | "firebase" => Some(Self::Fcm),
            "harmony_push_kit" | "harmony" | "harmonyos" | "hms" | "ohos" => {
                Some(Self::HarmonyPushKit)
            }
            _ => None,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Apns => "Apple APNs",
            Self::Fcm => "Google FCM",
            Self::HarmonyPushKit => "HarmonyOS Push Kit",
        }
    }
}

/// Categories of Orbit daemon events eligible for native background push delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushEventKind {
    PermissionRequired,
    TurnCompleted,
    AgentError,
    GoalFinished,
    TimerFired,
}

impl PushEventKind {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::PermissionRequired => "permission_required",
            Self::TurnCompleted => "turn_completed",
            Self::AgentError => "agent_error",
            Self::GoalFinished => "goal_finished",
            Self::TimerFired => "timer_fired",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "permission_required" => Some(Self::PermissionRequired),
            "turn_completed" => Some(Self::TurnCompleted),
            "agent_error" => Some(Self::AgentError),
            "goal_finished" => Some(Self::GoalFinished),
            "timer_fired" => Some(Self::TimerFired),
            _ => None,
        }
    }

    /// Minimal zero-knowledge title prefix (never contains prompt or code content).
    pub const fn minimal_title_prefix(self) -> &'static str {
        match self {
            Self::PermissionRequired => "Approval required",
            Self::TurnCompleted => "Turn completed",
            Self::AgentError => "Agent error",
            Self::GoalFinished => "Goal finished",
            Self::TimerFired => "Timer fired",
        }
    }

    /// Minimal zero-knowledge body text used when `include_message_preview == false`.
    pub const fn minimal_body(self) -> &'static str {
        match self {
            Self::PermissionRequired => "An agent is waiting for your approval.",
            Self::TurnCompleted => "An agent finished its background turn.",
            Self::AgentError => "An agent stopped with an error.",
            Self::GoalFinished => "An autonomous goal reached a terminal state.",
            Self::TimerFired => "A scheduled timer triggered.",
        }
    }
}

/// Host-scoped mobile push notification preferences.
///
/// `include_message_preview` defaults to `false` to preserve the zero-knowledge privacy boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushPreferences {
    #[serde(default = "push_default_true")]
    pub enabled: bool,
    #[serde(default = "push_default_true")]
    pub notify_permission_required: bool,
    #[serde(default = "push_default_true")]
    pub notify_turn_completed: bool,
    #[serde(default = "push_default_true")]
    pub notify_agent_error: bool,
    #[serde(default = "push_default_true")]
    pub notify_goal_finished: bool,
    #[serde(default = "push_default_true")]
    pub notify_timer_fired: bool,
    #[serde(default)]
    pub include_message_preview: bool,
    #[serde(default)]
    pub quiet_hours_enabled: bool,
    #[serde(default = "push_default_quiet_start")]
    pub quiet_start_hour: u8,
    #[serde(default = "push_default_quiet_end")]
    pub quiet_end_hour: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub push_gateway_url: Option<String>,
    #[serde(default)]
    pub has_gateway_auth_token: bool,
}

impl Default for PushPreferences {
    fn default() -> Self {
        Self {
            enabled: true,
            notify_permission_required: true,
            notify_turn_completed: true,
            notify_agent_error: true,
            notify_goal_finished: true,
            notify_timer_fired: true,
            include_message_preview: false,
            quiet_hours_enabled: false,
            quiet_start_hour: push_default_quiet_start(),
            quiet_end_hour: push_default_quiet_end(),
            push_gateway_url: None,
            has_gateway_auth_token: false,
        }
    }
}

impl PushPreferences {
    /// Returns whether a given event kind is enabled in these preferences.
    pub const fn allows_event(&self, kind: PushEventKind) -> bool {
        if !self.enabled {
            return false;
        }
        match kind {
            PushEventKind::PermissionRequired => self.notify_permission_required,
            PushEventKind::TurnCompleted => self.notify_turn_completed,
            PushEventKind::AgentError => self.notify_agent_error,
            PushEventKind::GoalFinished => self.notify_goal_finished,
            PushEventKind::TimerFired => self.notify_timer_fired,
        }
    }
}

/// Sanitized mobile push device record returned over wire RPCs.
/// Raw device tokens are NEVER serialized in this record (`masked_token` holds `••••<last6>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushDeviceRecord {
    pub device_id: String,
    pub platform: PushPlatform,
    pub device_name: String,
    pub masked_token: String,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default = "push_default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_delivered_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub registered_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Structured push notification payload dispatched to APNs / FCM / HarmonyOS Push Kit gateways.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushDispatchPayload {
    pub event_kind: PushEventKind,
    pub host_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub title: String,
    pub body: String,
    pub deep_link: String,
}

/// Mask a mobile push device token for safe snapshot display (`••••<last6>`).
/// Returns `"••••"` for short tokens (<= 8 chars) and `""` for empty strings.
pub fn mask_push_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        "••••".to_string()
    } else {
        let tail: String = chars[chars.len() - 6..].iter().collect();
        format!("••••{tail}")
    }
}

/// Percent-encode a single query parameter value for `orbit://open?...` deep links.
fn encode_query_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            _ => {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// Build a canonical multi-host Orbit deep link (`orbit://open?host=...&agent=...`).
pub fn build_push_deep_link(host_id: &str, agent_id: Option<&str>) -> String {
    let host_enc = encode_query_component(host_id.trim());
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(aid) => format!(
            "orbit://open?host={host_enc}&agent={}",
            encode_query_component(aid)
        ),
        None => format!("orbit://open?host={host_enc}"),
    }
}

/// Check whether `hour` (0..24) falls inside `[start_hour, end_hour)` on a 24-hour clock.
/// Supports both daytime ranges (`9..17`) and overnight wrap-around ranges (`23..7`).
/// When `start_hour == end_hour`, the quiet window is empty (`false`).
pub const fn is_in_quiet_hours(hour: u8, start_hour: u8, end_hour: u8) -> bool {
    let h = hour % 24;
    let s = start_hour % 24;
    let e = end_hour % 24;
    if s == e {
        false
    } else if s < e {
        h >= s && h < e
    } else {
        h >= s || h < e
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_push_token_masks_to_last_6_chars() {
        assert_eq!(mask_push_token(""), "");
        assert_eq!(mask_push_token("   "), "");
        assert_eq!(mask_push_token("12345678"), "••••");
        assert_eq!(mask_push_token("apns_token_abcdef123456"), "••••123456");
    }

    #[test]
    fn quiet_hours_handles_normal_and_wraparound_windows() {
        // Overnight wrap-around: 23:00 -> 07:00
        assert!(is_in_quiet_hours(23, 23, 7));
        assert!(is_in_quiet_hours(0, 23, 7));
        assert!(is_in_quiet_hours(6, 23, 7));
        assert!(!is_in_quiet_hours(7, 23, 7));
        assert!(!is_in_quiet_hours(12, 23, 7));
        assert!(!is_in_quiet_hours(22, 23, 7));

        // Daytime range: 09:00 -> 17:00
        assert!(!is_in_quiet_hours(8, 9, 17));
        assert!(is_in_quiet_hours(9, 9, 17));
        assert!(is_in_quiet_hours(16, 9, 17));
        assert!(!is_in_quiet_hours(17, 9, 17));
        assert!(!is_in_quiet_hours(23, 9, 17));

        // Empty range when start == end
        assert!(!is_in_quiet_hours(10, 10, 10));
    }

    #[test]
    fn deep_link_encodes_host_and_optional_agent() {
        assert_eq!(
            build_push_deep_link("host_devbox", Some("ag_42")),
            "orbit://open?host=host_devbox&agent=ag_42"
        );
        assert_eq!(
            build_push_deep_link("host with space", None),
            "orbit://open?host=host%20with%20space"
        );
    }

    #[test]
    fn push_preferences_default_is_zero_knowledge() {
        let prefs = PushPreferences::default();
        assert!(prefs.enabled);
        assert!(!prefs.include_message_preview);
        assert!(!prefs.quiet_hours_enabled);
        assert_eq!(prefs.quiet_start_hour, 23);
        assert_eq!(prefs.quiet_end_hour, 7);
        assert!(prefs.allows_event(PushEventKind::PermissionRequired));
        assert!(prefs.allows_event(PushEventKind::GoalFinished));
    }

    #[test]
    fn push_platform_and_event_slugs_roundtrip() {
        for p in [
            PushPlatform::Apns,
            PushPlatform::Fcm,
            PushPlatform::HarmonyPushKit,
        ] {
            assert_eq!(PushPlatform::from_slug(p.slug()), Some(p));
        }
        for e in [
            PushEventKind::PermissionRequired,
            PushEventKind::TurnCompleted,
            PushEventKind::AgentError,
            PushEventKind::GoalFinished,
            PushEventKind::TimerFired,
        ] {
            assert_eq!(PushEventKind::from_slug(e.slug()), Some(e));
        }
    }
}
