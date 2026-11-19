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

export interface AgentCardDto {
  agent_id: string;
  workspace_id: string;
  cwd: string;
  title: string;
  provider: string;
  provider_label: string;
  model: string | null;
  effort: string | null;
  mode: string | CustomSessionModeDto;
  status: AgentStatusDto;
  status_label: string;
  is_running: boolean;
  is_awaiting_permission: boolean;
  parent_agent_id: string | null;
  latest_seq: number;
  updated_at_rfc3339: string;
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
}

export interface PromptBadgeDto {
  kind: string;
  label: string;
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
  headers?: string[];
  rows?: string[][];
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
  label_includes_failure: boolean;
}

export interface TurnLiveSegmentDto {
  kind: string;
  key: string;
  first_seq?: number;
  items?: TimelineItemDto[];
  summary?: WorkSummaryResultDto;
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
  is_compaction_command: boolean;
  folded_steps: TimelineItemDto[];
  work_counts: TurnWorkCountsDto;
  work_summary_label: string;
  work_summary_live_label: string;
  work_summary_includes_failure: boolean;
  thought_duration_ms: number | null;
  turn_duration_ms: number | null;
  terminal_reply: TimelineItemDto | null;
  notices: TimelineItemDto[];
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
  rows_data: TerminalRowDto[];
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
  createdAtMs: number;
}

export interface ComposerMentionCandidate {
  key: string;
  kind: string; // 'diff' | 'staged' | 'terminal' | 'file'
  tokenLabel: string;
  title: string;
  subtitle: string;
  filePath: string;
  gitStatus: string;
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
    return 'High';
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
  return 'High';
}

export function formatShortEffortLabel(effort: string | null | undefined): string {
  if (effort === null || effort === undefined || effort.length === 0) {
    return 'High';
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
  return 'High';
}

export function normalizeModeSlug(raw: string | CustomSessionModeDto | null | undefined): string {
  if (raw === null || raw === undefined) {
    return 'accept_edits';
  }
  if (typeof raw === 'string') {
    return raw.length > 0 ? raw : 'default';
  }
  if (raw.custom !== undefined && raw.custom.length > 0) {
    return raw.custom;
  }
  return 'default';
}

export function formatModeLabel(modeSlug: string): string {
  if (modeSlug === 'default') {
    return 'Always Ask';
  }
  if (modeSlug === 'plan') {
    return 'Plan Mode';
  }
  if (modeSlug === 'accept_edits') {
    return 'Accept Edits';
  }
  if (modeSlug === 'full_access') {
    return 'Full Access';
  }
  return modeSlug;
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
