//! Fine-grained persistent permission rules engine wire types and safe batch approval DTOs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ProviderKind;

/// Maximum length of a permission rule pattern string.
pub const PERMISSION_RULE_MAX_PATTERN_LEN: usize = 512;

/// Maximum length of an optional rule note.
pub const PERMISSION_RULE_MAX_NOTE_LEN: usize = 256;

/// Scope at which a persistent permission rule applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleScopeKind {
    Host,
    #[default]
    Project,
}

impl PermissionRuleScopeKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Project => "project",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "host" => Some(Self::Host),
            "project" => Some(Self::Project),
            _ => None,
        }
    }
}

/// High-level tool category matched by a permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleToolKind {
    Shell,
    FileWrite,
    FileRead,
    McpTool,
    Network,
    #[default]
    Any,
}

impl PermissionRuleToolKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::FileWrite => "file_write",
            Self::FileRead => "file_read",
            Self::McpTool => "mcp_tool",
            Self::Network => "network",
            Self::Any => "any",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "shell" => Some(Self::Shell),
            "file_write" => Some(Self::FileWrite),
            "file_read" => Some(Self::FileRead),
            "mcp_tool" => Some(Self::McpTool),
            "network" => Some(Self::Network),
            "any" => Some(Self::Any),
            _ => None,
        }
    }
}

/// Pattern matching strategy used by a permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRulePatternKind {
    #[default]
    CommandPrefix,
    ExactCommand,
    PathGlob,
    ToolName,
}

impl PermissionRulePatternKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommandPrefix => "command_prefix",
            Self::ExactCommand => "exact_command",
            Self::PathGlob => "path_glob",
            Self::ToolName => "tool_name",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "command_prefix" => Some(Self::CommandPrefix),
            "exact_command" => Some(Self::ExactCommand),
            "path_glob" => Some(Self::PathGlob),
            "tool_name" => Some(Self::ToolName),
            _ => None,
        }
    }
}

/// Enforcement decision for a matching permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleDecision {
    #[default]
    Allow,
    Deny,
}

impl PermissionRuleDecision {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "allow" => Some(Self::Allow),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// Provenance of a persisted permission rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionRuleSource {
    #[default]
    Manual,
    AlwaysAllowGrant,
}

impl PermissionRuleSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::AlwaysAllowGrant => "always_allow_grant",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "manual" => Some(Self::Manual),
            "always_allow_grant" => Some(Self::AlwaysAllowGrant),
            _ => None,
        }
    }
}

/// Persisted fine-grained permission rule evaluated deterministically across all providers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRuleRecord {
    pub id: String,
    pub scope_kind: PermissionRuleScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_cwd: Option<String>,
    /// `None` applies across all providers (Claude, Codex, Antigravity, Pi, ACP, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    pub tool_kind: PermissionRuleToolKind,
    pub pattern_kind: PermissionRulePatternKind,
    pub pattern: String,
    pub decision: PermissionRuleDecision,
    #[serde(default)]
    pub source: PermissionRuleSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_matched_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub match_count: u64,
}

/// Input payload used to create or update a persistent permission rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpsertPermissionRuleInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub scope_kind: PermissionRuleScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<ProviderKind>,
    pub tool_kind: PermissionRuleToolKind,
    pub pattern_kind: PermissionRulePatternKind,
    pub pattern: String,
    pub decision: PermissionRuleDecision,
    #[serde(default)]
    pub source: PermissionRuleSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl UpsertPermissionRuleInput {
    /// Validate and normalize static fields (trimming pattern/note and enforcing bounds).
    pub fn validate_basic(&self) -> Result<(), String> {
        let pattern = self.pattern.trim();
        if pattern.is_empty() {
            return Err("permission rule pattern cannot be empty".into());
        }
        if pattern.chars().count() > PERMISSION_RULE_MAX_PATTERN_LEN {
            return Err(format!(
                "permission rule pattern exceeds maximum length ({PERMISSION_RULE_MAX_PATTERN_LEN} chars)"
            ));
        }
        if pattern.contains('\0') {
            return Err("permission rule pattern cannot contain NUL bytes".into());
        }
        if matches!(self.scope_kind, PermissionRuleScopeKind::Project) {
            let cwd = self.project_cwd.as_deref().map(str::trim).unwrap_or("");
            if cwd.is_empty() {
                return Err("project-scoped permission rule requires a project_cwd".into());
            }
            if cwd.contains('\0') {
                return Err("project_cwd cannot contain NUL bytes".into());
            }
        }
        if matches!(self.decision, PermissionRuleDecision::Allow) {
            // Disallow trivial universal wildcards on Allow rules so users cannot accidentally
            // bypass every confirmation gate.
            if matches!(self.pattern_kind, PermissionRulePatternKind::CommandPrefix)
                && matches!(pattern, "*" | "**")
            {
                return Err("allow command_prefix rule cannot be a bare wildcard".into());
            }
            if matches!(self.pattern_kind, PermissionRulePatternKind::ToolName)
                && matches!(self.tool_kind, PermissionRuleToolKind::Any)
                && matches!(pattern, "*" | "**")
            {
                return Err("allow tool_name rule for any tool cannot be a bare wildcard".into());
            }
        }
        Ok(())
    }
}

