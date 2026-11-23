import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "@/lib/i18n";

export type ProviderKind =
  | "antigravity"
  | "codex"
  | "claude_code"
  | "opencode"
  | "gemini_cli"
  | "cursor_agent"
  | "copilot"
  | "goose"
  | "pi"
  | "kimi"
  | "cline"
  | "mistral_vibe"
  | "augment"
  | "acp";

export interface ProviderMeta {
  readonly id: ProviderKind;
  readonly label: string;
  readonly shortLabel: string;
  readonly bin: string;
}

/** First-class native coding agent CLIs (generic ACP agents are discovered via HostAcpSection / acp_agents). */
export const PROVIDER_LIST: readonly ProviderMeta[] = [
  { id: "claude_code", label: "Claude Code", shortLabel: "Claude", bin: "claude" },
  { id: "codex", label: "Codex", shortLabel: "Codex", bin: "codex" },
  { id: "antigravity", label: "Antigravity", shortLabel: "AGY", bin: "agy" },
  { id: "opencode", label: "OpenCode", shortLabel: "OpenCode", bin: "opencode" },
  { id: "gemini_cli", label: "Gemini CLI", shortLabel: "Gemini", bin: "gemini" },
  { id: "cursor_agent", label: "Cursor Agent", shortLabel: "Cursor", bin: "cursor-agent" },
  { id: "copilot", label: "GitHub Copilot", shortLabel: "Copilot", bin: "copilot" },
  { id: "goose", label: "Goose", shortLabel: "Goose", bin: "goose" },
  { id: "pi", label: "Pi", shortLabel: "Pi", bin: "pi" },
  { id: "kimi", label: "Kimi CLI", shortLabel: "Kimi", bin: "kimi" },
  { id: "cline", label: "Cline", shortLabel: "Cline", bin: "cline" },
  { id: "mistral_vibe", label: "Mistral Vibe", shortLabel: "Vibe", bin: "vibe-acp" },
  { id: "augment", label: "Augment", shortLabel: "Augment", bin: "auggie" },
] as const;

export const ACP_PROVIDER_META: ProviderMeta = {
  id: "acp",
  label: "ACP Agent",
  shortLabel: "ACP",
  bin: "acp",
};

export const PROVIDER_META: Record<ProviderKind, ProviderMeta> = Object.fromEntries(
  [...PROVIDER_LIST, ACP_PROVIDER_META].map((p) => [p.id, p]),
) as Record<ProviderKind, ProviderMeta>;

export function formatProviderLabel(provider: ProviderKind | string): string {
  if (Object.hasOwn(PROVIDER_META, provider)) {
    return PROVIDER_META[provider as ProviderKind].label;
  }
  return provider.replace(/_/g, " ");
}

export function formatProviderShortLabel(provider: ProviderKind | string): string {
  if (Object.hasOwn(PROVIDER_META, provider)) {
    return PROVIDER_META[provider as ProviderKind].shortLabel;
  }
  return provider.replace(/_/g, " ");
}

export type AcpAgentSource = "builtin_catalog" | "remote_registry" | "custom";

export interface AcpAgentSpec {
  id: string;
  name: string;
  description: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  install_hint?: string | null;
  homepage?: string | null;
  version?: string | null;
  source: AcpAgentSource;
  enabled: boolean;
}

export interface AcpAgentAvailability {
  spec: AcpAgentSpec;
  installed: boolean;
  resolved_path?: string | null;
}

export interface AcpRegistryRefreshView {
  agents: AcpAgentAvailability[];
  warning?: string | null;
}

export interface AcpProbeResult {
  agent_id: string;
  ok: boolean;
  protocol_version?: string | null;
  server_name?: string | null;
  server_version?: string | null;
  capabilities_summary: string[];
  auth_methods: string[];
  latency_ms: number;
  error?: string | null;
}

export type McpTransportKind =
  | {
      kind: "stdio";
      command: string;
      args: string[];
    }
  | {
      kind: "streamable_http";
      url: string;
      header_keys: string[];
    };

export type McpServerScope =
  | "orbit_user"
  | "project_dot_mcp"
  | "claude_user"
  | "codex_user"
  | "antigravity_user";

export type McpWriteScope = "user" | "project";

export interface McpToolSummary {
  name: string;
  description: string;
}

export interface McpProbeResult {
  ok: boolean;
  server_name?: string | null;
  server_version?: string | null;
  protocol_version?: string | null;
  tools: McpToolSummary[];
  latency_ms: number;
  error?: string | null;
  probed_at_ms: number;
}

export interface ExternalMcpServerRecord {
  id: string;
  name: string;
  scope: McpServerScope;
  transport: McpTransportKind;
  env_keys: string[];
  masked_env: Record<string, string>;
  masked_headers?: Record<string, string>;
  enabled: boolean;
  editable: boolean;
  source_path: string;
  last_probe?: McpProbeResult | null;
}

export interface UpsertExternalMcpServerRequest {
  host_id: string;
  scope: McpWriteScope;
  cwd?: string | null;
  name: string;
  transport: McpTransportKind;
  env: Record<string, string>;
  headers: Record<string, string>;
  keep_existing_secrets: boolean;
  enabled: boolean;
}

export type ArtifactKind = "markdown" | "html" | "code" | "svg" | "mermaid" | "mcp_app";

export interface ArtifactSummary {
  artifact_id: string;
  workspace_id: string;
  agent_id?: string | null;
  title: string;
  kind: ArtifactKind;
  language?: string | null;
  mime_type?: string | null;
  resource_uri?: string | null;
  version: number;
  byte_len: number;
  created_at: string;
  updated_at: string;
}

export interface ArtifactRevisionMeta {
  version: number;
  created_at: string;
  summary_note?: string | null;
  byte_len: number;
}

export interface ArtifactDetail {
  summary: ArtifactSummary;
  content: string;
  revisions: ArtifactRevisionMeta[];
}

export interface UpsertArtifactRequest {
  host_id: string;
  artifact_id?: string | null;
  workspace_id: string;
  agent_id?: string | null;
  title: string;
  kind: ArtifactKind;
  language?: string | null;
  mime_type?: string | null;
  resource_uri?: string | null;
  content: string;
  summary_note?: string | null;
}
export type ReasoningEffort =
  | "off"
  | "low"
  | "medium"
  | "high"
  | "xhigh"
  | "max"
  | "ultra";
export type SessionMode =
  | "default"
  | "auto"
  | "plan"
  | "accept_edits"
  | "full_access"
  | { custom: string };

export type GuardianRiskLevel = "low" | "medium" | "high" | "critical";

export type GuardianDecision = "auto_allow" | "require_human" | "auto_deny";

export interface GuardianPolicyConfig {
  enabled: boolean;
  auto_approve_max_risk: GuardianRiskLevel;
  use_ai_classifier_for_medium: boolean;
  classifier_timeout_ms: number;
  allow_read_only_tools: boolean;
  allow_workspace_tests: boolean;
  blocked_command_patterns: string[];
  trusted_command_prefixes: string[];
}

export interface GuardianAssessment {
  risk_level: GuardianRiskLevel;
  decision: GuardianDecision;
  confidence: number;
  classifier_tier: "deterministic" | "ai_rubric" | "fail_closed" | string;
  reason: string;
  signals: string[];
  latency_ms: number;
}

export interface GuardianAuditEntry {
  id: string;
  agent_id: string;
  request_id: string;
  tool_name: string;
  target_summary: string;
  assessment: GuardianAssessment;
  decided_at_ms: number;
}

export interface GuardianConfigSnapshotView {
  config: GuardianPolicyConfig;
  recent_audits: GuardianAuditEntry[];
}

export type ImPlatform = "lark" | "telegram" | "slack" | "discord";
export type ImConnectionStatus = "disconnected" | "connecting" | "connected" | "error";

