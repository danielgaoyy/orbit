use super::*;
use orbit_client_core::contract::PermissionRequestId as FfiPermissionRequestId;
#[uniffi::remote(Record)]
pub struct FfiPermissionRequestId {
    pub agent_id: String,
    pub permission_request_id: String,
}
use orbit_protocol::AgentStatus;
#[uniffi::remote(Enum)]
pub enum AgentStatus {
    Initializing,
    Idle,
    Running { turn_id: String },

    AutonomousWakeup { turn_id: String, source: String },

    AwaitingPermission { turn_id: String, request_id: String },
    Error { message: String, recoverable: bool },
    Closed,
}
use orbit_client_core::contract::ConversationSearchHit;
#[uniffi::remote(Record)]
pub struct ConversationSearchHit {
    pub host_id: String,
    pub host_name: String,
    pub agent_id: String,
    pub seq: u64,
    pub turn_id: Option<String>,
    pub role: String,
    pub title: String,
    pub project_name: String,
    pub snippet: String,
    pub updated_at_rfc3339: String,
    pub is_remote: bool,
    pub is_unloaded: bool,
}
use orbit_protocol::DagStepSpec;
#[uniffi::remote(Record)]
pub struct DagStepSpec {
    pub step_id: String,
    pub name: String,
    pub depends_on: Vec<String>,
    pub prompt_template: String,
    pub provider: Option<ProviderKind>,
    pub model: Option<String>,
    pub isolated_worktree: bool,
    pub retry_count: u32,
    pub timeout_secs: u64,
}
use orbit_protocol::GitCiCheckState;
#[uniffi::remote(Enum)]
pub enum GitCiCheckState {
    Passing,
    Failing,
    Pending,
    Neutral,
}
use orbit_protocol::GitCommitResult;
#[uniffi::remote(Record)]
pub struct GitCommitResult {
    pub commit_sha: String,
    pub short_sha: String,
    pub summary: String,
    pub branch: Option<String>,
}
use orbit_protocol::GitConflictBlock;
#[uniffi::remote(Record)]
pub struct GitConflictBlock {
    pub index: u32,
    pub start_line: u32,
    pub end_line: u32,
    pub ours_label: String,
    pub base_label: Option<String>,
    pub theirs_label: String,
    pub ours: String,
    pub base: Option<String>,
    pub theirs: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}
