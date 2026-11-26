//! Wire types for the Personal Router ("Chief-of-Staff / 分工师") orchestration role,
//! multi-host Fleet Router auto-discovery, 3-tier Guardian pre-review policy, and
//! bidirectional Hermes MCP / Webhook bridging.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AgentStatus, AgentSummary, ProviderKind, ReasoningEffort};

/// Label key persisted on `AgentSummary.labels` identifying the session's orchestration role.
pub const LABEL_AGENT_ROLE: &str = "orbit.role";
/// Label value stored under [`LABEL_AGENT_ROLE`] for the host's canonical Personal Router session.
pub const AGENT_ROLE_PERSONAL_ROUTER: &str = "personal_router";
/// Audit-chain category recorded for Personal Agent, Guardian Tier-1, and permission-rule decisions.
pub const AUDIT_CATEGORY_PERSONAL_AGENT_DECISION: &str = "personal_agent_decision";

/// Label key persisted on `AgentSummary.labels` recording the immutable visibility & lifecycle tier.
pub const LABEL_SESSION_TIER: &str = "orbit.session_tier";
pub const SESSION_TIER_CONVERSATION: &str = "conversation";
pub const SESSION_TIER_SUBAGENT: &str = "subagent";
pub const SESSION_TIER_PERSONAL_ROUTER: &str = "personal_router";
pub const SESSION_TIER_AUTOMATION: &str = "automation";

/// Label keys persisted on `AgentSummary.labels` when a user-visible `Conversation` was dispatched
/// by a `PersonalRouter` (kept strictly orthogonal to `parent_agent_id` so routed conversations
/// appear in the user's Left Sidebar while still notifying and answering to the Router).
pub const LABEL_ROUTED_BY_AGENT: &str = "orbit.routed_by_agent";
pub const LABEL_ROUTED_BY_HOST: &str = "orbit.routed_by_host";

/// Canonical session visibility and lifecycle tier across Daemon, Store, and all Client UIs.
///
/// Only [`SessionTier::Conversation`] sessions appear in the user's Left Sidebar, Workspace Peer
/// strips, Auto-Organizer, and `<orbit-fleet-atlas>`. [`SessionTier::Subagent`] sessions (both
/// local and cross-host remote) are strictly encapsulated inside their parent Conversation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTier {
    /// Primary user-visible conversation in the Left Sidebar (created ONLY by Human or PersonalRouter).
    #[default]
    Conversation,
    /// Strictly internal child execution session of a Conversation (local or cross-host; NEVER in Sidebar).
    Subagent,
    /// Persistent Personal Router ("Chief-of-Staff / 分工师") session.
    PersonalRouter,
    /// Scheduled/background automation or ephemeral system organizer session.
    Automation,
}

impl SessionTier {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Conversation => SESSION_TIER_CONVERSATION,
            Self::Subagent => SESSION_TIER_SUBAGENT,
            Self::PersonalRouter => SESSION_TIER_PERSONAL_ROUTER,
            Self::Automation => SESSION_TIER_AUTOMATION,
        }
    }

    pub fn from_slug(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "conversation" | "primary" => Some(Self::Conversation),
            "subagent" | "child" => Some(Self::Subagent),
            "personal_router" | "router" | "chief_of_staff" => Some(Self::PersonalRouter),
            "automation" | "background" => Some(Self::Automation),
            _ => None,
        }
    }
}

/// Canonical single-source-of-truth resolution of [`SessionTier`] shared by `AgentSummary` and `AgentCardView`.
///
/// Crucially, `Subagent` status NEVER depends on `parent_host_id.is_none()` or whether the parent
/// session is still unarchived in memory: cross-host remote subagents and orphaned subagents remain
/// `SessionTier::Subagent` deterministically.
pub fn resolve_session_tier(
    background: bool,
    parent_agent_id: Option<&str>,
    labels: &std::collections::HashMap<String, String>,
) -> SessionTier {
    if background {
        return SessionTier::Automation;
    }
    if labels
        .get(LABEL_AGENT_ROLE)
        .is_some_and(|v| AgentRole::from_slug(v) == AgentRole::PersonalRouter)
    {
        return SessionTier::PersonalRouter;
    }
    if let Some(explicit) = labels
        .get(LABEL_SESSION_TIER)
        .and_then(|v| SessionTier::from_slug(v))
    {
        return explicit;
    }
    if parent_agent_id.is_some_and(|p| !p.trim().is_empty())
        && !labels.contains_key(LABEL_ROUTED_BY_AGENT)
    {
        return SessionTier::Subagent;
    }
    SessionTier::Conversation
}

