//! Path-scoped glob rules types (`.orbit/rules/*.md`, `.claude/rules/*.md`, `.cursor/rules/*.mdc`).

use serde::{Deserialize, Serialize};

/// Warning threshold (in approximate tokens) for always-applied rules.
pub const PATH_RULES_WARN_TOKENS: u32 = 8_000;
/// Hard token cap for injected rules per prompt turn.
pub const PATH_RULES_HARD_CAP_TOKENS: u32 = 24_000;

fn default_rule_enabled_true() -> bool {
    true
}

/// How a path-scoped rule is triggered for prompt injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleTriggerMode {
    Always,
    GlobAutoAttach,
    AgentDecided,
    Manual,
}

impl RuleTriggerMode {
    pub const fn as_frontmatter_str(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::GlobAutoAttach => "glob",
            Self::AgentDecided => "agent",
            Self::Manual => "manual",
        }
    }

    pub fn from_frontmatter_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "always" | "always_apply" => Some(Self::Always),
            "glob" | "glob_auto_attach" | "auto" | "auto_attach" => Some(Self::GlobAutoAttach),
            "agent" | "agent_decided" | "agent_requested" => Some(Self::AgentDecided),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Discovery and storage scope for a path-scoped rule file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    UserGlobal,
    ProjectOrbit,
    ProjectClaude,
    ProjectCursor,
}

impl RuleScope {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::UserGlobal => "user_global",
            Self::ProjectOrbit => "project_orbit",
            Self::ProjectClaude => "project_claude",
            Self::ProjectCursor => "project_cursor",
        }
    }

    pub const fn file_extension(self) -> &'static str {
        match self {
            Self::ProjectCursor => "mdc",
            Self::UserGlobal | Self::ProjectOrbit | Self::ProjectClaude => "md",
        }
    }

    pub const fn sort_rank(self) -> u8 {
        match self {
            Self::ProjectOrbit => 0,
            Self::ProjectClaude => 1,
            Self::ProjectCursor => 2,
            Self::UserGlobal => 3,
        }
    }
}

/// A discovered path-scoped rule document with parsed frontmatter and body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathScopedRule {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub scope: RuleScope,
    pub path: String,
    pub relative_path: String,
    pub trigger_mode: RuleTriggerMode,
    #[serde(default)]
    pub globs: Vec<String>,
    #[serde(default = "default_rule_enabled_true")]
    pub enabled: bool,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub approx_tokens: u32,
    #[serde(default)]
    pub etag: String,
}

/// Compact summary of a rule matched for a prompt turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedRuleSummary {
    pub id: String,
    pub name: String,
    pub trigger_mode: RuleTriggerMode,
    #[serde(default)]
    pub matched_globs: Vec<String>,
    #[serde(default)]
    pub approx_tokens: u32,
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::WireMessage;

    #[test]
    fn path_rules_wire_messages_roundtrip() {
        let req_id = Uuid::new_v4();
        let rule = PathScopedRule {
            id: "project_orbit:rust-style".into(),
            name: "rust-style".into(),
            description: "Enforce zero-unwrap Rust error handling".into(),
            scope: RuleScope::ProjectOrbit,
            path: "/repo/.orbit/rules/rust-style.md".into(),
            relative_path: ".orbit/rules/rust-style.md".into(),
            trigger_mode: RuleTriggerMode::GlobAutoAttach,
            globs: vec!["**/*.rs".into(), "!**/tests/**".into()],
            enabled: true,
            body: "Never use `.unwrap()` in production code.".into(),
            approx_tokens: 12,
            etag: "a".repeat(64),
        };
        let summary = MatchedRuleSummary {
            id: rule.id.clone(),
            name: rule.name.clone(),
            trigger_mode: RuleTriggerMode::GlobAutoAttach,
            matched_globs: vec!["**/*.rs".into()],
            approx_tokens: 12,
        };

        let messages = vec![
            WireMessage::ListPathRules {
                request_id: req_id,
                cwd: "/repo".into(),
            },
            WireMessage::PathRulesSnapshot {
                request_id: req_id,
                cwd: "/repo".into(),
                rules: vec![rule.clone()],
                total_always_tokens: 120,
                token_budget_warning: false,
            },
            WireMessage::UpsertPathRule {
                request_id: req_id,
                cwd: "/repo".into(),
                scope: RuleScope::ProjectCursor,
                name: "desktop-ui".into(),
                description: "Desktop UI guidelines".into(),
                trigger_mode: RuleTriggerMode::GlobAutoAttach,
                globs: vec!["apps/desktop/src/**/*.{ts,tsx}".into()],
                enabled: true,
                body: "Keep UI minimal and restrained.".into(),
                expected_etag: Some("deadbeef".into()),
            },
            WireMessage::DeletePathRule {
                request_id: req_id,
                cwd: "/repo".into(),
                path: "/repo/.cursor/rules/desktop-ui.mdc".into(),
            },
            WireMessage::MatchPathRules {
                request_id: req_id,
                cwd: "/repo".into(),
                active_paths: vec!["crates/orbit-daemon/src/lib.rs".into()],
                manual_rule_names: vec!["release-checklist".into()],
            },
            WireMessage::MatchedPathRulesResponse {
                request_id: req_id,
                cwd: "/repo".into(),
                matched: vec![summary],
                injected_markdown: "<WORKSPACE_RULES>\n...\n</WORKSPACE_RULES>".into(),
                total_tokens: 12,
            },
        ];

        for msg in messages {
            let encoded = msg.encode_zstd(3).expect("encode path rules wire msg");
            let decoded = WireMessage::decode_zstd(&encoded).expect("decode path rules wire msg");
            assert_eq!(
                serde_json::to_value(&msg).expect("orig json"),
                serde_json::to_value(&decoded).expect("decoded json")
            );
        }
    }
}
