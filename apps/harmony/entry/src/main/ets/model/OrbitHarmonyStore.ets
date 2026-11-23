import { OrbitHarmonyCore } from 'liborbit_ohos.so';
import { ConfigurationConstant, common } from '@kit.AbilityKit';
import {
  AgentCardDto,
  AgentStageDeltaDto,
  AskSideQuestionRequestDto,
  CancelWorkflowRunRequestDto,
  ClientProjectDto,
  CloudSyncResultDto,
  CloudSyncStatusDto,
  ComposePromptRequestDto,
  ComposedPromptDto,
  ComposerAttachmentItem,
  ComposerContextInputDto,
  ComposerQuoteInputDto,
  ConditionalTimerRecordDto,
  ControlGoalRunRequestDto,
  CoreNotificationDto,
  CreateSchedulePayloadDto,
  CreateSessionRequestDto,
  DiffFileDto,
  DiffLineDto,
  ExportOptionsDto,
  ExportedTranscriptDto,
  FleetOverviewDto,
  FleetSidebarDto,
  FleetSidebarRequestDto,
  ForkAgentRequestDto,
  ForkAgentResultDto,
  GitReviewDto,
  GitStatusSnapshotDto,
  GoalRunRecordDto,
  HostCardDto,
  HostDetailDto,
  InlineMentionRegistration,
  LaunchpadProjectDto,
  MentionCandidateDto,
  MentionRankRequestDto,
  ModelOptionDto,
  ModelSelectionRequestDto,
  MonitorWatchRecordDto,
  PromptBadgeDto,
  ProviderModelCatalogDto,
  ProviderModelSpecDto,
  QueuedPromptDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  RenderDiagramRequestDto,
  RenderDiagramViewDto,
  ResolvedModelSelectionDto,
  RewindAgentResultDto,
  ScheduleSpecDto,
  SlashCommandDto,
  SpawnWorkspacePeerRequestDto,
  StageTurnDto,
  StagedAttachmentDto,
  StopMonitorWatchRequestDto,
  TerminalRowDto,
  TerminalScreenDto,
  ThreadPrefSeedDto,
  TimelineItemDto,
  ToolStepDescriptorDto,
  WorkflowRunRecordDto,
  WorkspaceBoardSnapshotDto,
  WorkspaceDto,
  WorkspaceEventRecordDto,
  WorkspaceFileViewDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatEffortLabel,
  formatModeLabel,
  normalizeModeSlug
} from './OrbitTypes';
import {
  LIGHT_PALETTE,
  OrbitThemePalette,
  OrbitTokens,
  resolveOrbitPalette
} from '../theme/OrbitTokens';
import { orbitLogWarn } from './OrbitLogger';
import { OrbitPreferencesStore } from './OrbitPreferences';
import {
  checkHarmonyNotificationEnabled,
  publishOrbitDeepLinkNotification,
  readAuthorizedClipboardText,
  readUriBytesAsync,
  registerHarmonyPushToken,
  requestHarmonyNotificationPermission,
  scanPairingQrCode,
  triggerNativeHaptic,
  writeClipboardText
} from './OrbitDeviceBridge';

interface DraftModelSelection {
  model: string;
  effort: string;
}

interface SideQuestionDeltaBody {
  request_id?: string;
  text_delta?: string;
}

interface SideQuestionDoneBody {
  request_id?: string;
  full_text?: string;
  error?: string | null;
}

interface SideQuestionEnvelopeDto {
  kind?: string;
  request_id?: string;
  Delta?: SideQuestionDeltaBody;
  Done?: SideQuestionDoneBody;
  text_delta?: string;
  full_text?: string;
  error?: string | null;
}

@ObservedV2
export class OrbitHarmonyStore {
  private static instance: OrbitHarmonyStore | null = null;
  private core: OrbitHarmonyCore | null = null;
  private appContext: common.ApplicationContext | null = null;
  private prefsStore: OrbitPreferencesStore = new OrbitPreferencesStore();
  private pendingDeepLink: string = '';
  private pendingOpenHostId: string = '';
  private pendingOpenAgentId: string = '';
  private automationsRefreshSeq: number = 0;
  private sideQuestionGen: number = 0;
  @Trace public isNewSessionDraft: boolean = false;
  private draftSelectionByProvider: Map<string, DraftModelSelection> = new Map<string, DraftModelSelection>();
  private hasUserCustomizedDraftModel: boolean = false;

  // Theme & Window Safe Area State
  @Trace public themeMode: string = 'system';
  @Trace public systemIsDark: boolean = false;
  @Trace public palette: OrbitThemePalette = LIGHT_PALETTE;
  @Trace public safeTopVp: number = 0;
  @Trace public safeBottomVp: number = 0;
  @Trace public safeLeftVp: number = 0;
  @Trace public safeRightVp: number = 0;
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
  @Trace public showAutomationsSheet: boolean = false;
  @Trace public showWorkspaceBoardSheet: boolean = false;
  @Trace public showSideQuestionSheet: boolean = false;
  @Trace public showArchivedSessionsSheet: boolean = false;
  @Trace public sidebarProjectSort: string = 'recent';
  @Trace public sidebarThreadFilter: string = 'all';
  @Trace public sidebarSearchQuery: string = '';
  @Trace public collapsedProjectIds: string[] = [];
  @Trace public sidebarView: FleetSidebarDto | null = null;
  @Trace public slashCommands: SlashCommandDto[] = [];
  private cachedFallbackCatalogs: ProviderModelCatalogDto[] = [];
  private diagramRenderCache: Map<string, RenderDiagramViewDto> = new Map<string, RenderDiagramViewDto>();
  private diagramRenderOrder: string[] = [];
  private pinnedThreadsFilePath: string = '';
  private draftPrefsFilePath: string = '';
  private stageHostId: string = '';
  private stageAgentId: string = '';
  @Trace public draftUseWorktree: boolean = false;
  @Trace public sideQuestionActive: boolean = false;
  @Trace public sideQuestionStreaming: boolean = false;
  @Trace public sideQuestionPrompt: string = '';
  @Trace public sideQuestionAnswer: string = '';
  @Trace public sideQuestionError: string = '';

  // Wave 7 Parity State: Automations, Shared Board, Cloud Sync, Push & Fast Mode
  @Trace public composerFastMode: boolean = false;
  @Trace public schedules: ScheduleSpecDto[] = [];
  @Trace public conditionalTimers: ConditionalTimerRecordDto[] = [];
  @Trace public goalRuns: GoalRunRecordDto[] = [];
  @Trace public monitorWatches: MonitorWatchRecordDto[] = [];
  @Trace public workflowRuns: WorkflowRunRecordDto[] = [];
  @Trace public automationsLoading: boolean = false;
  @Trace public workspaceBoardContent: string = '';
  @Trace public workspaceBoardRevision: number = 0;
  @Trace public workspaceBoardUpdatedBy: string = '';
  @Trace public workspaceBoardPath: string = '';
  @Trace public workspaceEvents: WorkspaceEventRecordDto[] = [];
  @Trace public cloudSyncStatus: CloudSyncStatusDto | null = null;
  @Trace public cloudSyncBusy: boolean = false;
  @Trace public pushToken: string = '';
  @Trace public pushStatusMessage: string = '';
  @Trace public notificationsEnabled: boolean = false;

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
  @Trace public composerEffort: string = '';
  @Trace public composerMode: string = 'accept_edits';
  @Trace public activeSlashDirective: string = '';
  @Trace public composerBadges: PromptBadgeDto[] = [];
  @Trace public stagedAttachments: ComposerAttachmentItem[] = [];
  @Trace public inlineMentions: InlineMentionRegistration[] = [];
  @Trace public isStagingAttachment: boolean = false;
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
  @Trace public toastRevision: number = 0;
  private toastTimerId: number = -1;

  public static getInstance(): OrbitHarmonyStore {
    if (OrbitHarmonyStore.instance === null) {
      OrbitHarmonyStore.instance = new OrbitHarmonyStore();
    }
    return OrbitHarmonyStore.instance;
  }