fn router_default_true() -> bool {
    true
}

/// Logical role of an Orbit agent session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    /// Standard coding/research worker conversation (default).
    #[default]
    Worker,
    /// Persistent personal router / chief-of-staff orchestrating worker conversations.
    PersonalRouter,
}

impl AgentRole {
    pub const fn slug(self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::PersonalRouter => AGENT_ROLE_PERSONAL_ROUTER,
        }
    }

    pub fn from_slug(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "personal_router" | "router" | "chief_of_staff" => Self::PersonalRouter,
            _ => Self::Worker,
        }
    }
}

/// Visibility and control scope granted to the Personal Router.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterScope {
    /// Global view & orchestration authority across the local host and all connected Fleet hosts.
    #[default]
    GlobalFleet,
    /// Restricted to conversations and workspaces on the local host only.
    LocalHostOnly,
}

/// Autonomy level for Personal Router pre-review of Worker permission checkpoints.
///
/// NOTE: Tier-3 (`High` / `Critical`) security-boundary requests are ALWAYS fail-closed and
/// hard-blocked from Router auto-approval regardless of this setting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterPreReviewMode {
    /// Never route permission checkpoints to the Personal Router; escalate directly to human.
    Disabled,
    /// Router may pre-review and allow/deny `Medium`-risk checkpoints when aligned with user intent;
    /// `High`/`Critical` always escalate to human interactive cards.
    #[default]
    MediumRiskOnly,
    /// Router summarizes context and recommends allow/deny to the human, but never resolves directly.
    AssistOnly,
}

/// Controls how messages in the canonical Personal Router session mirror to bound IM channels (e.g. Feishu).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouterImMirrorPolicy {
    /// Only send replies back to IM for turns that originated from IM.
    ReplyOriginOnly,
    /// Reply to IM turns AND push milestone summaries & Tier-3 approval cards to bound IM chats.
    #[default]
    MilestonesAndApprovals,
    /// Mirror all Router assistant responses and approval cards to bound IM chats.
    MirrorAll,
}

/// Host-local configuration for the Personal Router persisted in `$ORBIT_HOME/personal_router.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalRouterConfig {
    /// Whether the Personal Router role is enabled on this host.
    #[serde(default)]
    pub enabled: bool,
    /// Canonical persistent agent session ID for this host's Personal Router.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_agent_id: Option<String>,
    /// Underlying provider driving the Personal Router (defaults to `Acp` with `acp_agent_id = "hermes"`).
    #[serde(default = "default_router_provider")]
    pub provider: ProviderKind,
    /// ACP registry ID when `provider == ProviderKind::Acp` (default: `Some("hermes")`).
    #[serde(
        default = "default_router_acp_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub acp_agent_id: Option<String>,
    /// Optional explicit model override for the Router brain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional reasoning effort override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<ReasoningEffort>,
    /// Working directory for the Personal Router session (defaults to user home directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Visibility & control scope (`GlobalFleet` or `LocalHostOnly`).
    #[serde(default)]
    pub scope: RouterScope,
    /// Tier-2 permission pre-review autonomy policy (Tier-3 is always hard-blocked).
    #[serde(default)]
    pub pre_review_mode: RouterPreReviewMode,
    /// Share and interoperate with the host's native `~/.hermes/memories/` & skills (default: `true`).
    #[serde(default = "router_default_true")]
    pub share_host_hermes_memory: bool,
    /// Optional user-customized routing persona / discipline appended to the Router SOUL prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul_prompt_override: Option<String>,
    /// Optional outbound webhook URL to push Worker finish/error/approval events to an external Hermes Gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_webhook_url: Option<String>,
    /// Whether an external webhook signing/bearer secret is stored on disk (secret itself is never serialized).
    #[serde(default)]
    pub has_external_webhook_secret: bool,
}