use orbit_protocol::GitConflictFile;
#[uniffi::remote(Record)]
pub struct GitConflictFile {
    pub path: String,
    pub blocks: Vec<GitConflictBlock>,
    pub is_binary: bool,
    pub unmerged_in_index: bool,
    pub total_blocks: u32,
    pub content_sha256: Option<String>,
}
use orbit_protocol::GitConflictListResult;
#[uniffi::remote(Record)]
pub struct GitConflictListResult {
    pub files: Vec<GitConflictFile>,
}
use orbit_protocol::GitConflictResolution;
#[uniffi::remote(Enum)]
pub enum GitConflictResolution {
    Ours,
    Theirs,
    Both,
    ManualContent { content: String },
}
use orbit_protocol::GitCreatePrResult;
#[uniffi::remote(Record)]
pub struct GitCreatePrResult {
    pub url: String,
    pub number: Option<u64>,
    pub title: String,
}
use orbit_protocol::GitGeneratedCommitMessage;
#[uniffi::remote(Record)]
pub struct GitGeneratedCommitMessage {
    pub message: String,
    pub subject: String,
    pub body: Option<String>,
    pub commit_type: String,
    pub scope: Option<String>,
    pub diff_source: String,
    pub files_analyzed: u32,
    pub generator: String,
}
use orbit_protocol::GitMutationOutcome;
#[uniffi::remote(Enum)]
pub enum GitMutationOutcome {
    FilesStaged {
        paths: Vec<String>,
        all: bool,
    },
    FilesUnstaged {
        paths: Vec<String>,
        all: bool,
    },
    HunkApplied {
        reverse: bool,
        cached: bool,
    },
    FilesDiscarded {
        paths: Vec<String>,
        include_untracked: bool,
    },
    Committed(GitCommitResult),
    Pushed(GitPushResult),
    PrCreated(GitCreatePrResult),
    StashMutated(GitStashMutationResult),
    RebaseAction(GitRebaseActionResult),
    ConflictResolved(GitResolveConflictResult),
}
use orbit_protocol::GitPrAvailability;
#[uniffi::remote(Enum)]
pub enum GitPrAvailability {
    Available,
    NoPullRequest,
    Unavailable,
}
use orbit_protocol::GitPrCheckItem;
#[uniffi::remote(Record)]
pub struct GitPrCheckItem {
    pub name: String,
    pub state: GitCiCheckState,
    pub workflow: Option<String>,
    pub raw_status: Option<String>,
    pub raw_conclusion: Option<String>,
    pub details_url: Option<String>,
}
use orbit_protocol::GitPrReviewComment;
#[uniffi::remote(Record)]
pub struct GitPrReviewComment {
    pub id: String,
    pub author: String,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub url: Option<String>,
    pub created_at: Option<String>,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub kind: String,
}
use orbit_protocol::GitPrStatus;
#[uniffi::remote(Record)]
pub struct GitPrStatus {
    pub availability: GitPrAvailability,
    pub reason: Option<String>,
    pub number: Option<u64>,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<String>,
    pub is_draft: bool,
    pub mergeable: Option<String>,
    pub review_decision: Option<String>,
    pub head_ref: Option<String>,
    pub base_ref: Option<String>,
    pub checks: Vec<GitPrCheckItem>,
    pub passing_checks: u32,
    pub failing_checks: u32,
    pub pending_checks: u32,
    pub review_comments: Vec<GitPrReviewComment>,
    pub unresolved_comment_count: u32,
}
use orbit_protocol::GitPushResult;
#[uniffi::remote(Record)]
pub struct GitPushResult {
    pub remote: String,
    pub branch: String,
    pub output: String,
}
use orbit_protocol::GitRebaseActionResult;
#[uniffi::remote(Record)]
pub struct GitRebaseActionResult {
    pub action: String,
    pub status: GitRebaseStatus,
    pub summary: String,
}
use orbit_protocol::GitRebaseKind;
#[uniffi::remote(Enum)]
pub enum GitRebaseKind {
    None,
    Merge,
    Apply,
}
use orbit_protocol::GitRebaseStatus;
#[uniffi::remote(Record)]
pub struct GitRebaseStatus {
    pub in_progress: bool,
    pub kind: GitRebaseKind,
    pub current_step: Option<u32>,
    pub total_steps: Option<u32>,
    pub head_name: Option<String>,
    pub onto: Option<String>,
    pub stopped_message: Option<String>,
    pub conflicted_files: Vec<String>,
    pub merge_in_progress: bool,
    pub cherry_pick_in_progress: bool,
}
use orbit_protocol::GitResolveConflictResult;
#[uniffi::remote(Record)]
pub struct GitResolveConflictResult {
    pub path: String,
    pub remaining_blocks: u32,
    pub staged: bool,
    pub summary: String,
}
use orbit_protocol::GitStashEntry;
#[uniffi::remote(Record)]
pub struct GitStashEntry {
    pub stash_ref: String,
    pub index: u32,
    pub branch: Option<String>,
    pub message: String,
    pub oid: Option<String>,
    pub short_sha: Option<String>,
    pub created_at: Option<String>,
}
use orbit_protocol::GitStashListResult;
#[uniffi::remote(Record)]
pub struct GitStashListResult {
    pub entries: Vec<GitStashEntry>,
}
use orbit_protocol::GitStashMutationResult;
#[uniffi::remote(Record)]
pub struct GitStashMutationResult {
    pub action: String,
    pub stash_ref: Option<String>,
    pub summary: String,
    pub had_conflicts: bool,
}
use orbit_protocol::GuardianPolicyConfig;
#[uniffi::remote(Record)]
pub struct GuardianPolicyConfig {
    pub enabled: bool,
    pub auto_approve_max_risk: GuardianRiskLevel,
    pub use_ai_classifier_for_medium: bool,
    pub classifier_timeout_ms: u64,
    pub allow_read_only_tools: bool,
    pub allow_workspace_tests: bool,
    pub blocked_command_patterns: Vec<String>,
    pub trusted_command_prefixes: Vec<String>,
    pub llm_tier2_enabled: bool,
    pub llm_provider: Option<ProviderKind>,
    pub llm_model: Option<String>,
    pub llm_timeout_ms: u64,
}
use orbit_protocol::GuardianRiskLevel;
#[uniffi::remote(Enum)]
pub enum GuardianRiskLevel {
    Low,
    Medium,
    High,
    Critical,
}
use orbit_protocol::MonitorKind;
#[uniffi::remote(Enum)]
pub enum MonitorKind {
    CommandStream {
        command: String,

        args: Vec<String>,

        pattern: Option<String>,
    },
    LogFileTail {
        path: String,
        pattern: String,
    },
    HttpHealth {
        url: String,

        expect_status: u16,

        interval_secs: u64,
    },
}
use orbit_protocol::PermissionRuleDecision;
#[uniffi::remote(Enum)]
pub enum PermissionRuleDecision {
    Allow,
    Deny,
}
use orbit_protocol::PermissionRulePatternKind;
#[uniffi::remote(Enum)]
pub enum PermissionRulePatternKind {
    CommandPrefix,
    ExactCommand,
    PathGlob,
    ToolName,
}
use orbit_protocol::PermissionRuleScopeKind;
#[uniffi::remote(Enum)]
pub enum PermissionRuleScopeKind {
    Host,
    Project,
}
use orbit_protocol::PermissionRuleSource;
#[uniffi::remote(Enum)]
pub enum PermissionRuleSource {
    Manual,
    AlwaysAllowGrant,
}
use orbit_protocol::PermissionRuleToolKind;
#[uniffi::remote(Enum)]
pub enum PermissionRuleToolKind {
    Shell,
    FileWrite,
    FileRead,
    McpTool,
    Network,
    Any,
}
use orbit_protocol::PersonalRouterConfig;
#[uniffi::remote(Record)]
pub struct PersonalRouterConfig {
    pub enabled: bool,
    pub router_agent_id: Option<String>,
    pub provider: ProviderKind,
    pub acp_agent_id: Option<String>,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub cwd: Option<String>,
    pub scope: RouterScope,
    pub pre_review_mode: RouterPreReviewMode,
    pub share_host_hermes_memory: bool,
    pub soul_prompt_override: Option<String>,
    pub external_webhook_url: Option<String>,
    pub has_external_webhook_secret: bool,
}
use orbit_protocol::PlanApprovalAction;
#[uniffi::remote(Enum)]
pub enum PlanApprovalAction {
    ApproveAndBuild,
    RequestChanges,
    Dismiss,
}
use orbit_protocol::ProviderKind;
#[uniffi::remote(Enum)]
pub enum ProviderKind {
    Antigravity,
    Codex,
    ClaudeCode,
    OpenCode,
    GeminiCli,
    CursorAgent,
    Copilot,
    Goose,
    Pi,
    Kimi,
    Cline,
    MistralVibe,
    Augment,
    Acp,
}
use orbit_protocol::PushPreferences;
#[uniffi::remote(Record)]
pub struct PushPreferences {
    pub enabled: bool,
    pub notify_permission_required: bool,
    pub notify_turn_completed: bool,
    pub notify_agent_error: bool,
    pub notify_goal_finished: bool,
    pub notify_timer_fired: bool,
    pub include_message_preview: bool,
    pub quiet_hours_enabled: bool,
    pub quiet_start_hour: u8,
    pub quiet_end_hour: u8,
    pub push_gateway_url: Option<String>,
    pub has_gateway_auth_token: bool,
}
use orbit_client_core::app_core::PushTestResultView;
#[uniffi::remote(Record)]
pub struct PushTestResultView {
    pub attempted: u32,
    pub delivered: u32,
    pub detail: String,
}
use orbit_protocol::ReasoningEffort;
#[uniffi::remote(Enum)]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}
use orbit_protocol::RouterHostCapabilities;
#[uniffi::remote(Record)]
pub struct RouterHostCapabilities {
    pub host_id: String,
    pub host_name: String,
    pub is_local: bool,
    pub online: bool,
    pub hermes_installed: bool,
    pub hermes_version: Option<String>,
    pub hermes_has_existing_memory: bool,
    pub hermes_acp_ready: bool,
    pub hermes_gateway_running: bool,
    pub available_providers: Vec<ProviderKind>,
    pub active_router_agent_id: Option<String>,
    pub active_router_status: Option<AgentStatus>,
    pub router_enabled: bool,
    pub recommended_score: u8,
    pub recommendation_reason: Option<String>,
}
use orbit_protocol::RouterPreReviewMode;
#[uniffi::remote(Enum)]
pub enum RouterPreReviewMode {
    Disabled,
    MediumRiskOnly,
    AssistOnly,
}
use orbit_protocol::RouterScope;
#[uniffi::remote(Enum)]
pub enum RouterScope {
    GlobalFleet,
    LocalHostOnly,
}
use orbit_client_core::contract::SearchMessagesRequest;
#[uniffi::remote(Record)]
pub struct SearchMessagesRequest {
    pub query: String,
    pub host_id: Option<String>,
    pub agent_id: Option<String>,
    pub limit: Option<u32>,
}
use orbit_client_core::contract::SearchMessagesResponse;
#[uniffi::remote(Record)]
pub struct SearchMessagesResponse {
    pub hits: Vec<ConversationSearchHit>,
    pub remote_hosts_queried: u32,
    pub remote_hosts_failed: u32,
}
use orbit_protocol::SessionMode;
#[uniffi::remote(Enum)]
pub enum SessionMode {
    Default,
    Auto,
    Plan,
    AcceptEdits,
    FullAccess,
    Custom(String),
}
use orbit_protocol::SessionUsage;
#[uniffi::remote(Record)]
pub struct SessionUsage {
    pub agent_id: String,
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub estimated_cost_usd: f64,
    pub cache_savings_usd: f64,
}
use orbit_client_core::contract::StartDagWorkflowRequest;
#[uniffi::remote(Record)]
pub struct StartDagWorkflowRequest {
    pub host_id: String,
    pub name: String,
    pub cwd: String,
    pub steps: Vec<DagStepSpec>,
    pub default_provider: ProviderKind,
    pub default_mode: SessionMode,
    pub max_concurrency: Option<u32>,
}
use orbit_client_core::contract::StartGoalRunRequest;
#[uniffi::remote(Record)]
pub struct StartGoalRunRequest {
    pub host_id: String,
    pub agent_id: String,
    pub objective: String,
    pub criteria: Vec<String>,
    pub verify_command: Option<String>,
    pub max_iterations: Option<u32>,
    pub max_duration_secs: Option<u64>,
    pub replace_existing: bool,
    pub dispatch_initial_prompt: Option<bool>,
}
use orbit_client_core::contract::StartMonitorWatchRequest;
#[uniffi::remote(Record)]
pub struct StartMonitorWatchRequest {
    pub host_id: String,
    pub agent_id: String,
    pub name: String,
    pub kind: MonitorKind,
    pub expires_in_secs: Option<u64>,
}
use orbit_protocol::TimerCondition;
#[uniffi::remote(Enum)]
pub enum TimerCondition {
    Never,
    AnyMessage,