  public initialize(dbPath: string): void {
    if (this.pinnedThreadsFilePath.length === 0 && dbPath.length > 0) {
      this.pinnedThreadsFilePath = `${dbPath}.pinned_threads.json`;
    }
    if (this.draftPrefsFilePath.length === 0 && dbPath.length > 0) {
      this.draftPrefsFilePath = `${dbPath}.draft_prefs.json`;
      this.loadDraftPrefsFromDisk();
    }
    if (this.core !== null) {
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        this.pendingDeepLink = '';
        this.handleDeepLinkUri(uri);
      }
      return;
    }
    try {
      this.core = new OrbitHarmonyCore(dbPath, true);
      this.nativeAvailable = true;

      if (!this.prefsStore.hasMigratedThreadPrefsV1()) {
        const legacyPinned = this.prefsStore.loadPinnedThreads(this.pinnedThreadsFilePath);
        if (legacyPinned.length > 0) {
          const nowMs = Date.now();
          const seeds: ThreadPrefSeedDto[] = [];
          for (let i = 0; i < legacyPinned.length; i++) {
            const composite = legacyPinned[i];
            const sep = composite.indexOf('::');
            if (sep > 0 && sep + 2 < composite.length) {
              seeds.push({
                host_id: composite.substring(0, sep),
                agent_id: composite.substring(sep + 2),
                seen_seq: 0,
                is_pinned: true,
                pinned_order: i,
                updated_at_ms: nowMs
              });
            }
          }
          if (seeds.length > 0) {
            this.core.seedThreadPrefsJson(JSON.stringify(seeds));
          }
        }
        this.prefsStore.markMigratedThreadPrefsV1();
      }

      try {
        this.cachedFallbackCatalogs = JSON.parse(
          this.core.fallbackProviderCatalogsJson()
        ) as ProviderModelCatalogDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'Failed to parse fallback catalogs', err as Object);
      }
      try {
        this.slashCommands = JSON.parse(
          this.core.slashCommandsJson('harmony', null)
        ) as SlashCommandDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'Failed to parse slash commands', err as Object);
      }

      this.core.subscribeEvents((err: Error | null, payloadJson: string) => {
        if (err !== null || payloadJson.length === 0) {
          return;
        }
        this.handleCoreEvent(payloadJson);
      });
      this.reloadFleet();
      this.refreshSidebar();
      this.refreshCloudSyncStatus();
      this.core.connectAllHosts();
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        this.pendingDeepLink = '';
        this.handleDeepLinkUri(uri);
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

  public bindContext(ctx: common.Context): void {
    this.prefsStore.init(ctx);
  }

  public bindApplicationContext(ctx: common.ApplicationContext): void {
    this.appContext = ctx;
  }

  public applyPersistedThemeMode(): void {
    const saved = this.prefsStore.getThemeMode();
    this.themeMode = saved;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    this.syncAppColorMode(saved);
  }

  private syncAppColorMode(normalized: string): void {
    if (this.appContext === null) {
      return;
    }
    try {
      const targetMode = normalized === 'light'
        ? ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT
        : (normalized === 'dark'
          ? ConfigurationConstant.ColorMode.COLOR_MODE_DARK
          : ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET);
      this.appContext.setColorMode(targetMode);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'Failed to apply color mode', err as Object);
    }
  }

  public setThemeMode(mode: string): void {
    const normalized = mode === 'light'
      ? 'light'
      : (mode === 'dark' || mode === 'graphite' || mode === 'oled' ? 'dark' : 'system');
    this.themeMode = normalized;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    this.prefsStore.setThemeMode(normalized);
    this.syncAppColorMode(normalized);
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
    this.toastRevision += 1;
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

  public onMemoryPressure(): void {
    if (!this.showGitDiffSheet && !this.showInlineDiffInspector) {
      this.gitDiffFiles = [];
    }
    if (!this.showPtySheet) {
      this.terminalRows = [];
    }
  }

  public dispose(): void {
    if (this.toastTimerId !== -1) {
      clearTimeout(this.toastTimerId);
      this.toastTimerId = -1;
    }
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

  public threadKey(hostId: string, agentId: string): string {
    return `${hostId}::${agentId}`;
  }

  public otherHostsPendingApprovals(): number {
    let total = 0;
    for (let i = 0; i < this.hosts.length; i++) {
      if (this.hosts[i].host_id !== this.selectedHostId) {
        total += this.hosts[i].pending_approval_count;
      }
    }
    return total;
  }

  public refreshSidebar(): void {
    if (this.core === null) {
      return;
    }
    try {
      const req: FleetSidebarRequestDto = {
        sort: (this.sidebarProjectSort === 'name' || this.sidebarProjectSort === 'alphabetical')
          ? 'alphabetical'
          : 'recent',
        filter: this.sidebarThreadFilter === 'active_unread' ? 'active_unread' : 'all',
        query: this.sidebarSearchQuery,
        active_host_id: this.selectedHostId.length > 0 ? this.selectedHostId : null,
        active_agent_id: (!this.isNewSessionDraft && this.selectedAgentId.length > 0)
          ? this.selectedAgentId
          : null,
        active_cwd: this.activeCwd.length > 0 ? this.activeCwd : null
      };
      this.sidebarView = JSON.parse(
        this.core.fleetSidebarByRequestJson(JSON.stringify(req))
      ) as FleetSidebarDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshSidebar failed', err as Object);
    }
  }

  public setSidebarSearchQuery(query: string): void {
    this.sidebarSearchQuery = query;
    this.refreshSidebar();
  }

  public setSidebarProjectSort(sort: string): void {
    this.sidebarProjectSort = sort;
    this.saveDraftPrefsToDisk();
    this.refreshSidebar();
    this.triggerHaptic(6);
  }

  public setSidebarThreadFilter(filter: string): void {
    this.sidebarThreadFilter = filter;
    this.saveDraftPrefsToDisk();
    this.refreshSidebar();
    this.triggerHaptic(6);
  }

  private loadDraftPrefsFromDisk(): void {
    const loaded = this.prefsStore.loadDraftPrefs(this.draftPrefsFilePath);
    if (loaded === null) {
      return;
    }
    if (loaded.sidebarProjectSort === 'recent' || loaded.sidebarProjectSort === 'alphabetical') {
      this.sidebarProjectSort = loaded.sidebarProjectSort;
    }
    if (loaded.sidebarThreadFilter === 'all' || loaded.sidebarThreadFilter === 'active_unread') {
      this.sidebarThreadFilter = loaded.sidebarThreadFilter;
    }
    if (loaded.permissionMode === 'worktree') {
      this.draftUseWorktree = true;
    }
  }

  private saveDraftPrefsToDisk(): void {
    this.prefsStore.saveDraftPrefs({
      provider: this.composerProvider,
      model: this.composerModel,
      effort: this.composerEffort,
      permissionMode: this.draftUseWorktree ? 'worktree' : 'local',
      sidebarProjectSort: this.sidebarProjectSort,
      sidebarThreadFilter: this.sidebarThreadFilter
    });
  }

  public setDraftUseWorktree(useWorktree: boolean): void {
    this.draftUseWorktree = useWorktree;
    this.saveDraftPrefsToDisk();
    this.triggerHaptic(6);
  }

  public activeGitBranch(): string {
    return this.resolveWorkspaceBranch(this.activeCwd);
  }

  public launchpadProjects(): LaunchpadProjectDto[] {
    const hid = this.selectedHostId;
    if (hid.length === 0 || this.sidebarView === null) {
      return [];
    }
    const results: LaunchpadProjectDto[] = [];
    const list = this.sidebarView.launchpad_projects;
    for (let i = 0; i < list.length; i++) {
      const item = list[i];
      if (item.host_id === undefined || item.host_id === hid) {
        results.push(item);
      }
    }
    return results;
  }

  public isProjectCollapsed(projectId: string): boolean {
    return this.collapsedProjectIds.indexOf(projectId) >= 0;
  }

  public toggleProjectCollapsed(projectId: string): void {
    if (this.collapsedProjectIds.indexOf(projectId) >= 0) {
      this.collapsedProjectIds = this.collapsedProjectIds.filter((id: string) => id !== projectId);
    } else {
      this.collapsedProjectIds = this.collapsedProjectIds.concat([projectId]);
    }
    this.triggerHaptic(6);
  }

  public isThreadPinned(hostId: string, agentId: string): boolean {
    if (this.sidebarView === null || hostId.length === 0 || agentId.length === 0) {
      return false;
    }
    const pinned = this.sidebarView.pinned_conversations;
    for (let i = 0; i < pinned.length; i++) {
      if (pinned[i].host_id === hostId && pinned[i].agent.agent_id === agentId) {
        return true;
      }
    }
    return false;
  }

  public togglePinThread(hostId: string, agentId: string): void {
    if (this.core === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    this.core.togglePinAgent(hostId, agentId);
    this.refreshSidebar();
    this.triggerHaptic(8);
  }

  public markAgentUnread(hostId: string, agentId: string): void {
    if (this.core === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    this.core.markAgentUnread(hostId, agentId);
    this.refreshSidebar();
    this.triggerHaptic(8);
  }

  public isThreadUnread(hostId: string, agent: AgentCardDto): boolean {
    if (
      this.selectedHostId === hostId &&
      !this.isNewSessionDraft &&
      this.selectedAgentId === agent.agent_id
    ) {
      return false;
    }
    if (agent.is_running || agent.is_awaiting_permission) {
      return false;
    }
    if (this.sidebarView === null) {
      return false;
    }
    for (let i = 0; i < this.sidebarView.pinned_conversations.length; i++) {
      const conv = this.sidebarView.pinned_conversations[i];
      if (conv.host_id === hostId && conv.agent.agent_id === agent.agent_id) {
        return conv.is_unread;
      }
    }
    for (let i = 0; i < this.sidebarView.projects.length; i++) {
      const grp = this.sidebarView.projects[i];
      if (grp.host_id !== hostId) {
        continue;
      }
      for (let j = 0; j < grp.conversations.length; j++) {
        if (grp.conversations[j].agent.agent_id === agent.agent_id) {
          return grp.conversations[j].is_unread;
        }
      }
    }
    return false;
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

  public updateSafeArea(
    topVp: number,
    bottomVp: number,
    keyboardVp: number,
    leftVp: number = 0,
    rightVp: number = 0
  ): void {
    this.safeTopVp = Math.max(0, Math.round(topVp));
    this.safeBottomVp = Math.max(0, Math.round(bottomVp));
    this.keyboardHeightVp = Math.max(0, Math.round(keyboardVp));
    this.safeLeftVp = Math.max(0, Math.round(leftVp));
    this.safeRightVp = Math.max(0, Math.round(rightVp));
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
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onAppForeground failed', err as Object);
      }
    }
  }

  public onAppBackground(): void {
    if (this.core !== null) {
      try {
        this.core.onAppBackground();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onAppBackground failed', err as Object);
      }
    }
  }

  public onNetworkChanged(): void {
    if (this.core !== null) {
      try {
        this.core.onNetworkChanged();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onNetworkChanged failed', err as Object);
      }
    }
  }

  public handleDeepLinkUri(uri: string): void {
    const trimmed = uri.trim();
    const hashIdx = trimmed.indexOf('#');
    const clean = hashIdx >= 0 ? trimmed.substring(0, hashIdx).trim() : trimmed;
    const lower = clean.toLowerCase();
    if (!lower.startsWith('orbit://')) {
      return;
    }
    if (this.core === null) {
      this.pendingDeepLink = clean;
      return;
    }
    const isOpenRoute =
      lower === 'orbit://open' ||
      lower.startsWith('orbit://open?') ||
      lower.startsWith('orbit://open/');
    const isPairRoute =
      lower === 'orbit://pair' ||
      lower.startsWith('orbit://pair?') ||
      lower.startsWith('orbit://pair/');

    if (isOpenRoute) {
      const qIdx = clean.indexOf('?');
      let hostId = '';
      let agentId = '';
      if (qIdx >= 0 && qIdx + 1 < clean.length) {
        const queryStr = clean.substring(qIdx + 1);
        const parts = queryStr.split('&');
        for (let i = 0; i < parts.length; i++) {
          const seg = parts[i];
          const eq = seg.indexOf('=');
          if (eq <= 0) {
            continue;
          }
          const k = seg.substring(0, eq).trim().toLowerCase();
          const rawVal = seg.substring(eq + 1).trim();
          let val = rawVal;
          try {
            val = decodeURIComponent(rawVal);
          } catch (_e) {
            val = rawVal;
          }
          if (k === 'host' || k === 'host_id') {
            hostId = val;
          } else if (k === 'agent' || k === 'agent_id') {
            agentId = val;
          }
        }
      }
      if (hostId.length > 0) {
        this.openSessionFromDeepLink(hostId, agentId);
      } else {
        this.showToast('Missing host in Orbit link', true);
      }
      return;
    }
    if (isPairRoute) {
      this.pairHost(clean);
      return;
    }
    this.showToast('Unsupported Orbit link', true);
  }

  private selectValidatedDeepLinkAgent(hostId: string, agentId: string): void {
    const cleanAgent = agentId.trim();
    if (cleanAgent.length === 0) {
      return;
    }
    const detail = this.currentHostDetail;
    const isOnline = detail !== null &&
      detail.host.host_id === hostId &&
      (detail.host.connection_state === 'online_direct' ||
        detail.host.connection_state === 'online_relay');
    if (isOnline && detail !== null) {
      const found = detail.agents.some((ag: AgentCardDto): boolean => ag.agent_id === cleanAgent);
      if (!found) {
        this.showToast('Session not found', true);
        return;
      }
    }
    this.selectAgent(cleanAgent);
  }

  public openSessionFromDeepLink(hostId: string, agentId: string): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    if (this.core === null) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      return;
    }
    const hostExists = this.hosts.some((h: HostCardDto): boolean => h.host_id === cleanHost);
    if (!hostExists) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      this.reloadFleet();
      return;
    }
    this.pendingOpenHostId = '';
    this.pendingOpenAgentId = '';
    if (this.selectedHostId !== cleanHost) {
      this.selectHost(cleanHost);
    }
    if (cleanAgent.length > 0) {
      this.selectValidatedDeepLinkAgent(cleanHost, cleanAgent);
    }
  }

  // Haptics, Clipboard & Native ScanKit QR Pairing

  public triggerHaptic(durationMs: number = 12): void {
    triggerNativeHaptic(durationMs);
  }

  public copyToClipboard(content: string, label: string = 'Copied to clipboard'): void {
    if (content.length === 0) {
      return;
    }
    if (writeClipboardText(content)) {
      this.showToast(label, false);
      this.triggerHaptic(10);
    } else {
      this.showToast('Failed to copy to clipboard', true);
    }
  }

  public pasteAuthorizedPairingUri(onFilled?: (uri: string) => void): void {
    readAuthorizedClipboardText().then((raw: string) => {
      if (raw.length === 0) {
        this.showToast('Clipboard is empty', true);
        return;
      }
      const match = /orbit:\/\/pair[^\s"'<>]*/.exec(raw);
      if (match !== null && match[0].length > 'orbit://pair'.length) {
        const candidate = match[0].trim();
        if (onFilled !== undefined) {
          onFilled(candidate);
        }
        this.pairHost(candidate);
      } else if (onFilled !== undefined) {
        onFilled(raw);
      } else {
        this.showToast('No orbit://pair URI found in clipboard', true);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'pasteAuthorizedPairingUri failed', err);
      this.showToast('Unable to read clipboard', true);
    });
  }

  public openHostSettings(): void {
    this.showHostSettingsSheet = true;
    this.triggerHaptic(8);
  }

  public scanQrAndPair(context: common.Context | undefined): void {
    scanPairingQrCode(context).then((outcome) => {
      if (outcome.cancelled) {
        return;
      }
      if (outcome.uri.length > 0) {
        this.triggerHaptic(18);
        this.pairHost(outcome.uri);
      } else if (outcome.errorMessage.length > 0) {
        this.showToast(outcome.errorMessage, true);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'scanQrAndPair failed', err);
      this.showToast('QR Scanner unavailable', true);
    });
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
        this.reloadGitDiff(this.activeCwd);
        this.reloadWorkspaceFiles();
      } else if (
        ev.kind === 'terminal_changed' &&
        ev.host_id === this.selectedHostId &&
        this.showPtySheet
      ) {
        this.reloadTerminal();
      }
    } catch (err) {
      orbitLogWarn('OrbitStore', 'Malformed core notification payload', err as Object);
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
      if (this.pendingOpenHostId.length > 0) {
        const targetHost = this.pendingOpenHostId;
        const targetAgent = this.pendingOpenAgentId;
        this.pendingOpenHostId = '';
        this.pendingOpenAgentId = '';
        if (snap.hosts.some((h: HostCardDto): boolean => h.host_id === targetHost)) {
          this.selectHost(targetHost);
          if (targetAgent.length > 0) {
            this.selectValidatedDeepLinkAgent(targetHost, targetAgent);
          }
        } else {
          this.showToast('Host not found', true);
          if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
            this.selectHost(snap.hosts[0].host_id);
          } else {
            this.refreshSidebar();
          }
        }
      } else if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
        this.selectHost(snap.hosts[0].host_id);
      } else {
        this.refreshSidebar();
      }
    } catch (e) {
      this.lastError = (e as Error).message;
    }
  }

  private agentQueueKey(hostId: string, agentId: string): string {
    return `${hostId}::${agentId}`;
  }

  private selectedHostSupportsServerQueue(): boolean {
    if (this.currentHostDetail !== null && this.currentHostDetail.host.host_id === this.selectedHostId) {
      return this.currentHostDetail.host.supports_server_queue === true;
    }
    for (let i = 0; i < this.hosts.length; i++) {
      if (this.hosts[i].host_id === this.selectedHostId) {
        return this.hosts[i].supports_server_queue === true;
      }
    }
    return false;
  }

  private hostSupportsServerQueueById(hostId: string): boolean {
    if (this.currentHostDetail !== null && this.currentHostDetail.host.host_id === hostId) {
      return this.currentHostDetail.host.supports_server_queue === true;
    }
    for (let i = 0; i < this.hosts.length; i++) {
      if (this.hosts[i].host_id === hostId) {
        return this.hosts[i].supports_server_queue === true;
      }
    }
    return false;
  }

  private syncActiveQueuedPrompts(): void {
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      this.activeQueuedPrompts = [];
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    const localList = this.queuedPromptsByAgent.get(key);
    if (this.selectedHostSupportsServerQueue()) {
      let activeAgent: AgentCardDto | null = null;
      if (this.currentHostDetail !== null) {
        for (let i = 0; i < this.currentHostDetail.agents.length; i++) {
          if (this.currentHostDetail.agents[i].agent_id === this.selectedAgentId) {
            activeAgent = this.currentHostDetail.agents[i];
            break;
          }
        }
      }
      const serverList: QueuedPromptDto[] | undefined =
        activeAgent !== null ? activeAgent.queued_prompts : undefined;
      if (serverList === undefined || serverList.length === 0) {
        this.activeQueuedPrompts = [];
        return;
      }
      const merged: QueuedPromptItem[] = [];
      for (let i = 0; i < serverList.length; i++) {
        const sq = serverList[i];
        let cached: QueuedPromptItem | null = null;
        if (localList !== undefined) {
          for (let j = 0; j < localList.length; j++) {
            if (localList[j].id === sq.queue_id) {
              cached = localList[j];
              break;
            }
          }
        }
        if (cached !== null) {
          merged.push(cached);
        } else {
          const sqBadges: PromptBadgeDto[] = sq.badges !== undefined ? sq.badges : [];
          merged.push({
            id: sq.queue_id,
            hostKey: key,
            rawText: sq.display_text,
            displayPrompt: sq.display_text,
            wirePrompt: sq.display_text,
            badgesJson: sqBadges.length > 0 ? JSON.stringify(sqBadges) : null,
            badges: sqBadges,
            quotedSnippets: [],
            stagedAttachments: [],
            inlineMentions: [],
            createdAtMs: Date.now()
          });
        }
      }
      this.activeQueuedPrompts = merged;
      return;
    }
    if (localList === undefined || localList.length === 0) {
      this.activeQueuedPrompts = [];
      return;
    }
    const copy: QueuedPromptItem[] = [];
    for (let i = 0; i < localList.length; i++) {
      copy.push(localList[i]);
    }
    this.activeQueuedPrompts = copy;
  }

  private drainQueuedPromptsForAgentIfIdle(hostId: string, agent: AgentCardDto): void {
    if (this.core === null || hostId.length === 0 || agent.agent_id.length === 0) {
      return;
    }
    if (this.hostSupportsServerQueueById(hostId)) {
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
      this.automationsRefreshSeq += 1;
      this.sideQuestionGen += 1;
      this.sideQuestionStreaming = false;
      this.selectedAgentId = '';
      this.currentAgent = null;
      this.stageRevision = 0;
      this.stageHostId = '';
      this.stageAgentId = '';
      this.stageTurns = [];
      this.pendingPermissions = [];
      this.pendingQuestions = [];
      this.sessionDiffs = [];
      this.schedules = [];
      this.conditionalTimers = [];
      this.goalRuns = [];
      this.monitorWatches = [];
      this.workflowRuns = [];
      this.automationsLoading = false;
      this.resetWorkspaceBoardState();
      this.syncActiveQueuedPrompts();
    }
    this.reloadHost(hostId);
    this.refreshSidebar();
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
      this.schedules = detail.schedules ?? [];
      this.conditionalTimers = detail.conditional_timers ?? [];
      this.goalRuns = detail.goal_runs ?? [];
      this.monitorWatches = detail.monitor_watches ?? [];
      this.workflowRuns = detail.workflow_runs ?? [];
      this.syncActiveWorkspaceBoardFromDetail(detail);

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
        let matchedAgent: AgentCardDto | null = null;
        for (let i = 0; i < detail.agents.length; i++) {
          if (detail.agents[i].agent_id === this.selectedAgentId) {
            matchedAgent = detail.agents[i];
            break;
          }
        }
        this.currentAgent = matchedAgent;
        this.syncActiveQueuedPrompts();
      } else if (
        !this.isNewSessionDraft &&
        detail.agents.length > 0 &&
        this.stageTurns.length === 0 &&
        this.composerText.length === 0
      ) {
        this.selectAgent(detail.agents[0].agent_id);
      }

      if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
        const available = detail.host.available_providers;
        const targetProvider =
          available.length > 0 && !available.includes(this.composerProvider)
            ? available[0]
            : this.composerProvider;
        this.syncDefaultModelAndEffort(targetProvider);
      }

      if (!this.isNewSessionDraft && this.selectedAgentId.length > 0) {
        for (let i = 0; i < detail.agents.length; i++) {
          if (detail.agents[i].agent_id === this.selectedAgentId) {
            this.core.markAgentSeen(hostId, this.selectedAgentId, detail.agents[i].latest_seq);
            break;
          }
        }
      }
      this.refreshSidebar();

      for (let i = 0; i < detail.agents.length; i++) {
        this.drainQueuedPromptsForAgentIfIdle(hostId, detail.agents[i]);
      }
    } catch (e) {
      orbitLogWarn('OrbitStore', 'reloadHost failed', e as Object);
    }
  }

  public setActiveCwd(cwd: string): void {
    if (cwd.length === 0) {
      return;
    }
    const changed = this.activeCwd !== cwd;
    this.activeCwd = cwd;
    if (changed) {
      this.refreshSidebar();
      if (this.currentHostDetail !== null) {
        this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
      } else {
        this.resetWorkspaceBoardState();
      }
    }
    if (this.core !== null && this.selectedHostId.length > 0) {
      try {
        this.core.fetchGitStatus(this.selectedHostId, cwd);
        this.core.fetchGitDiff(this.selectedHostId, cwd, null, this.gitStagedMode);
        this.core.fetchWorkspaceFiles(this.selectedHostId, cwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'setActiveCwd fetch failed', err as Object);
      }
      this.reloadGitDiff(cwd);
      this.reloadWorkspaceFiles();
    }
  }

  public startNewSessionDraft(cwd?: string): void {
    this.isNewSessionDraft = true;
    this.selectedAgentId = '';
    this.currentAgent = null;
    this.stageRevision = 0;
    this.stageHostId = '';
    this.stageAgentId = '';
    this.stageTurns = [];
    this.pendingPermissions = [];
    this.pendingQuestions = [];
    this.sessionDiffs = [];
    this.expandedTurnKeys = [];
    this.expandedToolIds = [];
    this.syncDefaultModelAndEffort(this.composerProvider);
    this.syncActiveQueuedPrompts();
    if (cwd !== undefined && cwd.length > 0) {
      this.setActiveCwd(cwd);
    } else if (this.currentHostDetail !== null) {
      this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
    } else {
      this.resetWorkspaceBoardState();
    }
    this.refreshSidebar();
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
    if (this.core !== null && this.selectedHostId.length > 0) {
      const agents = this.currentHostDetail?.agents ?? [];
      let matchedSeq = 0;
      for (let i = 0; i < agents.length; i++) {
        if (agents[i].agent_id === agentId) {
          matchedSeq = agents[i].latest_seq;
          break;
        }
      }
      this.core.markAgentSeen(this.selectedHostId, agentId, matchedSeq);
    }
    this.syncActiveQueuedPrompts();
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.core.openAgentTimeline(this.selectedHostId, agentId);
      this.reloadStage();
      if (this.currentHostDetail !== null) {
        this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
      } else {
        this.resetWorkspaceBoardState();
      }
      this.refreshSidebar();
    }
    this.triggerHaptic(8);
  }

  public reloadStage(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      const sameStage =
        this.stageHostId === this.selectedHostId &&
        this.stageAgentId === this.selectedAgentId;
      const sinceRev = sameStage ? this.stageRevision : 0;
      let delta = JSON.parse(
        this.core.agentStageDeltaJson(this.selectedHostId, this.selectedAgentId, sinceRev)
      ) as AgentStageDeltaDto;
      if (
        !delta.reset &&
        sinceRev > 0 &&
        delta.replace_from_turn_index > this.stageTurns.length
      ) {
        delta = JSON.parse(
          this.core.agentStageDeltaJson(this.selectedHostId, this.selectedAgentId, 0)
        ) as AgentStageDeltaDto;
      }

      if (delta.reset || !sameStage || delta.replace_from_turn_index === 0) {
        this.stageTurns = delta.changed_turns;
      } else {
        const prefix = this.stageTurns.slice(0, delta.replace_from_turn_index);
        this.stageTurns = prefix.concat(delta.changed_turns);
      }

      this.stageHostId = delta.host_id;
      this.stageAgentId = delta.agent !== null ? delta.agent.agent_id : this.selectedAgentId;
      this.stageRevision = delta.revision;
      this.pendingPermissions = delta.pending_permissions;
      this.pendingQuestions = delta.pending_questions;
      this.sessionDiffs = delta.session_turn_diffs;

      if (delta.agent !== null) {
        const prevAgent = this.currentAgent;
        const modelChanged =
          prevAgent === null ||
          prevAgent.agent_id !== delta.agent.agent_id ||
          prevAgent.provider !== delta.agent.provider ||
          prevAgent.model !== delta.agent.model ||
          prevAgent.effort !== delta.agent.effort;
        this.currentAgent = delta.agent;
        if (this.activeCwd !== delta.agent.cwd) {
          this.activeCwd = delta.agent.cwd;
        }
        if (modelChanged) {
          this.composerProvider = delta.agent.provider;
          const resolved = this.resolveDefaultModelAndEffort(
            delta.agent.provider,
            delta.agent.model ?? '',
            delta.agent.effort ?? ''
          );
          this.composerModel = (
            delta.agent.model !== null &&
            delta.agent.model !== undefined &&
            delta.agent.model.trim().length > 0 &&
            resolved.model.length === 0
          )
            ? delta.agent.model
            : resolved.model;
          this.composerEffort = (
            delta.agent.effort !== null &&
            delta.agent.effort !== undefined &&
            delta.agent.effort.trim().length > 0 &&
            resolved.effort.length === 0
          )
            ? delta.agent.effort
            : resolved.effort;
        }
        const nextMode = normalizeModeSlug(delta.agent.mode);
        if (this.composerMode !== nextMode) {
          this.composerMode = nextMode;
        }
        const nextFast = delta.agent.fast_mode === true;
        if (this.composerFastMode !== nextFast) {
          this.composerFastMode = nextFast;
        }
        if (!this.isNewSessionDraft && this.selectedAgentId === delta.agent.agent_id) {
          this.core.markAgentSeen(
            this.selectedHostId,
            delta.agent.agent_id,
            Math.max(delta.agent.latest_seq, delta.latest_seq)
          );
          if (!delta.agent.is_running) {
            this.refreshSidebar();
          }
        }
        this.drainQueuedPromptsForAgentIfIdle(this.selectedHostId, delta.agent);
      } else if (!this.isNewSessionDraft && this.selectedAgentId.length > 0) {
        this.core.markAgentSeen(this.selectedHostId, this.selectedAgentId, delta.latest_seq);
      }
    } catch (e) {
      orbitLogWarn('OrbitStore', 'reloadStage failed', e as Object);
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

  public refreshModelsAndQuotas(forceModels: boolean = true): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.core.refreshQuotas(this.selectedHostId);
    this.core.refreshProviderModels(this.selectedHostId, forceModels);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing models…', false);
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
    } catch (err) {
      orbitLogWarn('OrbitStore', 'removePinnedProject failed', err as Object);
    }
  }

  public browseDirectory(path: string | null): void {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    this.showDirBrowser = true;
    try {
      this.core.listDirectory(this.selectedHostId, path);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'browseDirectory failed', err as Object);
    }
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
    } catch (err) {
      orbitLogWarn('OrbitStore', 'reloadWorkspaceFiles failed', err as Object);
    }
  }

  public ensureWorkspaceFilesLoaded(): void {
    if (
      this.workspaceFiles.length === 0 &&
      this.core !== null &&
      this.selectedHostId.length > 0 &&
      this.activeCwd.length > 0
    ) {
      try {
        this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'ensureWorkspaceFilesLoaded failed', err as Object);
      }
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

  public filterSlashCommands(query: string): SlashCommandDto[] {
    const clean = query.trim();
    if (clean.length === 0 && this.slashCommands.length > 0) {
      return this.slashCommands;
    }
    if (this.core !== null) {
      try {
        return JSON.parse(
          this.core.slashCommandsJson('harmony', clean.length > 0 ? clean : null)
        ) as SlashCommandDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'filterSlashCommands failed', err as Object);
      }
    }
    return this.slashCommands;
  }

  public rankMentionCandidates(query: string, limit: number = 11): MentionCandidateDto[] {
    if (this.core === null || this.selectedHostId.length === 0) {
      return [];
    }
    try {
      const req: MentionRankRequestDto = {
        host_id: this.selectedHostId,
        cwd: this.activeCwd,
        query: query,
        limit: limit
      };
      return JSON.parse(
        this.core.rankMentionCandidatesJson(JSON.stringify(req))
      ) as MentionCandidateDto[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'rankMentionCandidates failed', err as Object);
      return [];
    }
  }

  public attachInlineMention(candidate: MentionCandidateDto, strippedText: string): void {
    this.composerText = strippedText;
    if (candidate.kind === 'git_diff' || candidate.kind === 'diff') {
      this.attachWorkingTreeDiffBadge();
    } else if (candidate.kind === 'staged') {
      this.attachStagedDiffBadge();
    } else if (candidate.kind === 'terminal') {
      this.attachTerminalOutputBadge();
    } else if (candidate.kind === 'file' && candidate.file_path.length > 0) {
      this.attachFileBadge(candidate.file_path);
    }
  }

  public registerInlineMention(
    token: string,
    kind: string,
    path: string = '',
    staged: boolean = false,
    label: string = ''
  ): void {
    if (token.trim().length === 0) {
      return;
    }
    const next: InlineMentionRegistration[] = [];
    for (let i = 0; i < this.inlineMentions.length; i++) {
      if (this.inlineMentions[i].token !== token) {
        next.push(this.inlineMentions[i]);
      }
    }
    next.push({
      token: token,
      kind: kind,
      path: path,
      staged: staged,
      label: label
    });
    this.inlineMentions = next;
    if (kind === 'terminal' && this.terminalRows.length === 0 && this.core !== null && this.selectedHostId.length > 0) {
      this.reloadTerminal();
    }
    this.triggerHaptic(8);
  }

  public setSlashDirective(slash: string): void {
    this.activeSlashDirective = slash.trim();
    this.triggerHaptic(8);
  }

  public clearSlashDirective(): void {
    if (this.activeSlashDirective.length > 0) {
      this.activeSlashDirective = '';
      this.triggerHaptic(6);
    }
  }

  public applyQuickWorkflow(slash: string): void {
    const clean = slash.trim().toLowerCase();
    if (clean === '/compact') {
      this.submitSlashCompact('');
      return;
    }
    this.setSlashDirective(clean);
  }

  public executeSlashCommand(
    cmd: SlashCommandDto,
    onOpenTerminal?: () => void,
    onOpenSettings?: () => void
  ): void {
    if (cmd.id === 'plan') {
      this.setSlashDirective('/plan');
      return;
    }
    if (cmd.id === 'review') {
      this.setSlashDirective('/review');
      return;
    }
    if (cmd.id === 'diff') {
      this.toggleTopBarGitDiff();
      return;
    }
    if (cmd.id === 'compact') {
      this.submitSlashCompact('');
      return;
    }
    if (cmd.id === 'btw') {
      this.openSideQuestionSheet();
      return;
    }
    if (cmd.id === 'ask' || cmd.id === 'supervised') {
      this.setSessionMode('default');
      this.showToast('Permissions: Always Ask', false);
      return;
    }
    if (cmd.id === 'auto') {
      this.setSessionMode('auto');
      this.showToast('Permissions: Auto (Guardian)', false);
      return;
    }
    if (cmd.id === 'edits') {
      this.setSessionMode('accept_edits');
      this.showToast('Permissions: Accept Edits', false);
      return;
    }
    if (cmd.id === 'full') {
      this.setSessionMode('full_access');
      this.showToast('Permissions: Full Access', false);
      return;
    }
    if (cmd.id === 'terminal') {
      if (onOpenTerminal !== undefined) {
        onOpenTerminal();
      } else {
        this.showPtySheet = !this.showPtySheet;
      }
      return;
    }
    if (cmd.id === 'new') {
      this.startNewSessionDraft();
      return;
    }
    if (cmd.id === 'settings') {
      if (onOpenSettings !== undefined) {
        onOpenSettings();
      } else {
        this.showHostSettingsSheet = true;
      }
    }
  }

  public async attachPhotoUri(uri: string): Promise<void> {
    await this.stageUriAttachment(uri, 'photo.jpg', 'image/jpeg', true);
  }

  public async attachDocumentUri(uri: string): Promise<void> {
    await this.stageUriAttachment(uri, 'attachment.txt', 'application/octet-stream', false);
  }

  public removeStagedAttachment(id: string): void {
    const next: ComposerAttachmentItem[] = [];
    for (let i = 0; i < this.stagedAttachments.length; i++) {
      if (this.stagedAttachments[i].id !== id) {
        next.push(this.stagedAttachments[i]);
      }
    }
    this.stagedAttachments = next;
    this.triggerHaptic(6);
  }

  private async stageUriAttachment(
    uri: string,
    fallbackName: string,
    fallbackMime: string,
    forceImage: boolean
  ): Promise<void> {
    if (uri.trim().length === 0) {
      return;
    }
    if (this.core === null || this.selectedHostId.length === 0) {
      this.showToast('Select a host first to attach files', true);
      return;
    }
    this.isStagingAttachment = true;
    try {
      const loaded = await readUriBytesAsync(uri, fallbackName, fallbackMime);
      const isImage = forceImage || loaded.mimeType.startsWith('image/');
      const rawJson = await this.core.stageAttachmentJson(
        this.selectedHostId,
        loaded.filename,
        loaded.mimeType,
        loaded.bytes
      );
      const staged = JSON.parse(rawJson) as StagedAttachmentDto;
      const item: ComposerAttachmentItem = {
        id: `att_${Date.now()}_${this.stagedAttachments.length}`,
        path: staged.path,
        filename: staged.filename.length > 0 ? staged.filename : loaded.filename,
        sizeBytes: staged.size_bytes,
        isImage: isImage,
        localUri: isImage ? uri : ''
      };
      this.stagedAttachments = this.stagedAttachments.concat([item]);
      this.triggerHaptic(10);
    } catch (e) {
      orbitLogWarn('OrbitStore', 'stageUriAttachment failed', e as Object);
      this.showToast((e as Error).message || 'Failed to attach file', true);
    } finally {
      this.isStagingAttachment = false;
    }
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
    const normalized = mode === 'plan' || mode.length === 0 ? 'default' : mode;
    this.composerMode = normalized;
    if (this.core !== null && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
      this.core.setSessionMode(this.selectedHostId, this.selectedAgentId, normalized);
    }
    this.triggerHaptic(8);
  }

  public cycleSessionMode(): void {
    const order: string[] = ['default', 'auto', 'accept_edits', 'full_access'];
    const normalized = normalizeModeSlug(this.composerMode);
    const curIdx = order.indexOf(normalized);
    const nextMode = curIdx >= 0 ? order[(curIdx + 1) % order.length] : 'accept_edits';
    this.setSessionMode(nextMode);
  }

  public catalogForProvider(provider: string): ProviderModelCatalogDto | null {
    const detail = this.currentHostDetail;
    if (detail !== null) {
      for (let i = 0; i < detail.provider_catalogs.length; i++) {
        if (detail.provider_catalogs[i].provider === provider && detail.provider_catalogs[i].models.length > 0) {
          return detail.provider_catalogs[i];
        }
      }
    }
    for (let i = 0; i < this.cachedFallbackCatalogs.length; i++) {
      if (this.cachedFallbackCatalogs[i].provider === provider) {
        return this.cachedFallbackCatalogs[i];
      }
    }
    if (this.core !== null) {
      try {
        return JSON.parse(
          this.core.fallbackProviderCatalogJson(provider)
        ) as ProviderModelCatalogDto;
      } catch (err) {
        orbitLogWarn('OrbitStore', 'fallbackProviderCatalogJson failed', err as Object);
      }
    }
    return null;
  }

  public resolveModelSelectionFromCore(
    provider: string,
    desiredModel: string,
    desiredEffort: string
  ): ResolvedModelSelectionDto | null {
    if (this.core === null) {
      return null;
    }
    try {
      const cleanModel = desiredModel.trim();
      const cleanEffort = desiredEffort.trim();
      const req: ModelSelectionRequestDto = {
        host_id: this.selectedHostId.length > 0 ? this.selectedHostId : null,
        provider: provider,
        desired_model: cleanModel.length > 0 ? cleanModel : null,
        desired_effort: cleanEffort.length > 0 ? cleanEffort : null
      };
      return JSON.parse(
        this.core.resolveModelSelectionJson(JSON.stringify(req))
      ) as ResolvedModelSelectionDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolveModelSelectionJson failed', err as Object);
      return null;
    }
  }

  public effortDescriptionText(effort: string): string {
    if (this.core !== null && effort.trim().length > 0) {
      try {
        return this.core.defaultEffortDescription(effort);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'defaultEffortDescription failed', err as Object);
      }
    }
    return effort.length > 0 ? formatEffortLabel(effort) : '';
  }

  public resolveDefaultModelAndEffort(
    provider: string,
    desiredModel: string,
    desiredEffort: string
  ): DraftModelSelection {
    const fromCore = this.resolveModelSelectionFromCore(provider, desiredModel, desiredEffort);
    if (fromCore !== null) {
      return {
        model: fromCore.selected_model_id ?? '',
        effort: fromCore.selected_effort ?? ''
      };
    }

    const models = this.modelsForProvider(provider);
    const catalog = this.catalogForProvider(provider);

    let resolvedModel = '';
    if (desiredModel.trim().length > 0) {
      const cleanDesired = desiredModel.trim();
      const existsInCatalog = models.length === 0 || models.some((m: ProviderModelSpecDto) => m.id === cleanDesired);
      if (existsInCatalog) {
        resolvedModel = cleanDesired;
      }
    }
    if (resolvedModel.length === 0 && catalog !== null && catalog.configured_default_model !== null && catalog.configured_default_model.trim().length > 0) {
      resolvedModel = catalog.configured_default_model.trim();
    }
    if (resolvedModel.length === 0) {
      for (let i = 0; i < models.length; i++) {
        if (models[i].is_default) {
          resolvedModel = models[i].id;
          break;
        }
      }
    }
    if (resolvedModel.length === 0 && models.length > 0) {
      resolvedModel = models[0].id;
    }

    let spec: ProviderModelSpecDto | null = null;
    for (let i = 0; i < models.length; i++) {
      if (models[i].id === resolvedModel) {
        spec = models[i];
        break;
      }
    }
    if (spec === null && models.length > 0) {
      spec = models[0];
    }

    const options = spec !== null ? spec.thinking_options : [];
    if (options.length === 0) {
      const fallbackEff = (spec !== null && spec.default_effort !== undefined && spec.default_effort !== null)
        ? spec.default_effort
        : '';
      return { model: resolvedModel, effort: fallbackEff };
    }
    for (let o = 0; o < options.length; o++) {
      if (options[o].is_default) {
        return { model: resolvedModel, effort: options[o].effort };
      }
    }
    return { model: resolvedModel, effort: options[0].effort };
  }

  public syncDefaultModelAndEffort(provider?: string): void {
    const targetProvider = provider !== undefined && provider.length > 0 ? provider : this.composerProvider;
    const saved = this.draftSelectionByProvider.get(targetProvider);
    const desiredModel = (this.hasUserCustomizedDraftModel && this.composerProvider === targetProvider)
      ? this.composerModel
      : (saved !== undefined ? saved.model : '');
    const desiredEffort = (this.hasUserCustomizedDraftModel && this.composerProvider === targetProvider)
      ? this.composerEffort
      : (saved !== undefined ? saved.effort : '');
    const resolved = this.resolveDefaultModelAndEffort(targetProvider, desiredModel, desiredEffort);
    this.composerProvider = targetProvider;
    this.composerModel = resolved.model;
    this.composerEffort = resolved.effort;
  }

  public switchDraftProvider(provider: string): void {
    if (provider.length === 0) {
      return;
    }
    const prevSel: DraftModelSelection = {
      model: this.composerModel,
      effort: this.composerEffort
    };
    this.draftSelectionByProvider.set(this.composerProvider, prevSel);
    this.hasUserCustomizedDraftModel = true;
    const saved = this.draftSelectionByProvider.get(provider);
    const resolved = this.resolveDefaultModelAndEffort(
      provider,
      saved !== undefined ? saved.model : '',
      saved !== undefined ? saved.effort : ''
    );
    this.draftSelectionByProvider.set(provider, resolved);
    this.applyProviderModelEffort(provider, resolved.model, resolved.effort);
  }

  public selectEffort(effort: string): void {
    this.applyProviderModelEffort(this.composerProvider, this.composerModel, effort);
  }

  public applyModelSelection(modelId: string, effort: string): void {
    this.applyProviderModelEffort(this.composerProvider, modelId, effort);
  }

  public applyProviderModelEffort(provider: string, model: string, effort: string): void {
    const prevProvider = this.composerProvider;
    const resolved = this.resolveDefaultModelAndEffort(provider, model, effort);
    this.composerProvider = provider;
    this.composerModel = resolved.model;
    this.composerEffort = resolved.effort;
    if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
      this.hasUserCustomizedDraftModel = true;
      this.draftSelectionByProvider.set(provider, resolved);
    }
    if (this.core !== null && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
      const modelOpt = resolved.model.trim().length > 0 ? resolved.model.trim() : null;
      const effortOpt = resolved.effort.trim().length > 0 ? resolved.effort.trim() : null;
      if (prevProvider !== provider) {
        this.core.switchAgentProvider(
          this.selectedHostId,
          this.selectedAgentId,
          provider,
          modelOpt,
          effortOpt,
          null
        );
      } else {
        this.core.setAgentConfig(this.selectedHostId, this.selectedAgentId, modelOpt, effortOpt);
      }
    }
    this.triggerHaptic(10);
  }

  public currentModelDisplayLabel(): string {
    const resolved = this.resolveModelSelectionFromCore(
      this.composerProvider,
      this.composerModel,
      this.composerEffort
    );
    let modelLabel = resolved !== null
      ? resolved.selected_model_label
      : this.composerModel.trim();
    if (modelLabel.length === 0) {
      const specs = this.modelsForProvider(this.composerProvider);
      modelLabel = specs.length > 0 ? specs[0].label : 'Default';
    }
    if (this.composerProvider === 'claude_code' && modelLabel.startsWith('Claude ')) {
      modelLabel = modelLabel.substring('Claude '.length).trim();
    }
    return modelLabel;
  }

  public currentEffortDisplayLabel(): string {
    const resolved = this.resolveModelSelectionFromCore(
      this.composerProvider,
      this.composerModel,
      this.composerEffort
    );
    if (resolved !== null) {
      return resolved.selected_effort_label ?? '';
    }
    return '';
  }

  public currentModelPillLabel(): string {
    const modelLabel = this.currentModelDisplayLabel();
    const effortTag = this.currentEffortDisplayLabel();
    return effortTag.length > 0 ? `${modelLabel} ${effortTag}` : modelLabel;
  }

  public currentModePillLabel(): string {
    return formatModeLabel(this.composerMode);
  }

  public modelsForProvider(provider: string): ProviderModelSpecDto[] {
    const cat = this.catalogForProvider(provider);
    if (cat !== null && cat.models.length > 0) {
      return cat.models;
    }
    const detail = this.currentHostDetail;
    if (detail !== null) {
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
    return [];
  }

  public isActiveAgentRunning(): boolean {
    if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
      return false;
    }
    const turnActive = this.stageTurns.some((t: StageTurnDto) => t.has_active_item);
    if (turnActive) {
      return true;
    }
    if (this.currentAgent === null) {
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
      this.activeSlashDirective.length > 0 ||
      this.stagedAttachments.length > 0 ||
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

  private parseSlashDirectiveEnum(rawDirective: string): string | null {
    const clean = rawDirective.trim().toLowerCase().replace(/^\//, '');
    if (
      clean === 'plan' ||
      clean === 'review' ||
      clean === 'compact' ||
      clean === 'orbit' ||
      clean === 'orbit-handoff' ||
      clean === 'orbit-committee' ||
      clean === 'orbit-advisor'
    ) {
      return clean;
    }
    return null;
  }

  private parseQuoteLineNumber(lineLabel: string): number | null {
    const marker = lineLabel.lastIndexOf(':L');
    if (marker < 0) {
      return null;
    }
    const num = Number.parseInt(lineLabel.substring(marker + 2), 10);
    return Number.isFinite(num) && num > 0 ? num : null;
  }

  private composePromptViaCore(
    rawInput: string,
    explicitDirectiveRaw: string,
    sourceBadges: PromptBadgeDto[],
    sourceQuotes: QuotedDiffSnippet[],
    sourceAttachments: ComposerAttachmentItem[],
    sourceInlineMentions: InlineMentionRegistration[]
  ): ComposedPromptDto | null {
    if (this.core === null) {
      return null;
    }
    const contexts: ComposerContextInputDto[] = [];
    const quotes: ComposerQuoteInputDto[] = [];
    let effectiveExplicit = this.parseSlashDirectiveEnum(explicitDirectiveRaw);

    for (let i = 0; i < sourceAttachments.length; i++) {
      const att = sourceAttachments[i];
      if (att.isImage) {
        contexts.push({ kind: 'image', path: att.path, filename: att.filename });
      } else {
        contexts.push({ kind: 'staged_file', path: att.path, filename: att.filename });
      }
    }

    const termSnap = this.extractRecentTerminalText();
    for (let i = 0; i < sourceBadges.length; i++) {
      const b = sourceBadges[i];
      if (b.kind === 'file') {
        const cleanPath = b.label.startsWith('@') ? b.label.substring(1) : b.label;
        if (cleanPath.length > 0) {
          contexts.push({ kind: 'workspace_file', path: cleanPath });
        }
      } else if (b.kind === 'diff') {
        contexts.push({
          kind: 'git_diff',
          staged: false,
          label: 'Working Tree Diff',
          diff_text: null
        });
      } else if (b.kind === 'staged') {
        contexts.push({
          kind: 'git_diff',
          staged: true,
          label: 'Staged Diff',
          diff_text: null
        });
      } else if (b.kind === 'terminal') {
        contexts.push({
          kind: 'terminal_output',
          label: 'Terminal',
          content: termSnap
        });
      } else if (b.kind === 'command' && effectiveExplicit === null) {
        effectiveExplicit = this.parseSlashDirectiveEnum(b.label);
      }
    }

    for (let i = 0; i < sourceInlineMentions.length; i++) {
      const m = sourceInlineMentions[i];
      if (!rawInput.includes(m.token)) {
        continue;
      }
      if (m.kind === 'file' && m.path.length > 0) {
        contexts.push({ kind: 'workspace_file', path: m.path });
      } else if (m.kind === 'git_diff') {
        contexts.push({
          kind: 'git_diff',
          staged: m.staged,
          label: m.staged ? 'Staged Diff' : 'Working Tree Diff',
          diff_text: null
        });
      } else if (m.kind === 'terminal') {
        contexts.push({
          kind: 'terminal_output',
          label: 'Terminal',
          content: termSnap
        });
      }
    }

    for (let i = 0; i < sourceQuotes.length; i++) {
      const snip = sourceQuotes[i];
      if (snip.kind === 'reply') {
        contexts.push({
          kind: 'assistant_quote',
          label: snip.lineLabel,
          content: snip.content
        });
      } else {
        const prefix = snip.kind === 'added' ? '+' : snip.kind === 'removed' ? '-' : ' ';
        quotes.push({
          path: snip.path.length > 0 ? snip.path : snip.lineLabel,
          line_number: this.parseQuoteLineNumber(snip.lineLabel),
          code: `${prefix}${snip.content}`
        });
      }
    }

    const req: ComposePromptRequestDto = {
      provider: this.composerProvider,
      raw_text: rawInput,
      explicit_directive: effectiveExplicit,
      host_id: this.selectedHostId.length > 0 ? this.selectedHostId : null,
      cwd: this.activeCwd.length > 0 ? this.activeCwd : null,
      contexts: contexts,
      quotes: quotes,
      pastes: []
    };
    try {
      return JSON.parse(this.core.composePromptJson(JSON.stringify(req))) as ComposedPromptDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'composePromptJson failed', err as Object);
      return null;
    }
  }

  public submitSlashCompact(extraFocus: string = ''): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.isNewSessionDraft || this.selectedAgentId.length === 0) {
      this.showToast('Start an active session first to compact context', false);
      return;
    }
    const provider = this.currentAgent !== null ? this.currentAgent.provider : this.composerProvider;
    const req: ComposePromptRequestDto = {
      provider: provider,
      raw_text: extraFocus.trim(),
      explicit_directive: 'compact',
      host_id: this.selectedHostId,
      cwd: this.activeCwd.length > 0 ? this.activeCwd : null,
      contexts: [],
      quotes: [],
      pastes: []
    };
    try {
      const composed = JSON.parse(this.core.composePromptJson(JSON.stringify(req))) as ComposedPromptDto;
      const badgesJson = composed.badges.length > 0 ? JSON.stringify(composed.badges) : null;
      this.core.sendPromptWithMeta(
        this.selectedHostId,
        this.selectedAgentId,
        composed.wire_prompt,
        composed.display_prompt,
        badgesJson
      );
      this.reloadStage();
      this.triggerHaptic(12);
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public enqueueCurrentDraft(): void {
    const rawInput = this.composerText.trim();
    if (!this.hasComposerDraftContent()) {
      return;
    }
    const composed = this.composePromptViaCore(
      rawInput,
      this.activeSlashDirective,
      this.composerBadges,
      this.quotedSnippets,
      this.stagedAttachments,
      this.inlineMentions
    );
    if (composed === null || composed.is_empty) {
      return;
    }
    if (composed.is_btw) {
      const q = (composed.btw_question ?? '').trim();
      this.composerText = '';
      this.activeSlashDirective = '';
      if (q.length > 0) {
        this.askSideQuestion(q);
      }
      return;
    }
    if (composed.is_compact) {
      this.composerText = '';
      this.activeSlashDirective = '';
      this.submitSlashCompact(composed.display_prompt);
      return;
    }
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }

    const copiedBadges: PromptBadgeDto[] = [];
    for (let i = 0; i < composed.badges.length; i++) {
      copiedBadges.push(composed.badges[i]);
    }
    const copiedQuotes: QuotedDiffSnippet[] = [];
    for (let i = 0; i < this.quotedSnippets.length; i++) {
      copiedQuotes.push(this.quotedSnippets[i]);
    }
    const copiedAttachments: ComposerAttachmentItem[] = [];
    for (let i = 0; i < this.stagedAttachments.length; i++) {
      copiedAttachments.push(this.stagedAttachments[i]);
    }
    const copiedMentions: InlineMentionRegistration[] = [];
    for (let i = 0; i < this.inlineMentions.length; i++) {
      copiedMentions.push(this.inlineMentions[i]);
    }

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
      displayPrompt: composed.display_prompt,
      wirePrompt: composed.wire_prompt,
      badgesJson: copiedBadges.length > 0 ? JSON.stringify(copiedBadges) : null,
      badges: copiedBadges,
      quotedSnippets: copiedQuotes,
      stagedAttachments: copiedAttachments,
      inlineMentions: copiedMentions,
      createdAtMs: nowMs
    };
    nextList.push(item);
    this.queuedPromptsByAgent.set(key, nextList);
    if (this.selectedHostSupportsServerQueue() && this.core !== null) {
      try {
        this.core.enqueuePrompt(
          this.selectedHostId,
          this.selectedAgentId,
          item.wirePrompt,
          item.displayPrompt,
          item.badgesJson,
          item.id
        );
      } catch (_err) {
        // Fallback to local cache already saved above
      }
    }
    this.syncActiveQueuedPrompts();

    this.composerText = '';
    this.activeSlashDirective = '';
    this.composerBadges = [];
    this.stagedAttachments = [];
    this.inlineMentions = [];
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
    if (existing !== undefined) {
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
    }
    if (this.selectedHostSupportsServerQueue() && this.core !== null) {
      try {
        this.core.removeQueuedPrompt(this.selectedHostId, this.selectedAgentId, itemId);
      } catch (_err) {
        // Ignore transient disconnect
      }
    }
    this.syncActiveQueuedPrompts();
    this.triggerHaptic(8);
  }

  public editQueuedPrompt(itemId: string): void {
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const key = this.agentQueueKey(this.selectedHostId, this.selectedAgentId);
    let target: QueuedPromptItem | null = null;
    for (let i = 0; i < this.activeQueuedPrompts.length; i++) {
      if (this.activeQueuedPrompts[i].id === itemId) {
        target = this.activeQueuedPrompts[i];
        break;
      }
    }
    if (target === null) {
      return;
    }
    const existing = this.queuedPromptsByAgent.get(key);
    if (existing !== undefined) {
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
    }
    if (this.selectedHostSupportsServerQueue() && this.core !== null) {
      try {
        this.core.removeQueuedPrompt(this.selectedHostId, this.selectedAgentId, itemId);
      } catch (_err) {
        // Ignore transient disconnect
      }
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

    const mergedAttachments: ComposerAttachmentItem[] = [];
    for (let i = 0; i < this.stagedAttachments.length; i++) {
      mergedAttachments.push(this.stagedAttachments[i]);
    }
    for (let i = 0; i < target.stagedAttachments.length; i++) {
      mergedAttachments.push(target.stagedAttachments[i]);
    }
    this.stagedAttachments = mergedAttachments;

    const mergedMentions: InlineMentionRegistration[] = [];
    for (let i = 0; i < this.inlineMentions.length; i++) {
      mergedMentions.push(this.inlineMentions[i]);
    }
    for (let i = 0; i < target.inlineMentions.length; i++) {
      mergedMentions.push(target.inlineMentions[i]);
    }
    this.inlineMentions = mergedMentions;
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
    if (this.selectedHostSupportsServerQueue()) {
      const existingLocal = this.queuedPromptsByAgent.get(key);
      if (existingLocal !== undefined) {
        const restLocal: QueuedPromptItem[] = [];
        for (let i = 0; i < existingLocal.length; i++) {
          if (existingLocal[i].id !== itemId) {
            restLocal.push(existingLocal[i]);
          }
        }
        if (restLocal.length > 0) {
          this.queuedPromptsByAgent.set(key, restLocal);
        } else {
          this.queuedPromptsByAgent.delete(key);
        }
      }
      try {
        this.core.flushQueuedPromptNow(this.selectedHostId, this.selectedAgentId, itemId);
      } catch (_err) {
        // Ignore transient disconnect
      }
      this.syncActiveQueuedPrompts();
      this.triggerHaptic(10);
      return;
    }
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
      overridePrompt === undefined &&
      !this.hasComposerDraftContent()
    ) {
      return;
    }
    if (overridePrompt !== undefined && rawInput.length === 0) {
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

    const composed = this.composePromptViaCore(
      rawInput,
      overridePrompt === undefined ? this.activeSlashDirective : '',
      overridePrompt === undefined ? this.composerBadges : [],
      overridePrompt === undefined ? this.quotedSnippets : [],
      overridePrompt === undefined ? this.stagedAttachments : [],
      overridePrompt === undefined ? this.inlineMentions : []
    );
    if (composed === null || composed.is_empty) {
      return;
    }
    if (overridePrompt === undefined && composed.is_btw) {
      const q = (composed.btw_question ?? '').trim();
      this.composerText = '';
      this.activeSlashDirective = '';
      if (q.length > 0) {
        this.askSideQuestion(q);
      }
      return;
    }
    if (overridePrompt === undefined && composed.is_compact) {
      this.composerText = '';
      this.activeSlashDirective = '';
      this.submitSlashCompact(composed.display_prompt);
      return;
    }

    if (overridePrompt === undefined && this.isActiveAgentRunning()) {
      this.enqueueCurrentDraft();
      return;
    }

    const badgesJson = composed.badges.length > 0 ? JSON.stringify(composed.badges) : null;

    try {
      if (this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
        this.core.sendPromptWithMeta(
          this.selectedHostId,
          this.selectedAgentId,
          composed.wire_prompt,
          composed.display_prompt,
          badgesJson
        );
        this.reloadStage();
      } else {
        const modelOpt = this.composerModel.trim().length > 0 ? this.composerModel.trim() : null;
        const effortOpt = this.composerEffort.trim().length > 0 ? this.composerEffort.trim() : null;
        const wtOpt = this.worktreeBranch.trim().length > 0
          ? this.worktreeBranch.trim()
          : (this.draftUseWorktree ? 'auto' : null);
        const createReq: CreateSessionRequestDto = {
          host_id: this.selectedHostId,
          cwd: this.activeCwd,
          extra_dirs: [],
          provider: this.composerProvider,
          acp_agent_id: null,
          model: modelOpt,
          effort: effortOpt,
          fast_mode: this.composerFastMode,
          mode: normalizeModeSlug(this.composerMode),
          initial_prompt: composed.wire_prompt,
          initial_display_prompt: composed.display_prompt,
          initial_badges: composed.badges,
          worktree_branch: wtOpt,
          workspace_id: null,
          handle: null,
          auto_cluster: null
        };
        const optId = this.core.createSessionJson(JSON.stringify(createReq));
        this.selectedAgentId = optId;
        this.isNewSessionDraft = false;
        this.syncActiveQueuedPrompts();
        this.reloadHost(this.selectedHostId);
        this.reloadStage();
      }
      if (overridePrompt === undefined) {
        this.composerText = '';
        this.activeSlashDirective = '';
        this.composerBadges = [];
        this.stagedAttachments = [];
        this.inlineMentions = [];
        this.quotedSnippets = [];
        this.worktreeBranch = '';
      }
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
      try {
        this.core.connectHost(this.selectedHostId);
        this.core.refreshHost(this.selectedHostId);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'pullRefreshStage host refresh failed', err as Object);
      }
      this.reloadHost(this.selectedHostId);
      if (this.selectedAgentId.length > 0) {
        try {
          this.core.openAgentTimeline(this.selectedHostId, this.selectedAgentId);
        } catch (err) {
          orbitLogWarn('OrbitStore', 'pullRefreshStage timeline open failed', err as Object);
        }
        this.reloadStage();
      }
      this.refreshGitReview(
        this.gitStagedMode,
        this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
      );
      try {
        this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'pullRefreshStage workspace files failed', err as Object);
      }
      this.reloadWorkspaceFiles();
    }
  }

  public pullRefreshGitReview(): void {
    this.triggerHaptic(10);
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      this.core.refreshHost(this.selectedHostId);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pullRefreshGitReview host refresh failed', err as Object);
    }
    this.refreshGitReview(
      this.gitStagedMode,
      this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
    );
    try {
      this.core.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pullRefreshGitReview workspace files failed', err as Object);
    }
    this.reloadWorkspaceFiles();
  }

  public interruptCurrentAgent(): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.core.interruptAgent(this.selectedHostId, this.selectedAgentId);
      this.triggerHaptic(16);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'interruptCurrentAgent failed', err as Object);
      this.showToast('Failed to stop agent', true);
    }
  }

  public archiveSession(agentId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.core.archiveAgent(this.selectedHostId, agentId);
      if (this.selectedAgentId === agentId) {
        this.startNewSessionDraft();
      }
      this.reloadHost(this.selectedHostId);
      this.triggerHaptic(12);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'archiveSession failed', err as Object);
      this.showToast('Failed to archive session', true);
    }
  }

  // Sticky Decision Dock Actions

  public resolvePermission(requestId: string, decision: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.core.resolvePermission(this.selectedHostId, this.selectedAgentId, requestId, decision);
      this.triggerHaptic(14);
      this.reloadStage();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolvePermission failed', err as Object);
      this.showToast('Failed to send approval decision', true);
    }
  }

  public answerQuestion(requestId: string, answers: string[][]): void {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.core.answerQuestion(
        this.selectedHostId,
        this.selectedAgentId,
        requestId,
        JSON.stringify(answers)
      );
      this.triggerHaptic(14);
      this.reloadStage();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'answerQuestion failed', err as Object);
      this.showToast('Failed to submit answer', true);
    }
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
    try {
      this.core.fetchGitStatus(this.selectedHostId, this.activeCwd);
      this.core.fetchGitDiff(this.selectedHostId, this.activeCwd, path, staged);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshGitReview failed', err as Object);
    }
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
    } catch (err) {
      orbitLogWarn('OrbitStore', 'reloadGitDiff failed', err as Object);
    }
  }

  public totalDiffFileCount(): number {
    const source = this.gitDiffFiles.length > 0 ? this.gitDiffFiles : this.sessionDiffs;
    return source.length;
  }

  public activeTerminalCount(): number {
    return this.currentHostDetail !== null ? this.currentHostDetail.terminals.length : 0;
  }

  public openFullGitDiffSheet(): void {
    this.refreshGitReview(
      this.gitStagedMode,
      this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
    );
    this.showGitDiffSheet = true;
    this.triggerHaptic(8);
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
    try {
      this.core.startTerminal(
        this.selectedHostId,
        this.activeCwd,
        this.terminalCols,
        this.terminalGridRows
      );
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startNewTerminal failed', err as Object);
      this.showToast('Failed to start terminal', true);
    }
  }

  public attachTerminal(terminalId: string, slot: number = 1): void {
    if (this.core === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    try {
      this.core.attachTerminal(this.selectedHostId, terminalId, Math.max(1, slot));
      this.reloadHost(this.selectedHostId);
      this.reloadTerminal();
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'attachTerminal failed', err as Object);
    }
  }

  public killTerminal(terminalId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    try {
      this.core.killTerminal(this.selectedHostId, terminalId);
      this.reloadHost(this.selectedHostId);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'killTerminal failed', err as Object);
    }
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
      try {
        this.core.resizeTerminal(this.selectedHostId, clampedCols, clampedRows);
        this.reloadTerminal();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'resizeTerminal failed', err as Object);
      }
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
    } catch (err) {
      orbitLogWarn('OrbitStore', 'reloadTerminal failed', err as Object);
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
    try {
      this.core.sendTerminalKeys(this.selectedHostId, out);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'sendTerminalKeys failed', err as Object);
    }
  }

  // Client Bridge Contract v2 Wrappers

  public async rewindAgent(toBeforeSeq: number, restoreFiles: boolean): Promise<RewindAgentResultDto | null> {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return null;
    }
    try {
      const json = await this.core.rewindAgentJson(
        this.selectedHostId,
        this.selectedAgentId,
        toBeforeSeq,
        restoreFiles
      );
      const result = JSON.parse(json) as RewindAgentResultDto;
      this.reloadHost(this.selectedHostId);
      this.reloadStage();
      if (result.restored_prompt !== null && result.restored_prompt.length > 0) {
        this.composerText = result.restored_prompt;
      }
      if (result.notice !== null && result.notice.length > 0) {
        this.showToast(result.notice, false);
      }
      return result;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'rewindAgentJson failed', err as Object);
      this.showToast((err as Error).message, true);
      return null;
    }
  }

  public async readWorkspaceFile(path: string, cwdOverride?: string): Promise<WorkspaceFileViewDto | null> {
    if (this.core === null || this.selectedHostId.length === 0 || path.trim().length === 0) {
      return null;
    }
    const targetCwd = cwdOverride !== undefined && cwdOverride.length > 0 ? cwdOverride : this.activeCwd;
    try {
      const json = await this.core.readWorkspaceFileJson(this.selectedHostId, targetCwd, path);
      return JSON.parse(json) as WorkspaceFileViewDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'readWorkspaceFileJson failed', err as Object);
      this.showToast((err as Error).message, true);
      return null;
    }
  }

  public buildSideQuestionContext(maxChars: number = 4000): string {
    if (this.core === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return '';
    }
    try {
      return this.core.buildSideQuestionContext(this.selectedHostId, this.selectedAgentId, maxChars);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'buildSideQuestionContext failed', err as Object);
      return '';
    }
  }

  public askSideQuestion(question: string, contextSummary?: string): string {
    const cleanQ = question.trim();
    if (this.core === null || this.selectedHostId.length === 0 || cleanQ.length === 0) {
      return '';
    }
    this.sideQuestionGen += 1;
    const gen = this.sideQuestionGen;
    this.showSideQuestionSheet = true;
    this.sideQuestionActive = true;
    this.sideQuestionStreaming = true;
    this.sideQuestionPrompt = cleanQ;
    this.sideQuestionAnswer = '';
    this.sideQuestionError = '';

    const resolvedContext = contextSummary !== undefined
      ? contextSummary
      : (this.selectedAgentId.length > 0 ? this.buildSideQuestionContext(4000) : '');
    const req: AskSideQuestionRequestDto = {
      host_id: this.selectedHostId,
      agent_id: this.selectedAgentId.length > 0 ? this.selectedAgentId : null,
      cwd: this.activeCwd,
      provider: this.composerProvider,
      model: this.composerModel.trim().length > 0 ? this.composerModel.trim() : null,
      question: cleanQ,
      context_summary: resolvedContext.length > 0 ? resolvedContext : null
    };
    try {
      return this.core.askSideQuestion(JSON.stringify(req), (err: Error | null, eventJson: string) => {
        if (gen !== this.sideQuestionGen) {
          return;
        }
        if (err !== null && err !== undefined) {
          this.sideQuestionError = err.message;
          this.sideQuestionStreaming = false;
          return;
        }
        if (eventJson === undefined || eventJson === null || eventJson.length === 0) {
          return;
        }
        try {
          const ev = JSON.parse(eventJson) as SideQuestionEnvelopeDto;
          if (ev.Delta !== undefined) {
            this.sideQuestionAnswer += ev.Delta.text_delta ?? '';
          } else if (ev.Done !== undefined) {
            const full = ev.Done.full_text ?? '';
            if (full.length > 0) {
              this.sideQuestionAnswer = full;
            }
            const errMsg = ev.Done.error ?? '';
            if (errMsg.length > 0) {
              this.sideQuestionError = errMsg;
            }
            this.sideQuestionStreaming = false;
          } else {
            if (ev.text_delta !== undefined && ev.text_delta.length > 0) {
              this.sideQuestionAnswer += ev.text_delta;
            }
            if (ev.full_text !== undefined && ev.full_text.length > 0) {
              this.sideQuestionAnswer = ev.full_text;
              this.sideQuestionStreaming = false;
            }
            if (ev.error !== undefined && ev.error !== null && ev.error.length > 0) {
              this.sideQuestionError = ev.error;
              this.sideQuestionStreaming = false;
            }
            if (ev.kind === 'done') {
              this.sideQuestionStreaming = false;
            }
          }
        } catch (parseErr) {
          orbitLogWarn('OrbitStore', 'askSideQuestion event parse failed', parseErr as Object);
        }
      });
    } catch (err) {
      orbitLogWarn('OrbitStore', 'askSideQuestion failed', err as Object);
      if (gen === this.sideQuestionGen) {
        this.sideQuestionError = (err as Error).message;
        this.sideQuestionStreaming = false;
      }
      return '';
    }
  }

  public detectDiagramKind(language: string, source: string): string | null {
    if (this.core === null) {
      return null;
    }
    try {
      return this.core.detectDiagramKind(language, source);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'detectDiagramKind failed', err as Object);
      return null;
    }
  }

  public async renderDiagram(
    kind: string,
    source: string,
    isDark: boolean
  ): Promise<RenderDiagramViewDto | null> {
    if (this.core === null || kind.length === 0) {
      return null;
    }
    const themeStr = isDark ? 'dark' : 'light';
    const cacheKey = `${kind}::${themeStr}::${source}`;
    const hit = this.diagramRenderCache.get(cacheKey);
    if (hit !== undefined) {
      return hit;
    }
    const req: RenderDiagramRequestDto = {
      kind: kind,
      source: source,
      theme: themeStr
    };
    try {
      const json = await this.core.renderDiagramJson(JSON.stringify(req));
      const view = JSON.parse(json) as RenderDiagramViewDto;
      if (this.diagramRenderOrder.length >= 96) {
        const oldest = this.diagramRenderOrder.shift();
        if (oldest !== undefined) {
          this.diagramRenderCache.delete(oldest);
        }
      }
      this.diagramRenderCache.set(cacheKey, view);
      this.diagramRenderOrder.push(cacheKey);
      return view;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'renderDiagramJson failed', err as Object);
      return null;
    }
  }

  // --- Wave 7 Parity Actions ---

  // 1. Session Controls: Fast Mode, Fork / Branch, Export Markdown

  public toggleFastMode(): void {
    const next = !this.composerFastMode;
    this.composerFastMode = next;
    if (
      this.core !== null &&
      this.selectedHostId.length > 0 &&
      this.selectedAgentId.length > 0 &&
      !this.isNewSessionDraft
    ) {
      try {
        this.core.setAgentFastMode(this.selectedHostId, this.selectedAgentId, next);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'setAgentFastMode failed', err as Object);
      }
    }
    this.showToast(next ? 'Fast Mode enabled' : 'Fast Mode disabled', false);
    this.triggerHaptic(8);
  }

  public async forkCurrentSession(useWorktree: boolean): Promise<void> {
    if (
      this.core === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0 ||
      this.isNewSessionDraft
    ) {
      this.showToast('Open a conversation to fork', true);
      return;
    }
    const hostId = this.selectedHostId;
    const req: ForkAgentRequestDto = {
      host_id: hostId,
      source_agent_id: this.selectedAgentId,
      up_to_seq: null,
      target: useWorktree
        ? { kind: 'new_worktree', branch: null, base_ref: null }
        : { kind: 'same_directory' },
      title: null,
      initial_prompt: null
    };
    try {
      const json = await this.core.forkAgentJson(JSON.stringify(req));
      const res = JSON.parse(json) as ForkAgentResultDto;
      if (this.selectedHostId !== hostId) {
        return;
      }
      this.reloadHost(hostId);
      if (res.agent !== undefined && res.agent.agent_id.length > 0) {
        this.selectAgent(res.agent.agent_id);
      }
      this.showToast(
        useWorktree ? 'Forked into new worktree' : 'Forked conversation',
        false
      );
      this.triggerHaptic(14);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'forkAgentJson failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Fork failed', true);
      }
    }
  }

  public exportCurrentSessionMarkdown(): void {
    if (
      this.core === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0 ||
      this.isNewSessionDraft
    ) {
      this.showToast('Open a conversation to export', true);
      return;
    }
    const opts: ExportOptionsDto = {
      format: 'markdown',
      include_thinking: false,
      include_tool_calls: true,
      include_timestamps: true
    };
    try {
      const json = this.core.exportAgentTranscriptJson(
        this.selectedHostId,
        this.selectedAgentId,
        JSON.stringify(opts)
      );
      const exported = JSON.parse(json) as ExportedTranscriptDto;
      if (exported.content.length === 0) {
        this.showToast('Nothing to export yet', false);
        return;
      }
      this.copyToClipboard(
        exported.content,
        `Copied Markdown (${exported.turn_count} turns)`
      );
    } catch (err) {
      orbitLogWarn('OrbitStore', 'exportAgentTranscriptJson failed', err as Object);
      this.showToast((err as Error).message || 'Export failed', true);
    }
  }

  // 2. Side Question (`/btw`) Sheet

  public openSideQuestionSheet(): void {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.showSideQuestionSheet = true;
    this.triggerHaptic(8);
  }

  public clearSideQuestion(): void {
    this.sideQuestionGen += 1;
    this.sideQuestionActive = false;
    this.sideQuestionStreaming = false;
    this.sideQuestionPrompt = '';
    this.sideQuestionAnswer = '';
    this.sideQuestionError = '';
  }

  // 3. Archived Sessions

  public openArchivedSessions(): void {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    if (this.core !== null && this.selectedHostId.length > 0) {
      this.reloadHost(this.selectedHostId);
    }
    this.showArchivedSessionsSheet = true;
    this.triggerHaptic(8);
  }

  public archivedAgents(): AgentCardDto[] {
    const detail = this.currentHostDetail;
    if (detail === null) {
      return [];
    }
    const result: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const ag = detail.agents[i];
      const hasArchivedTs = ag.archived_at_rfc3339 !== undefined &&
        ag.archived_at_rfc3339 !== null &&
        ag.archived_at_rfc3339.length > 0;
      if (hasArchivedTs || ag.status.state === 'closed') {
        result.push(ag);
      }
    }
    return result;
  }

  public unarchiveSession(agentId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.core.unarchiveAgent(this.selectedHostId, agentId);
      this.reloadHost(this.selectedHostId);
      this.showToast('Session restored', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'unarchiveSession failed', err as Object);
      this.showToast('Failed to restore session', true);
    }
  }

  public deleteSessionPermanently(agentId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.core.deleteAgent(this.selectedHostId, agentId);
      if (this.selectedAgentId === agentId) {
        this.startNewSessionDraft();
      }
      this.reloadHost(this.selectedHostId);
      this.showToast('Session deleted', false);
      this.triggerHaptic(12);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'deleteSessionPermanently failed', err as Object);
      this.showToast('Failed to delete session', true);
    }
  }

  // 4. Mission Workspace Shared Board (`BOARD.md`) & Peers

  public activeWorkspaceId(): string {
    if (this.currentAgent !== null && this.currentAgent.workspace_id.length > 0) {
      return this.currentAgent.workspace_id;
    }
    const detail = this.currentHostDetail;
    if (detail !== null) {
      for (let i = 0; i < detail.workspaces.length; i++) {
        if (cwdPathsEquivalent(detail.workspaces[i].cwd, this.activeCwd)) {
          return detail.workspaces[i].workspace_id;
        }
      }
      if (detail.workspaces.length > 0) {
        return detail.workspaces[0].workspace_id;
      }
    }
    return '';
  }

  public activeWorkspaceInfo(): WorkspaceDto | null {
    const wsId = this.activeWorkspaceId();
    const detail = this.currentHostDetail;
    if (detail === null || wsId.length === 0) {
      return null;
    }
    for (let i = 0; i < detail.workspaces.length; i++) {
      if (detail.workspaces[i].workspace_id === wsId) {
        return detail.workspaces[i];
      }
    }
    return null;
  }

  public activeWorkspacePeers(): AgentCardDto[] {
    const wsId = this.activeWorkspaceId();
    const detail = this.currentHostDetail;
    if (detail === null || wsId.length === 0) {
      return [];
    }
    const peers: AgentCardDto[] = [];
    for (let i = 0; i < detail.agents.length; i++) {
      const ag = detail.agents[i];
      const archived = ag.archived_at_rfc3339 !== undefined &&
        ag.archived_at_rfc3339 !== null &&
        ag.archived_at_rfc3339.length > 0;
      if (ag.workspace_id === wsId && !archived && ag.status.state !== 'closed') {
        peers.push(ag);
      }
    }
    return peers;
  }

  private resetWorkspaceBoardState(): void {
    this.workspaceBoardContent = '';
    this.workspaceBoardRevision = 0;
    this.workspaceBoardUpdatedBy = '';
    this.workspaceBoardPath = '';
    this.workspaceEvents = [];
  }

  private syncActiveWorkspaceBoardFromDetail(detail: HostDetailDto): void {
    const wsId = this.activeWorkspaceId();
    if (wsId.length === 0) {
      this.resetWorkspaceBoardState();
      return;
    }
    if (detail.workspace_boards !== undefined && detail.workspace_boards[wsId] !== undefined) {
      const snap = detail.workspace_boards[wsId];
      this.workspaceBoardContent = snap.content;
      this.workspaceBoardRevision = snap.revision;
      this.workspaceBoardUpdatedBy = snap.updated_by ?? '';
      this.workspaceBoardPath = snap.board_path ?? '';
    } else {
      this.workspaceBoardContent = '';
      this.workspaceBoardRevision = 0;
      this.workspaceBoardUpdatedBy = '';
      this.workspaceBoardPath = '';
    }
    if (detail.workspace_events !== undefined && detail.workspace_events[wsId] !== undefined) {
      this.workspaceEvents = detail.workspace_events[wsId];
    } else {
      this.workspaceEvents = [];
    }
  }

  public openWorkspaceBoard(): void {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.showWorkspaceBoardSheet = true;
    this.triggerHaptic(8);
  }

  public refreshWorkspaceBoard(): void {
    const wsId = this.activeWorkspaceId();
    if (this.core === null || this.selectedHostId.length === 0 || wsId.length === 0) {
      this.resetWorkspaceBoardState();
      return;
    }
    try {
      this.core.fetchWorkspaceBoard(this.selectedHostId, wsId);
      this.core.fetchWorkspaceEvents(this.selectedHostId, wsId, 30);
      const snapJson = this.core.workspaceBoardJson(this.selectedHostId, wsId);
      const snap = JSON.parse(snapJson) as WorkspaceBoardSnapshotDto | null;
      if (snap !== null) {
        this.workspaceBoardContent = snap.content;
        this.workspaceBoardRevision = snap.revision;
        this.workspaceBoardUpdatedBy = snap.updated_by ?? '';
        this.workspaceBoardPath = snap.board_path ?? '';
      } else {
        this.workspaceBoardContent = '';
        this.workspaceBoardRevision = 0;
        this.workspaceBoardUpdatedBy = '';
        this.workspaceBoardPath = '';
      }
      const eventsJson = this.core.workspaceEventsJson(this.selectedHostId, wsId);
      this.workspaceEvents = JSON.parse(eventsJson) as WorkspaceEventRecordDto[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshWorkspaceBoard failed', err as Object);
    }
  }

  public saveWorkspaceBoard(content: string): void {
    const wsId = this.activeWorkspaceId();
    if (this.core === null || this.selectedHostId.length === 0 || wsId.length === 0) {
      this.showToast('No active workspace selected', true);
      return;
    }
    try {
      this.core.updateWorkspaceBoard(
        this.selectedHostId,
        wsId,
        content,
        null,
        'harmony-user'
      );
      this.workspaceBoardContent = content;
      this.workspaceBoardRevision += 1;
      this.showToast('Saved BOARD.md', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'saveWorkspaceBoard failed', err as Object);
      this.showToast('Failed to save BOARD.md', true);
    }
  }

  public spawnWorkspacePeer(handle: string, prompt: string, provider: string): boolean {
    const wsId = this.activeWorkspaceId();
    const cleanPrompt = prompt.trim();
    if (
      this.core === null ||
      this.selectedHostId.length === 0 ||
      wsId.length === 0 ||
      cleanPrompt.length === 0
    ) {
      this.showToast(wsId.length === 0 ? 'No active workspace selected' : 'Enter a prompt to spawn a peer', true);
      return false;
    }
    const cleanHandle = handle.trim();
    const req: SpawnWorkspacePeerRequestDto = {
      host_id: this.selectedHostId,
      workspace_id: wsId,
      cwd: this.activeCwd,
      provider: provider.length > 0 ? provider : this.composerProvider,
      model: null,
      effort: null,
      mode: normalizeModeSlug(this.composerMode),
      initial_prompt: cleanPrompt,
      handle: cleanHandle.length > 0 ? cleanHandle : null
    };
    try {
      const optId = this.core.spawnWorkspacePeerJson(JSON.stringify(req));
      this.reloadHost(this.selectedHostId);
      if (optId.length > 0) {
        this.showToast(
          cleanHandle.length > 0 ? `Spawned @${cleanHandle}` : 'Spawned workspace peer',
          false
        );
      }
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'spawnWorkspacePeer failed', err as Object);
      this.showToast((err as Error).message || 'Failed to spawn peer', true);
      return false;
    }
  }

  // 5. Automations: Schedules, Conditional Timers, Goals, Monitors & Workflows

  public openAutomationsSheet(): void {
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.showAutomationsSheet = true;
    this.triggerHaptic(8);
  }

  private async settleAutomationJson(promise: Promise<string>, label: string): Promise<string | null> {
    try {
      return await promise;
    } catch (err) {
      orbitLogWarn('OrbitStore', `${label} failed`, err as Object);
      return null;
    }
  }

  public async refreshAutomations(): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const seq = ++this.automationsRefreshSeq;
    this.automationsLoading = true;
    try {
      this.core.refreshHost(hostId);
      this.reloadHost(hostId);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshAutomations host refresh failed', err as Object);
    }

    const coreRef = this.core;
    const settled: (string | null)[] = await Promise.all<string | null>([
      this.settleAutomationJson(
        coreRef.listConditionalTimersJson(hostId, null, true),
        'listConditionalTimersJson'
      ),
      this.settleAutomationJson(
        coreRef.listGoalRunsJson(hostId, null),
        'listGoalRunsJson'
      ),
      this.settleAutomationJson(
        coreRef.listMonitorWatchesJson(hostId, null),
        'listMonitorWatchesJson'
      ),
      this.settleAutomationJson(
        coreRef.listWorkflowRunsJson(hostId, true),
        'listWorkflowRunsJson'
      )
    ]);

    if (this.selectedHostId !== hostId || seq !== this.automationsRefreshSeq) {
      return;
    }

    if (settled[0] !== null) {
      try {
        this.conditionalTimers = JSON.parse(settled[0]) as ConditionalTimerRecordDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'parse conditionalTimers failed', err as Object);
      }
    }
    if (settled[1] !== null) {
      try {
        this.goalRuns = JSON.parse(settled[1]) as GoalRunRecordDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'parse goalRuns failed', err as Object);
      }
    }
    if (settled[2] !== null) {
      try {
        this.monitorWatches = JSON.parse(settled[2]) as MonitorWatchRecordDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'parse monitorWatches failed', err as Object);
      }
    }
    if (settled[3] !== null) {
      try {
        this.workflowRuns = JSON.parse(settled[3]) as WorkflowRunRecordDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'parse workflowRuns failed', err as Object);
      }
    }

    this.automationsLoading = false;
  }

  public createSchedule(
    name: string,
    cronExpr: string,
    prompt: string,
    provider: string,
    useWorktree: boolean
  ): boolean {
    if (this.core === null || this.selectedHostId.length === 0) {
      this.showToast('Select a host first', true);
      return false;
    }
    const cleanName = name.trim();
    const cleanCron = cronExpr.trim();
    const cleanPrompt = prompt.trim();
    if (cleanName.length === 0 || cleanCron.length === 0 || cleanPrompt.length === 0) {
      this.showToast('Name, cron expression, and prompt are required', true);
      return false;
    }
    const payload: CreateSchedulePayloadDto = {
      host_id: this.selectedHostId,
      name: cleanName,
      cron_expr: cleanCron,
      prompt: cleanPrompt,
      cwd: this.activeCwd,
      provider: provider.length > 0 ? provider : this.composerProvider,
      model: this.composerModel.trim().length > 0 ? this.composerModel.trim() : null,
      effort: this.composerEffort.trim().length > 0 ? this.composerEffort.trim() : null,
      mode: normalizeModeSlug(this.composerMode),
      worktree: useWorktree,
      archive_on_finish: true,
      max_runs: null
    };
    try {
      this.core.createScheduleJson(JSON.stringify(payload));
      this.showToast(`Created schedule "${cleanName}"`, false);
      this.core.refreshHost(this.selectedHostId);
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createScheduleJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to create schedule', true);
      return false;
    }
  }

  public pauseSchedule(scheduleId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.core.pauseSchedule(this.selectedHostId, scheduleId);
      this.showToast('Schedule paused', false);
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pauseSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to pause schedule', true);
    }
  }

  public resumeSchedule(scheduleId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.core.resumeSchedule(this.selectedHostId, scheduleId);
      this.showToast('Schedule resumed', false);
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resumeSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to resume schedule', true);
    }
  }

  public runScheduleOnce(scheduleId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.core.runScheduleOnce(this.selectedHostId, scheduleId);
      this.showToast('Triggered schedule run', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'runScheduleOnce failed', err as Object);
      this.showToast((err as Error).message || 'Failed to trigger schedule', true);
    }
  }

  public deleteSchedule(scheduleId: string): void {
    if (this.core === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.core.deleteSchedule(this.selectedHostId, scheduleId);
      this.reloadHost(this.selectedHostId);
      this.showToast('Schedule deleted', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'deleteSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to delete schedule', true);
    }
  }

  public async cancelConditionalTimer(timerId: string): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0 || timerId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    try {
      const json = await this.core.cancelConditionalTimerJson(hostId, timerId);
      if (this.selectedHostId !== hostId) {
        return;
      }
      const updated = JSON.parse(json) as ConditionalTimerRecordDto;
      const next: ConditionalTimerRecordDto[] = [];
      for (let i = 0; i < this.conditionalTimers.length; i++) {
        if (this.conditionalTimers[i].timer_id === updated.timer_id) {
          next.push(updated);
        } else {
          next.push(this.conditionalTimers[i]);
        }
      }
      this.conditionalTimers = next;
      this.showToast('Timer canceled', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cancelConditionalTimer failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Failed to cancel timer', true);
      }
    }
  }

  public async fireConditionalTimerNow(timerId: string): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0 || timerId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    try {
      await this.core.fireConditionalTimerNowJson(hostId, timerId);
      if (this.selectedHostId !== hostId) {
        return;
      }
      this.showToast('Timer fired', false);
      this.triggerHaptic(10);
      await this.refreshAutomations();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'fireConditionalTimerNow failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Failed to fire timer', true);
      }
    }
  }

  public async controlGoalRun(goalId: string, action: string): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0 || goalId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: ControlGoalRunRequestDto = {
      host_id: hostId,
      goal_id: goalId,
      action: action
    };
    try {
      const json = await this.core.controlGoalRunJson(JSON.stringify(req));
      if (this.selectedHostId !== hostId) {
        return;
      }
      const updated = JSON.parse(json) as GoalRunRecordDto;
      const next: GoalRunRecordDto[] = [];
      let replaced = false;
      for (let i = 0; i < this.goalRuns.length; i++) {
        if (this.goalRuns[i].goal_id === updated.goal_id) {
          next.push(updated);
          replaced = true;
        } else {
          next.push(this.goalRuns[i]);
        }
      }
      if (!replaced) {
        next.push(updated);
      }
      this.goalRuns = next;
      const label = action === 'pause'
        ? 'Goal paused'
        : (action === 'resume' ? 'Goal resumed' : (action === 'cancel' ? 'Goal stopped' : 'Goal verifying'));
      this.showToast(label, false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'controlGoalRun failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Failed to update goal', true);
      }
    }
  }

  public async stopMonitorWatch(monitorId: string): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0 || monitorId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: StopMonitorWatchRequestDto = {
      host_id: hostId,
      monitor_id: monitorId
    };
    try {
      const json = await this.core.stopMonitorWatchJson(JSON.stringify(req));
      if (this.selectedHostId !== hostId) {
        return;
      }
      const updated = JSON.parse(json) as MonitorWatchRecordDto;
      const next: MonitorWatchRecordDto[] = [];
      for (let i = 0; i < this.monitorWatches.length; i++) {
        if (this.monitorWatches[i].monitor_id === updated.monitor_id) {
          next.push(updated);
        } else {
          next.push(this.monitorWatches[i]);
        }
      }
      this.monitorWatches = next;
      this.showToast('Monitor stopped', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'stopMonitorWatch failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Failed to stop monitor', true);
      }
    }
  }

  public async cancelWorkflowRun(runId: string): Promise<void> {
    if (this.core === null || this.selectedHostId.length === 0 || runId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: CancelWorkflowRunRequestDto = {
      host_id: hostId,
      run_id: runId
    };
    try {
      const json = await this.core.cancelWorkflowRunJson(JSON.stringify(req));
      if (this.selectedHostId !== hostId) {
        return;
      }
      const updated = JSON.parse(json) as WorkflowRunRecordDto;
      const next: WorkflowRunRecordDto[] = [];
      for (let i = 0; i < this.workflowRuns.length; i++) {
        if (this.workflowRuns[i].run_id === updated.run_id) {
          next.push(updated);
        } else {
          next.push(this.workflowRuns[i]);
        }
      }
      this.workflowRuns = next;
      this.showToast('Workflow canceled', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cancelWorkflowRun failed', err as Object);
      if (this.selectedHostId === hostId) {
        this.showToast((err as Error).message || 'Failed to cancel workflow', true);
      }
    }
  }

  // 6. Cloud Sync (GitHub Gist E2EE)

  public refreshCloudSyncStatus(): void {
    if (this.core === null) {
      return;
    }
    try {
      this.cloudSyncStatus = JSON.parse(this.core.cloudSyncStatusJson()) as CloudSyncStatusDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncStatusJson failed', err as Object);
    }
  }

  public generateCloudSyncPassphrase(): string {
    if (this.core === null) {
      return '';
    }
    try {
      return this.core.cloudSyncGeneratePassphrase();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncGeneratePassphrase failed', err as Object);
      return '';
    }
  }

  public async cloudSyncLoginWithToken(token: string): Promise<boolean> {
    const clean = token.trim();
    if (this.core === null || clean.length === 0) {
      return false;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.core.cloudSyncLoginWithTokenJson(clean);
      this.cloudSyncStatus = JSON.parse(json) as CloudSyncStatusDto;
      this.showToast('Connected GitHub account', false);
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncLoginWithTokenJson failed', err as Object);
      this.showToast((err as Error).message || 'GitHub sign-in failed', true);
      return false;
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public async cloudSyncUnlockWithPassphrase(passphrase: string): Promise<boolean> {
    const clean = passphrase.trim();
    if (this.core === null || clean.length === 0) {
      return false;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.core.cloudSyncUnlockWithPassphraseJson(clean);
      const res = JSON.parse(json) as CloudSyncResultDto;
      this.refreshCloudSyncStatus();
      this.reloadFleet();
      this.showToast(
        `Vault unlocked (rev ${res.vault_revision})`,
        false
      );
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncUnlockWithPassphraseJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to unlock vault', true);
      return false;
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public async cloudSyncNow(): Promise<void> {
    if (this.core === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.core.cloudSyncNowJson();
      const res = JSON.parse(json) as CloudSyncResultDto;
      this.refreshCloudSyncStatus();
      this.reloadFleet();
      this.showToast(`Synced vault rev ${res.vault_revision}`, false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncNowJson failed', err as Object);
      this.showToast((err as Error).message || 'Cloud sync failed', true);
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public cloudSyncSetAutoSync(enabled: boolean): void {
    if (this.core === null) {
      return;
    }
    try {
      this.cloudSyncStatus = JSON.parse(
        this.core.cloudSyncSetAutoSyncJson(enabled)
      ) as CloudSyncStatusDto;
      this.triggerHaptic(6);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncSetAutoSyncJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update auto-sync', true);
    }
  }

  public cloudSyncDisconnect(clearLocalHosts: boolean): void {
    if (this.core === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      this.cloudSyncStatus = JSON.parse(
        this.core.cloudSyncLogoutJson(clearLocalHosts)
      ) as CloudSyncStatusDto;
      this.reloadFleet();
      this.showToast('Disconnected Cloud Sync', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncLogoutJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to disconnect', true);
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  // 7. HarmonyOS Push Kit & Local Notification Deep-Link Hooks

  public registerPushTokenSilently(): void {
    registerHarmonyPushToken().then((outcome) => {
      if (outcome.token.length > 0) {
        this.pushToken = outcome.token;
        this.pushStatusMessage = 'Push Kit token ready';
      } else {
        this.pushStatusMessage = outcome.errorMessage;
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'registerPushTokenSilently failed', err);
    });
  }

  public async probeNotificationPermission(): Promise<void> {
    this.notificationsEnabled = await checkHarmonyNotificationEnabled();
  }

  public async configurePushAndNotifications(context: common.Context | undefined): Promise<void> {
    const notifOk = await requestHarmonyNotificationPermission(context);
    this.notificationsEnabled = notifOk;
    const outcome = await registerHarmonyPushToken();
    if (outcome.token.length > 0) {
      this.pushToken = outcome.token;
      this.pushStatusMessage = 'Push Kit token registered';
    } else {
      this.pushStatusMessage = outcome.errorMessage;
    }
    if (notifOk && outcome.token.length > 0) {
      this.showToast('Push Kit & notifications ready', false);
    } else if (notifOk) {
      this.showToast(
        outcome.errorMessage.length > 0
          ? `Notifications enabled · ${outcome.errorMessage}`
          : 'Notifications enabled',
        false
      );
    } else if (outcome.token.length > 0) {
      this.showToast('Push Kit registered · enable notifications in Settings', false);
    } else {
      this.showToast(outcome.errorMessage || 'Notifications unavailable', true);
    }
    this.triggerHaptic(10);
  }

  public async notifyDeepLinkForActiveSession(
    context: common.Context | undefined,
    title: string,
    body: string
  ): Promise<void> {
    if (this.selectedHostId.length === 0) {
      return;
    }
    const ok = await publishOrbitDeepLinkNotification(
      context,
      Math.floor(Date.now() % 1000000),
      title,
      body,
      this.selectedHostId,
      this.selectedAgentId
    );
    if (ok) {
      this.showToast('Notification dispatched', false);
    }
  }
}
