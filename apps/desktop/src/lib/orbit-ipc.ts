import { Channel, invoke } from "@tauri-apps/api/core";

export type ProviderKind = "antigravity" | "codex" | "claude_code";
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
  projects: ClientProjectView[];
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
  mode: SessionMode;
  status_label: string;
  is_running: boolean;
  is_awaiting_permission: boolean;
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

export type TimelineItemView =
  | {
      item_type: "user_prompt";
      seq: number;
      message_id: string;
      text: string;
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

  pairHost: (pairingUri: string) => invoke<HostCardView>("pair_host", { pairingUri }),
  pairSshHost: (sshAlias: string, remotePort?: number | null) =>
    invoke<HostCardView>("pair_ssh_host", { sshAlias, remotePort: remotePort ?? null }),
  removeHost: (hostId: string) => invoke<void>("remove_host", { hostId }),
  updateHostConfig: (params: {
    hostId: string;
    customName?: string | null;
    routeMode?: HostRouteMode;
    sshAlias?: string | null;
    remoteDaemonPort?: number;
    customDirectEndpoint?: string | null;
    relayUrl?: string | null;
    reconnect?: boolean;
  }) => invoke<HostCardView>("update_host_config", params),
  updateHostRelay: (hostId: string, relayUrl: string | null) =>
    invoke<void>("update_host_relay", { hostId, relayUrl }),
  upgradeRemoteDaemon: (hostId: string, sshAlias: string, remotePort?: number | null) =>
    invoke<string>("upgrade_remote_daemon", {
      hostId,
      sshAlias,
      remotePort: remotePort ?? null,
    }),

  upsertProject: (params: {
    projectId?: string | null;
    hostId: string;
    name: string;
    directories: string[];
  }) => invoke<ClientProjectView>("upsert_project", params),
  removeProject: (projectId: string) => invoke<void>("remove_project", { projectId }),
  listDirectory: (hostId: string, path: string | null) =>
    invoke<void>("list_directory", { hostId, path }),

  connectHost: (hostId: string) => invoke<void>("connect_host", { hostId }),
  refreshHost: (hostId: string) => invoke<void>("refresh_host", { hostId }),
  refreshQuotas: (hostId: string) => invoke<void>("refresh_quotas", { hostId }),

  createAgent: (params: {
    hostId: string;
    cwd: string;
    extraDirs?: string[];
    provider: ProviderKind;
    model?: string | null;
    mode: SessionMode;
    initialPrompt: string;
    worktreeBranch: string | null;
  }) => invoke<string>("create_agent", params),

  openAgentTimeline: (hostId: string, agentId: string) =>
    invoke<void>("open_agent_timeline", { hostId, agentId }),

  sendPrompt: (hostId: string, agentId: string, prompt: string) =>
    invoke<void>("send_prompt", { hostId, agentId, prompt }),

  interruptAgent: (hostId: string, agentId: string) =>
    invoke<void>("interrupt_agent", { hostId, agentId }),

  setSessionMode: (hostId: string, agentId: string, mode: SessionMode) =>
    invoke<void>("set_session_mode", { hostId, agentId, mode }),

  setAgentModel: (hostId: string, agentId: string, model: string | null) =>
    invoke<void>("set_agent_model", { hostId, agentId, model }),

  switchAgentProvider: (hostId: string, agentId: string, provider: ProviderKind) =>
    invoke<void>("switch_agent_provider", { hostId, agentId, provider }),

  archiveAgent: (hostId: string, agentId: string) =>
    invoke<void>("archive_agent", { hostId, agentId }),

  resolvePermission: (
    hostId: string,
    agentId: string,
    permissionRequestId: string,
    decision: string,
  ) =>
    invoke<void>("resolve_permission", {
      hostId,
      agentId,
      permissionRequestId,
      decision,
    }),

  answerQuestion: (
    hostId: string,
    agentId: string,
    questionRequestId: string,
    answers: string[][],
  ) =>
    invoke<void>("answer_question", {
      hostId,
      agentId,
      questionRequestId,
      answers,
    }),

  fetchGitStatus: (hostId: string, cwd: string) =>
    invoke<void>("fetch_git_status", { hostId, cwd }),

  fetchGitDiff: (hostId: string, cwd: string, path: string | null, staged: boolean) =>
    invoke<void>("fetch_git_diff", { hostId, cwd, path, staged }),

  startTerminal: (hostId: string, cwd: string, cols: number, rows: number) =>
    invoke<void>("start_terminal", { hostId, cwd, cols, rows }),

  sendTerminalInput: (hostId: string, data: string) =>
    invoke<void>("send_terminal_input", { hostId, data }),

  resizeTerminal: (hostId: string, cols: number, rows: number) =>
    invoke<void>("resize_terminal", { hostId, cols, rows }),

  listSshHosts: async (): Promise<SshHostItem[]> => {
    if (!isTauriRuntime()) return [];
    return invoke("list_ssh_hosts");
  },

  provisionSshRelay: (params: {
    targetHostId: string;
    sshAlias: string;
    fallbackHostname: string;
    relayPort: number;
  }) => invoke<string>("provision_ssh_relay", params),

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