    Sender { sender_id: String },
    AllSubagentsSettled,
    AllPeersSettled,

    FileChanged { path: String },
}
use orbit_client_core::app_core::UpdatePushPreferencesRequest;
#[uniffi::remote(Record)]
pub struct UpdatePushPreferencesRequest {
    pub preferences: PushPreferences,
    pub gateway_auth_token: Option<String>,
    pub keep_existing_gateway_token: bool,
}
use orbit_protocol::UpsertPermissionRuleInput;
#[uniffi::remote(Record)]
pub struct UpsertPermissionRuleInput {
    pub id: Option<String>,
    pub scope_kind: PermissionRuleScopeKind,
    pub project_cwd: Option<String>,
    pub provider: Option<ProviderKind>,
    pub tool_kind: PermissionRuleToolKind,
    pub pattern_kind: PermissionRulePatternKind,
    pub pattern: String,
    pub decision: PermissionRuleDecision,
    pub source: PermissionRuleSource,
    pub note: Option<String>,
}

type MobileCount = usize;
uniffi::custom_type!(MobileCount, u64, { remote, lower: |v| v as u64, try_lift: |v| Ok(usize::try_from(v)?) });
type Rfc3339Timestamp = chrono::DateTime<chrono::Utc>;
uniffi::custom_type!(Rfc3339Timestamp, String, { remote, lower: |v| v.to_rfc3339(), try_lift: |v| Ok(v.parse()?) });