/// Explanation returned when an item in `ResolvePermissionsBatch` is skipped for safety or state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchPermissionSkipReason {
    pub agent_id: String,
    pub permission_request_id: String,
    pub reason: String,
}

/// Aggregated result of a server-verified `ResolvePermissionsBatch` RPC.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ResolvePermissionsBatchResult {
    pub resolved_count: usize,
    pub skipped_count: usize,
    #[serde(default)]
    pub skipped_reasons: Vec<BatchPermissionSkipReason>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_rule_record_serde_roundtrip() {
        let now = Utc::now();
        let rec = PermissionRuleRecord {
            id: "pr_001".into(),
            scope_kind: PermissionRuleScopeKind::Project,
            project_cwd: Some("/workspace/orbit".into()),
            provider: Some(ProviderKind::Codex),
            tool_kind: PermissionRuleToolKind::Shell,
            pattern_kind: PermissionRulePatternKind::CommandPrefix,
            pattern: "cargo test".into(),
            decision: PermissionRuleDecision::Allow,
            source: PermissionRuleSource::AlwaysAllowGrant,
            note: Some("Granted via Always allow (Codex)".into()),
            created_at: now,
            last_matched_at: Some(now),
            match_count: 7,
        };
        let encoded = serde_json::to_string(&rec).expect("serialize rule");
        let decoded: PermissionRuleRecord = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, rec);
    }

    #[test]
    fn upsert_input_validation_rejects_empty_and_wildcard_allow() {
        let empty = UpsertPermissionRuleInput {
            id: None,
            scope_kind: PermissionRuleScopeKind::Host,
            project_cwd: None,
            provider: None,
            tool_kind: PermissionRuleToolKind::Shell,
            pattern_kind: PermissionRulePatternKind::CommandPrefix,
            pattern: "   ".into(),
            decision: PermissionRuleDecision::Allow,
            source: PermissionRuleSource::Manual,
            note: None,
        };
        assert!(empty.validate_basic().is_err());

        let missing_cwd = UpsertPermissionRuleInput {
            scope_kind: PermissionRuleScopeKind::Project,
            pattern: "cargo test".into(),
            ..empty.clone()
        };
        assert!(missing_cwd.validate_basic().is_err());

        let wildcard_allow = UpsertPermissionRuleInput {
            scope_kind: PermissionRuleScopeKind::Host,
            pattern: "*".into(),
            ..empty
        };
        assert!(wildcard_allow.validate_basic().is_err());
    }

    #[test]
    fn resolve_permission_agent_originated_flag_defaults_false_and_roundtrips() {
        let req_id = uuid::Uuid::new_v4();
        // Legacy payload without `agent_originated` deserializes as `false`
        let legacy_json = format!(
            r#"{{"op":"resolve_permission","request_id":"{req_id}","agent_id":"ag1","permission_request_id":"p1","decision":"allow"}}"#
        );
        let decoded_legacy: crate::WireMessage =
            serde_json::from_str(&legacy_json).expect("decode legacy");
        match decoded_legacy {
            crate::WireMessage::ResolvePermission {
                agent_originated, ..
            } => assert!(!agent_originated),
            other => panic!("unexpected variant: {other:?}"),
        }

        // Agent-originated payload round-trips `agent_originated: true`
        let msg = crate::WireMessage::ResolvePermission {
            request_id: req_id,
            agent_id: "ag1".into(),
            permission_request_id: "p1".into(),
            decision: "allow".into(),
            agent_originated: true,
        };
        let encoded = serde_json::to_string(&msg).expect("encode");
        assert!(encoded.contains("\"agent_originated\":true"));
        let decoded: crate::WireMessage = serde_json::from_str(&encoded).expect("decode");
        match decoded {
            crate::WireMessage::ResolvePermission {
                agent_originated, ..
            } => assert!(agent_originated),
            other => panic!("unexpected variant: {other:?}"),
        }
    }
}