fn default_router_provider() -> ProviderKind {
    ProviderKind::Acp
}

fn default_router_acp_id() -> Option<String> {
    Some("hermes".to_string())
}

impl Default for PersonalRouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            router_agent_id: None,
            provider: default_router_provider(),
            acp_agent_id: default_router_acp_id(),
            model: None,
            effort: None,
            cwd: None,
            scope: RouterScope::GlobalFleet,
            pre_review_mode: RouterPreReviewMode::MediumRiskOnly,
            share_host_hermes_memory: true,
            soul_prompt_override: None,
            external_webhook_url: None,
            has_external_webhook_secret: false,
        }
    }
}

/// Detected Personal Router & Hermes readiness on a single host in the Multi-Host Fleet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterHostCapabilities {
    pub host_id: String,
    pub host_name: String,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub online: bool,
    /// Whether the `hermes` CLI binary was found on PATH or standard install locations.
    #[serde(default)]
    pub hermes_installed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hermes_version: Option<String>,
    /// Whether existing Hermes long-term memory (`~/.hermes/memories/` or `state.db`) exists on this host.
    #[serde(default)]
    pub hermes_has_existing_memory: bool,
    /// Whether `hermes acp` is supported and ready to spawn.
    #[serde(default)]
    pub hermes_acp_ready: bool,
    /// Whether a background `hermes gateway` process/port appears active on this host.
    #[serde(default)]
    pub hermes_gateway_running: bool,
    /// Fallback / alternative native coding providers installed on this host.
    #[serde(default)]
    pub available_providers: Vec<ProviderKind>,
    /// Active Personal Router session ID on this host, if already created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_router_agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_router_status: Option<AgentStatus>,
    #[serde(default)]
    pub router_enabled: bool,
    /// Heuristic recommendation score (0..=100) for ranking candidate Router hosts in the UI.
    #[serde(default)]
    pub recommended_score: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_reason: Option<String>,
}

/// Complete live snapshot of the Personal Router on a host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterStatusSnapshot {
    pub config: PersonalRouterConfig,
    pub host_capabilities: RouterHostCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub router_agent: Option<AgentSummary>,
    #[serde(default)]
    pub managed_workers_count: usize,
    #[serde(default)]
    pub pending_approvals_count: usize,
}

/// Exportable configuration snippets for connecting an external Hermes Gateway to Orbit MCP + Webhooks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HermesBridgeBundle {
    pub host_id: String,
    pub host_name: String,
    pub mcp_http_url: String,
    pub mcp_yaml_snippet: String,
    pub acp_toolset_yaml_snippet: String,
    pub soul_md_template: String,
    pub skill_md_template: String,
}

/// Personal Agent's non-binding recommendation attached to a pending permission request
/// (used in `assist_only` mode or for Tier-3 human-only checkpoints).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterRecommendation {
    /// Recommended decision: `"allow"` or `"deny"`.
    pub decision: String,
    pub reason: String,
    pub created_at: DateTime<Utc>,
}

/// Logged automatic permission decision performed by the Personal Agent, Guardian Tier-1,
/// or a persistent permission rule, projected from the host's signed security audit chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalAgentActivityEntry {
    pub seq: u64,
    pub timestamp_ms: i64,
    pub actor: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub agent_title: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// Client-persisted global selection of WHICH host in the Multi-Host Fleet owns the user's
/// canonical Primary Personal Router (e.g. an always-on remote Mac mini or cloud Devbox).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrimaryRouterSelection {
    pub host_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub updated_at_ms: u64,
}

/// Concise project entry discovered on a local or connected remote host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetProjectEntry {
    pub host_id: String,
    pub host_name: String,
    #[serde(default)]
    pub is_local: bool,
    pub name: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
}