use orbit_protocol::AgentSummary;
#[uniffi::remote(Record)]
pub struct AgentSummary {
    pub agent_id: String,
    pub workspace_id: String,
    pub cwd: String,
    pub title: String,
    pub provider: ProviderKind,
    pub model: Option<String>,
    pub effort: Option<ReasoningEffort>,
    pub fast_mode: bool,
    pub mode: SessionMode,
    pub status: AgentStatus,
    pub parent_agent_id: Option<String>,
    pub parent_host_id: Option<String>,
    pub latest_seq: u64,
    pub timeline_epoch: u64,
    pub created_at: Rfc3339Timestamp,
    pub updated_at: Rfc3339Timestamp,
    pub archived_at: Option<Rfc3339Timestamp>,
    pub background: bool,
    pub auto_archive: bool,
    pub profile_id: Option<String>,
    pub labels: HashMap<String, String>,
    pub attention: AttentionState,
    pub last_activity_at: Option<Rfc3339Timestamp>,
    pub queued_prompts: Vec<QueuedPromptView>,
    pub read_seq: u64,
    pub is_pinned: bool,
    pub pinned_order: i64,
    pub handle: Option<String>,
    pub cluster_suggestion: Option<WorkspaceClusterSuggestion>,
    pub task_summary: Option<String>,
    pub forked_from_agent_id: Option<String>,
    pub forked_at_seq: Option<u64>,
    pub acp_agent_id: Option<String>,
}
use orbit_protocol::AttentionState;
#[uniffi::remote(Enum)]
pub enum AttentionState {
    None,
    Finished,
    Error,
    Permission,
    ManualUnread,
}
use orbit_protocol::BatchPermissionSkipReason;
#[uniffi::remote(Record)]
pub struct BatchPermissionSkipReason {
    pub agent_id: String,
    pub permission_request_id: String,
    pub reason: String,
}
use orbit_protocol::ConnectedHostStatus;
#[uniffi::remote(Enum)]
pub enum ConnectedHostStatus {
    Disabled,
    Connecting,
    Online,
    Error,
}
use orbit_protocol::ConnectedHostSummary;
#[uniffi::remote(Record)]
pub struct ConnectedHostSummary {
    pub registration_id: String,
    pub host_id: Option<String>,
    pub label: String,
    pub daemon_pubkey_b64: String,
    pub direct_endpoints: Vec<String>,
    pub relay_url: Option<String>,
    pub enabled: bool,
    pub status: ConnectedHostStatus,
    pub last_error: Option<String>,
    pub available_providers: Vec<ProviderKind>,
    pub hostname: Option<String>,
    pub daemon_version: Option<String>,
    pub updated_at: Rfc3339Timestamp,
}
use orbit_protocol::GuardianAssessment;
#[uniffi::remote(Record)]
pub struct GuardianAssessment {
    pub risk_level: GuardianRiskLevel,
    pub decision: GuardianDecision,
    pub confidence: f32,
    pub classifier_tier: String,
    pub reason: String,
    pub signals: Vec<String>,
    pub latency_ms: u64,
}
use orbit_protocol::GuardianAuditEntry;
#[uniffi::remote(Record)]
pub struct GuardianAuditEntry {
    pub id: String,
    pub agent_id: String,
    pub request_id: String,
    pub tool_name: String,
    pub target_summary: String,
    pub assessment: GuardianAssessment,
    pub decided_at_ms: u64,
}
use orbit_client_core::app_core::GuardianConfigSnapshotView;
#[uniffi::remote(Record)]
pub struct GuardianConfigSnapshotView {
    pub config: GuardianPolicyConfig,
    pub recent_audits: Vec<GuardianAuditEntry>,
}
use orbit_protocol::GuardianDecision;
#[uniffi::remote(Enum)]
pub enum GuardianDecision {
    AutoAllow,
    RequireHuman,
    AutoDeny,
}

