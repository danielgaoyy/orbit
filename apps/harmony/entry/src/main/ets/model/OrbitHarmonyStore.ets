import { OrbitHarmonyCore } from 'liborbit_ohos.so';
import { common } from '@kit.AbilityKit';
import { scanBarcode, scanCore } from '@kit.ScanKit';
import { vibrator } from '@kit.SensorServiceKit';
import { pasteboard } from '@kit.BasicServicesKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import {
  AgentCardDto,
  AgentStageDto,
  ClientProjectDto,
  ComposerMentionCandidate,
  CoreNotificationDto,
  DiffFileDto,
  DiffLineDto,
  FleetOverviewDto,
  GitReviewDto,
  GitStatusSnapshotDto,
  HostCardDto,
  HostDetailDto,
  ModelOptionDto,
  PromptBadgeDto,
  ProviderModelCatalogDto,
  ProviderModelSpecDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  SlashCommandItem,
  StageTurnDto,
  TerminalRowDto,
  TerminalScreenDto,
  ThinkingOptionSpecDto,
  TimelineItemDto,
  ToolStepDescriptorDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatModeLabel,
  formatShortEffortLabel,
  normalizeModeSlug
} from './OrbitTypes';
import {
  GRAPHITE_DARK_PALETTE,
  OrbitThemePalette,
  OrbitTokens,
  resolveOrbitPalette
} from '../theme/OrbitTokens';

interface BuiltPromptPayload {
  wirePrompt: string;
  displayPrompt: string;
  badges: PromptBadgeDto[];
  badgesJson: string | null;
}

const DEFAULT_SLASH_COMMANDS: SlashCommandItem[] = [
  {
    command: '/plan',
    title: 'Architecture Plan',
    description: 'Draft step-by-step plan before editing',
    modeOverride: 'plan'
  },
  {
    command: '/review',
    title: 'Code Review',
    description: 'Attach working tree diff for engineering review',
    modeOverride: ''
  },
  {
    command: '/compact',
    title: 'Compact Context',
    description: 'Summarize and compress session context',
    modeOverride: ''
  }
];

@ObservedV2
export class OrbitHarmonyStore {
  private static instance: OrbitHarmonyStore | null = null;
  private core: OrbitHarmonyCore | null = null;
  private pendingDeepLink: string = '';
  private isNewSessionDraft: boolean = false;

  // Theme & Window Safe Area State
  @Trace public themeMode: string = 'dark';
  @Trace public systemIsDark: boolean = true;
  @Trace public palette: OrbitThemePalette = GRAPHITE_DARK_PALETTE;
  @Trace public safeTopVp: number = 0;
  @Trace public safeBottomVp: number = 0;
  @Trace public keyboardHeightVp: number = 0;
  @Trace public windowWidthVp: number = 390;
  @Trace public windowHeightVp: number = 844;

  // Shell & Contextual Sheet Visibility
  @Trace public sidebarOpen: boolean = false;
  @Trace public isSidebarCollapsed: boolean = false;
  @Trace public showInlineDiffInspector: boolean = false;
  @Trace public showGitDiffSheet: boolean = false;
  @Trace public showPtySheet: boolean = false;
  @Trace public showModelPickerSheet: boolean = false;
  @Trace public showContextPickerSheet: boolean = false;
  @Trace public showHostSettingsSheet: boolean = false;
  @Trace public showDirBrowser: boolean = false;

  // Multi-Host Fleet & Workspace State
  @Trace public nativeAvailable: boolean = false;
  @Trace public hosts: HostCardDto[] = [];
  @Trace public projects: ClientProjectDto[] = [];
  @Trace public totalPendingApprovals: number = 0;
  @Trace public selectedHostId: string = '';
  @Trace public currentHostDetail: HostDetailDto | null = null;
  @Trace public selectedAgentId: string = '';
  @Trace public currentAgent: AgentCardDto | null = null;
  @Trace public activeCwd: string = '~';

  // Semantic Stage & Decision Dock State
  @Trace public stageRevision: number = 0;
  @Trace public stageTurns: StageTurnDto[] = [];
  @Trace public pendingPermissions: TimelineItemDto[] = [];
  @Trace public pendingQuestions: TimelineItemDto[] = [];
  @Trace public sessionDiffs: DiffFileDto[] = [];
  @Trace public expandedTurnKeys: string[] = [];
  @Trace public expandedToolIds: string[] = [];

  // OmniComposer & Context State
  @Trace public composerText: string = '';
  @Trace public composerProvider: string = 'antigravity';
  @Trace public composerModel: string = '';
  @Trace public composerEffort: string = 'high';
  @Trace public composerMode: string = 'accept_edits';
  @Trace public composerBadges: PromptBadgeDto[] = [];
  @Trace public quotedSnippets: QuotedDiffSnippet[] = [];
  @Trace public worktreeBranch: string = '';
  @Trace public workspaceFiles: string[] = [];
  @Trace public activeQueuedPrompts: QueuedPromptItem[] = [];
  public queuedPromptsByAgent: Map<string, QueuedPromptItem[]> = new Map<string, QueuedPromptItem[]>();

  // Git Working Tree Inspector State
  @Trace public gitStatus: GitStatusSnapshotDto | null = null;
  @Trace public gitDiffFiles: DiffFileDto[] = [];
  @Trace public gitSelectedPath: string = '';
  @Trace public gitStagedMode: boolean = false;

  // Detached PTY Multi-Terminal State
  @Trace public terminalScreen: TerminalScreenDto | null = null;
  @Trace public terminalRows: TerminalRowDto[] = [];
  @Trace public terminalCols: number = 84;
  @Trace public terminalGridRows: number = 28;
  @Trace public ctrlSticky: boolean = false;
  @Trace public altSticky: boolean = false;

  // Status & Non-Intrusive Feedback
  @Trace public lastError: string = '';
  @Trace public statusNote: string = '';
  @Trace public toastMessage: string = '';
  @Trace public toastIsError: boolean = false;
  @Trace public clipboardPairingUri: string = '';
  private dismissedClipboardUri: string = '';
  private toastTimerId: number = -1;

  public static getInstance(): OrbitHarmonyStore {
    if (OrbitHarmonyStore.instance === null) {
      OrbitHarmonyStore.instance = new OrbitHarmonyStore();
    }
    return OrbitHarmonyStore.instance;
  }

  public get slashCommands(): SlashCommandItem[] {
    return DEFAULT_SLASH_COMMANDS;
  }