/// Concise curated Mission Workspace entry discovered on a local or connected remote host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetWorkspaceEntry {
    pub host_id: String,
    pub host_name: String,
    #[serde(default)]
    pub is_local: bool,
    pub workspace_id: String,
    pub title: String,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_goal: Option<String>,
    #[serde(default)]
    pub board_revision: u64,
}

/// Token-bounded remote/local project context packet fetched by a Personal Router before
/// dispatching or steering a Worker on any connected host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectContextPacket {
    pub host_id: String,
    pub host_name: String,
    pub cwd: String,
    pub project_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub git_dirty: bool,
    #[serde(default)]
    pub top_level_entries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme_excerpt: Option<String>,
    #[serde(default)]
    pub active_workspaces: Vec<String>,
    #[serde(default)]
    pub active_workers: Vec<String>,
}

impl AgentSummary {
    /// Return the logical orchestration role of this conversation (`Worker` or `PersonalRouter`).
    pub fn role(&self) -> AgentRole {
        self.labels
            .get(LABEL_AGENT_ROLE)
            .map(|v| AgentRole::from_slug(v))
            .unwrap_or(AgentRole::Worker)
    }

    /// Returns `true` when this conversation is a host Personal Router ("Chief-of-Staff / 分工师").
    pub fn is_personal_router(&self) -> bool {
        self.role() == AgentRole::PersonalRouter
    }

    /// Return the authoritative visibility & lifecycle tier of this session.
    pub fn session_tier(&self) -> SessionTier {
        resolve_session_tier(
            self.background,
            self.parent_agent_id.as_deref(),
            &self.labels,
        )
    }

    /// Returns `true` when this session is an internal `Subagent` (local or remote; hidden from Sidebar).
    pub fn is_subagent(&self) -> bool {
        self.session_tier() == SessionTier::Subagent
    }

    /// Returns `true` when this session is a primary user-visible `Conversation` in the Sidebar.
    pub fn is_primary_conversation(&self) -> bool {
        self.session_tier() == SessionTier::Conversation
    }

    /// Explicitly persist the session's [`SessionTier`] in `self.labels`.
    pub fn set_session_tier(&mut self, tier: SessionTier) {
        self.labels
            .insert(LABEL_SESSION_TIER.to_string(), tier.slug().to_string());
    }