use orbit_protocol::PermissionRuleRecord;
#[uniffi::remote(Record)]
pub struct PermissionRuleRecord {
    pub id: String,
    pub scope_kind: PermissionRuleScopeKind,
    pub project_cwd: Option<String>,
    pub provider: Option<ProviderKind>,
    pub tool_kind: PermissionRuleToolKind,
    pub pattern_kind: PermissionRulePatternKind,
    pub pattern: String,
    pub decision: PermissionRuleDecision,
    pub source: PermissionRuleSource,
    pub note: Option<String>,
    pub created_at: Rfc3339Timestamp,
    pub last_matched_at: Option<Rfc3339Timestamp>,
    pub match_count: u64,
}

use orbit_protocol::PromptBadge;
#[uniffi::remote(Record)]
pub struct PromptBadge {
    pub kind: String,
    pub label: String,
    pub path: Option<String>,
}

use orbit_protocol::QueuedPromptView;
#[uniffi::remote(Record)]
pub struct QueuedPromptView {
    pub queue_id: String,
    pub display_text: String,
    pub badges: Vec<PromptBadge>,
    pub created_at: Rfc3339Timestamp,
}

use orbit_protocol::ResolvePermissionsBatchResult;
#[uniffi::remote(Record)]
pub struct ResolvePermissionsBatchResult {
    pub resolved_count: MobileCount,
    pub skipped_count: MobileCount,
    pub skipped_reasons: Vec<BatchPermissionSkipReason>,
}

