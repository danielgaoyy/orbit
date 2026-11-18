import { OrbitHarmonyCore } from 'liborbit_ohos.so';

export interface HostCardDto {
  host_id: string;
  display_name: string;
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
}

export interface WorkspaceDto {
  workspace_id: string;
  title: string;
  cwd: string;
  git_branch: string | null;
  is_worktree: boolean;
}

export interface AgentCardDto {
  agent_id: string;
  workspace_id: string;
  cwd: string;
  title: string;
  provider: string;
  provider_label: string;
  model: string | null;
  status_label: string;
  is_running: boolean;
  is_awaiting_permission: boolean;
  latest_seq: number;
}

export interface QuotaWindowDto {
  label: string;
  used_percent: number;
}

export interface ProviderQuotaDto {
  provider: string;
  account_label: string | null;
  windows: QuotaWindowDto[];
  credits_remaining: number | null;
}

export interface HostDetailDto {
  revision: number;
  host: HostCardDto;
  workspaces: WorkspaceDto[];
  agents: AgentCardDto[];
  quotas: ProviderQuotaDto[];
  active_terminal_id: string | null;
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
  text?: string;
  streaming?: boolean;
  blocks?: RichBlockDto[];
  call_id?: string;
  status?: string;
  summary_title?: string;
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

export interface AgentTimelineDto {
  revision: number;
  host_id: string;
  agent: AgentCardDto | null;
  items: TimelineItemDto[];
}

export interface GitReviewDto {
  revision: number;
  host_id: string;
  cwd: string;
  diff_files: DiffFileDto[];
}

export interface TerminalSpanDto {
  text: string;
  fg_hex: string;
  bg_hex: string | null;
  bold: boolean;
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

interface CoreNotificationDto {
  kind: string;
  revision: number;
  host_id?: string;
  agent_id?: string;
  cwd?: string;
}

/**
 * HarmonyOS 7+ (API 26) ArkUI State Management V2 (`@ObservedV2` + `@Trace`) Store
 * backed 100% by Rust `liborbit_ohos.so` (`OrbitHarmonyCore`).
 */
@ObservedV2
export class OrbitHarmonyStore {
  private static instance: OrbitHarmonyStore | null = null;
  private core: OrbitHarmonyCore | null = null;

  @Trace public hosts: HostCardDto[] = [];
  @Trace public totalPendingApprovals: number = 0;
  @Trace public selectedHostId: string = '';
  @Trace public currentHostDetail: HostDetailDto | null = null;
  @Trace public selectedAgentId: string = '';
  @Trace public timelineItems: TimelineItemDto[] = [];
  @Trace public gitDiffFiles: DiffFileDto[] = [];
  @Trace public terminalRows: TerminalRowDto[] = [];
  @Trace public lastError: string = '';

  public static getInstance(): OrbitHarmonyStore {
    if (OrbitHarmonyStore.instance === null) {
      OrbitHarmonyStore.instance = new OrbitHarmonyStore();
    }
    return OrbitHarmonyStore.instance;
  }

  public initialize(dbPath: string): void {
    if (this.core !== null) {
      return;
    }
    try {
      this.core = new OrbitHarmonyCore(dbPath);
      this.core.subscribeEvents((err: Error | null, payloadJson: string) => {
        if (err !== null || payloadJson.length === 0) {
          return;
        }
        this.handleCoreEvent(payloadJson);
      });
      this.reloadFleet();
      this.core.connectAllHosts();
    } catch (e) {
      this.lastError = `Native Rust core init error: ${(e as Error).message}`;
    }
  }

  private handleCoreEvent(payloadJson: string): void {
    try {
      const ev = JSON.parse(payloadJson) as CoreNotificationDto;
      if (ev.kind === 'fleet_changed') {
        this.reloadFleet();
        if (this.selectedHostId.length > 0) {
          this.reloadHost(this.selectedHostId);
        }
      } else if (ev.kind === 'host_changed' && ev.host_id === this.selectedHostId) {
        this.reloadFleet();
        this.reloadHost(this.selectedHostId);
      } else if (
        ev.kind === 'timeline_changed' &&
        ev.host_id === this.selectedHostId &&
        ev.agent_id === this.selectedAgentId
      ) {
        this.reloadTimeline();
      } else if (ev.kind === 'git_changed' && ev.host_id === this.selectedHostId) {
        if (ev.cwd !== undefined) {
          this.reloadGitDiff(ev.cwd);
        }
      } else if (ev.kind === 'terminal_changed' && ev.host_id === this.selectedHostId) {
        this.reloadTerminal();
      }
    } catch (_) {
      // Ignore malformed event
    }
  }