  public initialize(dbPath: string): void {
    if (this.core !== null) {
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        this.pendingDeepLink = '';
        this.pairHost(uri);
      }
      return;
    }
    try {
      this.core = new OrbitHarmonyCore(dbPath, true);
      this.nativeAvailable = true;
      this.core.subscribeEvents((err: Error | null, payloadJson: string) => {
        if (err !== null || payloadJson.length === 0) {
          return;
        }
        this.handleCoreEvent(payloadJson);
      });
      this.reloadFleet();
      this.core.connectAllHosts();
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        this.pendingDeepLink = '';
        this.pairHost(uri);
      }
    } catch (e) {
      this.nativeAvailable = false;
      const msg = (e as Error).message;
      this.statusNote = msg !== undefined && msg.length > 0
        ? `Preview mode (${msg})`
        : 'Preview mode (liborbit_ohos.so not loaded)';
    }
  }

  // Theme & Layout Management

  public setThemeMode(mode: string): void {
    this.themeMode = mode;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    this.triggerHaptic(8);
  }

  public setSystemDark(isDark: boolean): void {
    this.systemIsDark = isDark;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
  }

  public showToast(message: string, isError: boolean = false): void {
    const clean = message.trim();
    if (clean.length === 0) {
      return;
    }
    if (this.toastTimerId !== -1) {
      clearTimeout(this.toastTimerId);
      this.toastTimerId = -1;
    }
    this.toastIsError = isError;
    this.toastMessage = clean;
    if (isError) {
      this.lastError = clean;
    } else {
      this.statusNote = clean;
    }
    this.toastTimerId = setTimeout(() => {
      this.toastMessage = '';
      this.toastTimerId = -1;
    }, 2600);
  }

  public dismissToast(): void {
    if (this.toastTimerId !== -1) {
      clearTimeout(this.toastTimerId);
      this.toastTimerId = -1;
    }
    this.toastMessage = '';
  }

  public effectiveSidebarWidth(): number {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN || this.isSidebarCollapsed) {
      return 0;
    }
    return OrbitTokens.SIDEBAR_FOLD_WIDTH;
  }

  public canShowInlineDiffInspector(): boolean {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      return false;
    }
    if (this.windowWidthVp >= OrbitTokens.BP_EXPANDED_MIN) {
      return true;
    }
    return (this.windowWidthVp - this.effectiveSidebarWidth()) >= 780;
  }

  public toggleSidebar(): void {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = !this.sidebarOpen;
      this.triggerHaptic(8);
      return;
    }
    this.isSidebarCollapsed = !this.isSidebarCollapsed;
    this.sidebarOpen = !this.isSidebarCollapsed;
    if (this.canShowInlineDiffInspector() && this.showGitDiffSheet) {
      this.showGitDiffSheet = false;
      this.showInlineDiffInspector = true;
    } else if (!this.canShowInlineDiffInspector() && this.showInlineDiffInspector) {
      this.showInlineDiffInspector = false;
    }
    this.triggerHaptic(8);
  }

  public updateWindowSize(widthVp: number, heightVp: number): void {
    if (heightVp > 0) {
      this.windowHeightVp = heightVp;
    }
    if (widthVp > 0) {
      this.updateWindowWidth(widthVp);
    }
  }

  public updateWindowWidth(widthVp: number): void {
    const prevWasPhone = this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN;
    const nextIsPhone = widthVp < OrbitTokens.BP_FOLDABLE_MIN;
    const prevWasExpanded = this.windowWidthVp >= OrbitTokens.BP_EXPANDED_MIN;
    const nextIsExpanded = widthVp >= OrbitTokens.BP_EXPANDED_MIN;
    this.windowWidthVp = widthVp;
    if (prevWasPhone && !nextIsPhone) {
      this.sidebarOpen = !this.isSidebarCollapsed;
    } else if (!prevWasPhone && nextIsPhone) {
      this.sidebarOpen = false;
    }
    if (!prevWasExpanded && nextIsExpanded) {
      this.showGitDiffSheet = false;
      this.showInlineDiffInspector = true;
      if (this.selectedHostId.length > 0) {
        this.refreshGitReview(
          this.gitStagedMode,
          this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
        );
      }
    } else if (!this.canShowInlineDiffInspector() && this.showInlineDiffInspector) {
      this.showInlineDiffInspector = false;
    }
  }

  public updateSafeArea(topVp: number, bottomVp: number, keyboardVp: number): void {
    this.safeTopVp = Math.max(0, Math.round(topVp));
    this.safeBottomVp = Math.max(0, Math.round(bottomVp));
    this.keyboardHeightVp = Math.max(0, Math.round(keyboardVp));
  }

  // Lifecycle & Network Hooks

  public onAppForeground(): void {
    if (this.core !== null) {
      try {
        this.core.onAppForeground();
        this.reloadFleet();
        if (this.selectedHostId.length > 0) {
          this.reloadHost(this.selectedHostId);
        }
        if (this.selectedAgentId.length > 0) {
          this.reloadStage();
        }
      } catch (_) {
        // Ignore transient foreground error
      }
    }
  }

  public onAppBackground(): void {
    if (this.core !== null) {
      try {
        this.core.onAppBackground();
      } catch (_) {
        // Ignore transient background error
      }
    }
  }

  public onNetworkChanged(): void {
    if (this.core !== null) {
      try {
        this.core.onNetworkChanged();
      } catch (_) {
        // Ignore transient network callback error
      }
    }
  }

  public handleDeepLinkUri(uri: string): void {
    const clean = uri.trim();
    if (!clean.startsWith('orbit://')) {
      return;
    }
    if (this.core === null) {
      this.pendingDeepLink = clean;
      return;
    }
    this.pairHost(clean);
  }

  // Haptics, Clipboard & Native ScanKit QR Pairing

  public triggerHaptic(durationMs: number = 12): void {
    try {
      const effect: vibrator.VibrateTime = {
        type: 'time',
        duration: durationMs
      };
      const attr: vibrator.VibrateAttribute = {
        id: 0,
        usage: 'touch'
      };
      vibrator.startVibration(effect, attr).catch(() => {});
    } catch (_) {
      // Haptics optional on emulators / previewer
    }
  }

  public copyToClipboard(content: string, label: string = 'Copied to clipboard'): void {
    if (content.length === 0) {
      return;
    }
    try {
      const sysBoard = pasteboard.getSystemPasteboard();
      const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, content);
      sysBoard.setData(data).then(() => {
        this.showToast(label, false);
        this.triggerHaptic(10);
      }).catch(() => {
        this.showToast('Failed to copy to clipboard', true);
      });
    } catch (_) {
      this.showToast('Failed to copy to clipboard', true);
    }
  }

  public pasteAuthorizedPairingUri(onFilled?: (uri: string) => void): void {
    try {
      const sysBoard = pasteboard.getSystemPasteboard();
      sysBoard.getData().then((data: pasteboard.PasteData) => {
        const primary = data.getPrimaryText();
        if (primary === undefined || primary === null) {
          this.showToast('Clipboard is empty', true);
          return;
        }
        const raw = primary.trim();
        const match = /orbit:\/\/pair[^\s"'<>]*/.exec(raw);
        if (match !== null && match[0].length > 'orbit://pair'.length) {
          const candidate = match[0].trim();
          if (onFilled !== undefined) {
            onFilled(candidate);
          }
          this.pairHost(candidate);
        } else if (raw.length > 0 && onFilled !== undefined) {
          onFilled(raw);
        } else {
          this.showToast('No orbit://pair URI found in clipboard', true);
        }
      }).catch(() => {
        this.showToast('Unable to read clipboard', true);
      });
    } catch (_) {
      // Pasteboard unavailable in previewer
    }
  }

  public dismissClipboardPairingBanner(): void {
    if (this.clipboardPairingUri.length > 0) {
      this.dismissedClipboardUri = this.clipboardPairingUri;
      this.clipboardPairingUri = '';
      this.triggerHaptic(6);
    }
  }

  public pairFromClipboard(): void {
    const uri = this.clipboardPairingUri.trim();
    if (uri.length === 0) {
      return;
    }
    this.dismissedClipboardUri = uri;
    this.clipboardPairingUri = '';
    this.pairHost(uri);
  }

  public openHostSettings(): void {
    this.showHostSettingsSheet = true;
    this.triggerHaptic(8);
  }

  public scanQrAndPair(context: common.Context | undefined): void {
    if (context === undefined) {
      this.showToast('Unable to acquire UI context for QR scanner', true);
      return;
    }
    if (
      canIUse('SystemCapability.Multimedia.Scan.ScanBarcode') &&
      canIUse('SystemCapability.Multimedia.Scan.Core')
    ) {
      try {
        const options: scanBarcode.ScanOptions = {
          scanTypes: [scanCore.ScanType.QR_CODE],
          enableMultiMode: false,
          enableAlbum: true
        };
        scanBarcode.startScanForResult(context, options)
          .then((result: scanBarcode.ScanResult) => {
            const scanned = result.originalValue.trim();
            if (scanned.length > 0) {
              this.triggerHaptic(18);
              this.pairHost(scanned);
            }
          })
          .catch((_err: Error) => {
            // User canceled scan or ScanKit unavailable on simulator
          });
      } catch (e) {
        this.showToast(`QR Scanner unavailable: ${(e as Error).message}`, true);
      }
    } else {
      this.showToast('System QR scanner is not supported on this device', true);
    }
  }

  // Core Event Loop

  private handleCoreEvent(payloadJson: string): void {
    try {
      const ev = JSON.parse(payloadJson) as CoreNotificationDto;
      if (ev.kind === 'fleet_changed') {
        this.reloadFleet();
        if (this.selectedHostId.length > 0) {
          this.reloadHost(this.selectedHostId);
        }
      } else if (ev.kind === 'host_changed') {
        this.reloadFleet();
        if (ev.host_id === this.selectedHostId && this.selectedHostId.length > 0) {
          this.reloadHost(this.selectedHostId);
          this.reloadWorkspaceFiles();
        }
      } else if (
        ev.kind === 'timeline_changed' &&
        ev.host_id === this.selectedHostId &&
        ev.agent_id === this.selectedAgentId
      ) {
        this.reloadStage();
      } else if (
        ev.kind === 'git_changed' &&
        ev.host_id === this.selectedHostId &&
        this.selectedHostId.length > 0
      ) {
        // Do NOT require raw string equality cwd === this.activeCwd (~/... vs /Users/...).
        // Always refresh gitReview and workspaceFiles using this.activeCwd.
        this.reloadGitDiff(this.activeCwd);
        this.reloadWorkspaceFiles();
      } else if (ev.kind === 'terminal_changed' && ev.host_id === this.selectedHostId) {
        this.reloadTerminal();
      }
    } catch (_) {
      // Ignore malformed notification payload
    }
  }

  // Fleet, Host & Directory Operations

  public reloadFleet(): void {
    if (this.core === null) {
      return;
    }
    try {
      const snap = JSON.parse(this.core.fleetOverviewJson()) as FleetOverviewDto;
      this.hosts = snap.hosts;
      this.projects = snap.projects;
      this.totalPendingApprovals = snap.total_pending_approvals;
      if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
        this.selectHost(snap.hosts[0].host_id);
      }
    } catch (e) {
      this.lastError = (e as Error).message;
    }
  }

  private agentQueueKey(hostId: string, agentId: string): string {
    return `${hostId}::${agentId}`;
  }

  private syncActiveQueuedPrompts(): void {
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      this.activeQueuedPrompts = [];
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const list = this.queuedPromptsByAgent.get(key);
    if (list === undefined || list.length === 0) {
      this.activeQueuedPrompts = [];
      return;
    }
    const copy: QueuedPromptItem[] = [];
    for (let i = 0; i < list.length; i++) {
      copy.push(list[i]);
    }
    this.activeQueuedPrompts = copy;
  }

  private drainQueuedPromptsForAgentIfIdle(hostId: string, agent: AgentCardDto): void {
    if (this.core === null || hostId.length === 0 || agent.agent_id.length === 0) {
      return;
    }
    if (
      agent.is_running ||
      agent.is_awaiting_permission ||
      agent.status.state === 'running' ||
      agent.status.state === 'initializing' ||
      agent.status.state === 'awaiting_permission' ||
      agent.status.state === 'closed'
    ) {
      return;
    }
    if (
      agent.agent_id === this.selectedAgentId &&
      (this.pendingPermissions.length > 0 || this.pendingQuestions.length > 0)
    ) {
      return;
    }
    const key = this.agentQueueKey(hostId, agent.agent_id);
    const queue = this.queuedPromptsByAgent.get(key);
    if (queue === undefined || queue.length === 0) {
      return;
    }
    const nextItem = queue[0];
    const rest: QueuedPromptItem[] = [];
    for (let i = 1; i < queue.length; i++) {
      rest.push(queue[i]);
    }
    if (rest.length > 0) {
      this.queuedPromptsByAgent.set(key, rest);
    } else {
      this.queuedPromptsByAgent.delete(key);
    }
    this.syncActiveQueuedPrompts();
    try {
      this.core.sendPromptWithMeta(
        hostId,
        agent.agent_id,
        nextItem.wirePrompt,
        nextItem.displayPrompt,
        nextItem.badgesJson
      );
      if (hostId === this.selectedHostId && agent.agent_id === this.selectedAgentId) {
        this.reloadStage();
      }
      this.showToast('Sent queued prompt', false);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public resolveWorkspaceBranch(cwd: string): string {
    if (cwd.trim().length === 0) {
      return '';
    }
    const st = this.gitStatus;
    if (
      st !== null &&
      st.branch !== null &&
      st.branch.trim().length > 0 &&
      (cwdPathsEquivalent(cwd, this.activeCwd) || cwdPathsEquivalent(cwd, st.cwd))
    ) {
      return st.branch.trim();
    }
    const detail = this.currentHostDetail;
    if (detail !== null) {
      for (let i = 0; i < detail.discovered_projects.length; i++) {
        const dp = detail.discovered_projects[i];
        if (
          cwdPathsEquivalent(dp.path, cwd) &&
          dp.git_branch !== null &&
          dp.git_branch.trim().length > 0
        ) {
          return dp.git_branch.trim();
        }
      }
      for (let i = 0; i < detail.workspaces.length; i++) {
        const ws = detail.workspaces[i];
        if (
          cwdPathsEquivalent(ws.cwd, cwd) &&
          ws.git_branch !== null &&
          ws.git_branch.trim().length > 0
        ) {
          return ws.git_branch.trim();
        }
      }
    }
    return '';
  }

  public selectHost(hostId: string): void {
    if (hostId.length === 0) {
      return;
    }
    const changed = this.selectedHostId !== hostId;
    this.selectedHostId = hostId;
    if (changed) {
      this.selectedAgentId = '';
      this.currentAgent = null;
      this.stageTurns = [];
      this.pendingPermissions = [];
      this.pendingQuestions = [];
      this.sessionDiffs = [];
      this.syncActiveQueuedPrompts();
    }
    this.reloadHost(hostId);
    if (this.core !== null) {
      this.core.connectHost(hostId);
      this.core.refreshHost(hostId);
      this.core.refreshQuotas(hostId);
      this.core.refreshProviderModels(hostId, false);
    }
  }

  public reloadHost(hostId: string): void {
    if (this.core === null || hostId.length === 0) {
      return;
    }
    try {
      const detail = JSON.parse(this.core.hostDetailJson(hostId)) as HostDetailDto | null;
      this.currentHostDetail = detail;
      if (detail === null) {
        return;
      }

      // Seamlessly upgrade optimistic session ID when daemon confirms AgentCreated
      const replacedId = this.selectedAgentId.startsWith('opt_')
        ? (detail.replaced_optimistic_agents?.[this.selectedAgentId] ?? detail.last_created_agent_id)
        : null;
      if (this.selectedAgentId.startsWith('opt_') && replacedId !== null && replacedId.length > 0) {
        const stillHasOpt = detail.agents.some((a: AgentCardDto) => a.agent_id === this.selectedAgentId);
        if (!stillHasOpt) {
          const oldKey = this.agentQueueKey(hostId, this.selectedAgentId);
          const newAgentId = replacedId;
          const newKey = this.agentQueueKey(hostId, newAgentId);
          const pendingQueue = this.queuedPromptsByAgent.get(oldKey);
          if (pendingQueue !== undefined && pendingQueue.length > 0) {
            const existing = this.queuedPromptsByAgent.get(newKey) ?? [];
            this.queuedPromptsByAgent.delete(oldKey);
            this.queuedPromptsByAgent.set(newKey, existing.concat(pendingQueue));
          }
          this.selectedAgentId = newAgentId;
          this.isNewSessionDraft = false;
          this.syncActiveQueuedPrompts();
          this.core.openAgentTimeline(hostId, this.selectedAgentId);
          this.reloadStage();
        }
      }

      if (this.activeCwd === '~' || this.activeCwd.length === 0) {
        if (detail.workspaces.length > 0) {
          this.setActiveCwd(detail.workspaces[0].cwd);
        } else if (detail.discovered_projects.length > 0) {
          this.setActiveCwd(detail.discovered_projects[0].path);
        } else if (detail.agents.length > 0) {
          this.setActiveCwd(detail.agents[0].cwd);
        }
      }

      if (this.selectedAgentId.length > 0) {
        for (let i = 0; i < detail.agents.length; i++) {
          if (detail.agents[i].agent_id === this.selectedAgentId) {
            this.currentAgent = detail.agents[i];
            break;
          }
        }
      } else if (
        !this.isNewSessionDraft &&
        detail.agents.length > 0 &&
        this.stageTurns.length === 0 &&
        this.composerText.length === 0
      ) {
        this.selectAgent(detail.agents[0].agent_id);
      }

      for (let i = 0; i < detail.agents.length; i++) {
        this.drainQueuedPromptsForAgentIfIdle(hostId, detail.agents[i]);
      }
    } catch (e) {
      hilog.warn(0x0B17, 'OrbitStore', 'reloadHost failed: %{public}s', String(e));
    }
  }

  public setActiveCwd(cwd: string): void {
    if (cwd.length === 0) {
      return;
    }
    this.activeCwd = cwd;
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.fetchGitStatus(this.selectedHostId, cwd);
      this.core.fetchGitDiff(this.selectedHostId, cwd, null, this.gitStagedMode);
      this.core.fetchWorkspaceFiles(this.selectedHostId, cwd);
      this.reloadGitDiff(cwd);
      this.reloadWorkspaceFiles();
    }
  }

  public startNewSessionDraft(cwd?: string): void {
    this.isNewSessionDraft = true;
    this.selectedAgentId = '';
    this.currentAgent = null;
    this.stageTurns = [];
    this.pendingPermissions = [];
    this.pendingQuestions = [];
    this.sessionDiffs = [];
    this.expandedTurnKeys = [];
    this.expandedToolIds = [];
    this.syncActiveQueuedPrompts();
    if (cwd !== undefined && cwd.length > 0) {
      this.setActiveCwd(cwd);
    }
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.triggerHaptic(10);
  }

  public selectAgent(agentId: string): void {
    if (agentId.length === 0) {
      return;
    }
    this.isNewSessionDraft = false;
    this.selectedAgentId = agentId;
    this.expandedTurnKeys = [];
    this.expandedToolIds = [];
    this.syncActiveQueuedPrompts();
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.openAgentTimeline(this.selectedHostId, agentId);
      this.reloadStage();
    }
    this.triggerHaptic(8);
  }

  public reloadStage(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      const stage = JSON.parse(
        this.core.agentStageJson(this.selectedHostId, this.selectedAgentId)
      ) as AgentStageDto;
      this.stageRevision = stage.revision;
      this.stageTurns = stage.turns;
      this.pendingPermissions = stage.pending_permissions;
      this.pendingQuestions = stage.pending_questions;
      this.sessionDiffs = stage.session_turn_diffs;
      if (stage.agent !== null) {
        this.currentAgent = stage.agent;
        this.activeCwd = stage.agent.cwd;
        this.composerProvider = stage.agent.provider;
        this.composerModel = stage.agent.model ?? '';
        this.composerEffort = stage.agent.effort ?? 'high';
        this.composerMode = normalizeModeSlug(stage.agent.mode);
        this.drainQueuedPromptsForAgentIfIdle(this.selectedHostId, stage.agent);
      }
    } catch (e) {
      hilog.warn(0x0B17, 'OrbitStore', 'reloadStage failed: %{public}s', String(e));
    }
  }

  public describeToolStepFallback(item: TimelineItemDto): ToolStepDescriptorDto | null {
    return item.step_descriptor ?? null;
  }

  // Turn & Step Fold Toggles

  public isTurnExpanded(turnKey: string): boolean {
    return this.expandedTurnKeys.indexOf(turnKey) >= 0;
  }

  public toggleTurnExpanded(turnKey: string): void {
    const idx = this.expandedTurnKeys.indexOf(turnKey);
    if (idx >= 0) {
      this.expandedTurnKeys = this.expandedTurnKeys.filter((k: string) => k !== turnKey);
    } else {
      this.expandedTurnKeys = this.expandedTurnKeys.concat([turnKey]);
    }
    this.triggerHaptic(8);
  }

  public isToolExpanded(callId: string): boolean {
    return this.expandedToolIds.indexOf(callId) >= 0;
  }

  public toggleToolExpanded(callId: string): void {
    const idx = this.expandedToolIds.indexOf(callId);
    if (idx >= 0) {
      this.expandedToolIds = this.expandedToolIds.filter((k: string) => k !== callId);
    } else {
      this.expandedToolIds = this.expandedToolIds.concat([callId]);
    }
    this.triggerHaptic(6);
  }

  // Pairing, Host Configuration & Projects

  public pairHost(uri: string): void {
    const clean = uri.trim();
    if (clean.length === 0) {
      return;
    }
    if (this.core === null) {
      this.showToast('Native core unavailable in previewer', true);
      return;
    }
    try {
      const card = JSON.parse(this.core.pairHostJson(clean)) as HostCardDto;
      this.lastError = '';
      this.showToast(`Paired ${card.display_name}`, false);
      this.reloadFleet();
      this.selectHost(card.host_id);
      this.showHostSettingsSheet = false;
      this.triggerHaptic(18);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public discoverAndPairLocalDaemon(): void {
    if (this.core === null) {
      return;
    }
    try {
      const uri = this.core.discoverLocalDaemonUri();
      if (uri !== null && uri.length > 0) {
        this.pairHost(uri);
      } else {
        this.showToast('No local daemon found on loopback', false);
      }
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public updateHostConfig(
    hostId: string,
    customName: string,
    routeMode: string,
    sshAlias: string,
    remotePort: number,
    customDirectEndpoint: string,
    relayUrl: string
  ): void {
    if (this.core === null || hostId.length === 0) {
      return;
    }
    try {
      this.core.updateHostConfigJson(
        hostId,
        customName.trim().length > 0 ? customName.trim() : null,
        routeMode,
        sshAlias.trim().length > 0 ? sshAlias.trim() : null,
        remotePort > 0 ? remotePort : 6868,
        customDirectEndpoint.trim().length > 0 ? customDirectEndpoint.trim() : null,
        relayUrl.trim().length > 0 ? relayUrl.trim() : null,
        true
      );
      this.lastError = '';
      this.showToast('Host route configuration saved', false);
      this.reloadFleet();
      if (hostId === this.selectedHostId) {
        this.reloadHost(hostId);
      }
      this.triggerHaptic(12);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public reconnectHost(hostId: string): void {
    if (this.core === null || hostId.length === 0) {
      return;
    }
    try {
      this.core.connectHost(hostId);
      this.core.refreshHost(hostId);
      this.core.refreshQuotas(hostId);
      this.core.refreshProviderModels(hostId, false);
      this.reloadFleet();
      if (hostId === this.selectedHostId) {
        this.reloadHost(hostId);
      }
      this.showToast('Reconnecting to host…', false);
      this.triggerHaptic(10);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public refreshActiveQuotas(): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.refreshQuotas(this.selectedHostId);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing account quotas…', false);
    this.triggerHaptic(8);
  }

  public removeHost(hostId: string): void {
    if (this.core === null || hostId.length === 0) {
      return;
    }
    try {
      this.core.removeHost(hostId);
      if (this.selectedHostId === hostId) {
        this.selectedHostId = '';
        this.currentHostDetail = null;
        this.selectedAgentId = '';
        this.currentAgent = null;
        this.stageTurns = [];
      }
      this.reloadFleet();
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public pinDirectoryAsProject(dirPath: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || dirPath.trim().length === 0) {
      return;
    }
    try {
      const clean = dirPath.trim();
      const name = basenameOfPath(clean);
      this.core.upsertProjectJson(null, this.selectedHostId, name, JSON.stringify([clean]));
      this.reloadFleet();
      this.setActiveCwd(clean);
      this.showToast(`Pinned ${name}`, false);
      this.triggerHaptic(10);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public findPinnedProjectIdForCwd(cwd: string): string {
    if (cwd.trim().length === 0) {
      return '';
    }
    for (let i = 0; i < this.projects.length; i++) {
      const proj = this.projects[i];
      if (proj.host_id !== this.selectedHostId) {
        continue;
      }
      for (let j = 0; j < proj.directories.length; j++) {
        if (cwdPathsEquivalent(proj.directories[j], cwd)) {
          return proj.project_id;
        }
      }
    }
    return '';
  }

  public unpinWorkspaceByCwd(cwd: string): void {
    const projId = this.findPinnedProjectIdForCwd(cwd);
    if (projId.length === 0) {
      return;
    }
    this.removePinnedProject(projId);
    this.showToast(`Unpinned ${basenameOfPath(cwd)}`, false);
    this.triggerHaptic(8);
  }

  public removePinnedProject(projectId: string): void {
    if (this.core === null || projectId.length === 0) {
      return;
    }
    try {
      this.core.removeProject(projectId);
      this.reloadFleet();
    } catch (_) {
      // Ignore project removal error
    }
  }

  public browseDirectory(path: string | null): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.showDirBrowser = true;
    this.core.listDirectory(this.selectedHostId, path);
  }

  // Composer Context, Provider/Model/Effort & Prompt Submission

  public reloadWorkspaceFiles(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.activeCwd.length === 0) {
      return;
    }
    try {
      const files = JSON.parse(
        this.core.workspaceFilesJson(this.selectedHostId, this.activeCwd)
      ) as string[];
      this.workspaceFiles = files;
    } catch (_) {
      // Ignore parse error
    }
  }

  public ensureWorkspaceFilesLoaded(): void {
    if (
      this.workspaceFiles.length === 0 &&
      this.core !== null &&
      this.selectedHostId.length > 0 &&
      this.activeCwd.length > 0
    ) {
      this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
    }
  }

  public openContextPicker(): void {
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
    }
    this.showContextPickerSheet = true;
    this.triggerHaptic(8);
  }

  private addUniqueComposerBadge(kind: string, label: string): void {
    for (let i = 0; i < this.composerBadges.length; i++) {
      if (this.composerBadges[i].label === label) {
        return;
      }
    }
    const next: PromptBadgeDto[] = [];
    for (let i = 0; i < this.composerBadges.length; i++) {
      next.push(this.composerBadges[i]);
    }
    next.push({ kind: kind, label: label });
    this.composerBadges = next;
  }

  public attachFileBadge(relPath: string): void {
    this.addUniqueComposerBadge('file', `@${relPath}`);
    this.triggerHaptic(8);
  }

  public attachWorkingTreeDiffBadge(): void {
    this.addUniqueComposerBadge('diff', '@diff');
    this.triggerHaptic(8);
  }

  public attachStagedDiffBadge(): void {
    this.addUniqueComposerBadge('staged', '@staged');
    this.triggerHaptic(8);
  }

  public attachTerminalOutputBadge(): void {
    if (this.terminalRows.length === 0 && this.core !== null && this.selectedHostId.length > 0) {
      this.reloadTerminal();
    }
    this.addUniqueComposerBadge('terminal', '@terminal');
    this.triggerHaptic(8);
  }

  public attachInlineMention(candidate: ComposerMentionCandidate, strippedText: string): void {
    this.composerText = strippedText;
    if (candidate.kind === 'diff') {
      this.attachWorkingTreeDiffBadge();
    } else if (candidate.kind === 'staged') {
      this.attachStagedDiffBadge();
    } else if (candidate.kind === 'terminal') {
      this.attachTerminalOutputBadge();
    } else if (candidate.kind === 'file' && candidate.filePath.length > 0) {
      this.attachFileBadge(candidate.filePath);
    }
  }

  public applySlashCommand(item: SlashCommandItem): void {
    if (item.modeOverride.length > 0) {
      this.setSessionMode(item.modeOverride);
    }
    if (item.command === '/review') {
      this.addUniqueComposerBadge('diff', '@diff');
    }
    const next: PromptBadgeDto[] = [];
    for (let i = 0; i < this.composerBadges.length; i++) {
      if (this.composerBadges[i].kind !== 'command') {
        next.push(this.composerBadges[i]);
      }
    }
    const badge: PromptBadgeDto = { kind: 'command', label: item.command };
    next.push(badge);
    this.composerBadges = next;
    this.showContextPickerSheet = false;
    this.triggerHaptic(10);
  }

  public applyInlineSlashCommand(item: SlashCommandItem, strippedText: string): void {
    this.composerText = strippedText;
    this.applySlashCommand(item);
  }

  public removeComposerBadge(label: string): void {
    const next: PromptBadgeDto[] = [];
    for (let i = 0; i < this.composerBadges.length; i++) {
      if (this.composerBadges[i].label !== label) {
        next.push(this.composerBadges[i]);
      }
    }
    this.composerBadges = next;
  }

  public quoteDiffLineIntoComposer(filePath: string, line: DiffLineDto): void {
    const lineNum = line.new_line !== null ? line.new_line : line.old_line !== null ? line.old_line : 0;
    const lineLabel = lineNum > 0 ? `${basenameOfPath(filePath)}:L${lineNum}` : basenameOfPath(filePath);
    const id = `${filePath}_${lineNum}_${this.quotedSnippets.length}`;
    const snippet: QuotedDiffSnippet = {
      id: id,
      path: filePath,
      lineLabel: lineLabel,
      content: line.content,
      kind: line.kind
    };
    const next: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      next.push(this.quotedSnippets[i]);
    }
    next.push(snippet);
    this.quotedSnippets = next;
    this.showToast(`Quoted ${lineLabel} into composer`, false);
    if (!this.canShowInlineDiffInspector()) {
      this.showGitDiffSheet = false;
    }
    this.triggerHaptic(12);
  }

  public quoteAssistantReplyIntoComposer(replyText: string, turnIndex: number): void {
    const clean = replyText.trim();
    if (clean.length === 0) {
      return;
    }
    const firstLineRaw = clean.split('\n')[0].trim();
    const compactPreview = firstLineRaw.length > 26
      ? `${firstLineRaw.substring(0, 26)}…`
      : firstLineRaw;
    const lineLabel = compactPreview.length > 0
      ? `Reply #${turnIndex + 1}: ${compactPreview}`
      : `Reply #${turnIndex + 1}`;
    const cappedContent = clean.length > 800 ? `${clean.substring(0, 800)}…` : clean;
    const id = `reply_${turnIndex}_${this.quotedSnippets.length}`;
    const snippet: QuotedDiffSnippet = {
      id: id,
      path: '',
      lineLabel: lineLabel,
      content: cappedContent,
      kind: 'reply'
    };
    const next: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      next.push(this.quotedSnippets[i]);
    }
    next.push(snippet);
    this.quotedSnippets = next;
    this.showToast('Quoted reply into composer', false);
    this.triggerHaptic(12);
  }

  public removeQuotedSnippet(id: string): void {
    const next: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      if (this.quotedSnippets[i].id !== id) {
        next.push(this.quotedSnippets[i]);
      }
    }
    this.quotedSnippets = next;
  }

  public setSessionMode(mode: string): void {
    this.composerMode = mode;
    if (this.core !== null && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0) {
      this.core.setSessionMode(this.selectedHostId, this.selectedAgentId, mode);
    }
    this.triggerHaptic(8);
  }

  public cycleSessionMode(): void {
    const order: string[] = ['default', 'plan', 'accept_edits', 'full_access'];
    const curIdx = order.indexOf(this.composerMode);
    const nextMode = order[(curIdx + 1) % order.length];
    this.setSessionMode(nextMode);
  }

  public applyProviderModelEffort(provider: string, model: string, effort: string): void {
    const prevProvider = this.composerProvider;
    this.composerProvider = provider;
    this.composerModel = model;
    this.composerEffort = effort;
    if (this.core !== null && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0) {
      const modelOpt = model.trim().length > 0 ? model.trim() : null;
      const effortOpt = effort.trim().length > 0 ? effort.trim() : null;
      if (prevProvider !== provider) {
        this.core.switchAgentProvider(
          this.selectedHostId,
          this.selectedAgentId,
          provider,
          modelOpt,
          effortOpt
        );
      } else {
        this.core.setAgentConfig(this.selectedHostId, this.selectedAgentId, modelOpt, effortOpt);
      }
    }
    this.triggerHaptic(10);
  }

  public currentModelPillLabel(): string {
    let modelLabel = this.composerModel.trim();
    if (modelLabel.length === 0) {
      const specs = this.modelsForProvider(this.composerProvider);
      modelLabel = specs.length > 0 ? specs[0].label : 'Default';
    } else {
      const specs = this.modelsForProvider(this.composerProvider);
      for (let i = 0; i < specs.length; i++) {
        if (specs[i].id === modelLabel) {
          modelLabel = specs[i].label;
          break;
        }
      }
    }
    return `${modelLabel} · ${formatShortEffortLabel(this.composerEffort)}`;
  }

  public currentModePillLabel(): string {
    return formatModeLabel(this.composerMode);
  }

  public modelsForProvider(provider: string): ProviderModelSpecDto[] {
    const detail = this.currentHostDetail;
    if (detail !== null) {
      for (let i = 0; i < detail.provider_catalogs.length; i++) {
        const cat: ProviderModelCatalogDto = detail.provider_catalogs[i];
        if (cat.provider === provider && cat.models.length > 0) {
          return cat.models;
        }
      }
      let legacy: ModelOptionDto[] | undefined = undefined;
      if (provider === 'antigravity') {
        legacy = detail.models_by_provider.antigravity;
      } else if (provider === 'codex') {
        legacy = detail.models_by_provider.codex;
      } else if (provider === 'claude_code') {
        legacy = detail.models_by_provider.claude_code;
      }
      if (legacy !== undefined && legacy.length > 0) {
        const mapped: ProviderModelSpecDto[] = [];
        for (let i = 0; i < legacy.length; i++) {
          const item = legacy[i];
          const spec: ProviderModelSpecDto = {
            id: item.id,
            label: item.label,
            description: item.description ?? '',
            is_default: item.is_default,
            default_effort: 'high',
            thinking_options: []
          };
          mapped.push(spec);
        }
        return mapped;
      }
    }

    // Built-in catalog fallback for offline/previewer rendering
    const defaultThinking: ThinkingOptionSpecDto[] = [
      { effort: 'low', label: 'Low', is_default: false },
      { effort: 'medium', label: 'Medium', is_default: false },
      { effort: 'high', label: 'High', is_default: true },
      { effort: 'xhigh', label: 'Extra High', is_default: false }
    ];
    if (provider === 'codex') {
      const codexModels: ProviderModelSpecDto[] = [
        {
          id: 'gpt-5-codex',
          label: 'GPT-5 Codex',
          description: 'Flagship autonomous software engineering model',
          is_default: true,
          default_effort: 'high',
          thinking_options: defaultThinking
        },
        {
          id: 'o3',
          label: 'o3',
          description: 'Deep reasoning for architecture and tough bugs',
          is_default: false,
          default_effort: 'high',
          thinking_options: defaultThinking
        }
      ];
      return codexModels;
    }
    if (provider === 'claude_code') {
      const claudeModels: ProviderModelSpecDto[] = [
        {
          id: 'claude-sonnet-4-5',
          label: 'Claude Sonnet 4.5',
          description: 'Balanced speed and coding accuracy',
          is_default: true,
          default_effort: 'high',
          thinking_options: defaultThinking
        },
        {
          id: 'claude-opus-4-1',
          label: 'Claude Opus 4.1',
          description: 'Maximum depth for multi-file refactors',
          is_default: false,
          default_effort: 'max',
          thinking_options: defaultThinking
        }
      ];
      return claudeModels;
    }
    const agyModels: ProviderModelSpecDto[] = [
      {
        id: 'gemini-2.5-pro',
        label: 'Gemini 2.5 Pro',
        description: 'Long-context agentic synthesis and code navigation',
        is_default: true,
        default_effort: 'high',
        thinking_options: defaultThinking
      }
    ];
    return agyModels;
  }

  public isActiveAgentRunning(): boolean {
    if (this.currentAgent === null || this.selectedAgentId.length === 0) {
      return false;
    }
    return (
      this.currentAgent.is_running ||
      this.currentAgent.status.state === 'running' ||
      this.currentAgent.status.state === 'initializing'
    );
  }

  public hasComposerDraftContent(): boolean {
    return (
      this.composerText.trim().length > 0 ||
      this.composerBadges.length > 0 ||
      this.quotedSnippets.length > 0
    );
  }

  private extractRecentTerminalText(): string {
    if (this.terminalRows.length === 0) {
      return '';
    }
    const lines: string[] = [];
    for (let i = 0; i < this.terminalRows.length; i++) {
      const row = this.terminalRows[i];
      let text = '';
      for (let j = 0; j < row.spans.length; j++) {
        text += row.spans[j].text;
      }
      const trimmedRight = text.replace(/\s+$/, '');
      if (trimmedRight.length > 0) {
        lines.push(trimmedRight);
      }
    }
    if (lines.length === 0) {
      return '';
    }
    const tail = lines.length > 30 ? lines.slice(lines.length - 30) : lines;
    return tail.join('\n');
  }

  private buildPromptPayload(
    rawInput: string,
    sourceBadges: PromptBadgeDto[],
    sourceQuotes: QuotedDiffSnippet[]
  ): BuiltPromptPayload {
    const badges: PromptBadgeDto[] = [];
    for (let i = 0; i < sourceBadges.length; i++) {
      badges.push(sourceBadges[i]);
    }
    for (let i = 0; i < sourceQuotes.length; i++) {
      badges.push({
        kind: 'quote',
        label: sourceQuotes[i].lineLabel
      });
    }

    const contextParts: string[] = [];
    let commandDirective = '';
    for (let i = 0; i < sourceBadges.length; i++) {
      const b = sourceBadges[i];
      if (b.kind === 'file') {
        const cleanPath = b.label.startsWith('@') ? b.label.substring(1) : b.label;
        contextParts.push(`Context file: ${cleanPath}`);
      } else if (b.kind === 'diff') {
        contextParts.push('Please inspect the current unstaged git working tree diff in this workspace.');
      } else if (b.kind === 'staged') {
        contextParts.push('Please inspect the staged git index diff (git diff --cached) in this workspace.');
      } else if (b.kind === 'terminal') {
        const termSnap = this.extractRecentTerminalText();
        if (termSnap.length > 0) {
          contextParts.push(`Recent PTY terminal output:\n\`\`\`text\n${termSnap}\n\`\`\``);
        } else {
          contextParts.push('Please inspect the active PTY terminal session context in this workspace.');
        }
      } else if (b.kind === 'command') {
        commandDirective = b.label;
        if (b.label === '/plan') {
          contextParts.push(
            'First draft a clear, step-by-step engineering plan before making file edits.'
          );
        } else if (b.label === '/review') {
          contextParts.push(
            'Perform a thorough code review of the current working tree changes, calling out bugs, edge cases, and concrete improvements.'
          );
        } else if (!rawInput.startsWith(b.label) && b.label !== '/compact') {
          contextParts.push(`Command intent: ${b.label}`);
        }
      }
    }

    let hasQuotedReply = false;
    for (let i = 0; i < sourceQuotes.length; i++) {
      const snip = sourceQuotes[i];
      if (snip.kind === 'reply') {
        hasQuotedReply = true;
        const quotedLines = snip.content.split('\n').join('\n> ');
        contextParts.push(`Quoted assistant reply (${snip.lineLabel}):\n> ${quotedLines}`);
      } else {
        const prefix = snip.kind === 'added' ? '+' : snip.kind === 'removed' ? '-' : ' ';
        contextParts.push(
          `Referenced diff (${snip.lineLabel}):\n\`\`\`diff\n${prefix}${snip.content}\n\`\`\``
        );
      }
    }

    let wirePrompt = rawInput;
    if (commandDirective === '/compact' && rawInput.length === 0) {
      wirePrompt = '/compact';
    } else if (contextParts.length > 0) {
      wirePrompt = rawInput.length > 0
        ? `${contextParts.join('\n\n')}\n\n${rawInput}`
        : contextParts.join('\n\n');
    }

    let displayPrompt = rawInput;
    if (displayPrompt.length === 0) {
      if (commandDirective.length > 0) {
        displayPrompt = commandDirective === '/review'
          ? 'Review working tree changes'
          : commandDirective === '/plan'
            ? 'Draft implementation plan'
            : commandDirective;
      } else if (hasQuotedReply) {
        displayPrompt = 'Follow up on quoted reply';
      } else if (sourceQuotes.length > 0) {
        displayPrompt = 'Review referenced diff lines';
      } else if (sourceBadges.length > 0) {
        displayPrompt = `Inspect ${sourceBadges[0].label}`;
      }
    }

    return {
      wirePrompt: wirePrompt.trim(),
      displayPrompt: displayPrompt.trim(),
      badges: badges,
      badgesJson: badges.length > 0 ? JSON.stringify(badges) : null
    };
  }

  public enqueueCurrentDraft(): void {
    const rawInput = this.composerText.trim();
    if (
      rawInput.length === 0 &&
      this.composerBadges.length === 0 &&
      this.quotedSnippets.length === 0
    ) {
      return;
    }
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }

    const copiedBadges: PromptBadgeDto[] = [];
    for (let i = 0; i < this.composerBadges.length; i++) {
      copiedBadges.push(this.composerBadges[i]);
    }
    const copiedQuotes: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      copiedQuotes.push(this.quotedSnippets[i]);
    }

    const payload = this.buildPromptPayload(rawInput, copiedBadges, copiedQuotes);
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const existing = this.queuedPromptsByAgent.get(key);
    const nextList: QueuedPromptItem[] = [];
    if (existing !== undefined) {
      for (let i = 0; i < existing.length; i++) {
        nextList.push(existing[i]);
      }
    }
    const nowMs = Date.now();
    const item: QueuedPromptItem = {
      id: `q_${nowMs}_${nextList.length}`,
      hostKey: key,
      rawText: rawInput,
      displayPrompt: payload.displayPrompt,
      wirePrompt: payload.wirePrompt,
      badgesJson: payload.badgesJson,
      badges: copiedBadges,
      quotedSnippets: copiedQuotes,
      createdAtMs: nowMs
    };
    nextList.push(item);
    this.queuedPromptsByAgent.set(key, nextList);
    this.syncActiveQueuedPrompts();

    this.composerText = '';
    this.composerBadges = [];
    this.quotedSnippets = [];
    this.showToast('Queued next prompt', false);
    this.triggerHaptic(12);
  }

  public removeQueuedPrompt(itemId: string): void {
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const existing = this.queuedPromptsByAgent.get(key);
    if (existing === undefined) {
      return;
    }
    const nextList: QueuedPromptItem[] = [];
    for (let i = 0; i < existing.length; i++) {
      if (existing[i].id !== itemId) {
        nextList.push(existing[i]);
      }
    }
    if (nextList.length > 0) {
      this.queuedPromptsByAgent.set(key, nextList);
    } else {
      this.queuedPromptsByAgent.delete(key);
    }
    this.syncActiveQueuedPrompts();
    this.triggerHaptic(8);
  }

  public editQueuedPrompt(itemId: string): void {
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const existing = this.queuedPromptsByAgent.get(key);
    if (existing === undefined) {
      return;
    }
    let target: QueuedPromptItem | null = null;
    const nextList: QueuedPromptItem[] = [];
    for (let i = 0; i < existing.length; i++) {
      if (existing[i].id === itemId && target === null) {
        target = existing[i];
      } else {
        nextList.push(existing[i]);
      }
    }
    if (target === null) {
      return;
    }
    if (nextList.length > 0) {
      this.queuedPromptsByAgent.set(key, nextList);
    } else {
      this.queuedPromptsByAgent.delete(key);
    }
    this.syncActiveQueuedPrompts();

    if (this.composerText.trim().length > 0 && target.rawText.length > 0) {
      this.composerText = `${this.composerText}\n${target.rawText}`;
    } else if (target.rawText.length > 0) {
      this.composerText = target.rawText;
    }

    const mergedBadges: PromptBadgeDto[] = [];
    for (let i = 0; i < this.composerBadges.length; i++) {
      mergedBadges.push(this.composerBadges[i]);
    }
    for (let i = 0; i < target.badges.length; i++) {
      let exists = false;
      for (let j = 0; j < mergedBadges.length; j++) {
        if (mergedBadges[j].label === target.badges[i].label) {
          exists = true;
          break;
        }
      }
      if (!exists) {
        mergedBadges.push(target.badges[i]);
      }
    }
    this.composerBadges = mergedBadges;

    const mergedQuotes: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      mergedQuotes.push(this.quotedSnippets[i]);
    }
    for (let i = 0; i < target.quotedSnippets.length; i++) {
      mergedQuotes.push(target.quotedSnippets[i]);
    }
    this.quotedSnippets = mergedQuotes;
    this.triggerHaptic(8);
  }

  public sendQueuedPromptNow(itemId: string): void {
    if (
      this.core === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0
    ) {
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const existing = this.queuedPromptsByAgent.get(key);
    if (existing === undefined) {
      return;
    }
    let target: QueuedPromptItem | null = null;
    const nextList: QueuedPromptItem[] = [];
    for (let i = 0; i < existing.length; i++) {
      if (existing[i].id === itemId && target === null) {
        target = existing[i];
      } else {
        nextList.push(existing[i]);
      }
    }
    if (target === null) {
      return;
    }
    if (nextList.length > 0) {
      this.queuedPromptsByAgent.set(key, nextList);
    } else {
      this.queuedPromptsByAgent.delete(key);
    }
    this.syncActiveQueuedPrompts();

    try {
      if (this.isActiveAgentRunning()) {
        this.core.interruptAgent(this.selectedHostId, this.selectedAgentId);
      }
      this.core.sendPromptWithMeta(
        this.selectedHostId,
        this.selectedAgentId,
        target.wirePrompt,
        target.displayPrompt,
        target.badgesJson
      );
      this.reloadStage();
      this.triggerHaptic(14);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public submitComposerPrompt(overridePrompt?: string): void {
    const rawInput = (overridePrompt !== undefined ? overridePrompt : this.composerText).trim();
    if (
      rawInput.length === 0 &&
      this.composerBadges.length === 0 &&
      this.quotedSnippets.length === 0
    ) {
      return;
    }
    if (this.core === null || this.selectedHostId.length === 0) {
      this.showToast(
        this.selectedHostId.length === 0
          ? 'Pair or select a host first'
          : 'Native core unavailable in previewer',
        true
      );
      return;
    }

    if (overridePrompt === undefined && this.isActiveAgentRunning()) {
      this.enqueueCurrentDraft();
      return;
    }

    const payload = this.buildPromptPayload(rawInput, this.composerBadges, this.quotedSnippets);

    try {
      if (this.selectedAgentId.length > 0) {
        this.core.sendPromptWithMeta(
          this.selectedHostId,
          this.selectedAgentId,
          payload.wirePrompt,
          payload.displayPrompt,
          payload.badgesJson
        );
        this.reloadStage();
      } else {
        const modelOpt = this.composerModel.trim().length > 0 ? this.composerModel.trim() : null;
        const effortOpt = this.composerEffort.trim().length > 0 ? this.composerEffort.trim() : null;
        const wtOpt = this.worktreeBranch.trim().length > 0 ? this.worktreeBranch.trim() : null;
        const optId = this.core.createAgentFull(
          this.selectedHostId,
          this.activeCwd,
          null,
          this.composerProvider,
          modelOpt,
          effortOpt,
          this.composerMode,
          payload.wirePrompt,
          payload.displayPrompt,
          payload.badgesJson,
          wtOpt
        );
        this.selectedAgentId = optId;
        this.syncActiveQueuedPrompts();
        this.reloadHost(this.selectedHostId);
        this.reloadStage();
      }
      this.composerText = '';
      this.composerBadges = [];
      this.quotedSnippets = [];
      this.worktreeBranch = '';
      this.triggerHaptic(14);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  // Native Pull-to-Refresh Actions

  public pullRefreshStage(): void {
    this.triggerHaptic(10);
    if (this.core === null) {
      return;
    }
    this.reloadFleet();
    if (this.selectedHostId.length > 0) {
      this.core.connectHost(this.selectedHostId);
      this.core.refreshHost(this.selectedHostId);
      this.reloadHost(this.selectedHostId);
      if (this.selectedAgentId.length > 0) {
        this.core.openAgentTimeline(this.selectedHostId, this.selectedAgentId);
        this.reloadStage();
      }
      this.refreshGitReview(
        this.gitStagedMode,
        this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
      );
      this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
    }
  }

  public pullRefreshGitReview(): void {
    this.triggerHaptic(10);
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.refreshHost(this.selectedHostId);
    this.refreshGitReview(
      this.gitStagedMode,
      this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
    );
    this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
    this.reloadWorkspaceFiles();
  }

  public interruptCurrentAgent(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.interruptAgent(this.selectedHostId, this.selectedAgentId);
    this.triggerHaptic(16);
  }

  public archiveAgent(agentId: string): void {
    this.archiveSession(agentId);
  }

  public archiveSession(agentId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    this.core.archiveAgent(this.selectedHostId, agentId);
    if (this.selectedAgentId === agentId) {
      this.startNewSessionDraft();
    }
    this.reloadHost(this.selectedHostId);
    this.triggerHaptic(12);
  }

  // Sticky Decision Dock Actions

  public resolvePermission(requestId: string, decision: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.resolvePermission(this.selectedHostId, this.selectedAgentId, requestId, decision);
    this.triggerHaptic(14);
    this.reloadStage();
  }

  public answerQuestion(requestId: string, answers: string[][]): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.core.answerQuestion(
      this.selectedHostId,
      this.selectedAgentId,
      requestId,
      JSON.stringify(answers)
    );
    this.triggerHaptic(14);
    this.reloadStage();
  }

  // Git Working Tree Diff Actions

  public toggleTopBarGitDiff(): void {
    if (this.canShowInlineDiffInspector()) {
      this.showInlineDiffInspector = !this.showInlineDiffInspector;
      if (this.showInlineDiffInspector) {
        this.showGitDiffSheet = false;
        this.refreshGitReview(
          this.gitStagedMode,
          this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
        );
      }
      this.triggerHaptic(8);
      return;
    }
    this.openGitInspector();
  }

  public openGitInspector(focusPath?: string): void {
    if (focusPath !== undefined) {
      this.gitSelectedPath = focusPath;
    }
    this.refreshGitReview(this.gitStagedMode, this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null);
    if (this.canShowInlineDiffInspector()) {
      this.showInlineDiffInspector = true;
      this.showGitDiffSheet = false;
    } else {
      this.showGitDiffSheet = true;
    }
    this.triggerHaptic(8);
  }

  public setGitStagedMode(staged: boolean): void {
    this.gitStagedMode = staged;
    this.refreshGitReview(staged, this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null);
    this.triggerHaptic(8);
  }

  public selectGitFileFilter(path: string): void {
    this.gitSelectedPath = path;
    this.refreshGitReview(this.gitStagedMode, path.length > 0 ? path : null);
    this.triggerHaptic(6);
  }

  public refreshGitReview(staged: boolean, path: string | null): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.fetchGitStatus(this.selectedHostId, this.activeCwd);
    this.core.fetchGitDiff(this.selectedHostId, this.activeCwd, path, staged);
    this.reloadGitDiff(this.activeCwd);
  }

  public reloadGitDiff(cwd: string): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      const pathOpt = this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null;
      const gr = JSON.parse(this.core.gitReviewJson(this.selectedHostId, cwd, pathOpt)) as GitReviewDto;
      this.gitStatus = gr.status;
      this.gitDiffFiles = gr.diff_files;
    } catch (_) {
      // Ignore parse error
    }
  }

  public totalDiffAdditions(): number {
    const source = this.gitDiffFiles.length > 0 ? this.gitDiffFiles : this.sessionDiffs;
    let sum = 0;
    for (let i = 0; i < source.length; i++) {
      sum += source[i].additions;
    }
    return sum;
  }

  public totalDiffDeletions(): number {
    const source = this.gitDiffFiles.length > 0 ? this.gitDiffFiles : this.sessionDiffs;
    let sum = 0;
    for (let i = 0; i < source.length; i++) {
      sum += source[i].deletions;
    }
    return sum;
  }

  // Detached PTY Multi-Terminal Operations

  public inspectGitDiffForCwd(cwd: string): void {
    if (cwd.trim().length > 0) {
      this.setActiveCwd(cwd.trim());
    }
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.openGitInspector('');
  }

  public openPtyTerminalInCwd(cwd: string): void {
    const clean = cwd.trim();
    if (clean.length > 0) {
      this.setActiveCwd(clean);
    }
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.showPtySheet = true;
    if (this.core !== null && this.selectedHostId.length > 0) {
      const detail = this.currentHostDetail;
      let matchedTermId = '';
      if (detail !== null && clean.length > 0) {
        for (let i = 0; i < detail.terminals.length; i++) {
          if (cwdPathsEquivalent(detail.terminals[i].cwd, clean)) {
            matchedTermId = detail.terminals[i].terminal_id;
            break;
          }
        }
      }
      if (matchedTermId.length > 0) {
        this.attachTerminal(matchedTermId, 1);
      } else {
        this.startNewTerminal();
      }
    }
    this.triggerHaptic(10);
  }

  public openPtyTerminal(): void {
    this.showPtySheet = true;
    if (this.core !== null && this.selectedHostId.length > 0) {
      const detail = this.currentHostDetail;
      if (detail === null || detail.terminals.length === 0) {
        this.startNewTerminal();
      } else if (detail.active_terminal_id === null && detail.terminals.length > 0) {
        this.attachTerminal(detail.terminals[0].terminal_id, 1);
      } else {
        this.reloadTerminal();
      }
    }
    this.triggerHaptic(10);
  }

  public startNewTerminal(): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.startTerminal(
      this.selectedHostId,
      this.activeCwd,
      this.terminalCols,
      this.terminalGridRows
    );
    this.triggerHaptic(10);
  }

  public attachTerminal(terminalId: string, _slot: number = 1): void {
    if (this.core === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    this.core.attachTerminal(this.selectedHostId, terminalId, 1);
    this.reloadHost(this.selectedHostId);
    this.reloadTerminal();
    this.triggerHaptic(8);
  }

  public killTerminal(terminalId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    this.core.killTerminal(this.selectedHostId, terminalId);
    this.reloadHost(this.selectedHostId);
    this.triggerHaptic(10);
  }

  public resizeTerminalIfChanged(cols: number, rows: number): void {
    const clampedCols = Math.max(32, Math.min(240, cols));
    const clampedRows = Math.max(8, Math.min(100, rows));
    if (clampedCols === this.terminalCols && clampedRows === this.terminalGridRows) {
      return;
    }
    this.terminalCols = clampedCols;
    this.terminalGridRows = clampedRows;
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.resizeTerminal(this.selectedHostId, clampedCols, clampedRows);
      this.reloadTerminal();
    }
  }

  public reloadTerminal(): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      const screen = JSON.parse(
        this.core.terminalScreenJson(this.selectedHostId)
      ) as TerminalScreenDto | null;
      this.terminalScreen = screen;
      if (screen !== null) {
        this.terminalRows = screen.rows_data;
      }
    } catch (_) {
      // Ignore parse error
    }
  }

  public extractVisibleTerminalSnapshot(): string {
    if (this.terminalRows.length === 0) {
      this.reloadTerminal();
    }
    if (this.terminalRows.length === 0) {
      return '';
    }
    const lines: string[] = [];
    for (let i = 0; i < this.terminalRows.length; i++) {
      const row = this.terminalRows[i];
      let text = '';
      for (let j = 0; j < row.spans.length; j++) {
        text += row.spans[j].text;
      }
      lines.push(text.replace(/\s+$/, ''));
    }
    while (lines.length > 0 && lines[lines.length - 1].length === 0) {
      lines.pop();
    }
    return lines.join('\n').trim();
  }

  public copyTerminalSnapshot(): void {
    const snapshot = this.extractVisibleTerminalSnapshot();
    if (snapshot.length === 0) {
      this.showToast('Terminal output is empty', false);
      this.triggerHaptic(8);
      return;
    }
    this.copyToClipboard(snapshot, 'Copied terminal output');
  }

  public quoteTerminalIntoComposer(): void {
    this.attachTerminalOutputBadge();
    this.showPtySheet = false;
    this.showToast('Attached @terminal to composer', false);
  }

  public sendTerminalKeys(rawKeys: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || rawKeys.length === 0) {
      return;
    }
    let out = rawKeys;
    if (this.ctrlSticky && rawKeys.length === 1) {
      const code = rawKeys.toUpperCase().charCodeAt(0);
      if (code >= 64 && code <= 95) {
        out = String.fromCharCode(code - 64);
      }
      this.ctrlSticky = false;
    } else if (this.altSticky) {
      out = `\x1b${rawKeys}`;
      this.altSticky = false;
    }
    this.core.sendTerminalKeys(this.selectedHostId, out);
  }
}