use orbit_protocol::RouterStatusSnapshot;
#[uniffi::remote(Record)]
pub struct RouterStatusSnapshot {
    pub config: PersonalRouterConfig,
    pub host_capabilities: RouterHostCapabilities,
    pub router_agent: Option<AgentSummary>,
    pub managed_workers_count: MobileCount,
    pub pending_approvals_count: MobileCount,
}

use orbit_protocol::WorkspaceClusterSuggestion;
#[uniffi::remote(Record)]
pub struct WorkspaceClusterSuggestion {
    pub target_workspace_id: String,
    pub target_workspace_title: String,
    pub confidence: f32,
    pub reason: String,
    pub auto_applied: bool,
    pub previous_workspace_id: Option<String>,
}

#[uniffi::export]
impl OrbitMobileCore {
    pub fn get_guardian_config(
        &self,
        host_id: String,
    ) -> Result<GuardianConfigSnapshotView, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.get_guardian_config_on_host(&host_id))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn update_guardian_config(
        &self,
        host_id: String,
        config: GuardianPolicyConfig,
    ) -> Result<GuardianConfigSnapshotView, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.update_guardian_config_on_host(&host_id, config))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn list_permission_rules(
        &self,
        host_id: String,
        project_cwd: Option<String>,
    ) -> Result<Vec<PermissionRuleRecord>, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .list_permission_rules_on_host(&host_id, project_cwd),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn upsert_permission_rule(
        &self,
        host_id: String,
        rule: UpsertPermissionRuleInput,
    ) -> Result<PermissionRuleRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.upsert_permission_rule_on_host(&host_id, rule))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn delete_permission_rule(
        &self,
        host_id: String,
        rule_id: String,
    ) -> Result<bool, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.delete_permission_rule_on_host(&host_id, rule_id))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn resolve_permissions_batch(
        &self,
        host_id: String,
        items: Vec<FfiPermissionRequestId>,
        decision: String,
    ) -> Result<ResolvePermissionsBatchResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.resolve_permissions_batch_on_host(
                &host_id,
                items.into_iter().map(Into::into).collect(),
                decision,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn resolve_plan_approval(
        &self,
        host_id: String,
        agent_id: String,
        permission_request_id: String,
        action: PlanApprovalAction,
        edited_plan_markdown: Option<String>,
        feedback: Option<String>,
        target_mode: Option<SessionMode>,
    ) -> Result<(), FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.resolve_plan_approval_on_host(
                &host_id,
                &agent_id,
                &permission_request_id,
                action,
                edited_plan_markdown,
                feedback,
                target_mode,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })?;
        Ok(())
    }

    pub fn git_commit(
        &self,
        host_id: String,
        cwd: String,
        message: String,
        amend: bool,
    ) -> Result<GitCommitResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_commit_on_host(&host_id, &cwd, message, amend),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_generate_commit_message(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitGeneratedCommitMessage, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_generate_commit_message_on_host(&host_id, &cwd),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stash_list(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitStashListResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_stash_list_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stash_apply(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<GitStashMutationResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_stash_apply_on_host(
                &host_id,
                &cwd,
                stash_ref,
                expected_oid,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stash_drop(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<GitStashMutationResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_stash_drop_on_host(
                &host_id,
                &cwd,
                stash_ref,
                expected_oid,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stash_pop(
        &self,
        host_id: String,
        cwd: String,
        stash_ref: String,
        expected_oid: Option<String>,
    ) -> Result<GitStashMutationResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_stash_pop_on_host(
                &host_id,
                &cwd,
                stash_ref,
                expected_oid,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_create_pr(
        &self,
        host_id: String,
        cwd: String,
        title: String,
        body: String,
        base: Option<String>,
        draft: bool,
    ) -> Result<GitCreatePrResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_create_pr_on_host(&host_id, &cwd, title, body, base, draft),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_pr_status(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitPrStatus, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_pr_status_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn git_resolve_conflict(
        &self,
        host_id: String,
        cwd: String,
        path: String,
        block_index: Option<u32>,
        resolution: GitConflictResolution,
        auto_stage: bool,
        expected_sha256: Option<String>,
    ) -> Result<GitResolveConflictResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_resolve_conflict_on_host(
                &host_id,
                &cwd,
                path,
                block_index,
                resolution,
                auto_stage,
                expected_sha256,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_rebase_status(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitRebaseStatus, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_rebase_status_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_rebase_start(
        &self,
        host_id: String,
        cwd: String,
        onto_ref: String,
    ) -> Result<GitRebaseActionResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_rebase_start_on_host(&host_id, &cwd, onto_ref),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_rebase_continue(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitRebaseActionResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_rebase_continue_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_rebase_abort(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitRebaseActionResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_rebase_abort_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_rebase_skip(
        &self,
        host_id: String,
        cwd: String,
        expected_step: Option<u32>,
    ) -> Result<GitRebaseActionResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_rebase_skip_on_host(&host_id, &cwd, expected_step),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn update_push_preferences(
        &self,
        host_id: String,
        req: UpdatePushPreferencesRequest,
    ) -> Result<FfiPushDevicesSnapshot, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.update_push_preferences_on_host(&host_id, req))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn test_push_notification(
        &self,
        host_id: String,
        device_id: Option<String>,
    ) -> Result<PushTestResultView, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .test_push_notification_on_host(&host_id, device_id),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stage_paths(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> Result<GitMutationOutcome, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_stage_files_on_host(&host_id, &cwd, paths, all),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_unstage_paths(
        &self,
        host_id: String,
        cwd: String,
        paths: Vec<String>,
        all: bool,
    ) -> Result<GitMutationOutcome, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .git_unstage_files_on_host(&host_id, &cwd, paths, all),
            )
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_stash_save(
        &self,
        host_id: String,
        cwd: String,
        message: Option<String>,
        include_untracked: bool,
    ) -> Result<GitStashMutationResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_stash_push_on_host(
                &host_id,
                &cwd,
                message,
                include_untracked,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn git_conflicts(
        &self,
        host_id: String,
        cwd: String,
    ) -> Result<GitConflictListResult, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.git_list_conflicts_on_host(&host_id, &cwd))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn start_goal(&self, req: StartGoalRunRequest) -> Result<FfiGoalRunRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.start_goal_run_on_host(req))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn create_monitor(
        &self,
        req: StartMonitorWatchRequest,
    ) -> Result<FfiMonitorWatchRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.start_monitor_watch_on_host(req))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn create_timer(
        &self,
        host_id: String,
        agent_id: String,
        duration_seconds: u64,
        prompt: String,
        condition: TimerCondition,
    ) -> Result<FfiConditionalTimerRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.create_conditional_timer_on_host(
                &host_id,
                &agent_id,
                duration_seconds,
                prompt,
                condition,
            ))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn cancel_timer(
        &self,
        host_id: String,
        timer_id: String,
    ) -> Result<FfiConditionalTimerRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(
                self.inner
                    .cancel_conditional_timer_on_host(&host_id, &timer_id),
            )
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn start_workflow(
        &self,
        req: StartDagWorkflowRequest,
    ) -> Result<FfiWorkflowRunRecord, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.start_dag_workflow_on_host(req))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn get_router_config(
        &self,
        host_id: String,
    ) -> Result<RouterStatusSnapshot, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.get_personal_router_status_on_host(&host_id))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn update_router_config(
        &self,
        host_id: String,
        config: PersonalRouterConfig,
        external_webhook_secret: Option<String>,
        activate: bool,
    ) -> Result<RouterStatusSnapshot, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.configure_personal_router_on_host(
                &host_id,
                config,
                external_webhook_secret,
                activate,
            ))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn update_mesh_peer(
        &self,
        host_id: String,
        registration_id: String,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<(), FfiOrbitError> {
        self.inner
            .update_connected_host(&host_id, &registration_id, label, enabled);
        Ok(())
    }

    pub fn search_messages_remote(
        &self,
        req: SearchMessagesRequest,
    ) -> Result<SearchMessagesResponse, FfiOrbitError> {
        Ok(self.inner.search_messages_remote_blocking(req))
    }

    pub fn discover_fleet_router_hosts(
        &self,
    ) -> Result<Vec<RouterHostCapabilities>, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.discover_fleet_router_hosts())
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn get_fleet_mesh_snapshot(
        &self,
    ) -> Result<HashMap<String, Vec<ConnectedHostSummary>>, FfiOrbitError> {
        self.inner
            .get_fleet_mesh_snapshot()
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn get_session_usage(
        &self,
        host_id: String,
        agent_id: String,
    ) -> Result<SessionUsage, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.get_session_usage(&host_id, &agent_id))
            .map_err(|detail| FfiOrbitError::Core { detail })
    }

    pub fn register_live_activity_token(
        &self,
        host_id: String,
        device_id: String,
        live_activity_token: Option<String>,
    ) -> Result<FfiPushDevicesSnapshot, FfiOrbitError> {
        self.inner
            .run_mobile_rpc(self.inner.register_live_activity_token(
                &host_id,
                device_id,
                live_activity_token,
            ))
            .map(Into::into)
            .map_err(|detail| FfiOrbitError::Core { detail })
    }
}