  public reloadFleet(): void {
    if (this.core === null) {
      return;
    }
    const snap = JSON.parse(this.core.fleetOverviewJson()) as FleetOverviewDto;
    this.hosts = snap.hosts;
    this.totalPendingApprovals = snap.total_pending_approvals;
    if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
      this.selectHost(snap.hosts[0].host_id);
    }
  }

  public selectHost(hostId: string): void {
    this.selectedHostId = hostId;
    this.reloadHost(hostId);
    if (this.core !== null) {
      this.core.refreshQuotas(hostId);
    }
  }

  public reloadHost(hostId: string): void {
    if (this.core === null || hostId.length === 0) {
      return;
    }
    const raw = this.core.hostDetailJson(hostId);
    const detail = JSON.parse(raw) as HostDetailDto | null;
    this.currentHostDetail = detail;
    if (detail !== null && this.selectedAgentId.length === 0 && detail.agents.length > 0) {
      this.selectAgent(detail.agents[0].agent_id);
    }
  }

  public selectAgent(agentId: string): void {
    this.selectedAgentId = agentId;
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.openAgentTimeline(this.selectedHostId, agentId);
      this.reloadTimeline();
    }
  }

  public reloadTimeline(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const tl = JSON.parse(
      this.core.agentTimelineJson(this.selectedHostId, this.selectedAgentId)
    ) as AgentTimelineDto;
    this.timelineItems = tl.items;
  }

  public reloadGitDiff(cwd: string): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    const gr = JSON.parse(this.core.gitReviewJson(this.selectedHostId, cwd, null)) as GitReviewDto;
    this.gitDiffFiles = gr.diff_files;
  }

  public reloadTerminal(): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    const screen = JSON.parse(
      this.core.terminalScreenJson(this.selectedHostId)
    ) as TerminalScreenDto | null;
    if (screen !== null) {
      this.terminalRows = screen.rows_data;
    }
  }

  public pairHost(uri: string): void {
    if (this.core === null || uri.trim().length === 0) {
      return;
    }
    try {
      const card = JSON.parse(this.core.pairHostJson(uri.trim())) as HostCardDto;
      this.lastError = '';
      this.reloadFleet();
      this.selectHost(card.host_id);
    } catch (e) {
      this.lastError = (e as Error).message;
    }
  }

  public createAgent(cwd: string, provider: string, mode: string, prompt: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || prompt.trim().length === 0) {
      return;
    }
    this.core.createAgent(this.selectedHostId, cwd, provider, mode, prompt.trim(), null);
  }

  public sendPrompt(prompt: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.sendPrompt(this.selectedHostId, this.selectedAgentId, prompt);
  }

  public interruptCurrentAgent(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.interruptAgent(this.selectedHostId, this.selectedAgentId);
  }

  public resolvePermission(requestId: string, decision: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.resolvePermission(this.selectedHostId, this.selectedAgentId, requestId, decision);
  }

  public answerSingleQuestion(requestId: string, option: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.answerQuestion(
      this.selectedHostId,
      this.selectedAgentId,
      requestId,
      JSON.stringify([[option]])
    );
  }

  public openGitReview(cwd: string): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.fetchGitStatus(this.selectedHostId, cwd);
    this.core.fetchGitDiff(this.selectedHostId, cwd, null, false);
    this.reloadGitDiff(cwd);
  }

  public startPty(cwd: string): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.startTerminal(this.selectedHostId, cwd, 84, 28);
  }

  public sendTerminalKeys(keys: string): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.sendTerminalKeys(this.selectedHostId, keys);
  }
}