export interface ImChannelRecord {
  channel_id: string;
  name: string;
  platform: ImPlatform;
  enabled: boolean;
  status: ImConnectionStatus;
  status_message?: string | null;
  webhook_path: string;
  app_id?: string | null;
  has_primary_secret: boolean;
  has_signing_secret: boolean;
  masked_primary_secret?: string | null;
  masked_signing_secret?: string | null;
  allowed_chat_ids: string[];
  allowed_user_ids: string[];
  default_workspace_cwd: string;
  default_provider: ProviderKind;
  default_mode: SessionMode;
  forward_approvals: boolean;
  message_count: number;
  last_activity_at_ms?: number | null;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface ImThreadBinding {
  channel_id: string;
  external_chat_id: string;
  external_thread_id: string;
  agent_id: string;
  updated_at_ms: number;
}

export interface ImChannelsSnapshotView {
  channels: ImChannelRecord[];
  bindings: ImThreadBinding[];
  base_webhook_url: string;
}

export interface ImChannelTestResultView {
  ok: boolean;
  detail: string;
}

export type PushPlatform = "apns" | "fcm" | "harmony_push_kit";

export type PushEventKind =
  | "permission_required"
  | "turn_completed"
  | "agent_error"
  | "goal_finished"
  | "timer_fired";

export interface PushPreferences {
  enabled: boolean;
  notify_permission_required: boolean;
  notify_turn_completed: boolean;
  notify_agent_error: boolean;
  notify_goal_finished: boolean;
  notify_timer_fired: boolean;
  include_message_preview: boolean;
  quiet_hours_enabled: boolean;
  quiet_start_hour: number;
  quiet_end_hour: number;
  push_gateway_url?: string | null;
  has_gateway_auth_token: boolean;
}

export interface PushDeviceRecord {
  device_id: string;
  platform: PushPlatform;
  device_name: string;
  masked_token: string;
  sandbox: boolean;
  enabled: boolean;
  last_delivered_at_ms?: number | null;
  last_error?: string | null;
  registered_at_ms: number;
  updated_at_ms: number;
}

export interface PushDevicesSnapshotView {
  devices: PushDeviceRecord[];
  preferences: PushPreferences;
}

export interface PushTestResultView {
  attempted: number;
  delivered: number;
  detail: string;
}

export interface RegisterPushDeviceRequest {
  device_id: string;
  platform: PushPlatform;
  device_name: string;
  device_token: string;
  sandbox: boolean;
}

export interface UpdatePushPreferencesRequest {
  preferences: PushPreferences;
  gateway_auth_token?: string | null;
  keep_existing_gateway_token: boolean;
}

export interface UpsertImChannelRequest {
  channel_id?: string | null;
  name: string;
  platform: ImPlatform;
  enabled: boolean;
  app_id?: string | null;
  primary_secret?: string | null;
  signing_secret?: string | null;
  keep_existing_secrets: boolean;
  allowed_chat_ids: string[];
  allowed_user_ids: string[];
  default_workspace_cwd: string;
  default_provider: ProviderKind;
  default_mode: SessionMode;
  forward_approvals: boolean;
}

export type HostConnectionState =
  | "disconnected"
  | "connecting_direct"
  | "connecting_relay"
  | "online_direct"
  | "online_relay";

export type ExternalEditorKind =
  | "cursor"
  | "zed"
  | "vs_code"
  | "terminal"
  | "file_manager";

export interface ExternalEditorInfo {
  kind: ExternalEditorKind;
  label: string;
  installed: boolean;
}

export type ProviderAuthStatus = "authenticated" | "unauthenticated" | "unknown";

export interface ProviderReadinessInfo {
  kind: ProviderKind;
  display_name: string;
  installed: boolean;
  executable_path: string | null;
  auth_status: ProviderAuthStatus;
  install_hint: string;
  login_hint: string;
}

export type HostRouteMode = "auto" | "ssh_tunnel" | "direct_only" | "relay_only";

export interface ReleaseUpdateInfo {
  current_version: string;
  tag_name: string;
  latest_version: string;
  html_url: string;
  notes: string;
  has_newer_version: boolean;
  macos_dmg_url: string | null;
  linux_arm64_deb_url: string | null;
  linux_x64_deb_url: string | null;
  windows_nsis_url?: string | null;
  windows_msi_url?: string | null;
}

export interface AppUpdateProgress {
  progress: number;
  status: string;
  done: boolean;
}

export interface LocalDaemonSyncResult {
  status: "updated" | "restarted" | "skipped_dev" | string;
  daemon_version: string | null;
  source: "bundled" | "github" | "existing" | "dev" | string;
}

export type CloudSyncState = "signed_out" | "locked" | "ready" | "syncing" | "error";

export type CloudSyncDevicePollStatus =
  | "pending"
  | "slow_down"
  | "authorized"
  | "expired"
  | "denied";

export interface CloudSyncStatusView {
  state: CloudSyncState;
  provider: string;
  github_oauth_configured: boolean;
  github_client_id_override: string | null;
  account_login: string | null;
  account_name: string | null;
  account_avatar_url: string | null;
  gist_id: string | null;
  gist_html_url: string | null;
  remote_vault_exists: boolean;
  auto_sync_enabled: boolean;
  last_synced_at_ms: number | null;
  last_vault_revision: number;
  synced_host_count: number;
  synced_project_count: number;
  last_error: string | null;
}

export interface GitHubDeviceFlowStartView {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in_secs: number;
  interval_secs: number;
}

export interface CloudSyncDevicePollView {
  status: CloudSyncDevicePollStatus;
  interval_secs: number | null;
  error: string | null;
  sync_status: CloudSyncStatusView | null;
}

export interface CloudSyncResultView {
  hosts_added: number;
  hosts_updated: number;
  hosts_removed: number;
  projects_added: number;
  projects_updated: number;
  projects_removed: number;
  preferences_updated: number;
  pushed_remote: boolean;
  vault_revision: number;
  synced_at_ms: number;
  gist_html_url: string | null;
}

export function normalizeSemver(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim().replace(/^v/i, "");
  return trimmed.length > 0 ? trimmed : null;
}

export function compareSemver(a: string | null | undefined, b: string | null | undefined): number {
  const parse = (raw: string | null | undefined): [number, number, number] => {
    const clean = (normalizeSemver(raw) ?? "0.0.0").split(/[+-]/)[0] ?? "0.0.0";
    const parts = clean.split(".").map((n) => Number.parseInt(n, 10) || 0);
    return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
  };
  const pa = parse(a);
  const pb = parse(b);
  for (let i = 0; i < 3; i += 1) {
    if (pa[i] !== pb[i]) return pa[i] > pb[i] ? 1 : -1;
  }
  return 0;
}

export interface ClientProjectView {
  project_id: string;
  host_id: string;
  name: string;
  directories: string[];
  updated_at_ms: number;
}

export interface DirectoryEntry {
  name: string;
  path: string;
  is_git_repo: boolean;
}

export interface DirectoryListingView {
  current_path: string;
  parent_path: string | null;
  entries: DirectoryEntry[];
}

export interface ModelOption {
  id: string;
  label: string;
  description: string | null;
  is_default: boolean;
}

export interface HostCardView {
  host_id: string;
  display_name: string;
  hostname: string;
  custom_name: string | null;
  ssh_alias: string | null;
  route_mode: HostRouteMode;
  remote_daemon_port: number;
  custom_direct_endpoint: string | null;
  via_ssh_tunnel: boolean;
  last_ping_ms: number | null;
  daemon_version: string | null;
  daemon_protocol_version?: number | null;
  client_protocol_version?: number;
  os_arch: string | null;
  daemon_pubkey_b64: string;
  direct_endpoints: string[];
  relay_url: string | null;
  connection_state: HostConnectionState;
  last_error: string | null;
  available_providers: ProviderKind[];
  workspace_count: number;
  agent_count: number;
  pending_approval_count: number;
  supports_server_queue?: boolean;
}

export interface FleetOverviewView {
  revision: number;
  total_pending_approvals: number;
  hosts: HostCardView[];
  projects?: ClientProjectView[];
  project_order?: string[];
}

export interface ConversationSearchHit {
  host_id: string;
  host_name: string;
  agent_id: string;
  seq: number;
  role: string;
  title: string;
  project_name: string;
  snippet: string;
  updated_at_rfc3339: string;
}

export type ExportFormat = "markdown" | "json" | "html";

export interface ExportOptions {
  format: ExportFormat;
  include_thinking: boolean;
  include_tool_calls: boolean;
  include_timestamps: boolean;
}

export interface ExportedTranscript {
  suggested_filename: string;
  mime_type: string;
  content: string;
  turn_count: number;
  byte_len: number;
}

export interface ExtractedCodeBlock {
  index: number;
  turn_index: number;
  language: string;
  preview: string;
  line_count: number;
  code: string;
}

export type ActiveTurnBehavior = "steer" | "queue" | "interrupt";
export type AttentionState = "none" | "finished" | "error" | "permission" | "manual_unread";
export type WorkspaceIsolation = "local" | "worktree";
export type WorktreeCreateMode = "branch_off" | "checkout_branch" | "checkout_pr";
export type WorkspaceSetupStatus = "none" | "idle" | "running" | "succeeded" | "failed";
export type AgentMessageKind = "direct_message" | "finish_notification" | "heartbeat";
export type ProviderSubagentStatus = "running" | "completed" | "failed" | "stopped" | "canceled";
export type ScheduleKind = "schedule" | "heartbeat";
export type ScheduleStatus = "active" | "paused" | "completed";
export type ScheduleRunStatus = "running" | "succeeded" | "failed" | "skipped";
export type ConnectedHostStatus = "disabled" | "connecting" | "online" | "error";
export type WorkspaceScriptKind = "command" | "setup" | "teardown" | "service" | "task";
export type WorkspaceScriptStatus = "stopped" | "running" | "failed";

export interface NestedGitRepo {
  path: string;
  relative_path: string;
  name: string;
  git_branch?: string | null;
}

export interface GitReposInspectionView {
  path: string;
  is_git_repo: boolean;
  git_branch?: string | null;
  nested_git_repos: NestedGitRepo[];
}

export interface HostProjectEntry {
  project_id: string;
  name: string;
  directories: string[];
  sort_order: number;
  hidden: boolean;
  updated_at_ms: number;
}

export interface HostSyncState {
  projects: HostProjectEntry[];
  project_order: string[];
  project_sort_mode?: string | null;
  updated_at_ms: number;
}

export interface WorkspaceClusterSuggestion {
  target_workspace_id: string;
  target_workspace_title: string;
  confidence: number;
  reason: string;
  auto_applied: boolean;
  previous_workspace_id?: string | null;
}

export interface WorkspaceBoardSnapshot {
  workspace_id: string;
  content: string;
  revision: number;
  updated_by?: string | null;
  updated_at: string;
  board_path?: string | null;
}

export type WorkspaceEventKind =
  | "peer_joined"
  | "peer_finished"
  | "board_updated"
  | "peer_message"
  | "file_edited"
  | "conflict_warning"
  | "workspace_merged";

export interface WorkspaceEventRecord {
  seq: number;
  workspace_id: string;
  agent_id?: string | null;
  agent_handle?: string | null;
  kind: WorkspaceEventKind;
  summary: string;
  file_path?: string | null;
  created_at: string;
}

export type WorkspaceOrganizePhase = "running" | "completed" | "failed";

export interface WorkspaceOrganizeEvent {
  request_id?: string | null;
  project_root: string;
  phase: WorkspaceOrganizePhase;
  message: string;
  created_workspaces: number;
  grouped_agents: number;
  summarized_agents: number;
}

const GENERIC_HARNESS_HANDLES = new Set([
  "peer",
  "lead",
  "agy",
  "codex",
  "claude",
  "claude-code",
  "claude_code",
  "antigravity",
  "opencode",
  "open_code",
  "open-code",
  "gemini",
  "gemini_cli",
  "gemini-cli",
  "cursor",
  "cursor_agent",
  "cursor-agent",
  "copilot",
  "goose",
  "pi",
  "kimi",
  "kimi_cli",
  "kimi-cli",
  "cline",
  "vibe",
  "mistral_vibe",
  "mistral-vibe",
  "augment",
  "auggie",
  "agent",
  "assistant",
  "chat",
  "session",
  "task",
  "default",
  "worker",
]);

/** Returns true when a handle is just a bare harness/placeholder name (e.g. `agy`, `claude-2`, `peer`). */
export function isGenericHarnessHandle(handle: string | null | undefined): boolean {
  if (!handle) return true;
  const norm = handle.trim().replace(/^@+/, "").toLowerCase();
  if (!norm) return true;
  if (GENERIC_HARNESS_HANDLES.has(norm)) return true;
  const match = /^(.+)-\d+$/.exec(norm);
  return Boolean(match && GENERIC_HARNESS_HANDLES.has(match[1]));
}

/**
 * Derive a clean, human-meaningful top-bar tab label for an agent.
 * Never falls back to bare harness names (`agy`/`claude`/`codex`/`peer`) when a thread title exists.
 */
export function semanticAgentTabLabel(
  agent: Pick<AgentCardView, "handle" | "title" | "provider">,
  customTitle?: string | null,
): string {
  const rawHandle = agent.handle?.trim().replace(/^@+/, "") ?? "";
  if (rawHandle && !isGenericHarnessHandle(rawHandle)) {
    return rawHandle;
  }
  const titleSource = (customTitle?.trim() || agent.title?.trim() || "").replace(/\s+/g, " ");
  if (titleSource && !isGenericHarnessHandle(titleSource)) {
    return titleSource.length > 18 ? `${titleSource.slice(0, 17)}…` : titleSource;
  }
  return t("common.newSession");
}

export interface SpawnWorkspacePeerRequest {
  host_id: string;
  workspace_id: string;
  cwd: string;
  extra_dirs?: string[];
  provider: ProviderKind;
  model?: string | null;
  effort?: ReasoningEffort | null;
  fast_mode?: boolean;
  mode: SessionMode;
  initial_prompt: string;
  worktree_branch?: string | null;
  handle?: string | null;
  profile_id?: string | null;
}

export interface PermissionCheckpointRef {
  child_agent_id: string;
  child_host_id?: string | null;
  permission_request_id: string;
  title: string;
  allowed_decisions: string[];
}

export interface PendingPermissionSummary {
  agent_id: string;
  agent_title: string;
  request_id: string;
  title: string;
  detail: ToolCallDetail;
  allowed_decisions: string[];
  guardian_assessment?: GuardianAssessment | null;
  created_at: string;
}

export interface McpToolPolicy {
  enabled: boolean;
  disabled_tools: string[];
  enabled_packs?: string[] | null;
  disabled_packs?: string[];
}

export interface AgentProfile {
  profile_id: string;
  name: string;
  provider: ProviderKind;
  acp_agent_id?: string | null;
  model: string | null;
  effort: ReasoningEffort | null;
  fast_mode?: boolean;
  mode: SessionMode;
  when_to_use: string;
  system_prompt_append: string | null;
  mcp_policy: McpToolPolicy;
  created_at: string;
  updated_at: string;
}

export interface OrchestrationConfig {
  mcp_inject_enabled: boolean;
  idle_cleanup_enabled: boolean;
  idle_timeout_secs: number;
  resume_on_startup: boolean;
  resume_prompt: string;
  installed_skills: string[];
  auto_cluster_enabled?: boolean;
  keep_awake_while_running?: boolean;
}

export interface KeepAwakeStatus {
  enabled: boolean;
  active: boolean;
  active_agents: number;
  backend: string;
}

export interface OrchestrationSkillInfo {
  id: string;
  name: string;
  slash_command: string;
  description: string;
  installed: boolean;
  installed_targets: string[];
  outdated: boolean;
}

export type SkillSource =
  | "orbit_user"
  | "orbit_project"
  | "claude_user"
  | "claude_project"
  | "codex_user"
  | "antigravity_user"
  | "builtin";

export type SkillWriteScope = "user" | "project";

export interface ManagedSkillRecord {
  id: string;
  slug: string;
  name: string;
  description: string;
  source: SkillSource;
  path: string;
  enabled: boolean;
  editable: boolean;
  triggers: string[];
  allowed_tools: string[];
  has_scripts: boolean;
  has_references: boolean;
  approx_tokens: number;
  updated_at_ms: number;
}

export interface ManagedSkillContentView {
  skill_id: string;
  content: string;
  etag: string;
}

export interface ScheduleNewAgentConfig {
  cwd: string;
  workspace_id?: string | null;
  provider: ProviderKind;
  model?: string | null;
  effort?: ReasoningEffort | null;
  fast_mode?: boolean;
  profile_id?: string | null;
  mode: SessionMode;
  worktree: boolean;
  base_ref?: string | null;
  background?: boolean;
  archive_on_finish: boolean;
}

export type ScheduleTarget =
  | { type: "new_agent" } & ScheduleNewAgentConfig
  | { type: "existing_agent"; agent_id: string };

export interface ScheduleSpec {
  schedule_id: string;
  kind: ScheduleKind;
  name: string;
  cron_expr: string;
  timezone: string | null;
  prompt: string;
  status: ScheduleStatus;
  target: ScheduleTarget;
  max_runs: number | null;
  run_count: number;
  expires_at: string | null;
  last_run_at: string | null;
  next_run_at: string | null;
  created_by_agent: string | null;
  created_at: string;
  updated_at: string;
}

export interface ScheduleRunRecord {
  run_id: string;
  schedule_id: string;
  agent_id: string | null;
  workspace_id: string | null;
  status: ScheduleRunStatus;
  summary: string | null;
  error: string | null;
  started_at: string;
  finished_at: string | null;
}

export interface ConnectedHostSummary {
  registration_id: string;
  host_id: string | null;
  label: string;
  daemon_pubkey_b64: string;
  direct_endpoints: string[];
  relay_url: string | null;
  enabled: boolean;
  status: ConnectedHostStatus;
  last_error: string | null;
  hostname: string | null;
  available_providers: ProviderKind[];
  updated_at: string;
}

export interface WorkspaceScriptInfo {
  workspace_id: string;
  name: string;
  command: string;
  kind: WorkspaceScriptKind;
  status: WorkspaceScriptStatus;
  terminal_id: string | null;
  port: number | null;
  proxy_url: string | null;
  last_error?: string | null;
}

export interface ProviderSubagentDescriptor {
  parent_agent_id: string;
  subagent_id: string;
  parent_subagent_id: string | null;
  title: string;
  subtitle: string | null;
  status: ProviderSubagentStatus;
  created_at: string;
  updated_at: string;
}

export interface ProviderSubagentEvent {
  seq: number;
  parent_agent_id: string;
  subagent_id: string;
  timestamp: string;
  payload: unknown;
}

export interface WorkspaceSummary {
  workspace_id: string;
  title: string;
  cwd: string;
  git_branch: string | null;
  is_worktree: boolean;
  base_ref?: string | null;
  parent_workspace_id?: string | null;
  source_cwd?: string | null;
  setup_status?: WorkspaceSetupStatus;
  background?: boolean;
  archived_at?: string | null;
  mission_goal?: string | null;
  project_root?: string | null;
  board_revision?: number;
  title_locked?: boolean;
  auto_clustered?: boolean;
  is_curated?: boolean;
  updated_at: string;
}

export type WorktreeHandoffDirection = "worktree_to_local" | "local_to_worktree";
export type WorktreeHandoffMode = "apply" | "move";

export interface HandoffFileChange {
  path: string;
  status: string;
  insertions: number;
  deletions: number;
  is_untracked: boolean;
  has_conflict: boolean;
}

export interface WorktreeHandoffPreview {
  direction: WorktreeHandoffDirection;
  source_cwd: string;
  target_cwd: string;
  source_branch?: string | null;
  target_branch?: string | null;
  commit_count: number;
  files: HandoffFileChange[];
  total_insertions: number;
  total_deletions: number;
  target_dirty: boolean;
  conflicts: string[];
  can_apply_cleanly: boolean;
  warning?: string | null;
}

export interface WorktreeHandoffOutcome {
  direction: WorktreeHandoffDirection;
  mode: WorktreeHandoffMode;
  applied_files: number;
  backup_ref?: string | null;
  target_workspace?: WorkspaceSummary | null;
  target_cwd: string;
  had_conflicts: boolean;
  conflict_files: string[];
  summary: string;
}

export interface PreviewWorktreeHandoffRequest {
  host_id: string;
  workspace_id: string;
  direction: WorktreeHandoffDirection;
  target_workspace_id?: string | null;
  include_committed: boolean;
}

export interface ExecuteWorktreeHandoffRequest {
  host_id: string;
  workspace_id: string;
  direction: WorktreeHandoffDirection;
  mode: WorktreeHandoffMode;
  target_workspace_id?: string | null;
  include_committed: boolean;
  branch?: string | null;
  allow_three_way_conflicts: boolean;
}

export interface QueuedPromptView {
  queue_id: string;
  display_text: string;
  badges?: PromptBadge[];
  created_at: string;
}

export interface AgentCardView {
  agent_id: string;
  workspace_id: string;
  cwd: string;
  title: string;
  provider: ProviderKind;
  provider_label: string;
  acp_agent_id?: string | null;
  model: string | null;
  effort: ReasoningEffort | null;
  fast_mode?: boolean;
  mode: SessionMode;
  status_label: string;
  is_running: boolean;
  has_background_tasks?: boolean;
  has_running_subagents?: boolean;
  is_awaiting_permission: boolean;
  parent_agent_id: string | null;
  parent_host_id?: string | null;
  profile_id?: string | null;
  background?: boolean;
  auto_archive?: boolean;
  attention?: AttentionState;
  archived_at_rfc3339?: string | null;
  labels?: Record<string, string>;
  latest_seq: number;
  read_seq?: number;
  handle?: string | null;
  cluster_suggestion?: WorkspaceClusterSuggestion | null;
  worktree_branch?: string | null;
  task_summary?: string | null;
  forked_from_agent_id?: string | null;
  forked_at_seq?: number | null;
  updated_at_rfc3339: string;
  is_pinned: boolean;
  seen_seq: number;
  is_unread: boolean;
  queued_prompts?: QueuedPromptView[];
}

/**
 * Returns true when an agent's foreground turn has finished and it is only keeping track of
 * asynchronous background tasks (e.g. a dev server or long-running build).
 * Includes a legacy fallback for older remote daemons that don't yet emit `has_background_tasks`.
 */
export function isAgentBackgroundWaiting(
  agent: Pick<AgentCardView, "has_background_tasks" | "status_label"> | null | undefined,
): boolean {
  if (!agent) return false;
  if (typeof agent.has_background_tasks === "boolean") {
    return agent.has_background_tasks;
  }
  const lbl = agent.status_label.trim();
  return /^running\s*\(.*background task/i.test(lbl) || /^background task$/i.test(lbl);
}

/**
 * Returns true ONLY when a foreground turn or active autonomous wakeup is actively executing.
 * Background-only waiting states never block sending new user prompts or draining prompt queues.
 */
export function isAgentForegroundRunning(
  agent:
    | Pick<AgentCardView, "is_running" | "has_background_tasks" | "status_label">
    | null
    | undefined,
): boolean {
  if (!agent) return false;
  return Boolean(agent.is_running) && !isAgentBackgroundWaiting(agent);
}

export interface QuotaWindow {
  label: string;
  used_percent: number;
  resets_at: string | null;
}

export interface ProviderQuotaSnapshot {
  provider: ProviderKind;
  account_label: string | null;
  windows: QuotaWindow[];
  credits_remaining: number | null;
  error: string | null;
  updated_at: string;
}

export interface TerminalSummary {
  terminal_id: string;
  cwd: string;
  title: string;
  cols: number;
  rows: number;
  created_at: string;
}

export interface DiscoveredProject {
  path: string;
  name: string;
  git_branch: string | null;
  is_git_repo: boolean;
  nested_git_repos?: NestedGitRepo[];
  last_modified_unix: number;
}

export interface ThinkingOptionSpec {
  effort: ReasoningEffort;
  label: string;
  description?: string | null;
  is_default?: boolean;
}

export interface ProviderModelSpec {
  id: string;
  label: string;
  description: string;
  is_default: boolean;
  default_effort: ReasoningEffort | null;
  thinking_options: ThinkingOptionSpec[];
  supports_fast_mode?: boolean;
}

export interface ProviderModelCatalog {
  provider: ProviderKind;
  models: ProviderModelSpec[];
  configured_default_model: string | null;
  configured_default_effort: ReasoningEffort | null;
}

export type TimerCondition =
  | { kind: "never" }
  | { kind: "any_message" }
  | { kind: "sender"; sender_id: string }
  | { kind: "all_subagents_settled" }
  | { kind: "all_peers_settled" }
  | { kind: "file_changed"; path: string };

export type ConditionalTimerState =
  | "armed"
  | "fired"
  | "canceled_early"
  | "canceled_manual";

export interface ConditionalTimerRecord {
  timer_id: string;
  agent_id: string;
  workspace_id: string;
  prompt: string;
  duration_seconds: number;
  condition: TimerCondition;
  state: ConditionalTimerState;
  created_at_ms: number;
  fire_at_ms: number;
  resolved_at_ms?: number | null;
  resolved_reason?: string | null;
}

export function formatTimerConditionLabel(condition: TimerCondition): string {
  switch (condition.kind) {
    case "never":
      return "fixed timer";
    case "any_message":
      return "until any message";
    case "sender":
      return `until ${condition.sender_id}`;
    case "all_subagents_settled":
      return "until subagents settle";
    case "all_peers_settled":
      return "until workspace peers settle";
    case "file_changed": {
      const leaf = condition.path.split(/[\\/]/).pop() || condition.path;
      return `until ${leaf} changes`;
    }
  }
}

export function formatTimerCountdown(fireAtMs: number, nowMs: number = Date.now()): string {
  const remainingSec = Math.max(0, Math.ceil((fireAtMs - nowMs) / 1000));
  const hrs = Math.floor(remainingSec / 3600);
  const mins = Math.floor((remainingSec % 3600) / 60);
  const secs = remainingSec % 60;
  const mm = String(mins).padStart(2, "0");
  const ss = String(secs).padStart(2, "0");
  if (hrs > 0) {
    return `${String(hrs).padStart(2, "0")}:${mm}:${ss}`;
  }
  return `${mm}:${ss}`;
}

export interface HostDetailView {
  revision: number;
  host: HostCardView;
  discovered_projects: DiscoveredProject[];
  directory_listing: DirectoryListingView | null;
  models_by_provider: Partial<Record<ProviderKind, ModelOption[]>>;
  workspaces: WorkspaceSummary[];
  agents: AgentCardView[];
  last_created_agent_id: string | null;
  replaced_optimistic_agents: Record<string, string>;
  quotas: ProviderQuotaSnapshot[];
  provider_catalogs?: ProviderModelCatalog[];
  terminals: TerminalSummary[];
  active_terminal_id: string | null;
  active_terminal_slot: number;
  profiles?: AgentProfile[];
  schedules?: ScheduleSpec[];
  conditional_timers?: ConditionalTimerRecord[];
  connected_hosts?: ConnectedHostSummary[];
  orchestration_config?: OrchestrationConfig;
  orchestration_skills?: OrchestrationSkillInfo[];
  managed_skills?: ManagedSkillRecord[];
  keep_awake_status?: KeepAwakeStatus | null;
  workspace_scripts?: Record<string, WorkspaceScriptInfo[]>;
  pending_permissions?: PendingPermissionSummary[];
  provider_subagents?: Record<string, ProviderSubagentDescriptor[]>;
  git_repos_by_cwd?: Record<string, GitReposInspectionView>;
  workspace_boards?: Record<string, WorkspaceBoardSnapshot>;
  workspace_events?: Record<string, WorkspaceEventRecord[]>;
  workspace_organize_status?: Record<string, WorkspaceOrganizeEvent>;
  host_sync_state?: HostSyncState | null;
  acp_agents?: AcpAgentAvailability[];
  artifacts?: ArtifactSummary[];
  goal_runs?: GoalRunRecord[];
  monitor_watches?: MonitorWatchRecord[];
  workflow_runs?: WorkflowRunRecord[];
}

export type WorkflowKind = "batch_csv" | "dag" | "best_of_n";

export type WorkflowRunStatus =
  | "pending"
  | "running"
  | "succeeded"
  | "partial_failure"
  | "failed"
  | "canceled";

export type WorkflowStepStatus =
  | "pending"
  | "running"
  | "verifying"
  | "succeeded"
  | "failed"
  | "canceled"
  | "skipped";

export interface DagStepSpec {
  step_id: string;
  name: string;
  depends_on: string[];
  prompt_template: string;
  provider?: ProviderKind | null;
  model?: string | null;
  isolated_worktree: boolean;
  retry_count: number;
  timeout_secs: number;
}

export interface BestOfNCandidateSpec {
  label: string;
  provider: ProviderKind;
  model?: string | null;
  effort?: string | null;
}

export interface WorkflowStepRecord {
  run_id: string;
  step_id: string;
  name: string;
  status: WorkflowStepStatus;
  depends_on: string[];
  agent_id?: string | null;
  worktree_cwd?: string | null;
  provider: ProviderKind;
  model?: string | null;
  output_preview?: string | null;
  diff_additions: number;
  diff_deletions: number;
  diff_files: number;
  verify_passed?: boolean | null;
  verify_output?: string | null;
  error?: string | null;
  is_winner: boolean;
  started_at_ms?: number | null;
  finished_at_ms?: number | null;
}

export interface WorkflowRunRecord {
  run_id: string;
  name: string;
  kind: WorkflowKind;
  workspace_cwd: string;
  owner_agent_id?: string | null;
  status: WorkflowRunStatus;
  max_concurrency: number;
  verify_command?: string | null;
  winner_step_id?: string | null;
  steps: WorkflowStepRecord[];
  created_at_ms: number;
  updated_at_ms: number;
  finished_at_ms?: number | null;
}

export type GoalStatus =
  | "running"
  | "verifying"
  | "paused"
  | "succeeded"
  | "failed"
  | "canceled";

export type GoalControlAction = "pause" | "resume" | "cancel" | "verify_now";

export interface GoalCriterion {
  id: string;
  label: string;
  passed: boolean;
}

export interface GoalRunRecord {
  goal_id: string;
  agent_id: string;
  workspace_id: string;
  objective: string;
  criteria: GoalCriterion[];
  verify_command?: string | null;
  max_iterations: number;
  current_iteration: number;
  max_duration_secs: number;
  status: GoalStatus;
  last_verify_exit_code?: number | null;
  last_verify_output?: string | null;
  consecutive_identical_failures: number;
  started_at_ms: number;
  updated_at_ms: number;
  finished_at_ms?: number | null;
}

export type MonitorKind =
  | {
      kind: "command_stream";
      command: string;
      args: string[];
      pattern?: string | null;
    }
  | {
      kind: "log_file_tail";
      path: string;
      pattern: string;
    }
  | {
      kind: "http_health";
      url: string;
      expect_status: number;
      interval_secs: number;
    };

export type MonitorStatus = "active" | "paused" | "triggered" | "stopped" | "error";

export interface MonitorWatchRecord {
  monitor_id: string;
  agent_id: string;
  name: string;
  kind: MonitorKind;
  status: MonitorStatus;
  match_count: number;
  last_event_preview?: string | null;
  last_triggered_at_ms?: number | null;
  expires_at_ms?: number | null;
  created_at_ms: number;
}

export interface BrowserPolicyConfig {
  enabled: boolean;
  headless: boolean;
  allow_localhost: boolean;
  allowed_domains: string[];
  blocked_domains: string[];
  block_password_inputs: boolean;
  max_sessions: number;
}

export interface BrowserElementRef {
  ref_id: string;
  role: string;
  name: string;
  tag: string;
  input_type?: string | null;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface BrowserActionAudit {
  id: string;
  action: string;
  target: string;
  ok: boolean;
  detail: string;
  timestamp_ms: number;
}

export interface BrowserSessionSummary {
  session_id: string;
  agent_id?: string | null;
  url: string;
  title: string;
  viewport_width: number;
  viewport_height: number;
  headless: boolean;
  element_count: number;
  console_errors: string[];
  recent_actions: BrowserActionAudit[];
  last_screenshot_base64?: string | null;
  updated_at_ms: number;
}

export type BrowserActionRequest =
  | { kind: "navigate"; url: string }
  | { kind: "snapshot" }
  | { kind: "screenshot"; full_page?: boolean }
  | { kind: "click"; ref_id?: string | null; x?: number | null; y?: number | null }
  | { kind: "type"; ref_id?: string | null; text: string; submit?: boolean }
  | { kind: "scroll"; delta_x: number; delta_y: number }
  | { kind: "back" }
  | { kind: "reload" };

export interface BrowserSessionsSnapshotView {
  host_id: string;
  sessions: BrowserSessionSummary[];
  policy: BrowserPolicyConfig;
  chrome_binary?: string | null;
}

export interface BrowserActionResponseView {
  host_id: string;
  session: BrowserSessionSummary;
  elements: BrowserElementRef[];
  screenshot_base64?: string | null;
  text_excerpt?: string | null;
  captcha_detected?: boolean;
}

export interface InlineSpanView {
  text: string;
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  inline_code: boolean;
  link_url: string | null;
  math_ml?: string | null;
}

export interface HighlightedTokenView {
  text: string;
  fg_hex: string;
  light_fg_hex?: string;
  bold: boolean;
  italic: boolean;
}

export interface HighlightedLineView {
  line_number: number;
  tokens: HighlightedTokenView[];
}

export type TableAlignmentView = "none" | "left" | "center" | "right";

export interface TableCellView {
  spans: InlineSpanView[];
}

export interface TableRowView {
  cells: TableCellView[];
}

export type RichBlockView =
  | { kind: "heading"; level: number; spans: InlineSpanView[] }
  | { kind: "paragraph"; spans: InlineSpanView[] }
  | {
      kind: "code_block";
      language: string;
      raw_code: string;
      lines: HighlightedLineView[];
    }
  | {
      kind: "list_item";
      ordered: boolean;
      index: number;
      checked: boolean | null;
      spans: InlineSpanView[];
    }
  | { kind: "block_quote"; spans: InlineSpanView[] }
  | {
      kind: "table";
      alignments: TableAlignmentView[];
      headers: TableCellView[];
      rows: TableRowView[];
    }
  | { kind: "math_block"; tex: string; math_ml: string }
  | { kind: "horizontal_rule" };

export type DiffLineKind = "meta" | "hunk_header" | "context" | "added" | "removed";

export interface InlineDiffSpan {
  start: number;
  end: number;
}

export interface DiffLineView {
  kind: DiffLineKind;
  old_line: number | null;
  new_line: number | null;
  content: string;
  tokens?: HighlightedTokenView[];
  inline_changes?: InlineDiffSpan[];
}

export interface DiffFileView {
  path: string;
  additions: number;
  deletions: number;
  lines: DiffLineView[];
}

export type ToolCallDetail =
  | {
      kind: "shell";
      command: string;
      cwd: string | null;
      exit_code: number | null;
      output_preview: string | null;
    }
  | {
      kind: "file_read";
      path: string;
      start_line: number | null;
      end_line: number | null;
    }
  | {
      kind: "file_edit";
      path: string;
      unified_diff: string | null;
    }
  | {
      kind: "search";
      query: string;
      scope: string | null;
    }
  | {
      kind: "subagent";
      child_agent_id: string | null;
      role: string;
      prompt_preview: string;
    }
  | {
      kind: "generic";
      tool_name: string;
      arguments_json: string;
    };

export interface QuestionItem {
  id: string;
  header: string | null;
  question: string;
  options: string[];
  is_multi_select: boolean;
}

export interface PromptBadge {
  kind: "slash" | "file" | "git_diff" | "terminal" | "image" | "quote" | "paste" | string;
  label: string;
  path?: string | null;
}

export interface BackgroundTaskInfo {
  task_id?: string | null;
  label?: string | null;
  log_path?: string | null;
}

export type TimelineItemView =
  | {
      item_type: "user_prompt";
      seq: number;
      message_id: string;
      text: string;
      badges?: PromptBadge[];
      blocks: RichBlockView[];
      timestamp_ms?: number | null;
    }
  | {
      item_type: "assistant_thinking";
      seq: number;
      item_id: string;
      text: string;
      timestamp_ms?: number | null;
    }
  | {
      item_type: "assistant_message";
      seq: number;
      item_id: string;
      text: string;
      streaming: boolean;
      blocks: RichBlockView[];
      timestamp_ms?: number | null;
    }
  | {
      item_type: "tool_call";
      seq: number;
      call_id: string;
      status: "running" | "completed" | "failed" | "denied";
      summary_title: string;
      detail: ToolCallDetail;
      background_task?: BackgroundTaskInfo | null;
      diff_files: DiffFileView[];
      timestamp_ms?: number | null;
    }
  | {
      item_type: "permission_request";
      seq: number;
      request_id: string;
      title: string;
      detail: ToolCallDetail;
      allowed_decisions: string[];
      resolved_decision: string | null;
      guardian_assessment?: GuardianAssessment | null;
      timestamp_ms?: number | null;
    }
  | {
      item_type: "question_request";
      seq: number;
      request_id: string;
      questions: QuestionItem[];
      resolved_answers: string[][] | null;
      timestamp_ms?: number | null;
    }
  | {
      item_type: "notice";
      seq: number;
      level: string;
      message: string;
      timestamp_ms?: number | null;
    }
  | {
      item_type: "agent_message";
      seq: number;
      message_id: string;
      kind: AgentMessageKind;
      source_agent_id?: string | null;
      source_host_id?: string | null;
      source_title?: string | null;
      event?: string | null;
      text: string;
      permission_context?: PermissionCheckpointRef | null;
      blocks: RichBlockView[];
      timestamp_ms?: number | null;
    };

export interface AgentTimelineView {
  revision: number;
  host_id: string;
  agent: AgentCardView | null;
  items: TimelineItemView[];
}

export interface AgentTimelineDeltaView {
  revision: number;
  host_id: string;
  agent: AgentCardView | null;
  latest_seq: number;
  upserted_items: TimelineItemView[];
  resolved_permissions: Record<string, string>;
  resolved_questions: Record<string, string[][]>;
  reset?: boolean;
}

export interface RewindAgentResultView {
  agent: AgentCardView;
  restored_files: number;
  restored_prompt: string | null;
  notice: string | null;
}

export type ForkTarget =
  | { kind: "same_directory" }
  | { kind: "new_worktree"; branch?: string | null; base_ref?: string | null };

export interface ForkAgentRequest {
  host_id: string;
  source_agent_id: string;
  up_to_seq?: number | null;
  target?: ForkTarget;
  title?: string | null;
  initial_prompt?: string | null;
}

export interface ForkAgentResultView {
  agent: AgentCardView;
  workspace?: WorkspaceSummary | null;
}

export interface GitFileEntry {
  path: string;
  status: string;
  staged: boolean;
  untracked: boolean;
}

export interface GitStatusSnapshot {
  cwd: string;
  is_git_repo: boolean;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFileEntry[];
  nested_git_repos?: NestedGitRepo[];
}

export interface GitReviewView {
  revision: number;
  host_id: string;
  cwd: string;
  status: GitStatusSnapshot | null;
  selected_path: string | null;
  raw_diff: string;
  diff_files: DiffFileView[];
}

export interface GitCommitResult {
  commit_sha: string;
  short_sha: string;
  summary: string;
  branch: string | null;
}

export interface GitPushResult {
  remote: string;
  branch: string;
  output: string;
}

export interface GitCreatePrResult {
  url: string;
  number: number | null;
  title: string;
}

export type GitMutationOutcome =
  | { kind: "files_staged"; paths: string[]; all: boolean }
  | { kind: "files_unstaged"; paths: string[]; all: boolean }
  | { kind: "hunk_applied"; reverse: boolean; cached: boolean }
  | { kind: "files_discarded"; paths: string[]; include_untracked: boolean }
  | ({ kind: "committed" } & GitCommitResult)
  | ({ kind: "pushed" } & GitPushResult)
  | ({ kind: "pr_created" } & GitCreatePrResult);

export interface WorkspaceFileView {
  host_id: string;
  cwd: string;
  path: string;
  resolved_path: string;
  language: string;
  content_kind: "text" | "image" | "pdf" | "audio" | "video" | "binary" | string;
  mime_type: string | null;
  content: string;
  size_bytes: number;
  truncated: boolean;
  etag?: string | null;
  mtime_unix_ms?: number | null;
  writable?: boolean;
  line_ending?: "lf" | "crlf" | "mixed" | string | null;
  highlighted_lines: HighlightedLineView[];
  markdown_blocks: RichBlockView[];
}

export interface WorkspaceFileConflict {
  current_etag: string;
  current_content?: string | null;
  current_size_bytes: number;
  current_mtime_unix_ms?: number | null;
}

export interface WorkspaceFileWriteResult {
  host_id: string;
  cwd: string;
  path: string;
  etag: string;
  size_bytes: number;
  mtime_unix_ms?: number | null;
  conflict?: WorkspaceFileConflict | null;
}

export interface WorkspaceEntryMutationResult {
  host_id: string;
  cwd: string;
  path: string;
  new_path?: string | null;
  action: "created_file" | "created_dir" | "renamed" | "deleted" | string;
}

export type InstructionScope =
  | "global_user"
  | "repo_root"
  | "ancestor_dir"
  | "workspace_dir"
  | "local_override";

export interface InstructionFileEntry {
  id: string;
  path: string;
  relative_path: string;
  filename: string;
  scope: InstructionScope;
  provider_hint?: string | null;
  precedence_order: number;
  exists: boolean;
  size_bytes: number;
  approx_tokens: number;
  etag?: string | null;
  preview: string;
  imports: string[];
}

export interface InstructionFilesSnapshotView {
  host_id: string;
  cwd: string;
  files: InstructionFileEntry[];
  total_tokens: number;
  merged_preview: string;
}

export interface InstructionFileContentView {
  host_id: string;
  cwd: string;
  path: string;
  content: string;
  etag?: string | null;
  exists: boolean;
}

export interface InstructionFileSavedView {
  host_id: string;
  cwd: string;
  path: string;
  etag: string;
  size_bytes: number;
  approx_tokens: number;
}

export type RuleTriggerMode = "always" | "glob_auto_attach" | "agent_decided" | "manual";

export type RuleScope =
  | "user_global"
  | "project_orbit"
  | "project_claude"
  | "project_cursor";

export interface PathScopedRule {
  id: string;
  name: string;
  description: string;
  scope: RuleScope;
  path: string;
  relative_path: string;
  trigger_mode: RuleTriggerMode;
  globs: string[];
  enabled: boolean;
  body: string;
  approx_tokens: number;
  etag: string;
}

export interface MatchedRuleSummary {
  id: string;
  name: string;
  trigger_mode: RuleTriggerMode;
  matched_globs: string[];
  approx_tokens: number;
}

export interface PathRulesSnapshotView {
  host_id: string;
  cwd: string;
  rules: PathScopedRule[];
  total_always_tokens: number;
  token_budget_warning: boolean;
}

export interface UpsertPathRuleParams {
  hostId: string;
  cwd: string;
  scope: RuleScope;
  name: string;
  description: string;
  triggerMode: RuleTriggerMode;
  globs: string[];
  enabled: boolean;
  body: string;
  expectedEtag?: string | null;
}

export interface MatchedPathRulesView {
  host_id: string;
  cwd: string;
  matched: MatchedRuleSummary[];
  injected_markdown: string;
  total_tokens: number;
}
export type DiagramKind = "mermaid" | "svg" | "graphviz";
export type DiagramTheme = "light" | "dark";

export interface RenderDiagramRequest {
  kind: DiagramKind;
  source: string;
  theme: DiagramTheme;
}

export interface RenderDiagramView {
  kind: DiagramKind;
  theme: DiagramTheme;
  svg: string | null;
  width: number;
  height: number;
  error: string | null;
  diagram_type_label: string | null;
}

export function detectDiagramKind(language: string, source: string): DiagramKind | null {
  const lang = language.trim().toLowerCase();
  if (lang === "mermaid" || lang === "mmd") return "mermaid";
  if (lang === "svg") return "svg";
  if (lang === "dot" || lang === "graphviz") return "graphviz";
  if (lang === "xml" || lang === "html") {
    const cleaned = source
      .replace(/^\uFEFF/, "")
      .replace(/^(?:\s*(?:<\?xml[\s\S]*?\?>|<!--[\s\S]*?-->|<!DOCTYPE[^>]*>))*\s*/i, "");
    if (/^<svg(?:\s|>)/i.test(cleaned)) {
      return "svg";
    }
  }
  return null;
}

export type SideQuestionStreamEvent =
  | {
      kind: "delta";
      request_id: string;
      text_delta: string;
    }
  | {
      kind: "done";
      request_id: string;
      full_text: string;
      error: string | null;
    };

export interface SshHostItem {
  alias: string;
  host_name: string;
  user: string | null;
  port: number | null;
  display_target: string;
}

export interface LocalMachineInfo {
  hostname: string;
  host_id: string | null;
  os_arch: string;
}

export interface StagedAttachmentInfo {
  path: string;
  filename: string;
  size_bytes: number;
}

export interface ClipboardImagePayload {
  filename: string;
  mime_type: string;
  data_b64: string;
}

export type ToolStepKind =
  | "file_read"
  | "file_create"
  | "file_edit"
  | "search"
  | "shell"
  | "subagent"
  | "generic";

export interface ToolStepDescriptorView {
  kind: ToolStepKind;
  verb: string;
  target: string;
  secondary_meta: string | null;
  open_path: string | null;
  additions: number;
  deletions: number;
  exit_code: number | null;
  is_running: boolean;
  is_failed: boolean;
  is_denied: boolean;
  has_expandable_detail: boolean;
}

export interface TurnFileDiffSummaryView {
  path: string;
  additions: number;
  deletions: number;
}

export interface TurnWorkCountsView {
  edited_file_count: number;
  created_file_count: number;
  command_count: number;
  read_file_count: number;
  search_count: number;
  subagent_count: number;
  mcp_server_count: number;
  other_tool_count: number;
  thought_count: number;
  narration_count: number;
  action_steps: number;
  total_steps: number;
  failed_count: number;
  denied_count: number;
  unrecovered_failure_count: number;
  additions: number;
  deletions: number;
}

export interface WorkSummaryResultView {
  counts: TurnWorkCountsView;
  label: string;
  live_label: string;
  live_detail?: string | null;
  label_includes_failure: boolean;
}

export interface TurnCompactionItemView {
  seq: number;
  label: string;
}

export type TurnLiveSegmentView =
  | {
      kind: "step_group";
      key: string;
      first_seq: number;
      items: TimelineItemView[];
      summary: WorkSummaryResultView;
      thought_duration_ms?: number | null;
      work_duration_ms?: number | null;
      started_at_ms?: number | null;
      latest_activity_ms?: number | null;
      has_following_content: boolean;
      has_running_step: boolean;
    }
  | {
      kind: "assistant_message";
      key: string;
      item: TimelineItemView;
    }
  | {
      kind: "agent_message";
      key: string;
      item: TimelineItemView;
    }
  | {
      kind: "notice";
      key: string;
      item: TimelineItemView;
    }
  | {
      kind: "compaction";
      key: string;
      label: string;
    }
  | {
      kind: "wakeup";
      key: string;
      label: string;
    };

export interface StageTurnView {
  key: string;
  turn_index: number;
  anchor_seq: number;
  user_prompt: TimelineItemView | null;
  agent_message?: TimelineItemView | null;
  is_compaction_command: boolean;
  folded_steps: TimelineItemView[];
  work_counts: TurnWorkCountsView;
  work_summary_label: string;
  work_summary_live_label: string;
  work_summary_live_detail?: string | null;
  work_summary_includes_failure: boolean;
  thought_duration_ms: number | null;
  work_duration_ms?: number | null;
  turn_duration_ms: number | null;
  terminal_reply: TimelineItemView | null;
  last_assistant_reply?: TimelineItemView | null;
  notices: TimelineItemView[];
  compactions: TurnCompactionItemView[];
  live_segments: TurnLiveSegmentView[];
  turn_diffs: TurnFileDiffSummaryView[];
  has_error: boolean;
  has_unrecovered_error: boolean;
  has_active_item: boolean;
  started_at_ms: number | null;
  completed_at_ms: number | null;
}

export type SessionTouchedFileAction = "created" | "edited" | "read";

export interface SessionTouchedFileView {
  path: string;
  display_path: string;
  action: SessionTouchedFileAction;
  additions: number;
  deletions: number;
  last_seq: number;
}

export type SessionBackgroundTaskKind = "shell" | "wakeup" | "queued";
export type SessionBackgroundTaskStatus = "running" | "queued";

export interface SessionBackgroundTaskView {
  id: string;
  kind: SessionBackgroundTaskKind;
  title: string;
  subtitle: string;
  status: SessionBackgroundTaskStatus;
  started_at_ms: number | null;
  command: string | null;
  output_preview: string | null;
  log_path?: string | null;
}

export interface SessionSubagentEntryView {
  id: string;
  role: string;
  prompt_preview: string;
  status: "running" | "completed" | "failed" | "denied";
  child_agent_id: string | null;
  provider_subagent_id?: string | null;
  last_seq: number;
}

export interface SessionInsightsView {
  touched_files: SessionTouchedFileView[];
  background_tasks: SessionBackgroundTaskView[];
  subagents: SessionSubagentEntryView[];
  detected_local_urls: string[];
}

export interface AgentStageView {
  revision: number;
  host_id: string;
  agent: AgentCardView | null;
  turns: StageTurnView[];
  window_start_turn_index?: number;
  total_turn_count?: number;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
  pending_permissions: TimelineItemView[];
  pending_questions: TimelineItemView[];
  session_turn_diffs: DiffFileView[];
  insights: SessionInsightsView;
  latest_seq: number;
  aux_revision?: number;
}

export interface AgentStageDeltaView {
  revision: number;
  host_id: string;
  agent: AgentCardView | null;
  replace_from_turn_index: number;
  window_start_turn_index?: number;
  changed_turns: StageTurnView[];
  total_turn_count: number;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
  pending_permissions: TimelineItemView[];
  pending_questions: TimelineItemView[];
  session_turn_diffs: DiffFileView[];
  insights: SessionInsightsView;
  latest_seq: number;
  reset: boolean;
  aux_revision?: number;
  aux_unchanged?: boolean;
}

export type SidebarProjectSort = "recent" | "alphabetical" | "manual";
export type SidebarThreadFilter = "all" | "active_unread";

export interface SidebarConversationView {
  host_id: string;
  agent: AgentCardView;
  is_worktree: boolean;
  is_pinned: boolean;
  is_unread: boolean;
  seen_seq: number;
}

export interface SidebarWorkspaceGroupView {
  workspace_id: string;
  title: string;
  mission_goal?: string | null;
  cwd: string;
  git_branch?: string | null;
  is_worktree: boolean;
  board_revision: number;
  has_conflict: boolean;
  peer_count: number;
  running_count: number;
  unread_count: number;
  is_multi_agent: boolean;
  is_curated?: boolean;
  conversations: SidebarConversationView[];
}

export interface SidebarProjectGroupView {
  id: string;
  host_id: string;
  name: string;
  primary_cwd: string;
  directories: string[];
  git_branch: string | null;
  is_git_repo?: boolean;
  nested_git_repos?: NestedGitRepo[];
  pinned_project_id: string | null;
  is_home_workspace: boolean;
  sort_weight_ms: number;
  sort_order_key?: string;
  conversations: SidebarConversationView[];
  workspaces?: SidebarWorkspaceGroupView[];
  unclassified_conversations?: SidebarConversationView[];
}

export interface LaunchpadProjectView {
  id: string;
  host_id: string;
  name: string;
  path: string;
  branch: string | null;
  is_git_repo?: boolean;
  nested_git_repos?: NestedGitRepo[];
}

export interface FleetSidebarView {
  revision: number;
  projects: SidebarProjectGroupView[];
  launchpad_projects: LaunchpadProjectView[];
  needs_attention: SidebarConversationView[];
  pinned_conversations: SidebarConversationView[];
}

export interface ClientThreadPrefSeed {
  host_id: string;
  agent_id: string;
  seen_seq: number;
  is_pinned: boolean;
  pinned_order: number;
  updated_at_ms: number;
}

export interface BridgeInfoView {
  protocol_version: number;
  core_version: string;
  capabilities: string[];
}

export interface FleetSidebarRequest {
  sort: SidebarProjectSort;
  filter: SidebarThreadFilter;
  query: string;
  active_host_id: string | null;
  active_agent_id: string | null;
  active_cwd: string | null;
}

export interface ModelSelectionRequest {
  host_id: string | null;
  provider: ProviderKind;
  desired_model: string | null;
  desired_effort: ReasoningEffort | null;
}

export interface ResolvedModelSelectionView {
  provider: ProviderKind;
  catalog: ProviderModelCatalog;
  selected_model_id: string | null;
  selected_model_label: string;
  selected_effort: ReasoningEffort | null;
  selected_effort_label: string | null;
  thinking_options: ThinkingOptionSpec[];
  supports_fast_mode?: boolean;
}

export type ClientPlatform = "desktop" | "ios" | "android" | "harmony";
export type SlashDirective =
  | "plan"
  | "review"
  | "compact"
  | "orbit"
  | "orbit-handoff"
  | "orbit-committee"
  | "orbit-advisor";

export interface SlashCommandView {
  id: string;
  command: string;
  label: string;
  description: string;
  shortcut_key: string | null;
  icon_key: string;
  keywords: string[];
  desktop_only: boolean;
}

export type ComposerContextInput =
  | { kind: "image"; path: string; filename: string }
  | { kind: "pdf"; path: string; filename: string }
  | { kind: "audio"; path: string; filename: string }
  | { kind: "video"; path: string; filename: string }
  | { kind: "staged_file"; path: string; filename: string }
  | { kind: "workspace_file"; path: string }
  | { kind: "git_diff"; staged: boolean; label: string; diff_text?: string | null }
  | { kind: "terminal_output"; label: string; content: string }
  | { kind: "assistant_quote"; label: string; content: string }
  | { kind: "workspace_peer"; handle: string; summary: string }
  | {
      kind: "code_symbol";
      name: string;
      symbol_kind: string;
      path: string;
      start_line: number;
      end_line: number;
      signature?: string | null;
      container?: string | null;
      language: string;
      snippet?: string | null;
    };

export interface ComposerQuoteInput {
  path: string;
  line_number: number | null;
  code: string;
}

export interface ComposerPasteInput {
  label: string;
  filename: string | null;
  content: string;
}

export interface ComposePromptRequest {
  provider: ProviderKind;
  raw_text: string;
  explicit_directive: SlashDirective | null;
  host_id: string | null;
  cwd: string | null;
  contexts: ComposerContextInput[];
  quotes: ComposerQuoteInput[];
  pastes: ComposerPasteInput[];
}

export type FastModeSlashAction = "toggle" | "enable" | "disable";

export interface ComposedPromptView {
  wire_prompt: string;
  display_prompt: string;
  badges: PromptBadge[];
  effective_directive: SlashDirective | null;
  is_compact: boolean;
  is_btw: boolean;
  btw_question: string | null;
  is_fork?: boolean;
  fork_worktree?: boolean;
  fork_branch?: string | null;
  fork_prompt?: string | null;
  fast_mode_action?: FastModeSlashAction | null;
  is_empty: boolean;
}

export interface UsageDailyPoint {
  date: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cost_usd: number;
  turns: number;
}

export interface UsageModelBreakdown {
  provider: string;
  model: string;
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cost_usd: number;
  turns: number;
}

export interface UsageProjectBreakdown {
  workspace_cwd: string;
  cost_usd: number;
  total_tokens: number;
  turns: number;
}

export interface QuotaDepletionForecast {
  provider: string;
  window_label: string;
  remaining_percent: number;
  burn_rate_pct_per_hour: number;
  estimated_depletion_secs?: number | null;
  resets_in_secs?: number | null;
  likely_to_exhaust_before_reset: boolean;
}

export interface ContextSegment {
  category: string;
  label: string;
  tokens: number;
  percent: number;
}

export interface ContextOccupancyBreakdown {
  agent_id: string;
  model: string;
  context_window_tokens: number;
  total_used_tokens: number;
  occupancy_percent: number;
  segments: ContextSegment[];
}

export interface UsageInsightsSnapshot {
  days: number;
  total_cost_usd: number;
  total_cache_savings_usd: number;
  total_input_tokens: number;
  total_output_tokens: number;
  total_cache_read_tokens: number;
  total_turns: number;
  daily: UsageDailyPoint[];
  by_model: UsageModelBreakdown[];
  by_project: UsageProjectBreakdown[];
  forecasts: QuotaDepletionForecast[];
}

export interface WorkspaceSymbolEntry {
  name: string;
  kind: string;
  path: string;
  start_line: number;
  end_line: number;
  signature?: string | null;
  container?: string | null;
  language: string;
}

export interface WorkspaceSymbolsView {
  host_id: string;
  cwd: string;
  symbols: WorkspaceSymbolEntry[];
  truncated: boolean;
}

export interface MentionCandidateView {
  key: string;
  kind: string;
  token_label: string;
  title: string;
  subtitle: string;
  file_path: string;
  git_status: string;
  score: number;
  symbol_kind?: string;
  start_line?: number;
  end_line?: number;
  signature?: string | null;
  container?: string | null;
  language?: string;
}

export type LspConfigSource = "builtin" | "project_json" | "user_config";
export type LspServerState = "stopped" | "starting" | "ready" | "error" | "unavailable";
export type LspDiagnosticSeverity = "error" | "warning" | "info" | "hint";

export interface LspServerStatus {
  id: string;
  name: string;
  state: LspServerState;
  workspace_cwd: string;
  binary_path?: string | null;
  diagnostics_count: number;
  error?: string | null;
  language_ids: string[];
  file_extensions: string[];
  source: LspConfigSource;
}

export interface LspDiagnostic {
  path: string;
  line: number;
  col: number;
  end_line: number;
  end_col: number;
  severity: LspDiagnosticSeverity;
  message: string;
  source?: string | null;
  code?: string | null;
}

export interface LspSymbolInfo {
  name: string;
  kind: string;
  container_name?: string | null;
  path: string;
  line: number;
  col: number;
  end_line: number;
}

export interface LspLocation {
  path: string;
  line: number;
  col: number;
  end_line: number;
  preview?: string | null;
}

export interface MentionRankRequest {
  host_id: string;
  cwd: string;
  query: string;
  limit: number;
  workspace_id?: string | null;
  active_agent_id?: string | null;
}

export type CoreNotification =
  | { kind: "fleet_changed"; revision: number }
  | { kind: "host_changed"; revision: number; host_id: string }
  | { kind: "timeline_changed"; revision: number; host_id: string; agent_id: string }
  | { kind: "git_changed"; revision: number; host_id: string; cwd: string }
  | {
      kind: "terminal_changed";
      revision: number;
      host_id: string;
      slot: number;
      raw_bytes: number[];
    };

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function isMacDesktop(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);
}

export function isWindowsDesktop(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Win/i.test(navigator.platform || navigator.userAgent);
}

export function isLinuxDesktop(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Linux/i.test(navigator.platform || navigator.userAgent) && !isWindowsDesktop();
}

export function timelineItemLogicalKey(item: TimelineItemView): string {
  switch (item.item_type) {
    case "user_prompt":
      return `msg:${item.message_id}`;
    case "assistant_thinking":
      return `think:${item.item_id}`;
    case "assistant_message":
      return `asst:${item.item_id}`;
    case "tool_call":
      return `tool:${item.call_id}`;
    case "permission_request":
      return `perm:${item.request_id}`;
    case "question_request":
      return `quest:${item.request_id}`;
    case "notice":
      return item.seq > 0
        ? `notice:${item.seq}`
        : `notice:0:${item.timestamp_ms ?? 0}:${item.message}`;
    case "agent_message":
      return `am:${item.message_id}`;
  }
}

export type HookEventKind =
  | "session_start"
  | "pre_prompt_submit"
  | "pre_tool_use"
  | "post_tool_use"
  | "post_file_edit"
  | "notification"
  | "turn_stop"
  | "session_end";

export type HookScope = "user_global" | "project_orbit";

export type HookDecision = "allow" | "block" | "modify";

export type HookAction =
  | {
      type: "command";
      command: string;
      args: string[];
      env: Record<string, string>;
    }
  | {
      type: "http_webhook";
      url: string;
      headers: Record<string, string>;
    };

export interface LifecycleHookRule {
  id: string;
  name: string;
  scope: HookScope;
  event: HookEventKind;
  matcher?: string | null;
  action: HookAction;
  timeout_ms: number;
  enabled: boolean;
  trusted: boolean;
  description?: string | null;
}

export interface HookExecutionRecord {
  id: string;
  hook_id: string;
  hook_name: string;
  event: HookEventKind;
  matched_target?: string | null;
  decision: HookDecision;
  reason?: string | null;
  exit_code?: number | null;
  duration_ms: number;
  stdout_preview: string;
  stderr_preview: string;
  executed_at_ms: number;
}

export interface LifecycleHooksSnapshotView {
  hooks: LifecycleHookRule[];
  recent_executions: HookExecutionRecord[];
  project_hooks_trusted: boolean;
}

export type TriggerSourceKind = "github" | "slack" | "linear" | "generic_webhook";

export type TriggerDispatchTarget =
  | {
      type: "existing_agent";
      agent_id: string;
    }
  | {
      type: "spawn_workspace_agent";
      cwd: string;
      provider: ProviderKind;
      model?: string | null;
      mode: SessionMode;
    };

export interface GitHubPollConfig {
  repo: string;
  poll_interval_secs: number;
  watch_prs: boolean;
  watch_issues: boolean;
  watch_ci_failures: boolean;
  has_token: boolean;
}

export interface EventTriggerRecord {
  trigger_id: string;
  name: string;
  source: TriggerSourceKind;
  event_filters: string[];
  prompt_template: string;
  target: TriggerDispatchTarget;
  webhook_path: string;
  has_secret: boolean;
  masked_secret?: string | null;
  github_poll?: GitHubPollConfig | null;
  enabled: boolean;
  trigger_count: number;
  last_triggered_at_ms?: number | null;
  created_at_ms: number;
  updated_at_ms: number;
}

export interface EventTriggerDelivery {
  delivery_id: string;
  trigger_id: string;
  source: TriggerSourceKind;
  event_type: string;
  status:
    | "dispatched"
    | "filtered"
    | "rejected_signature"
    | "rate_limited"
    | "duplicate"
    | "error"
    | string;
  summary: string;
  dispatched_agent_id?: string | null;
  received_at_ms: number;
}

export interface EventTriggersSnapshotView {
  triggers: EventTriggerRecord[];
  recent_deliveries: EventTriggerDelivery[];
  base_webhook_url: string;
}

export interface UpsertEventTriggerRequest {
  host_id: string;
  trigger_id?: string | null;
  name: string;
  source: TriggerSourceKind;
  event_filters: string[];
  prompt_template: string;
  target: TriggerDispatchTarget;
  secret?: string | null;
  keep_existing_secret: boolean;
  github_poll?: GitHubPollConfig | null;
  github_token?: string | null;
  keep_existing_token: boolean;
  enabled: boolean;
}

export interface ReattachTerminalEvent {
  host_id: string;
  terminal_id: string;
  title: string;
  cwd: string;
  slot?: number | null;
}

export function normalizeBinaryChunk(payload: unknown): Uint8Array | null {
  if (payload instanceof Uint8Array) {
    return payload.byteLength > 0 ? payload : null;
  }
  if (payload instanceof ArrayBuffer) {
    return payload.byteLength > 0 ? new Uint8Array(payload) : null;
  }
  if (ArrayBuffer.isView(payload)) {
    return payload.byteLength > 0
      ? new Uint8Array(payload.buffer, payload.byteOffset, payload.byteLength)
      : null;
  }
  if (Array.isArray(payload) && payload.length > 0) {
    return Uint8Array.from(payload as number[]);
  }
  return null;
}

export const orbitIpc = {
  async subscribe(onNotification: (ev: CoreNotification) => void): Promise<void> {
    if (!isTauriRuntime()) return;
    const channel = new Channel<CoreNotification>();
    channel.onmessage = onNotification;
    await invoke("subscribe_core_events", { onEvent: channel });
  },

  async getFleetOverview(): Promise<FleetOverviewView> {
    if (!isTauriRuntime()) {
      return { revision: 0, total_pending_approvals: 0, hosts: [], projects: [] };
    }
    return invoke("get_fleet_overview");
  },

  async getBridgeInfo(): Promise<BridgeInfoView> {
    if (!isTauriRuntime()) {
      return { protocol_version: 2, core_version: "0.3.7", capabilities: [] };
    }
    return invoke("get_bridge_info");
  },

  async getFleetSidebar(req: FleetSidebarRequest): Promise<FleetSidebarView> {
    if (!isTauriRuntime()) {
      return {
        revision: 0,
        projects: [],
        launchpad_projects: [],
        needs_attention: [],
        pinned_conversations: [],
      };
    }
    return invoke("get_fleet_sidebar", { req });
  },

  async getHostDetail(hostId: string): Promise<HostDetailView | null> {
    if (!isTauriRuntime()) return null;
    return invoke("get_host_detail", { hostId });
  },

  async getAgentStage(hostId: string, agentId: string): Promise<AgentStageView> {
    if (!isTauriRuntime()) {
      return {
        revision: 0,
        host_id: hostId,
        agent: null,
        turns: [],
        pending_permissions: [],
        pending_questions: [],
        session_turn_diffs: [],
        insights: {
          touched_files: [],
          background_tasks: [],
          subagents: [],
          detected_local_urls: [],
        },
        latest_seq: 0,
      };
    }
    return invoke("get_agent_stage", { hostId, agentId });
  },

  async getAgentStageDelta(
    hostId: string,
    agentId: string,
    sinceRevision: number,
    sinceAuxRevision?: number | null,
    tailTurnLimit?: number | null,
  ): Promise<AgentStageDeltaView> {
    if (!isTauriRuntime()) {
      return {
        revision: 0,
        host_id: hostId,
        agent: null,
        replace_from_turn_index: 0,
        window_start_turn_index: 0,
        changed_turns: [],
        total_turn_count: 0,
        pending_permissions: [],
        pending_questions: [],
        session_turn_diffs: [],
        insights: {
          touched_files: [],
          background_tasks: [],
          subagents: [],
          detected_local_urls: [],
        },
        latest_seq: 0,
        reset: sinceRevision > 0,
        aux_revision: 0,
        aux_unchanged: false,
        has_older_turns: false,
        has_older_history: false,
        oldest_loaded_seq: 0,
      };
    }
    return invoke("get_agent_stage_delta", {
      hostId,
      agentId,
      sinceRevision,
      sinceAuxRevision: sinceAuxRevision ?? null,
      tailTurnLimit: tailTurnLimit ?? null,
    });
  },

  async getAgentStageTurnsSlice(
    hostId: string,
    agentId: string,
    startTurnIndex: number,
    count: number,
  ): Promise<StageTurnView[]> {
    if (!isTauriRuntime()) return [];
    return invoke("get_agent_stage_turns_slice", {
      hostId,
      agentId,
      startTurnIndex,
      count,
    });
  },

  async loadOlderAgentTimeline(
    hostId: string,
    agentId: string,
    limitEvents = 300,
  ): Promise<boolean> {
    if (!isTauriRuntime()) return false;
    return invoke("load_older_agent_timeline", {
      hostId,
      agentId,
      limitEvents,
    });
  },

  async getFallbackProviderCatalogs(): Promise<ProviderModelCatalog[]> {
    if (!isTauriRuntime()) return [];
    return invoke("get_fallback_provider_catalogs");
  },

  async resolveModelSelection(req: ModelSelectionRequest): Promise<ResolvedModelSelectionView | null> {
    if (!isTauriRuntime()) return null;
    return invoke("resolve_model_selection", { req });
  },

  async getSlashCommands(
    platform: ClientPlatform = "desktop",
    query?: string | null,
  ): Promise<SlashCommandView[]> {
    if (!isTauriRuntime()) return [];
    return invoke("get_slash_commands", { platform, query: query ?? null });
  },

  async composePrompt(req: ComposePromptRequest): Promise<ComposedPromptView> {
    if (!isTauriRuntime()) {
      const text = req.raw_text.trim();
      const fastAction = (() => {
        if (
          req.explicit_directive ||
          req.contexts.length > 0 ||
          req.quotes.length > 0 ||
          req.pastes.length > 0
        ) {
          return null;
        }
        if (!text.startsWith("/")) return null;
        const parts = text.split(/\s+/);
        if (parts[0]?.toLowerCase() !== "/fast") return null;
        if (parts.length === 1) return "toggle" as const;
        if (parts.length === 2) {
          const arg = parts[1].toLowerCase();
          if (arg === "on" || arg === "true" || arg === "enable" || arg === "1") {
            return "enable" as const;
          }
          if (arg === "off" || arg === "false" || arg === "disable" || arg === "0") {
            return "disable" as const;
          }
        }
        return null;
      })();
      if (fastAction) {
        return {
          wire_prompt: "",
          display_prompt: "",
          badges: [],
          effective_directive: null,
          is_compact: false,
          is_btw: false,
          btw_question: null,
          is_fork: false,
          fork_worktree: false,
          fork_branch: null,
          fork_prompt: null,
          fast_mode_action: fastAction,
          is_empty: false,
        };
      }
      return {
        wire_prompt: text,
        display_prompt: text,
        badges: [],
        effective_directive: req.explicit_directive,
        is_compact: false,
        is_btw: false,
        btw_question: null,
        is_fork: false,
        fork_worktree: false,
        fork_branch: null,
        fork_prompt: null,
        fast_mode_action: null,
        is_empty: text.length === 0,
      };
    }
    return invoke("compose_prompt", { req });
  },

  async rankMentionCandidates(req: MentionRankRequest): Promise<MentionCandidateView[]> {
    if (!isTauriRuntime()) return [];
    return invoke("rank_mention_candidates", { req });
  },

  async buildSideQuestionContext(
    hostId: string,
    agentId: string,
    maxChars = 3000,
  ): Promise<string> {
    if (!isTauriRuntime()) return "";
    return invoke("build_side_question_context", { hostId, agentId, maxChars });
  },

  async renderDiagram(req: RenderDiagramRequest): Promise<RenderDiagramView> {
    if (!isTauriRuntime()) {
      return {
        kind: req.kind,
        theme: req.theme,
        svg: null,
        width: 0,
        height: 0,
        error: "Diagram rendering requires desktop native core",
        diagram_type_label: null,
      };
    }
    return invoke<RenderDiagramView>("render_diagram", { req });
  },

  async markAgentSeen(hostId: string, agentId: string, seq: number): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("mark_agent_seen", { hostId, agentId, seq });
  },

  async markAgentUnread(hostId: string, agentId: string): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("mark_agent_unread", { hostId, agentId });
  },

