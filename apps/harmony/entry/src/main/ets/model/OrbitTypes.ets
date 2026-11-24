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
  last_synced_at_ms: number | null;
  last_vault_revision: number;
  synced_host_count: number;
  synced_project_count: number;
  last_error: string | null;
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
  schedules?: ScheduleSpecDto[];
  conditional_timers?: ConditionalTimerRecordDto[];
  goal_runs?: GoalRunRecordDto[];
  monitor_watches?: MonitorWatchRecordDto[];
  workflow_runs?: WorkflowRunRecordDto[];
  workspace_boards?: Record<string, WorkspaceBoardSnapshotDto>;
  workspace_events?: Record<string, WorkspaceEventRecordDto[]>;
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
  step_descriptor?: ToolStepDescriptorDto | null;
  diff_files?: DiffFileDto[];
  request_id?: string;
  title?: string;
  allowed_decisions?: string[];
  resolved_decision?: string | null;
  questions?: QuestionItemDto[];
  resolved_answers?: string[][] | null;
  level?: string;
  message?: string;
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
  notices: TimelineItemDto[];
  compactions?: TurnCompactionItemDto[];
  live_segments: TurnLiveSegmentDto[];
  turn_diffs: TurnFileDiffSummaryDto[];
  has_error: boolean;
  has_unrecovered_error: boolean;
  has_active_item: boolean;
  started_at_ms: number | null;
  completed_at_ms: number | null;
}

export interface AgentStageDto {
  revision: number;
  host_id: string;
  agent: AgentCardDto | null;
  turns: StageTurnDto[];
  pending_permissions: TimelineItemDto[];
  pending_questions: TimelineItemDto[];
  session_turn_diffs: DiffFileDto[];
  latest_seq: number;
}

export interface AgentStageDeltaDto {
  revision: number;
  host_id: string;
  agent: AgentCardDto | null;
  replace_from_turn_index: number;
  changed_turns: StageTurnDto[];
  total_turn_count: number;
  pending_permissions: TimelineItemDto[];
  pending_questions: TimelineItemDto[];
  session_turn_diffs: DiffFileDto[];
  latest_seq: number;
  reset: boolean;
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
  highlighted_lines: HighlightedLineDto[];
  markdown_blocks: RichBlockDto[];
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
}

export interface GitReviewDto {
  revision: number;
  host_id: string;
  cwd: string;
  status: GitStatusSnapshotDto | null;
  selected_path: string | null;
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
  cursor_visible?: boolean;
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

function stripHomePrefixToRel(normPath: string): string | null {
  if (normPath === '~') {
    return '';
  }
  if (normPath.startsWith('~/')) {
    return normPath.substring(2);
  }
  const prefixes: string[] = ['/Users/', '/home/'];
  for (let i = 0; i < prefixes.length; i++) {
    const p = prefixes[i];
    if (normPath.startsWith(p)) {
      const rest = normPath.substring(p.length);
      const slashIdx = rest.indexOf('/');
      if (slashIdx < 0) {
        return rest.length > 0 ? '' : null;
      }
      return rest.substring(slashIdx + 1);
    }
  }
  if (normPath === '/root') {
    return '';
  }
  if (normPath.startsWith('/root/')) {
    return normPath.substring('/root/'.length);
  }
  return null;
}

export function cwdPathsEquivalent(pathA: string, pathB: string): boolean {
  let a = pathA.trim();
  let b = pathB.trim();
  while (a.length > 1 && a.endsWith('/')) {
    a = a.substring(0, a.length - 1);
  }
  while (b.length > 1 && b.endsWith('/')) {
    b = b.substring(0, b.length - 1);
  }
  if (a === b) {
    return true;
  }
  const relA = stripHomePrefixToRel(a);
  const relB = stripHomePrefixToRel(b);
  if (relA !== null && relB !== null) {
    return relA === relB;
  }
  return false;
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
  const cleanPath = path.trim();
  let cleanCwd = cwd.trim();
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
