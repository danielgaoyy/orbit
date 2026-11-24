//! Wire types for Bidirectional IM Channels (Lark / Feishu, Telegram, Slack, Discord)
//! and remote tool approval forwarding.

use serde::{Deserialize, Serialize};

use crate::{ProviderKind, RouterImMirrorPolicy, SessionMode};

/// How inbound IM messages on a channel are routed to Orbit agent sessions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImRoutingMode {
    /// Existing behavior: each IM chat/thread binds to its own dedicated coding agent conversation.
    #[default]
    PerThreadAgent,
    /// Route all authorized messages directly into the host's canonical Personal Router ("分工师").
    PersonalRouter,
}

fn im_default_true() -> bool {
    true
}

/// Supported native pure-Rust IM platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImPlatform {
    Lark,
    Telegram,
    Slack,
    Discord,
}

impl ImPlatform {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Lark => "lark",
            Self::Telegram => "telegram",
            Self::Slack => "slack",
            Self::Discord => "discord",
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "lark" | "feishu" => Some(Self::Lark),
            "telegram" | "tg" => Some(Self::Telegram),
            "slack" => Some(Self::Slack),
            "discord" => Some(Self::Discord),
            _ => None,
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Lark => "Lark / 飞书",
            Self::Telegram => "Telegram",
            Self::Slack => "Slack",
            Self::Discord => "Discord",
        }
    }

    /// Canonical webhook path suffix on the Orbit Daemon HTTP server.
    pub fn webhook_path_for(self, channel_id: &str) -> String {
        format!("/im/{}/{channel_id}", self.slug())
    }
}

/// Inbound event transport mode for an IM channel.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImTransportMode {
    /// NAT-friendly outbound connection (Feishu/Lark WS, Slack Socket Mode WSS,
    /// Discord Gateway v10 WSS, Telegram long-polling). Requires no public webhook URL.
    #[default]
    LongConnection,
    /// Inbound HTTP webhook callback exposed on the daemon HTTP listener.
    Webhook,
}

impl ImTransportMode {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::LongConnection => "long_connection",
            Self::Webhook => "webhook",
        }
    }

    pub fn from_slug(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "webhook" => Self::Webhook,
            _ => Self::LongConnection,
        }
    }
}

/// Live connection/poller state for an IM channel bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImConnectionStatus {
    #[default]
    Disconnected,
    Connecting,
    Reconnecting,
    Connected,
    Error,
}

impl ImConnectionStatus {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Connecting => "connecting",
            Self::Reconnecting => "reconnecting",
            Self::Connected => "connected",
            Self::Error => "error",
        }
    }

    pub fn from_slug(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "connecting" => Self::Connecting,
            "reconnecting" => Self::Reconnecting,
            "connected" => Self::Connected,
            "error" => Self::Error,
            _ => Self::Disconnected,
        }
    }
}

/// Sanitized, secret-masked IM channel configuration and runtime status returned over wire RPCs.
/// Plaintext tokens and signing secrets are NEVER serialized in this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImChannelRecord {
    pub channel_id: String,
    pub name: String,
    pub platform: ImPlatform,
    pub enabled: bool,
    #[serde(default)]
    pub transport_mode: ImTransportMode,
    pub status: ImConnectionStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
    pub webhook_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub has_primary_secret: bool,
    pub has_signing_secret: bool,
    #[serde(default)]
    pub has_app_token: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_primary_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_signing_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked_app_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(default)]
    pub allowed_chat_ids: Vec<String>,
    #[serde(default)]
    pub allowed_user_ids: Vec<String>,
    pub default_workspace_cwd: String,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    #[serde(default = "im_default_true")]
    pub forward_approvals: bool,
    #[serde(default)]
    pub routing_mode: ImRoutingMode,
    #[serde(default)]
    pub router_mirror_policy: RouterImMirrorPolicy,
    #[serde(default)]
    pub message_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Persistent mapping between an external IM chat/thread and an Orbit agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImThreadBinding {
    pub channel_id: String,
    pub external_chat_id: String,
    pub external_thread_id: String,
    pub agent_id: String,
    pub updated_at_ms: u64,
}

/// Mask a secret token for safe UI display without exposing sensitive prefix/full value.
/// Returns `None` for empty strings, `"••••"` for short secrets, or `"••••<last4>"` for longer ones.
pub fn mask_im_secret(secret: Option<&str>) -> Option<String> {
    let raw = secret.map(str::trim).filter(|s| !s.is_empty())?;
    let chars: Vec<char> = raw.chars().collect();
    if chars.len() <= 8 {
        Some("••••".to_string())
    } else {
        let tail: String = chars[chars.len() - 4..].iter().collect();
        Some(format!("••••{tail}"))
    }
}

/// Normalize and deduplicate a list of allowlisted chat/user IDs (preserving order).
pub fn sanitize_im_allowlist(ids: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for raw in ids {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.len() > 256 {
            continue;
        }
        if !out.iter().any(|existing: &String| existing == trimmed) {
            out.push(trimmed.to_string());
        }
        if out.len() >= 128 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_im_secret_never_leaks_plaintext() {
        assert_eq!(mask_im_secret(None), None);
        assert_eq!(mask_im_secret(Some("   ")), None);
        assert_eq!(mask_im_secret(Some("short")), Some("••••".to_string()));
        assert_eq!(mask_im_secret(Some("12345678")), Some("••••".to_string()));
        assert_eq!(
            mask_im_secret(Some("xoxb-secret-token-9876")),
            Some("••••9876".to_string())
        );
    }

    #[test]
    fn sanitize_im_allowlist_trims_and_dedups() {
        let res = sanitize_im_allowlist(vec![
            " oc_123 ".into(),
            "".into(),
            "oc_123".into(),
            "oc_456".into(),
        ]);
        assert_eq!(res, vec!["oc_123".to_string(), "oc_456".to_string()]);
    }

    #[test]
    fn im_transport_mode_and_reconnecting_status_roundtrip_and_compat() {
        assert_eq!(
            ImTransportMode::from_slug("long_connection"),
            ImTransportMode::LongConnection
        );
        assert_eq!(
            ImTransportMode::from_slug("webhook"),
            ImTransportMode::Webhook
        );
        assert_eq!(
            ImTransportMode::from_slug("unknown"),
            ImTransportMode::LongConnection
        );
        assert_eq!(
            ImConnectionStatus::from_slug("reconnecting"),
            ImConnectionStatus::Reconnecting
        );
        assert_eq!(ImConnectionStatus::Reconnecting.slug(), "reconnecting");

        // Legacy JSON missing new transport_mode/app_token/public_key fields deserializes cleanly
        let legacy_json = r#"{
            "channel_id": "im_old",
            "name": "Legacy",
            "platform": "slack",
            "enabled": true,
            "status": "connected",
            "webhook_path": "/im/slack/im_old",
            "has_primary_secret": true,
            "has_signing_secret": true,
            "default_workspace_cwd": "/tmp",
            "default_provider": "claude_code",
            "default_mode": "default",
            "created_at_ms": 1,
            "updated_at_ms": 2
        }"#;
        let decoded: ImChannelRecord =
            serde_json::from_str(legacy_json).expect("decode legacy ImChannelRecord");
        assert_eq!(decoded.transport_mode, ImTransportMode::LongConnection);
        assert!(!decoded.has_app_token);
        assert_eq!(decoded.masked_app_token, None);
        assert_eq!(decoded.public_key, None);
    }
}
