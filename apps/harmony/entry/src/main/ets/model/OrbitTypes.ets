export interface CoreNotificationDto {
  kind: string;
  revision: number;
  host_id?: string;
  agent_id?: string;
  cwd?: string;
  slot?: number;
}

export interface ClientProjectDto {
  project_id: string;
  host_id: string;
  name: string;
  directories: string[];
  updated_at_ms: number;
}

export interface DirectoryEntryDto {
  name: string;
  path: string;
  is_git_repo: boolean;
}

export interface DirectoryListingDto {
  current_path: string;
  parent_path: string | null;
  entries: DirectoryEntryDto[];
}

export interface DiscoveredProjectDto {
  path: string;
  name: string;
  git_branch: string | null;
  is_git_repo: boolean;
  last_modified_unix: number;
}

export interface LaunchpadProjectDto {
  id: string;
  host_id?: string;
  name: string;
  path: string;
  branch: string | null;
}

export interface HostCardDto {
  host_id: string;
  display_name: string;
  hostname: string;
  custom_name: string | null;
  ssh_alias: string | null;
  route_mode: string;
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
  connection_state: string;
  last_error: string | null;
  available_providers: string[];
  workspace_count: number;
  agent_count: number;
  pending_approval_count: number;
  supports_server_queue?: boolean;
  is_local?: boolean;
}

export interface FleetOverviewDto {
  revision: number;
  total_pending_approvals: number;
  hosts: HostCardDto[];
  projects: ClientProjectDto[];
  project_order?: string[];
}

export interface CustomSessionModeDto {
  custom?: string;
}

export interface AgentStatusDto {
  state: string;
  turn_id?: string;
  source?: string;
  request_id?: string;
  message?: string;
  recoverable?: boolean;
}

export interface QueuedPromptDto {
  queue_id: string;
  display_text: string;
  badges?: PromptBadgeDto[];
  created_at?: string;
}

export interface AgentCardDto {
  agent_id: string;
  workspace_id: string;
  cwd: string;
  title: string;
  provider: string;
  provider_label: string;
  model: string | null;
  effort: string | null;
  fast_mode?: boolean;
  mode: string | CustomSessionModeDto;
  status: AgentStatusDto;
  status_label: string;
  is_running: boolean;
  has_background_tasks?: boolean;
  has_armed_timers?: boolean;
  is_awaiting_permission: boolean;
  parent_agent_id: string | null;
  parent_host_id?: string | null;
  archived_at_rfc3339?: string | null;
  handle?: string | null;
  latest_seq: number;
  created_at_rfc3339?: string | null;
  updated_at_rfc3339: string;
  is_pinned?: boolean;
  seen_seq?: number;
  is_unread?: boolean;
  queued_prompts?: QueuedPromptDto[];
}

export interface ThinkingOptionSpecDto {
  effort: string;
  label: string;
  description?: string | null;
  is_default: boolean;
}

export interface ProviderModelSpecDto {
  id: string;
  label: string;
  description: string;
  is_default: boolean;
  default_effort: string | null;
  thinking_options: ThinkingOptionSpecDto[];
}

export interface ProviderModelCatalogDto {
  provider: string;
  models: ProviderModelSpecDto[];
  configured_default_model: string | null;
  configured_default_effort: string | null;
}

export interface ModelOptionDto {
  id: string;
  label: string;
  description: string | null;
  is_default: boolean;
}

export interface ModelsByProviderDto {
  antigravity?: ModelOptionDto[];
  codex?: ModelOptionDto[];
  claude_code?: ModelOptionDto[];
}

export interface WorkspaceDto {
  workspace_id: string;
  title: string;
  cwd: string;
  git_branch: string | null;
  is_worktree: boolean;
  updated_at?: string;
  archived_at?: string | null;
  mission_goal?: string | null;
  board_revision?: number;
}

export interface QuotaWindowDto {
  label: string;
  used_percent: number;
  resets_at: string | null;
}

export interface ProviderQuotaDto {
  provider: string;
  account_label: string | null;
  windows: QuotaWindowDto[];
  credits_remaining: number | null;
  error: string | null;
  updated_at: string;
}

export interface TerminalSummaryDto {
  terminal_id: string;
  cwd: string;
  title: string;
  cols: number;
  rows: number;
  created_at: string;
}

export interface ScheduleNewAgentConfigDto {
  cwd: string;
  workspace_id?: string | null;
  provider: string;
  model?: string | null;
  effort?: string | null;
  fast_mode?: boolean;
  mode: string | CustomSessionModeDto;
  worktree?: boolean;
  archive_on_finish?: boolean;
  background?: boolean;
}

export interface ScheduleTargetDto {
  type: string; // 'new_agent' | 'existing_agent'
  cwd?: string;
  provider?: string;
  agent_id?: string;
}

