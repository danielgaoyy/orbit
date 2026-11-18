import { Channel, invoke } from "@tauri-apps/api/core";

export type ProviderKind = "antigravity" | "codex" | "claude_code";
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
  | "plan"
  | "accept_edits"
  | "full_access"
  | { custom: string };

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
}

export interface AppUpdateProgress {
  progress: number;
  status: string;
  done: boolean;
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
}

export interface FleetOverviewView {
  revision: number;
  total_pending_approvals: number;
  hosts: HostCardView[];
  projects?: ClientProjectView[];
}

export interface WorkspaceSummary {
  workspace_id: string;
  title: string;
  cwd: string;
  git_branch: string | null;
  is_worktree: boolean;
  updated_at: string;
}

export interface AgentCardView {
  agent_id: string;
  workspace_id: string;
  cwd: string;
  title: string;
  provider: ProviderKind;
  provider_label: string;
  model: string | null;
  effort: ReasoningEffort | null;
  mode: SessionMode;
  status_label: string;
  is_running: boolean;
  is_awaiting_permission: boolean;
  parent_agent_id: string | null;
  latest_seq: number;
  updated_at_rfc3339: string;
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
}

export interface ProviderModelCatalog {
  provider: ProviderKind;
  models: ProviderModelSpec[];
  configured_default_model: string | null;
  configured_default_effort: ReasoningEffort | null;
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
}

export interface InlineSpanView {
  text: string;
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  inline_code: boolean;
  link_url: string | null;
}

export interface HighlightedTokenView {
  text: string;
  fg_hex: string;
  bold: boolean;
  italic: boolean;
}

export interface HighlightedLineView {
  line_number: number;
  tokens: HighlightedTokenView[];
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
  | { kind: "horizontal_rule" };

export type DiffLineKind = "meta" | "hunk_header" | "context" | "added" | "removed";

export interface DiffLineView {
  kind: DiffLineKind;
  old_line: number | null;
  new_line: number | null;
  content: string;
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
}

export type TimelineItemView =
  | {
      item_type: "user_prompt";
      seq: number;
      message_id: string;
      text: string;
      badges?: PromptBadge[];
      blocks: RichBlockView[];
    }
  | {
      item_type: "assistant_thinking";
      seq: number;
      item_id: string;
      text: string;
    }
  | {
      item_type: "assistant_message";
      seq: number;
      item_id: string;
      text: string;
      streaming: boolean;
      blocks: RichBlockView[];
    }
  | {
      item_type: "tool_call";
      seq: number;
      call_id: string;
      status: "running" | "completed" | "failed" | "denied";
      summary_title: string;
      detail: ToolCallDetail;
      diff_files: DiffFileView[];
    }
  | {
      item_type: "permission_request";
      seq: number;
      request_id: string;
      title: string;
      detail: ToolCallDetail;
      allowed_decisions: string[];
      resolved_decision: string | null;
    }
  | {
      item_type: "question_request";
      seq: number;
      request_id: string;
      questions: QuestionItem[];
      resolved_answers: string[][] | null;
    }
  | {
      item_type: "notice";
      seq: number;
      level: string;
      message: string;
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

export interface SshHostItem {
  alias: string;
  host_name: string;
  user: string | null;
  port: number | null;
  display_target: string;
}

export interface StagedAttachmentInfo {
  path: string;
  filename: string;
  size_bytes: number;
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
      return `notice:${item.seq}`;
  }
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

  async getHostDetail(hostId: string): Promise<HostDetailView | null> {
    if (!isTauriRuntime()) return null;
    return invoke("get_host_detail", { hostId });
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

  pairSshHost: async (sshAlias: string, remotePort?: number | null): Promise<HostCardView> => {
    return invoke<HostCardView>("pair_ssh_host", {
      sshAlias,
      remotePort: remotePort ?? null,
    });
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
    model?: string | null;
    effort?: ReasoningEffort | null;
    mode: SessionMode;
    initialPrompt: string;
    initialDisplayPrompt?: string | null;
    initialBadges?: PromptBadge[];
    worktreeBranch: string | null;
  }): Promise<string> => {
    if (!isTauriRuntime()) return "";
    return invoke<string>("create_agent", {
      ...params,
      model: params.model ?? null,
      effort: params.effort ?? null,
      initialDisplayPrompt: params.initialDisplayPrompt ?? null,
      initialBadges: params.initialBadges ?? [],
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
    meta?: { displayPrompt?: string; badges?: PromptBadge[] },
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("send_prompt", {
      hostId,
      agentId,
      prompt,
      displayPrompt: meta?.displayPrompt ?? null,
      badges: meta?.badges ?? [],
    });
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
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("switch_agent_provider", {
      hostId,
      agentId,
      provider,
      model,
      effort,
    });
  },

  archiveAgent: async (hostId: string, agentId: string): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("archive_agent", { hostId, agentId });
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

  sendTerminalInputSlot: async (
    hostId: string,
    slot: number,
    data: string,
  ): Promise<void> => {
    if (!isTauriRuntime()) return;
    await invoke<void>("send_terminal_input_slot", { hostId, slot, data });
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

  getWorkspaceFiles: async (hostId: string, cwd: string): Promise<string[]> => {
    if (!isTauriRuntime()) return [];
    return invoke<string[]>("get_workspace_files", { hostId, cwd });
  },

  stageAttachment: async (
    filename: string,
    bytes: number[],
  ): Promise<StagedAttachmentInfo> => {
    if (!isTauriRuntime()) {
      return { path: `/tmp/${filename}`, filename, size_bytes: bytes.length };
    }
    return invoke<StagedAttachmentInfo>("stage_attachment", { filename, bytes });
  },

  provisionSshRelay: async (params: {
    targetHostId: string;
    sshAlias: string;
    fallbackHostname: string;
    relayPort: number;
  }): Promise<string> => {
    return invoke<string>("provision_ssh_relay", params);
  },

  getAppVersion: async (): Promise<string> => {
    if (!isTauriRuntime()) return "0.2.4";
    return invoke<string>("get_app_version");
  },

  checkAppUpdate: async (): Promise<ReleaseUpdateInfo> => {
    return invoke<ReleaseUpdateInfo>("check_app_update");
  },

  performAppSelfUpdate: async (
    release: ReleaseUpdateInfo,
    onProgress: (ev: AppUpdateProgress) => void,
  ): Promise<void> => {
    const ch = new Channel<AppUpdateProgress>();
    ch.onmessage = onProgress;
    return invoke<void>("perform_app_self_update", {
      release,
      onProgress: ch,
    });
  },
};
