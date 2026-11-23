//! Deterministic lifecycle hooks wire types (`$ORBIT_HOME/hooks.json` & `<cwd>/.orbit/hooks.json`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default per-hook execution timeout (10 seconds).
pub const DEFAULT_HOOK_TIMEOUT_MS: u64 = 10_000;
/// Hard upper bound on per-hook execution timeout (30 seconds).
pub const MAX_HOOK_TIMEOUT_MS: u64 = 30_000;
/// Maximum number of recent hook executions retained in daemon memory.
pub const MAX_HOOK_EXECUTIONS: usize = 100;
/// Maximum stdout / stderr / HTTP response body captured per hook run (64 KiB).
pub const MAX_HOOK_OUTPUT_BYTES: usize = 64 * 1024;

fn default_hook_timeout_ms() -> u64 {
    DEFAULT_HOOK_TIMEOUT_MS
}

fn default_hook_enabled_true() -> bool {
    true
}

/// Lifecycle event point at which a deterministic hook executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookEventKind {
    SessionStart,
    PrePromptSubmit,
    PreToolUse,
    PostToolUse,
    PostFileEdit,
    Notification,
    TurnStop,
    SessionEnd,
}

impl HookEventKind {
    const ALL: [Self; 8] = [
        Self::SessionStart,
        Self::PrePromptSubmit,
        Self::PreToolUse,
        Self::PostToolUse,
        Self::PostFileEdit,
        Self::Notification,
        Self::TurnStop,
        Self::SessionEnd,
    ];

    pub const fn all() -> &'static [Self] {
        &Self::ALL
    }

    pub const fn slug(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::PrePromptSubmit => "pre_prompt_submit",
            Self::PreToolUse => "pre_tool_use",
            Self::PostToolUse => "post_tool_use",
            Self::PostFileEdit => "post_file_edit",
            Self::Notification => "notification",
            Self::TurnStop => "turn_stop",
            Self::SessionEnd => "session_end",
        }
    }

    pub const fn display_label(self) -> &'static str {
        match self {
            Self::SessionStart => "Session Start",
            Self::PrePromptSubmit => "Pre-Prompt Submit",
            Self::PreToolUse => "Pre-Tool Use",
            Self::PostToolUse => "Post-Tool Use",
            Self::PostFileEdit => "Post-File Edit",
            Self::Notification => "Notification",
            Self::TurnStop => "Turn Stop",
            Self::SessionEnd => "Session End",
        }
    }
}

/// Storage and trust scope for a lifecycle hook rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookScope {
    UserGlobal,
    ProjectOrbit,
}

/// Action executed when a lifecycle hook rule matches an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookAction {
    Command {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
    },
    HttpWebhook {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// Decision returned by a lifecycle hook execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookDecision {
    Allow,
    Block,
    Modify,
}

/// Persisted lifecycle hook rule definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifecycleHookRule {
    pub id: String,
    pub name: String,
    pub scope: HookScope,
    pub event: HookEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matcher: Option<String>,
    pub action: HookAction,
    #[serde(default = "default_hook_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_hook_enabled_true")]
    pub enabled: bool,
    #[serde(default)]
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Audit log record for a single hook execution (live or dry-run test).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookExecutionRecord {
    pub id: String,
    pub hook_id: String,
    pub hook_name: String,
    pub event: HookEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_target: Option<String>,
    pub decision: HookDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    #[serde(default)]
    pub stdout_preview: String,
    #[serde(default)]
    pub stderr_preview: String,
    pub executed_at_ms: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::WireMessage;

    #[test]
    fn lifecycle_hooks_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let cmd_rule = LifecycleHookRule {
            id: "hk_rustfmt".into(),
            name: "Format Rust on save".into(),
            scope: HookScope::ProjectOrbit,
            event: HookEventKind::PostFileEdit,
            matcher: Some("*.rs".into()),
            action: HookAction::Command {
                command: "rustfmt".into(),
                args: vec!["--edition".into(), "2024".into()],
                env: BTreeMap::from([("RUST_LOG".into(), "warn".into())]),
            },
            timeout_ms: 5_000,
            enabled: true,
            trusted: true,
            description: Some("Run rustfmt on edited Rust files".into()),
        };
        let webhook_rule = LifecycleHookRule {
            id: "hk_notify".into(),
            name: "Notify Slack".into(),
            scope: HookScope::UserGlobal,
            event: HookEventKind::TurnStop,
            matcher: None,
            action: HookAction::HttpWebhook {
                url: "https://hooks.example.com/orbit".into(),
                headers: BTreeMap::from([("Authorization".into(), "••••1234".into())]),
            },
            timeout_ms: DEFAULT_HOOK_TIMEOUT_MS,
            enabled: true,
            trusted: true,
            description: None,
        };
        let record = HookExecutionRecord {
            id: "exec_1".into(),
            hook_id: "hk_rustfmt".into(),
            hook_name: "Format Rust on save".into(),
            event: HookEventKind::PostFileEdit,
            matched_target: Some("src/lib.rs".into()),
            decision: HookDecision::Allow,
            reason: None,
            exit_code: Some(0),
            duration_ms: 14,
            stdout_preview: String::new(),
            stderr_preview: String::new(),
            executed_at_ms: 1_760_000_000_000,
        };

        let messages = vec![
            WireMessage::ListLifecycleHooks {
                request_id: req_id,
                cwd: Some("/repo".into()),
            },
            WireMessage::LifecycleHooksSnapshot {
                request_id: req_id,
                hooks: vec![cmd_rule.clone(), webhook_rule],
                recent_executions: vec![record.clone()],
                project_hooks_trusted: true,
            },
            WireMessage::UpsertLifecycleHook {
                request_id: req_id,
                cwd: Some("/repo".into()),
                hook: cmd_rule,
            },
            WireMessage::DeleteLifecycleHook {
                request_id: req_id,
                cwd: Some("/repo".into()),
                hook_id: "hk_rustfmt".into(),
                scope: HookScope::ProjectOrbit,
            },
            WireMessage::SetProjectHooksTrust {
                request_id: req_id,
                cwd: "/repo".into(),
                trusted: true,
            },
            WireMessage::TestLifecycleHook {
                request_id: req_id,
                cwd: Some("/repo".into()),
                hook_id: "hk_rustfmt".into(),
                sample_payload: json!({ "path": "src/lib.rs" }),
            },
            WireMessage::HookTestResultResponse {
                request_id: req_id,
                record,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode lifecycle hooks wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode lifecycle hooks msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig json"),
                serde_json::to_value(&decoded).expect("decoded json")
            );
        }
    }
}