export interface ScheduleSpecDto {
  schedule_id: string;
  kind: string; // 'cron' | 'heartbeat'
  name: string;
  cron_expr: string;
  timezone?: string | null;
  prompt: string;
  status: string; // 'active' | 'paused' | 'completed'
  target: ScheduleTargetDto;
  max_runs?: number | null;
  run_count: number;
  last_run_at?: string | null;
  next_run_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface ScheduleRunRecordDto {
  run_id: string;
  schedule_id: string;
  agent_id?: string | null;
  workspace_id?: string | null;
  status: string; // 'running' | 'succeeded' | 'failed' | 'skipped'
  summary?: string | null;
  error?: string | null;
  started_at: string;
  finished_at?: string | null;
}

export interface CreateSchedulePayloadDto {
  host_id: string;
  name: string;
  cron_expr: string;
  prompt: string;
  cwd: string;
  provider: string;
  model: string | null;
  effort: string | null;
  mode: string;
  worktree: boolean;
  archive_on_finish: boolean;
  max_runs: number | null;
}

export interface TimerConditionDto {
  kind: string; // 'never' | 'any_message' | 'sender' | 'all_subagents_settled' | 'file_changed'
  sender_id?: string;
  path?: string;
}

export interface ConditionalTimerRecordDto {
  timer_id: string;
  agent_id: string;
  workspace_id: string;
  prompt: string;
  duration_seconds: number;
  condition: TimerConditionDto;
  state: string; // 'armed' | 'fired' | 'canceled_early' | 'canceled_manual'
  created_at_ms: number;
  fire_at_ms: number;
  resolved_at_ms?: number | null;
  resolved_reason?: string | null;
}

export interface GoalCriterionDto {
  id: string;
  label: string;
  passed: boolean;
}

export interface GoalRunRecordDto {
  goal_id: string;
  agent_id: string;
  workspace_id: string;
  objective: string;
  criteria: GoalCriterionDto[];
  verify_command?: string | null;
  max_iterations: number;
  current_iteration: number;
  max_duration_secs: number;
  status: string; // 'running' | 'verifying' | 'paused' | 'succeeded' | 'failed' | 'canceled'
  last_verify_exit_code?: number | null;
  last_verify_output?: string | null;
  pause_reason?: string | null;
  paused_by_user?: boolean;
  active_duration_secs?: number;
  started_at_ms: number;
  updated_at_ms: number;
  finished_at_ms?: number | null;
}

export interface ControlGoalRunRequestDto {
  host_id: string;
  goal_id: string;
  action: string; // 'pause' | 'resume' | 'cancel' | 'verify_now'
}

export interface MonitorKindDto {
  kind: string; // 'command_stream' | 'log_file_tail' | 'http_health'
  command?: string;
  path?: string;
  pattern?: string | null;
  url?: string;
}

export interface MonitorWatchRecordDto {
  monitor_id: string;
  agent_id: string;
  name: string;
  kind: MonitorKindDto;
  status: string; // 'active' | 'paused' | 'triggered' | 'stopped' | 'error'
  match_count: number;
  last_event_preview?: string | null;
  last_triggered_at_ms?: number | null;
  created_at_ms: number;
}

export interface StopMonitorWatchRequestDto {
  host_id: string;
  monitor_id: string;
}

export interface WorkflowStepRecordDto {
  run_id: string;
  step_id: string;
  name: string;
  status: string;
  agent_id?: string | null;
  provider: string;
  model?: string | null;
  output_preview?: string | null;
  diff_additions: number;
  diff_deletions: number;
  diff_files: number;
  verify_passed?: boolean | null;
  error?: string | null;
  is_winner: boolean;
}

export interface WorkflowRunRecordDto {
  run_id: string;
  name: string;
  kind: string; // 'batch_csv' | 'dag' | 'best_of_n'
  workspace_cwd: string;
  owner_agent_id?: string | null;
  status: string; // 'pending' | 'running' | 'succeeded' | 'partial_failure' | 'failed' | 'canceled'
  max_concurrency: number;
  verify_command?: string | null;
  winner_step_id?: string | null;
  steps: WorkflowStepRecordDto[];
  created_at_ms: number;
  updated_at_ms: number;
  finished_at_ms?: number | null;
}

export interface CancelWorkflowRunRequestDto {
  host_id: string;
  run_id: string;
}

export interface WorkspaceBoardSnapshotDto {
  workspace_id: string;
  content: string;
  revision: number;
  updated_by?: string | null;
  updated_at: string;
  board_path?: string | null;
}

export interface WorkspaceEventRecordDto {
  seq: number;
  workspace_id: string;
  agent_id?: string | null;
  agent_handle?: string | null;
  kind: string;
  summary: string;
  file_path?: string | null;
  created_at: string;
}

export interface SpawnWorkspacePeerRequestDto {
  host_id: string;
  workspace_id: string;
  cwd: string;
  provider: string;
  model: string | null;
  effort: string | null;
  mode: string;
  initial_prompt: string;
  handle: string | null;
}

export interface CreateSessionRequestDto {
  host_id: string;
  cwd: string;
  extra_dirs: string[];
  provider: string;
  acp_agent_id: string | null;
  model: string | null;
  effort: string | null;
  fast_mode: boolean;
  mode: string;
  initial_prompt: string;
  initial_display_prompt: string | null;
  initial_badges: PromptBadgeDto[];
  worktree_branch: string | null;
  workspace_id: string | null;
  handle: string | null;
  auto_cluster: boolean | null;
}

export interface ForkTargetDto {
  kind: string; // 'same_directory' | 'new_worktree'
  branch?: string | null;
  base_ref?: string | null;
}

export interface ForkAgentRequestDto {
  host_id: string;
  source_agent_id: string;
  up_to_seq: number | null;
  target: ForkTargetDto;
  title: string | null;
  initial_prompt: string | null;
}

export interface ForkAgentResultDto {
  agent: AgentCardDto;
  workspace?: WorkspaceDto | null;
}

export interface ExportOptionsDto {
  format: string; // 'markdown' | 'json' | 'html'
  include_thinking: boolean;
  include_tool_calls: boolean;
  include_timestamps: boolean;
}

export interface ExportedTranscriptDto {
  suggested_filename: string;
  mime_type: string;
  content: string;
  turn_count: number;
  byte_len: number;
}

export interface CloudSyncStatusDto {
  state: string; // 'signed_out' | 'locked' | 'ready' | 'syncing' | 'error'
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
  instant_unlock_enabled?: boolean;
  last_synced_at_ms: number | null;
  last_vault_revision: number;
  synced_host_count: number;
  synced_project_count: number;
  last_error: string | null;
}

export interface GithubDeviceFlowStartDto {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export interface GithubDeviceFlowPollDto {
  status: string; // 'pending' | 'slow_down' | 'expired' | 'denied' | 'authorized'
  interval_secs?: number;
  cloud_status?: CloudSyncStatusDto | null;
}

export interface CloudSyncResultDto {
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

export interface McpToolPolicyDto {
  enabled: boolean;
  disabled_tools: string[];
  enabled_packs?: string[] | null;
  disabled_packs?: string[];
}

export interface AgentProfileDto {
  profile_id: string;
  name: string;
  provider: string;
  acp_agent_id?: string | null;
  model?: string | null;
  effort?: string | null;
  fast_mode: boolean;
  mode: string | CustomSessionModeDto;
  when_to_use: string;
  system_prompt_append?: string | null;
  mcp_policy: McpToolPolicyDto;
  created_at: string;
  updated_at: string;
}

export interface OrchestrationConfigDto {
  mcp_inject_enabled: boolean;
  idle_cleanup_enabled: boolean;
  idle_timeout_secs: number;
  resume_on_startup: boolean;
  resume_prompt: string;
  installed_skills: string[];
  auto_cluster_enabled: boolean;
  keep_awake_while_running: boolean;
}

export interface OrchestrationSkillInfoDto {
  id: string;
  name: string;
  slash_command: string;
  description: string;
  installed: boolean;
  installed_targets: string[];
  outdated: boolean;
}

export interface ManagedSkillRecordDto {
  id: string;
  slug: string;
  name: string;
  description: string;
  source: string;
  path: string;
  enabled: boolean;
  editable: boolean;
  triggers: string[];
  allowed_tools: string[];
  has_scripts: boolean;
  has_references: boolean;
  approx_tokens: number;
  updated_at_ms: number;
  argument_hint?: string | null;
  is_command: boolean;
}

export interface KeepAwakeStatusDto {
  enabled: boolean;
  active: boolean;
  active_agents: number;
  backend: string;
}

export interface WorkspaceScriptInfoDto {
  workspace_id: string;
  name: string;
  command: string;
  kind: string;
  status: string;
  terminal_id?: string | null;
  port?: number | null;
  proxy_url?: string | null;
  last_error?: string | null;
}

export interface RouterRecommendationDto {
  decision: string;
  reason: string;
  created_at: string;
}

export interface GuardianAssessmentDto {
  risk_level: string;
  decision: string;
  confidence: number;
  classifier_tier: string;
  reason: string;
  signals: string[];
  latency_ms: number;
}

export interface PlanApprovalPayloadDto {
  plan_markdown: string;
  plan_title?: string | null;
  plan_file_path?: string | null;
  summary?: string | null;
  plan_file_backed?: boolean;
  allowed_prompts?: string[];
  source?: string | null;
}

export interface PendingPermissionSummaryDto {
  agent_id: string;
  agent_title: string;
  workspace_id: string;
  request_id: string;
  title: string;
  detail: ToolCallDetailDto;
  allowed_decisions: string[];
  guardian_assessment?: GuardianAssessmentDto | null;
  plan_approval?: PlanApprovalPayloadDto | null;
  human_only: boolean;
  router_recommendation?: RouterRecommendationDto | null;
  created_at: string;
}

export interface ProviderSubagentDescriptorDto {
  subagent_id: string;
  parent_agent_id: string;
  parent_subagent_id?: string | null;
  title: string;
  subtitle?: string | null;
  model?: string | null;
  status: string;
  created_at: string;
  updated_at: string;
}

export interface NestedGitRepoDto {
  path: string;
  relative_path: string;
  name: string;
  git_branch?: string | null;
}

export interface GitReposInspectionDto {
  path: string;
  is_git_repo: boolean;
  git_branch?: string | null;
  nested_git_repos: NestedGitRepoDto[];
}

export interface WorkspaceOrganizeEventDto {
  request_id?: string | null;
  project_root: string;
  phase: string;
  message: string;
  created_workspaces: number;
  grouped_agents: number;
  summarized_agents: number;
}

export interface HostProjectEntryDto {
  project_id: string;
  name: string;
  directories: string[];
  sort_order: number;
  hidden: boolean;
  updated_at_ms: number;
}

export interface HostSyncStateDto {
  projects: HostProjectEntryDto[];
  project_order: string[];
  project_sort_mode?: string | null;
  updated_at_ms: number;
}

export interface AcpAgentSpecDto {
  id: string;
  name: string;
  description: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  install_hint?: string | null;
  homepage?: string | null;
  version?: string | null;
  source: string;
  enabled: boolean;
}

export interface AcpAgentAvailabilityDto {
  spec: AcpAgentSpecDto;
  installed: boolean;
  resolved_path?: string | null;
}

export interface ArtifactSummaryDto {
  artifact_id: string;
  workspace_id: string;
  agent_id?: string | null;
  title: string;
  kind: string;
  language?: string | null;
  mime_type?: string | null;
  resource_uri?: string | null;
  mcp_server_name?: string | null;
  version: number;
  byte_len: number;
  created_at: string;
  updated_at: string;
}

export interface HostDetailDto {
  revision: number;
  host: HostCardDto;
  discovered_projects: DiscoveredProjectDto[];
  directory_listing: DirectoryListingDto | null;
  models_by_provider: ModelsByProviderDto;
  workspaces: WorkspaceDto[];
  agents: AgentCardDto[];
  last_created_agent_id: string | null;
  replaced_optimistic_agents?: Record<string, string>;
  quotas: ProviderQuotaDto[];
  provider_catalogs: ProviderModelCatalogDto[];
  terminals: TerminalSummaryDto[];
  active_terminal_id: string | null;
  active_terminal_slot: number;
  profiles?: AgentProfileDto[];
  schedules?: ScheduleSpecDto[];
  conditional_timers?: ConditionalTimerRecordDto[];
  connected_hosts?: ConnectedHostSummaryDto[];
  orchestration_config?: OrchestrationConfigDto;
  orchestration_skills?: OrchestrationSkillInfoDto[];
  managed_skills?: ManagedSkillRecordDto[];
  keep_awake_status?: KeepAwakeStatusDto | null;
  workspace_scripts?: Record<string, WorkspaceScriptInfoDto[]>;
  pending_permissions?: PendingPermissionSummaryDto[];
  provider_subagents?: Record<string, ProviderSubagentDescriptorDto[]>;
  git_repos_by_cwd?: Record<string, GitReposInspectionDto>;
  workspace_boards?: Record<string, WorkspaceBoardSnapshotDto>;
  workspace_events?: Record<string, WorkspaceEventRecordDto[]>;
  workspace_organize_status?: Record<string, WorkspaceOrganizeEventDto>;
  host_sync_state?: HostSyncStateDto;
  acp_agents?: AcpAgentAvailabilityDto[];
  artifacts?: ArtifactSummaryDto[];
  goal_runs?: GoalRunRecordDto[];
  monitor_watches?: MonitorWatchRecordDto[];
  workflow_runs?: WorkflowRunRecordDto[];
}

export interface PromptBadgeDto {
  kind: string;
  label: string;
  path?: string | null;
}

export interface InlineSpanDto {
  text: string;
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  inline_code: boolean;
  link_url: string | null;
}

export interface HighlightedTokenDto {
  text: string;
  fg_hex: string;
  bold: boolean;
  italic: boolean;
}

export interface HighlightedLineDto {
  line_number: number;
  tokens: HighlightedTokenDto[];
}

export interface TableCellDto {
  spans: InlineSpanDto[];
}

export interface TableRowDto {
  cells: TableCellDto[];
}

export interface RichBlockDto {
  kind: string;
  level?: number;
  spans?: InlineSpanDto[];
  language?: string;
  raw_code?: string;
  lines?: HighlightedLineDto[];
  ordered?: boolean;
  index?: number;
  checked?: boolean | null;
  alignments?: string[];
  headers?: (string | TableCellDto)[];
  rows?: (string[] | TableRowDto)[];
}

export interface DiffLineDto {
  kind: string;
  old_line: number | null;
  new_line: number | null;
  content: string;
}

export interface DiffFileDto {
  path: string;
  additions: number;
  deletions: number;
  lines: DiffLineDto[];
}

export interface ToolCallDetailDto {
  kind: string;
  command?: string;
  cwd?: string | null;
  exit_code?: number | null;
  output_preview?: string | null;
  path?: string;
  start_line?: number | null;
  end_line?: number | null;
  unified_diff?: string | null;
  query?: string;
  scope?: string | null;
  child_agent_id?: string | null;
  role?: string;
  prompt_preview?: string;
  tool_name?: string;
  arguments_json?: string;
}

export interface ToolStepDescriptorDto {
  kind: string;
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

export interface QuestionItemDto {
  id: string;
  header: string | null;
  question: string;
  options: string[];
  is_multi_select: boolean;
}

export interface BackgroundTaskInfoDto {
  task_id?: string | null;
  label?: string | null;
  log_path?: string | null;
}

export interface PermissionCheckpointRefDto {
  agent_id: string;
  host_id?: string | null;
  request_id: string;
  title: string;
  detail: ToolCallDetailDto;
  allowed_decisions: string[];
}

export interface TimelineItemDto {
  item_type: string;
  seq: number;
  timestamp_ms?: number | null;
  message_id?: string;
  item_id?: string;
  text?: string;
  badges?: PromptBadgeDto[];
  streaming?: boolean;
  blocks?: RichBlockDto[];
  call_id?: string;
  status?: string;
  summary_title?: string;
  detail?: ToolCallDetailDto;
  background_task?: BackgroundTaskInfoDto | null;
  step_descriptor?: ToolStepDescriptorDto | null;
  diff_files?: DiffFileDto[];
  request_id?: string;
  title?: string;
  allowed_decisions?: string[];
  resolved_decision?: string | null;
  guardian_assessment?: GuardianAssessmentDto | null;
  plan_approval?: PlanApprovalPayloadDto | null;
  questions?: QuestionItemDto[];
  resolved_answers?: string[][] | null;
  level?: string;
  message?: string;
  kind?: string;
  source_agent_id?: string | null;
  source_host_id?: string | null;
  source_title?: string | null;
  event?: string | null;
  permission_context?: PermissionCheckpointRefDto | null;
  delivery_status?: 'queued' | 'dispatched' | 'settled' | 'dropped' | null;
  delivery_turn_id?: string | null;
  blocking_turn_id?: string | null;
  queued_at_ms?: number | null;
  dispatched_at_ms?: number | null;
  settled_at_ms?: number | null;
}

export interface TurnFileDiffSummaryDto {
  path: string;
  additions: number;
  deletions: number;
}

export interface TurnWorkCountsDto {
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

export interface WorkSummaryResultDto {
  counts: TurnWorkCountsDto;
  label: string;
  live_label: string;
  live_detail?: string | null;
  label_includes_failure: boolean;
}

export interface TurnCompactionItemDto {
  seq: number;
  label: string;
}

export interface TurnLiveSegmentDto {
  kind: string;
  key: string;
  first_seq?: number;
  items?: TimelineItemDto[];
  summary?: WorkSummaryResultDto;
  thought_duration_ms?: number | null;
  work_duration_ms?: number | null;
  started_at_ms?: number | null;
  latest_activity_ms?: number | null;
  has_following_content?: boolean;
  has_running_step?: boolean;
  item?: TimelineItemDto;
  label?: string;
}

export interface StageTurnDto {
  key: string;
  turn_index: number;
  anchor_seq: number;
  user_prompt: TimelineItemDto | null;
  agent_message?: TimelineItemDto | null;
  is_compaction_command: boolean;
  folded_steps: TimelineItemDto[];
  work_counts: TurnWorkCountsDto;
  work_summary_label: string;
  work_summary_live_label: string;
  work_summary_live_detail?: string | null;
  work_summary_includes_failure: boolean;
  thought_duration_ms: number | null;
  work_duration_ms?: number | null;
  turn_duration_ms: number | null;
  terminal_reply: TimelineItemDto | null;
  last_assistant_reply?: TimelineItemDto | null;
  notices: TimelineItemDto[];
  compactions?: TurnCompactionItemDto[];
  live_segments: TurnLiveSegmentDto[];
  turn_diffs: TurnFileDiffSummaryDto[];
  has_error: boolean;
  has_unrecovered_error: boolean;
  has_active_item: boolean;
  started_at_ms: number | null;
  completed_at_ms: number | null;
  latest_activity_ms?: number | null;
  latest_visible_progress_ms?: number | null;
}

export interface SessionPlanStepDto {
  id: string;
  title: string;
  status: string; // 'pending' | 'in_progress' | 'completed' | 'skipped' | 'failed'
  active_form?: string | null;
  updated_seq: number;
}

export interface SessionOutlinePromptDto {
  seq: number;
  preview: string;
}

export interface SubagentLineageStepDto {
  host_id: string;
  agent_id: string;
  role: string;
}

export interface SessionSubagentEntryDto {
  id: string;
  role: string;
  prompt_preview: string;
  status: string;
  child_agent_id: string | null;
  provider_subagent_id?: string | null;
  last_seq: number;
  host_id?: string | null;
  host_name?: string | null;
  parent_agent_id?: string | null;
  parent_host_id?: string | null;
  depth?: number;
  lineage?: SubagentLineageStepDto[];
}

export function isSubagentSettledStatus(status: string): boolean {
  const s = status.trim().toLowerCase();
  return (
    s === 'completed' ||
    s === 'done' ||
    s === 'succeeded' ||
    s === 'failed' ||
    s === 'error' ||
    s === 'denied' ||
    s === 'canceled' ||
    s === 'cancelled' ||
    s === 'closed' ||
    s === 'idle'
  );
}

export function isSubagentActiveStatus(status: string): boolean {
  return !isSubagentSettledStatus(status);
}

export function resolveSubagentActualHostId(
  sub: SessionSubagentEntryDto,
  fallbackRootHostId: string
): string {
  if (sub.host_id !== undefined && sub.host_id !== null && sub.host_id.trim().length > 0) {
    return sub.host_id.trim();
  }
  return fallbackRootHostId.trim();
}

export function resolveSubagentDirectParentHostId(
  sub: SessionSubagentEntryDto,
  fallbackRootHostId: string
): string {
  if (
    sub.parent_host_id !== undefined &&
    sub.parent_host_id !== null &&
    sub.parent_host_id.trim().length > 0
  ) {
    return sub.parent_host_id.trim();
  }
  return fallbackRootHostId.trim();
}

export function resolveSubagentDirectParentAgentId(
  sub: SessionSubagentEntryDto,
  fallbackRootAgentId: string
): string {
  if (
    sub.parent_agent_id !== undefined &&
    sub.parent_agent_id !== null &&
    sub.parent_agent_id.trim().length > 0
  ) {
    return sub.parent_agent_id.trim();
  }
  return fallbackRootAgentId.trim();
}

export interface SessionInsightsDto {
  outline_prompts?: SessionOutlinePromptDto[];
  plan_steps?: SessionPlanStepDto[];
  subagents?: SessionSubagentEntryDto[];
  detected_local_urls?: string[];
}

export interface PendingPlanApprovalDto {
  seq: number;
  request_id: string;
  title: string;
  plan_markdown: string;
  plan_title?: string | null;
  plan_file_path?: string | null;
  summary?: string | null;
  allowed_prompts?: string[];
  source?: string | null;
  blocks?: RichBlockDto[];
  timestamp_ms?: number | null;
}

export interface AgentStageDto {
  revision: number;
  host_id: string;
  agent: AgentCardDto | null;
  turns: StageTurnDto[];
  pending_permissions: TimelineItemDto[];
  pending_questions: TimelineItemDto[];
  pending_plan_approval?: PendingPlanApprovalDto | null;
  session_turn_diffs: DiffFileDto[];
  insights?: SessionInsightsDto;
  latest_seq: number;
}

export interface AgentStageDeltaDto {
  revision: number;
  host_id: string;
  agent: AgentCardDto | null;
  replace_from_turn_index: number;
  window_start_turn_index?: number;
  changed_turns: StageTurnDto[];
  total_turn_count: number;
  pending_permissions: TimelineItemDto[];
  pending_questions: TimelineItemDto[];
  pending_plan_approval?: PendingPlanApprovalDto | null;
  session_turn_diffs: DiffFileDto[];
  insights?: SessionInsightsDto;
  latest_seq: number;
  reset: boolean;
  aux_revision?: number;
  aux_unchanged?: boolean;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
}

export interface FleetSidebarRequestDto {
  sort: string;
  filter: string;
  query: string;
  active_host_id: string | null;
  active_agent_id: string | null;
  active_cwd: string | null;
}

export interface SidebarConversationDto {
  host_id: string;
  agent: AgentCardDto;
  is_worktree: boolean;
  is_pinned: boolean;
  is_unread: boolean;
  seen_seq: number;
}

export interface SidebarProjectGroupDto {
  id: string;
  host_id: string;
  name: string;
  primary_cwd: string;
  directories: string[];
  git_branch: string | null;
  pinned_project_id: string | null;
  is_home_workspace: boolean;
  sort_weight_ms: number;
  conversations: SidebarConversationDto[];
}

export interface FleetSidebarDto {
  revision: number;
  projects: SidebarProjectGroupDto[];
  launchpad_projects: LaunchpadProjectDto[];
  needs_attention: SidebarConversationDto[];
  pinned_conversations: SidebarConversationDto[];
}

export interface ThreadPrefSeedDto {
  host_id: string;
  agent_id: string;
  seen_seq: number;
  is_pinned: boolean;
  pinned_order: number;
  updated_at_ms: number;
}

export interface ModelSelectionRequestDto {
  host_id: string | null;
  provider: string;
  desired_model: string | null;
  desired_effort: string | null;
}

export interface ResolvedModelSelectionDto {
  provider: string;
  catalog: ProviderModelCatalogDto;
  selected_model_id: string | null;
  selected_model_label: string;
  selected_effort: string | null;
  selected_effort_label: string | null;
  thinking_options: ThinkingOptionSpecDto[];
}

export interface SlashCommandDto {
  id: string;
  command: string;
  label: string;
  description: string;
  shortcut_key: string | null;
  icon_key: string;
  keywords: string[];
  desktop_only: boolean;
}

export interface MentionRankRequestDto {
  host_id: string;
  cwd: string;
  query: string;
  limit: number;
}

export interface MentionCandidateDto {
  key: string;
  kind: string; // 'git_diff' | 'staged' | 'terminal' | 'file'
  token_label: string;
  title: string;
  subtitle: string;
  file_path: string;
  git_status: string;
  score: number;
}

export interface ComposerContextInputDto {
  kind: string; // 'image' | 'staged_file' | 'workspace_file' | 'git_diff' | 'terminal_output' | 'assistant_quote'
  path?: string;
  filename?: string;
  staged?: boolean;
  label?: string;
  diff_text?: string | null;
  content?: string;
}

export interface ComposerQuoteInputDto {
  path: string;
  line_number: number | null;
  code: string;
}

export interface ComposerPasteInputDto {
  label: string;
  filename: string | null;
  content: string;
}

export interface ComposePromptRequestDto {
  provider: string;
  raw_text: string;
  explicit_directive: string | null;
  host_id: string | null;
  cwd: string | null;
  contexts: ComposerContextInputDto[];
  quotes: ComposerQuoteInputDto[];
  pastes: ComposerPasteInputDto[];
}

export interface ComposedPromptDto {
  wire_prompt: string;
  display_prompt: string;
  badges: PromptBadgeDto[];
  effective_directive: string | null;
  is_compact: boolean;
  is_btw: boolean;
  btw_question: string | null;
  is_empty: boolean;
}

export interface RewindAgentResultDto {
  agent: AgentCardDto;
  restored_files: number;
  restored_prompt: string | null;
  notice: string | null;
}

export interface WorkspaceFileViewDto {
  host_id: string;
  cwd: string;
  path: string;
  resolved_path: string;
  language: string;
  content_kind: string;
  mime_type: string | null;
  content: string;
  size_bytes: number;
  truncated: boolean;
  etag?: string | null;
  mtime_unix_ms?: number | null;
  writable?: boolean;
  line_ending?: string | null;
  highlighted_lines: HighlightedLineDto[];
  markdown_blocks: RichBlockDto[];
}

export interface WorkspaceFileConflictDto {
  current_etag: string;
  current_content?: string | null;
  current_size_bytes: number;
  current_mtime_unix_ms?: number | null;
}

export interface WorkspaceFileWriteResultDto {
  host_id: string;
  cwd: string;
  path: string;
  etag: string;
  size_bytes: number;
  mtime_unix_ms?: number | null;
  conflict?: WorkspaceFileConflictDto | null;
}

export function isWorkspaceFileEditable(view: WorkspaceFileViewDto | null): boolean {
  if (view === null) {
    return false;
  }
  return view.content_kind === 'text' && !view.truncated && view.writable === true;
}

export function orbitTextHashSig(text: string): string {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `${text.length}:${hash.toString(16)}`;
}

export interface AskSideQuestionRequestDto {
  host_id: string;
  agent_id: string | null;
  cwd: string;
  provider: string;
  model: string | null;
  question: string;
  context_summary: string | null;
}

export interface SideQuestionEventDto {
  request_id?: string;
  text_delta?: string;
  full_text?: string;
  error?: string | null;
}

export interface RenderDiagramRequestDto {
  kind: string; // 'mermaid' | 'svg' | 'graphviz'
  source: string;
  theme: string; // 'light' | 'dark'
}

export interface RenderDiagramViewDto {
  kind: string;
  theme: string;
  svg: string | null;
  width: number;
  height: number;
  error: string | null;
  diagram_type_label: string | null;
}

export interface GitFileEntryDto {
  path: string;
  status: string;
  staged: boolean;
  untracked: boolean;
}

export interface GitStatusSnapshotDto {
  cwd: string;
  is_git_repo: boolean;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFileEntryDto[];
  repo_prefix?: string | null;
  repo_root?: string | null;
}

export interface GitReviewDto {
  revision: number;
  host_id: string;
  cwd: string;
  status: GitStatusSnapshotDto | null;
  selected_path: string | null;
  staged: boolean;
  diff_revision: number;
  diff_in_flight?: boolean;
  status_revision?: number;
  status_identity_epoch?: number;
  status_reverify_pending?: boolean;
  cwd_identity_epoch?: number;
  last_diff_error_revision?: number;
  last_diff_error_status_revision?: number;
  raw_diff: string;
  diff_files: DiffFileDto[];
}

export interface TerminalSpanDto {
  text: string;
  fg_hex: string;
  bg_hex: string | null;
  bold: boolean;
  underline: boolean;
  inverse: boolean;
}

export interface TerminalRowDto {
  row_index: number;
  spans: TerminalSpanDto[];
}

export interface TerminalScreenDto {
  cols: number;
  rows: number;
  cursor_row: number;
  cursor_col: number;
  rows_data: TerminalRowDto[];
}

export interface StagedAttachmentDto {
  path: string;
  filename: string;
  size_bytes: number;
}

export interface ComposerAttachmentItem {
  id: string;
  path: string;
  filename: string;
  sizeBytes: number;
  isImage: boolean;
  localUri: string;
}

export interface InlineMentionRegistration {
  token: string;
  kind: string;
  path: string;
  staged: boolean;
  label: string;
}

export interface QuotedDiffSnippet {
  id: string;
  path: string;
  lineLabel: string;
  content: string;
  kind: string;
}

export interface SlashCommandItem {
  command: string;
  title: string;
  description: string;
  modeOverride: string;
}

export interface QueuedPromptItem {
  id: string;
  hostKey: string;
  rawText: string;
  displayPrompt: string;
  wirePrompt: string;
  badgesJson: string | null;
  badges: PromptBadgeDto[];
  quotedSnippets: QuotedDiffSnippet[];
  stagedAttachments: ComposerAttachmentItem[];
  inlineMentions: InlineMentionRegistration[];
  createdAtMs: number;
}

export interface DevKeyItem {
  id: string;
  label: string;
  sequence: string;
  isModifier: boolean;
  a11yLabel: string;
}

export interface FlatDiffRowDto {
  key: string;
  rowKind: string;
  filePath: string;
  additions: number;
  deletions: number;
  isCollapsed: boolean;
  isAutoGenerated: boolean;
  totalLineCount: number;
  hiddenLineCount: number;
  line: DiffLineDto | null;
}

// Pure English UI Formatting Helpers matching Orbit Desktop

export function formatProviderLabel(provider: string): string {
  const clean = provider.trim().toLowerCase();
  if (clean === 'codex') {
    return 'Codex';
  }
  if (clean === 'claude_code' || clean === 'claude') {
    return 'Claude Code';
  }
  return 'Antigravity';
}

export function formatProviderShortLabel(provider: string): string {
  const clean = provider.trim().toLowerCase();
  if (clean === 'codex') {
    return 'Codex';
  }
  if (clean === 'claude_code' || clean === 'claude') {
    return 'Claude';
  }
  return 'AGY';
}

export function formatEffortLabel(effort: string | null | undefined): string {
  if (effort === null || effort === undefined || effort.length === 0) {
    return '';
  }
  const clean = effort.trim().toLowerCase();
  if (clean === 'off') {
    return 'Off';
  }
  if (clean === 'low') {
    return 'Low';
  }
  if (clean === 'medium') {
    return 'Medium';
  }
  if (clean === 'xhigh' || clean === 'extra_high') {
    return 'Extra High';
  }
  if (clean === 'max') {
    return 'Max';
  }
  if (clean === 'ultra') {
    return 'Ultra';
  }
  if (clean === 'high') {
    return 'High';
  }
  return effort;
}

export function formatShortEffortLabel(effort: string | null | undefined): string {
  if (effort === null || effort === undefined || effort.length === 0) {
    return '';
  }
  const clean = effort.trim().toLowerCase();
  if (clean === 'off') {
    return 'Off';
  }
  if (clean === 'low') {
    return 'Low';
  }
  if (clean === 'medium') {
    return 'Med';
  }
  if (clean === 'xhigh' || clean === 'extra_high') {
    return 'XHigh';
  }
  if (clean === 'max') {
    return 'Max';
  }
  if (clean === 'ultra') {
    return 'Ultra';
  }
  if (clean === 'high') {
    return 'High';
  }
  return effort;
}

export function normalizeModeSlug(raw: string | CustomSessionModeDto | null | undefined): string {
  if (raw === null || raw === undefined) {
    return 'accept_edits';
  }
  if (typeof raw === 'string') {
    if (raw === 'plan' || raw.length === 0) {
      return 'default';
    }
    return raw;
  }
  if (raw.custom !== undefined && raw.custom.length > 0) {
    return raw.custom;
  }
  return 'default';
}

export function formatModeLabel(modeSlug: string): string {
  if (modeSlug === 'default' || modeSlug === 'plan') {
    return 'Always Ask';
  }
  if (modeSlug === 'auto') {
    return 'Auto';
  }
  if (modeSlug === 'accept_edits') {
    return 'Accept Edits';
  }
  if (modeSlug === 'full_access') {
    return 'Full Access';
  }
  return modeSlug;
}

export function formatTimerConditionLabel(cond: TimerConditionDto): string {
  if (cond.kind === 'any_message') {
    return 'until any message';
  }
  if (cond.kind === 'all_subagents_settled') {
    return 'until subagents settle';
  }
  if (cond.kind === 'all_peers_settled') {
    return 'until workspace peers settle';
  }
  if (cond.kind === 'sender' && cond.sender_id !== undefined && cond.sender_id.length > 0) {
    return `until ${cond.sender_id}`;
  }
  if (cond.kind === 'file_changed' && cond.path !== undefined && cond.path.length > 0) {
    return `until ${basenameOfPath(cond.path)} changes`;
  }
  return 'fixed timer';
}

export function formatWorkflowKindLabel(kind: string): string {
  if (kind === 'batch_csv') {
    return 'Batch CSV';
  }
  if (kind === 'best_of_n') {
    return 'Best-of-N';
  }
  return 'DAG';
}

export function formatAgentStatusLabel(agent: AgentCardDto): string {
  if (agent.is_awaiting_permission || agent.status.state === 'awaiting_permission') {
    return 'Needs Approval';
  }
  if (agent.status.state === 'initializing') {
    return 'Starting';
  }
  if (agent.status.state === 'running') {
    return 'Working';
  }
  if (agent.status.state === 'autonomous_wakeup') {
    return 'Background';
  }
  if (agent.status.state === 'error') {
    return 'Error';
  }
  if (agent.status.state === 'closed') {
    return 'Archived';
  }
  return 'Ready';
}

export function formatConnectionStateLabel(state: string, viaSsh: boolean): string {
  if (state === 'online_direct') {
    return viaSsh ? 'SSH Tunnel' : 'LAN Direct';
  }
  if (state === 'online_relay') {
    return 'E2EE Relay';
  }
  if (state === 'connecting_direct') {
    return 'Connecting…';
  }
  if (state === 'connecting_relay') {
    return 'Relaying…';
  }
  return 'Offline';
}

export function formatCompactTopBarConnectionLabel(state: string, viaSsh: boolean): string {
  if (state === 'online_direct') {
    return viaSsh ? 'SSH' : 'Direct';
  }
  if (state === 'online_relay') {
    return 'Relay';
  }
  if (state === 'connecting_direct' || state === 'connecting_relay') {
    return 'Connecting';
  }
  return 'Offline';
}

export function formatRouteModeLabel(routeMode: string): string {
  if (routeMode === 'direct_only') {
    return 'Direct Only';
  }
  if (routeMode === 'ssh_tunnel') {
    return 'SSH Tunnel';
  }
  if (routeMode === 'relay_only') {
    return 'Relay Only';
  }
  return 'Auto';
}

export function formatPermissionDecisionLabel(decision: string): string {
  const lower = decision.trim().toLowerCase();
  if (lower === 'allow_always' || lower === 'allow_session' || lower === 'always') {
    return 'Allow for Session';
  }
  if (lower === 'deny' || lower === 'reject' || lower === 'decline') {
    return 'Decline';
  }
  if (lower === 'allow' || lower === 'approve' || lower === 'accept') {
    return 'Allow Once';
  }
  return decision;
}

export function basenameOfPath(rawPath: string): string {
  if (rawPath.length === 0) {
    return '~';
  }
  if (rawPath === '~' || rawPath === '/') {
    return rawPath;
  }
  let trimmed = rawPath;
  while (trimmed.length > 1 && trimmed.endsWith('/')) {
    trimmed = trimmed.substring(0, trimmed.length - 1);
  }
  const idx = trimmed.lastIndexOf('/');
  if (idx >= 0 && idx < trimmed.length - 1) {
    return trimmed.substring(idx + 1);
  }
  return trimmed;
}

export function compactPath(rawPath: string): string {
  const trimmed = rawPath.trim();
  if (trimmed.length === 0 || trimmed === '~') {
    return '~';
  }
  let clean = trimmed;
  while (clean.length > 1 && clean.endsWith('/')) {
    clean = clean.substring(0, clean.length - 1);
  }
  const parts = clean.split('/').filter((seg: string) => seg.length > 0);
  if (clean.startsWith('/') && parts.length >= 2 && (parts[0] === 'Users' || parts[0] === 'home')) {
    const rest = parts.slice(2);
    return rest.length === 0 ? '~' : `~/${rest.join('/')}`;
  }
  return clean;
}

/**
 * Returns `rawPath` verbatim when it contains at least one non-whitespace character, or `''`
 * when blank/nullish. Preserves legal trailing/leading spaces in POSIX directory & file names
 * (`repo `, `report.txt `) while still rejecting whitespace-only inputs (R14-P1-3).
 */
export function nonBlankPath(rawPath: string | null | undefined): string {
  if (rawPath === undefined || rawPath === null || rawPath.trim().length === 0) {
    return '';
  }
  return rawPath;
}

/**
 * Resolves the exact file path for a Git diff row action (Stage / Unstage / line quote).
 * Always uses the parsed literal `rowPath` when non-empty — NEVER substitutes `selectedPath`
 * based on filtered row count, because `selectedPath` may be a directory pathspec (e.g. `d`
 * containing `d/one` and `d/two`). Falls back to `selectedPath` only when `rowPath` is empty
 * (R14-P1-2).
 */
export function resolveGitDiffRowActionPath(rowPath: string, selectedPath: string | null): string {
  if (rowPath.length > 0) {
    return rowPath;
  }
  if (selectedPath !== null && selectedPath.length > 0) {
    return selectedPath;
  }
  return rowPath;
}

function normalizeGitRelPathSegments(rawPath: string): string {
  if (rawPath.length === 0) {
    return '';
  }
  const parts = rawPath.split('/');
  const out: string[] = [];
  for (let i = 0; i < parts.length; i++) {
    const seg = parts[i];
    if (seg.length === 0 || seg === '.') {
      continue;
    }
    out.push(seg);
  }
  return out.join('/');
}

/**
 * Pure mirror of Rust `orbit-client-core`'s post-mutation slot matching (`R17-P1-3` / `R17-P1-4` /
 * `R17-P1-5`): determines whether a visible Review slot `(focusPath, staged)` that was just
 * invalidated (`diff_revision === 0`) may still have a real diff in `status` and should trigger a
 * single targeted re-fetch.
 * - Returns `false` when `status` is absent, clean (`files.length === 0`), or has zero entries in
 *   the requested `staged` mode (0 unnecessary re-fetches on clean repos or `AM -> A ` U slots).
 * - Returns `true` for whole-repo view (`focusPath === null`) whenever the target mode has entries.
 * - Returns `true` conservatively when `status.repo_prefix` is absent (`undefined`/`null`, legacy
 *   daemon) and the target `staged` mode still has entries (`R17-P1-5`).
 * - Otherwise converts `focusPath` via `status.repo_prefix` to the exact repo-root-relative path
 *   and matches ONLY exact files, untracked directory summaries (`bucket/`), or directory-scope
 *   queries (`d` -> `d/g`) in the matching `staged` mode, preserving legal trailing spaces.
 */
export function gitStatusMayHaveDiffForSlot(
  status: GitStatusSnapshotDto | null | undefined,
  focusPath: string | null,
  staged: boolean
): boolean {
  if (status === undefined || status === null || status.files.length === 0) {
    return false;
  }
  let hasAnyInMode = false;
  for (let i = 0; i < status.files.length; i++) {
    if (status.files[i].staged === staged) {
      hasAnyInMode = true;
      break;
    }
  }
  if (!hasAnyInMode) {
    return false;
  }
  if (focusPath === null) {
    return true;
  }
  const normReq = normalizeGitRelPathSegments(focusPath);
  if (normReq.length === 0) {
    return true;
  }
  if (status.repo_prefix === undefined || status.repo_prefix === null) {
    return true;
  }
  const normPrefix = normalizeGitRelPathSegments(status.repo_prefix);
  const repoRel = normPrefix.length === 0 ? normReq : `${normPrefix}/${normReq}`;
  for (let i = 0; i < status.files.length; i++) {
    const entry = status.files[i];
    if (entry.staged !== staged) {
      continue;
    }
    const isDirSummary = entry.path.endsWith('/');
    const rem = normalizeGitRelPathSegments(entry.path);
    if (rem.length === 0) {
      continue;
    }
    if (rem === repoRel) {
      return true;
    }
    if (isDirSummary && repoRel.length > rem.length && repoRel.startsWith(`${rem}/`)) {
      return true;
    }
    if (rem.length > repoRel.length && rem.startsWith(`${repoRel}/`)) {
      return true;
    }
  }
  return false;
}

/**
 * Normalizes a POSIX workspace path for exact per-host comparison without ever stripping
 * `/home/<user>`, `/Users/<user>`, `/root` prefixes, or legal trailing spaces (R3-P0-1 /
 * R3-P1-6 / R14-P1-3).
 * Expands leading `~` or `~/...` ONLY when an explicit absolute `hostHomeDir` is supplied.
 */
export function normalizeAbsoluteCwd(rawPath: string, hostHomeDir?: string | null): string {
  let clean = nonBlankPath(rawPath);
  if (clean.length === 0) {
    return '';
  }
  const cleanHome = nonBlankPath(hostHomeDir);
  if (cleanHome.length > 0 && cleanHome.startsWith('/')) {
    let normHome = cleanHome.replace(/\/+/g, '/');
    while (normHome.length > 1 && normHome.endsWith('/')) {
      normHome = normHome.substring(0, normHome.length - 1);
    }
    if (clean === '~') {
      clean = normHome;
    } else if (clean.startsWith('~/')) {
      clean = `${normHome}/${clean.substring(2)}`;
    }
  }
  clean = clean.replace(/\/+/g, '/');
  const isAbs = clean.startsWith('/');
  const rawParts = clean.split('/');
  const normParts: string[] = [];
  for (let i = 0; i < rawParts.length; i++) {
    const part = rawParts[i];
    if (part.length === 0 || part === '.') {
      continue;
    }
    normParts.push(part);
  }
  if (isAbs) {
    return normParts.length === 0 ? '/' : `/${normParts.join('/')}`;
  }
  return normParts.join('/');
}

export function cwdPathsEquivalent(
  pathA: string,
  pathB: string,
  hostHomeDir?: string | null
): boolean {
  const a = normalizeAbsoluteCwd(pathA, hostHomeDir);
  const b = normalizeAbsoluteCwd(pathB, hostHomeDir);
  if (a.length === 0 || b.length === 0) {
    return false;
  }
  return a === b;
}

/**
 * Normalizes a workspace-relative file path for exact receipt/target matching (R3-P1-1 / R14-P1-3).
 * Strips a matching `cwd/` prefix if present; preserves leading `/` for paths outside `cwd` and
 * preserves trailing spaces in filenames/cwds.
 */
export function normalizeWorkspaceRelPath(rawPath: string, cwd?: string | null): string {
  let clean = nonBlankPath(rawPath).replace(/\/+/g, '/');
  if (clean.length === 0) {
    return '';
  }
  if (cwd !== undefined && cwd !== null && cwd.trim().length > 0) {
    const normCwd = normalizeAbsoluteCwd(cwd);
    if (normCwd.length > 1 && clean.startsWith(`${normCwd}/`)) {
      clean = clean.substring(normCwd.length + 1);
    } else if (normCwd === '/' && clean.startsWith('/')) {
      clean = clean.substring(1);
    }
  }
  const isAbs = clean.startsWith('/');
  const rawParts = clean.split('/');
  const normParts: string[] = [];
  for (let i = 0; i < rawParts.length; i++) {
    const part = rawParts[i];
    if (part.length === 0 || part === '.') {
      continue;
    }
    normParts.push(part);
  }
  if (isAbs) {
    return normParts.length === 0 ? '/' : `/${normParts.join('/')}`;
  }
  return normParts.join('/');
}

export function formatCompactDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || ms <= 0) {
    return '';
  }
  const totalSec = Math.max(1, Math.round(ms / 1000));
  if (totalSec < 60) {
    return `${totalSec}s`;
  }
  const mins = Math.floor(totalSec / 60);
  const secs = totalSec % 60;
  if (mins >= 60) {
    const hrs = Math.floor(mins / 60);
    const remMins = mins % 60;
    return remMins > 0 ? `${hrs}h ${remMins}m` : `${hrs}h`;
  }
  return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
}

export function formatQuotaResetCountdown(resetsAt: string | null | undefined): string {
  if (resetsAt === null || resetsAt === undefined) {
    return '';
  }
  const clean = resetsAt.trim();
  if (clean.length === 0) {
    return '';
  }
  let targetMs: number = NaN;
  if (/^\d+$/.test(clean)) {
    const num = Number(clean);
    targetMs = num < 100000000000 ? num * 1000 : num;
  } else {
    targetMs = Date.parse(clean);
  }
  if (Number.isNaN(targetMs)) {
    return clean.toLowerCase().startsWith('reset') ? clean : `Resets ${clean}`;
  }
  const diffMs = targetMs - Date.now();
  if (diffMs <= 0) {
    return 'Resets soon';
  }
  const totalMins = Math.max(1, Math.floor(diffMs / 60000));
  if (totalMins < 60) {
    return `Resets in ${totalMins}m`;
  }
  const hrs = Math.floor(totalMins / 60);
  const mins = totalMins % 60;
  if (hrs < 24) {
    return mins > 0 ? `Resets in ${hrs}h ${mins}m` : `Resets in ${hrs}h`;
  }
  const days = Math.floor(hrs / 24);
  const remHrs = hrs % 24;
  return remHrs > 0 ? `Resets in ${days}d ${remHrs}h` : `Resets in ${days}d`;
}

export function isAutoCollapsedDiffPath(rawPath: string): boolean {
  const clean = rawPath.trim();
  if (clean.length === 0) {
    return false;
  }
  const lower = clean.toLowerCase();
  const base = basenameOfPath(clean).toLowerCase();
  if (
    base === 'cargo.lock' ||
    base === 'package-lock.json' ||
    base === 'pnpm-lock.yaml' ||
    base === 'yarn.lock' ||
    base === 'podfile.lock' ||
    base === 'oh-package-lock.json5'
  ) {
    return true;
  }
  if (lower.indexOf('uniffi') >= 0) {
    return true;
  }
  if (lower.startsWith('generated/') || lower.indexOf('/generated/') >= 0) {
    return true;
  }
  if (base.endsWith('.pbxproj')) {
    return true;
  }
  return false;
}

export function slashCommandSymbol(id: string): Resource {
  if (id === 'plan') {
    return $r('sys.symbol.list_checkmark');
  }
  if (id === 'review') {
    return $r('sys.symbol.doc_text_badge_magnifyingglass');
  }
  if (id === 'diff') {
    return $r('sys.symbol.arrow_left_arrow_right');
  }
  if (id === 'compact') {
    return $r('sys.symbol.cut');
  }
  if (id === 'ask' || id === 'supervised') {
    return $r('sys.symbol.security_shield');
  }
  if (id === 'edits' || id === 'auto') {
    return $r('sys.symbol.checkmark_shield');
  }
  if (id === 'full') {
    return $r('sys.symbol.key_shield_slash');
  }
  if (id === 'terminal') {
    return $r('sys.symbol.code');
  }
  if (id === 'new') {
    return $r('sys.symbol.square_and_pencil');
  }
  if (id === 'settings') {
    return $r('sys.symbol.gearshape');
  }
  return $r('sys.symbol.message');
}

export function formatRelativeTimeLabel(unixSecs: number): string {
  if (unixSecs <= 0) {
    return '';
  }
  const nowSecs = Math.floor(Date.now() / 1000);
  const delta = Math.max(0, nowSecs - unixSecs);
  if (delta < 60) {
    return 'just now';
  }
  if (delta < 3600) {
    return `${Math.floor(delta / 60)}m ago`;
  }
  if (delta < 86400) {
    return `${Math.floor(delta / 3600)}h ago`;
  }
  return `${Math.floor(delta / 86400)}d ago`;
}

export function stripWorkspacePrefix(path: string, cwd: string): string {
  const cleanPath = nonBlankPath(path);
  let cleanCwd = nonBlankPath(cwd);
  while (cleanCwd.length > 1 && cleanCwd.endsWith('/')) {
    cleanCwd = cleanCwd.substring(0, cleanCwd.length - 1);
  }
  if (cleanPath.length === 0 || cleanCwd.length === 0 || cleanCwd === '~') {
    return cleanPath;
  }
  if (cleanPath === cleanCwd) {
    return '.';
  }
  if (cleanPath.startsWith(`${cleanCwd}/`)) {
    return cleanPath.substring(cleanCwd.length + 1);
  }
  return cleanPath;
}

export function parseRfc3339Ms(rfc3339: string): number {
  const trimmed = rfc3339.trim();
  if (trimmed.length === 0) {
    return 0;
  }
  const ms = Date.parse(trimmed);
  return Number.isFinite(ms) ? ms : 0;
}

export function formatCompactRelativeTime(rfc3339: string): string {
  const ms = parseRfc3339Ms(rfc3339);
  if (ms <= 0) {
    return '';
  }
  const deltaSecs = Math.max(0, Math.floor((Date.now() - ms) / 1000));
  if (deltaSecs < 60) {
    return 'now';
  }
  if (deltaSecs < 3600) {
    return `${Math.floor(deltaSecs / 60)}m`;
  }
  if (deltaSecs < 86400) {
    return `${Math.floor(deltaSecs / 3600)}h`;
  }
  if (deltaSecs < 604800) {
    return `${Math.floor(deltaSecs / 86400)}d`;
  }
  return `${Math.floor(deltaSecs / 604800)}w`;
}

// --- M2-Core & Full Feature Parity DTOs ---

export interface RegisterPushDeviceRequestDto {
  device_id: string;
  platform: string; // 'harmony_push_kit'
  device_name: string;
  device_token: string;
  live_activity_token: string | null;
  sandbox: boolean;
}

export interface PushPreferencesDto {
  enabled: boolean;
  notify_permission_required: boolean;
  notify_turn_completed: boolean;
  notify_agent_error: boolean;
  notify_goal_finished: boolean;
  notify_timer_fired: boolean;
  include_message_preview: boolean;
}

export interface PushRegistrationStatusDto {
  platform: string;
  token_registered: boolean;
  token_preview: string | null;
  relay_sync_supported: boolean;
}

export interface PermissionBatchItemDto {
  agent_id: string;
  permission_request_id: string;
}

export interface OverviewPendingApprovalItem {
  key: string;
  host_id: string;
  host_name: string;
  agent_id: string;
  agent_title: string;
  request_id: string;
  title: string;
  target_preview: string;
  is_low_risk: boolean;
}

export interface ConversationSearchHitDto {
  host_id: string;
  host_name: string;
  agent_id: string;
  seq: number;
  turn_id?: string | null;
  role: string;
  title: string;
  project_name: string;
  snippet: string;
  updated_at_rfc3339: string;
  is_remote: boolean;
  is_unloaded: boolean;
}

export interface SearchMessagesRequestDto {
  query: string;
  host_id: string | null;
  agent_id: string | null;
  limit: number | null;
}

export interface SearchMessagesResponseDto {
  hits: ConversationSearchHitDto[];
  remote_hosts_queried: number;
  remote_hosts_failed: number;
}

export interface WriteWorkspaceFileRequestDto {
  host_id: string;
  cwd: string;
  path: string;
  content: string;
  expected_etag: string | null;
  force: boolean;
}

export interface CreateWorkspaceEntryRequestDto {
  host_id: string;
  cwd: string;
  path: string;
  is_dir: boolean;
  initial_content: string | null;
}

export interface RenameWorkspaceEntryRequestDto {
  host_id: string;
  cwd: string;
  old_path: string;
  new_path: string;
}

export interface DeleteWorkspaceEntryRequestDto {
  host_id: string;
  cwd: string;
  path: string;
}

export interface GitCommitResultDto {
  commit_sha: string;
  short_sha: string;
  summary: string;
  branch?: string | null;
}

export interface GitGeneratedCommitMessageDto {
  message?: string;
  subject?: string;
  body?: string | null;
}

export interface GitPrCheckItemDto {
  name: string;
  status: string;
  conclusion?: string | null;
}

export interface GitPrStatusDto {
  availability: string;
  reason?: string | null;
  number?: number | null;
  title?: string | null;
  url?: string | null;
  state?: string | null;
  is_draft?: boolean;
  head_ref?: string | null;
  base_ref?: string | null;
  passing_checks?: number;
  failing_checks?: number;
  checks?: GitPrCheckItemDto[];
}

export interface GitCreatePrResultDto {
  url: string;
  number?: number | null;
  title: string;
}

export interface GitStashEntryDto {
  stash_ref: string;
  oid?: string | null;
  message: string;
  branch?: string | null;
}

export interface GitConflictEntryDto {
  path: string;
  sha256?: string | null;
  block_count?: number;
}

export interface GitRebaseStatusDto {
  in_progress: boolean;
  current_step?: number | null;
  total_steps?: number | null;
  onto_ref?: string | null;
}

export interface RouterHostCapabilitiesDto {
  host_id: string;
  host_name: string;
  is_local: boolean;
  online: boolean;
  hermes_installed: boolean;
  hermes_version?: string | null;
  hermes_has_existing_memory: boolean;
  hermes_acp_ready: boolean;
  hermes_gateway_running: boolean;
  available_providers: string[];
  active_router_agent_id?: string | null;
  router_enabled: boolean;
  recommended_score: number;
  recommendation_reason?: string | null;
}

export interface PersonalRouterConfigDto {
  enabled: boolean;
  router_agent_id: string | null;
  provider: string;
  acp_agent_id: string | null;
  model: string | null;
  effort: string | null;
  cwd: string | null;
  scope: string; // 'global_fleet' | 'local_host_only'
  pre_review_mode: string; // 'disabled' | 'medium_risk_only' | 'assist_only'
  share_host_hermes_memory: boolean;
  soul_prompt_override: string | null;
  external_webhook_url: string | null;
  has_external_webhook_secret: boolean;
}

export interface RouterStatusSnapshotDto {
  config: PersonalRouterConfigDto;
  host_capabilities: RouterHostCapabilitiesDto;
  managed_workers_count: number;
  pending_approvals_count: number;
}

export interface ConnectedHostSummaryDto {
  registration_id: string;
  host_id?: string | null;
  label: string;
  daemon_pubkey_b64: string;
  direct_endpoints?: string[];
  relay_url?: string | null;
  enabled: boolean;
  status: string;
  last_error?: string | null;
  hostname?: string | null;
  daemon_version?: string | null;
}

export interface OrbitMeshPeerRow {
  owner_host_id: string;
  owner_host_name: string;
  peer: ConnectedHostSummaryDto;
}

export interface SessionUsageDto {
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  total_cost_usd?: number | null;
  context_percent?: number | null;
}

export interface StartGoalRunRequestDto {
  host_id: string;
  agent_id: string;
  objective: string;
  criteria: string[];
  verify_command: string | null;
  max_iterations: number | null;
  max_duration_secs: number | null;
  replace_existing?: boolean;
  dispatch_initial_prompt?: boolean | null;
}

export interface StartMonitorWatchRequestDto {
  host_id: string;
  agent_id: string;
  name: string;
  kind: MonitorKindDto;
  expires_in_secs: number | null;
}

export interface DagStepSpecDto {
  step_id: string;
  name: string;
  prompt: string;
  depends_on: string[];
}

export interface StartDagWorkflowRequestDto {
  host_id: string;
  name: string;
  cwd: string;
  steps: DagStepSpecDto[];
  default_provider: string;
  default_mode: string;
  max_concurrency: number | null;
}

export interface ProviderSubagentTimelineDto {
  subagent_id: string;
  items: TimelineItemDto[];
}

export interface ProviderSubagentEventDto {
  seq: number;
  parent_agent_id?: string;
  subagent_id?: string;
  timestamp?: string;
  payload?: Record<string, Object | null | undefined>;
}

function readOptionalStringField(
  obj: Record<string, Object | null | undefined>,
  key: string
): string {
  const val = obj[key];
  return typeof val === 'string' ? val : '';
}

/**
 * Parses `providerSubagentTimelineJson` payloads returned by `orbit-ffi-ohos`.
 * Rust serializes `Vec<ProviderSubagentEvent>` (JSON array where each element holds
 * `{ seq, parent_agent_id, subagent_id, timestamp, payload: { type, ... } }`).
 * Also tolerates legacy `{ items: TimelineItemDto[] }` objects if supplied by mocks.
 */
export function parseProviderSubagentTimelineJson(rawJson: string): TimelineItemDto[] {
  if (rawJson.trim().length === 0) {
    return [];
  }
  let parsed: Object;
  try {
    parsed = JSON.parse(rawJson) as Object;
  } catch (_parseErr) {
    return [];
  }
  if (parsed === null || parsed === undefined || typeof parsed !== 'object') {
    return [];
  }
  if (!Array.isArray(parsed)) {
    const legacy = parsed as ProviderSubagentTimelineDto;
    return Array.isArray(legacy.items) ? legacy.items : [];
  }
  const events = parsed as ProviderSubagentEventDto[];
  const out: TimelineItemDto[] = [];
  for (let i = 0; i < events.length; i++) {
    const ev = events[i];
    if (ev === null || ev === undefined || typeof ev !== 'object') {
      continue;
    }
    const seq = typeof ev.seq === 'number' && Number.isFinite(ev.seq) ? ev.seq : i + 1;
    const payload = ev.payload;
    if (payload === undefined || payload === null || typeof payload !== 'object') {
      continue;
    }
    const rawType = readOptionalStringField(payload, 'type');
    if (rawType.length === 0) {
      continue;
    }
    const text = readOptionalStringField(payload, 'text');
    const message = readOptionalStringField(payload, 'message');
    const title = readOptionalStringField(payload, 'title');
    const status = readOptionalStringField(payload, 'status');
    const rawDetail = payload['detail'];
    let detailObj: ToolCallDetailDto | undefined = undefined;
    let summaryTitle: string = title;

    if (rawDetail !== undefined && rawDetail !== null && typeof rawDetail === 'object' && !Array.isArray(rawDetail)) {
      const dRec = rawDetail as Record<string, Object | null | undefined>;
      const kind = readOptionalStringField(dRec, 'kind');
      const command = readOptionalStringField(dRec, 'command');
      const path = readOptionalStringField(dRec, 'path');
      const toolName = readOptionalStringField(dRec, 'tool_name');
      const query = readOptionalStringField(dRec, 'query');
      const role = readOptionalStringField(dRec, 'role');
      const output = readOptionalStringField(dRec, 'output_preview');
      detailObj = {
        kind: kind.length > 0 ? kind : 'generic',
        command: command.length > 0 ? command : undefined,
        path: path.length > 0 ? path : undefined,
        tool_name: toolName.length > 0 ? toolName : undefined,
        query: query.length > 0 ? query : undefined,
        role: role.length > 0 ? role : undefined,
        output_preview: output.length > 0 ? output : undefined
      };
      if (summaryTitle.length === 0) {
        if (command.length > 0) {
          summaryTitle = `$ ${command}`;
        } else if (path.length > 0) {
          summaryTitle = path;
        } else if (toolName.length > 0) {
          summaryTitle = toolName;
        } else if (query.length > 0) {
          summaryTitle = query;
        } else if (role.length > 0) {
          summaryTitle = role;
        }
      }
    }

    const effectiveText = text.length > 0 ? text : message;
    out.push({
      item_type: rawType,
      seq: seq,
      text: effectiveText.length > 0 ? effectiveText : undefined,
      title: title.length > 0 ? title : undefined,
      summary_title: summaryTitle.length > 0 ? summaryTitle : undefined,
      status: status.length > 0 ? status : undefined,
      detail: detailObj
    });
  }
  return out;
}

export interface BriefingAgentRefDto {
  host_id: string;
  host_name: string;
  agent_id: string;
  title: string;
  provider: string;
  status_label: string;
  is_busy: boolean;
  is_awaiting: boolean;
}

export interface BriefingDecisionDto {
  key: string;
  kind: string; // 'plan' | 'permission' | 'awaiting' | 'error'
  host_id: string;
  host_name: string;
  agent_id: string;
  agent_title: string;
  request_id: string | null;
  summary: string;
  risk_level: string | null;
  human_only: boolean;
  batch_eligible: boolean;
  router_recommendation: RouterRecommendationDto | null;
  dispatched_by_personal_agent: boolean;
  updated_at_ms: number;
}

export interface BriefingWorkerDto {
  host_id: string;
  host_name: string;
  agent_id: string;
  title: string;
  provider: string;
  state: string; // 'running' | 'awaiting' | 'error' | 'done' | 'idle'
  step_preview: string;
  worktree_branch: string | null;
  updated_at_ms: number;
}

export interface BriefingMissionDto {
  key: string;
  root: BriefingWorkerDto;
  workers: BriefingWorkerDto[];
  host_names: string[];
  done: number;
  total: number;
  state: string; // 'running' | 'awaiting' | 'error' | 'done'
  updated_at_ms: number;
}

export interface BriefingWaitItemDto {
  key: string;
  kind: string; // 'timer' | 'goal' | 'background'
  host_id: string;
  agent_id: string;
  title: string;
  detail: string;
  due_at_ms: number | null;
}

export interface BriefingHostDto {
  host_id: string;
  name: string;
  online: boolean;
  is_personal_agent_host: boolean;
}

export interface BriefingCountsDto {
  needs_you: number;
  running: number;
  done_recent: number;
  hosts_online: number;
  hosts_total: number;
}

export interface BriefingSinceLastVisitDto {
  completed: number;
  auto_handled: number;
}

export interface PersonalAgentBriefingDto {
  personal_agent: BriefingAgentRefDto | null;
  needs_you: BriefingDecisionDto[];
  missions: BriefingMissionDto[];
  other_active: BriefingWorkerDto[];
  waiting: BriefingWaitItemDto[];
  hosts: BriefingHostDto[];
  counts: BriefingCountsDto;
  since_last_visit: BriefingSinceLastVisitDto;
}

export interface AgentStageDeltaRequestDto {
  host_id: string;
  agent_id: string;
  since_revision: number;
  since_aux_revision?: number | null;
  turn_limit?: number | null;
  since_window_start_turn_index?: number | null;
}

export interface PersonalAgentActivityEntryDto {
  seq: number;
  timestamp_ms: number;
  actor: string;
  decision: string;
  host_id?: string | null;
  agent_id?: string | null;
  agent_title: string;
  summary: string;
  risk_level?: string | null;
  reason: string;
  request_id?: string | null;
}

export interface PersistedPastedAttachmentDto {
  id: string;
  label: string;
  filename?: string;
  lineCount: number;
  content: string;
}

export interface PersistedDiffQuoteDto {
  id: string;
  path: string;
  lineNumber: number | null;
  code: string;
}

export interface PersistedContextItemDto {
  id: string;
  kind: string;
  path?: string;
  filename?: string;
  previewUrl?: string;
  sizeBytes?: number;
  stagedHostId?: string;
  mediaKind?: string;
  mimeType?: string;
  pdfPageCount?: number | null;
  staged?: boolean;
  label?: string;
  lineCount?: number;
  content?: string;
  startLine?: number;
  endLine?: number;
  symbolName?: string;
  symbolKind?: string;
  signature?: string | null;
  container?: string | null;
  language?: string;
  snippet?: string | null;
  slug?: string;
  description?: string;
  url?: string;
  fileCount?: number;
  treeSummary?: string | null;
  number?: number;
  title?: string;
  state?: string | null;
  headRef?: string | null;
  baseRef?: string | null;
  summary?: string | null;
}

export interface PersistedInlineMentionDto {
  token: string;
  kind: string;
  path?: string;
  staged?: boolean;
  label?: string;
  lineCount?: number;
  content?: string;
  startLine?: number;
  endLine?: number;
  symbolName?: string;
  symbolKind?: string;
  signature?: string | null;
  container?: string | null;
  language?: string;
  fileCount?: number;
  number?: number;
  title?: string;
  url?: string | null;
  state?: string | null;
  headRef?: string | null;
  baseRef?: string | null;
  summary?: string | null;
}

export interface PersistedScopeDraftDto {
  updatedAt: number;
  text?: string;
  pastedAttachments?: PersistedPastedAttachmentDto[];
  diffQuotes?: PersistedDiffQuoteDto[];
  contextItems?: PersistedContextItemDto[];
  inlineMentions?: PersistedInlineMentionDto[];
  slashDirective?: string;
  sideQuestionDraft?: string;
}

export interface PersistedDraftPayloadV1Dto {
  version: number;
  drafts: Record<string, PersistedScopeDraftDto>;
}