  async setAgentPinned(hostId: string, agentId: string, pinned: boolean): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("set_agent_pinned", { hostId, agentId, pinned });
  },

  async togglePinAgent(hostId: string, agentId: string): Promise<boolean> {
    if (!isTauriRuntime()) return false;
    return invoke<boolean>("toggle_pin_agent", { hostId, agentId });
  },

  async seedThreadPrefs(seeds: ClientThreadPrefSeed[]): Promise<void> {
    if (!isTauriRuntime() || seeds.length === 0) return;
    await invoke<void>("seed_thread_prefs", { seeds });
  },

  async onAppForeground(): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("on_app_foreground");
  },

  async onAppBackground(): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("on_app_background");
  },

  async onNetworkChanged(): Promise<void> {
    if (!isTauriRuntime()) return;
    await invoke<void>("on_network_changed");
  },

  async getAgentTimeline(hostId: string, agentId: string): Promise<AgentTimelineView> {
    if (!isTauriRuntime()) {
      return { revision: 0, host_id: hostId, agent: null, items: [] };
    }
    return invoke("get_agent_timeline", { hostId, agentId });
  },

  async getAgentTimelineDelta(
    hostId: string,
    agentId: string,
    sinceSeq: number,
  ): Promise<AgentTimelineDeltaView> {
    if (!isTauriRuntime()) {
      return {
        revision: 0,
        host_id: hostId,
        agent: null,
        latest_seq: sinceSeq,
        upserted_items: [],
        resolved_permissions: {},
        resolved_questions: {},
      };
    }
    return invoke("get_agent_timeline_delta", { hostId, agentId, sinceSeq });
  },

  async getGitReview(hostId: string, cwd: string, path: string | null): Promise<GitReviewView> {
    if (!isTauriRuntime()) {
      return {
        revision: 0,
        host_id: hostId,
        cwd,
        status: null,
        selected_path: path,
        raw_diff: "",
        diff_files: [],
      };
    }
    return invoke("get_git_review", { hostId, cwd, path });
  },

  pairHost: async (pairingUri: string): Promise<HostCardView> => {
    return invoke<HostCardView>("pair_host", { pairingUri });
  },

  exportHostPairingUri: async (hostId: string): Promise<string> => {
    return invoke<string>("export_host_pairing_uri", { hostId });
  },

  pairSshHost: async (sshAlias: string, remotePort?: number | null): Promise<HostCardView> => {
    return invoke<HostCardView>("pair_ssh_host", {
      sshAlias,
      remotePort: remotePort ?? null,
    });
  },

  getLocalMachineInfo: async (): Promise<LocalMachineInfo | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<LocalMachineInfo>("get_local_machine_info");
  },

  pairLocalHost: async (): Promise<HostCardView> => {
    return invoke<HostCardView>("pair_local_host");
  },

  updateHostConfig: async (params: {
    hostId: string;
    customName?: string | null;
    routeMode?: HostRouteMode;
    sshAlias?: string | null;
    remoteDaemonPort?: number;
    customDirectEndpoint?: string | null;
    relayUrl?: string | null;
    reconnect?: boolean;
  }): Promise<HostCardView> => {
    return invoke<HostCardView>("update_host_config", params);
  },

  upgradeRemoteDaemon: async (
    hostId: string,
    sshAlias: string,
    remotePort?: number | null,
  ): Promise<string> => {
    return invoke<string>("upgrade_remote_daemon", {
      hostId,
      sshAlias,
      remotePort: remotePort ?? null,
    });
  },

  upsertProject: async (params: {
    projectId?: string | null;
    hostId: string;
    name: string;
    directories: string[];
  }): Promise<ClientProjectView> => {
    return invoke<ClientProjectView>("upsert_project", params);
  },

  removeProject: async (projectId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("remove_project", { projectId });
  },

  reorderProjects: async (orderedKeys: string[]): Promise<string[]> => {
    if (!isTauriRuntime()) return orderedKeys;
    return invoke<string[]>("reorder_projects", { orderedKeys });
  },

  getProjectOrder: async (): Promise<string[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<string[]>("get_project_order");
  },

  inspectGitRepos: async (
    hostId: string,
    path: string,
    extraDirs?: string[],
  ): Promise<GitReposInspectionView> => {
    if (!isTauriRuntime()) {
      return { path, is_git_repo: false, git_branch: null, nested_git_repos: [] };
    }
    return invoke<GitReposInspectionView>("inspect_git_repos", {
      hostId,
      path,
      extraDirs: extraDirs ?? [],
    });
  },

  getGitRepos: async (hostId: string, path: string): Promise<GitReposInspectionView | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<GitReposInspectionView | null>("get_git_repos", { hostId, path });
  },

  listDirectory: async (hostId: string, path: string | null): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("list_directory", { hostId, path });
  },

  removeHost: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("remove_host", { hostId });
  },

  updateHostRelay: async (hostId: string, relayUrl: string | null): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_host_relay", { hostId, relayUrl });
  },

  applyRelayToAllHosts: async (rawRelayUrl: string): Promise<number> => {
    if (!isTauriRuntime()) return 0;
    return invoke<number>("apply_relay_to_all_hosts", { rawRelayUrl });
  },

  connectHost: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("connect_host", { hostId });
  },

  connectAllHosts: async (): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("connect_all_hosts");
  },

  refreshHost: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("refresh_host", { hostId });
  },

  refreshQuotas: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("refresh_quotas", { hostId });
  },

  refreshProviderModels: async (hostId: string, forceRefresh = false): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("refresh_provider_models", { hostId, forceRefresh });
  },

  createAgent: async (params: {
    hostId: string;
    cwd: string;
    extraDirs?: string[];
    provider: ProviderKind;
    acpAgentId?: string | null;
    model?: string | null;
    effort?: ReasoningEffort | null;
    fastMode?: boolean | null;
    mode: SessionMode;
    initialPrompt: string;
    initialDisplayPrompt?: string | null;
    initialBadges?: PromptBadge[];
    worktreeBranch: string | null;
    profileId?: string | null;
    workspaceId?: string | null;
    handle?: string | null;
    autoCluster?: boolean | null;
  }): Promise<string> => {
    if (!isTauriRuntime()) return "";
    return invoke<string>("create_agent", {
      ...params,
      acpAgentId: params.acpAgentId ?? null,
      model: params.model ?? null,
      effort: params.effort ?? null,
      fastMode: params.fastMode ?? null,
      initialDisplayPrompt: params.initialDisplayPrompt ?? null,
      initialBadges: params.initialBadges ?? [],
      profileId: params.profileId ?? null,
      workspaceId: params.workspaceId ?? null,
      handle: params.handle ?? null,
      autoCluster: params.autoCluster ?? null,
    });
  },

  openAgentTimeline: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("open_agent_timeline", { hostId, agentId });
  },

  sendPrompt: async (
    hostId: string,
    agentId: string,
    prompt: string,
    meta?: {
      displayPrompt?: string;
      badges?: PromptBadge[];
      activeTurnBehavior?: ActiveTurnBehavior;
    },
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("send_prompt", {
      hostId,
      agentId,
      prompt,
      displayPrompt: meta?.displayPrompt ?? null,
      badges: meta?.badges ?? [],
      activeTurnBehavior: meta?.activeTurnBehavior ?? null,
    });
  },

  enqueuePrompt: async (
    hostId: string,
    agentId: string,
    prompt: string,
    meta?: {
      displayPrompt?: string;
      badges?: PromptBadge[];
      queueId?: string;
    },
  ): Promise<string> => {
    if (!isTauriRuntime()) return meta?.queueId ?? `q_${Date.now()}`;
    return invoke<string>("enqueue_prompt", {
      hostId,
      agentId,
      prompt,
      displayPrompt: meta?.displayPrompt ?? null,
      badges: meta?.badges ?? [],
      queueId: meta?.queueId ?? null,
    });
  },

  removeQueuedPrompt: async (
    hostId: string,
    agentId: string,
    queueId: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("remove_queued_prompt", { hostId, agentId, queueId });
  },

  flushQueuedPromptNow: async (
    hostId: string,
    agentId: string,
    queueId: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("flush_queued_prompt_now", { hostId, agentId, queueId });
  },

  reorderQueuedPrompts: async (
    hostId: string,
    agentId: string,
    queueIds: string[],
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("reorder_queued_prompts", { hostId, agentId, queueIds });
  },

  interruptAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("interrupt_agent", { hostId, agentId });
  },

  setSessionMode: async (
    hostId: string,
    agentId: string,
    mode: SessionMode,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("set_session_mode", { hostId, agentId, mode });
  },

  setAgentConfig: async (
    hostId: string,
    agentId: string,
    model: string | null,
    effort: ReasoningEffort | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("set_agent_config", { hostId, agentId, model, effort });
  },

  setAgentModel: async (
    hostId: string,
    agentId: string,
    model: string | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("set_agent_model", { hostId, agentId, model });
  },

  switchAgentProvider: async (
    hostId: string,
    agentId: string,
    provider: ProviderKind,
    model: string | null = null,
    effort: ReasoningEffort | null = null,
    acpAgentId: string | null = null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("switch_agent_provider", {
      hostId,
      agentId,
      provider,
      model,
      effort,
      acpAgentId,
    });
  },

  archiveAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("archive_agent", { hostId, agentId });
  },

  unarchiveAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("unarchive_agent", { hostId, agentId });
  },

  deleteAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("delete_agent", { hostId, agentId });
  },

  detachAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("detach_agent", { hostId, agentId });
  },

  markAgentRead: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("mark_agent_read", { hostId, agentId });
  },

  updateAgent: async (params: {
    hostId: string;
    agentId: string;
    title?: string | null;
    model?: string | null;
    effort?: ReasoningEffort | null;
    fastMode?: boolean | null;
    mode?: SessionMode | null;
    labels?: Record<string, string> | null;
    handle?: string | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_agent", {
      hostId: params.hostId,
      agentId: params.agentId,
      title: params.title ?? null,
      model: params.model ?? null,
      effort: params.effort ?? null,
      fastMode: params.fastMode ?? null,
      mode: params.mode ?? null,
      labels: params.labels ?? null,
      handle: params.handle ?? null,
    });
  },

  fetchWorkspaceBoard: async (hostId: string, workspaceId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("fetch_workspace_board", { hostId, workspaceId });
  },

  getWorkspaceBoard: async (
    hostId: string,
    workspaceId: string,
  ): Promise<WorkspaceBoardSnapshot | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<WorkspaceBoardSnapshot | null>("get_workspace_board", {
      hostId,
      workspaceId,
    });
  },

  updateWorkspaceBoard: async (params: {
    hostId: string;
    workspaceId: string;
    content: string;
    section?: string | null;
    updatedBy?: string | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_workspace_board", {
      hostId: params.hostId,
      workspaceId: params.workspaceId,
      content: params.content,
      section: params.section ?? null,
      updatedBy: params.updatedBy ?? null,
    });
  },

  fetchWorkspaceEvents: async (
    hostId: string,
    workspaceId: string,
    limit?: number | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("fetch_workspace_events", {
      hostId,
      workspaceId,
      limit: limit ?? null,
    });
  },

  getWorkspaceEvents: async (
    hostId: string,
    workspaceId: string,
  ): Promise<WorkspaceEventRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<WorkspaceEventRecord[]>("get_workspace_events", { hostId, workspaceId });
  },

  updateWorkspaceGoal: async (
    hostId: string,
    workspaceId: string,
    goal: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_workspace_goal", { hostId, workspaceId, goal });
  },

  moveAgentToWorkspace: async (
    hostId: string,
    agentId: string,
    targetWorkspaceId?: string | null,
    opts?: { newWorkspaceTitle?: string | null; makeUnclassified?: boolean },
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("move_agent_to_workspace_v2", {
      hostId,
      agentId,
      targetWorkspaceId: targetWorkspaceId ?? null,
      newWorkspaceTitle: opts?.newWorkspaceTitle ?? null,
      makeUnclassified: Boolean(opts?.makeUnclassified),
    });
  },

  organizeProjectWorkspaces: async (
    hostId: string,
    projectRoot: string,
    agentIds: string[] = [],
    autoApply = true,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("organize_project_workspaces", {
      hostId,
      projectRoot,
      agentIds,
      autoApply,
    });
  },

  summarizeAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("summarize_agent", { hostId, agentId });
  },

  updateAgentIdentity: async (params: {
    hostId: string;
    agentId: string;
    handle?: string | null;
    title?: string | null;
    taskSummary?: string | null;
    lockHandle?: boolean;
    lockTitle?: boolean;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_agent_identity", {
      hostId: params.hostId,
      agentId: params.agentId,
      handle: params.handle ?? null,
      title: params.title ?? null,
      taskSummary: params.taskSummary ?? null,
      lockHandle: Boolean(params.lockHandle),
      lockTitle: Boolean(params.lockTitle),
    });
  },

  createMissionWorkspace: async (params: {
    hostId: string;
    sourceCwd: string;
    title: string;
    missionGoal?: string | null;
    isolation?: WorkspaceIsolation;
    worktreeBranch?: string | null;
    baseRef?: string | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("create_mission_workspace", {
      hostId: params.hostId,
      sourceCwd: params.sourceCwd,
      title: params.title,
      missionGoal: params.missionGoal ?? null,
      isolation: params.isolation ?? "local",
      worktreeBranch: params.worktreeBranch ?? null,
      baseRef: params.baseRef ?? null,
    });
  },

  mergeWorkspaces: async (
    hostId: string,
    sourceWorkspaceId: string,
    targetWorkspaceId: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("merge_workspaces", {
      hostId,
      sourceWorkspaceId,
      targetWorkspaceId,
    });
  },

  dismissClusterSuggestion: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("dismiss_cluster_suggestion", { hostId, agentId });
  },

  spawnWorkspacePeer: async (req: SpawnWorkspacePeerRequest): Promise<string> => {
    if (!isTauriRuntime()) return "";
    return invoke<string>("spawn_workspace_peer", { req });
  },

  createWorkspaceV2: async (params: {
    hostId: string;
    sourceCwd: string;
    title?: string | null;
    isolation: WorkspaceIsolation;
    worktreeMode: WorktreeCreateMode;
    worktreeBranch?: string | null;
    baseRef?: string | null;
    prNumber?: number | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("create_workspace_v2", {
      hostId: params.hostId,
      sourceCwd: params.sourceCwd,
      title: params.title ?? null,
      isolation: params.isolation,
      worktreeMode: params.worktreeMode,
      worktreeBranch: params.worktreeBranch ?? null,
      baseRef: params.baseRef ?? null,
      prNumber: params.prNumber ?? null,
    });
  },

  renameWorkspace: async (hostId: string, workspaceId: string, title: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("rename_workspace", { hostId, workspaceId, title });
  },

  archiveWorkspace: async (
    hostId: string,
    workspaceId: string,
    keepAgentsUnclassified = false,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("archive_workspace_v2", {
      hostId,
      workspaceId,
      keepAgentsUnclassified,
    });
  },

  unarchiveWorkspace: async (hostId: string, workspaceId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("unarchive_workspace", { hostId, workspaceId });
  },

  previewWorktreeHandoff: async (
    req: PreviewWorktreeHandoffRequest,
  ): Promise<WorktreeHandoffPreview> => {
    if (!isTauriRuntime()) {
      throw new Error("Worktree handoff requires the Orbit desktop runtime");
    }
    return invoke<WorktreeHandoffPreview>("preview_worktree_handoff", { req });
  },

  executeWorktreeHandoff: async (
    req: ExecuteWorktreeHandoffRequest,
  ): Promise<WorktreeHandoffOutcome> => {
    if (!isTauriRuntime()) {
      throw new Error("Worktree handoff requires the Orbit desktop runtime");
    }
    return invoke<WorktreeHandoffOutcome>("execute_worktree_handoff", { req });
  },

  listWorkspaceScripts: async (hostId: string, workspaceId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("list_workspace_scripts", { hostId, workspaceId });
  },

  startWorkspaceScript: async (
    hostId: string,
    workspaceId: string,
    scriptName: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("start_workspace_script", { hostId, workspaceId, scriptName });
  },

  stopWorkspaceScript: async (
    hostId: string,
    workspaceId: string,
    scriptName: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("stop_workspace_script", { hostId, workspaceId, scriptName });
  },

  upsertProfile: async (hostId: string, profile: AgentProfile): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("upsert_profile", { hostId, profile });
  },

  deleteProfile: async (hostId: string, profileId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("delete_profile", { hostId, profileId });
  },

  listArtifacts: async (
    hostId: string,
    workspaceId?: string | null,
    agentId?: string | null,
  ): Promise<ArtifactSummary[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ArtifactSummary[]>("list_artifacts", {
      hostId,
      workspaceId: workspaceId ?? null,
      agentId: agentId ?? null,
    });
  },

  getArtifact: async (
    hostId: string,
    artifactId: string,
    version?: number | null,
  ): Promise<ArtifactDetail> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<ArtifactDetail>("get_artifact", {
      hostId,
      artifactId,
      version: version ?? null,
    });
  },

  upsertArtifact: async (request: UpsertArtifactRequest): Promise<ArtifactSummary> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<ArtifactSummary>("upsert_artifact", { request });
  },

  deleteArtifact: async (hostId: string, artifactId: string): Promise<string> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<string>("delete_artifact", { hostId, artifactId });
  },

  listAcpAgents: async (hostId: string): Promise<AcpAgentAvailability[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<AcpAgentAvailability[]>("list_acp_agents", { hostId });
  },

  refreshAcpAgents: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("refresh_acp_agents", { hostId });
  },

  upsertCustomAcpAgent: async (
    hostId: string,
    spec: AcpAgentSpec,
  ): Promise<AcpAgentAvailability[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<AcpAgentAvailability[]>("upsert_custom_acp_agent", { hostId, spec });
  },

  deleteCustomAcpAgent: async (
    hostId: string,
    agentId: string,
  ): Promise<AcpAgentAvailability[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<AcpAgentAvailability[]>("delete_custom_acp_agent", { hostId, agentId });
  },

  testAcpAgent: async (
    hostId: string,
    agentId: string,
    cwd?: string | null,
  ): Promise<AcpProbeResult> => {
    if (!isTauriRuntime()) {
      return {
        agent_id: agentId,
        ok: false,
        capabilities_summary: [],
        auth_methods: [],
        latency_ms: 0,
        error: "Desktop runtime unavailable",
      };
    }
    return invoke<AcpProbeResult>("test_acp_agent", {
      hostId,
      agentId,
      cwd: cwd ?? null,
    });
  },

  refreshAcpRegistry: async (
    hostId: string,
    registryUrl?: string | null,
  ): Promise<AcpRegistryRefreshView> => {
    if (!isTauriRuntime()) return { agents: [], warning: null };
    return invoke<AcpRegistryRefreshView>("refresh_acp_registry", {
      hostId,
      registryUrl: registryUrl ?? null,
    });
  },

  listExternalMcpServers: async (
    hostId: string,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ExternalMcpServerRecord[]>("list_external_mcp_servers", {
      hostId,
      cwd: cwd ?? null,
    });
  },

  upsertExternalMcpServer: async (
    request: UpsertExternalMcpServerRequest,
  ): Promise<ExternalMcpServerRecord[]> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<ExternalMcpServerRecord[]>("upsert_external_mcp_server", { request });
  },

  toggleExternalMcpServer: async (
    hostId: string,
    serverId: string,
    enabled: boolean,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<ExternalMcpServerRecord[]>("toggle_external_mcp_server", {
      hostId,
      serverId,
      enabled,
      cwd: cwd ?? null,
    });
  },

  deleteExternalMcpServer: async (
    hostId: string,
    serverId: string,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime unavailable");
    }
    return invoke<ExternalMcpServerRecord[]>("delete_external_mcp_server", {
      hostId,
      serverId,
      cwd: cwd ?? null,
    });
  },

  probeExternalMcpServer: async (
    hostId: string,
    serverId: string,
    cwd?: string | null,
  ): Promise<McpProbeResult> => {
    if (!isTauriRuntime()) {
      return {
        ok: false,
        tools: [],
        latency_ms: 0,
        error: "Desktop runtime unavailable",
        probed_at_ms: Date.now(),
      };
    }
    return invoke<McpProbeResult>("probe_external_mcp_server", {
      hostId,
      serverId,
      cwd: cwd ?? null,
    });
  },

  createSchedule: async (params: {
    hostId: string;
    name: string;
    cronExpr: string;
    timezone?: string | null;
    prompt: string;
    target: ScheduleTarget;
    maxRuns?: number | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("create_schedule", {
      hostId: params.hostId,
      name: params.name,
      cronExpr: params.cronExpr,
      timezone: params.timezone ?? null,
      prompt: params.prompt,
      target: params.target,
      maxRuns: params.maxRuns ?? null,
    });
  },

  updateSchedule: async (params: {
    hostId: string;
    scheduleId: string;
    name?: string | null;
    cronExpr?: string | null;
    prompt?: string | null;
    maxRuns?: number | null;
    archiveOnFinish?: boolean | null;
  }): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_schedule", {
      hostId: params.hostId,
      scheduleId: params.scheduleId,
      name: params.name ?? null,
      cronExpr: params.cronExpr ?? null,
      prompt: params.prompt ?? null,
      maxRuns: params.maxRuns ?? null,
      archiveOnFinish: params.archiveOnFinish ?? null,
    });
  },

  pauseSchedule: async (hostId: string, scheduleId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("pause_schedule", { hostId, scheduleId });
  },

  resumeSchedule: async (hostId: string, scheduleId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("resume_schedule", { hostId, scheduleId });
  },

  runScheduleOnce: async (hostId: string, scheduleId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("run_schedule_once", { hostId, scheduleId });
  },

  deleteSchedule: async (hostId: string, scheduleId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("delete_schedule", { hostId, scheduleId });
  },

  fetchScheduleRuns: async (
    hostId: string,
    scheduleId: string,
    limit?: number | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("fetch_schedule_runs", { hostId, scheduleId, limit: limit ?? null });
  },

  getScheduleRuns: async (hostId: string, scheduleId: string): Promise<ScheduleRunRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ScheduleRunRecord[]>("get_schedule_runs", { hostId, scheduleId });
  },

  addConnectedHost: async (
    hostId: string,
    pairingUri: string,
    label?: string | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("add_connected_host", {
      hostId,
      label: label ?? null,
      pairingUri,
    });
  },

  updateConnectedHost: async (
    hostId: string,
    registrationId: string,
    params: { label?: string | null; enabled?: boolean | null },
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_connected_host", {
      hostId,
      registrationId,
      label: params.label ?? null,
      enabled: params.enabled ?? null,
    });
  },

  removeConnectedHost: async (hostId: string, registrationId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("remove_connected_host", { hostId, registrationId });
  },

  syncFleetConnectedHosts: async (targetHostId: string): Promise<number> => {
    if (!isTauriRuntime()) return 0;
    return invoke<number>("sync_fleet_connected_hosts", { targetHostId });
  },

  updateOrchestrationConfig: async (
    hostId: string,
    config: OrchestrationConfig,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_orchestration_config", { hostId, config });
  },

  syncOrchestrationSkills: async (hostId: string, skillIds: string[]): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("sync_orchestration_skills", { hostId, skillIds });
  },

  listManagedSkills: async (
    hostId: string,
    cwd?: string | null,
  ): Promise<ManagedSkillRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ManagedSkillRecord[]>("list_managed_skills", {
      hostId,
      cwd: cwd ?? null,
    });
  },

  readManagedSkill: async (
    hostId: string,
    skillId: string,
    cwd?: string | null,
  ): Promise<ManagedSkillContentView> => {
    if (!isTauriRuntime()) {
      return { skill_id: skillId, content: "", etag: "" };
    }
    return invoke<ManagedSkillContentView>("read_managed_skill", {
      hostId,
      skillId,
      cwd: cwd ?? null,
    });
  },

  upsertManagedSkill: async (
    hostId: string,
    params: {
      scope: SkillWriteScope;
      cwd?: string | null;
      slug: string;
      name: string;
      description: string;
      content: string;
      enabled: boolean;
      expectedEtag?: string | null;
    },
  ): Promise<ManagedSkillRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ManagedSkillRecord[]>("upsert_managed_skill", {
      hostId,
      scope: params.scope,
      cwd: params.cwd ?? null,
      slug: params.slug,
      name: params.name,
      description: params.description,
      content: params.content,
      enabled: params.enabled,
      expectedEtag: params.expectedEtag ?? null,
    });
  },

  toggleManagedSkill: async (
    hostId: string,
    skillId: string,
    enabled: boolean,
    cwd?: string | null,
  ): Promise<ManagedSkillRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ManagedSkillRecord[]>("toggle_managed_skill", {
      hostId,
      skillId,
      enabled,
      cwd: cwd ?? null,
    });
  },

  deleteManagedSkill: async (
    hostId: string,
    skillId: string,
    cwd?: string | null,
  ): Promise<ManagedSkillRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ManagedSkillRecord[]>("delete_managed_skill", {
      hostId,
      skillId,
      cwd: cwd ?? null,
    });
  },

  importManagedSkill: async (
    hostId: string,
    params: {
      scope: SkillWriteScope;
      cwd?: string | null;
      sourceMarkdownOrPath: string;
      slugOverride?: string | null;
    },
  ): Promise<ManagedSkillRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ManagedSkillRecord[]>("import_managed_skill", {
      hostId,
      scope: params.scope,
      cwd: params.cwd ?? null,
      sourceMarkdownOrPath: params.sourceMarkdownOrPath,
      slugOverride: params.slugOverride ?? null,
    });
  },

  listProviderSubagents: async (hostId: string, parentAgentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("list_provider_subagents", { hostId, parentAgentId });
  },

  openProviderSubagentTimeline: async (
    hostId: string,
    parentAgentId: string,
    subagentId: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("open_provider_subagent_timeline", { hostId, parentAgentId, subagentId });
  },

  getProviderSubagentTimeline: async (
    hostId: string,
    parentAgentId: string,
    subagentId: string,
  ): Promise<ProviderSubagentEvent[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ProviderSubagentEvent[]>("get_provider_subagent_timeline", {
      hostId,
      parentAgentId,
      subagentId,
    });
  },

  getProviderSubagentTurns: async (
    hostId: string,
    parentAgentId: string,
    subagentId: string,
    fallbackPrompt?: string | null,
  ): Promise<StageTurnView[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<StageTurnView[]>("get_provider_subagent_turns", {
      hostId,
      parentAgentId,
      subagentId,
      fallbackPrompt: fallbackPrompt ?? null,
    });
  },

  refreshPendingPermissions: async (hostId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("refresh_pending_permissions", { hostId });
  },

  rewindAgent: async (
    hostId: string,
    agentId: string,
    toBeforeSeq: number,
    restoreFiles: boolean,
  ): Promise<RewindAgentResultView> => {
    if (!isTauriRuntime()) {
      throw new Error("Rewind is unavailable outside the desktop runtime");
    }
    return invoke<RewindAgentResultView>("rewind_agent", {
      hostId,
      agentId,
      toBeforeSeq,
      restoreFiles,
    });
  },
  forkAgent: async (req: ForkAgentRequest): Promise<ForkAgentResultView> => {
    if (!isTauriRuntime()) {
      throw new Error("Fork is unavailable outside the desktop runtime");
    }
    return invoke<ForkAgentResultView>("fork_agent", { req });
  },
  resolvePermission: async (
    hostId: string,
    agentId: string,
    permissionRequestId: string,
    decision: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("resolve_permission", {
      hostId,
      agentId,
      permissionRequestId,
      decision,
    });
  },

  answerQuestion: async (
    hostId: string,
    agentId: string,
    questionRequestId: string,
    answers: string[][],
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("answer_question", {
      hostId,
      agentId,
      questionRequestId,
      answers,
    });
  },

  fetchGitStatus: async (hostId: string, cwd: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("fetch_git_status", { hostId, cwd });
  },

  fetchGitDiff: async (
    hostId: string,
    cwd: string,
    path: string | null,
    staged: boolean,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("fetch_git_diff", { hostId, cwd, path, staged });
  },

  gitStageFiles: async (params: {
    hostId: string;
    cwd: string;
    paths?: string[];
    all?: boolean;
  }): Promise<GitMutationOutcome> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to stage git files.");
    }
    return invoke<GitMutationOutcome>("git_stage_files", {
      hostId: params.hostId,
      cwd: params.cwd,
      paths: params.paths ?? [],
      all: params.all ?? false,
    });
  },

  gitUnstageFiles: async (params: {
    hostId: string;
    cwd: string;
    paths?: string[];
    all?: boolean;
  }): Promise<GitMutationOutcome> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to unstage git files.");
    }
    return invoke<GitMutationOutcome>("git_unstage_files", {
      hostId: params.hostId,
      cwd: params.cwd,
      paths: params.paths ?? [],
      all: params.all ?? false,
    });
  },

  gitApplyHunk: async (params: {
    hostId: string;
    cwd: string;
    patch: string;
    reverse?: boolean;
    cached?: boolean;
  }): Promise<GitMutationOutcome> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to apply git hunks.");
    }
    return invoke<GitMutationOutcome>("git_apply_hunk", {
      hostId: params.hostId,
      cwd: params.cwd,
      patch: params.patch,
      reverse: params.reverse ?? false,
      cached: params.cached ?? false,
    });
  },

  gitDiscardFiles: async (params: {
    hostId: string;
    cwd: string;
    paths: string[];
    includeUntracked?: boolean;
  }): Promise<GitMutationOutcome> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to discard git changes.");
    }
    return invoke<GitMutationOutcome>("git_discard_files", {
      hostId: params.hostId,
      cwd: params.cwd,
      paths: params.paths,
      includeUntracked: params.includeUntracked ?? false,
    });
  },

  gitCommit: async (params: {
    hostId: string;
    cwd: string;
    message: string;
    amend?: boolean;
  }): Promise<GitCommitResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to create git commits.");
    }
    return invoke<GitCommitResult>("git_commit", {
      hostId: params.hostId,
      cwd: params.cwd,
      message: params.message,
      amend: params.amend ?? false,
    });
  },

  gitPush: async (params: {
    hostId: string;
    cwd: string;
    remote?: string | null;
    branch?: string | null;
    setUpstream?: boolean;
  }): Promise<GitPushResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to push git branches.");
    }
    return invoke<GitPushResult>("git_push", {
      hostId: params.hostId,
      cwd: params.cwd,
      remote: params.remote ?? null,
      branch: params.branch ?? null,
      setUpstream: params.setUpstream ?? false,
    });
  },

  gitCreatePr: async (params: {
    hostId: string;
    cwd: string;
    title: string;
    body?: string;
    base?: string | null;
    draft?: boolean;
  }): Promise<GitCreatePrResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to create pull requests.");
    }
    return invoke<GitCreatePrResult>("git_create_pr", {
      hostId: params.hostId,
      cwd: params.cwd,
      title: params.title,
      body: params.body ?? "",
      base: params.base ?? null,
      draft: params.draft ?? false,
    });
  },

  startTerminal: async (
    hostId: string,
    cwd: string,
    cols: number,
    rows: number,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("start_terminal", { hostId, cwd, cols, rows });
  },

  attachTerminal: async (
    hostId: string,
    terminalId: string,
    slot: number,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("attach_terminal", { hostId, terminalId, slot });
  },

  killTerminal: async (hostId: string, terminalId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("kill_terminal", { hostId, terminalId });
  },

  subscribeTerminalBinary: async (
    hostId: string,
    terminalId: string,
    onChunk: (bytes: Uint8Array) => void,
    slot?: number,
  ): Promise<string | null> => {
    if (!isTauriRuntime()) return null;
    const channel = new Channel<ArrayBuffer | Uint8Array | number[]>();
    channel.onmessage = (payload) => {
      const bytes = normalizeBinaryChunk(payload);
      if (bytes) {
        onChunk(bytes);
      }
    };
    return invoke<string>("subscribe_terminal_binary", {
      hostId,
      terminalId,
      slot: slot ?? null,
      onChunk: channel,
    });
  },

  unsubscribeTerminalBinary: async (subscriptionId: string): Promise<void> => {
    if (!isTauriRuntime() || !subscriptionId) return;
    await invoke<void>("unsubscribe_terminal_binary", { subscriptionId });
  },

  ensureTerminalSlot: async (hostId: string, terminalId: string): Promise<number | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<number>("ensure_terminal_slot", { hostId, terminalId });
  },

  writeTerminalInput: async (
    hostId: string,
    terminalId: string,
    data: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("write_terminal_input", { hostId, terminalId, data });
  },

  sendTerminalInputSlot: async (
    hostId: string,
    slot: number,
    data: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("send_terminal_input_slot", { hostId, slot, data });
  },

  resizeTerminal: async (
    hostId: string,
    cols: number,
    rows: number,
    terminalId?: string | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("resize_terminal", {
      hostId,
      cols,
      rows,
      terminalId: terminalId ?? null,
    });
  },

  resizeTerminalSlot: async (
    hostId: string,
    slot: number,
    cols: number,
    rows: number,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("resize_terminal_slot", { hostId, slot, cols, rows });
  },

  openDetachedTerminalWindow: async (
    hostId: string,
    terminalId: string,
    title: string,
    cwd?: string,
  ): Promise<string | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<string>("open_detached_terminal_window", {
      hostId,
      terminalId,
      title,
      cwd: cwd ?? null,
    });
  },

  reattachDetachedTerminal: async (
    hostId: string,
    terminalId: string,
    title: string,
    cwd?: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("reattach_detached_terminal", {
      hostId,
      terminalId,
      title,
      cwd: cwd ?? null,
    });
  },

  onReattachTerminal: async (
    handler: (payload: ReattachTerminalEvent) => void,
  ): Promise<() => void> => {
    if (!isTauriRuntime()) return () => {};
    return listen<ReattachTerminalEvent>("orbit://reattach-terminal", (event) => {
      if (event.payload) {
        handler(event.payload);
      }
    });
  },

  listSshHosts: async (): Promise<SshHostItem[]> => {
    if (!isTauriRuntime()) return [];
    return invoke("list_ssh_hosts");
  },

  listExternalEditors: async (): Promise<ExternalEditorInfo[]> => {
    if (!isTauriRuntime()) {
      return [
        { kind: "cursor", label: "Cursor", installed: true },
        { kind: "zed", label: "Zed", installed: true },
        { kind: "vs_code", label: "VS Code", installed: true },
        { kind: "terminal", label: "System Terminal", installed: true },
        { kind: "file_manager", label: "Finder", installed: true },
      ];
    }
    return invoke("list_external_editors");
  },

  openInEditor: async (
    editor: ExternalEditorKind,
    cwd: string,
    sshTarget: string | null,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("open_in_editor", { editor, cwd, sshTarget });
  },

  pickProjectFolder: async (initialDir?: string | null): Promise<string | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<string | null>("pick_project_folder", {
      initialDir: initialDir ?? null,
    });
  },

  getWorkspaceFiles: async (hostId: string, cwd: string, force = false): Promise<string[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<string[]>("get_workspace_files", { hostId, cwd, force });
  },

  readWorkspaceFile: async (
    hostId: string,
    cwd: string,
    path: string,
  ): Promise<WorkspaceFileView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: hostId,
        cwd,
        path,
        resolved_path: path,
        language: "text",
        content_kind: "text",
        mime_type: "text/plain",
        content: "// Desktop runtime required to read host files.",
        size_bytes: 0,
        truncated: false,
        etag: null,
        mtime_unix_ms: null,
        writable: false,
        line_ending: "lf",
        highlighted_lines: [
          {
            line_number: 1,
            tokens: [
              {
                text: "// Desktop runtime required to read host files.",
                fg_hex: "#94A3B8",
                light_fg_hex: "#6E7781",
                bold: false,
                italic: false,
              },
            ],
          },
        ],
        markdown_blocks: [],
      };
    }
    return invoke<WorkspaceFileView>("read_workspace_file", { hostId, cwd, path });
  },

  async searchWorkspaceSymbols(
    hostId: string,
    cwd: string,
    query: string,
    limit = 30,
  ): Promise<WorkspaceSymbolsView> {
    if (!isTauriRuntime()) {
      return { host_id: hostId, cwd, symbols: [], truncated: false };
    }
    return invoke<WorkspaceSymbolsView>("search_workspace_symbols", {
      hostId,
      cwd,
      query,
      limit,
    });
  },

  writeWorkspaceFile: async (params: {
    hostId: string;
    cwd: string;
    path: string;
    content: string;
    expectedEtag?: string | null;
    force?: boolean;
  }): Promise<WorkspaceFileWriteResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to save host files.");
    }
    return invoke<WorkspaceFileWriteResult>("write_workspace_file", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
      content: params.content,
      expectedEtag: params.expectedEtag ?? null,
      force: params.force ?? false,
    });
  },

  createWorkspaceEntry: async (params: {
    hostId: string;
    cwd: string;
    path: string;
    isDir: boolean;
    initialContent?: string | null;
  }): Promise<WorkspaceEntryMutationResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to create workspace entries.");
    }
    return invoke<WorkspaceEntryMutationResult>("create_workspace_entry", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
      isDir: params.isDir,
      initialContent: params.initialContent ?? null,
    });
  },

  renameWorkspaceEntry: async (params: {
    hostId: string;
    cwd: string;
    oldPath: string;
    newPath: string;
  }): Promise<WorkspaceEntryMutationResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to rename workspace entries.");
    }
    return invoke<WorkspaceEntryMutationResult>("rename_workspace_entry", {
      hostId: params.hostId,
      cwd: params.cwd,
      oldPath: params.oldPath,
      newPath: params.newPath,
    });
  },

  deleteWorkspaceEntry: async (params: {
    hostId: string;
    cwd: string;
    path: string;
  }): Promise<WorkspaceEntryMutationResult> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to delete workspace entries.");
    }
    return invoke<WorkspaceEntryMutationResult>("delete_workspace_entry", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
    });
  },

  listInstructionFiles: async (
    hostId: string,
    cwd: string,
  ): Promise<InstructionFilesSnapshotView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: hostId,
        cwd,
        files: [],
        total_tokens: 0,
        merged_preview: "",
      };
    }
    return invoke<InstructionFilesSnapshotView>("list_instruction_files", {
      hostId,
      cwd,
    });
  },

  readInstructionFile: async (
    hostId: string,
    cwd: string,
    path: string,
  ): Promise<InstructionFileContentView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: hostId,
        cwd,
        path,
        content: "",
        etag: null,
        exists: false,
      };
    }
    return invoke<InstructionFileContentView>("read_instruction_file", {
      hostId,
      cwd,
      path,
    });
  },

  saveInstructionFile: async (params: {
    hostId: string;
    cwd: string;
    path: string;
    content: string;
    expectedEtag?: string | null;
  }): Promise<InstructionFileSavedView> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to save instruction files.");
    }
    return invoke<InstructionFileSavedView>("save_instruction_file", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
      content: params.content,
      expectedEtag: params.expectedEtag ?? null,
    });
  },

  listPathRules: async (hostId: string, cwd: string): Promise<PathRulesSnapshotView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: hostId,
        cwd,
        rules: [],
        total_always_tokens: 0,
        token_budget_warning: false,
      };
    }
    return invoke<PathRulesSnapshotView>("list_path_rules", { hostId, cwd });
  },

  upsertPathRule: async (params: UpsertPathRuleParams): Promise<PathRulesSnapshotView> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to save path rules.");
    }
    return invoke<PathRulesSnapshotView>("upsert_path_rule", {
      hostId: params.hostId,
      cwd: params.cwd,
      scope: params.scope,
      name: params.name,
      description: params.description,
      triggerMode: params.triggerMode,
      globs: params.globs,
      enabled: params.enabled,
      body: params.body,
      expectedEtag: params.expectedEtag ?? null,
    });
  },

  deletePathRule: async (params: {
    hostId: string;
    cwd: string;
    path: string;
  }): Promise<PathRulesSnapshotView> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to delete path rules.");
    }
    return invoke<PathRulesSnapshotView>("delete_path_rule", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
    });
  },

  matchPathRules: async (params: {
    hostId: string;
    cwd: string;
    activePaths: string[];
    manualRuleNames: string[];
  }): Promise<MatchedPathRulesView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: params.hostId,
        cwd: params.cwd,
        matched: [],
        injected_markdown: "",
        total_tokens: 0,
      };
    }
    return invoke<MatchedPathRulesView>("match_path_rules", {
      hostId: params.hostId,
      cwd: params.cwd,
      activePaths: params.activePaths,
      manualRuleNames: params.manualRuleNames,
    });
  },

  listLspServers: async (hostId: string, cwd: string): Promise<LspServerStatus[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<LspServerStatus[]>("list_lsp_servers", { hostId, cwd });
  },

  lspQuerySymbols: async (params: {
    hostId: string;
    cwd: string;
    query: string;
    path?: string | null;
    limit?: number;
  }): Promise<LspSymbolInfo[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<LspSymbolInfo[]>("lsp_query_symbols", {
      hostId: params.hostId,
      cwd: params.cwd,
      query: params.query,
      path: params.path ?? null,
      limit: params.limit ?? 50,
    });
  },

  lspGetDiagnostics: async (
    hostId: string,
    cwd: string,
    path?: string | null,
  ): Promise<LspDiagnostic[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<LspDiagnostic[]>("lsp_get_diagnostics", {
      hostId,
      cwd,
      path: path ?? null,
    });
  },

  lspGotoDefinition: async (params: {
    hostId: string;
    cwd: string;
    path: string;
    line: number;
    col: number;
  }): Promise<LspLocation[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<LspLocation[]>("lsp_goto_definition", {
      hostId: params.hostId,
      cwd: params.cwd,
      path: params.path,
      line: params.line,
      col: params.col,
    });
  },

  restartLspServer: async (
    hostId: string,
    cwd: string,
    serverId: string,
  ): Promise<LspServerStatus[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<LspServerStatus[]>("restart_lsp_server", {
      hostId,
      cwd,
      serverId,
    });
  },

  createConditionalTimer: async (params: {
    hostId: string;
    agentId: string;
    durationSeconds: number;
    prompt: string;
    condition: TimerCondition;
  }): Promise<ConditionalTimerRecord> => {
    if (!isTauriRuntime()) {
      throw new Error("Tauri runtime unavailable");
    }
    return invoke<ConditionalTimerRecord>("create_conditional_timer", {
      hostId: params.hostId,
      agentId: params.agentId,
      durationSeconds: params.durationSeconds,
      prompt: params.prompt,
      condition: params.condition,
    });
  },

  listConditionalTimers: async (
    hostId: string,
    agentId?: string | null,
    includeResolved = true,
  ): Promise<ConditionalTimerRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ConditionalTimerRecord[]>("list_conditional_timers", {
      hostId,
      agentId: agentId ?? null,
      includeResolved,
    });
  },

  cancelConditionalTimer: async (
    hostId: string,
    timerId: string,
  ): Promise<ConditionalTimerRecord> => {
    if (!isTauriRuntime()) {
      throw new Error("Tauri runtime unavailable");
    }
    return invoke<ConditionalTimerRecord>("cancel_conditional_timer", {
      hostId,
      timerId,
    });
  },

  fireConditionalTimerNow: async (
    hostId: string,
    timerId: string,
  ): Promise<ConditionalTimerRecord> => {
    if (!isTauriRuntime()) {
      throw new Error("Tauri runtime unavailable");
    }
    return invoke<ConditionalTimerRecord>("fire_conditional_timer_now", {
      hostId,
      timerId,
    });
  },

  getGuardianConfig: async (hostId: string): Promise<GuardianConfigSnapshotView> => {
    if (!isTauriRuntime()) {
      return {
        config: {
          enabled: true,
          auto_approve_max_risk: "low",
          use_ai_classifier_for_medium: true,
          classifier_timeout_ms: 4000,
          allow_read_only_tools: true,
          allow_workspace_tests: true,
          blocked_command_patterns: [],
          trusted_command_prefixes: [],
        },
        recent_audits: [],
      };
    }
    return invoke<GuardianConfigSnapshotView>("get_guardian_config", { hostId });
  },

  updateGuardianConfig: async (
    hostId: string,
    config: GuardianPolicyConfig,
  ): Promise<GuardianConfigSnapshotView> => {
    if (!isTauriRuntime()) {
      return { config, recent_audits: [] };
    }
    return invoke<GuardianConfigSnapshotView>("update_guardian_config", { hostId, config });
  },

  listImChannels: async (hostId: string): Promise<ImChannelsSnapshotView> => {
    if (!isTauriRuntime()) {
      return { channels: [], bindings: [], base_webhook_url: "http://127.0.0.1:7788" };
    }
    return invoke<ImChannelsSnapshotView>("list_im_channels", { hostId });
  },

  upsertImChannel: async (
    hostId: string,
    request: UpsertImChannelRequest,
  ): Promise<ImChannelsSnapshotView> => {
    if (!isTauriRuntime()) {
      return { channels: [], bindings: [], base_webhook_url: "http://127.0.0.1:7788" };
    }
    return invoke<ImChannelsSnapshotView>("upsert_im_channel", { hostId, request });
  },

  deleteImChannel: async (
    hostId: string,
    channelId: string,
  ): Promise<ImChannelsSnapshotView> => {
    if (!isTauriRuntime()) {
      return { channels: [], bindings: [], base_webhook_url: "http://127.0.0.1:7788" };
    }
    return invoke<ImChannelsSnapshotView>("delete_im_channel", { hostId, channelId });
  },

  testImChannel: async (
    hostId: string,
    channelId: string,
    targetChatId: string,
    message: string,
  ): Promise<ImChannelTestResultView> => {
    if (!isTauriRuntime()) {
      return { ok: false, detail: "Not running in Tauri runtime" };
    }
    return invoke<ImChannelTestResultView>("test_im_channel", {
      hostId,
      channelId,
      targetChatId,
      message,
    });
  },
  highlightWorkspaceCode: async (
    language: string,
    content: string,
    maxLines = 2500,
  ): Promise<HighlightedLineView[]> => {
    if (!isTauriRuntime()) {
      return content.split(/\r?\n/).map((line, i) => ({
        line_number: i + 1,
        tokens: [
          {
            text: line,
            fg_hex: "#E2E8F0",
            light_fg_hex: "#24292F",
            bold: false,
            italic: false,
          },
        ],
      }));
    }
    return invoke<HighlightedLineView[]>("highlight_workspace_code", {
      language,
      content,
      maxLines,
    });
  },

  askSideQuestion: async (
    params: {
      hostId: string;
      agentId: string | null;
      cwd: string;
      provider: ProviderKind;
      model: string | null;
      question: string;
      contextSummary: string;
    },
    onEvent: (ev: SideQuestionStreamEvent) => void,
  ): Promise<string> => {
    if (!isTauriRuntime()) {
      const id = `mock_${Date.now()}`;
      setTimeout(() => {
        onEvent({
          kind: "done",
          request_id: id,
          full_text: "Side questions require the Orbit desktop runtime.",
          error: null,
        });
      }, 150);
      return id;
    }
    const ch = new Channel<SideQuestionStreamEvent>();
    ch.onmessage = onEvent;
    return invoke<string>("ask_side_question", {
      hostId: params.hostId,
      agentId: params.agentId,
      cwd: params.cwd,
      provider: params.provider,
      model: params.model,
      question: params.question,
      contextSummary: params.contextSummary,
      onEvent: ch,
    });
  },

  parseMarkdownPreview: async (
    markdown: string,
    streaming: boolean,
  ): Promise<RichBlockView[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<RichBlockView[]>("parse_markdown_preview", { markdown, streaming });
  },

  openBrowserPopoutWindow: async (url: string, title?: string | null): Promise<void> => {
    if (!isTauriRuntime()) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    await invoke<void>("open_browser_popout_window", {
      url,
      title: title ?? null,
    });
  },

  openExternalBrowserUrl: async (url: string): Promise<void> => {
    if (!isTauriRuntime()) {
      window.open(url, "_blank", "noopener,noreferrer");
      return;
    }
    await invoke<void>("open_external_browser_url", { url });
  },

  openLocalFileExternally: async (
    hostId: string,
    cwd: string,
    path: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) {
      throw new Error("Desktop runtime required to open local files externally");
    }
    await invoke<void>("open_local_file_externally", { hostId, cwd, path });
  },

  stageHostAttachmentBytes: async (
    hostId: string,
    filename: string,
    bytes: Uint8Array,
  ): Promise<StagedAttachmentInfo> => {
    if (!isTauriRuntime()) {
      return { path: `/tmp/${filename}`, filename, size_bytes: bytes.byteLength };
    }
    return invoke<StagedAttachmentInfo>("stage_host_attachment_bytes", bytes, {
      headers: {
        "x-orbit-host-id": hostId,
        "x-orbit-filename": encodeURIComponent(filename),
      },
    });
  },

  stageHostAttachment: async (
    hostId: string,
    filename: string,
    dataB64: string,
  ): Promise<StagedAttachmentInfo> => {
    if (!isTauriRuntime()) {
      const approxBytes = Math.floor((dataB64.length * 3) / 4);
      return { path: `/tmp/${filename}`, filename, size_bytes: approxBytes };
    }
    return invoke<StagedAttachmentInfo>("stage_host_attachment", {
      hostId,
      filename,
      dataB64,
    });
  },

  readClipboardImages: async (): Promise<ClipboardImagePayload[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ClipboardImagePayload[]>("read_clipboard_images");
  },

  provisionSshRelay: async (params: {
    targetHostId: string;
    sshAlias: string;
    fallbackHostname: string;
    relayPort: number;
    applyToAllHosts?: boolean;
  }): Promise<string> => {
    return invoke<string>("provision_ssh_relay", params);
  },

  getAppVersion: async (): Promise<string> => {
    if (!isTauriRuntime()) return "0.9.9";
    return invoke<string>("get_app_version");
  },

  checkAppUpdate: async (): Promise<ReleaseUpdateInfo> => {
    return invoke<ReleaseUpdateInfo>("check_app_update");
  },

  performAppSelfUpdate: async (
    targetTag: string | null | undefined,
    onProgress: (ev: AppUpdateProgress) => void,
  ): Promise<void> => {
    const ch = new Channel<AppUpdateProgress>();
    ch.onmessage = onProgress;
    return invoke<void>("perform_app_self_update", {
      targetTag: targetTag ?? null,
      onProgress: ch,
    });
  },

  syncLocalDaemon: async (
    hostId?: string | null,
    force = false,
  ): Promise<LocalDaemonSyncResult> => {
    if (!isTauriRuntime()) {
      return { status: "skipped_dev", daemon_version: null, source: "dev" };
    }
    return invoke<LocalDaemonSyncResult>("sync_local_daemon", {
      hostId: hostId ?? null,
      force,
    });
  },

  upgradeHostDaemonOverWire: async (
    hostId: string,
    targetVersion?: string | null,
  ): Promise<string> => {
    if (!isTauriRuntime()) return "ok";
    return invoke<string>("upgrade_host_daemon_over_wire", {
      hostId,
      targetVersion: targetVersion ?? null,
    });
  },

  getCloudSyncStatus: async (): Promise<CloudSyncStatusView> => {
    if (!isTauriRuntime()) {
      return {
        state: "signed_out",
        provider: "github_gist",
        github_oauth_configured: false,
        github_client_id_override: null,
        account_login: null,
        account_name: null,
        account_avatar_url: null,
        gist_id: null,
        gist_html_url: null,
        remote_vault_exists: false,
        auto_sync_enabled: true,
        last_synced_at_ms: null,
        last_vault_revision: 0,
        synced_host_count: 0,
        synced_project_count: 0,
        last_error: null,
      };
    }
    return invoke<CloudSyncStatusView>("get_cloud_sync_status");
  },

  cloudSyncGeneratePassphrase: async (): Promise<string> => {
    if (!isTauriRuntime()) return "orbit-DEMO-KEY1-KEY2-KEY3-KEY4";
    return invoke<string>("cloud_sync_generate_passphrase");
  },

  cloudSyncSetGithubClientId: async (
    clientId: string | null,
  ): Promise<CloudSyncStatusView> => {
    return invoke<CloudSyncStatusView>("cloud_sync_set_github_client_id", {
      clientId,
    });
  },

  cloudSyncSetAutoSync: async (enabled: boolean): Promise<CloudSyncStatusView> => {
    return invoke<CloudSyncStatusView>("cloud_sync_set_auto_sync", { enabled });
  },

  cloudSyncStartGithubDeviceFlow: async (
    clientIdOverride?: string | null,
  ): Promise<GitHubDeviceFlowStartView> => {
    return invoke<GitHubDeviceFlowStartView>("cloud_sync_start_github_device_flow", {
      clientIdOverride: clientIdOverride ?? null,
    });
  },

  cloudSyncPollGithubDeviceFlow: async (
    deviceCode: string,
    clientIdOverride?: string | null,
  ): Promise<CloudSyncDevicePollView> => {
    return invoke<CloudSyncDevicePollView>("cloud_sync_poll_github_device_flow", {
      deviceCode,
      clientIdOverride: clientIdOverride ?? null,
    });
  },

  cloudSyncLoginWithToken: async (token: string): Promise<CloudSyncStatusView> => {
    return invoke<CloudSyncStatusView>("cloud_sync_login_with_token", { token });
  },

  cloudSyncUnlockWithPassphrase: async (passphrase: string): Promise<CloudSyncResultView> => {
    return invoke<CloudSyncResultView>("cloud_sync_unlock_with_passphrase", { passphrase });
  },

  cloudSyncNow: async (): Promise<CloudSyncResultView> => {
    return invoke<CloudSyncResultView>("cloud_sync_now");
  },

  cloudSyncTriggerBackground: async (): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("cloud_sync_trigger_background").catch(() => undefined);
  },

  cloudSyncLogout: async (clearLocalHosts = false): Promise<CloudSyncStatusView> => {
    return invoke<CloudSyncStatusView>("cloud_sync_logout", { clearLocalHosts });
  },

  cloudSyncGetPreferences: async (): Promise<Record<string, unknown>> => {
    if (!isTauriRuntime()) return {};
    return invoke<Record<string, unknown>>("cloud_sync_get_preferences");
  },

  cloudSyncUpdatePreferences: async (patch: Record<string, unknown>): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("cloud_sync_update_preferences", { patch }).catch(() => undefined);
  },

  searchMessages: async (query: string, limit = 20): Promise<ConversationSearchHit[]> => {
    if (!isTauriRuntime()) return [];
    const trimmed = query.trim();
    if (!trimmed) return [];
    return invoke<ConversationSearchHit[]>("search_messages", {
      query: trimmed,
      limit,
    });
  },

  exportAgentTranscript: async (
    hostId: string,
    agentId: string,
    options?: Partial<ExportOptions>,
  ): Promise<ExportedTranscript> => {
    if (!isTauriRuntime()) {
      throw new Error("Transcript export requires the Orbit desktop runtime");
    }
    return invoke<ExportedTranscript>("export_agent_transcript", {
      hostId,
      agentId,
      options: options
        ? {
            format: options.format ?? "markdown",
            include_thinking: options.include_thinking ?? true,
            include_tool_calls: options.include_tool_calls ?? true,
            include_timestamps: options.include_timestamps ?? true,
          }
        : null,
    });
  },

  saveExportedTranscript: async (
    hostId: string,
    agentId: string,
    options?: Partial<ExportOptions>,
    initialDir?: string | null,
  ): Promise<string | null> => {
    if (!isTauriRuntime()) return null;
    return invoke<string | null>("save_exported_transcript", {
      hostId,
      agentId,
      options: options
        ? {
            format: options.format ?? "markdown",
            include_thinking: options.include_thinking ?? true,
            include_tool_calls: options.include_tool_calls ?? true,
            include_timestamps: options.include_timestamps ?? true,
          }
        : null,
      initialDir: initialDir ?? null,
    });
  },

  listAgentCodeBlocks: async (
    hostId: string,
    agentId: string,
  ): Promise<ExtractedCodeBlock[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ExtractedCodeBlock[]>("list_agent_code_blocks", {
      hostId,
      agentId,
    });
  },

  getUsageInsights: async (
    hostId: string,
    days = 7,
  ): Promise<UsageInsightsSnapshot> => {
    if (!isTauriRuntime()) {
      return {
        days,
        total_cost_usd: 0,
        total_cache_savings_usd: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_cache_read_tokens: 0,
        total_turns: 0,
        daily: [],
        by_model: [],
        by_project: [],
        forecasts: [],
      };
    }
    return invoke<UsageInsightsSnapshot>("get_usage_insights", {
      hostId,
      days,
    });
  },

  getContextOccupancy: async (
    hostId: string,
    agentId: string,
  ): Promise<ContextOccupancyBreakdown> => {
    if (!isTauriRuntime()) {
      return {
        agent_id: agentId,
        model: "default",
        context_window_tokens: 200_000,
        total_used_tokens: 0,
        occupancy_percent: 0,
        segments: [
          {
            category: "free",
            label: "Free",
            tokens: 200_000,
            percent: 100,
          },
        ],
      };
    }
    return invoke<ContextOccupancyBreakdown>("get_context_occupancy", {
      hostId,
      agentId,
    });
  },

  listLifecycleHooks: async (
    hostId: string,
    cwd: string | null,
  ): Promise<LifecycleHooksSnapshotView> => {
    if (!isTauriRuntime()) {
      return { hooks: [], recent_executions: [], project_hooks_trusted: false };
    }
    return invoke<LifecycleHooksSnapshotView>("list_lifecycle_hooks", {
      hostId,
      cwd,
    });
  },

  upsertLifecycleHook: async (
    hostId: string,
    cwd: string | null,
    hook: LifecycleHookRule,
  ): Promise<LifecycleHooksSnapshotView> => {
    if (!isTauriRuntime()) {
      return { hooks: [hook], recent_executions: [], project_hooks_trusted: false };
    }
    return invoke<LifecycleHooksSnapshotView>("upsert_lifecycle_hook", {
      hostId,
      cwd,
      hook,
    });
  },

  deleteLifecycleHook: async (
    hostId: string,
    cwd: string | null,
    hookId: string,
    scope: HookScope,
  ): Promise<LifecycleHooksSnapshotView> => {
    if (!isTauriRuntime()) {
      return { hooks: [], recent_executions: [], project_hooks_trusted: false };
    }
    return invoke<LifecycleHooksSnapshotView>("delete_lifecycle_hook", {
      hostId,
      cwd,
      hookId,
      scope,
    });
  },

  setProjectHooksTrust: async (
    hostId: string,
    cwd: string,
    trusted: boolean,
  ): Promise<LifecycleHooksSnapshotView> => {
    if (!isTauriRuntime()) {
      return { hooks: [], recent_executions: [], project_hooks_trusted: trusted };
    }
    return invoke<LifecycleHooksSnapshotView>("set_project_hooks_trust", {
      hostId,
      cwd,
      trusted,
    });
  },

  testLifecycleHook: async (
    hostId: string,
    cwd: string | null,
    hookId: string,
    samplePayload: Record<string, unknown>,
  ): Promise<HookExecutionRecord> => {
    if (!isTauriRuntime()) {
      throw new Error("Hook testing requires the Orbit desktop runtime");
    }
    return invoke<HookExecutionRecord>("test_lifecycle_hook", {
      hostId,
      cwd,
      hookId,
      samplePayload,
    });
  },

  startGoalRun: async (req: {
    hostId: string;
    agentId: string;
    objective: string;
    criteria?: string[];
    verifyCommand?: string | null;
    maxIterations?: number | null;
    maxDurationSecs?: number | null;
  }): Promise<GoalRunRecord> => {
    return invoke<GoalRunRecord>("start_goal_run", {
      hostId: req.hostId,
      agentId: req.agentId,
      objective: req.objective,
      criteria: req.criteria ?? null,
      verifyCommand: req.verifyCommand?.trim() ? req.verifyCommand.trim() : null,
      maxIterations: req.maxIterations ?? null,
      maxDurationSecs: req.maxDurationSecs ?? null,
    });
  },

  listGoalRuns: async (hostId: string, agentId?: string | null): Promise<GoalRunRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<GoalRunRecord[]>("list_goal_runs", {
      hostId,
      agentId: agentId ?? null,
    });
  },

  controlGoalRun: async (
    hostId: string,
    goalId: string,
    action: GoalControlAction,
  ): Promise<GoalRunRecord> => {
    return invoke<GoalRunRecord>("control_goal_run", {
      hostId,
      goalId,
      action,
    });
  },

  startMonitorWatch: async (req: {
    hostId: string;
    agentId: string;
    name: string;
    kind: MonitorKind;
    expiresInSecs?: number | null;
  }): Promise<MonitorWatchRecord> => {
    return invoke<MonitorWatchRecord>("start_monitor_watch", {
      hostId: req.hostId,
      agentId: req.agentId,
      name: req.name,
      kind: req.kind,
      expiresInSecs: req.expiresInSecs ?? null,
    });
  },

  listMonitorWatches: async (
    hostId: string,
    agentId?: string | null,
  ): Promise<MonitorWatchRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<MonitorWatchRecord[]>("list_monitor_watches", {
      hostId,
      agentId: agentId ?? null,
    });
  },

  stopMonitorWatch: async (hostId: string, monitorId: string): Promise<MonitorWatchRecord> => {
    return invoke<MonitorWatchRecord>("stop_monitor_watch", {
      hostId,
      monitorId,
    });
  },

  listEventTriggers: async (hostId: string): Promise<EventTriggersSnapshotView> => {
    if (!isTauriRuntime()) {
      return { triggers: [], recent_deliveries: [], base_webhook_url: "http://127.0.0.1:6868" };
    }
    return invoke<EventTriggersSnapshotView>("list_event_triggers", { hostId });
  },

  upsertEventTrigger: async (
    req: UpsertEventTriggerRequest,
  ): Promise<EventTriggersSnapshotView> => {
    if (!isTauriRuntime()) {
      return { triggers: [], recent_deliveries: [], base_webhook_url: "http://127.0.0.1:6868" };
    }
    return invoke<EventTriggersSnapshotView>("upsert_event_trigger", { req });
  },

  deleteEventTrigger: async (
    hostId: string,
    triggerId: string,
  ): Promise<EventTriggersSnapshotView> => {
    if (!isTauriRuntime()) {
      return { triggers: [], recent_deliveries: [], base_webhook_url: "http://127.0.0.1:6868" };
    }
    return invoke<EventTriggersSnapshotView>("delete_event_trigger", {
      hostId,
      triggerId,
    });
  },

  testEventTrigger: async (
    hostId: string,
    triggerId: string,
    sampleEventType: string,
    samplePayload: Record<string, unknown>,
  ): Promise<EventTriggerDelivery> => {
    if (!isTauriRuntime()) {
      throw new Error("Trigger testing requires the Orbit desktop runtime");
    }
    return invoke<EventTriggerDelivery>("test_event_trigger", {
      hostId,
      triggerId,
      sampleEventType,
      samplePayload,
    });
  },

  listBrowserSessions: async (hostId: string): Promise<BrowserSessionsSnapshotView> => {
    if (!isTauriRuntime()) {
      return {
        host_id: hostId,
        sessions: [],
        policy: {
          enabled: true,
          headless: true,
          allow_localhost: true,
          allowed_domains: [],
          blocked_domains: [],
          block_password_inputs: true,
          max_sessions: 4,
        },
        chrome_binary: null,
      };
    }
    return invoke<BrowserSessionsSnapshotView>("list_browser_sessions", { hostId });
  },

  createBrowserSession: async (req: {
    hostId: string;
    agentId?: string | null;
    initialUrl?: string | null;
    headless?: boolean | null;
    viewportWidth?: number | null;
    viewportHeight?: number | null;
  }): Promise<BrowserSessionSummary> => {
    return invoke<BrowserSessionSummary>("create_browser_session", {
      hostId: req.hostId,
      agentId: req.agentId ?? null,
      initialUrl: req.initialUrl ?? null,
      headless: req.headless ?? null,
      viewportWidth: req.viewportWidth ?? null,
      viewportHeight: req.viewportHeight ?? null,
    });
  },

  executeBrowserAction: async (req: {
    hostId: string;
    sessionId: string;
    action: BrowserActionRequest;
  }): Promise<BrowserActionResponseView> => {
    return invoke<BrowserActionResponseView>("execute_browser_action", {
      hostId: req.hostId,
      sessionId: req.sessionId,
      action: req.action,
    });
  },

  closeBrowserSession: async (
    hostId: string,
    sessionId: string,
  ): Promise<BrowserSessionsSnapshotView> => {
    return invoke<BrowserSessionsSnapshotView>("close_browser_session", {
      hostId,
      sessionId,
    });
  },

  updateBrowserPolicy: async (
    hostId: string,
    policy: BrowserPolicyConfig,
  ): Promise<BrowserSessionsSnapshotView> => {
    return invoke<BrowserSessionsSnapshotView>("update_browser_policy", {
      hostId,
      policy,
    });
  },

  startBatchFanout: async (req: {
    hostId: string;
    name: string;
    cwd: string;
    csvContent: string;
    promptTemplate: string;
    provider: ProviderKind;
    model?: string | null;
    mode: SessionMode;
    maxConcurrency?: number | null;
    isolatedWorktree?: boolean;
  }): Promise<WorkflowRunRecord> => {
    return invoke<WorkflowRunRecord>("start_batch_fanout", {
      hostId: req.hostId,
      name: req.name,
      cwd: req.cwd,
      csvContent: req.csvContent,
      promptTemplate: req.promptTemplate,
      provider: req.provider,
      model: req.model?.trim() ? req.model.trim() : null,
      mode: req.mode,
      maxConcurrency: req.maxConcurrency ?? null,
      isolatedWorktree: req.isolatedWorktree ?? false,
    });
  },

  startDagWorkflow: async (req: {
    hostId: string;
    name: string;
    cwd: string;
    steps: DagStepSpec[];
    defaultProvider: ProviderKind;
    defaultMode: SessionMode;
    maxConcurrency?: number | null;
  }): Promise<WorkflowRunRecord> => {
    return invoke<WorkflowRunRecord>("start_dag_workflow", {
      hostId: req.hostId,
      name: req.name,
      cwd: req.cwd,
      steps: req.steps,
      defaultProvider: req.defaultProvider,
      defaultMode: req.defaultMode,
      maxConcurrency: req.maxConcurrency ?? null,
    });
  },

  startBestOfN: async (req: {
    hostId: string;
    name: string;
    cwd: string;
    prompt: string;
    candidates: BestOfNCandidateSpec[];
    mode: SessionMode;
    verifyCommand?: string | null;
  }): Promise<WorkflowRunRecord> => {
    return invoke<WorkflowRunRecord>("start_best_of_n", {
      hostId: req.hostId,
      name: req.name,
      cwd: req.cwd,
      prompt: req.prompt,
      candidates: req.candidates,
      mode: req.mode,
      verifyCommand: req.verifyCommand?.trim() ? req.verifyCommand.trim() : null,
    });
  },

  listWorkflowRuns: async (
    hostId: string,
    includeCompleted = true,
  ): Promise<WorkflowRunRecord[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<WorkflowRunRecord[]>("list_workflow_runs", {
      hostId,
      includeCompleted,
    });
  },

  cancelWorkflowRun: async (hostId: string, runId: string): Promise<WorkflowRunRecord> => {
    return invoke<WorkflowRunRecord>("cancel_workflow_run", {
      hostId,
      runId,
    });
  },

  selectBestOfNWinner: async (
    hostId: string,
    runId: string,
    winnerStepId: string,
    applyToWorkspace = true,
  ): Promise<WorkflowRunRecord> => {
    return invoke<WorkflowRunRecord>("select_best_of_n_winner", {
      hostId,
      runId,
      winnerStepId,
      applyToWorkspace,
    });
  },

  // --- Enhanced Desktop Notifications & Native Mobile Background Push ---

  focusMainWindow: async (): Promise<void> => {
    if (!isTauriRuntime()) {
      if (typeof window !== "undefined") window.focus();
      return;
    }
    await invoke<void>("focus_main_window");
  },

  updateTrayStatus: async (runningCount: number, pendingApprovals: number): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("update_tray_status", {
      runningCount: Math.max(0, Math.floor(runningCount)),
      pendingApprovals: Math.max(0, Math.floor(pendingApprovals)),
    });
  },

  onTrayNewConversation: async (handler: () => void): Promise<() => void> => {
    if (!isTauriRuntime()) return () => {};
    return listen("orbit://tray-new-conversation", () => {
      handler();
    });
  },

  probeLocalProviderReadiness: async (): Promise<ProviderReadinessInfo[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<ProviderReadinessInfo[]>("probe_local_provider_readiness");
  },

  renderQrSvg: async (content: string): Promise<string> => {
    if (!isTauriRuntime()) {
      throw new Error("QR rendering requires the Orbit Desktop runtime");
    }
    return invoke<string>("render_qr_svg", { content });
  },

  listPushDevices: async (hostId: string): Promise<PushDevicesSnapshotView> => {
    if (!isTauriRuntime()) {
      return {
        devices: [],
        preferences: {
          enabled: true,
          notify_permission_required: true,
          notify_turn_completed: true,
          notify_agent_error: true,
          notify_goal_finished: true,
          notify_timer_fired: true,
          include_message_preview: false,
          quiet_hours_enabled: false,
          quiet_start_hour: 23,
          quiet_end_hour: 7,
          push_gateway_url: null,
          has_gateway_auth_token: false,
        },
      };
    }
    return invoke<PushDevicesSnapshotView>("list_push_devices", { hostId });
  },

  registerPushDevice: async (
    hostId: string,
    request: RegisterPushDeviceRequest,
  ): Promise<PushDevicesSnapshotView> => {
    return invoke<PushDevicesSnapshotView>("register_push_device", { hostId, request });
  },

  unregisterPushDevice: async (
    hostId: string,
    deviceId: string,
  ): Promise<PushDevicesSnapshotView> => {
    return invoke<PushDevicesSnapshotView>("unregister_push_device", { hostId, deviceId });
  },

  updatePushPreferences: async (
    hostId: string,
    request: UpdatePushPreferencesRequest,
  ): Promise<PushDevicesSnapshotView> => {
    return invoke<PushDevicesSnapshotView>("update_push_preferences", { hostId, request });
  },

  testPushNotification: async (
    hostId: string,
    deviceId?: string | null,
  ): Promise<PushTestResultView> => {
    return invoke<PushTestResultView>("test_push_notification", {
      hostId,
      deviceId: deviceId ?? null,
    });
  },
};