    /// Return the `PersonalRouter` agent ID that dispatched this conversation, if any.
    pub fn routed_by_agent_id(&self) -> Option<&str> {
        self.labels
            .get(LABEL_ROUTED_BY_AGENT)
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Return the `PersonalRouter` host ID that dispatched this conversation, if any.
    pub fn routed_by_host_id(&self) -> Option<&str> {
        self.labels
            .get(LABEL_ROUTED_BY_HOST)
            .map(String::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// Stamp Personal Router provenance on a user-visible `Conversation` without turning it into a hidden `Subagent`.
    pub fn set_routed_by(&mut self, router_agent_id: String, router_host_id: Option<String>) {
        self.labels
            .insert(LABEL_ROUTED_BY_AGENT.to_string(), router_agent_id);
        if let Some(hid) = router_host_id.filter(|h| !h.trim().is_empty()) {
            self.labels.insert(LABEL_ROUTED_BY_HOST.to_string(), hid);
        }
    }

    /// Set the logical orchestration role in `self.labels`.
    pub fn set_role(&mut self, role: AgentRole) {
        match role {
            AgentRole::Worker => {
                self.labels.remove(LABEL_AGENT_ROLE);
                if self.labels.get(LABEL_SESSION_TIER).map(String::as_str)
                    == Some(SESSION_TIER_PERSONAL_ROUTER)
                {
                    self.labels.remove(LABEL_SESSION_TIER);
                }
            }
            AgentRole::PersonalRouter => {
                self.labels.insert(
                    LABEL_AGENT_ROLE.to_string(),
                    AGENT_ROLE_PERSONAL_ROUTER.to_string(),
                );
                self.labels.insert(
                    LABEL_SESSION_TIER.to_string(),
                    SESSION_TIER_PERSONAL_ROUTER.to_string(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn router_config_roundtrip_defaults() {
        let cfg = PersonalRouterConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.provider, ProviderKind::Acp);
        assert_eq!(cfg.acp_agent_id.as_deref(), Some("hermes"));
        assert_eq!(cfg.scope, RouterScope::GlobalFleet);
        assert_eq!(cfg.pre_review_mode, RouterPreReviewMode::MediumRiskOnly);
        assert!(cfg.share_host_hermes_memory);

        let json = serde_json::to_string(&cfg).expect("serialize");
        let back: PersonalRouterConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cfg, back);
    }

    #[test]
    fn agent_role_slug_parsing() {
        assert_eq!(
            AgentRole::from_slug("personal_router"),
            AgentRole::PersonalRouter
        );
        assert_eq!(AgentRole::from_slug("router"), AgentRole::PersonalRouter);
        assert_eq!(AgentRole::from_slug("worker"), AgentRole::Worker);
        assert_eq!(AgentRole::from_slug("unknown"), AgentRole::Worker);
    }

    #[test]
    fn pending_permission_summary_backward_compat_without_router_recommendation() {
        let legacy_json = serde_json::json!({
            "agent_id": "ag_1",
            "agent_title": "Worker 1",
            "workspace_id": "ws_1",
            "request_id": "perm_1",
            "title": "Run tests",
            "detail": {
                "kind": "shell",
                "command": "cargo test",
                "cwd": null,
                "exit_code": null,
                "output_preview": null
            },
            "allowed_decisions": ["allow", "deny"],
            "created_at": "2026-11-26T00:00:00Z"
        });
        let parsed: crate::PendingPermissionSummary =
            serde_json::from_value(legacy_json).expect("legacy PendingPermissionSummary parses");
        assert!(parsed.router_recommendation.is_none());
        assert!(!parsed.human_only);

        let wire_list: crate::WireMessage = serde_json::from_value(serde_json::json!({
            "op": "list_personal_agent_activity",
            "request_id": uuid::Uuid::new_v4(),
            "limit": 50
        }))
        .expect("ListPersonalAgentActivity parses without since_ms");
        assert!(matches!(
            wire_list,
            crate::WireMessage::ListPersonalAgentActivity {
                limit: 50,
                since_ms: None,
                ..
            }
        ));

        let req_id = uuid::Uuid::new_v4();
        let rec_wire = crate::WireMessage::RecommendPermission {
            request_id: req_id,
            agent_id: "ag_1".into(),
            permission_request_id: "perm_1".into(),
            decision: "deny".into(),
            reason: "Too broad".into(),
        };
        let encoded = serde_json::to_string(&rec_wire).expect("serialize RecommendPermission");
        let decoded: crate::WireMessage =
            serde_json::from_str(&encoded).expect("deserialize RecommendPermission");
        assert!(matches!(
            decoded,
            crate::WireMessage::RecommendPermission {
                request_id: rid,
                ref decision,
                ..
            } if rid == req_id && decision == "deny"
        ));
    }

    #[test]
    fn resolve_session_tier_keeps_remote_and_orphan_subagents_hidden_and_routed_conversations_visible()
     {
        let empty = HashMap::new();
        assert_eq!(
            resolve_session_tier(false, None, &empty),
            SessionTier::Conversation
        );
        assert_eq!(
            resolve_session_tier(false, Some("parent_remote"), &empty),
            SessionTier::Subagent
        );
        assert_eq!(
            resolve_session_tier(true, None, &empty),
            SessionTier::Automation
        );

        let mut routed_labels = HashMap::new();
        routed_labels.insert(LABEL_ROUTED_BY_AGENT.to_string(), "ag_router".to_string());
        routed_labels.insert(
            LABEL_SESSION_TIER.to_string(),
            SESSION_TIER_CONVERSATION.to_string(),
        );
        assert_eq!(
            resolve_session_tier(false, None, &routed_labels),
            SessionTier::Conversation
        );
    }
}
